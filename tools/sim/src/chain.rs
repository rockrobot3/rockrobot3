//! `--chain` 演练（0.0.6 批F4 sim 档，§5.2 判据 10）：六真进程全链 mock 闭环。
//!
//! rrd（boot spawn rr3-sense）+ 外部 rr3-brain + rr3-interact + rr3-act + 演练自带
//! 的**总线观察者**（rrd 连接五真进程 + 演练进程自己持一条 UDS 观察连接），无人工
//! 干预跑通"采集→意图→判向→转向→表情"一轮以上：
//!
//! - **采集**：sense 以双麦 config（channels=2，R14 双麦期）+ `RR3_HAL_SIM_WAVE=burst`
//!   起 sim 源（偶秒有声/奇秒静音，onset/offset 周期性成对），pcm 下混 mono 入总线；
//! - **判向**：`RR3_HAL_SIM_DOA_DEG=-60` 注入（与批F2 判向 e2e 同值），onset 携
//!   `doa{deg,bin}` 上 `rr3-sense/audio/vad_start`——观察者现场收帧取证；
//! - **意图**：brain 走 mock 云（`RR3_BRAIN_CLOUD=mock`）+ mock ASR 文本"向左看"
//!   （`RR3_BRAIN_MOCK_ASR_TEXT` 批F2 注入键），`对话轮完成` 行携 `actions=1`；
//! - **转向**：brain 词表把左档意图映射成 `rr3-act/servo/command`(70°)（策略层
//!   唯一归属，0.0.3 Q1），act 的 sim 舵机落角并发 `rr3-act/servo/state`——
//!   观察者同时取证落角；
//! - **表情**：TTS 播放期间 interact 表情机自动挂起 talk（批F3 语义，不依赖
//!   调用方自觉），`tts:` 日志行携「talk 挂起」留痕；sim 档无显示件。
//!
//! 观察者与 rrd/tests/support 的 `rr3_connect_observer` 同一机制（注册最小身份
//! 专收广播），但演练是脚本编排不依赖测试夹具——裸 UDS + rr3.register，订阅
//! vad_start/servo/state 两个取证主题。观察者在**外部模块起跑前**注册（先于
//! 意图动作），servo/state 是一次性广播，晚注册会漏收。
//!
//! 云端走 mock（R4 口径）；报告按 R9 真/mock 矩阵逐项标注交付档位——本演练是
//! **采购前档**，不充当真件收口证据。仅 Unix 参与编译（依赖 UDS 与真进程）。

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rr_bus::RR3_TASK_RRD;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType,
};
use serde_json::{json, Value};

use crate::assemble::rr3_ctl_status_rows as rr3_ctl_rows;
use crate::brain::{
    rr3_brain_cfg_text, rr3_ctl_denied_zero, rr3_spawn_logged, rr3_teardown_procs,
    wait_log_contains, wait_socket_exists,
};
use crate::drill::{
    rr3_manifest_ext, rr3_require_bins_ext, rr3_section, rr3_wait_modules, Rr3SimArgs,
    RR3_SIM_MODULE_A, RR3_SIM_MODULE_B, RR3_SIM_MODULE_BRAIN, RR3_SIM_PANEL_ADDR,
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
const RR3_CHAIN_READY_DEADLINE: Duration = Duration::from_secs(60);
/// 模块健康参数：与 --brain 演练同值（比生产紧、比测试宽）
const RR3_CHAIN_HB_MS: u64 = 200;
const RR3_CHAIN_WD_MS: u64 = 1000;
/// mock ASR 注入文本：词表左档（判向→转向的证据链起点）
const RR3_CHAIN_ASR_TEXT: &str = "向左看";
/// 演练结论（`--keep` 保留现场档）：不追加人工干预说明。
const RR3_CHAIN_OK_KEEP: &str = "全链 mock 闭环 OK：采集=sim burst 注入口 / 意图=mock 云 / 判向=sim 方位注入 / 转向=sim 舵机 / 表情=sim 状态机（无显示件）\n\
     本演练是采购前档（R9），不充当真件收口证据——真件档随采购后批验收。\n";
/// 演练结论（常规档）。
const RR3_CHAIN_OK: &str = "全链 mock 闭环 OK（无人工干预 ≥1 轮）：采集=sim burst 注入口 / 意图=mock 云（R4） / 判向=sim 方位注入 / 转向=sim 舵机 / 表情=sim 状态机（无显示件）\n\
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

/// 观察者取证袋：判向档（deg/bin）与转向落角。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Rr3ChainObs {
    pub doa_deg: Option<i64>,
    pub doa_bin: Option<String>,
    pub servo_angle: Option<i64>,
}

