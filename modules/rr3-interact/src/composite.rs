//! rr3-interact 组合件实现（组合件五件套③，0.0.5 批C③ 原子重铺）。
//!
//! 组合件 = face ⊗ player（composite.toml 的 ⊗ 链；⊗=装配期、执行单元内函数
//! 调用，ADR-0019 裁定 2）。本文件承载装配与运行机制：总线注册/心跳 + TTS
//! 播放 + 表情机 + Web 面板（0.0.4 批C，panel 状态面留组合件，0.0.5 方案
//! §12.1——面板是 HTTP 线程 + 动作通道的接入编排面）。
//!
//! 启动序列（自原入口 rr3_run 逐字迁入，语义零改动）与 rr3-sense 同构：读
//! `RR3_MANIFEST`（rrd spawn 注入，supervisor.rs）→ [`Rr3BusClient::rr3_connect`]
//! → 打开 `Rr3AudioSink` → 面板线程 → 事件循环。循环做两件事：
//! - `rr3_poll_wait` 收帧：订阅的 `rr3-brain/audio/tts_pcm`（MC-1 语音下行，流是
//!   载荷通路）逐帧解码进收流缓冲；订阅的 `rr3-connect/net/link` 事件快照进缓存
//!   （面板状态页数据源）；应答 exposes 的两个点名——
//!   `rr3-interact/tts/text`（对账 `pcm_bytes` 后排空缓冲播放，回
//!   `{"played_ms","chars"}`）与 `rr3-interact/face/expression`（表情机切换，回
//!   `{"expression","previous"}`）；
//! - 排空面板动作通道：face 由主循环串行代执行（面板 HTTP 线程不碰 sink/face
//!   所有权，与 TTS 点名天然互斥，无锁）；beep 占位已随 MC-1 退役，如实回
//!   业务错误"无本地波形源"；
//! - 表情帧动画节拍（批F3 sim 档）：~250ms 一拍推一帧，显示件随批F5a
//!   显示 trait 实装，sim 档只推帧指针。
//!
//! rr3.call 转发帧的 trace_id=调用方 task（rrd 路由语义），顺原路应答。
//! 业务性失败（缺参数、不认的表情、后端写失败）折成 error 应答回话；传输性
//! 失败（发布/应答写不出去）`eprintln` + `exit(1)`——模块不装"活着"，死交给
//! supervisor 按 restart_policy=always 重启（rrd 的进程语义）。

#[path = "../atoms/face/face.rs"]
mod face;
#[path = "panel/mod.rs"]
mod panel;
#[path = "../atoms/player/player.rs"]
mod player;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rr_bus::Rr3BusClient;
use rr_hal::backend::{rr3_hal_audio_sink, rr3_hal_display};
use rr_hal_api::RR3_AUDIO_CFG_16K_MONO16;
use rr_ipc::{opcode, Rr3PayloadType, RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};
use serde_json::{json, Value};

use face::{
    rr3_face_expression_lookup, rr3_face_frames, rr3_face_non_pointable_join,
    rr3_face_with_default, Rr3Face,
};
use panel::server::{rr3_panel_serve, Rr3PanelAction, Rr3PanelLimits, Rr3PanelShared};

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_INTERACT_CONFIG_ENV: &str = "RR3_INTERACT_CONFIG";

/// 面板状态页的链路卡片数据源（0.0.4 批C 只做订阅缓存，发布方 rr3-connect 随批D 上线）
const RR3_NET_LINK_TOPIC: &str = "rr3-connect/net/link";

/// 收帧等待上界：无点名时空转一圈的代价，值不敏感（心跳在独立线程）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

/// tts_pcm 流收缓冲（MC-1）：brain 语音下行**先流后点名**——流是载荷通路，
/// `rr3-interact/tts/text` 点名是"该不该播"的唯一判据（0.0.5 方案 §20 第 1 条
/// (a)）。Event 帧逐帧解码进缓冲，点名对账后排空播放；对账不过整缓冲作废
/// （残流不跨轮滞留）。
#[derive(Default)]
struct Rr3TtsPcmBuf {
    bytes: Vec<u8>,
}

impl Rr3TtsPcmBuf {
    /// 收一帧：`pcm_b64` 解码追加（STREAM 语义有序到达，帧序即字节序）。
    /// 解码失败上抛，由调用方弃帧——缺口由点名的对账兜住，不静默。
    fn rr3_tts_buf_push(&mut self, pcm_b64: &str) -> Result<(), rr_ipc::Rr3Base64Error> {
        self.bytes
            .extend_from_slice(&rr_ipc::rr3_base64_decode(pcm_b64)?);
        Ok(())
    }

    /// 缓冲实收字节数（对账的一侧）
    fn rr3_tts_buf_len(&self) -> usize {
        self.bytes.len()
    }

