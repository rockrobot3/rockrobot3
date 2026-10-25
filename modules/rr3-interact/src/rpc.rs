//! rr3-interact 协议层（0.0.8 W-A 批4 拆分，ADR-0017 五层表协议层）。
//!
//! 载荷组装面（0.0.5 批I I-8：上移承载层，ADR-0019 修订 4——件的对外契约面 =
//! 结构体 + 判定函数；点名下钻 parse_args 与应答体 result 的 Value 生产归本
//! 文件，线上字节逐字不变，用例随迁继续守字节面）＋ 错误文案渲染面（批N24
//! 件四 L-3：机制层零堆枚举，文案在编排层渲染）＋ 点名执行（[`rr3_handle_tts`]
//! / [`rr3_handle_face`]，自原 composite.rs 迁入；bus 参数换 rr3mod
//! [`BusHandle`]——counter_bump 透传）。

use rr3mod::BusHandle;
use serde_json::{json, Value};

use crate::boot::{Rr3InteractCfg, Rr3PlayerCfg};
use crate::dispatch::Rr3TtsPcmBuf;
use crate::face;
use crate::player;

/// 本模块 exposes 的 ops 槽快照读口（批N4 件九二，module.toml [rpc].exposes 同源）：
/// 白名单组体应答，槽名与 assemble.toml/composite.toml 槽表申报同源
/// （门 d covers 申报的对账面）
pub(crate) const RR3_OPS_LIST_TOPIC: &str = "rr3-interact/ops/list";

/// face 机制错误的人读文案（批N24 件四，L-3 形状清理）：机制层零堆枚举
/// （`Rr3FaceSetError`，拒因数据随枚举走），文案在编排层渲染——线上字节
/// 与改前逐字一致（契约面）。白名单连接串用常量投影（渲染不再 join 落堆）；
/// 不可点名集是 config 注入的运行时数据，渲染面经
/// `rr3_face_non_pointable_join` 组装（每句落在唯一一处）。
pub(crate) fn rr3_face_set_error_text(e: &face::Rr3FaceSetError, non_pointable: &[&str]) -> String {
    match e {
        face::Rr3FaceSetError::UnknownExpression(expr) => format!(
            "{} 不认表情 {expr:?}（允许 {}）",
            face::RR3_FACE_TOPIC,
            face::RR3_FACE_EXPRESSIONS_JOINED
        ),
        face::Rr3FaceSetError::NonPointable(expr) => format!(
            "{expr} 不可被点名（本形态不可点名集 {}）",
            face::rr3_face_non_pointable_join(non_pointable)
        ),
    }
}

/// player 机制错误的人读文案（批N24 件四，L-3 形状清理）：机制层零堆枚举
/// （`Rr3TtsPlayError`，HAL 错原样随行），文案在编排层渲染——`{e}` 走
/// `Rr3HalError` Display，与改前逐字一致（每句落在唯一一处）。
pub(crate) fn rr3_tts_play_error_text(e: &player::Rr3TtsPlayError) -> String {
    match e {
        player::Rr3TtsPlayError::Write(e) => format!("写播放面失败: {e}"),
    }
}

