//! 演练环境与装配单副本准备：二进制定位、工作目录、manifest 与 config 文本落盘。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::brain::rr3_brain_cfg_text;
use crate::drill::{
    rr3_manifest_ext, rr3_require_bins_ext, Rr3SimArgs, RR3_SIM_MODULE_A, RR3_SIM_MODULE_B,
    RR3_SIM_MODULE_BRAIN,
};

/// 全链第四个真模块：执行器（sim 舵机/继电器面）
pub const RR3_SIM_MODULE_ACT: &str = "rr3-act";
/// 观察者身份（注册后专收广播，不发布；演练寿命远小于其 watchdog 上界）
pub const RR3_CHAIN_OBS: &str = "rr3-obs";
/// 判向注入档（左档）：与批F2 判向 e2e 同值，ILD 反演 ±2° 容差同口径
pub const RR3_CHAIN_DOA_DEG: i32 = -60;
/// 词表左档角（brain intent.py：向左 → 70°，落 pan 轴行程 [70,110]）
pub const RR3_CHAIN_LEFT_ANGLE: i64 = 70;

/// 全链取证主题：判向（sense onset 负载扩展，批F2）
pub const RR3_SENSE_TOPIC_VAD: &str = "rr3-sense/audio/vad_start";
/// 全链取证主题：转向落角（act 状态摘要流）
pub const RR3_ACT_TOPIC_SERVO_STATE: &str = "rr3-act/servo/state";
/// TTS 下行流主题（MC-1）：brain 发布、interact 订阅
pub const RR3_BRAIN_TOPIC_TTS_PCM: &str = "rr3-brain/audio/tts_pcm";

/// 就绪上界：冷启动 + 首轮对话（burst 2s 周期 + 判停尾静音）也要够用
pub(crate) const RR3_CHAIN_READY_DEADLINE: Duration = Duration::from_secs(60);
/// 模块健康参数：与 --brain 演练同值（比生产紧、比测试宽）
const RR3_CHAIN_HB_MS: u64 = 200;
const RR3_CHAIN_WD_MS: u64 = 1000;
/// mock ASR 注入文本：词表左档（判向→转向的证据链起点）
pub(crate) const RR3_CHAIN_ASR_TEXT: &str = "向左看";
/// 演练结论（`--keep` 保留现场档）：不追加人工干预说明。
pub(crate) const RR3_CHAIN_OK_KEEP: &str = "全链 mock 闭环 OK：采集=sim burst 注入口 / 意图=mock 云 / 判向=sim 方位注入 / 转向=sim 舵机 / 表情=sim 状态机（无显示件）\n\
     本演练是采购前档（R9），不充当真件收口证据——真件档随采购后批验收。\n";
/// 演练结论（常规档）。
pub(crate) const RR3_CHAIN_OK: &str = "全链 mock 闭环 OK（无人工干预 ≥1 轮）：采集=sim burst 注入口 / 意图=mock 云（R4） / 判向=sim 方位注入 / 转向=sim 舵机 / 表情=sim 状态机（无显示件）\n\
     本演练是采购前档（R9），不充当真件收口证据——真件档随采购后批验收。\n";

/// 四份 wire manifest 文本（纯函数，tests 直测形状）：
/// sense（boot resident）/ interact / brain / act，身份与主题全部对齐仓内
/// module.toml 契约面（tests 有对账用例钉住）。
pub fn rr3_chain_manifests(
    sense_entry: &Path,
    interact_entry: &Path,
    act_entry: &Path,
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
                RR3_CHAIN_HB_MS,
                RR3_CHAIN_WD_MS,
                "always",
                &["rr3-sense/audio/pcm", RR3_SENSE_TOPIC_VAD],
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
                RR3_CHAIN_HB_MS,
                RR3_CHAIN_WD_MS,
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
                RR3_CHAIN_HB_MS,
                RR3_CHAIN_WD_MS,
                "never",
                &["rr3-brain/intent/result", RR3_BRAIN_TOPIC_TTS_PCM],
                // C14（ADR-0019 修订 2 ②）：asr/text retired 撤申报、net/link 撤申报
                //（不补实现）；订阅面对齐实然面 = pcm + vad_start（B-1 补齐）
                &["rr3-sense/audio/pcm", RR3_SENSE_TOPIC_VAD],
                &["rr3-brain/llm/completion"],
            ),
        ),
        (
            RR3_SIM_MODULE_ACT.to_string(),
            rr3_manifest_ext(
                RR3_SIM_MODULE_ACT,
                act_entry,
                "module",
                "rust",
                RR3_CHAIN_HB_MS,
                RR3_CHAIN_WD_MS,
                "never",
                &[
                    RR3_ACT_TOPIC_SERVO_STATE,
                    "rr3-act/relay/state",
                    "rr3-act/motor/reflex",
                ],
                &["rr3-brain/intent/result", "rr3-runtime/module/state"],
                &[
                    "rr3-act/servo/command",
                    "rr3-act/led/effect",
                    "rr3-act/relay/set",
                ],
            ),
        ),
    ]
}