impl Rr3ChainObs {
    /// 两路取证齐了（收集循环的停机条件）
    pub fn rr3_obs_ready(&self) -> bool {
        self.doa_deg.is_some() && self.servo_angle.is_some()
    }
}

/// 事件体喂进取证袋（纯函数，tests 直测）：vad_start 携 doa 取判向档
/// （offset 不带 doa，批F2 负载纪律，喂了也取不到），servo/state 取落角。
pub fn rr3_obs_feed(obs: &mut Rr3ChainObs, body: &Value) {
    match body.get("topic").and_then(Value::as_str) {
        Some(RR3_SENSE_TOPIC_VAD) => rr3_obs_doa(obs, body),
        Some(RR3_ACT_TOPIC_SERVO_STATE) => {
            obs.servo_angle = body.get("angle_deg").and_then(Value::as_i64);
        }
        _ => {}
    }
}

/// vad_start 的 doa 档入袋（offset 不带 doa，喂了也取不到）。
fn rr3_obs_doa(obs: &mut Rr3ChainObs, body: &Value) {
    if let Some(d) = body.get("doa") {
        obs.doa_deg = d.get("deg").and_then(Value::as_i64);
        obs.doa_bin = d.get("bin").and_then(Value::as_str).map(str::to_string);
    }
}

/// 取证判定（纯函数）：判向档落注入值的 ±2° 容差且分左档；落角 = 词表左档 70°。
/// 收集循环只等 presence，档位对不对在这里裁——证据与判定分离，tests 可直测。
pub fn rr3_chain_judge(obs: &Rr3ChainObs) -> Result<String, String> {
    let deg = rr3_chain_doa_ok(obs)?;
    let angle = rr3_chain_servo_ok(obs)?;
    Ok(format!(
        "doa={deg}°/left（注入 {RR3_CHAIN_DOA_DEG}° ±2° 容差内）→ servo={angle}°（词表左档）"
    ))
}

/// 判向档判定：落注入值的 ±2° 容差且分左档。
fn rr3_chain_doa_ok(obs: &Rr3ChainObs) -> Result<i64, String> {
    let deg = obs
        .doa_deg
        .ok_or("判向档缺失：onset 没携 doa（注入没生效或链路没走通）")?;
    if !(-62..=-58).contains(&deg) {
        return Err(format!(
            "注入 {RR3_CHAIN_DOA_DEG}° 的 ILD 反演必须落 ±2° 容差: {deg}°"
        ));
    }
    rr3_chain_left(obs.doa_bin.as_deref())?;
    Ok(deg)
}

/// 分档判定：必须落左档（|deg| > 30 的注入档）。
fn rr3_chain_left(bin: Option<&str>) -> Result<(), String> {
    if bin != Some("left") {
        return Err(format!("|deg| > 30 必须落左档: {bin:?}"));
    }
    Ok(())
}

/// 转向落角判定：必须是词表左档角。
fn rr3_chain_servo_ok(obs: &Rr3ChainObs) -> Result<i64, String> {
    let angle = obs
        .servo_angle
        .ok_or("转向落角缺失：intent 动作没落到 servo/state（词表映射或 act 链路断）")?;
    if angle != RR3_CHAIN_LEFT_ANGLE {
        return Err(format!("词表左档必须落 {RR3_CHAIN_LEFT_ANGLE}°: {angle}°"));
    }
    Ok(angle)
}

/// 裸 UDS 观察者：rr3.register 最小身份（与 e2e support 的 rr3_connect_observer
/// 同一机制）。应答 id=1 的 error 面原样带回（注册被拒要当场可辨，不等取证超时）。
fn rr3_obs_register(stream: &mut UnixStream, manifest_text: &str) -> Result<(), String> {
    // 读超时先行：注册应答不来时按 deadline 收口，不无限阻塞演练
    rr3_obs_arm(stream, Duration::from_secs(2))?;
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "rr3.register",
        "params": {"name": RR3_CHAIN_OBS, "manifest": manifest_text}
    });
    rr3_frame_write(stream, &rr3_obs_frame(Rr3PayloadType::Request, &body))
        .map_err(|e| format!("写注册帧失败: {e:?}"))?;
    rr3_obs_wait(stream)
}