    /// 排空：取走全部字节，缓冲归零（残流不跨轮滞留）。
    fn rr3_tts_buf_take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

/// 组合件入口：装配原子件并跑主循环（原 main.rs `rr3_run` 逐字迁入，打印序列不变）。
pub fn rr3_interact_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    println!("rr3-interact 0.0.1 (M3 实装)");
    for line in face::SURFACE
        .iter()
        .copied()
        .chain(player::SURFACE.iter().copied())
    {
        println!("  {line}");
    }
    // panel 自述是推导文案（端点数由 RR3_PANEL_ROUTES 推导，MC-8③），String 面单独成段
    for line in panel::surface() {
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

    // config 装载（I-11 重开装载面）：不可点名集等策略经 config 数据化注入，机制件
    // 零策略载荷。缺文件/缺键/在册外值一律拒 boot（fail-closed）。
    let cfg = rr3_config_load(&manifest_path)?;
    // 注入的策略面在启动自述里可见（I-11：改数据即改行为，日志是第一证据）。
    // I-17/I-18② 新增键随批N4 件五进自述：节拍/兜底表情/面板并发放自检期望。
    println!(
        "config 就绪: face non_pointable={} tick_ms={} default={} player prebuffer_ms={} volume_pct={} panel max_conns={} selftest face_expect={}",
        rr3_face_non_pointable_join(&cfg.face.non_pointable),
        cfg.face.face_tick_ms,
        cfg.face.default_expr,
        cfg.player.prebuffer_ms,
        cfg.player.volume_pct,
        cfg.panel.max_conns,
        cfg.selftest.face_expect
    );

    let (mut sink, backend) = rr3_hal_audio_sink();
    sink.open(RR3_AUDIO_CFG_16K_MONO16)
        .map_err(|e| format!("打开播放面({backend})失败: {e}"))?;
    println!("播放面就绪: backend={backend} rate=16000 ch=1");

    // 兜底表情注入（批N4 件五 I-17/M-3）：mech 各回落点取 config
    // `default_expression`（parse 已对账白名单），legacy 缺键回落 idle
    let mut face = rr3_face_with_default(cfg.face.default_expr);
    let mut face_tick_at = std::time::Instant::now();

    // tts_pcm 收流缓冲（MC-1）：brain 先发流后点名，Event 帧逐帧解码进这里，
    // tts/text 点名对账后排空播放
    let mut tts_pcm = Rr3TtsPcmBuf::default();

    // 显示面选择 + 开机探活：读回当前帧，Unsupported 即致命——显式回错不静默
    // 退化（真件档板显式 linux 时 MAX7219 后端未实装，探活 exit(1) 是预期语义，
    // R11③）；sim 档走虚拟点阵内存记录，主循环出帧全链可见
    let (mut display, display_backend) = rr3_hal_display();
    display
        .frame()
        .map_err(|e| format!("显示面探活失败({display_backend}): {e}"))?;
    println!("显示面就绪: backend={display_backend} 8x8");

    // 面板（0.0.4 批C，Q3 裁定）：独立 HTTP 线程 + 动作通道。link 缓存是状态页
    // 卡片的数据源——面板线程只读、主循环只写；总线句柄 clone 是同一连接的另一
    // 份Arc 共享句柄（rr-bus 语义），面板的 rr3.call 与主循环应答互不干扰。
    let link = Arc::new(Mutex::new(None::<Value>));
    let (act_tx, act_rx) = mpsc::channel::<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>();
    // 面板五限额/超时（批N4 件五 I-18②）：config 毫秒值转 Duration，装配期
    // 一次换算，server 侧机制面只见 Duration 不再携带策略
    let panel_limits = Rr3PanelLimits {
        max_conns: cfg.panel.max_conns as usize,
        read_timeout: Duration::from_millis(cfg.panel.read_timeout_ms),
        header_limit: cfg.panel.header_limit as usize,
        body_limit: cfg.panel.body_limit as usize,
        call_timeout: Duration::from_millis(cfg.panel.call_timeout_ms),
    };
    let panel_shared = Arc::new(Rr3PanelShared {
        bus: bus.clone(),
        bus_path: std::env::var(RR3_BUS_PATH_ENV)
            .unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string()),
        actions: act_tx,
        link: Arc::clone(&link),
        limits: panel_limits,
        selftest_face_expect: cfg.selftest.face_expect,
    });
    std::thread::Builder::new()
        .name("rr3-panel".into())
        .spawn(move || {
            if let Err(e) = rr3_panel_serve(panel_shared) {
                eprintln!("面板服务退出: {e}");
            }
        })
        .map_err(|e| format!("起面板线程失败: {e}"))?;

