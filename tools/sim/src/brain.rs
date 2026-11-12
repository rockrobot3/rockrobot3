//! `--brain` 演练（M4，ADR-0014 裁定 4）：rrd + rr3-sense(sim) + rr3-brain + rr3-interact
//! 四真进程全语音对话闭环。
//!
//! 与既有两面演练（main.rs `run` 模块）同构：夹具（临时目录 + 套接字 + 三份 wire
//! manifest）→ rrd boot spawn rr3-sense（sim 正弦源，必过 VAD 起音阈值）→ 外部起
//! rr3-interact 与 rr3-brain（`target/debug` 同名二进制，与其余模块同形）→ 有界等待
//! brain 日志出现"对话轮完成"证据 → `rr3-ctl status` 断言三模块 running 且
//! `denied_publish = 0` → 报告 + 收尾。
//!
//! 云端缺省走 mock（`RR3_BRAIN_CLOUD=mock`，ADR-0014 裁定 2）：brain 的 ASR/LLM/TTS
//! 全部确定性应答——闭环里只有云端 socket 一跳是假的，其余（总线、rrd 路由、
//! rr3.call、事件扇出）全是真进程真套接字。真云档（`--brain-cloud real
//! --brain-base-url <端点>` + 环境变量 `RR3_CLOUD_API_KEY`）把最后一跳也接真——
//! 端点 fail-closed 必给（演练 config 缺省是死地址，不落）。音频后端缺省 sim，
//! `--audio-backend mac` 换 macOS 真麦/扬声器（rr-hal `mac` 后端，真件显式启用
//! 纪律）；`--sink-backend` 分列 interact 回放面（缺省跟随源面，既有链路零改动；
//! `--audio-backend esp32` 必给——flock 排他下 interact 不得吃源面 UART，A 批 c4）。
//! 本模块依赖 UDS 与真进程，仅 Unix 参与编译。

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::assemble::rr3_ctl_status_rows;
use crate::drill::{
    rr3_manifest_ext, rr3_require_bins_ext, rr3_wait_modules, RR3_SIM_MODULE_A, RR3_SIM_MODULE_B,
    RR3_SIM_MODULE_BRAIN,
};

use crate::drill::Rr3SimArgs;

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

/// brain 的演练判据主题（名字必须真实在契约里，tests 有对账用例）
pub const RR3_BRAIN_TOPIC_INTENT: &str = "rr3-brain/intent/result";
pub const RR3_BRAIN_TOPIC_COMPLETION: &str = "rr3-brain/llm/completion";
pub const RR3_BRAIN_TOPIC_TTS_PCM: &str = "rr3-brain/audio/tts_pcm";
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
            rr3_manifest_ext(
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
            rr3_manifest_ext(
                RR3_SIM_MODULE_B,
                interact_entry,
                "module",
                "rust",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "never",
                &[],
                &["rr3-runtime/module/state", RR3_BRAIN_TOPIC_TTS_PCM],
                &["rr3-interact/tts/text", "rr3-interact/face/expression"],
            ),
        ),
        (
            RR3_SIM_MODULE_BRAIN.to_string(),
            rr3_manifest_ext(
                RR3_SIM_MODULE_BRAIN,
                brain_entry,
                "brain",
                "rust",
                RR3_BRAIN_HB_MS,
                RR3_BRAIN_WD_MS,
                "never",
                &[RR3_BRAIN_TOPIC_INTENT, RR3_BRAIN_TOPIC_TTS_PCM],
                // C14（ADR-0019 修订 2 ②，2026-10-10 评审裁定）：asr/text retired 撤申报、
                // net/link 撤申报（不补实现）；订阅面对齐实然面 = pcm + vad_start（B-1 补齐）
                &[RR3_SENSE_TOPIC_PCM, "rr3-sense/audio/vad_start"],
                &[RR3_BRAIN_TOPIC_COMPLETION],
            ),
        ),
    ]
}