/// 等注册应答：2s 一拍读，10s deadline 统一收口。
fn rr3_obs_wait(stream: &mut UnixStream) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() <= deadline {
        if rr3_obs_one(stream)? {
            return Ok(());
        }
    }
    Err("注册应答超时（10s）：rrd 没应答 rr3.register".to_string())
}

/// 一拍：读到注册成功应答回 `true`；读超时/不入账回 `false` 继续等。
fn rr3_obs_one(stream: &mut UnixStream) -> Result<bool, String> {
    match rr3_frame_read(&mut *stream) {
        Ok(frame) => rr3_obs_reply(&frame.payload).map(|hit| hit.is_some()),
        // 读超时（2s 一拍）：到 deadline 统一收口
        Err(_) => Ok(false),
    }
}

/// 单帧归类：`Ok(Some(()))` = 注册成功，`Ok(None)` = 不入账继续等；
/// `Err` = JSON 非形或注册被拒（原样带回原因）。
fn rr3_obs_reply(payload: &[u8]) -> Result<Option<()>, String> {
    let Some(v) = rr3_obs_reply_body(payload)? else {
        return Ok(None);
    };
    if v.get("error").is_some_and(Value::is_object) {
        return Err(format!("观察者注册被拒: {}", v["error"]));
    }
    Ok(Some(()))
}

/// 注册应答体：非 Response、id≠1、非 JSON 都不入账（`Ok(None)`）；非 JSON 是错。
fn rr3_obs_reply_body(payload: &[u8]) -> Result<Option<Value>, String> {
    let Ok((ty, raw)) = rr3_body_split(payload) else {
        return Ok(None);
    };
    rr3_obs_response(ty, &raw)
}

/// Response 帧体：非 Response/id≠1 不入账，非 JSON 是错。
fn rr3_obs_response(ty: Rr3PayloadType, raw: &[u8]) -> Result<Option<Value>, String> {
    if ty != Rr3PayloadType::Response {
        return Ok(None); // 注册期间到达的广播/通知不入账
    }
    let v: Value = serde_json::from_slice(raw).map_err(|e| format!("注册应答非 JSON: {e}"))?;
    Ok(rr3_obs_id_ok(&v).then_some(v))
}

/// 注册应答的 id 门（id≠1 不入账）。
fn rr3_obs_id_ok(v: &Value) -> bool {
    v.get("id").and_then(Value::as_u64) == Some(1)
}

/// 取证收集：读到两路证据齐或超时。读超时/暂无帧按"这拍没帧"继续，
/// 到 deadline 收口——缺什么在错误里点名什么。
fn rr3_obs_collect(stream: &mut UnixStream, deadline: Duration) -> Result<Rr3ChainObs, String> {
    rr3_obs_arm(stream, Duration::from_millis(250))?;
    rr3_obs_gather(stream, deadline)
}

/// 读超时设置（取证 250ms 一拍；注册应答 2s）。
fn rr3_obs_arm(stream: &mut UnixStream, timeout: Duration) -> Result<(), String> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("设读超时失败: {e}"))
}

/// 收集循环：两路证据齐或到 deadline（缺什么在错误里点名什么）。
fn rr3_obs_gather(stream: &mut UnixStream, deadline: Duration) -> Result<Rr3ChainObs, String> {
    let mut obs = Rr3ChainObs::default();
    let t0 = Instant::now();
    while !obs.rr3_obs_ready() {
        rr3_obs_deadline(t0, deadline, &obs)?;
        rr3_obs_step(stream, &mut obs);
    }
    Ok(obs)
}

/// 取证超时门（缺什么在错误里点名什么）。
fn rr3_obs_deadline(t0: Instant, deadline: Duration, obs: &Rr3ChainObs) -> Result<(), String> {
    if t0.elapsed() >= deadline {
        return Err(format!(
            "超时（{deadline:?}）：全链取证未齐（doa={:?} servo={:?}）——判向注入或意图-转向链没走通",
            obs.doa_deg, obs.servo_angle
        ));
    }
    Ok(())
}

/// 一拍取证：读超时/非事件/非 JSON 一律按"这拍没帧"跳过。
fn rr3_obs_step(stream: &mut UnixStream, obs: &mut Rr3ChainObs) {
    // 读侧 Err（读超时 250ms 一拍）不进分支：到 deadline 统一收口
    let Ok(frame) = rr3_frame_read(&mut *stream) else {
        return;
    };
    let Ok((ty, raw)) = rr3_body_split(&frame.payload) else {
        return;
    };
    if ty != Rr3PayloadType::Event {
        return;
    }
    if let Ok(v) = serde_json::from_slice::<Value>(&raw) {
        rr3_obs_feed(obs, &v);
    }
}