    loop {
        // 总线死亡探针：断开后 poll_wait 立即返回空 Vec，与"暂时没帧"不可区分——
        // 不探测，总线一死本循环就满转成僵尸。假了就走"不装活着"的退出语义。
        if !bus.rr3_connected() {
            return Err("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定".into());
        }
        // 排空 inbox：订阅的 net/link 快照进缓存，点名 Request 走 rr3.call 语义——
        // 转发帧的 trace_id=调用方 task（rrd 路由语义），顺原路应答
        for frame in bus.rr3_poll_wait(RR3_POLL_WAIT) {
            if frame.header.opcode != opcode::SEND {
                continue;
            }
            let Ok((ty, body)) = Rr3BusClient::rr3_frame_body(&frame) else {
                continue;
            };
            match ty {
                Rr3PayloadType::Event => match body.get("topic").and_then(Value::as_str) {
                    Some(RR3_NET_LINK_TOPIC) => {
                        *link.lock().unwrap_or_else(|p| p.into_inner()) = Some(body);
                    }
                    Some(player::RR3_TTS_PCM_TOPIC) => {
                        // tts_pcm 收流（MC-1）：逐帧解码追加，帧数记账。坏帧
                        // （base64 非法）只弃该帧——缺口由 tts/text 点名的
                        // pcm_bytes 对账兜住折拒因，不静默。
                        if let Some(b64) = body.get("pcm_b64").and_then(Value::as_str) {
                            if tts_pcm.rr3_tts_buf_push(b64).is_ok() {
                                bus.rr3_counter_bump("tts_pcm_frames", 1);
                            }
                        }
                    }
                    _ => {}
                },
                Rr3PayloadType::Request => {
                    if body.get("method").and_then(Value::as_str) != Some(rr_bus::RR3_METHOD_CALL) {
                        continue;
                    }
                    let id = body.get("id").and_then(Value::as_u64).unwrap_or(0);
                    let params = body.get("params").cloned().unwrap_or(Value::Null);
                    let topic = params.get("topic").and_then(Value::as_str).unwrap_or("");
                    let args = params.get("args").cloned().unwrap_or(Value::Null);

                    // 业务性失败折成 error 应答（请求-应答语义），不当致命错误
                    let result = match topic {
                        player::RR3_TTS_TOPIC => rr3_handle_tts(
                            &bus,
                            sink.as_mut(),
                            &mut face,
                            &mut tts_pcm,
                            &args,
                            &cfg.player,
                        ),
                        face::RR3_FACE_TOPIC => {
                            rr3_handle_face(&mut face, &args, &cfg.face.non_pointable)
                        }
                        other => Err(format!(
                            "rr3-interact 只 exposes {}/{}，收到 {other}",
                            player::RR3_TTS_TOPIC,
                            face::RR3_FACE_TOPIC
                        )),
                    };
                    let result = result.unwrap_or_else(|e| json!({ "error": e }));
                    bus.rr3_reply_call(frame.header.trace_id, id, result)?;
                }
                _ => {}
            }
        }
        // 面板动作排空：face 主循环串行代执行（与 TTS 点名天然互斥，无锁）；
        // beep 占位已随 MC-1 退役——播放面只吃 tts_pcm 流，无本地波形源，如实回错
        while let Ok((action, tx)) = act_rx.try_recv() {
            let result = match action {
                Rr3PanelAction::Beep => {
                    Err("无本地波形源：beep 占位已随 MC-1 退役，播放面只接受 tts_pcm 流".into())
                }
                Rr3PanelAction::Face { expression } => face
                    .rr3_face_set(&expression, &cfg.face.non_pointable)
                    .map(|(c, p)| rr3_face_result(c, p)),
            };
            let _ = tx.send(result);
        }
        // 表情帧动画一拍（批F3 sim 档）：节拍自 config `face_tick_ms`
        // （批N4 件五 I-17/M-2 数据化，legacy 250ms）推进帧指针，多帧态
        // （blink/talk）由此循环；批F5a③ 接显示 trait——出帧交 HAL 渲染
        // （sim 后端内存记录，真件档 MAX7219 SPI 换后端零编排改动）。显示是
        // 妆面件，draw 失败只 eprintln 不退出（同 led worker 口径）
        if face_tick_at.elapsed() >= Duration::from_millis(cfg.face.face_tick_ms) {
            face_tick_at = std::time::Instant::now();
            let (expr, ix) = face.rr3_face_tick();
            let frame = rr3_face_frames(expr)[ix];
            display
                .draw(&frame)
                .unwrap_or_else(|e| eprintln!("rr3-interact-face: 出帧失败: {e}"));
        }
    }
}

