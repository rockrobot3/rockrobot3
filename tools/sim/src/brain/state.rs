//! 会话与状态账：夹具、进程起停、有界等待与巡检报告。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::assemble::rr3_ctl_status_rows;
use crate::drill::{
    rr3_require_bins_ext, rr3_wait_modules, Rr3SimArgs, RR3_SIM_MODULE_A, RR3_SIM_MODULE_B,
    RR3_SIM_MODULE_BRAIN,
};

use super::{
    rr3_brain_cfg_text, rr3_brain_defaults, rr3_brain_manifests, RR3_BRAIN_TURN_MARK,
    RR3_INTERACT_TTS_MARK,
};

/// 演练就绪上界：冷启动 + 第一轮对话（max_utterance_ms=1500 判停）也要够用。
/// 真板档（`--audio-backend esp32`）等真人说话，节奏不可预置——env
/// `RR3_BRAIN_READY_DEADLINE_MS` 可拉长（解析失败/缺省回 60s，不动 sim 档节奏）。
fn rr3_brain_ready_at() -> Duration {
    const DEFAULT_S: u64 = 60;
    Duration::from_secs(
        std::env::var("RR3_BRAIN_READY_DEADLINE_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|ms| ms / 1000)
            .filter(|&s| s >= 1)
            .unwrap_or(DEFAULT_S),
    )
}

pub fn rr3_brain_drill(a: &Rr3SimArgs) -> Result<String, String> {
    let bins = rr3_brain_bins(a)?;
    let (audio_backend, sink_backend, cloud_real, brain_base_url) = rr3_brain_defaults(a);
    let fix = rr3_brain_fix(a, &bins, cloud_real, &brain_base_url)?;
    let ctx = Rr3BrainCtx {
        bins: &bins,
        fix: &fix,
        audio_backend: &audio_backend,
        sink_backend: &sink_backend,
        cloud_real,
    };
    let mut children: Vec<(String, Child)> = Vec::new();
    let mut report = String::new();
    let outcome = rr3_brain_run(&ctx, &mut children, &mut report);
    rr3_brain_finish(&ctx, a, children, outcome, report)
}

/// 收尾面：--keep 保留现场或杀全部子进程；成功附录演练结论，失败附日志位置。
fn rr3_brain_finish(
    ctx: &Rr3BrainCtx,
    a: &Rr3SimArgs,
    children: Vec<(String, Child)>,
    outcome: Result<(), String>,
    report: String,
) -> Result<String, String> {
    let cloud_label = format!("云端 {}", rr3_brain_cloud_word(ctx));
    let chain_label = format!(
        "rrd + rr3-sense({}) + rr3-brain + rr3-interact({})",
        ctx.audio_backend, ctx.sink_backend
    );
    let ok_line = rr3_brain_ok_line(a.keep, &cloud_label, &chain_label);
    if a.keep {
        rr3_brain_keep(ctx);
        return rr3_brain_done(outcome, report, &ctx.fix.workdir, true, &ok_line);
    }
    rr3_teardown_procs(children, &ctx.fix.socket);
    rr3_brain_done(outcome, report, &ctx.fix.workdir, false, &ok_line)
}

/// 结论文案：`--keep` 档链条在括号内（全角冒号分隔），常规档在括号后分列。
fn rr3_brain_ok_line(keep: bool, cloud_label: &str, chain_label: &str) -> String {
    if keep {
        format!("\n=== 演练结论 ===\n全语音对话闭环 OK（{cloud_label}：{chain_label}）\n")
    } else {
        format!("\n=== 演练结论 ===\n全语音对话闭环 OK（{cloud_label}）：{chain_label}\n")
    }
}

/// `--keep`：打印现场保留与手动收尾指引（不杀子进程）。
fn rr3_brain_keep(ctx: &Rr3BrainCtx) {
    println!(
        "rr3-sim --brain: --keep 保留现场：socket={} workdir={}",
        ctx.fix.socket.display(),
        ctx.fix.workdir.display()
    );
    println!(
        "rr3-sim --brain: 手动收尾：kill 上述进程；rm -rf {:?}",
        ctx.fix.workdir
    );
}

/// 收尾结论：成功追加结论文本；失败带回现场位置（`--keep` 不追加日志路径）。
fn rr3_brain_done(
    outcome: Result<(), String>,
    mut report: String,
    workdir: &Path,
    keep: bool,
    ok_line: &str,
) -> Result<String, String> {
    match outcome {
        Ok(()) => {
            report.push_str(ok_line);
            Ok(report)
        }
        Err(e) => Err(rr3_brain_fail(e, workdir, keep)),
    }
}

/// 失败面：`--keep` 原样带回；否则补现场日志位置。
fn rr3_brain_fail(e: String, workdir: &Path, keep: bool) -> String {
    if keep {
        return e;
    }
    format!("{e}\n（现场日志在 {workdir:?}）")
}

/// 演练二进制集（定位 + 存在性校验一次做完）。
struct Rr3BrainBins {
    rrd: PathBuf,
    rrctl: PathBuf,
    sense: PathBuf,
    interact: PathBuf,
    brain: PathBuf,
}

fn rr3_brain_bins(a: &Rr3SimArgs) -> Result<Rr3BrainBins, String> {
    let repo = crate::drill::rr3_repo_locate(a)?;
    let dir = crate::drill::rr3_bin_dir(&repo);
    let (d_rrd, _peer, d_rrctl) = crate::drill::rr3_default_bins(&repo);
    let bins = Rr3BrainBins {
        rrd: a.rrd.clone().unwrap_or(d_rrd),
        rrctl: a.rrctl.clone().unwrap_or(d_rrctl),
        sense: a.sense.clone().unwrap_or(dir.join("rr3-sense")),
        interact: a.interact.clone().unwrap_or(dir.join("rr3-interact")),
        brain: a.brain_bin.clone().unwrap_or(dir.join("rr3-brain")),
    };
    rr3_brain_bins_check(&bins)?;
    Ok(bins)
}

/// 五件产物存在性校验（缺件逐件点名各自构建命令）。
fn rr3_brain_bins_check(bins: &Rr3BrainBins) -> Result<(), String> {
    rr3_require_bins_ext(&[
        (&bins.rrd, "cargo build -p rrd（在 base/runtime/ 下）"),
        (
            &bins.rrctl,
            "cargo build --manifest-path tools/rrctl/Cargo.toml",
        ),
        (
            &bins.sense,
            "cargo build -p rr3-sense（在 modules/rr3-sense/ 下）",
        ),
        (
            &bins.interact,
            "cargo build -p rr3-interact（在 modules/rr3-interact/ 下）",
        ),
        (
            &bins.brain,
            "cargo build -p rr3-brain（在 modules/rr3-brain/ 下）",
        ),
    ])
}

/// 演练夹具：工作目录 + 套接字 + 三份 manifest + 两份 config（写完即用）。
struct Rr3BrainFix {
    workdir: PathBuf,
    socket: PathBuf,
    brain_cfg: PathBuf,
    interact_cfg: PathBuf,
    m_sense: PathBuf,
    m_interact: PathBuf,
    m_brain: PathBuf,
}

fn rr3_brain_fix(
    a: &Rr3SimArgs,
    bins: &Rr3BrainBins,
    cloud_real: bool,
    base_url: &str,
) -> Result<Rr3BrainFix, String> {
    let (workdir, socket) = rr3_brain_place(a)?;

    let (brain_cfg, interact_cfg) = rr3_brain_cfgs(&workdir, cloud_real, base_url)?;
    rr3_mods_write(&workdir, bins)?;
    let m_sense = workdir.join(format!("{RR3_SIM_MODULE_A}.toml"));
    let m_interact = workdir.join(format!("{RR3_SIM_MODULE_B}.toml"));
    let m_brain = workdir.join(format!("{RR3_SIM_MODULE_BRAIN}.toml"));
    Ok(Rr3BrainFix {
        workdir,
        socket,
        brain_cfg,
        interact_cfg,
        m_sense,
        m_interact,
        m_brain,
    })
}

/// 演练 config 两份落点（写盘归 [`rr3_brain_cfg_write`]；返回两路径）。
fn rr3_brain_cfgs(
    workdir: &Path,
    cloud_real: bool,
    base_url: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let brain_cfg = workdir.join("config.toml");
    let interact_cfg = workdir.join("interact-config.toml");
    rr3_brain_cfg_write(&brain_cfg, &interact_cfg, cloud_real, base_url)?;
    Ok((brain_cfg, interact_cfg))
}

/// 三份 wire manifest 落盘（写完即用）。
fn rr3_mods_write(workdir: &Path, bins: &Rr3BrainBins) -> Result<(), String> {
    for (name, text) in rr3_brain_manifests(&bins.sense, &bins.interact, &bins.brain) {
        let p = workdir.join(format!("{name}.toml"));
        std::fs::write(&p, text).map_err(|e| format!("写 {p:?} 失败: {e}"))?;
    }
    Ok(())
}

/// 演练工作目录与套接字：`--socket` 显式给定则同目录，否则临时目录按进程号。
fn rr3_brain_place(a: &Rr3SimArgs) -> Result<(PathBuf, PathBuf), String> {
    let workdir = rr3_brain_workdir(a);
    std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
    let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
    let _ = std::fs::remove_file(&socket);
    Ok((workdir, socket))
}

/// 演练工作目录：`--socket` 显式给定取其父目录，否则临时目录按进程号。
fn rr3_brain_workdir(a: &Rr3SimArgs) -> PathBuf {
    match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!("rr3-sim-brain-{}", std::process::id())),
    }
}

