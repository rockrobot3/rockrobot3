//! 音频链主动用例的阶段体（自 `e2e_audio_chain.rs` 按阶段抽出，判据不变）。
//!
//! 主用例只保留顺序编排；每个阶段助手独立判读一段线上事实，失败点按阶段点名。

use serde_json::json;

use crate::support::*;

use super::{rr3_module_manifest, DOA_CONFIG};

/// 夹具：两枚真模块 manifest（pid+tag 唯一 /tmp 路径）+ 仓内缺省 config 路径。
/// sense/interact 的 config 必读；夹具 manifest 平铺 /tmp，故 env 显式指向仓内
/// 缺省档（单麦形状/不可点名集恰是本链被测形状，且与随版 config 零漂移）。
pub(super) fn stage(
    sense_bin: &std::path::Path,
    interact_bin: &std::path::Path,
) -> (SimManifest, SimManifest, String, String) {
    let m_sense = rr3_module_manifest("chain", "rr3-sense", sense_bin);
    let m_inter = rr3_module_manifest("chain", "rr3-interact", interact_bin);
    let cfg_arg = rr3_repo_root()
        .join("modules/rr3-sense/config.toml")
        .to_str()
        .expect("config 路径必须是 UTF-8")
        .to_string();
    let inter_cfg_arg = rr3_repo_root()
        .join("modules/rr3-interact/config.toml")
        .to_str()
        .expect("config 路径必须是 UTF-8")
        .to_string();
    (m_sense, m_inter, cfg_arg, inter_cfg_arg)
}

/// ① boot 起真进程；面板口 `:0` 由内核分配（缺省 8080 是单实例假设，并行必撞）。
pub(super) fn start(
    m_sense: &SimManifest,
    m_inter: &SimManifest,
    cfg: &str,
    inter_cfg: &str,
) -> RrdTest {
    RrdTest::rr3_start_boot_env(
        "e2e-chain",
        &[m_sense.path.as_path(), m_inter.path.as_path()],
        "chain",
        E2E_TICK_MS,
        &[
            ("RR3_SENSE_CONFIG", cfg),
            ("RR3_INTERACT_CONFIG", inter_cfg),
            ("RR3_PANEL_ADDR", "127.0.0.1:0"),
        ],
    )
}

/// ① 旁观连接先就位，再看模块起不起来（vad onset 是一次性边沿，只发给当时在册
/// 连接；模块 fork/exec 毫秒级，旁观者 connect+register 亚毫秒，注册必在首帧前）。
pub(super) fn register(rrd: &mut RrdTest) -> (RrdClient, u64, u64, u64) {
    let (obs, obs_task) = rr3_connect_observer(rrd);
    let sense_line = rrd.rr3_wait_log("注册 rr3-sense task=");
    let inter_line = rrd.rr3_wait_log("注册 rr3-interact task=");
    rrd.rr3_wait_log("rr3-sense → running");
    rrd.rr3_wait_log("rr3-interact → running");
    let sense_task = rr3_field_from_log(&sense_line, "task=");
    let inter_task = rr3_field_from_log(&inter_line, "task=");
    assert_ne!(sense_task, inter_task, "两个真模块各得一个 task 号");
    // 冷启动观测（P2 基线）：注册行必须带 boot_ms 数字；判据只要求可解析。
    let _ = rr3_field_from_log(&sense_line, "boot_ms=");
    let _ = rr3_field_from_log(&inter_line, "boot_ms=");
    (obs, obs_task, sense_task, inter_task)
}

/// ② vad_start 上总线：Event + edge=onset + src_task 是 sense 的分配值。
pub(super) fn vad_edge(obs: &mut RrdClient, sense_task: u64) {
    let vad = rr3_read_where(obs, " vad_start 事件", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/vad_start"
    });
    assert_eq!(
        vad.header.src_task, sense_task,
        "事件帧 src 必须是发布者的分配值"
    );
    let v = rr3_json_of(&vad);
    assert_eq!(v["edge"], "onset", "sim 正弦常幅，首个边沿必是 onset: {v}");
    assert!(
        v["rms_peak"].as_i64().unwrap_or(0) > 1500,
        "rms_peak 必须过起音阈值: {v}"
    );
}

/// ③ STREAM 有序：旁观者视角的连续后缀，seq 逐帧 +1；帧形状 16k/mono/20ms。
pub(super) fn pcm_ordered(obs: &mut RrdClient) -> u64 {
    let mut last = 0u64;
    for _ in 0..10 {
        let f = rr3_read_where(obs, " pcm 帧", |f| {
            rr3_json_of(f)["topic"] == "rr3-sense/audio/pcm"
        });
        let body = rr3_json_of(&f);
        assert_eq!(body["format"], "s16le");
        assert_eq!(body["rate"], 16000);
        assert_eq!(body["channels"], 1);
        assert_eq!(body["ms"], 20);
        let seq = body["seq"]
            .as_u64()
            .unwrap_or_else(|| panic!("pcm 体缺 seq: {body}"));
        if last != 0 {
            assert_eq!(seq, last + 1, "STREAM 必须逐帧递增（旁观者看到连续后缀）");
        }
        last = seq;
    }
    last
}