/// TTS 点名（MC-1 重铺 / MC-2 播放面参数）：参数校验 → PCM 缓冲对账（半流/
/// 残留皆拒，拒因路径同样清缓冲）→ 起播水位判定（fail-closed）→ 表情切 talk
/// → 样点增益 → 排空缓冲播放 → 恢复表情 → 回 played_ms。
/// 记账：拒 `tts_play_reject`、播成 `tts_play_ok`（收流帧另有 `tts_pcm_frames`）。
#[allow(clippy::too_many_arguments)]
fn rr3_handle_tts(
    bus: &Rr3BusClient,
    sink: &mut dyn rr_hal_api::Rr3AudioSink,
    face: &mut Rr3Face,
    tts_pcm: &mut Rr3TtsPcmBuf,
    args: &Value,
    player: &Rr3PlayerCfg,
) -> Result<Value, String> {
    let (text, pcm_bytes) = rr3_tts_parse_args(args)?;
    if let Err(e) = rr3_tts_pcm_integrity(pcm_bytes, tts_pcm.rr3_tts_buf_len()) {
        bus.rr3_counter_bump("tts_play_reject", 1);
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
        bus.rr3_counter_bump("tts_play_reject", 1);
        return Err(format!(
            "起播水位未达：缓冲 {total_ms}ms < 水位 {}ms（prebuffer_ms），弃播",
            player.prebuffer_ms
        ));
    }
    // 播放增益（MC-2）：config 注入的 volume_pct 整型饱和缩放，播放前就地生效
    player::rr3_tts_apply_gain(&mut samples, player.volume_pct);
    let suspended = face.rr3_face_begin_talk();
    let played = player::rr3_tts_play(sink, &samples);
    face.rr3_face_end_talk();
    let played_ms = played?;
    bus.rr3_counter_bump("tts_play_ok", 1);
    println!(
        "tts: {} 字符播了 {played_ms}ms（PCM {pcm_bytes} 字节，gain {}%，水位 {}ms，talk 挂起 {suspended}）",
        text.chars().count(),
        player.volume_pct,
        player.prebuffer_ms
    );
    Ok(rr3_tts_result(&text, played_ms))
}

/// 表情点名：校验并切换，回 `{"expression","previous"}`。`non_pointable` = 本形态
/// 不可点名集（I-11：config 注入，透传给件面判定）。
fn rr3_handle_face(
    face: &mut Rr3Face,
    args: &Value,
    non_pointable: &[&str],
) -> Result<Value, String> {
    let expr = rr3_face_parse_args(args)?;
    let (current, previous) = face.rr3_face_set(&expr, non_pointable)?;
    Ok(rr3_face_result(current, previous))
}

// ---------- 载荷组装面（0.0.5 批I I-8：上移承载层，ADR-0019 修订 4） ----------
// 件的对外契约面 = 结构体 + 判定函数；点名下钻（parse_args）与应答体（result）
// 的 Value 生产归本文件，线上字节逐字不变（用例随迁继续守字节面）。

/// TTS 点名下钻（MC-1）：`text` 必填且须非空字符串；`pcm_bytes` 必填——本轮
/// PCM 的字节总数，播放面对账收流缓冲完整性的唯一判据（无流不静默）。校验
/// 失败返回 `Err`，由调用方折成 error 应答（请求-应答语义，不当致命错误）。
fn rr3_tts_parse_args(args: &Value) -> Result<(String, u64), String> {
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
/// 字节。判定纯函数化：记账（tts_play_reject）与清缓冲归调用方。
fn rr3_tts_pcm_integrity(pcm_bytes: u64, buffered: usize) -> Result<(), String> {
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
fn rr3_pcm_bytes_to_samples(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]))
        .collect()
}

/// `rr3-interact/tts/text` 的应答体：实际播出的毫秒数与计费字符数
/// （played_ms 由实收样点导出，与字符数脱钩，一并回给调用方对表）。
fn rr3_tts_result(text: &str, played_ms: u64) -> Value {
    json!({ "played_ms": played_ms, "chars": text.chars().count() })
}

/// 表情点名下钻：`expression` 必填。合法性交给 [`face::Rr3Face::rr3_face_set`]
/// （错误信息里带白名单，这里不做两遍）。
fn rr3_face_parse_args(args: &Value) -> Result<String, String> {
    args.get("expression")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{} 缺 string 参数 expression", face::RR3_FACE_TOPIC))
}

/// `rr3-interact/face/expression` 的应答体：切换后的表情与切换前的表情。
fn rr3_face_result(current: &str, previous: &str) -> Value {
    json!({ "expression": current, "previous": previous })
}

