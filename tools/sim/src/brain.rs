//! `--brain` 演练（M4，ADR-0012 裁定 4）：rrd + rr3-sense(sim) + rr3-brain + rr3-interact
//! 四真进程全语音对话闭环。
//!
//! 与既有两面演练（main.rs `run` 模块）同构：夹具（临时目录 + 套接字 + 三份 wire
//! manifest）→ rrd boot spawn rr3-sense（sim 正弦源，必过 VAD 起音阈值）→ 外部起
//! rr3-interact 与 rr3-brain（shim 脚本对位"装机时写绝对 entry"）→ 有界等待
//! brain 日志出现"对话轮完成"证据 → `rr3-ctl status` 断言三模块 running 且
//! `denied_publish = 0` → 报告 + 收尾。
//!
//! 云端走 mock（`RR3_BRAIN_CLOUD=mock`，ADR-0012 裁定 2）：brain 的 ASR/LLM/TTS
//! 全部确定性应答——闭环里只有云端 socket 一跳是假的，其余（总线、rrd 路由、
//! rr3.call、事件扇出）全是真进程真套接字。本模块依赖 UDS 与真进程，仅 Unix 参与编译。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::drill::{
    rr3_find_repo_root, rr3_manifest_text_ext, rr3_parse_status, rr3_require_bins_ext,
    rr3_wait_modules, RR3_SIM_MODULE_A, RR3_SIM_MODULE_B, RR3_SIM_MODULE_BRAIN,
};

use crate::drill::Rr3SimArgs;

/// 演练就绪上界：冷启动 + 第一轮对话（max_utterance_ms=1500 判停）也要够用
const RR3_BRAIN_READY_DEADLINE: Duration = Duration::from_secs(60);
/// 模块健康参数：与两面演练同值（比生产紧、比测试宽）
const RR3_BRAIN_HB_MS: u64 = 200;
const RR3_BRAIN_WD_MS: u64 = 1000;
/// brain 演练专属 config 的判停上限：burst 源 1s 语音/1s 静音，正常轮由 offset 判停，
/// 时长上限只是兜底（防判停逻辑失效时闭环卡死）
const RR3_BRAIN_MAX_UTTERANCE_MS: u64 = 1500;
/// brain 日志里的轮次完成判据（__main__.py 的完成行前缀）
pub const RR3_BRAIN_TURN_MARK: &str = "对话轮完成";
/// interact 日志里的播放判据（player 的 tts 完成行前缀）
pub const RR3_INTERACT_TTS_MARK: &str = "tts:";

/// brain 的三个演练判据主题（名字必须真实在契约里，tests 有对账用例）
pub const RR3_BRAIN_TOPIC_INTENT: &str = "rr3-brain/intent/result";
pub const RR3_BRAIN_TOPIC_COMPLETION: &str = "rr3-brain/llm/completion";
pub const RR3_SENSE_TOPIC_PCM: &str = "rr3-sense/audio/pcm";

/// 三份 wire manifest 文本（纯函数，tests 直测形状）
pub fn rr3_brain_manifests(
    sense_entry: &Path,
    interact_entry: &Path,
    brain_entry: &Path,
) -> Vec<(String, String)> {
    vec![
        (
            RR3_SIM_MODULE_A.to_string(),
            rr3_manifest_text_ext(
                RR3_SIM_MODULE_A,
                sense_entry,
                "module",
                "rust",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "always",
                &[RR3_SENSE_TOPIC_PCM, "rr3-sense/audio/vad_start"],
                &["rr3-runtime/module/state"],
                &[],
            ),
        ),
        (
            RR3_SIM_MODULE_B.to_string(),
            rr3_manifest_text_ext(
                RR3_SIM_MODULE_B,
                interact_entry,
                "module",
                "rust",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "never",
                &[],
                &["rr3-runtime/module/state"],
                &["rr3-interact/tts/text", "rr3-interact/face/expression"],
            ),
        ),
        (
            RR3_SIM_MODULE_BRAIN.to_string(),
            rr3_manifest_text_ext(
                RR3_SIM_MODULE_BRAIN,
                brain_entry,
                "brain",
                "python",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "never",
                &[RR3_BRAIN_TOPIC_INTENT],
                &["rr3-interact/asr/text", "rr3-connect/net/link", RR3_SENSE_TOPIC_PCM, "rr3-sense/audio/vad_start"],
                &[RR3_BRAIN_TOPIC_COMPLETION],
            ),
        ),
    ]
}

