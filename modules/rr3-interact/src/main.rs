//! rr3-interact 真入口（M3）：总线注册/心跳 + TTS 播放 + 表情机。
//!
//! 启动序列与 rr3-sense 同构：读 `RR3_MANIFEST`（rrd spawn 注入，supervisor.rs）
//! → [`Rr3BusClient::rr3_connect`] → 打开 `Rr3AudioSink` → 事件循环。循环只做
//! 一件事：`rr3_poll_wait` 收帧，应答本模块 exposes 的两个点名——
//! - `rr3-interact/tts/text`：占位正弦 burst 写 sink，回 `{"played_ms","chars"}`；
//! - `rr3-interact/face/expression`：表情机切换，回 `{"expression","previous"}`。
//!
//! rr3.call 转发帧的 trace_id=调用方 task（rrd 路由语义），顺原路应答。
//! 业务性失败（缺参数、不认的表情、后端写失败）折成 error 应答回话；传输性
//! 失败（发布/应答写不出去）`eprintln` + `exit(1)`——模块不装"活着"，死交给
//! supervisor 按 restart_policy=always 重启（rrd 的进程语义）。

mod face;
mod panel;
mod player;

use std::time::Duration;

use rr_bus::Rr3BusClient;
use rr_hal::{backend::rr3_hal_audio_sink, rr3_block_on, RR3_AUDIO_CFG_16K_MONO16};
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

use face::Rr3Face;

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// 收帧等待上界：无点名时空转一圈的代价，值不敏感（心跳在独立线程）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

fn main() {
    if let Err(e) = rr3_run() {
        eprintln!("rr3-interact 退出: {e}");
        std::process::exit(1);
    }
}

fn rr3_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    println!("rr3-interact 0.0.1 (M3 实装)");
    for line in face::SURFACE
        .iter()
        .copied()
        .chain(player::SURFACE.iter().copied())
        .chain(panel::surface())
    {
        println!("  {line}");
    }

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    println!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());

    let (sink, backend) = rr3_hal_audio_sink();
    rr3_block_on(sink.open(RR3_AUDIO_CFG_16K_MONO16))
        .map_err(|e| format!("打开播放面({backend})失败: {e}"))?;
    println!("播放面就绪: backend={backend} rate=16000 ch=1");

    let mut face = Rr3Face::default();
    loop {
        // 总线死亡探针：断开后 poll_wait 立即返回空 Vec，与"暂时没帧"不可区分——
        // 不探测，总线一死本循环就满转成僵尸。假了就走"不装活着"的退出语义。
        if !bus.rr3_connected() {
            return Err("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定".into());
        }
        // 排空 inbox：v0.0.1 不订阅任何主题，inbox 只会有点名 Request——
        // rr3.call 转发帧的 trace_id=调用方 task（rrd 路由语义），顺原路应答
        for frame in bus.rr3_poll_wait(RR3_POLL_WAIT) {
            if frame.header.opcode != opcode::SEND {
                continue;
            }
            let Ok((Rr3PayloadType::Request, body)) = Rr3BusClient::rr3_frame_body(&frame) else {
                continue;
            };
            if body.get("method").and_then(Value::as_str) != Some(rr_bus::RR3_METHOD_CALL) {
                continue;
            }
            let id = body.get("id").and_then(Value::as_u64).unwrap_or(0);
            let params = body.get("params").cloned().unwrap_or(Value::Null);
            let topic = params.get("topic").and_then(Value::as_str).unwrap_or("");
            let args = params.get("args").cloned().unwrap_or(Value::Null);

            // 业务性失败折成 error 应答（请求-应答语义），不当致命错误
            let result = match topic {
                player::RR3_TTS_TOPIC => rr3_handle_tts(&bus, sink.as_ref(), &mut face, &args),
                face::RR3_FACE_TOPIC => rr3_handle_face(&mut face, &args),
                other => Err(format!(
                    "rr3-interact 只 exposes {}/{}，收到 {other}",
                    player::RR3_TTS_TOPIC,
                    face::RR3_FACE_TOPIC
                )),
            };
            let result = result.unwrap_or_else(|e| json!({ "error": e }));
            bus.rr3_reply_call(frame.header.trace_id, id, result)?;
        }
    }
}

/// TTS 点名：校验参数 → 表情切 talk → 播放 → 恢复表情 → 回 played_ms。
fn rr3_handle_tts(
    _bus: &Rr3BusClient,
    sink: &dyn rr_hal::Rr3AudioSink,
    face: &mut Rr3Face,
    args: &Value,
) -> Result<Value, String> {
    let text = player::rr3_tts_parse_args(args)?;
    let suspended = face.rr3_face_begin_talk();
    let played = player::rr3_tts_play(sink, &text);
    face.rr3_face_end_talk();
    let played_ms = played?;
    println!(
        "tts: {} 字符播了 {played_ms}ms（talk 挂起 {suspended}）",
        text.chars().count()
    );
    Ok(player::rr3_tts_result(&text, played_ms))
}

/// 表情点名：校验并切换，回 `{"expression","previous"}`。
fn rr3_handle_face(face: &mut Rr3Face, args: &Value) -> Result<Value, String> {
    let expr = face::rr3_face_parse_args(args)?;
    let (current, previous) = face.rr3_face_set(&expr)?;
    Ok(face::rr3_face_result(current, previous))
}