/// 观察者上行帧（src=0 注册前约定，dst=rrd；与 sim_peer 的本地 rr3_send 同形）
fn rr3_obs_frame(ty: Rr3PayloadType, body: &Value) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body.to_string().as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task: 0,
            dst_task: RR3_TASK_RRD,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

pub fn rr3_chain_drill(a: &Rr3SimArgs) -> Result<String, String> {
    let bins = rr3_chain_bins(a)?;
    let fix = rr3_chain_fix(a, &bins)?;
    let ctx = Rr3ChainCtx {
        bins: &bins,
        fix: &fix,
    };
    let mut children: Vec<(String, Child)> = Vec::new();
    let mut report = String::new();
    let outcome = rr3_chain_run(&ctx, &mut children, &mut report);
    rr3_chain_finish(a, &fix, children, outcome, &mut report)
}

/// 收尾：杀全部子进程；`--keep` 保留现场并给出手动收尾指引。
fn rr3_chain_finish(
    a: &Rr3SimArgs,
    fix: &Rr3ChainFix,
    children: Vec<(String, Child)>,
    outcome: Result<(), String>,
    report: &mut String,
) -> Result<String, String> {
    if a.keep {
        rr3_chain_keep(fix);
        return rr3_chain_done(outcome, report, fix, true, RR3_CHAIN_OK_KEEP);
    }
    rr3_teardown_procs(children, &fix.socket);
    rr3_chain_done(outcome, report, fix, false, RR3_CHAIN_OK)
}

/// `--keep`：打印现场保留与手动收尾指引（不杀子进程）。
fn rr3_chain_keep(fix: &Rr3ChainFix) {
    println!(
        "rr3-sim --chain: --keep 保留现场：socket={} workdir={}",
        fix.socket.display(),
        fix.workdir.display()
    );
    println!(
        "rr3-sim --chain: 手动收尾：kill 上述进程；rm -rf {:?}",
        fix.workdir
    );
}

/// 收尾结论：成功追加演练结论；失败带回现场位置（`--keep` 不追加日志路径）。
fn rr3_chain_done(
    outcome: Result<(), String>,
    report: &mut String,
    fix: &Rr3ChainFix,
    keep: bool,
    ok_text: &str,
) -> Result<String, String> {
    match outcome {
        Ok(()) => Ok(rr3_chain_ok_report(report, ok_text)),
        Err(e) => Err(rr3_chain_err(e, fix, keep)),
    }
}

/// 成功结论：分节标题 + 演练结论原文。
fn rr3_chain_ok_report(report: &mut String, ok_text: &str) -> String {
    report.push_str(rr3_section("演练结论（R9 档位逐项标注）").as_str());
    report.push_str(ok_text);
    std::mem::take(report)
}

/// 失败结论：`--keep` 原样带回，否则附现场日志位置。
fn rr3_chain_err(e: String, fix: &Rr3ChainFix, keep: bool) -> String {
    if keep {
        e
    } else {
        format!("{e}\n（现场日志在 {:?}）", fix.workdir)
    }
}

/// 演练二进制集（定位 + 存在性校验一次做完）。
struct Rr3ChainBins {
    rrd: PathBuf,
    rrctl: PathBuf,
    sense: PathBuf,
    interact: PathBuf,
    act: PathBuf,
    brain: PathBuf,
}