/// brain 演练专属 config。mock 档（缺省）：死地址 + mock 模型，闭环不碰网络、
/// burst 源 1.5s 短判停。real 档（`--brain-cloud real`）：真端点 + 生产模型名
/// （真值源 modules/rr3-brain/config.toml，此处按值镜像）+ 人声 8s 判停（生产值，
/// 真人说话远慢于 burst 周期）。0.0.6 模型连接抽象批：real 档补 `[[rr3-brain.model]]`
/// 连接行——`--brain-base-url` 旗标按主机根传入，行 base_url 补成完整 API 根
/// （ADR-0023：LLM 只拼 /chat/completions），LLM 腿走 openai 格式，密钥引用
/// `env:RR3_CLOUD_API_KEY`（演练面沿用旧环境变量契约，面板 secrets 是板上/宿主面）。
pub fn rr3_brain_cfg_text(cloud_real: bool, base_url: &str) -> String {
    if cloud_real {
        const ASR_MODEL: &str = "iic/SenseVoiceSmall";
        const LLM_MODEL: &str = "deepseek-ai/DeepSeek-V3";
        const TTS_MODEL: &str = "FunAudioLLM/CosyVoice2-0.5B";
        // 演练面两义分离（ADR-0023）：cloud.base_url = 主机根（ASR/TTS 自拼
        // /v1/audio/*）；模型行 base_url = 完整 API 根（LLM 只拼 /chat/completions）。
        let llm_root = rr3_llm_root_of(base_url);
        format!(
            "[rr3-brain]\nlog_level = \"info\"\n\n[rr3-brain.cloud]\n\
             base_url = \"{base_url}\"\nasr_model = \"{ASR_MODEL}\"\nllm_model = \"{LLM_MODEL}\"\n\
             tts_model = \"{TTS_MODEL}\"\ntimeout_s = 20\n\n[rr3-brain.dialog]\n\
             max_utterance_ms = 8000\n\n[[rr3-brain.model]]\nname = \"演练真云\"\n\
             mode = \"api\"\napi_format = \"openai\"\nbase_url = \"{llm_root}\"\n\
             model_id = \"{LLM_MODEL}\"\napi_key_ref = \"env:RR3_CLOUD_API_KEY\"\nactive = true\n"
        )
    } else {
        format!(
            "[rr3-brain]\nlog_level = \"info\"\n\n[rr3-brain.cloud]\n\
             base_url = \"http://127.0.0.1:9/v1\"\nasr_model = \"mock\"\nllm_model = \"mock\"\n\
             tts_model = \"mock\"\ntimeout_s = 5\n\n[rr3-brain.dialog]\nsilence_tail_ms = 700\n\
             max_turn_chars = 800\nmax_utterance_ms = {RR3_BRAIN_MAX_UTTERANCE_MS}\n"
        )
    }
}

/// `--brain-base-url` 取值换算（与 rr3-brain boot 的 cloud fallback 同义）：主机根
/// 路径为空补 `/v1`；已带路径（GLM `/api/paas/v4`、Qwen `/compatible-mode/v1`）原样。
fn rr3_llm_root_of(base_url: &str) -> String {
    let s = base_url.trim().trim_end_matches('/');
    let has_path = match s.split_once("://") {
        Some((_, rest)) => rest.contains('/'),
        None => s.contains('/'),
    };
    if has_path {
        s.to_string()
    } else {
        format!("{s}/v1")
    }
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

/// 演练缺省面：音频/回放后端缺省 sim（CI/无头纪律），回放跟随源面；云端缺省
/// mock、base_url 占位（real 必给已在参数面 fail-closed）。
fn rr3_brain_defaults(a: &Rr3SimArgs) -> (String, String, bool, String) {
    let audio_backend = a.audio_backend.clone().unwrap_or_else(|| "sim".to_string());
    let sink_backend = a
        .sink_backend
        .clone()
        .unwrap_or_else(|| audio_backend.clone());
    let cloud_real = a.brain_cloud.as_deref() == Some("real");
    let brain_base_url = a
        .brain_base_url
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:9/v1".to_string());
    (audio_backend, sink_backend, cloud_real, brain_base_url)
}

/// 收尾面：--keep 保留现场或杀全部子进程；成功附录演练结论，失败附日志位置。
fn rr3_brain_finish(
    ctx: &Rr3BrainCtx,
    a: &Rr3SimArgs,
    children: Vec<(String, Child)>,
    outcome: Result<(), String>,
    report: String,
) -> Result<String, String> {
    let cloud_label = if ctx.cloud_real {
        "云端 real"
    } else {
        "云端 mock"
    };
    let chain_label = format!(
        "rrd + rr3-sense({}) + rr3-brain + rr3-interact({})",
        ctx.audio_backend, ctx.sink_backend
    );
    if a.keep {
        rr3_brain_keep(ctx);
        let ok_line =
            format!("\n=== 演练结论 ===\n全语音对话闭环 OK（{cloud_label}：{chain_label}）\n");
        return rr3_brain_done(outcome, report, &ctx.fix.workdir, true, &ok_line);
    }
    rr3_teardown_procs(children, &ctx.fix.socket);
    let ok_line =
        format!("\n=== 演练结论 ===\n全语音对话闭环 OK（{cloud_label}）：{chain_label}\n");
    rr3_brain_done(outcome, report, &ctx.fix.workdir, false, &ok_line)
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
        Err(e) if keep => Err(e),
        Err(e) => Err(format!("{e}\n（现场日志在 {workdir:?}）")),
    }
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
    ])?;
    Ok(bins)
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

    let brain_cfg = workdir.join("config.toml");
    let interact_cfg = workdir.join("interact-config.toml");
    rr3_brain_cfg_write(&brain_cfg, &interact_cfg, cloud_real, base_url)?;
    rr3_brain_manifests_write(&workdir, bins)?;
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