/// 演练 config 两份：brain 云档 + interact 独立 config。
fn rr3_brain_cfg_write(
    brain_cfg: &Path,
    interact_cfg: &Path,
    cloud_real: bool,
    base_url: &str,
) -> Result<(), String> {
    std::fs::write(brain_cfg, rr3_brain_cfg_text(cloud_real, base_url))
        .map_err(|e| format!("写演练 config 失败: {e}"))?;
    // interact 是同一 workdir 里的外部进程：其 manifest 平铺 workdir，缺省 config.toml
    // 会撞上面 brain 的 config.toml（同目录同名）——I-11 起 interact 必读 config，
    // 故写独立 interact-config.toml 并 env 指定（与 chain.rs 同一纪律）。
    std::fs::write(interact_cfg, crate::chain::rr3_chain_itact_cfg())
        .map_err(|e| format!("写 interact config 失败: {e}"))
}

/// 演练上下文（一次演练内的不变量；只读借用，便于按阶段拆函数）。
struct Rr3BrainCtx<'a> {
    bins: &'a Rr3BrainBins,
    fix: &'a Rr3BrainFix,
    audio_backend: &'a str,
    sink_backend: &'a str,
    cloud_real: bool,
}

/// 演练主体：起 rrd → 等 bind → 起外部两模块 → 有界等证据 → 状态巡检。
fn rr3_brain_run(
    ctx: &Rr3BrainCtx,
    children: &mut Vec<(String, Child)>,
    report: &mut String,
) -> Result<(), String> {
    rr3_brain_up_rrd(ctx, children)?;
    // 等 rrd bind 完套接字再起外部模块：sense 由 rrd spawn（bind 后），
    // interact/brain 是外部进程，抢跑会撞 ENOENT（模块按契约快速退，不重试）
    wait_socket_exists(&ctx.fix.socket, rr3_brain_ready_at())?;
    rr3_brain_up_ext(ctx, children)?;
    rr3_brain_wait(ctx, report)
}