fn rr3_chain_bins(a: &Rr3SimArgs) -> Result<Rr3ChainBins, String> {
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
struct Rr3ChainFix {
    workdir: PathBuf,
    socket: PathBuf,
    brain_cfg: PathBuf,
    sense_cfg: PathBuf,
    interact_cfg: PathBuf,
    act_cfg: PathBuf,
    obs_manifest: PathBuf,
    m_sense: PathBuf,
}

fn rr3_chain_fix(a: &Rr3SimArgs, bins: &Rr3ChainBins) -> Result<Rr3ChainFix, String> {
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

/// 演练上下文（一次演练内的不变量；只读借用，便于按阶段拆函数）。
struct Rr3ChainCtx<'a> {
    bins: &'a Rr3ChainBins,
    fix: &'a Rr3ChainFix,
}

/// 演练主体：写 manifest → 起 rrd → 注册观察者 → 起外部三模块 → 取证 → 巡检。
fn rr3_chain_run(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
    report: &mut String,
) -> Result<(), String> {
    let mut obs_stream = rr3_chain_boot(ctx, children, report)?;
    rr3_chain_up_exts(ctx, children, report)?;
    rr3_chain_evidence(ctx, &mut obs_stream, report)?;
    rr3_chain_status(ctx, report)
}

/// 起跑段：写 manifest → 起 rrd → 等 bind → 观察者注册（先于外部模块）。
fn rr3_chain_boot(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
    report: &mut String,
) -> Result<UnixStream, String> {
    let fix = ctx.fix;
    rr3_chain_mans(ctx)?;
    rr3_chain_up_rrd(ctx, children)?;
    // 等 bind，观察者**先于外部模块**注册（servo/state 一次性广播，晚注册漏收）
    wait_socket_exists(&fix.socket, RR3_CHAIN_READY_DEADLINE)?;
    rr3_chain_obs_up(fix, report)
}

/// 四份 wire manifest 落盘（写完即用）。
fn rr3_chain_mans(ctx: &Rr3ChainCtx) -> Result<(), String> {
    let fix = ctx.fix;
    for (name, text) in rr3_chain_manifests(
        &ctx.bins.sense,
        &ctx.bins.interact,
        &ctx.bins.act,
        &ctx.bins.brain,
    ) {
        let p = fix.workdir.join(format!("{name}.toml"));
        std::fs::write(&p, text).map_err(|e| format!("写 {p:?} 失败: {e}"))?;
    }
    Ok(())
}

/// 外部模块起跑：三外部进程 → 有界等四模块 running。
fn rr3_chain_up_exts(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
    report: &mut String,
) -> Result<(), String> {
    rr3_chain_up_ext(ctx, children)?;
    rr3_chain_mods_up(ctx, report)
}

/// 起 rrd：boot spawn rr3-sense（双麦 burst 源 + 判向注入随 rrd 环境继承）。
fn rr3_chain_up_rrd(ctx: &Rr3ChainCtx, children: &mut Vec<(String, Child)>) -> Result<(), String> {
    let fix = ctx.fix;
    let rrd_log = fix.workdir.join("rrd.log");
    let rrd_child = Command::new(&ctx.bins.rrd)
        .arg("--socket")
        .arg(&fix.socket)
        .arg("--manifest")
        .arg(&fix.m_sense)
        .arg("--boot-role")
        .arg("resident")
        .args(["--pre-register-timeout-ms", "3000"])
        .env("RR3_HAL_BACKEND", "sim")
        .env("RR3_HAL_SIM_WAVE", "burst")
        .env("RR3_HAL_SIM_DOA_DEG", RR3_CHAIN_DOA_DEG.to_string())
        .env("RR3_SENSE_CONFIG", &fix.sense_cfg)
        .stdout(Stdio::from(
            std::fs::File::create(&rrd_log).map_err(|e| format!("建 {rrd_log:?} 失败: {e}"))?,
        ))
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("起 rrd 失败: {e}"))?;
    children.push(("rrd".into(), rrd_child));
    println!(
        "rr3-sim --chain: rrd 已起，socket={}（sense 双麦 burst + DOA 注入 {RR3_CHAIN_DOA_DEG}°）",
        fix.socket.display()
    );
    Ok(())
}

/// 观察者连总线并注册（最小身份，专收两个取证主题）。
fn rr3_chain_obs_up(fix: &Rr3ChainFix, report: &mut String) -> Result<UnixStream, String> {
    let mut obs_stream = UnixStream::connect(&fix.socket)
        .map_err(|e| format!("观察者连 {:?} 失败: {e}", fix.socket))?;
    let obs_manifest_text = std::fs::read_to_string(&fix.obs_manifest)
        .map_err(|e| format!("读 {:?} 失败: {e}", fix.obs_manifest))?;
    rr3_obs_register(&mut obs_stream, &obs_manifest_text)?;
    report.push_str(&format!(
        "总线观察者已注册（{RR3_CHAIN_OBS}，订阅 {RR3_SENSE_TOPIC_VAD} / {RR3_ACT_TOPIC_SERVO_STATE}）\n"
    ));
    Ok(obs_stream)
}