/// brain 演练现场生成的 shim 脚本（装机时由安装步骤写绝对 entry 的开发等价物）
pub fn rr3_brain_shim_script(brain_pkg_dir: &Path) -> String {
    format!(
        "#!/bin/sh\nPYTHONPATH={} exec python3 -m rr3_brain\n",
        brain_pkg_dir.display()
    )
}

/// brain 演练专属 config（mock 云 + 短判停；base_url 是死地址，mock 路径不碰网络）
pub fn rr3_brain_config_text() -> String {
    format!(
        "[rr3-brain]\nlog_level = \"info\"\n\n[rr3-brain.cloud]\n\
         base_url = \"http://127.0.0.1:9/v1\"\nasr_model = \"mock\"\nllm_model = \"mock\"\n\
         tts_model = \"mock\"\ntimeout_s = 5\n\n[rr3-brain.dialog]\nsilence_tail_ms = 700\n\
         max_turn_chars = 800\nmax_utterance_ms = {RR3_BRAIN_MAX_UTTERANCE_MS}\n"
    )
}

pub fn rr3_brain_drill(a: &Rr3SimArgs) -> Result<String, String> {
    // ---- 0. 定位：仓根与二进制 ----
    let repo = match &a.repo {
        Some(p) => p.clone(),
        None => std::env::current_dir()
            .ok()
            .and_then(|cwd| rr3_find_repo_root(&cwd))
            .ok_or("找不到仓根（缺 contracts/topics.toml）：请在仓内运行，或 --repo <路径>")?,
    };
    let (d_rrd, _peer, d_rrctl) = crate::drill::rr3_default_bins(&repo);
    let rrd = a.rrd.clone().unwrap_or(d_rrd);
    let rrctl = a.rrctl.clone().unwrap_or(d_rrctl);
    let sense = a
        .sense
        .clone()
        .unwrap_or(repo.join("modules/rr3-sense/target/debug/rr3-sense"));
    let interact = a
        .interact
        .clone()
        .unwrap_or(repo.join("modules/rr3-interact/target/debug/rr3-interact"));
    let brain_pkg = repo.join("modules/rr3-brain");
    rr3_require_bins_ext(&[
        (&rrd, "cargo build -p rrd（在 base/runtime/ 下）"),
        (&rrctl, "cargo build --manifest-path tools/rrctl/Cargo.toml"),
        (&sense, "cargo build -p rr3-sense（在 modules/rr3-sense/ 下）"),
        (&interact, "cargo build -p rr3-interact（在 modules/rr3-interact/ 下）"),
    ])?;
    if !brain_pkg.join("rr3_brain").is_dir() {
        return Err(format!("缺 Python 包 {}", brain_pkg.join("rr3_brain").display()));
    }
    let py = which_python3()?;

    // ---- 1. 夹具：套接字 + 三份 manifest + shim + 演练 config ----
    let workdir = match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!("rr3-sim-brain-{}", std::process::id())),
    };
    std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
    let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
    let _ = std::fs::remove_file(&socket);

    // brain 的 wire entry：workdir 里的 shim（exec python3 -m rr3_brain，PYTHONPATH 指仓内包）
    let shim_path = workdir.join("rr3-brain");
    std::fs::write(&shim_path, rr3_brain_shim_script(&brain_pkg))
        .map_err(|e| format!("写 shim 失败: {e}"))?;
    make_executable(&shim_path)?;
    let manifest_path = workdir.join("config.toml");
    std::fs::write(&manifest_path, rr3_brain_config_text())
        .map_err(|e| format!("写演练 config 失败: {e}"))?;

    let mut children: Vec<(String, Child)> = Vec::new();
    let mut report = String::new();
    let outcome = (|| -> Result<(), String> {
        for (name, text) in rr3_brain_manifests(&sense, &interact, &shim_path) {
            let p = workdir.join(format!("{name}.toml"));
            std::fs::write(&p, text).map_err(|e| format!("写 {p:?} 失败: {e}"))?;
        }
        let m_sense = workdir.join(format!("{RR3_SIM_MODULE_A}.toml"));
        let m_interact = workdir.join(format!("{RR3_SIM_MODULE_B}.toml"));
        let m_brain = workdir.join(format!("{RR3_SIM_MODULE_BRAIN}.toml"));

        // ---- 2. 起 rrd：boot spawn rr3-sense（sim 正弦源） ----
        let rrd_log = workdir.join("rrd.log");
        let rrd = Command::new(&rrd)
            .arg("--socket").arg(&socket)
            .arg("--manifest").arg(&m_sense)
            .arg("--boot-role").arg("resident")
            .args(["--pre-register-timeout-ms", "3000"])
            .env("RR3_HAL_BACKEND", "sim")
            // burst（1s 正弦 + 1s 静音循环）：VAD 周期性发 onset/offset，
            // brain 晚于首个 onset 注册也能赶上下一轮——sine 恒有声只发一次 onset
            .env("RR3_HAL_SIM_WAVE", "burst")
            .stdout(Stdio::from(std::fs::File::create(&rrd_log).map_err(|e| format!("建 {rrd_log:?} 失败: {e}"))?))
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("起 rrd 失败: {e}"))?;
        let rrd_pid = rrd.id();
        children.push(("rrd".into(), rrd));
        println!("rr3-sim --brain: rrd 已起 pid={rrd_pid}，socket={}", socket.display());

        // ---- 2.5 等 rrd bind 完套接字再起外部模块：sense 由 rrd spawn（bind 后），
        //      interact/brain 是外部进程，抢跑会撞 ENOENT（模块按契约快速退，不重试） ----
        wait_socket_exists(&socket, RR3_BRAIN_READY_DEADLINE)?;

        // ---- 3. 外部起 interact 与 brain ----
        let interact_log = workdir.join("interact.log");
        let interact_child = Command::new(&interact)
            .env("RR3_BUS_PATH", &socket)
            .env("RR3_MANIFEST", &m_interact)
            .env("RR3_HAL_BACKEND", "sim")
            .stdout(Stdio::from(std::fs::File::create(&interact_log).map_err(|e| format!("建 {interact_log:?} 失败: {e}"))?))
            .stderr(Stdio::from(std::fs::OpenOptions::new().append(true).open(&interact_log).map_err(|e| format!("开 {interact_log:?} 失败: {e}"))?))
            .spawn()
            .map_err(|e| format!("起 rr3-interact 失败: {e}"))?;
        children.push((RR3_SIM_MODULE_B.into(), interact_child));

        let brain_log = workdir.join("brain.log");
        let brain_child = Command::new(&py)
            .arg("-u") // 日志落文件不缓冲：轮次完成判据按行可读，进程被杀也不丢现场
            .arg("-m").arg("rr3_brain")
            .env("PYTHONPATH", &brain_pkg)
            .env("RR3_BUS_PATH", &socket)
            .env("RR3_MANIFEST", &m_brain)
            .env("RR3_BRAIN_CLOUD", "mock")
            .env("RR3_BRAIN_CONFIG", &manifest_path)
            .stdout(Stdio::from(std::fs::File::create(&brain_log).map_err(|e| format!("建 {brain_log:?} 失败: {e}"))?))
            .stderr(Stdio::from(std::fs::OpenOptions::new().append(true).open(&brain_log).map_err(|e| format!("开 {brain_log:?} 失败: {e}"))?))
            .spawn()
            .map_err(|e| format!("起 rr3-brain 失败: {e}"))?;
        children.push((RR3_SIM_MODULE_BRAIN.into(), brain_child));

        // ---- 4. 有界等待：三模块 running + brain 轮次完成 + interact 播放过 ----
        rr3_wait_modules(
            || rr3_ctl_status_rows(&rrctl, &socket),
            &[
                (RR3_SIM_MODULE_A, "running"),
                (RR3_SIM_MODULE_B, "running"),
                (RR3_SIM_MODULE_BRAIN, "running"),
            ],
            RR3_BRAIN_READY_DEADLINE,
        )?;
        report.push_str("\n=== 三模块 running ===\n");
        wait_log_contains(&brain_log, RR3_BRAIN_TURN_MARK, RR3_BRAIN_READY_DEADLINE)?;
        report.push_str(&format!("brain 日志出现「{RR3_BRAIN_TURN_MARK}」（云端 mock 的整轮 ASR→LLM→TTS 已走通）\n"));
        wait_log_contains(&interact_log, RR3_INTERACT_TTS_MARK, RR3_BRAIN_READY_DEADLINE)?;
        report.push_str(&format!("interact 日志出现「{RR3_INTERACT_TTS_MARK}」（brain 的 rr3.call 播放点名已应答）\n"));

        // ---- 5. rr3-ctl status 巡检：denied_publish = 0 是"事件真被接受"的线上证据 ----
        let out = Command::new(&rrctl)
            .arg("--bus-path").arg(&socket)
            .arg("status")
            .output()
            .map_err(|e| format!("跑 rr3-ctl status 失败: {e}"))?;
        let status_out = String::from_utf8_lossy(&out.stdout).into_owned();
        if !status_out.contains("denied_publish = 0") {
            return Err(format!("denied_publish ≠ 0：brain 的发布被 rrd 拒了\n{status_out}"));
        }
        report.push_str("\n=== rr3-ctl status 巡检 ===\n");
        report.push_str(&status_out);
        Ok(())
    })();

    // ---- 6. 收尾：杀全部子进程；--keep 保留现场 ----
    if a.keep {
        println!("rr3-sim --brain: --keep 保留现场：socket={} workdir={}", socket.display(), workdir.display());
        println!("rr3-sim --brain: 手动收尾：kill 上述进程；rm -rf {workdir:?}");
        return match outcome {
            Ok(()) => {
                report.push_str("\n=== 演练结论 ===\n全语音对话闭环 OK（云端 mock）\n");
                Ok(report)
            }
            Err(e) => Err(e),
        };
    }
    for (name, mut child) in children {
        let _ = child.kill();
        let _ = child.wait();
        let _ = name;
    }
    let _ = std::fs::remove_file(&socket);
    match outcome {
        Ok(()) => {
            report.push_str("\n=== 演练结论 ===\n全语音对话闭环 OK（云端 mock）：rrd + rr3-sense(sim) + rr3-brain + rr3-interact\n");
            Ok(report)
        }
        Err(e) => Err(format!("{e}\n（现场日志在 {workdir:?}）")),
    }
}

