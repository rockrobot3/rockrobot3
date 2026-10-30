//! 集成测试助手：起**真 rrd 子进程** + 裸 UDS 客户端，断言线级行为。
//!
//! M2 Task 9 起它还管**第二个真进程**：`SimPeer`（跑 `rrd` 包的 `sim_peer` 二进制）与
//! `rr3_fixture_manifest`（读 `base/runtime/tests/fixtures/<dir>/module.toml`，把 entry 占位符
//! 换成构建产物的绝对路径后落到 pid 唯一的 /tmp，返回 `SimManifest` 守卫）。逐行收集与等待
//! 抽成 `rr3_spawn_line_reader` / `rr3_wait_line` 两个自由函数，rrd 与 sim-peer 共用一份口径。
//!
//! 只在 Unix 编译（每个测试文件顶层 `#![cfg(unix)]`），Windows 侧这个模块整体不参与编译，
//! 所以 `cargo check --all-targets` 在 Windows 上是绿的——那不是证据，是盲区。
//!
//! 顺序约定（F1 之后是**真的**与顺序无关，不再只是注释）：应答由 conn 线程写、广播由这条连接的
//! writer 线程写，同一 socket 上**只保证整帧原子，不保证相对顺序**（Task 7 的并发形状，brief 明令；
//! 入队次序 ≠ 上线次序，见 `rrd/src/conn.rs` 的同一处注释）。
//! 所以本夹具按连接维护一条 `VecDeque<Rr3Frame>` 待处理队列：`rr3_read_where` 把谓词不成立的帧
//! **存回队列**而不是从 socket 上丢掉，"先按类型读应答、再按字段读 state"这类两阶段读因此与到达
//! 次序无关。读不到帧时给的是"没等到 X；队列里已积压 N 帧"的诊断，不是一句裸 io panic。
//! 没有"读第 N 帧"的助手——想这么写说明测试在赌时序。

#![allow(dead_code)] // 各测试文件按需取用，没用到不算死码

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame,
    Rr3FrameError, Rr3Header, Rr3PayloadType,
};
use serde_json::{json, Value};

/// 叠加层（Task 2 提交的那份）：夹具主题只在测试里在册
pub const RR3_OVERLAY_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/sim-topics.toml"
);

/// 既有 19+1+5 条线级用例（`register_publish` / `socket_banner` / `e2e_sim_peer`）统一用的
/// "安静 tick"（1 小时）。**为什么把既有测试钉在"不过期"上**：
/// Task 10 的 diff 不该改动 Task 7/8/9 任何一条用例的可观察行为——那里的模块注册完就不再
/// 喂狗（手搓帧用例只喂测试要的那一颗），真 tick 会让它们集体被判死，多出的 failed 广播与
/// 判死日志会污染"这条连接上不该出现 X"那类否定断言，也会让 `task=` 号段的既有断言变成
/// 重启后的新号。安静 tick 让线程照起、照打启动行，但在一条用例的寿命内永不扫到到期。
/// 只有 `rrd/tests/e2e_watchdog.rs` 显式要小 tick。
pub const RR3_QUIET_WATCHDOG_TICK_MS: u64 = 3_600_000;

/// watchdog 线级用例的 tick。放在 support 而不是测试文件里：它与夹具 manifest 的
/// `watchdog_ms = 500` 一起决定"到期上界 520ms"这条算式，两处各写一个数就是两处真值。
pub const E2E_TICK_MS: u64 = 20;

/// 监听面夹具的**缺格跳过装配单**（基-23）：schema 与 `e2e_profile.rs` 的 GOOD 同形
///（schema_version=5 起五段必填全给；[[supplies.domain]] 留 rail5v 一个域，与接线底稿的
/// `net = "rail5v"` 配套——校验⑦要求 net 在装配单供给域在册），唯 [bindings] /
/// [slots].present 引用集清零、[enabled_slots] 只留 rr3-listenstub 一个模块：它声明的
/// act.servo 既不在 present（声明缺）、又无 binding（注入缺）→ 缺格点名后 boot 队列
/// **跳过**（P2 的同一条"缺格≠拒 boot"语义）——装载门真跑（查册/缺格/预算/wiring 四
/// 校验全过）而零 spawn、零进程、task 号段仍从线上注册起算。三个不能更简的形状约束：
/// **空 enabled_slots 在 parse 层即拒**（"空装配单没有可起模块"，fail-closed，实测撞过）；
/// **stub 的 manifest 必须真实落盘**（P2 教训：少了就退化成另一条读取失败路径）；
/// **缺格点名行含"拒绝启动"**（监听面用例的否定 needle 不得用裸"拒绝"——见
/// register_publish.rs 那处收窄）。空装配还免探针：缺格判定先于探测，零格位零探测，
/// 不需要 P1 那样的 `RR3_PROBE=sim` 前提。
const RR3_LISTEN_PROFILE: &str = r#"
schema_version = 5

[os]
route = "distro"

[runtime]
log_ring_cap = 512

[platform]
tier = "host"
chip = "rk3566"

[supplies]
watchdog_present = false
watchdog_timeout_ms = 15000
watchdog_feeder = "rrd"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
mild_mv = 4600
severe_mv = 4400
current_cap_ma = 2000
sense = "none"
current_sense = "none"
switch = "none"
switchable = false
armed = false
evidence = "none"
max_age_ms = 0

[supplies.budget]
not_applicable = true

[slots]
present = []

[bindings]

[enabled_slots]
rr3-listenstub = ["act.servo"]
"#;

/// 监听面夹具的接线档底稿（与 `e2e_profile.rs` 的 WIRING 同一底稿，两份独立存在——
/// support 不引测试文件的常量）：装载期校验⑦要求装机档必在——vcc/gnd-only 单线组
///（net=rail5v 在装配单域在册），对 [[pin]] 增删免疫。
const RR3_LISTEN_WIRING: &str = r#"
[[wire]]
connector = "J-T"
hdr_pin = [1, 2]
signal = ["vcc", "gnd"]
net = "rail5v"
feeds = []
mate = "none"
keyed = false
rated_a = 1.0
gnd_common = "star-1"
"#;