/// 观察者 manifest：最小身份 + 两个取证主题订阅（不发布，无 exposes）。
/// `entry` 注册门禁要求绝对路径——观察者不 exec，指向任一真二进制即可。
pub fn rr3_chain_obs(entry_abs: &Path) -> String {
    rr3_manifest_ext(
        RR3_CHAIN_OBS,
        entry_abs,
        "module",
        "rust",
        3_600_000,
        3_600_001,
        "never",
        &[],
        &[RR3_SENSE_TOPIC_VAD, RR3_ACT_TOPIC_SERVO_STATE],
        &[],
    )
}

/// sense 双麦 config（与批F2 判向 e2e 的 DOA_CONFIG 同形状：channels=2，
/// VAD 阈值与仓内 config 同值；判向只认 ch_n>=2，单麦链路无方向语义）
pub fn rr3_chain_sense_cfg() -> String {
    "[rr3-sense]\nlog_level = \"info\"\n\n[rr3-sense.audio]\nsample_rate = 16000\nchannels = 2\nframe_ms = 20\n\n[rr3-sense.vad]\nrms_start = 1500\nsilence_tail_ms = 700\nmin_speech_ms = 200\n\n[rr3-sense.doa]\nbin_threshold_deg = 30\n"
        .to_string()
}

/// act 演练 config（与 e2e_act 夹具同形状，漂移当场由 main.rs 的形状解析红）。
/// 行程键不在 config（批N3 件五门 g 收敛：行程真值 = 执行轴档案经
/// RR3_ACT_AXIS_STATE 注入），本文本与仓内 modules/rr3-act/config.toml 同形。
pub fn rr3_chain_act_cfg() -> String {
    "[rr3-act]\nlog_level = \"info\"\n\n[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n\n[rr3-act.led]\nblink_period_ms = 500\neffects = [\"on\", \"off\", \"blink\"]\n\n[rr3-act.intent]\nopen_topics = [\"rr3-act/relay/set\", \"rr3-act/servo/command\"]\n\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n"
        .to_string()
}

/// interact 演练 config（I-11 起 interact 必读 config：不可点名集注入；与仓内
/// `modules/rr3-interact/config.toml` 同形状，缺键/在册外值由 main.rs 解析红）
pub fn rr3_chain_itact_cfg() -> String {
    "[rr3-interact]\nlog_level = \"info\"\n\n[rr3-interact.face]\nnon_pointable = [\"talk\"]\n"
        .to_string()
}

/// 演练二进制集（定位 + 存在性校验一次做完）。
pub(crate) struct Rr3ChainBins {
    pub(crate) rrd: PathBuf,
    pub(crate) rrctl: PathBuf,
    pub(crate) sense: PathBuf,
    pub(crate) interact: PathBuf,
    pub(crate) act: PathBuf,
    pub(crate) brain: PathBuf,
}

pub(crate) fn rr3_chain_bins(a: &Rr3SimArgs) -> Result<Rr3ChainBins, String> {
    let repo = crate::drill::rr3_repo_locate(a)?;
    let dir = crate::drill::rr3_bin_dir(&repo);
    let (d_rrd, _peer, d_rrctl) = crate::drill::rr3_default_bins(&repo);
    let bins = Rr3ChainBins {
        rrd: a.rrd.clone().unwrap_or(d_rrd),
        rrctl: a.rrctl.clone().unwrap_or(d_rrctl),
        sense: a.sense.clone().unwrap_or(dir.join("rr3-sense")),
        interact: a.interact.clone().unwrap_or(dir.join("rr3-interact")),
        act: a.act.clone().unwrap_or(dir.join("rr3-act")),
        brain: a.brain_bin.clone().unwrap_or(dir.join("rr3-brain")),
    };
    rr3_chain_bins_check(&bins)?;
    Ok(bins)
}