/// 轮询用的 status 快照（与 main.rs run 模块的 rr3_ctl_rows 同一真值）：
/// 连接失败（rrd 尚未就绪）当空表处理，由 deadline 收口
fn rr3_ctl_status_rows(rrctl: &Path, socket: &Path) -> Vec<(String, u64, String)> {
    Command::new(rrctl)
        .arg("--bus-path").arg(socket)
        .arg("status")
        .output()
        .map(|o| rr3_parse_status(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// 有界等套接字文件出现（rrd bind 的时序闸门；超时是 drill 自身故障）
fn wait_socket_exists(socket: &Path, deadline: Duration) -> Result<(), String> {
    let t0 = Instant::now();
    while t0.elapsed() < deadline {
        if socket.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!("超时（{deadline:?}）：rrd 未 bind 套接字 {socket:?}"))
}

fn wait_log_contains(path: &Path, needle: &str, deadline: Duration) -> Result<(), String> {
    let t0 = Instant::now();
    while t0.elapsed() < deadline {
        if let Ok(s) = std::fs::read_to_string(path) {
            if s.contains(needle) {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let tail = std::fs::read_to_string(path)
        .map(|s| s.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
        .unwrap_or_else(|e| format!("（读日志失败: {e}）"));
    Err(format!(
        "超时（{deadline:?}）：日志 {path:?} 未出现「{needle}」\n日志尾部:\n{tail}"
    ))
}

fn which_python3() -> Result<PathBuf, String> {
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let p = dir.join("python3");
        if p.is_file() {
            return Ok(p);
        }
    }
    Err("缺 python3（brain 是 Python 工程，PATH 里找不到）".into())
}

fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(path)
            .map_err(|e| format!("stat {path:?} 失败: {e}"))?
            .permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(path, perm).map_err(|e| format!("chmod {path:?} 失败: {e}"))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err("--brain 演练仅 Unix 可用".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钉住"演练用的名字/主题必须真实在契约里"——契约改名时这里红，不是演练跑到一半红
    #[test]
    fn brain_sim_identities_exist_in_the_real_contract() {
        let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/topics.toml");
        let text = std::fs::read_to_string(&contract)
            .unwrap_or_else(|e| panic!("读 {}: {e}", contract.display()));
        for needle in [
            RR3_SIM_MODULE_A,
            RR3_SIM_MODULE_B,
            RR3_SIM_MODULE_BRAIN,
            RR3_BRAIN_TOPIC_INTENT,
            RR3_BRAIN_TOPIC_COMPLETION,
            RR3_SENSE_TOPIC_PCM,
            "rr3-sense/audio/vad_start",
            "rr3-interact/tts/text",
            "rr3-connect/net/link",
        ] {
            assert!(text.contains(needle), "契约缺 {needle}");
        }
    }

    #[test]
    fn manifests_have_absolute_entries_and_contract_shapes() {
        let ms = rr3_brain_manifests(
            Path::new("/abs/rr3-sense"),
            Path::new("/abs/rr3-interact"),
            Path::new("/abs/rr3-brain"),
        );
        assert_eq!(ms.len(), 3);
        let sense = &ms[0].1;
        assert!(sense.contains("entry = \"/abs/rr3-sense\""), "{sense}");
        assert!(sense.contains("publishes = [\"rr3-sense/audio/pcm\", \"rr3-sense/audio/vad_start\"]"), "{sense}");
        let brain = &ms[2].1;
        assert!(brain.contains("type = \"brain\""), "{brain}");
        assert!(brain.contains("lang = \"python\""), "{brain}");
        assert!(brain.contains("exposes = [\"rr3-brain/llm/completion\"]"), "{brain}");
    }

    #[test]
    fn shim_and_config_shapes() {
        let shim = rr3_brain_shim_script(Path::new("/repo/modules/rr3-brain"));
        assert!(shim.starts_with("#!/bin/sh\n"), "{shim}");
        assert!(shim.contains("PYTHONPATH=/repo/modules/rr3-brain"), "{shim}");
        assert!(shim.contains("exec python3 -m rr3_brain"), "{shim}");
        let cfg = rr3_brain_config_text();
        assert!(cfg.contains("max_utterance_ms = 1500"), "{cfg}");
        assert!(cfg.contains("base_url = \"http://127.0.0.1:9/v1\""), "{cfg}");
    }
}