/// 外部起 interact / brain / act（三真模块，各自日志进 workdir，复用共用起法）。
fn rr3_chain_up_ext(ctx: &Rr3ChainCtx, children: &mut Vec<(String, Child)>) -> Result<(), String> {
    let fix = ctx.fix;
    for (name, mut cmd) in [
        {
            let mut c = Command::new(&ctx.bins.interact);
            c.env("RR3_HAL_BACKEND", "sim")
                .env("RR3_PANEL_ADDR", RR3_SIM_PANEL_ADDR)
                .env("RR3_INTERACT_CONFIG", &fix.interact_cfg);
            (RR3_SIM_MODULE_B, c)
        },
        {
            let mut c = Command::new(&ctx.bins.brain);
            c.env("RR3_BRAIN_CLOUD", "mock")
                .env("RR3_BRAIN_MOCK_ASR_TEXT", RR3_CHAIN_ASR_TEXT)
                .env("RR3_BRAIN_CONFIG", &fix.brain_cfg);
            (RR3_SIM_MODULE_BRAIN, c)
        },
        {
            let mut c = Command::new(&ctx.bins.act);
            c.env("RR3_HAL_BACKEND", "sim")
                .env("RR3_ACT_CONFIG", &fix.act_cfg)
                // 执行轴状态（批N3 件三建、件五五键）：chain 直起路径与
                // rrd --manifest 并存，模块 env 由 chain 直设——sim 档已
                // 标定 + 挡块在册，行程/中位同档案样本值，Auto 放行
                .env(
                    "RR3_ACT_AXIS_STATE",
                    "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
                );
            (RR3_SIM_MODULE_ACT, c)
        },
    ] {
        let log = fix.workdir.join(format!("{name}.log"));
        cmd.env("RR3_BUS_PATH", &fix.socket)
            .env("RR3_MANIFEST", fix.workdir.join(format!("{name}.toml")));
        let child = rr3_spawn_logged(cmd, &log, name)?;
        children.push((name.to_string(), child));
    }
    Ok(())
}

/// 有界等四模块 running。
fn rr3_chain_mods_up(ctx: &Rr3ChainCtx, report: &mut String) -> Result<(), String> {
    rr3_wait_modules(
        || rr3_ctl_rows(&ctx.bins.rrctl, &ctx.fix.socket),
        &[
            (RR3_SIM_MODULE_A, "running"),
            (RR3_SIM_MODULE_B, "running"),
            (RR3_SIM_MODULE_BRAIN, "running"),
            (RR3_SIM_MODULE_ACT, "running"),
        ],
        RR3_CHAIN_READY_DEADLINE,
    )?;
    report.push_str(rr3_section("四模块 running（sense/interact/brain/act）").as_str());
    Ok(())
}

/// 观察者取证（判向 + 转向）与三段进程内日志留痕。
fn rr3_chain_evidence(
    ctx: &Rr3ChainCtx,
    obs_stream: &mut UnixStream,
    report: &mut String,
) -> Result<(), String> {
    let obs = rr3_obs_collect(obs_stream, RR3_CHAIN_READY_DEADLINE)?;
    let verdict = rr3_chain_judge(&obs)?;
    report.push_str(rr3_section("总线取证（判向 → 转向）").as_str());
    report.push_str(&format!("{verdict}\n"));
    rr3_chain_logs(ctx, report)
}

/// 三段进程内日志留痕（brain 动作数 / interact 挂起 / act 落角）。
fn rr3_chain_logs(ctx: &Rr3ChainCtx, report: &mut String) -> Result<(), String> {
    report.push_str(rr3_section("全链留痕（进程内日志）").as_str());
    let turn = rr3_chain_turn(ctx)?;
    report.push_str(&format!("brain: {turn}\n"));
    let tts = rr3_chain_tts(ctx)?;
    report.push_str(&format!(
        "interact: {tts}（表情机 talk 自动挂起，sim 档无显示件）\n"
    ));
    let act_log = ctx.fix.workdir.join(format!("{RR3_SIM_MODULE_ACT}.log"));
    let intent = wait_log_contains(&act_log, "servo angle=", RR3_CHAIN_READY_DEADLINE)?;
    report.push_str(&format!("act: {intent}\n"));
    Ok(())
}

/// brain 轮次行：动作数必须是 1（词表左档应产一条 servo 动作）。
fn rr3_chain_turn(ctx: &Rr3ChainCtx) -> Result<String, String> {
    let brain_log = ctx.fix.workdir.join(format!("{RR3_SIM_MODULE_BRAIN}.log"));
    let turn = wait_log_contains(
        &brain_log,
        crate::brain::RR3_BRAIN_TURN_MARK,
        RR3_CHAIN_READY_DEADLINE,
    )?;
    if !turn.contains("actions=1") {
        return Err(format!(
            "brain 轮次行的动作数不是 1（词表左档应产一条 servo 动作）: {turn}"
        ));
    }
    Ok(turn)
}

