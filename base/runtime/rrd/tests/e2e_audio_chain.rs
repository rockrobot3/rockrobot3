#![cfg(unix)]
//! M3 音频链三真进程端到端：rrd + rr3-sense(sim) + rr3-interact(sim) 全是**真二进制**。
//!
//! 前面的 e2e 用的都是 sim_peer 夹具（行为脚本）；这一组把仓里两个真模块拉进同一条总线，
//! 钉的是音频链的整体形状：
//! 1. boot spawn → 注册 → running（supervisor 的进程语义对真 manifest 成立）；
//! 2. `rr3-sense/audio/vad_start` 事件上总线（sim 正弦必过 VAD 起音阈值，恰一次 onset）；
//! 3. `rr3-sense/audio/pcm` stream 有序——seq 从旁观连接看到的任意一点起逐帧 +1
//!    （旁观者看到的是流的**连续后缀**，起点不是 1 是正形，起点错序才是 bug）；
//! 4. `rr3.call` 回路穿过真进程：旁观连接点名 `rr3-interact/tts/text` 与
//!    `rr3-interact/face/expression`，应答体（played_ms/chars、expression/previous）
//!    由 interact 进程里的播放面与表情机真实算出——模块侧 rr3_reply_call 的
//!    trace_id 语义在这条线上被端到端复证。
//!
//! 模块二进制**由测试现场 cargo build**（独立工程，`CARGO_BIN_EXE` 够不着它们）：
//! 增量编译下缓存命中，代价只在首次全量。

mod support;

use std::path::{Path, PathBuf};

use serde_json::json;

use support::{
    rr3_build_module_bin, rr3_connect_observer, rr3_field_from_log, rr3_json_of,
    rr3_module_manifest_text, rr3_read_call_reply, rr3_read_where, rr3_rpc_frame, rr3_send,
    RrdTest, SimManifest, E2E_TICK_MS,
};

/// --manifest 直驱的落盘薄封装：渲染文本来自 support 的 [`rr3_module_manifest_text`]，
/// 这里只负责 pid+tag 唯一的 /tmp 路径（Global Constraints 8）。
fn rr3_module_manifest(tag: &str, name: &str, bin: &Path) -> SimManifest {
    let rendered = rr3_module_manifest_text(name, bin);
    let path = PathBuf::from(format!(
        "/tmp/rr3-m3-module-{}-{tag}-{name}.toml",
        std::process::id()
    ));
    std::fs::write(&path, rendered)
        .unwrap_or_else(|e| panic!("写夹具 manifest {} 失败: {e}", path.display()));
    SimManifest { path }
}