/// TTS 点名（MC-1 重铺 / MC-2 播放面参数）：参数校验 → PCM 缓冲对账（半流/
/// 残留皆拒，拒因路径同样清缓冲）→ 起播水位判定（fail-closed）→ 表情切 talk
/// → 样点增益 → 排空缓冲播放 → 恢复表情 → 回 played_ms。
/// 记账：拒 `tts_play_reject`、播成 `tts_play_ok`（收流帧另有 `tts_pcm_frames`）。
pub(crate) fn rr3_handle_tts(
    bus: &BusHandle,
    sink: &mut dyn rr_hal_api::Rr3AudioSink,
    face: &mut face::Rr3Face,
    tts_pcm: &mut Rr3TtsPcmBuf,
    args: &Value,
    player: &Rr3PlayerCfg,
) -> Result<Value, String> {
    let (text, pcm_bytes) = rr3_tts_parse_args(args)?;
    if let Err(e) = rr3_tts_pcm_integrity(pcm_bytes, tts_pcm.rr3_tts_buf_len()) {
        bus.counter_bump("tts_play_reject", 1);
        tts_pcm.rr3_tts_buf_take();
        return Err(e);
    }
    let mut samples = rr3_pcm_bytes_to_samples(&tts_pcm.rr3_tts_buf_take());
    // 起播水位（MC-2）：对账通过即缓冲完整——`buffered == total` 是对账蕴含的
    // 架构事实，两处如实传同一值。判据显式在场（legacy 0cf029e 语义：达水位
    // 才起播、流完整未攒满也播）；流式化（增补轮五批E）后 buffered < total 的
    // 真分支接管起播时机。fail-closed：不过即拒（拒因点名水位）。
    let total_ms = samples.len() as u64 * 1000 / player::RR3_TTS_SAMPLE_RATE as u64;
    if !player::rr3_tts_prebuffer_ready(total_ms, total_ms, player.prebuffer_ms) {
        bus.counter_bump("tts_play_reject", 1);
        return Err(format!(
            "起播水位未达：缓冲 {total_ms}ms < 水位 {}ms（prebuffer_ms），弃播",
            player.prebuffer_ms
        ));
    }
    // 播放增益（MC-2）：config 注入的 volume_pct 整型饱和缩放，播放前就地生效
    player::rr3_tts_apply_gain(&mut samples, player.volume_pct);
    let suspended = face.rr3_face_begin_talk();
    let played_ms = player::rr3_tts_play(sink, &samples)
        .map_err(|e| rr3_tts_play_error_text(&e));
    face.rr3_face_end_talk();
    let played_ms = played_ms?;
    // played_ms==0 不得记 ok（批N4 件六 I-21①）：对账通过但样点不足一个毫秒窗
    // （<16 样点）对用户等于没听见——按业务失败折 error 应答（brain 侧 audible
    // 不计，B-16① 拆账判据），与传输失败分账不折成功。
    if played_ms == 0 {
        bus.counter_bump("tts_play_reject", 1);
        return Err(format!(
            "tts 播出 0ms（PCM {pcm_bytes} 字节对账通过但不足 1ms 窗口），不得记 ok（I-21①）"
        ));
    }
    bus.counter_bump("tts_play_ok", 1);
    eprintln!(
        "tts: {} 字符播了 {played_ms}ms（PCM {pcm_bytes} 字节，gain {}%，水位 {}ms，talk 挂起 {suspended}）",
        text.chars().count(),
        player.volume_pct,
        player.prebuffer_ms
    );
    Ok(rr3_tts_result(&text, played_ms))
}

/// 表情点名：校验并切换，回 `{"expression","previous"}`。`non_pointable` = 本形态
/// 不可点名集（I-11：config 注入，透传给件面判定）。
pub(crate) fn rr3_handle_face(
    face: &mut face::Rr3Face,
    args: &Value,
    non_pointable: &[&str],
) -> Result<Value, String> {
    let expr = rr3_face_parse_args(args)?;
    let (current, previous) = face
        .rr3_face_set(&expr, non_pointable)
        .map_err(|e| rr3_face_set_error_text(&e, non_pointable))?;
    Ok(rr3_face_result(current, previous))
}

// ---------- 载荷组装面（0.0.5 批I I-8：上移承载层，ADR-0019 修订 4） ----------
// 件的对外契约面 = 结构体 + 判定函数；点名下钻（parse_args）与应答体（result）
// 的 Value 生产归本文件，线上字节逐字不变（用例随迁继续守字节面）。

/// TTS 点名下钻（MC-1）：`text` 必填且须非空字符串；`pcm_bytes` 必填——本轮
/// PCM 的字节总数，播放面对账收流缓冲完整性的唯一判据（无流不静默）。校验
/// 失败返回 `Err`，由调用方折成 error 应答（请求-应答语义，不当致命错误）。
pub(crate) fn rr3_tts_parse_args(args: &Value) -> Result<(String, u64), String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} 缺 string 参数 text", player::RR3_TTS_TOPIC))?;
    if text.is_empty() {
        return Err(format!("{} 的 text 不能为空", player::RR3_TTS_TOPIC));
    }
    let pcm_bytes = args
        .get("pcm_bytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{} 缺 integer 参数 pcm_bytes", player::RR3_TTS_TOPIC))?;
    Ok((text.to_string(), pcm_bytes))
}