/// 造一份监听面夹具（`e2e_profile.rs` 的 Root 同一装机布局：`<root>/profiles/default.toml`
/// + `<root>/hardware/default/wiring.toml` + `<root>/modules/rr3-listenstub/module.toml`
///——rr3_wiring_path_for_profile 推导的同一棵树；名字取套接字 stem，pid 级唯一）。
/// **stub manifest 必须真实落盘**（P2 教训）：缺格模块的 manifest 也进装载第 7 步的
/// 路径解析，少了就退化成另一条"读取失败"的失败路径，钉不住缺格跳过语义。
/// 返回 (装配单路径, 夹具根)——根交给 `RrdTest.fixture_root`，Drop 清整树：断言 panic
/// 后 /tmp 不许留残迹（Task 7 评审 N-1 的同一条教训）。
fn rr3_listen_profile(socket: &Path) -> (PathBuf, PathBuf) {
    let stem = socket
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("listen");
    let root = std::env::temp_dir().join(format!("rr3-listen-fix-{}-{stem}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root); // pid 复用留下的上次残根不许干扰本次
    let p = root.join("profiles").join("default.toml");
    std::fs::create_dir_all(p.parent().unwrap()).expect("建夹具 profiles 目录");
    std::fs::write(&p, RR3_LISTEN_PROFILE).expect("写监听面装配单");
    let w = root.join("hardware").join("default").join("wiring.toml");
    std::fs::create_dir_all(w.parent().unwrap()).expect("建夹具 hardware 目录");
    std::fs::write(&w, RR3_LISTEN_WIRING).expect("写监听面接线档");
    let m = root
        .join("modules")
        .join("rr3-listenstub")
        .join("module.toml");
    std::fs::create_dir_all(m.parent().unwrap()).expect("建夹具 modules 目录");
    std::fs::write(&m, rr3_manifest("rr3-listenstub", 100, 500, "never"))
        .expect("写监听面 stub manifest");
    (p, root)
}

/// "等一条**应当出现的行**"的统一耐心上限——既管启动就绪行（监听 / 线程横幅），也管
/// 运行期日志（注册 / 判死 / 重启）。为什么统一：两者的到来时刻都由**宿主机负载**支配
/// （boot 场景是 spawn → 子进程 exec → 连接 → 注册的整条链，exec 在过载机上就要秒级），
/// 都不是被测行为的时间语义。本仓库开发机（2015 双核 i7，4 线程）在 rust-analyzer/编译
/// 并发的 load>10 下实测：子进程 exec 后 **8.3s** 才打出横幅（sample 抓到 16K 物理内存 +
/// 未知二进制段的 exec 早期现场），空载只要 ~100ms；旧值 5s/3s 因此让线级用例间歇 spawn
/// 超时。20s = 实测最坏值 ×2 以上余量。**行为上的否定断言不走这里**——那是
/// `rr3_assert_no_log` 的显式时长，判断据是"窗口内没出现"。
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// 从子进程 stdout 逐行收集（`RrdTest` 与 `SimPeer` 共用一份口径）。
/// 为什么必须共用：两处各写一份逐行读码，EOF 与超时的处理迟早分叉——而这两件事正是
/// "跨进程用例为什么红"最常见的真实原因（管道坏了 vs 被测行为不对，得分得开）。
/// `tag` 只出现在"读流本身报错"这一条诊断行里——读不到日志时"是谁的管道坏了"必须一眼可辨。
pub fn rr3_spawn_line_reader(stdout: ChildStdout, tag: &'static str) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut log = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match log.read_line(&mut line) {
                Ok(0) => break, // EOF：子进程已退出
                Ok(_) => {
                    let t = line.trim_end().to_string();
                    if !t.is_empty() && tx.send(t).is_err() {
                        break; // 接收端已 drop
                    }
                }
                Err(e) => {
                    // 不静默 break：把坏流本身当一条"日志"送出去，等待方在超时诊断里能看到它
                    let _ = tx.send(format!("{tag} 读输出失败: {e}"));
                    break;
                }
            }
        }
    });
    rx
}

/// 等到一条含 needle 的行（先查已缓冲的）；超时把全部已读行打进 panic，便于定位
pub fn rr3_wait_line(
    lines: &mpsc::Receiver<String>,
    seen: &mut Vec<String>,
    needle: &str,
    dur: Duration,
) -> String {
    if let Some(i) = seen.iter().position(|l| l.contains(needle)) {
        return seen.remove(i);
    }
    let deadline = Instant::now() + dur;
    loop {
        match lines.recv_timeout(Duration::from_millis(50)) {
            Ok(l) if l.contains(needle) => return l,
            Ok(l) => seen.push(l),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected)
                if Instant::now() > deadline =>
            {
                panic!("{dur:?} 内没等到 {needle:?}；已读到:\n{}", seen.join("\n"))
            }
            Err(_) => {}
        }
    }
}

/// 从一行日志里抽 `key=<数字>` 的那个数（`pid=` / `task=` / `attempt=`）。
///
/// **一律抽取、不写字面量**（A-5① 的处置）：boot 之后 task 号跟着 spawn 序走，pid 更是
/// 每次都不一样，写死就是猜。它是 `e2e_sim_peer.rs` 里那段 inline `split(..).nth(1)` 的
/// 提取版——同一段代码出现第三次就该是个函数（那一处本轮不改：既有 28 条线级用例的断言
/// 一字不许动，等 Task 13 的夹具整合再切过来）。
/// 抽不到即 panic 并回显整行：把"needle 与日志格式对不上"这种夹具 bug 当场报出来，
/// 而不是让它退化成 `unwrap_or(0)` 之后比出一个假相等。
pub fn rr3_field_from_log(line: &str, key: &str) -> u64 {
    let raw = line
        .split(key)
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_else(|| panic!("日志行 {line:?} 里没有 {key:?}"));
    raw.parse::<u64>()
        .unwrap_or_else(|e| panic!("日志行 {line:?} 里的 {key:?} 不是数字（{raw}）: {e}"))
}