// ---------- config 装载面（I-11：重开装载面） ----------
// interact 自 0.0.5 批I I-1 起不装载 config（空壳）；MC-10 I-11 把"不可点名规则"
// 从件内 `if` 迁为 config 数据，本模块自此**装载 config.toml**（缺文件开机即退，
// fail-closed，同 act/sense 口径）。字面值（表情名）仍是契约面，可注入的是集合。

/// config.toml 解析：env 覆盖优先，缺省 manifest 同目录。
fn rr3_config_load(manifest_path: &str) -> Result<Rr3InteractCfg, String> {
    let path = std::env::var(RR3_INTERACT_CONFIG_ENV).unwrap_or_else(|_| {
        std::path::Path::new(manifest_path)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("config.toml")
            .to_string_lossy()
            .into_owned()
    });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读 config {path} 失败: {e}"))?;
    rr3_interact_config_parse(&text)
}

/// 本模块 config 的汇总形状
#[derive(Debug, Clone)]
struct Rr3InteractCfg {
    player: Rr3PlayerCfg,
    face: Rr3FaceCfg,
    panel: Rr3PanelCfg,
    selftest: Rr3SelftestCfg,
}

/// config `[rr3-interact.player]` 的形状（MC-2）：起播水位与播放增益。
/// 缺键/缺段回落默认（同 sense S-5 口径：代码默认是文件缺键的回落）；
/// 类型错/负数/越界显式拒（fail-closed，不静默吞）。
#[derive(Debug, Clone)]
struct Rr3PlayerCfg {
    prebuffer_ms: u64,
    volume_pct: u32,
}

impl Default for Rr3PlayerCfg {
    fn default() -> Self {
        // legacy 0cf029e 调优值（0.5s 预缓冲抗抖动、70% 音量）——一个事实
        // 一个真值点：默认值只在此处与 config.toml 注释对表
        Self {
            prebuffer_ms: 500,
            volume_pct: 70,
        }
    }
}

/// config `[rr3-interact.face]` 的形状（`non_pointable` = 本形态不可点名集，I-11）。
/// 值取 `&'static str`（经白名单对账），件面判定直接吃 `&[&str]`。批N4 件五
/// I-17/M-2/M-3：`face_tick_ms` = 表情节拍（组合件主循环节步频）与
/// `default_expr` = 兜底表情（mech 状态机各回落点）数据化——节拍与人格取向
/// 都是策略，缺键回落 legacy 值。
#[derive(Debug, Clone)]
struct Rr3FaceCfg {
    non_pointable: Vec<&'static str>,
    face_tick_ms: u64,
    default_expr: &'static str,
}

/// config `[rr3-interact.panel]` 的形状（批N4 件五 I-18②）：面板五限额/超时
/// 数据化——面板是"覆盖率门缺失"的活证（I-15 分子分母数不到它们），本批补齐。
/// 缺段/缺键回落默认（legacy 常量值，S-5 口径）；类型错/负数/越界点名拒 boot。
#[derive(Debug, Clone)]
struct Rr3PanelCfg {
    max_conns: u64,
    read_timeout_ms: u64,
    header_limit: u64,
    body_limit: u64,
    call_timeout_ms: u64,
}

impl Default for Rr3PanelCfg {
    fn default() -> Self {
        // legacy 常量值（server.rs 原 RR3_PANEL_* 常量）——一个事实一个真值点：
        // 默认值只在此处与 config.toml 注释对表
        Self {
            max_conns: 8,
            read_timeout_ms: 5000,
            header_limit: 8 * 1024,
            body_limit: 4 * 1024,
            call_timeout_ms: 5000,
        }
    }
}

/// config `[rr3-interact.selftest]` 的形状（批N4 件五 I-17/M-10）：自检期望
/// 表情是产线取向，可按形态改数据。值必须在册（白名单对账）。
#[derive(Debug, Clone)]
struct Rr3SelftestCfg {
    face_expect: &'static str,
}

impl Default for Rr3SelftestCfg {
    fn default() -> Self {
        Self { face_expect: "happy" }
    }
}