/// 三份 wire manifest 落盘（写完即用）。
fn rr3_brain_manifests_write(workdir: &Path, bins: &Rr3BrainBins) -> Result<(), String> {
    for (name, text) in rr3_brain_manifests(&bins.sense, &bins.interact, &bins.brain) {
        let p = workdir.join(format!("{name}.toml"));
        std::fs::write(&p, text).map_err(|e| format!("写 {p:?} 失败: {e}"))?;
    }
    Ok(())
}

/// 演练工作目录与套接字：`--socket` 显式给定则同目录，否则临时目录按进程号。
fn rr3_brain_place(a: &Rr3SimArgs) -> Result<(PathBuf, PathBuf), String> {
    let workdir = match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!("rr3-sim-brain-{}", std::process::id())),
    };
    std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
    let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
    let _ = std::fs::remove_file(&socket);
    Ok((workdir, socket))
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
    let rrd = Command::new(&ctx.bins.rrd)
        .arg("--socket")
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
        .stdout(Stdio::from(
            std::fs::File::create(&rrd_log).map_err(|e| format!("建 {rrd_log:?} 失败: {e}"))?,
        ))
        .stderr(Stdio::inherit())
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
    let mut ic = Command::new(&ctx.bins.interact);
    ic.env("RR3_BUS_PATH", &fix.socket)
        .env("RR3_MANIFEST", &fix.m_interact)
        // 回放面独立后端（A 批 c4）：interact 吃 sink_backend——esp32 源档下
        // 源（rrd/sense）与回放（interact）分列，同一 UART 的 flock 排他
        // 天然不成立
        .env("RR3_HAL_BACKEND", ctx.sink_backend)
        // 面板地址：用户显式给的 RR3_PANEL_ADDR 优先（手机/局域网面板面），
        // 未给才落演练缺省回环 18080
        .env(
            "RR3_PANEL_ADDR",
            std::env::var("RR3_PANEL_ADDR")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| crate::drill::RR3_SIM_PANEL_ADDR.to_string()),
        )
        .env("RR3_INTERACT_CONFIG", &fix.interact_cfg);
    let interact_log = fix.workdir.join("interact.log");
    let interact_child = rr3_spawn_logged(ic, &interact_log, RR3_SIM_MODULE_B)?;
    children.push((RR3_SIM_MODULE_B.into(), interact_child));

    let mut bc = Command::new(&ctx.bins.brain);
    bc.env("RR3_BUS_PATH", &fix.socket)
        .env("RR3_MANIFEST", &fix.m_brain)
        .env("RR3_BRAIN_CLOUD", rr3_brain_cloud_word(ctx))
        .env("RR3_BRAIN_CONFIG", &fix.brain_cfg);
    let brain_log = fix.workdir.join("brain.log");
    let brain_child = rr3_spawn_logged(bc, &brain_log, RR3_SIM_MODULE_BRAIN)?;
    children.push((RR3_SIM_MODULE_BRAIN.into(), brain_child));
    Ok(())
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
    let brain_log = fix.workdir.join("brain.log");
    wait_log_contains(&brain_log, RR3_BRAIN_TURN_MARK, rr3_brain_ready_at())?;
    report.push_str(&format!(
        "brain 日志出现「{RR3_BRAIN_TURN_MARK}」（云端 mock 的整轮 ASR→LLM→TTS 已走通）\n"
    ));
    let interact_log = fix.workdir.join("interact.log");
    wait_log_contains(&interact_log, RR3_INTERACT_TTS_MARK, rr3_brain_ready_at())?;
    report.push_str(&format!(
        "interact 日志出现「{RR3_INTERACT_TTS_MARK}」（brain 的 rr3.call 播放点名已应答）\n"
    ));
    let status_out = rr3_ctl_denied_zero(
        &ctx.bins.rrctl,
        &fix.socket,
        "denied_publish ≠ 0：brain 的发布被 rrd 拒了",
    )?;
    report.push_str("\n=== rr3-ctl status 巡检 ===\n");
    report.push_str(&status_out);
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
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Some(line) = s.lines().find(|l| l.contains(needle)) {
                return Ok(line.to_string());
            }
            seen = s;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let tail: String = seen
        .lines()
        .rev()
        .take(15)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "超时（{deadline:?}）：日志 {path:?} 未出现「{needle}」\n日志尾部:\n{tail}"
    ))
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
            RR3_BRAIN_TOPIC_TTS_PCM,
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
        assert!(
            sense.contains("publishes = [\"rr3-sense/audio/pcm\", \"rr3-sense/audio/vad_start\"]"),
            "{sense}"
        );
        let brain = &ms[2].1;
        assert!(brain.contains("type = \"brain\""), "{brain}");
        assert!(brain.contains("lang = \"rust\""), "{brain}");
        assert!(
            brain.contains("exposes = [\"rr3-brain/llm/completion\"]"),
            "{brain}"
        );
    }

    #[test]
    fn config_shapes() {
        // mock 档（缺省）：死地址 + mock 模型 + burst 短判停
        let cfg = rr3_brain_cfg_text(false, "http://127.0.0.1:9/v1");
        assert!(cfg.contains("max_utterance_ms = 1500"), "{cfg}");
        assert!(
            cfg.contains("base_url = \"http://127.0.0.1:9/v1\""),
            "{cfg}"
        );
        assert!(cfg.contains("llm_model = \"mock\""), "{cfg}");
    }

    #[test]
    fn real_cloud_config_carries_production_models_and_endpoint() {
        // 真值镜像 modules/rr3-brain/config.toml：cloud.base_url 是主机根（ASR/TTS
        // 自拼 /v1/audio/*）；模型行 base_url 是完整 API 根（LLM 只拼 /chat/completions）
        let cfg = rr3_brain_cfg_text(true, "https://api.siliconflow.cn");
        assert!(
            cfg.contains("base_url = \"https://api.siliconflow.cn\""),
            "{cfg}"
        );
        assert!(
            cfg.contains("base_url = \"https://api.siliconflow.cn/v1\""),
            "模型行补版本段: {cfg}"
        );
        // 已带路径的 API 根原样（GLM / Qwen 各写各的版本段，不按主机名猜）
        let glm = rr3_brain_cfg_text(true, "https://open.bigmodel.cn/api/paas/v4");
        assert!(
            glm.contains("base_url = \"https://open.bigmodel.cn/api/paas/v4\""),
            "{glm}"
        );
        // 生产模型名与 modules/rr3-brain/config.toml 同值（此处按值镜像，改名两侧同红）
        assert!(cfg.contains("asr_model = \"iic/SenseVoiceSmall\""), "{cfg}");
        assert!(
            cfg.contains("llm_model = \"deepseek-ai/DeepSeek-V3\""),
            "{cfg}"
        );
        assert!(
            cfg.contains("tts_model = \"FunAudioLLM/CosyVoice2-0.5B\""),
            "{cfg}"
        );
        assert!(cfg.contains("timeout_s = 20"), "{cfg}");
        // 0.0.6 模型连接抽象批：real 档必须携带 [[rr3-brain.model]] 连接行
        // （恰一 active、openai 格式、密钥走 env 引用——演练面沿用 RR3_CLOUD_API_KEY）
        assert!(cfg.contains("[[rr3-brain.model]]"), "{cfg}");
        assert!(cfg.contains("mode = \"api\""), "{cfg}");
        assert!(cfg.contains("api_format = \"openai\""), "{cfg}");
        assert!(
            cfg.contains("model_id = \"deepseek-ai/DeepSeek-V3\""),
            "{cfg}"
        );
        assert!(
            cfg.contains("api_key_ref = \"env:RR3_CLOUD_API_KEY\""),
            "{cfg}"
        );
        assert!(cfg.contains("active = true"), "{cfg}");
        // 人声 8s 判停（生产值）——burst 的 1.5s 兜底对真人太紧
        assert!(cfg.contains("max_utterance_ms = 8000"), "{cfg}");
        assert!(!cfg.contains("127.0.0.1:9"), "{cfg}");
    }
}
