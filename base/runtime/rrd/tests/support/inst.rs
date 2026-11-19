//! 真 rrd 子进程夹具：启动、逐行收取、字段等待与 Drop 收尸。

use super::*;

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
/// 装配单文本 = 数据夹具单一真值（`src/testdata/profile_host_prefix.toml`：schema/os/
/// runtime/platform/supplies+rail5v 域/budget 五段公共前缀）+ 本夹具独有的尾部三段。
/// 语义不变：**空 enabled_slots 在 parse 层即拒**、stub manifest 必须真实落盘等约束
/// 见下方函数文档。
const RR3_LISTEN_PROFILE: &str = concat!(
    include_str!("../../src/testdata/profile_host_prefix.toml"),
    r#"
[slots]
present = []

[bindings]

[enabled_slots]
rr3-listenstub = ["act.servo"]
"#,
);

/// 监听面夹具的接线档底稿（单一真值 `src/testdata/wiring_bottom.toml`，e2e 各装机档
/// 与 profile.rs 测试区同一底稿）：装载期校验⑦要求装机档必在——vcc/gnd-only 单线组
///（net=rail5v 在装配单域在册），对 [[pin]] 增删免疫。
const RR3_LISTEN_WIRING: &str = include_str!("../../src/testdata/wiring_bottom.toml");

/// 造一份监听面夹具（`e2e_profile.rs` 的 Root 同一装机布局）：
/// `<root>/profiles/default.toml`、`<root>/hardware/default/wiring.toml`、
/// `<root>/modules/rr3-listenstub/module.toml`——rr3_wiring_path 推导的
/// 同一棵树；名字取套接字 stem，pid 级唯一）。
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
/// 2026-11-15 复核：重负载下的 `→ running` 超时**不是**本常量不够——根因是面板缺省
/// 8080 在并行测试二进制间撞口，interact 绑不上即退 1、rrd 反复重启（夹具已改注入
/// `RR3_PANEL_ADDR=127.0.0.1:0`，三份并发 + 全量编译复跑 3/3 绿）。超时值维持 20s，
/// 不拿放宽判据掩盖真故障。
pub const READY_TIMEOUT: Duration = Duration::from_secs(20);

mod start;
mod util;
pub use util::*;

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