/// TOML 文本 → Rr3InteractCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读。
fn rr3_interact_config_parse(text: &str) -> Result<Rr3InteractCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    // player 段（MC-2）：段/键缺省回落默认（S-5 口径）；类型错或越界逐键点名
    // 拒 boot（fail-closed）。volume_pct 值域 0..=1000（10 倍封顶防呆，>100
    // 为放大、样点饱和由机制面夹断）。
    let mut player = Rr3PlayerCfg::default();
    if let Some(p) = v.get("rr3-interact").and_then(|s| s.get("player")) {
        let mut t = player.prebuffer_ms;
        rr3_cfg_u64(p, "prebuffer_ms", "player", &mut t)?;
        player.prebuffer_ms = t;
        let mut t = player.volume_pct as u64;
        rr3_cfg_u64(p, "volume_pct", "player", &mut t)?;
        player.volume_pct = u32::try_from(t)
            .ok()
            .filter(|pct| *pct <= 1000)
            .ok_or_else(|| {
                format!("[rr3-interact.player] volume_pct 越界（值域 0..=1000）: {t}")
            })?;
    }
    // panel 段（批N4 件五 I-18②）：段/键缺省回落 legacy 常量值（S-5 口径）；
    // 类型错/负数点名拒。四值下界：max_conns ≥1、read_timeout_ms ≥1、
    // header/body_limit ≥1（0 = 面板自残，不当合法配置放行）。
    let mut panel = Rr3PanelCfg::default();
    if let Some(p) = v.get("rr3-interact").and_then(|s| s.get("panel")) {
        let mut t = panel.max_conns;
        rr3_cfg_u64(p, "max_conns", "panel", &mut t)?;
        panel.max_conns = t.max(1);
        let mut t = panel.read_timeout_ms;
        rr3_cfg_u64(p, "read_timeout_ms", "panel", &mut t)?;
        panel.read_timeout_ms = t.max(1);
        let mut t = panel.header_limit;
        rr3_cfg_u64(p, "header_limit", "panel", &mut t)?;
        panel.header_limit = t.max(1);
        let mut t = panel.body_limit;
        rr3_cfg_u64(p, "body_limit", "panel", &mut t)?;
        panel.body_limit = t.max(1);
        let mut t = panel.call_timeout_ms;
        rr3_cfg_u64(p, "call_timeout_ms", "panel", &mut t)?;
        panel.call_timeout_ms = t.max(1);
    }
    // selftest 段（批N4 件五 I-17/M-10）：段缺省回落 legacy 期望（"happy"）；
    // 值必须在册（同 non_pointable 白名单对账口径），在册外点名拒 boot。
    let mut selftest = Rr3SelftestCfg::default();
    if let Some(s) = v.get("rr3-interact").and_then(|x| x.get("selftest")) {
        if let Some(name) = s.get("face_expect").and_then(toml::Value::as_str) {
            selftest.face_expect = rr3_face_expression_lookup(name).ok_or_else(|| {
                format!(
                    "config [rr3-interact.selftest] face_expect 非在册表情 {name:?}（在册值 {}）",
                    face::RR3_FACE_EXPRESSIONS.join("/")
                )
            })?;
        }
    }
    let face_sect = v
        .get("rr3-interact")
        .and_then(|s| s.get("face"))
        .ok_or_else(|| "config 缺 [rr3-interact.face] 段".to_string())?;
    // 不可点名集（I-11）：数组逐元素必须是**在册契约值**，非在册名或非字符串
    // 逐条点名拒 boot（fail-closed）；键在而数组为空 = 本形态不排除任何表情点名（合法）
    let arr = face_sect
        .get("non_pointable")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            "config [rr3-interact.face] 缺 non_pointable（不可点名表情数组）".to_string()
        })?;
    let mut non_pointable = Vec::with_capacity(arr.len());
    for (i, e) in arr.iter().enumerate() {
        let s = e.as_str().ok_or_else(|| {
            format!("config [rr3-interact.face] non_pointable[{i}] 必须是字符串，收到 {e}")
        })?;
        let expr = rr3_face_expression_lookup(s).ok_or_else(|| {
            format!(
                "config [rr3-interact.face] non_pointable[{i}] 非在册表情 {s:?}（在册值 {}）",
                face::RR3_FACE_EXPRESSIONS.join("/")
            )
        })?;
        non_pointable.push(expr);
    }
    // 表情节拍与兜底表情（批N4 件五 I-17/M-2/M-3）：缺键回落 legacy 值；
    // default_expression 必须在册（同 non_pointable 白名单对账）
    let mut face_tick_ms = 250u64;
    rr3_cfg_u64(face_sect, "face_tick_ms", "face", &mut face_tick_ms)?;
    let face_tick_ms = face_tick_ms.max(1);
    let default_expr = match face_sect.get("default_expression") {
        Some(name) => {
            let s = name.as_str().ok_or_else(|| {
                format!("config [rr3-interact.face] default_expression 必须是字符串，收到 {name}")
            })?;
            rr3_face_expression_lookup(s).ok_or_else(|| {
                format!(
                    "config [rr3-interact.face] default_expression 非在册表情 {s:?}（在册值 {}）",
                    face::RR3_FACE_EXPRESSIONS.join("/")
                )
            })?
        }
        None => rr3_face_expression_lookup("idle")
            .expect("在册白名单恒含 idle，lookup 必中"),
    };
    Ok(Rr3InteractCfg {
        player,
        face: Rr3FaceCfg {
            non_pointable,
            face_tick_ms,
            default_expr,
        },
        panel,
        selftest,
    })
}

