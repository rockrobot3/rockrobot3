//! 仓根/装机布局夹具：模块二进制、装配单与 SimManifest 守卫。

use super::*;

// ---------- 真模块夹具（e2e_audio_chain 起，多个真模块 e2e 共用）----------

/// 仓根（CARGO_MANIFEST_DIR = base/runtime/rrd，上溯三级）
pub fn rr3_repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// e2e 装机档共用的接线档底稿（单一真值 `src/testdata/wiring_bottom.toml`）：
/// vcc/gnd-only 单线组（net=rail5v 在装配单域在册），对 [[pin]] 增删免疫。
pub const RR3_E2E_WIRING: &str = include_str!("../../src/testdata/wiring_bottom.toml");

/// 装机布局夹具根（e2e_power / e2e_act / e2e_connect 同款，三处各写一份迟早分叉）：
/// `<root>/profiles/default.toml` + `<root>/hardware/default/wiring.toml`——
/// rr3_wiring_path 推导的同一棵树。`scenario` 拼目录名（power/act/connect →
/// `rr3-<scenario>-e2e-<pid>-<tag>`，pid 级唯一）。Drop 清整树——断言 panic 后
/// /tmp 不许留残迹（Task 7 评审 N-1 教训，`e2e_profile.rs` 的 Root 同一条纪律；
/// 那份 Root 行为更多，保持独立不并入）。
pub struct E2eRoot {
    pub dir: PathBuf,
}

impl E2eRoot {
    /// 建根：清 pid 复用留下的上次残根，铺 profiles/ 一级。
    pub fn rr3_new(scenario: &str, tag: &str) -> E2eRoot {
        let dir =
            std::env::temp_dir().join(format!("rr3-{scenario}-e2e-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); // pid 复用留下的上次残根不许干扰本次
        std::fs::create_dir_all(dir.join("profiles")).expect("建 profiles 目录");
        E2eRoot { dir }
    }

    /// 落一份装配单，并同批 stage 接线档（hardware/<形态>/wiring.toml）：
    /// 装载期校验⑦要求装机档必在。
    pub fn rr3_write_profile(&self, text: &str) -> PathBuf {
        let p = self.dir.join("profiles").join("default.toml");
        std::fs::write(&p, text).expect("写装配单");
        let w = self
            .dir
            .join("hardware")
            .join("default")
            .join("wiring.toml");
        std::fs::create_dir_all(w.parent().unwrap()).expect("建 hardware 目录");
        std::fs::write(&w, RR3_E2E_WIRING).expect("写接线档");
        p
    }
}

impl Drop for E2eRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 现场编译一个独立工程模块，返回其二进制绝对路径。
/// 模块二进制**由测试现场 cargo build**（独立工程，`CARGO_BIN_EXE` 够不着它们）：
/// 增量编译下缓存命中，代价只在首次全量。编译失败把 cargo 输出原样 panic 出来——
/// 夹具烂和被测行为红必须分得开。
/// 产物路径从 `--message-format=json` 的 `compiler-artifact.executable` 字段取，
/// 不假设 `target/debug/<name>`——仓根 `.cargo/config.toml` 可把 target-dir 外移到
/// `/tmp/rr3-build`，硬编码相对路径会找不到产物（P0-5 后回归门）。
pub fn rr3_build_module_bin(name: &str) -> PathBuf {
    let dir = rr3_repo_root().join("modules").join(name);
    let manifest = dir.join("Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(&cargo)
        .args(["build", "--message-format=json", "--manifest-path"])
        .arg(&manifest)
        .output()
        .unwrap_or_else(|e| panic!("起 {cargo} 失败: {e}"));
    if !out.status.success() {
        panic!(
            "编译 {name} 失败:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let bin = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| {
            v.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact")
                && v.get("target")
                    .and_then(|t| t.get("name"))
                    .and_then(|n| n.as_str())
                    == Some(name)
        })
        .and_then(|v| {
            v.get("executable")
                .and_then(|e| e.as_str())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| panic!("编译 {name} 成功但 JSON 输出里没有 executable 字段"));
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
/// 为什么存在：S1/S2/S3（e2e_spawn_restart）要钉"子进程退出码是 owned 崩溃定性的权威"，而
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