/// 六件产物存在性校验（缺件逐件点名各自构建命令）。
fn rr3_chain_bins_check(bins: &Rr3ChainBins) -> Result<(), String> {
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
            &bins.act,
            "cargo build -p rr3-act（在 modules/rr3-act/ 下）",
        ),
        (
            &bins.brain,
            "cargo build -p rr3-brain（在 modules/rr3-brain/ 下）",
        ),
    ])
}

/// 演练夹具：工作目录 + 套接字 + 四份 manifest + 五份文本（写完即用）。
pub(crate) struct Rr3ChainFix {
    pub(crate) workdir: PathBuf,
    pub(crate) socket: PathBuf,
    pub(crate) brain_cfg: PathBuf,
    pub(crate) sense_cfg: PathBuf,
    pub(crate) interact_cfg: PathBuf,
    pub(crate) act_cfg: PathBuf,
    pub(crate) obs_manifest: PathBuf,
    pub(crate) m_sense: PathBuf,
}

pub(crate) fn rr3_chain_fix(a: &Rr3SimArgs, bins: &Rr3ChainBins) -> Result<Rr3ChainFix, String> {
    let (workdir, socket) = rr3_chain_place(a)?;
    let obs_manifest = rr3_chain_texts(&workdir, &bins.act)?;
    Ok(Rr3ChainFix {
        brain_cfg: workdir.join("brain-config.toml"),
        sense_cfg: workdir.join("sense-config.toml"),
        interact_cfg: workdir.join("interact-config.toml"),
        act_cfg: workdir.join("act-config.toml"),
        obs_manifest,
        m_sense: workdir.join(format!("{RR3_SIM_MODULE_A}.toml")),
        workdir,
        socket,
    })
}

/// 四份 config 与观察者 manifest 落盘（返回观察者 manifest 路径）。
fn rr3_chain_texts(workdir: &Path, act: &Path) -> Result<PathBuf, String> {
    rr3_chain_cfgs(workdir)?;
    rr3_chain_obs_write(workdir, act)
}

/// 观察者 manifest 落盘（entry 指向 act 真二进制，只过注册门禁）。
fn rr3_chain_obs_write(workdir: &Path, act: &Path) -> Result<PathBuf, String> {
    let obs_manifest = workdir.join(format!("{RR3_CHAIN_OBS}.toml"));
    // 观察者 manifest 需要绝对 entry：指向 act 真二进制（不 exec，只过注册门禁）
    std::fs::write(&obs_manifest, rr3_chain_obs(act))
        .map_err(|e| format!("写 {obs_manifest:?} 失败: {e}"))?;
    Ok(obs_manifest)
}

/// 四份演练 config 落盘（写完即用）。
fn rr3_chain_cfgs(workdir: &Path) -> Result<(), String> {
    for (name, text) in [
        (
            "brain-config.toml",
            rr3_brain_cfg_text(false, "http://127.0.0.1:9/v1"),
        ),
        ("sense-config.toml", rr3_chain_sense_cfg()),
        ("interact-config.toml", rr3_chain_itact_cfg()),
        ("act-config.toml", rr3_chain_act_cfg()),
    ] {
        let p = workdir.join(name);
        std::fs::write(&p, text).map_err(|e| format!("写 {p:?} 失败: {e}"))?;
    }
    Ok(())
}

/// 演练工作目录与套接字：`--socket` 显式给定则同目录，否则临时目录按进程号。
fn rr3_chain_place(a: &Rr3SimArgs) -> Result<(PathBuf, PathBuf), String> {
    let workdir = rr3_chain_dir(a);
    std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
    let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
    let _ = std::fs::remove_file(&socket);
    Ok((workdir, socket))
}

/// 演练工作目录：`--socket` 显式给定取其父目录，否则临时目录按进程号。
fn rr3_chain_dir(a: &Rr3SimArgs) -> PathBuf {
    match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!("rr3-sim-chain-{}", std::process::id())),
    }
}