#[test]
fn audio_chain_sense_to_interact_end_to_end() {
    // ---------- 夹具：真二进制 + 真 manifest ----------
    let sense_bin = rr3_build_module_bin("rr3-sense");
    let interact_bin = rr3_build_module_bin("rr3-interact");
    let m_sense = rr3_module_manifest("chain", "rr3-sense", &sense_bin);
    let m_inter = rr3_module_manifest("chain", "rr3-interact", &interact_bin);

    // ---------- ① boot spawn → 注册 → running ----------
    // boot 用例必须传小 tick（support 注释：sweep 复用同一旋钮，安静 tick 会让收尸永不发生）
    let mut rrd = RrdTest::rr3_start_boot(
        "e2e-chain",
        &[m_sense.path.as_path(), m_inter.path.as_path()],
        "chain",
        E2E_TICK_MS,
    );

    // ---------- ① 旁观连接先就位，再看模块起不起来 ----------
    // 顺序不能反：vad_start 是**一次性边沿**（sim 正弦常幅，onset 恰一次），广播只发给
    // 当时在册的连接——等注册行到了再连，边沿早就广播完了。这里在 rr3_start_boot 返回后
    // 立即注册旁观者：模块那边要付 fork/exec 的整条链（毫秒级），旁观者只付一次 connect
    // +register 往返（亚毫秒），差着两个数量级，注册必落在 sense 第一帧发布之前。
    let (mut obs, obs_task) = rr3_connect_observer(&mut rrd);
    let sense_line = rrd.rr3_wait_log("注册 rr3-sense task=");
    let inter_line = rrd.rr3_wait_log("注册 rr3-interact task=");
    rrd.rr3_wait_log("rr3-sense → running");
    rrd.rr3_wait_log("rr3-interact → running");
    let sense_task = rr3_field_from_log(&sense_line, "task=");
    let inter_task = rr3_field_from_log(&inter_line, "task=");
    assert_ne!(sense_task, inter_task, "两个真模块各得一个 task 号");

    // ---------- ② vad_start 上总线：Event + edge=onset + src_task 是 sense 的分配值
    let vad = rr3_read_where(&mut obs, " vad_start 事件", |f| {
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

    // ---------- ③ STREAM 有序：旁观者视角的连续后缀，seq 逐帧 +1 ----------
    // 顺带钉住事件形状：s16le/16k/mono、20ms 一帧（320 采样）
    let mut last: Option<u64> = None;
    for _ in 0..10 {
        let f = rr3_read_where(&mut obs, " pcm 帧", |f| {
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
        if let Some(p) = last {
            assert_eq!(seq, p + 1, "STREAM 必须逐帧递增（旁观者看到连续后缀）");
        }
        last = Some(seq);
    }

    // ---------- ④ rr3.call 回路穿过真进程 ----------
    // TTS："你好"=2 字符 × 60ms = 120ms（占位合成公式在 interact 进程里真实执行）
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            42,
            json!({"topic": "rr3-interact/tts/text", "args": {"text": "你好"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 42);
    assert!(v.get("error").is_none(), "TTS 点名不该被拒: {v}");
    assert_eq!(
        v["result"]["played_ms"], 120,
        "60ms/字符的占位时长公式: {v}"
    );
    assert_eq!(v["result"]["chars"], 2);

    // 表情：五态白名单切换，应答带回切换前后
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            43,
            json!({"topic": "rr3-interact/face/expression", "args": {"expression": "happy"}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 43);
    assert_eq!(v["result"]["expression"], "happy", "{v}");
    assert_eq!(
        v["result"]["previous"], "idle",
        "TTS 的 talk 挂起已恢复，切换前是 idle: {v}"
    );

    // 坏参数折成 result.error 应答而不是断链（业务性失败 ≠ 传输性失败的线上分界；
    // rr3_reply_call 走的是 Response.result 槽，JSON-RPC error 槽留给 rrd 的路由拒绝）
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            44,
            json!({"topic": "rr3-interact/tts/text", "args": {}}),
            obs_task,
        ),
    );
    let v = rr3_read_call_reply(&mut obs, 44);
    let err = v["result"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("缺 text 必须回 result.error 字符串: {v}"));
    assert!(err.contains("text"), "错误信息要点名缺的参数: {err}");

    // 回路打完，sense 的流还活着（interact 的 120ms 阻塞没拖垮任何一方）
    let f = rr3_read_where(&mut obs, " 回路后的又一帧 pcm", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/pcm"
    });
    let seq = rr3_json_of(&f)["seq"].as_u64().unwrap_or(0);
    assert!(
        seq > last.unwrap_or(0),
        "回路之后流必须继续推进: {seq} <= {last:?}"
    );
}

/// 双麦判向 e2e 的 config 守卫：channels=2 的 sense 配置落 /tmp（pid 唯一路），Drop 清理
/// ——断言失败会 panic，自己删的那一行跑不到（SimManifest 同一条教训）。
struct CfgGuard {
    path: PathBuf,
}

impl Drop for CfgGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 双麦 config 夹具：与仓内 `modules/rr3-sense/config.toml` 同形状，唯 channels=2——
/// DoA 注入只在 ch_n>=2 时被 sim 源采纳（backend.rs 同一纪律），单麦链路无方向语义。
/// VAD 在下混后的 mono 上判，阈值与仓内 config 同值。
const DOA_CONFIG: &str = r#"
[rr3-sense]
log_level = "info"

[rr3-sense.audio]
sample_rate = 16000
channels = 2
frame_ms = 20

[rr3-sense.vad]
rms_start = 1500        # 起音阈值
silence_tail_ms = 700   # 判停尾静音
min_speech_ms = 200
"#;

#[test]
fn audio_chain_doa_stereo_env_injection_end_to_end() {
    // ---------- 夹具：真二进制 + 真 manifest + 双麦 config + DOA env ----------
    let sense_bin = rr3_build_module_bin("rr3-sense");
    let m_sense = rr3_module_manifest("doa", "rr3-sense", &sense_bin);
    let cfg_path =
        std::env::temp_dir().join(format!("rr3-m3-doa-cfg-{}.toml", std::process::id()));
    std::fs::write(&cfg_path, DOA_CONFIG).expect("写双麦 config");
    let cfg_arg = cfg_path
        .to_str()
        .expect("config 路径必须是 UTF-8")
        .to_string();
    let _cfg = CfgGuard { path: cfg_path };

    // ---------- ① boot：env 三件套经 rrd 进程继承传给真 rr3-sense ----------
    // DOA_DEG=-60（左）：sim 源按 ILD 模型给双通道加增益（g_l=1-sinθ, g_r=1+sinθ），
    // sense 侧在下混前反演——注入的方位经总线回到观察者手里必须仍是 -60° 档
    // （判据 4 的 sim 侧证据：DoA 在 sense 内部落档，零新契约）。
    let mut rrd = RrdTest::rr3_start_boot_env(
        "e2e-doa",
        &[m_sense.path.as_path()],
        "doa",
        E2E_TICK_MS,
        &[
            ("RR3_HAL_SIM_WAVE", "sine"),
            ("RR3_HAL_SIM_DOA_DEG", "-60"),
            ("RR3_SENSE_CONFIG", cfg_arg.as_str()),
        ],
    );

    // 旁观连接先就位（与既有用例同一条纪律：onset 是一次性边沿，广播只发给在册连接）
    let (mut obs, _obs_task) = rr3_connect_observer(&mut rrd);
    let sense_line = rrd.rr3_wait_log("注册 rr3-sense task=");
    let sense_task = rr3_field_from_log(&sense_line, "task=");
    rrd.rr3_wait_log("rr3-sense → running");

    // ---------- ② vad onset 带 doa：sense 内部反演 + ±30° 分档 ----------
    let vad = rr3_read_where(&mut obs, " vad_start 事件（doa）", |f| {
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

    // ---------- ③ pcm 总线恒 mono（R14）：双麦原始通道只供 sense 内部判向 ----------
    let f = rr3_read_where(&mut obs, " pcm 帧", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/pcm"
    });
    let body = rr3_json_of(&f);
    assert_eq!(body["channels"], 1, "下混后总线承载恒 mono: {body}");
}