/// tts PCM 缓冲对账（MC-1）：点名申报的 `pcm_bytes` vs 缓冲实收。少 = 半流
/// （发布侧断流/坏帧弃置），多 = 残留（上轮余量未清）——皆拒，拒因带缺口
/// 字节。下界（批N4 件六 I-21①）：申报 0 字节一律拒——`buffered == pcm_bytes`
/// 的等式判据会把"什么都没发"当"全发到了"（云 200 空体 → (0,0) → Ok →
/// `played_ms=0` → 计 ok 的静默假成功口），无流不静默。判定纯函数化：记账
/// （tts_play_reject）与清缓冲归调用方。
pub(crate) fn rr3_tts_pcm_integrity(pcm_bytes: u64, buffered: usize) -> Result<(), String> {
    if pcm_bytes == 0 {
        return Err(format!(
            "tts PCM 点名申报 0 字节（空体无流，I-21① 下界拒），弃播"
        ));
    }
    let buffered = buffered as u64;
    if buffered == pcm_bytes {
        return Ok(());
    }
    let (how, gap) = if buffered < pcm_bytes {
        ("半流", pcm_bytes - buffered)
    } else {
        ("残留", buffered - pcm_bytes)
    };
    Err(format!(
        "tts PCM 缓冲不完整（{how}）：点名申报 {pcm_bytes} 字节、缓冲实收 {buffered} 字节，缺口 {gap} 字节，弃播"
    ))
}

/// s16le 字节列 → i16 样点：小端两两成对。brain 分帧保证偶数字节，尾部奇数
/// 字节不成样点，防御性丢弃（chunks_exact 自带该语义）。
pub(crate) fn rr3_pcm_bytes_to_samples(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect()
}

/// `rr3-interact/tts/text` 的应答体：实际播出的毫秒数与计费字符数
/// （played_ms 由实收样点导出，与字符数脱钩，一并回给调用方对表）。
pub(crate) fn rr3_tts_result(text: &str, played_ms: u64) -> Value {
    json!({ "played_ms": played_ms, "chars": text.chars().count() })
}

/// 表情点名下钻：`expression` 必填。合法性交给 [`face::Rr3Face::rr3_face_set`]
/// （错误信息里带白名单，这里不做两遍）。
pub(crate) fn rr3_face_parse_args(args: &Value) -> Result<String, String> {
    args.get("expression")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{} 缺 string 参数 expression", face::RR3_FACE_TOPIC))
}

/// `rr3-interact/face/expression` 的应答体：切换后的表情与切换前的表情。
pub(crate) fn rr3_face_result(current: &str, previous: &str) -> Value {
    json!({ "expression": current, "previous": previous })
}