/// 起 rrd（boot spawn rr3-sense，sim burst 正弦源）。
fn rr3_brain_up_rrd(ctx: &Rr3BrainCtx, children: &mut Vec<(String, Child)>) -> Result<(), String> {
    let fix = ctx.fix;
    let rrd_log = fix.workdir.join("rrd.log");
    let log = Stdio::from(
        std::fs::File::create(&rrd_log).map_err(|e| format!("建 {rrd_log:?} 失败: {e}"))?,
    );
    let rrd = rr3_brain_rrd_cmd(ctx, log)
        .spawn()
        .map_err(|e| format!("起 rrd 失败: {e}"))?;
    let rrd_pid = rrd.id();
    children.push(("rrd".into(), rrd));
    println!(
        "rr3-sim --brain: rrd 已起 pid={rrd_pid}，socket={}，音频={}（源）/ {}（回放），云={}",
        fix.socket.display(),
        ctx.audio_backend,
        ctx.sink_backend,
        rr3_brain_cloud_word(ctx),
    );
    Ok(())
}

/// rrd 命令面（boot spawn rr3-sense；burst 波形让 VAD 周期发边沿）。
fn rr3_brain_rrd_cmd(ctx: &Rr3BrainCtx, log: Stdio) -> Command {
    let fix = ctx.fix;
    let mut cmd = Command::new(&ctx.bins.rrd);
    cmd.arg("--socket")
        .arg(&fix.socket)
        .arg("--manifest")
        .arg(&fix.m_sense)
        .arg("--boot-role")
        .arg("resident")
        .args(["--pre-register-timeout-ms", "3000"])
        .env("RR3_HAL_BACKEND", ctx.audio_backend)
        // burst（1s 正弦 + 1s 静音循环）：VAD 周期性发 onset/offset，
        // brain 晚于首个 onset 注册也能赶上下一轮——sine 恒有声只发一次 onset
        .env("RR3_HAL_SIM_WAVE", "burst")
        .stdout(log)
        .stderr(Stdio::inherit());
    cmd
}