/// interact 播放行：必须携「talk 挂起」（表情机没接管即红）。
fn rr3_chain_tts(ctx: &Rr3ChainCtx) -> Result<String, String> {
    let interact_log = ctx.fix.workdir.join(format!("{RR3_SIM_MODULE_B}.log"));
    let tts = wait_log_contains(&interact_log, "tts:", RR3_CHAIN_READY_DEADLINE)?;
    if !tts.contains("talk 挂起") {
        return Err(format!(
            "interact 播放行没携 talk 挂起（表情机没接管）: {tts}"
        ));
    }
    Ok(tts)
}

/// `rr3-ctl status` 巡检：denied_publish = 0 是"事件真被接受"的线上证据。
fn rr3_chain_status(ctx: &Rr3ChainCtx, report: &mut String) -> Result<(), String> {
    let status_out = rr3_ctl_denied_zero(
        &ctx.bins.rrctl,
        &ctx.fix.socket,
        "denied_publish ≠ 0：链上发布被 rrd 拒了",
    )?;
    report.push_str(rr3_section("rr3-ctl status 巡检").as_str());
    report.push_str(&status_out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::drill::rr3_sim_args_parse;

    #[test]
    fn chain_manifests_have_absolute_entries_and_contract_shapes() {
        let ms = rr3_chain_manifests(
            Path::new("/abs/rr3-sense"),
            Path::new("/abs/rr3-interact"),
            Path::new("/abs/rr3-act"),
            Path::new("/abs/rr3-brain"),
        );
        assert_eq!(ms.len(), 4);
        let (names, texts): (Vec<&str>, Vec<&str>) =
            ms.iter().map(|(n, t)| (n.as_str(), t.as_str())).unzip();
        assert_eq!(names, ["rr3-sense", "rr3-interact", "rr3-brain", "rr3-act"]);
        // entry 全部绝对（注册门禁）；boot 的 sense 是 resident 常驻
        for t in &texts {
            assert!(t.contains("entry = \"/abs/"), "{t}");
        }
        assert!(
            texts[0].contains("restart_policy = \"always\""),
            "{}",
            texts[0]
        );
        assert!(texts[3].contains("type = \"module\""), "{}", texts[3]);
        assert!(texts[2].contains("type = \"brain\""), "{}", texts[2]);
        assert!(texts[2].contains("lang = \"rust\""), "{}", texts[2]);
        // act 的三面契约面与仓内 module.toml 同源
        assert!(
            texts[3].contains(
                "publishes = [\"rr3-act/servo/state\", \"rr3-act/relay/state\", \"rr3-act/motor/reflex\"]"
            ),
            "{}",
            texts[3]
        );
        assert!(
            texts[3].contains("exposes = [\"rr3-act/servo/command\", \"rr3-act/led/effect\", \"rr3-act/relay/set\"]"),
            "{}",
            texts[3]
        );
    }

    #[test]
    fn obs_manifest_subscribes_both_evidence_topics() {
        let t = rr3_chain_obs(Path::new("/abs/rr3-act"));
        assert!(t.contains("name = \"rr3-obs\""), "{t}");
        assert!(
            t.contains(&format!(
                "subscribes = [\"{RR3_SENSE_TOPIC_VAD}\", \"{RR3_ACT_TOPIC_SERVO_STATE}\"]"
            )),
            "{t}"
        );
        assert!(t.contains("publishes = []"), "{t}");
    }

    #[test]
    fn configs_carry_stereo_sense_and_act_servo_shape() {
        let sense = rr3_chain_sense_cfg();
        assert!(sense.contains("channels = 2"), "{sense}");
        assert!(sense.contains("rms_start = 1500"), "{sense}");
        assert!(sense.contains("bin_threshold_deg = 30"), "{sense}");
        let act = rr3_chain_act_cfg();
        // 行程键已撤（批N3 件五门 g），断言改锚存活键——servo 节时限仍在
        assert!(act.contains("move_timeout_ms = 800"), "{act}");
        assert!(act.contains("state_min_interval_ms = 50"), "{act}");
        assert!(!act.contains("limit_deg"), "行程键不得回流 config: {act}");
        let interact = rr3_chain_itact_cfg();
        assert!(
            interact.contains("non_pointable = [\"talk\"]"),
            "{interact}"
        );
    }

    #[test]
    fn obs_feed_takes_doa_on_vad_and_angle_on_servo_only() {
        let mut obs = Rr3ChainObs::default();
        // offset 不带 doa：喂了也取不到（批F2 负载纪律）
        rr3_obs_feed(
            &mut obs,
            &json!({"topic": RR3_SENSE_TOPIC_VAD, "edge": "offset"}),
        );
        assert_eq!(obs, Rr3ChainObs::default());
        // onset 携 doa：判向档入袋
        rr3_obs_feed(
            &mut obs,
            &json!({"topic": RR3_SENSE_TOPIC_VAD, "edge": "onset",
                    "doa": {"deg": -60, "bin": "left"}}),
        );
        assert_eq!(obs.doa_deg, Some(-60));
        assert_eq!(obs.doa_bin.as_deref(), Some("left"));
        assert!(!obs.rr3_obs_ready(), "还缺转向落角");
        // servo/state：落角入袋；其余主题无视
        rr3_obs_feed(&mut obs, &json!({"topic": "rr3-sense/audio/pcm"}));
        rr3_obs_feed(
            &mut obs,
            &json!({"topic": RR3_ACT_TOPIC_SERVO_STATE, "seq": 2, "angle_deg": 70}),
        );
        assert_eq!(obs.servo_angle, Some(70));
        assert!(obs.rr3_obs_ready());
    }

    #[test]
    fn chain_judge_accepts_left_turn_and_rejects_drift() {
        let ok = Rr3ChainObs {
            doa_deg: Some(-59),
            doa_bin: Some("left".into()),
            servo_angle: Some(70),
        };
        assert!(rr3_chain_judge(&ok).is_ok());
        // 反演出容差
        let drift = Rr3ChainObs {
            doa_deg: Some(-70),
            ..ok.clone()
        };
        let e = rr3_chain_judge(&drift).unwrap_err();
        assert!(e.contains("±2° 容差"), "{e}");
        // 分档错
        let bin = Rr3ChainObs {
            doa_bin: Some("right".into()),
            ..ok.clone()
        };
        assert!(rr3_chain_judge(&bin).unwrap_err().contains("左档"));
        // 落角不是词表左档
        let angle = Rr3ChainObs {
            servo_angle: Some(90),
            ..ok.clone()
        };
        assert!(rr3_chain_judge(&angle).unwrap_err().contains("70°"));
        // 证据缺一面
        let half = Rr3ChainObs {
            doa_deg: None,
            ..ok
        };
        assert!(rr3_chain_judge(&half).unwrap_err().contains("判向档缺失"));
    }

    #[test]
    fn chain_args_parse_and_mutual_exclusion() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let a = rr3_sim_args_parse(&v(&["--chain"])).unwrap();
        assert!(a.chain && !a.brain && a.profile.is_none());
        let a = rr3_sim_args_parse(&v(&["--chain", "--act", "/abs/rr3-act"])).unwrap();
        assert_eq!(a.act.unwrap(), PathBuf::from("/abs/rr3-act"));
        // 三面互斥：chain 与 brain/profile 同给即拒
        for mixed in [
            &["--chain", "--brain"][..],
            &["--chain", "--profile", "/p"][..],
        ] {
            let e = rr3_sim_args_parse(&v(mixed)).unwrap_err();
            assert!(e.contains("互斥"), "{e}");
        }
        let e = rr3_sim_args_parse(&v(&["--act", "--chain"])).unwrap_err();
        assert!(e.contains("疑似开关"), "{e}");
    }

    #[test]
    fn chain_identities_exist_in_the_real_contract() {
        // 钉住"演练用的名字/主题必须真实在契约里"——契约改名时这里红，不是演练跑到一半红
        let contract = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/topics.toml");
        let text = std::fs::read_to_string(&contract)
            .unwrap_or_else(|e| panic!("读 {}: {e}", contract.display()));
        for needle in [
            RR3_SIM_MODULE_ACT,
            RR3_SENSE_TOPIC_VAD,
            RR3_ACT_TOPIC_SERVO_STATE,
            "rr3-act/servo/command",
            "rr3-brain/intent/result",
            RR3_BRAIN_TOPIC_TTS_PCM,
            "rr3-interact/face/expression",
        ] {
            assert!(text.contains(needle), "契约缺 {needle}");
        }
    }
}