/// ④ TTS 回路：本链无 tts_pcm 流，点名对账必拒并折 result.error（MC-1）。
pub(super) fn tts_reject(obs: &mut RrdClient, obs_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            42,
            json!({"topic": "rr3-interact/tts/text", "args": {"text": "你好", "pcm_bytes": 1920}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(obs, 42);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("无流 TTS 点名必须折 result.error 拒因: {v}"));
    assert!(err.contains("不完整"), "拒因要点名缓冲对账缺口: {err}");
}

/// ④ 表情面：白名单切换，应答带回切换前后（TTS 被拒未进 talk 态，前态 idle）。
pub(super) fn face_expr(obs: &mut RrdClient, obs_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            43,
            json!({"topic": "rr3-interact/face/expression", "args": {"expression": "happy"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(obs, 43);
    assert_eq!(v["result"]["expression"], "happy", "{v}");
    assert_eq!(
        v["result"]["previous"], "idle",
        "TTS 在对账处拒返未进 talk 态，切换前是 idle: {v}"
    );
}

/// ④ 坏参数折 result.error 而不是断链（业务性失败走 Response.result 槽）。
pub(super) fn bad_args(obs: &mut RrdClient, obs_task: u64) {
    rr3_send(
        obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            44,
            json!({"topic": "rr3-interact/tts/text", "args": {}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(obs, 44);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("缺 text 必须回 result.error 字符串: {v}"));
    assert!(err.contains("text"), "错误信息要点名缺的参数: {err}");
}

/// ④ 回路打完流仍活着（TTS 拒返即时，没拖垮任何一方）。
pub(super) fn stream_alive(obs: &mut RrdClient, last: u64) {
    let f = rr3_read_where(obs, " 回路后的又一帧 pcm", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/pcm"
    });
    let seq = rr3_json_of(&f)["seq"].as_u64().unwrap_or(0);
    assert!(seq > last, "回路之后流必须继续推进: {seq} <= {last}");
}

/// 双麦夹具：channels=2 config 落 /tmp（pid 唯一路；清理归调用方的 CfgGuard）。
pub(super) fn doa_stage(sense_bin: &std::path::Path) -> (SimManifest, String, std::path::PathBuf) {
    let m_sense = rr3_module_manifest("doa", "rr3-sense", sense_bin);
    let path = std::env::temp_dir().join(format!("rr3-m3-doa-cfg-{}.toml", std::process::id()));
    std::fs::write(&path, DOA_CONFIG).expect("写双麦 config");
    let cfg = path.to_str().expect("config 路径必须是 UTF-8").to_string();
    (m_sense, cfg, path)
}

/// 双麦 boot：DOA_DEG=-60（左）经 rrd 进程继承传给真 rr3-sense。
pub(super) fn doa_start(m_sense: &SimManifest, cfg: &str) -> RrdTest {
    RrdTest::rr3_start_boot_env(
        "e2e-doa",
        &[m_sense.path.as_path()],
        "doa",
        E2E_TICK_MS,
        &[
            ("RR3_HAL_SIM_WAVE", "sine"),
            ("RR3_HAL_SIM_DOA_DEG", "-60"),
            ("RR3_SENSE_CONFIG", cfg),
        ],
    )
}

/// 双麦注册：旁观连接先就位，返回 (obs, sense 分配 task)。
pub(super) fn doa_register(rrd: &mut RrdTest) -> (RrdClient, u64) {
    let (obs, _obs_task) = rr3_connect_observer(rrd);
    let line = rrd.rr3_wait_log("注册 rr3-sense task=");
    let task = rr3_field_from_log(&line, "task=");
    rrd.rr3_wait_log("rr3-sense → running");
    (obs, task)
}

/// ② vad onset 带 doa：注入 -60° 的 ILD 反演落 ±2°，|deg|>30 落左档。
pub(super) fn doa_onset(obs: &mut RrdClient, sense_task: u64) {
    let vad = rr3_read_where(obs, " vad_start 事件（doa）", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/vad_start"
    });
    assert_eq!(
        vad.header.src_task, sense_task,
        "事件帧 src 必须是发布者的分配值"
    );
    let v = rr3_json_of(&vad);
    assert_eq!(v["edge"], "onset", "sim 正弦常幅，首个边沿必是 onset: {v}");
    let deg = v["doa"]["deg"]
        .as_i64()
        .unwrap_or_else(|| panic!("双麦 onset 必须带 doa 字段: {v}"));
    assert!(
        (-62..=-58).contains(&deg),
        "注入 -60° 的 ILD 反演必须落 ±2° 容差: {v}"
    );
    assert_eq!(v["doa"]["bin"], "left", "|deg| > 30 落左档: {v}");
}

/// ③ pcm 总线恒 mono（R14）：双麦原始通道只供 sense 内部判向。
pub(super) fn doa_mono(obs: &mut RrdClient) {
    let f = rr3_read_where(obs, " pcm 帧", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/pcm"
    });
    let body = rr3_json_of(&f);
    assert_eq!(body["channels"], 1, "下混后总线承载恒 mono: {body}");
}

/// ⑤ 采集新鲜度记账：ctl.counters 有界轮询（12 拍 × 250ms = 3s 上界）。
pub(super) fn counters(rrd: &RrdTest) {
    let mut frames = 0u64;
    for attempt in 0..12u64 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let mut ctl = rrd.rr3_connect();
        let v = rr3_ask_ctl(&mut ctl, "rr3.ctl.counters", 500 + attempt);
        frames = v["result"]["counters"]["rr3-sense/pcm_frames"]
            .as_u64()
            .unwrap_or(0);
        if frames >= 1 {
            break;
        }
    }
    assert!(
        frames >= 1,
        "sim 源跑满数秒后 rr3-sense/pcm_frames 必须非零（采集新鲜度的记账线）: {frames}"
    );
}