/// 云端档位的报告词（real/mock）。
fn rr3_brain_cloud_word(ctx: &Rr3BrainCtx) -> &'static str {
    if ctx.cloud_real {
        "real"
    } else {
        "mock"
    }
}

/// 外部起 interact 与 brain（两个真模块，各自日志进 workdir）。
fn rr3_brain_up_ext(ctx: &Rr3BrainCtx, children: &mut Vec<(String, Child)>) -> Result<(), String> {
    let fix = ctx.fix;
    rr3_brain_up_ic(ctx, children)?;

    let bc = rr3_brain_cmd(ctx);
    let brain_log = fix.workdir.join("brain.log");
    let brain_child = rr3_spawn_logged(bc, &brain_log, RR3_SIM_MODULE_BRAIN)?;
    children.push((RR3_SIM_MODULE_BRAIN.into(), brain_child));
    Ok(())
}

/// brain 进程的命令面（总线/清单/云档/config 全在此接线）。
fn rr3_brain_cmd(ctx: &Rr3BrainCtx) -> Command {
    let fix = ctx.fix;
    let mut bc = Command::new(&ctx.bins.brain);
    bc.env("RR3_BUS_PATH", &fix.socket)
        .env("RR3_MANIFEST", &fix.m_brain)
        .env("RR3_BRAIN_CLOUD", rr3_brain_cloud_word(ctx))
        .env("RR3_BRAIN_CONFIG", &fix.brain_cfg);
    bc
}

/// 外部起 interact（回放面独立后端；面板地址可被 env 覆盖）。
fn rr3_brain_up_ic(ctx: &Rr3BrainCtx, children: &mut Vec<(String, Child)>) -> Result<(), String> {
    let fix = ctx.fix;
    let ic = rr3_ic_cmd(ctx);
    let interact_log = fix.workdir.join("interact.log");
    let interact_child = rr3_spawn_logged(ic, &interact_log, RR3_SIM_MODULE_B)?;
    children.push((RR3_SIM_MODULE_B.into(), interact_child));
    Ok(())
}

/// interact 进程的命令面（回放面后端 + 面板地址 + config 接线）。
fn rr3_ic_cmd(ctx: &Rr3BrainCtx) -> Command {
    let fix = ctx.fix;
    let mut ic = Command::new(&ctx.bins.interact);
    ic.env("RR3_BUS_PATH", &fix.socket)
        .env("RR3_MANIFEST", &fix.m_interact)
        // 回放面独立后端（A 批 c4）：interact 吃 sink_backend——esp32 源档下
        // 源（rrd/sense）与回放（interact）分列，同一 UART 的 flock 排他
        // 天然不成立
        .env("RR3_HAL_BACKEND", ctx.sink_backend)
        // 面板地址：用户显式给的 RR3_PANEL_ADDR 优先（手机/局域网面板面），
        // 未给才落演练缺省回环 18080
        .env("RR3_PANEL_ADDR", rr3_panel_addr())
        .env("RR3_INTERACT_CONFIG", &fix.interact_cfg);
    ic
}