/// config 键的 u64 下钻（S-5 同款口径）：键缺省不动 `out`（回落调用方给的
/// 默认）；类型错（非整数/负数）显式点名拒 boot，不静默吞。段名进文案便于
/// 逐段定位（player/face/panel 各段共用）。
fn rr3_cfg_u64(
    sect: &toml::Value,
    key: &str,
    sect_name: &str,
    out: &mut u64,
) -> Result<(), String> {
    let Some(v) = sect.get(key) else {
        return Ok(());
    };
    let t = v.as_integer().ok_or_else(|| {
        format!("[rr3-interact.{sect_name}] {key} 必须是整数，收到 {v}")
    })?;
    *out = u64::try_from(t)
        .map_err(|_| format!("[rr3-interact.{sect_name}] {key} 不得为负: {t}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// tts PCM 缓冲对账（MC-1）：完整 = 过；半流/残留 = 拒且缺口字节进拒因
    #[test]
    fn tts_pcm_integrity_rejects_half_and_residual_streams() {
        assert_eq!(rr3_tts_pcm_integrity(640, 640), Ok(()));
        let e = rr3_tts_pcm_integrity(1920, 640).unwrap_err();
        assert!(e.contains("半流") && e.contains("1280"), "{e}");
        let e = rr3_tts_pcm_integrity(640, 1280).unwrap_err();
        assert!(e.contains("残留") && e.contains("640"), "{e}");
        let e = rr3_tts_pcm_integrity(1920, 0).unwrap_err();
        assert!(e.contains("半流"), "空缓冲必拒（无流不静默）: {e}");
    }

    /// 收流缓冲：解码追加 + 排空清零（残流不跨轮滞留），坏帧 fail-closed
    #[test]
    fn tts_pcm_buf_pushes_frames_and_takes_all() {
        let mut buf = Rr3TtsPcmBuf::default();
        buf.rr3_tts_buf_push(&rr_ipc::rr3_base64_encode(&[1, 2, 3, 4]))
            .unwrap();
        buf.rr3_tts_buf_push(&rr_ipc::rr3_base64_encode(&[5, 6]))
            .unwrap();
        assert_eq!(buf.rr3_tts_buf_len(), 6);
        assert_eq!(buf.rr3_tts_buf_take(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(buf.rr3_tts_buf_len(), 0, "排空后缓冲清零");
        assert!(
            buf.rr3_tts_buf_push("not-b64!!").is_err(),
            "坏帧 fail-closed"
        );
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

    /// 仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-interact.player]
prebuffer_ms = 500
volume_pct = 70

[rr3-interact.face]
non_pointable = ["talk"]
face_tick_ms = 250
default_expression = "idle"

[rr3-interact.panel]
max_conns = 8
read_timeout_ms = 5000
header_limit = 8192
body_limit = 4096
call_timeout_ms = 5000

[rr3-interact.selftest]
face_expect = "happy"
"#;
        let cfg = rr3_interact_config_parse(text).unwrap();
        assert_eq!(cfg.face.non_pointable, vec!["talk"], "不可点名集注入");
        assert_eq!(cfg.player.prebuffer_ms, 500, "起播水位注入（MC-2）");
        assert_eq!(cfg.player.volume_pct, 70, "播放增益注入（MC-2）");
        assert_eq!(cfg.face.face_tick_ms, 250, "表情节拍注入（I-17/M-2）");
        assert_eq!(cfg.face.default_expr, "idle", "兜底表情注入（I-17/M-3）");
        assert_eq!(cfg.panel.max_conns, 8, "面板限额注入（I-18②）");
        assert_eq!(cfg.selftest.face_expect, "happy", "自检期望注入（I-17/M-10）");
    }

    /// face/panel/selftest 新键（批N4 件五 I-17/I-18②）：键/段缺省回落 legacy
    /// 值；类型错、在册外值、0 值逐键点名拒（fail-closed）
    #[test]
    fn face_panel_selftest_keys_fall_back_and_reject_bad_values() {
        let minimal = "[rr3-interact.face]\nnon_pointable = []\n";
        // 全缺省：face 节拍/兜底回落 250/idle，panel 五值回落 legacy，
        // selftest 回落 happy
        let cfg = rr3_interact_config_parse(minimal).unwrap();
        assert_eq!(cfg.face.face_tick_ms, 250);
        assert_eq!(cfg.face.default_expr, "idle");
        assert_eq!(cfg.panel.max_conns, 8);
        assert_eq!(cfg.panel.read_timeout_ms, 5000);
        assert_eq!(cfg.panel.header_limit, 8 * 1024);
        assert_eq!(cfg.panel.body_limit, 4 * 1024);
        assert_eq!(cfg.panel.call_timeout_ms, 5000);
        assert_eq!(cfg.selftest.face_expect, "happy");
        // 显式值合法读入：非 legacy 取值同样放行（在册表情任选）
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = 120\ndefault_expression = \"happy\"\n[rr3-interact.panel]\nmax_conns = 2\n[rr3-interact.selftest]\nface_expect = \"blink\"\n",
        )
        .unwrap();
        assert_eq!(cfg.face.face_tick_ms, 120);
        assert_eq!(cfg.face.default_expr, "happy");
        assert_eq!(cfg.panel.max_conns, 2);
        assert_eq!(cfg.selftest.face_expect, "blink");
        // 0 值自残拒放：节拍/限额折 1 下界——0 不当合法配置
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = 0\n[rr3-interact.panel]\nmax_conns = 0\n",
        )
        .unwrap();
        assert_eq!(cfg.face.face_tick_ms, 1, "节拍 0 折 1");
        assert_eq!(cfg.panel.max_conns, 1, "并发 0 折 1");
        // 类型错拒：字符串进整数键、整数进字符串键
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\nface_tick_ms = \"soon\"\n",
        )
        .unwrap_err();
        assert!(e.contains("face_tick_ms"), "类型错点名: {e}");
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\ndefault_expression = 3\n",
        )
        .unwrap_err();
        assert!(e.contains("default_expression"), "非字符串点名: {e}");
        // 在册外值拒：表情名必须过白名单对账
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\ndefault_expression = \"wink\"\n",
        )
        .unwrap_err();
        assert!(e.contains("非在册表情"), "兜底表情在册外点名: {e}");
        let e = rr3_interact_config_parse(
            "[rr3-interact.face]\nnon_pointable = []\n[rr3-interact.selftest]\nface_expect = \"wink\"\n",
        )
        .unwrap_err();
        assert!(e.contains("非在册表情"), "自检期望在册外点名: {e}");
    }

    /// player 两键（MC-2）：键/段缺省回落默认；类型错、负数、越界逐键点名拒
    #[test]
    fn player_keys_fall_back_to_defaults_and_reject_bad_values() {
        // 整段缺省（I-11 最小形状）→ 回落 legacy 默认 500/70
        let cfg = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = []\n").unwrap();
        assert_eq!(cfg.player.prebuffer_ms, 500, "缺段回落默认水位");
        assert_eq!(cfg.player.volume_pct, 70, "缺段回落默认增益");
        // 段在键缺 → 同样回落
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.player]\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap();
        assert_eq!(cfg.player.prebuffer_ms, 500);
        assert_eq!(cfg.player.volume_pct, 70);
        // 越界策略值合法读入：水位 0（无预缓冲）与增益 1000（封顶）
        let cfg = rr3_interact_config_parse(
            "[rr3-interact.player]\nprebuffer_ms = 0\nvolume_pct = 1000\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap();
        assert_eq!(cfg.player.prebuffer_ms, 0);
        assert_eq!(cfg.player.volume_pct, 1000);
        // 类型错拒：字符串进整数键
        let e = rr3_interact_config_parse(
            "[rr3-interact.player]\nprebuffer_ms = \"soon\"\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap_err();
        assert!(e.contains("prebuffer_ms"), "类型错点名: {e}");
        // 负数拒：水位不得为负
        let e = rr3_interact_config_parse(
            "[rr3-interact.player]\nprebuffer_ms = -1\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap_err();
        assert!(e.contains("不得为负"), "负数点名: {e}");
        // 增益越界拒：值域 0..=1000
        let e = rr3_interact_config_parse(
            "[rr3-interact.player]\nvolume_pct = 1001\n[rr3-interact.face]\nnon_pointable = []\n",
        )
        .unwrap_err();
        assert!(e.contains("volume_pct 越界"), "增益越界点名: {e}");
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_interact_config_parse("").unwrap_err();
        assert!(e.contains("[rr3-interact.face]"), "缺段点名: {e}");
        let e = rr3_interact_config_parse("[rr3-interact.face]\n").unwrap_err();
        assert!(e.contains("non_pointable"), "缺键点名: {e}");
        let e = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = 3\n").unwrap_err();
        assert!(e.contains("non_pointable"), "非数组点名: {e}");
        let e = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = [\"wink\"]\n")
            .unwrap_err();
        assert!(e.contains("非在册"), "在册外值点名: {e}");
        // 空数组合法：本形态不排除任何表情点名（fail-closed 的键必填 + 空值合法）
        let cfg = rr3_interact_config_parse("[rr3-interact.face]\nnon_pointable = []\n").unwrap();
        assert!(cfg.face.non_pointable.is_empty());
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
}