/// 有界轮询一个条件：每 20ms 求值一次，最多 `dur`。返回"是否在期限内成立"。
///
/// 与 `rr3_wait_line` 同一口径：**判据是内容**（条件成立与否），`dur` 只是"不再等"的上限，
/// 所以它不是 `sleep` 的别名——`sleep(2s)` 断言的是"过了两秒"，这条断言的是"两秒内成真"。
/// 不 panic 而是返回 bool：让调用方把"没成真时该看到什么"写在自己的断言消息里
/// （例如 `/proc/{pid}` 那一格，pid 才是现场）。
pub fn rr3_wait_until<F: FnMut() -> bool>(mut pred: F, dur: Duration) -> bool {
    let started = Instant::now();
    loop {
        if pred() {
            return true;
        }
        if started.elapsed() >= dur {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub struct RrdTest {
    pub socket: PathBuf,
    child: Child,
    lines: mpsc::Receiver<String>,
    seen: Vec<String>,
    /// 监听面夹具根（基-23）：经 `rr3_listen_profile` 造的 /tmp 树。非 None 时 Drop
    /// 连树一起清——断言 panic 后不留残迹（Task 7 评审 N-1 教训，`e2e_profile.rs`
    /// 的 Root 同一条 Drop 纪律）。
    fixture_root: Option<PathBuf>,
}

impl RrdTest {
    /// 起 rrd：套接字用 pid+tag 唯一名（Global Constraints 8），退避基数压到 20ms
    /// （Task 11 要跑完 1..60 档），注册前读超时 300ms（本任务测试 8 用它）。
    pub fn rr3_start(tag: &str) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        // 就绪判据用日志而非 connect：connect 探测会留下一条"首帧读取失败"噪声日志
        RrdTest::rr3_spawn_with_overlay(
            &socket,
            None,
            &socket,
            Some(Path::new(RR3_OVERLAY_PATH)),
            RR3_QUIET_WATCHDOG_TICK_MS,
        )
    }

    /// **不带** `--topics-overlay` 的起法：在册的只有 `contracts/topics.toml` 那 21 条生产主题。
    /// 专治"叠加层被当成生产放行口"（K4）：夹具命名空间 `rr3-sima/*` 在这里根本不存在，
    /// 而生产命名空间的 owner 又不是夹具模块 → 两种拒法都必须在**生产口径**下成立。
    /// 套接字名与 `rr3_start` 同一套 pid+tag 唯一名规则（Global Constraints 8）。
    /// 基-23 起与 `rr3_start` 共用同一份缺格跳过装配单（装载面正交于叠加层面）。
    pub fn rr3_start_no_overlay(tag: &str) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_with_overlay(&socket, None, &socket, None, RR3_QUIET_WATCHDOG_TICK_MS)
    }

    /// `rr3_start_no_overlay` 的带环境变量版（0.0.4 批B）：计数持久化用例经它给
    /// rrd 进程注入 `RR3_DATA_DIR`。不 `std::env::set_var`（与 `rr3_start_profile_env`
    /// 同一纪律：测试进程的 env 是共享的，改自己会串别的用例）。
    pub fn rr3_start_env(tag: &str, envs: &[(&str, &str)]) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        // 基-23：与 rr3_spawn_with_overlay 同一条缺格跳过装配门口径（它不走 overlay 形参、
        // 直接铺 rr3_spawn_inner，所以单独补装配面——两个入口都不许裸起）。
        let (profile, fixture_root) = rr3_listen_profile(&socket);
        let mut me = RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            None,
            RR3_QUIET_WATCHDOG_TICK_MS,
            &[],
            None,
            Some(&profile),
            envs,
        );
        me.fixture_root = Some(fixture_root);
        me
    }

    /// **显式 `--manifest` 调试后门**的起法（基-23）：manifest 指向 pid 唯一的**不存在
    /// 路径**——`rr3_boot_job_from_manifest` 打一行"拒绝启动：manifest … 读取失败"后返回
    /// None（E-3：boot 期配置错误不吃预算，读不动 = None 跳过，非 fatal），boot 队列为空，
    /// rrd 照常监听。专供"必须走后门路径"的用例（e2e_ctl 的 ctl.wiring unavailable 断言：
    /// 后门无装配快照，如实回 available:false）——监听面主口径（rr3_start 一族）走的是
    /// 缺格跳过装配单正门，两形状的可观察面同为零 spawn。
    pub fn rr3_start_manifest_stub(tag: &str) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        let stub = std::env::temp_dir().join(format!(
            "rr3-no-such-manifest-{}-{tag}.toml",
            std::process::id()
        ));
        RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            None,
            RR3_QUIET_WATCHDOG_TICK_MS,
            &[stub.as_path()],
            None,
            None,
            &[],
        )
    }

    /// watchdog 线级用例的唯一入口。**没有默认值**是刻意的：到期判死必须是用例自己声明的
    /// 条件，不能是"忘了传参数"传出来的。
    pub fn rr3_start_with_tick(tag: &str, tick_ms: u64) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_with_overlay(
            &socket,
            None,
            &socket,
            Some(Path::new(RR3_OVERLAY_PATH)),
            tick_ms,
        )
    }

    /// 带 boot spawn 的起法：`manifests` 原样进 `--manifest`（顺序即 CLI 序，spawn 序由
    /// start_priority 重排），`boot_role` 进 `--boot-role`。**tick 必传**（与
    /// `rr3_start_with_tick` 同一理由：到期判死是用例自己声明的条件）。
    ///
    /// **但这一处的 tick 不只是"判死节拍"**：Task 11b 的 E-5 让 supervisor 的**扫尸节拍**
    /// 复用同一个旋钮（`recv_timeout(sweep)`），所以拿 `RR3_QUIET_WATCHDOG_TICK_MS` 起 boot
    /// 用例会让收尸一小时不发生 —— boot 用例**必须**传小 tick（`E2E_TICK_MS`）。
    /// 这条耦合是指据不是坑：它钉住了"全 daemon 只有一条执行线程 + 一个节拍旋钮"。
    pub fn rr3_start_boot(
        tag: &str,
        manifests: &[&Path],
        boot_role: &str,
        tick_ms: u64,
    ) -> RrdTest {
        RrdTest::rr3_start_boot_env(tag, manifests, boot_role, tick_ms, &[])
    }

    /// `rr3_start_boot` 的带环境变量版（0.0.6 批F2）：`envs` 原样注入 rrd 进程环境，
    /// 经 spawn 继承传给 `--manifest` 直驱 boot 的**真模块**——判向 e2e 用它给
    /// rr3-sense 注入 `RR3_HAL_SIM_DOA_DEG`（与 `rr3_start_profile_env` 同一条纪律）。
    pub fn rr3_start_boot_env(
        tag: &str,
        manifests: &[&Path],
        boot_role: &str,
        tick_ms: u64,
        envs: &[(&str, &str)],
    ) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            Some(Path::new(RR3_OVERLAY_PATH)),
            tick_ms,
            manifests,
            Some(boot_role),
            None,
            envs,
        )
    }

    /// 装配单正门的起法（0.0.2 D2/T3）：`profile` 原样进 `--profile`。**tick 必传**
    /// （与 `rr3_start_boot` 同一理由：supervisor 的扫尸节拍复用同一旋钮，boot 用例必须小 tick）。
    pub fn rr3_start_profile(tag: &str, profile: &Path, tick_ms: u64) -> RrdTest {
        RrdTest::rr3_start_profile_env(tag, profile, tick_ms, &[])
    }

    /// `rr3_start_profile` 的带环境变量版（0.0.2 T4b）：`envs` 原样注入 rrd 进程环境，
    /// 并经 `Command::spawn` 的继承语义传给 rrd spawn 的**真模块**——sim 后端设备参数
    /// （如 `RR3_HAL_SIM_BATTERY_PERCENT`）的唯一注入通道。e2e 不用 `std::env::set_var`
    ///（unsafe 且与并行测试竞态，rr-hal backend.rs 同一条禁令），一律走这里显式传。
    pub fn rr3_start_profile_env(
        tag: &str,
        profile: &Path,
        tick_ms: u64,
        envs: &[(&str, &str)],
    ) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            None,
            tick_ms,
            &[],
            None,
            Some(profile),
            envs,
        )
    }

    /// 真正的起进程处。三个路径各管各的：
    /// - `socket_arg` 原样写进 `--socket`（可以是相对路径，CF-3 就靠这一点）
    /// - `cwd` 是子进程的工作目录（`None` = 继承测试进程的），相对 `socket_arg` 按它解析
    /// - `socket` 是**期望的绝对路径**：用于清残留、连它、以及等横幅回显它
    ///
    /// 把"传给 CLI 的原词"和"期望的绝对路径"分成两个参数，正是为了让用例能断言
    /// 横幅回显的是后者而不是前者（`rrd/tests/socket_banner.rs`）。
    ///
    /// 参数表里没有 `tag`：套接字名由**调用方**决定（`rr3_start` 把 tag 拼进路径，
    /// `socket_banner.rs` 自己拼名字），起进程这一层用不上它（M-5：死参数已删）。
    pub fn rr3_spawn(socket: &Path, cwd: Option<&Path>, socket_arg: &Path) -> RrdTest {
        RrdTest::rr3_spawn_with_overlay(
            socket,
            cwd,
            socket_arg,
            Some(Path::new(RR3_OVERLAY_PATH)),
            RR3_QUIET_WATCHDOG_TICK_MS,
        )
    }

    /// 起进程那一层：`overlay` 为 `None` 时**整条 `--topics-overlay` 都不传**
    /// （`rr3_start_no_overlay` 用这条口径证明叠加层不是生产放行口）。
    ///
    /// `watchdog_tick_ms` 也走参数而不是"各调用点自己拼"：它是本任务唯一的线程参数，
    /// 而 `rr3_spawn_with_overlay` 末尾会等 `watchdog 启动 tick={它}ms` 那一行——
    /// 传进去的数与等的数必须是同一个来源，否则接线断了没人发现（那条等待同时钉住接线）。
    pub fn rr3_spawn_with_overlay(
        socket: &Path,
        cwd: Option<&Path>,
        socket_arg: &Path,
        overlay: Option<&Path>,
        watchdog_tick_ms: u64,
    ) -> RrdTest {
        // 装配面（基-23）：监听面夹具走**空装配单正门**——装载门真跑（查册/缺格/预算/
        // wiring），enabled_slots 空表 = 零 boot 模块，可观察面与旧裸起完全一致（无 spawn
        // 行、task 号段仍从线上注册起算）。裸起（既无 --profile 又无 --manifest）在基-23
        // 后 parse 即拒，旧注释里的 `boot = (无 manifest, None)` 形状不再合法。
        let (profile, fixture_root) = rr3_listen_profile(socket);
        let mut me = RrdTest::rr3_spawn_inner(
            socket,
            cwd,
            socket_arg,
            overlay,
            watchdog_tick_ms,
            &[],
            None,
            Some(&profile),
            &[],
        );
        me.fixture_root = Some(fixture_root);
        me
    }

    /// 起进程的唯一实现。`boot_role: Some(role)` 时才传 `--boot-role`，并且**多等一行**
    /// `supervisor 启动 sweep={tick}ms`：与 `watchdog 启动 tick=` 同一口径——它证明的是
    /// "那条线程真的跑起来过"，而不是"daemon 走到了 spawn 那一行"，同时钉住
    /// "sweep 复用的就是 `--watchdog-tick-ms`"这条接线（E-5）。
    /// 9 个参数全是同一条 spawn 命令的零件（每个开关一段），拆结构体只会让九个调用点
    /// 各造一层壳——allow 而不是重构。
    #[allow(clippy::too_many_arguments)]
    fn rr3_spawn_inner(
        socket: &Path,
        cwd: Option<&Path>,
        socket_arg: &Path,
        overlay: Option<&Path>,
        watchdog_tick_ms: u64,
        boot_manifests: &[&Path],
        boot_role: Option<&str>,
        boot_profile: Option<&Path>,
        extra_env: &[(&str, &str)],
    ) -> RrdTest {
        let _ = std::fs::remove_file(socket);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rrd"));
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        cmd.arg("--socket").arg(socket_arg);
        if let Some(ov) = overlay {
            cmd.arg("--topics-overlay").arg(ov);
        }
        if let Some(p) = boot_profile {
            cmd.arg("--profile").arg(p);
        }
        for m in boot_manifests {
            cmd.arg("--manifest").arg(m);
        }
        if let Some(role) = boot_role {
            cmd.arg("--boot-role").arg(role);
        }
        cmd.args(["--restart-base-ms", "20"])
            .args(["--pre-register-timeout-ms", "300"])
            .args(["--watchdog-tick-ms", &watchdog_tick_ms.to_string()])
            .stdout(Stdio::piped())
            // stderr 直接继承：出错就在 cargo 输出里，不必再开一条管道读（也避免缓冲区写满把 rrd 卡死）
            .stderr(Stdio::inherit());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let mut child = cmd.spawn().unwrap_or_else(|e| panic!("起 rrd 失败: {e}"));

        let lines = rr3_spawn_line_reader(child.stdout.take().expect("stdout 必须是管道"), "rrd");

        let mut me = RrdTest {
            socket: socket.to_path_buf(),
            child,
            lines,
            seen: Vec::new(),
            fixture_root: None, // 夹具根由调用方（监听面两入口）在返回后认领并负责 Drop 清理
        };
        me.rr3_wait_log(&format!("监听 {}", socket.display()));
        // 每个用例都付这一行的等待代价，换来的是一条全局前提：**监视线程真的起了**。
        // "起了但不扫"（tick 传错、循环一开始就死锁）由 e2e_watchdog 的判据抓，
        // 这里只抓"根本没起"。needle 带上传进去的具体数值，所以它同时钉住接线。
        me.rr3_wait_log(&format!("watchdog 启动 tick={watchdog_tick_ms}ms"));
        if boot_role.is_some() {
            me.rr3_wait_log(&format!("supervisor 启动 sweep={watchdog_tick_ms}ms"));
        }
        me
    }

    /// 裸客户端：5s 读超时，卡住即失败而不是挂住整个测试目标。
    /// 返回的是带待处理队列的 `RrdClient`（F1），不是裸 `UnixStream`。
    pub fn rr3_connect(&self) -> RrdClient {
        let s = UnixStream::connect(&self.socket).unwrap_or_else(|e| {
            panic!(
                "连接 {} 失败: {e}；rrd 已打印:\n{}",
                self.socket.display(),
                self.seen.join("\n")
            )
        });
        s.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set_read_timeout");
        RrdClient {
            stream: s,
            pending: VecDeque::new(),
        }
    }

    fn rr3_next_line(&mut self, dur: Duration) -> Option<String> {
        match self.lines.recv_timeout(dur) {
            Ok(l) => Some(l),
            // 注：Rust 的模式里不能写 `RecvTimeoutError::{Timeout, Disconnected}`（简写只在
            // 表达式位置成立），这里用完整的 `|` 分支——语义与 brief 一致：超时与断开都当"暂无"。
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// 等到一条含 needle 的日志（先查已缓冲的）；超时把全部已读日志打进 panic，便于定位。
    /// 耐心上限是 READY_TIMEOUT（见其文档：日志的到来时刻由宿主机负载支配）。
    /// 逐行收集与等待都委托给本文件的 `rr3_spawn_line_reader` / `rr3_wait_line`——
    /// `SimPeer` 读的是另一个进程，必须走同一份口径，否则两处的 EOF/超时语义会分叉。
    pub fn rr3_wait_log(&mut self, needle: &str) -> String {
        rr3_wait_line(&self.lines, &mut self.seen, needle, READY_TIMEOUT)
    }

    /// 排干 dur 内的全部输出（等异步副作用：watchdog、重启），返回拼接文本
    pub fn rr3_logs(&mut self, dur: Duration) -> String {
        let started = Instant::now();
        while started.elapsed() < dur {
            if let Some(l) = self.rr3_next_line(Duration::from_millis(20)) {
                self.seen.push(l);
            }
        }
        self.seen.join("\n")
    }

    /// 断言 dur 内**没有**某串（重启/广播这类"不该发生"的副作用靠它抓）
    pub fn rr3_assert_no_log(&mut self, needle: &str, dur: Duration) {
        let logs = self.rr3_logs(dur);
        assert!(
            !logs.contains(needle),
            "不该出现 {needle:?}；实际日志:\n{logs}"
        );
    }

    /// 对 rrd 发 SIGTERM 并等它退出（0.0.5 增补轮四 批W3，R26 关停强制 flush 的
    /// e2e 用）。`/bin/kill` 与 `rr3_pid_alive` 同一先例（std 不暴露 kill）；发送
    /// 后 `child.wait()` 收尸——之后 Drop 的 kill/wait 对已收尸进程是 Err，无害。
    pub fn rr3_sigterm_and_wait(&mut self) -> std::process::ExitStatus {
        let pid = self.child.id();
        Command::new("/bin/kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .expect("kill -TERM 起不来");
        self.child.wait().expect("rrd 进程必须能被 wait 收尸")
    }
}

impl Drop for RrdTest {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
        if let Some(root) = self.fixture_root.take() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

// ---------- manifest 文本 ----------

/// 合法的最小可用 manifest。改这里的字面量必须同步改测试里的 `.replace(...)` 目标。
pub fn rr3_manifest(name: &str, hb: u64, wd: u64, policy: &str) -> String {
    format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/bin/{name}"

[events]
publishes = []
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "{policy}"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
    )
}

// ---------- 帧 ----------

pub fn rr3_request(method: &str, id: u64, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub fn rr3_json_frame(ty: Rr3PayloadType, body: &Value, src_task: u64, dst_task: u64) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body.to_string().as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task,
            dst_task,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 上行 RPC（默认 dst=rrd）
pub fn rr3_rpc_frame(method: &str, id: u64, params: Value, src_task: u64) -> Rr3Frame {
    rr3_json_frame(
        Rr3PayloadType::Request,
        &rr3_request(method, id, params),
        src_task,
        rr_bus::RR3_TASK_RRD,
    )
}

/// 注册帧（此时模块还没有 task，按契约填 0——"用户态不得自设"的另一种写法）
pub fn rr3_register_frame(manifest: &str, name: &str) -> Rr3Frame {
    rr3_rpc_frame(
        "rr3.register",
        1,
        json!({"name": name, "manifest": manifest}),
        0,
    )
}

/// 一条连接上的裸客户端 + **按连接存在的待处理队列**（F1）。
///
/// 为什么队列必须挂在连接上而不是全局：fd 会被复用，全局 map 会把上一条连接遗留的帧投给新连接
/// （评审裁定里明令不换 fd-keyed 全局 map）。也不动生产代码——顺序无关性本就该由读侧承担。
pub struct RrdClient {
    stream: UnixStream,
    pending: VecDeque<Rr3Frame>,
}

/// 一次"挑帧"最多扫多少帧。扫不到就是**夹具 bug**（谓词写错、或 rrd 真没发这一帧），
/// 不是把预算调大就能糊过去的偶发；提为常量是因为 Task 8 加 Event 分支、Task 10 加 watchdog
/// 广播之后一条连接上几秒内的帧数会超过旧的字面值 16（M-4）。
///
/// **预算的口径与它的前提（Task 8 评审遗留 ④）**：这颗预算**同时**计"从队列弹帧"与"从 socket
/// 读帧"——`rr3_try_recv` 先弹 `pending` 再读 socket，所以队列里有存货时，后续读法会先在队列上
/// 花掉帧数。之所以不把它改成"只计 socket 读"：那样一次挑帧的**总**工作量就没有上界了
/// （队列可以被反复扫过），而本常量存在的目的正是给"扫不到即夹具 bug"这一判定一个可终止的边界。
/// 于是这里立一条硬前提：**传给 `rr3_read_where` 的谓词必须是纯函数**——同一帧重复求值结果相同、
/// 且不改外部状态。原因就在上面那句：pending 里的帧会被下一轮、下下轮重新求值，带计数器/时间戳/
/// 内部缓存的谓词会在"第二次扫到同一帧"时给出不同答案，那时预算就不再是"扫了多少帧"而是
/// "谓词心情如何"。本目标现有谓词全部是字段/类型比较（`rr3_body_split` 的枚举比对、
/// `rr3_json_of(f)["字段"] == 值`），无一例外满足此条件；新增读法请照此约束写。
const SCAN_BUDGET: usize = 64;

/// 帧的一行摘要，只用于失败诊断
fn rr3_frame_brief(f: &Rr3Frame) -> String {
    format!("{:?} {}", f.header, String::from_utf8_lossy(&f.payload))
}

/// 队列现状的一句话诊断（失败文案里"没等到 X"之后紧跟它，省得再猜哪些帧被扫过）
fn rr3_pending_dump(c: &RrdClient) -> String {
    if c.pending.is_empty() {
        return "队列里已积压 0 帧".to_string();
    }
    format!(
        "队列里已积压 {} 帧:\n{}",
        c.pending.len(),
        c.pending
            .iter()
            .map(rr3_frame_brief)
            .collect::<Vec<_>>()
            .join("\n---\n")
    )
}

/// 非 panic 版读帧：`Ok` 一帧，`Err` 把底层错误原样交回（EOF 与超时都在此分流）。
/// 队列里有存货就先交存货——这是"不匹配的帧绝不丢"的落点。
pub fn rr3_try_recv(c: &mut RrdClient) -> Result<Rr3Frame, Rr3FrameError> {
    if let Some(f) = c.pending.pop_front() {
        return Ok(f);
    }
    rr3_frame_read(&mut c.stream)
}

pub fn rr3_send(c: &mut RrdClient, frame: &Rr3Frame) {
    rr3_frame_write(&mut c.stream, frame).unwrap_or_else(|e| panic!("写帧失败: {e:?}"));
}

pub fn rr3_recv(c: &mut RrdClient) -> Rr3Frame {
    match rr3_try_recv(c) {
        Ok(f) => f,
        Err(e) => panic!("读帧失败: {e:?}"),
    }
}

/// 拆出类型 + JSON 体（本套件的体一律 JSON）
pub fn rr3_json_of(frame: &Rr3Frame) -> Value {
    let (_, body) = rr3_body_split(&frame.payload).unwrap_or_else(|e| panic!("前缀非法: {e:?}"));
    serde_json::from_slice(&body).expect("体必须是 JSON")
}

/// 按谓词挑帧（最多 `SCAN_BUDGET` 帧）。为什么必须挑：见本文件头注释的"顺序约定"。
/// 谓词不成立的帧**存回队列**（`push_front` 回原位，保持到达次序），后来的读法还能读到它们。
///
/// 实现要点：本轮扫过的帧先攒在 `deferred` 里、**扫完才回队列**。要是边扫边 `push_back`，
/// 下一轮 `rr3_try_recv` 会优先弹队列，于是同一帧被反复弹出又塞回，永远读不到 socket 上
/// 后面的帧（第一版就栽在这里：`duplicate_name…` 等 6 条用例以"64 帧内没等到"的形式变红）。
/// 又因为"塞回队列的帧之后还会被重新求值"，**谓词必须是纯函数**（见 `SCAN_BUDGET` 的注释）。
pub fn rr3_read_where<F: FnMut(&Rr3Frame) -> bool>(
    c: &mut RrdClient,
    what: &str,
    mut pred: F,
) -> Rr3Frame {
    let mut deferred: Vec<Rr3Frame> = Vec::new();
    let mut outcome: Result<Rr3Frame, String> = Err(format!(
        "{SCAN_BUDGET} 帧内没等到{what}（超出预算即视为夹具 bug：谓词写错或 rrd 确实没发这一帧）"
    ));
    for _ in 0..SCAN_BUDGET {
        match rr3_try_recv(c) {
            Ok(frame) => {
                if pred(&frame) {
                    outcome = Ok(frame);
                    break;
                }
                deferred.push(frame);
            }
            // 对端关闭（Closed / Torn*）与 5 s 读超时（Io os error 11）都不是"没等到 X"的
            // 替代品：要说清没等到什么、哪些帧还压在队列里。
            Err(e) => {
                outcome = Err(format!("没等到{what}（读帧提前结束: {e:?}）"));
                break;
            }
        }
    }
    // 本轮取走的帧按到达次序回到队列头部（倒序 push_front 正好还原正序）
    for f in deferred.into_iter().rev() {
        c.pending.push_front(f);
    }
    match outcome {
        Ok(f) => f,
        // 本轮扫过的帧此刻已经在队列里，所以 `rr3_pending_dump` 就是"积压 + 本轮读到"的完整现场
        Err(msg) => panic!("{}；{}", msg, rr3_pending_dump(c)),
    }
}

/// 一直读到 EOF 或读超时，返回收到的全部帧（队列里的也算）。
/// 用于"这条连接该被关掉/不该出现某帧"的断言——它不会 panic，所以能把
/// "rrd 有没有回 accepted"这类否定式判据钉住。终止靠的是 `rr3_connect` 设的 5 s 读超时。
pub fn rr3_drain_until_closed(c: &mut RrdClient) -> Vec<Rr3Frame> {
    let mut got = Vec::new();
    while let Ok(f) = rr3_try_recv(c) {
        got.push(f);
    }
    got
}

/// 收集侧的硬墙钟上限：`rr3_collect_within` 的终止条件是"安静 dur"，一条持续发帧的连接
/// 让它永远安静不下来。真出现那种形状（风暴广播）时，正确的结论是"帧流在 cap 内没停过"
/// 并把手上收到的帧打出来，而不是让整条测试目标挂死。
const COLLECT_WALL_CLOCK_CAP: Duration = Duration::from_secs(5);

/// 在 `dur` 内把这一条连接上**能读到的帧全部收走**（不 panic：读不到就是这一轮结束）。
/// 队列里的存货先算进返回值（`rr3_try_recv` 的既有口径），用于"这条连接上不该出现 X"的否定断言。
/// 它是**破坏性**读法：取走的帧**不回队列**（与本文件头和 `rr3_read_where` 那种"未匹配帧存回队列、
/// 可重读"的口径正相反）。
/// **只准用在用例末尾**——之后同一连接不得再有读法，否则后续否定断言会空转、正向读法会白等 5 s。
///
/// 为什么不能直接用 `rr3_drain_until_closed`：那条的终止条件是 `rr3_connect` 设的 **5 s 读超时**
/// （`support/mod.rs` 的 `rr3_connect`），拿它做"150 ms 内没回显即通过"的判据会让每条这样的用例
/// 白等 4.85 s，Task 10/11 的否定断言一多就把测试时长推到分钟级。这里临时把读超时压到 `dur`、
/// **收完把原值设回去**：设回去不是洁癖——同一连接后来的读法要靠 5 s 等到慢副作用（重启、
/// watchdog），留下 150 ms 就等于把别的用例改成随机红。
///
/// 终止条件之外还有一条**墙钟上限**（`COLLECT_WALL_CLOCK_CAP`，Task 8 验收项 A-1）：
/// "安静 dur"这一条只挡得住"帧流会停"的连接，一条持续发帧的连接能让它永远收不完、
/// 于是整条测试目标挂到 cargo 超时且诊断信息为零。超限即 panic 并打印已收到的帧数与类型。
pub fn rr3_collect_within(c: &mut RrdClient, dur: Duration) -> Vec<Rr3Frame> {
    let saved = c.stream.read_timeout().ok();
    c.stream
        .set_read_timeout(Some(dur))
        .expect("临时读超时不可设");
    let started = Instant::now();
    let mut got = Vec::new();
    while let Ok(f) = rr3_try_recv(c) {
        got.push(f);
        if started.elapsed() > COLLECT_WALL_CLOCK_CAP {
            // 恢复超时要尽力做：panic 路径上管不上，至少不是"把一个 5 s 读超时留给别的用例"
            c.stream.set_read_timeout(saved.flatten()).ok();
            let kinds: Vec<String> = got
                .iter()
                .map(|f| {
                    rr3_body_split(&f.payload)
                        .map(|(ty, _)| format!("{ty:?}"))
                        .unwrap_or_else(|_| "坏前缀".into())
                })
                .collect();
            panic!(
                "帧流在 {COLLECT_WALL_CLOCK_CAP:?} 内没停过（收到 {} 帧）: {}",
                got.len(),
                kinds.join(",")
            );
        }
    }
    c.stream
        .set_read_timeout(saved.flatten())
        .expect("恢复读超时失败");
    got
}

/// 第一条 Response
pub fn rr3_read_rpc_frame(c: &mut RrdClient) -> Rr3Frame {
    rr3_read_where(c, " Response", |f| {
        rr3_body_split(&f.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    })
}

pub fn rr3_read_rpc(c: &mut RrdClient) -> Value {
    let f = rr3_read_rpc_frame(c);
    rr3_json_of(&f)
}

/// 第一条 `rr3-runtime/module/state` 广播且 module/phase 匹配，返回体
pub fn rr3_read_state(c: &mut RrdClient, module: &str, phase: &str) -> Value {
    let f = rr3_read_where(
        c,
        &format!("state 广播 {module}/{phase}"),
        |f| match rr3_body_split(&f.payload) {
            Ok((Rr3PayloadType::Notification, body)) => {
                let Ok(v) = serde_json::from_slice::<Value>(&body) else {
                    return false;
                };
                v["topic"] == "rr3-runtime/module/state"
                    && v["module"] == module
                    && v["phase"] == phase
            }
            _ => false,
        },
    );
    rr3_json_of(&f)
}

// ---------- 真模块夹具（e2e_audio_chain 起，多个真模块 e2e 共用）----------

/// 仓根（CARGO_MANIFEST_DIR = base/runtime/rrd，上溯三级）
pub fn rr3_repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// 现场编译一个独立工程模块，返回其二进制绝对路径（`modules/<name>/target/debug/<name>`）。
/// 模块二进制**由测试现场 cargo build**（独立工程，`CARGO_BIN_EXE` 够不着它们）：
/// 增量编译下缓存命中，代价只在首次全量。编译失败把 cargo 输出原样 panic 出来——
/// 夹具烂和被测行为红必须分得开。
pub fn rr3_build_module_bin(name: &str) -> PathBuf {
    let dir = rr3_repo_root().join("modules").join(name);
    let manifest = dir.join("Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(&cargo)
        .args(["build", "-q", "--manifest-path"])
        .arg(&manifest)
        .output()
        .unwrap_or_else(|e| panic!("起 {cargo} 失败: {e}"));
    assert!(
        out.status.success(),
        "编译 {name} 失败:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let bin = dir.join("target/debug").join(name);
    let bin = bin
        .canonicalize()
        .unwrap_or_else(|e| panic!("编译产物 {} 不存在: {e}", bin.display()));
    assert!(bin.is_file(), "{} 不是文件", bin.display());
    bin
}

/// 读真模块的 module.toml（生产契约工件），把 `entry = "<名>"` 换成刚编译出的
/// 二进制绝对路径，返回渲染后的 manifest 文本。替换没命中即 panic：module.toml
/// 的 entry 形状变了时，静默不改会让 rrd 去 spawn 一个不存在的 `<名>` 命令。
/// 落盘由调用方决定（--manifest 直驱落 /tmp 唯一路；profile 正门写进 modules-root 布局）。
pub fn rr3_module_manifest_text(name: &str, bin: &Path) -> String {
    let src = rr3_repo_root()
        .join("modules")
        .join(name)
        .join("module.toml");
    let text = std::fs::read_to_string(&src)
        .unwrap_or_else(|e| panic!("读模块 manifest {} 失败: {e}", src.display()));
    let needle = format!(r#"entry = "{name}""#);
    let rendered = text.replace(&needle, &format!(r#"entry = "{}""#, bin.display()));
    assert!(
        rendered != text && rendered.contains(&format!(r#"entry = "{}""#, bin.display())),
        "module.toml 里没有 {needle:?} 可替换——契约工件形状变了，本夹具前提失效"
    );
    rendered
}

/// 旁观连接：注册一个最小模块身份，专收广播与转发（sim_peer 用例的同一口径）。
/// 返回 (客户端, 本旁观者分配的 task 号)。
pub fn rr3_connect_observer(rrd: &mut RrdTest) -> (RrdClient, u64) {
    let mut obs = rrd.rr3_connect();
    // 裸测试客户端**不喂狗**（没有心跳线程），而本用例的 rrd 以 E2E_TICK_MS=20ms 扫表：
    // 默认 100/500ms 的观察者注册后 ~0.5s 就被 watchdog 判死摘册——之后 pcm 广播不再投它、
    // rr3.call 以"调用方已不在册"被拒，症状是"恰收到 1 帧 registered 通知后永久静默"。
    // 心跳/看门狗窗口放大到用例寿命之外（hb < wd 是校验硬规），判据是"别在用例中途死"。
    rr3_send(
        &mut obs,
        &rr3_register_frame(
            &rr3_manifest("rr3-obs", 3_600_000, 3_600_001, "never"),
            "rr3-obs",
        ),
    );
    let v = rr3_read_rpc(&mut obs);
    let task = v["result"]["src_task"]
        .as_u64()
        .unwrap_or_else(|| panic!("注册应答缺 src_task: {v}"));
    (obs, task)
}

/// 在持续广播流里等一条 rr3.call 应答（按 JSON-RPC id 挑）。
///
/// 不能用 `rr3_read_where`：它那条"64 帧预算扫不到即夹具 bug"的口径，前提是连接上
/// **没有持续帧流**。pcm 那类 50fps 广播把队列塞满——前几段读法存回队列的旧帧
/// 会被应答读法整段重扫，64 帧预算几乎全耗在旧帧上。所以流式环境下的挑选不走
/// 队列回存：无关广播直接丢弃，Response 按 id 挑；预算 600 帧（12s 流量）只是
/// 防挂死的上界，应答真的不来会在这里如实 panic，而不是被预算掩盖成"夹具 bug"。
pub fn rr3_read_call_reply(c: &mut RrdClient, id: u64) -> Value {
    for _ in 0..600 {
        match rr3_try_recv(c) {
            Ok(f) => {
                let hit = match rr3_body_split(&f.payload) {
                    Ok((Rr3PayloadType::Response, body)) => serde_json::from_slice::<Value>(&body)
                        .map(|v| v["id"].as_u64() == Some(id))
                        .unwrap_or(false),
                    _ => false,
                };
                if hit {
                    return rr3_json_of(&f);
                }
            }
            Err(e) => panic!("没等到 rr3.call 应答 id={id}（读帧提前结束: {e:?}）"),
        }
    }
    panic!("600 帧内没等到 rr3.call 应答 id={id}——应答没来或晚于 12s 的流量上界");
}

/// 夹具 manifest 的 entry 占位符（两份 module.toml 顶部注释解释了为什么留占位符）
pub const RR3_ENTRY_PLACEHOLDER: &str = "@SIM_PEER_ENTRY@";

/// 落盘守卫：Drop 里删文件。**必须**用守卫而不是让用例自己删——断言失败会 panic，
/// 自己删的那一行根本跑不到，/tmp 就攒下一堆残留（Task 7 评审 N-1 同一条教训）。
pub struct SimManifest {
    pub path: PathBuf,
}

impl Drop for SimManifest {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 读 `tests/fixtures/<dir>/module.toml` → 把 entry 占位符换成 sim_peer 的**真实绝对路径**
/// → 写到 pid+tag 唯一的 /tmp 路径（Global Constraints 8 的同一套唯一名规则），返回守卫。
///
/// 上溯方式与 `RR3_OVERLAY_PATH` 一致（`CARGO_MANIFEST_DIR` + `../tests/fixtures`）：夹具的
/// **数据文件**在 rrd 包外、bin 在 rrd 包内，这两件事各有理由，见 `rrd/src/lib.rs` 文件头。
///
/// 替换发生在这里、发生在交给 rrd 之前：所以"身份来自 manifest 文件"与
/// "entry 来自构建产物"两件事各归其位，占位符不上线。
pub fn rr3_fixture_manifest(tag: &str, dir: &str) -> SimManifest {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(dir)
        .join("module.toml");
    let text = std::fs::read_to_string(&src)
        .unwrap_or_else(|e| panic!("读夹具 manifest {} 失败: {e}", src.display()));
    let entry = env!("CARGO_BIN_EXE_sim_peer");
    assert!(
        Path::new(entry).is_absolute(),
        "CARGO_BIN_EXE 必须是绝对路径，否则 entry 门禁会拒: {entry}"
    );
    let rendered = text.replace(RR3_ENTRY_PLACEHOLDER, entry);
    assert!(
        !rendered.contains(RR3_ENTRY_PLACEHOLDER),
        "entry 占位符没被替换: {dir}"
    );
    let path = PathBuf::from(format!(
        "/tmp/rr3-m2-sim-{}-{tag}-{dir}.toml",
        std::process::id()
    ));
    std::fs::write(&path, rendered)
        .unwrap_or_else(|e| panic!("写夹具 manifest {} 失败: {e}", path.display()));
    SimManifest { path }
}

/// 把夹具 manifest 副本的 `watchdog_timeout_ms` 改写成 `ms`（只动本用例 pid 唯一的 /tmp 副本，
/// 共用夹具文件一字不动）。改写没命中就 panic——夹具形状变了时"静默不改"会让隔离失效。
///
/// 为什么存在：S2/S3（e2e_spawn_restart）要钉"子进程退出码是 owned 崩溃定性的权威"，而
/// 共用夹具的 500ms watchdog 是给 e2e_watchdog 的判据用的。高负载机上 supervisor 的收尸
/// （try_wait + 扫尸节拍）可能饿到 500ms 以上，watchdog 就抢先把条目记成 failed——
/// 收尸那一路于是走 Silent，断言红在竞争噪声而不是被测行为上。拉大这一用例副本的
/// watchdog 窗口，把"谁先记"的竞争从用判据里排掉（R-A4 本来就允许两路任一先记）。
pub fn rr3_fixture_watchdog_ms(m: &SimManifest, ms: u64) {
    let text = std::fs::read_to_string(&m.path).unwrap_or_else(|e| panic!("读夹具副本失败: {e}"));
    let needle = "watchdog_timeout_ms = 500";
    let replaced = text.replace(needle, &format!("watchdog_timeout_ms = {ms}"));
    assert!(
        replaced != text && replaced.contains(&format!("watchdog_timeout_ms = {ms}")),
        "夹具副本里没有 {needle:?} 可改——fixture 形状变了，本助手的隔离前提失效"
    );
    std::fs::write(&m.path, replaced).unwrap_or_else(|e| panic!("写夹具副本失败: {e}"));
}

/// pid 还活着吗。Linux 走 `/proc/{pid}`；macOS 没有 /proc（S5 在 mac 上恒红的根因），
/// 用 `kill -0` 探测——0 号信号不投递，只查权限，"能发"即"活着"。
/// 同用户进程不会 EPERM；僵尸进程返回"活着"，但 S5 的判据是"rrd 死后夹具自己退场"，
/// 夹具退出后由 launchd/init 收尸，窗口里多算的一拍不影响判据方向。
pub fn rr3_pid_alive(pid: u64) -> bool {
    if Path::new(&format!("/proc/{pid}")).exists() {
        return true;
    }
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// sim_peer 子进程 + 它的 stdout 收集线程。
pub struct SimPeer {
    child: Child,
    lines: mpsc::Receiver<String>,
    seen: Vec<String>,
}

impl SimPeer {
    /// 三个环境变量各管各的事：`RR3_BUS_PATH` 连哪儿、`RR3_MANIFEST` **是谁**（身份唯一来源）、
    /// `RR3_SIM_PEER_ROLE` 做**什么**（行为脚本）。role 不参与身份判定，见本轮约定 1。
    /// stderr 继承：夹具自己出错就直接进 cargo 输出，不必再开一条管道（与 `RrdTest` 同一理由）。
    pub fn rr3_start(socket: &Path, manifest: &SimManifest, role: &str) -> SimPeer {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sim_peer"));
        cmd.env("RR3_BUS_PATH", socket)
            .env("RR3_MANIFEST", &manifest.path)
            .env("RR3_SIM_PEER_ROLE", role)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("起 sim_peer（role={role}）失败: {e}"));
        let lines =
            rr3_spawn_line_reader(child.stdout.take().expect("stdout 必须是管道"), "sim-peer");
        SimPeer {
            child,
            lines,
            seen: Vec::new(),
        }
    }

    /// 等到夹具自己打出的里程碑行。上限用 READY_TIMEOUT（与 `RrdTest::rr3_wait_ready`
    /// 同一理由：里程碑行的到来时刻由宿主机负载决定，不是被测行为的时间语义）。
    pub fn rr3_wait(&mut self, needle: &str) -> String {
        rr3_wait_line(&self.lines, &mut self.seen, needle, READY_TIMEOUT)
    }

    /// 还活着吗。`try_wait` 不阻塞——"活着"在本轮是**否证**（没打出"喂狗被拒"、没退出），
    /// 不是时间测量，所以绝不用 `sleep + try_wait` 去猜"过了多久还没死"。
    pub fn rr3_alive(&mut self) -> bool {
        self.child.try_wait().expect("try_wait 不该失败").is_none()
    }

    /// 硬杀（spec §9 的 SIGKILL 场景；Task 11 的重启用例走同一条）
    pub fn rr3_kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SimPeer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