/// 面板地址：环境变量 RR3_PANEL_ADDR 优先，未给才落演练缺省回环。
fn rr3_panel_addr() -> String {
    std::env::var("RR3_PANEL_ADDR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| crate::drill::RR3_SIM_PANEL_ADDR.to_string())
}

/// 有界等：三模块 running → brain 轮次完成 → interact 播放过 → 拒绝发布为 0。
fn rr3_brain_wait(ctx: &Rr3BrainCtx, report: &mut String) -> Result<(), String> {
    let fix = ctx.fix;
    rr3_wait_modules(
        || rr3_ctl_status_rows(&ctx.bins.rrctl, &fix.socket),
        &[
            (RR3_SIM_MODULE_A, "running"),
            (RR3_SIM_MODULE_B, "running"),
            (RR3_SIM_MODULE_BRAIN, "running"),
        ],
        rr3_brain_ready_at(),
    )?;
    report.push_str("\n=== 三模块 running ===\n");
    rr3_brain_logs_wait(ctx, report)?;
    rr3_brain_status(ctx, report)
}

/// 巡检面：`rr3-ctl status` 的 denied_publish 归零门与状态表入报告。
fn rr3_brain_status(ctx: &Rr3BrainCtx, report: &mut String) -> Result<(), String> {
    let status_out = rr3_ctl_denied_zero(
        &ctx.bins.rrctl,
        &ctx.fix.socket,
        "denied_publish ≠ 0：brain 的发布被 rrd 拒了",
    )?;
    report.push_str("\n=== rr3-ctl status 巡检 ===\n");
    report.push_str(&status_out);
    Ok(())
}

/// 两处留痕：brain 轮次完成行 → interact TTS 播放行（各按演练 deadline 有界等）。
fn rr3_brain_logs_wait(ctx: &Rr3BrainCtx, report: &mut String) -> Result<(), String> {
    let brain_log = ctx.fix.workdir.join("brain.log");
    wait_log_contains(&brain_log, RR3_BRAIN_TURN_MARK, rr3_brain_ready_at())?;
    report.push_str(&format!(
        "brain 日志出现「{RR3_BRAIN_TURN_MARK}」（云端 mock 的整轮 ASR→LLM→TTS 已走通）\n"
    ));
    rr3_interact_trace(ctx, report)
}

/// interact 播放留痕（brain 的 rr3.call 播放点名已应答）。
fn rr3_interact_trace(ctx: &Rr3BrainCtx, report: &mut String) -> Result<(), String> {
    let interact_log = ctx.fix.workdir.join("interact.log");
    wait_log_contains(&interact_log, RR3_INTERACT_TTS_MARK, rr3_brain_ready_at())?;
    report.push_str(&format!(
        "interact 日志出现「{RR3_INTERACT_TTS_MARK}」（brain 的 rr3.call 播放点名已应答）\n"
    ));
    Ok(())
}

/// 起一个带日志的外部进程（stdout 建、stderr 追加；两份演练共用）。
pub(crate) fn rr3_spawn_logged(mut cmd: Command, log: &Path, name: &str) -> Result<Child, String> {
    let out = std::fs::File::create(log).map_err(|e| format!("建 {log:?} 失败: {e}"))?;
    let err = std::fs::OpenOptions::new()
        .append(true)
        .open(log)
        .map_err(|e| format!("开 {log:?} 失败: {e}"))?;
    cmd.stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| format!("起 {name} 失败: {e}"))
}

/// `rr3-ctl status` 巡检：`denied_publish = 0` 是"事件真被接受"的线上证据。
/// `denial` 是失败时点名文案（两份演练各自的语境），空 stdout 也会带出。
pub(crate) fn rr3_ctl_denied_zero(
    rrctl: &Path,
    socket: &Path,
    denial: &str,
) -> Result<String, String> {
    let out = Command::new(rrctl)
        .arg("--bus-path")
        .arg(socket)
        .arg("status")
        .output()
        .map_err(|e| format!("跑 rr3-ctl status 失败: {e}"))?;
    let status_out = String::from_utf8_lossy(&out.stdout).into_owned();
    if !status_out.contains("denied_publish = 0") {
        return Err(format!("{denial}\n{status_out}"));
    }
    Ok(status_out)
}

/// 收尾共用：杀光子进程、删总线套接字（--keep 保留现场分支不走这里）
pub(crate) fn rr3_teardown_procs(children: Vec<(String, Child)>, socket: &Path) {
    for (name, mut child) in children {
        let _ = child.kill();
        let _ = child.wait();
        let _ = name;
    }
    let _ = std::fs::remove_file(socket);
}

/// 有界等套接字文件出现（rrd bind 的时序闸门；超时是 drill 自身故障）
pub(crate) fn wait_socket_exists(socket: &Path, deadline: Duration) -> Result<(), String> {
    let t0 = Instant::now();
    while t0.elapsed() < deadline {
        if socket.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "超时（{deadline:?}）：rrd 未 bind 套接字 {socket:?}"
    ))
}

/// 有界等日志出现 needle（--brain 与 --chain 演练共用：等轮次完成/播放/转向留痕）
pub(crate) fn wait_log_contains(
    path: &Path,
    needle: &str,
    deadline: Duration,
) -> Result<String, String> {
    let t0 = Instant::now();
    let mut seen = String::new();
    while t0.elapsed() < deadline {
        if let Some(line) = rr3_log_probe(path, needle, &mut seen) {
            return Ok(line);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!(
        "超时（{deadline:?}）：日志 {path:?} 未出现「{needle}」\n日志尾部:\n{}",
        rr3_log_tail(&seen)
    ))
}

/// 日志尾部：末 15 行，保持原序。
fn rr3_log_tail(seen: &str) -> String {
    seen.lines()
        .rev()
        .take(15)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

/// 日志里首个含 `needle` 的行（无则 None）。
fn rr3_log_find(s: &str, needle: &str) -> Option<String> {
    s.lines().find(|l| l.contains(needle)).map(str::to_string)
}

/// 读一次日志并找 needle；未命中时把最新全文存进 `seen`（超时尾部用）。
fn rr3_log_probe(path: &Path, needle: &str, seen: &mut String) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let hit = rr3_log_find(&s, needle);
    *seen = s;
    hit
}