/// `rr3-interact/ops/list` 应答体（批N4 件九二，面板 /api/interact/ops 读口）：
/// ops 槽快照，**白名单组体**——槽名与 assemble.toml/composite.toml 槽表申报
/// 同源（face/player/panel/selftest 四组，门 d covers 申报的对账面），config
/// 其余键一律不进应答。组合件级七槽（件五 I-17/I-18② 数据化）住 face_tick_ms/
/// selftest_face_expect/panel 五限额，键 = 槽名逐字。
pub(crate) fn rr3_interact_ops_snapshot(cfg: &Rr3InteractCfg) -> Value {
    json!({
        "face": {
            "non_pointable": cfg.face.non_pointable,
            "default_expression": cfg.face.default_expr,
            "face_tick_ms": cfg.face.face_tick_ms,
        },
        "player": {
            "prebuffer_ms": cfg.player.prebuffer_ms,
            "volume_pct": cfg.player.volume_pct,
        },
        "panel": {
            "panel_max_conns": cfg.panel.max_conns,
            "panel_read_timeout_ms": cfg.panel.read_timeout_ms,
            "panel_header_limit": cfg.panel.header_limit,
            "panel_body_limit": cfg.panel.body_limit,
            "panel_call_timeout_ms": cfg.panel.call_timeout_ms,
        },
        "selftest": {
            "selftest_face_expect": cfg.selftest.face_expect,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::Rr3TtsPcmBuf;
    use crate::face::{Rr3Face, Rr3FaceSetError};
    use crate::player::Rr3TtsPlayError;

    /// 点名下钻与应答体形状（用例自件面随迁，I-8：承载层守字节面）。
    #[test]
    fn args_and_result_shapes() {
        assert_eq!(
            rr3_tts_parse_args(&json!({ "text": "hi", "pcm_bytes": 640 })).unwrap(),
            ("hi".to_string(), 640)
        );
        for bad in [
            json!({}),
            json!({ "text": 3 }),
            json!({ "text": "" }),
            json!({ "text": null }),
            json!({ "text": "hi" }), // 缺 pcm_bytes（对账判据必带）
            json!({ "text": "hi", "pcm_bytes": "640" }), // 非整数
            json!({ "text": "hi", "pcm_bytes": -1 }), // 负数不成字节
        ] {
            assert!(rr3_tts_parse_args(&bad).is_err(), "{bad}");
        }
        assert_eq!(
            rr3_face_parse_args(&json!({ "expression": "blink" })).unwrap(),
            "blink"
        );
        assert!(rr3_face_parse_args(&json!({})).is_err());
        assert!(rr3_face_parse_args(&json!({ "expression": 1 })).is_err());

        let r = rr3_tts_result("你好", 120);
        assert_eq!(r["played_ms"], 120);
        assert_eq!(r["chars"], 2);
        assert_eq!(r.as_object().unwrap().len(), 2, "tts 应答两字段无加戏: {r}");
        assert_eq!(r.get("topic"), None, "应答体不带 topic");
        let r = rr3_face_result("talk", "idle");
        assert_eq!(r["expression"], "talk");
        assert_eq!(r["previous"], "idle");
        assert_eq!(
            r.as_object().unwrap().len(),
            2,
            "face 应答两字段无加戏: {r}"
        );
        assert_eq!(r.get("topic"), None, "应答体不带 topic");
    }

    /// tts PCM 缓冲对账（MC-1）：完整 = 过；半流/残留 = 拒且缺口字节进拒因；
    /// 零字节申报一律拒（I-21① 下界：(0,0) 不再是合法通过态）
    #[test]
    fn tts_pcm_integrity_rejects_half_and_residual_streams() {
        assert_eq!(rr3_tts_pcm_integrity(640, 640), Ok(()));
        let e = rr3_tts_pcm_integrity(1920, 640).unwrap_err();
        assert!(e.contains("半流") && e.contains("1280"), "{e}");
        let e = rr3_tts_pcm_integrity(640, 1280).unwrap_err();
        assert!(e.contains("残留") && e.contains("640"), "{e}");
        let e = rr3_tts_pcm_integrity(1920, 0).unwrap_err();
        assert!(e.contains("半流"), "空缓冲必拒（无流不静默）: {e}");
        let e = rr3_tts_pcm_integrity(0, 0).unwrap_err();
        assert!(e.contains("0 字节"), "(0,0) 恒拒（I-21① 静默假成功口）: {e}");
        let e = rr3_tts_pcm_integrity(0, 640).unwrap_err();
        assert!(e.contains("0 字节"), "零申报有残留同样拒: {e}");
    }

    /// s16le 字节列 → i16 样点：小端两两成对，奇数尾字节防御性丢弃
    #[test]
    fn pcm_bytes_to_samples_is_little_endian_pairs() {
        assert_eq!(rr3_pcm_bytes_to_samples(&[0x34, 0x12]), vec![0x1234]);
        assert_eq!(rr3_pcm_bytes_to_samples(&[0x00, 0x80]), vec![i16::MIN]);
        assert!(rr3_pcm_bytes_to_samples(&[0x01]).is_empty());
    }

    /// 跨语言一致性（MC-1③）：`contracts/golden/tts-pcm-frames.json` 是 Python
    /// 腿（rr3-brain 分帧）与本腿共读的同一份基准——把 golden 帧列逐帧解码入
    /// 收流缓冲，拼接流必须逐字节还原基准 PCM，seq/ms 同拍（两侧各自的单测钉
    /// 语义，这条钉"同一份字节基准两侧对得上"）。
    #[test]
    fn tts_pcm_buf_matches_shared_golden_vector() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/golden/tts-pcm-frames.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读 golden {}: {e}", path.display()));
        let g: Value = serde_json::from_str(&text).expect("golden JSON 解析");

        fn hex_decode(s: &str) -> Vec<u8> {
            s.as_bytes()
                .chunks_exact(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect()
        }

        let mut buf = Rr3TtsPcmBuf::default();
        let mut seqs: Vec<u64> = Vec::new();
        let mut mss: Vec<u64> = Vec::new();
        for f in g["frames"].as_array().expect("golden frames 数组") {
            seqs.push(f["seq"].as_u64().expect("帧缺 seq"));
            mss.push(f["ms"].as_u64().expect("帧缺 ms"));
            buf.rr3_tts_buf_push(f["pcm_b64"].as_str().expect("帧缺 pcm_b64"))
                .expect("golden 帧 base64 必须合法");
        }
        assert_eq!(seqs, vec![1, 2, 3], "seq 从水位 0 起逐帧 +1");
        assert_eq!(mss, vec![20, 20, 10], "整块 20ms、尾块按实算");

        let want = hex_decode(g["pcm_hex"].as_str().expect("缺 pcm_hex"));
        assert_eq!(buf.rr3_tts_buf_len(), want.len(), "收流字节数 == 基准 PCM");
        assert_eq!(buf.rr3_tts_buf_take(), want, "收流拼接逐字节还原基准 PCM");
        // 1600 字节 = 800 样点 = 50ms（20ms×2 + 10ms）：解码产物可直接喂播放面
        let samples = rr3_pcm_bytes_to_samples(&want);
        assert_eq!(samples.len(), 800);
        assert_eq!(samples.len() as u64 * 1000 / 16000, 50);
    }

    /// 注入集经组合件透传进件面判定（I-11 端到端）：缺省排除 talk；放开则 talk 可点名
    #[test]
    fn non_pointable_flows_through_handle_face() {
        let mut f = Rr3Face::default();
        let err = rr3_handle_face(&mut f, &json!({ "expression": "talk" }), &["talk"]).unwrap_err();
        assert!(err.contains("不可被点名"), "talk 被排除集拒: {err}");
        let ok = rr3_handle_face(&mut f, &json!({ "expression": "talk" }), &[]).unwrap();
        assert_eq!(ok["expression"], "talk", "空集放开 talk 点名");
    }

    /// 错误文案逐字钉死（批N24 件四，L-3 形状清理的对账单测）：机制层改交
    /// 零堆枚举后，编排层渲染的 `String` 必须与改前逐字一致（线上契约面）。
    #[test]
    fn face_set_error_texts_are_verbatim() {
        let e = Rr3FaceSetError::UnknownExpression("wink");
        assert_eq!(
            rr3_face_set_error_text(&e, &["talk"]),
            "rr3-interact/face/expression 不认表情 \"wink\"（允许 idle/blink/talk/think/happy/listen/turn/charA/charB/charC/charD/charE/charF/charG/charH/charI/charJ/charK/charL/charM/charN/charO/charP/charQ/charR/charS/charT/charU/charV/charW/charX/charY/charZ/char0/char1/char2/char3/char4/char5/char6/char7/char8/char9/sleepy/surprised/angry/sad）"
        );
        let e = Rr3FaceSetError::NonPointable("talk");
        assert_eq!(
            rr3_face_set_error_text(&e, &["talk"]),
            "talk 不可被点名（本形态不可点名集 talk）"
        );
        let e = Rr3FaceSetError::NonPointable("talk");
        assert_eq!(
            rr3_face_set_error_text(&e, &[]),
            "talk 不可被点名（本形态不可点名集 (空)）",
            "空集渲染 (空)（rr3_face_non_pointable_join 同款口径）"
        );
    }

    /// tts 错误文案逐字钉死（批N24 件四）：`{e}` 走 `Rr3HalError` Display
    /// （HAL 错文本随行原样）。
    #[test]
    fn tts_play_error_text_is_verbatim() {
        let e = Rr3TtsPlayError::Write(rr_hal_api::Rr3HalError::Busy);
        assert_eq!(rr3_tts_play_error_text(&e), "写播放面失败: HAL 设备忙");
    }
}
