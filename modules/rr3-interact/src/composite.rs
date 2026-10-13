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
//! - `rr3_poll_wait` 收帧，应答本模块 exposes 的两个点名——
//!   `rr3-interact/tts/text`（占位正弦 burst，回 `{"played_ms","chars"}`）与
//!   `rr3-interact/face/expression`（表情机切换，回 `{"expression","previous"}`）；
//!   订阅的 `rr3-connect/net/link` 事件快照进缓存（面板状态页数据源）；
//! - 排空面板动作通道：beep/face 由主循环串行代执行（面板 HTTP 线程不碰
//!   sink/face 所有权，与 TTS 点名天然互斥，无锁）；
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
use rr_hal::{backend::rr3_hal_audio_sink, backend::rr3_hal_display, RR3_AUDIO_CFG_16K_MONO16};
use rr_ipc::{opcode, Rr3PayloadType, RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};
use serde_json::{json, Value};

use face::{rr3_face_expression_lookup, rr3_face_frames, rr3_face_non_pointable_join, Rr3Face};
use panel::server::{rr3_panel_serve, Rr3PanelAction, Rr3PanelShared};

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_INTERACT_CONFIG_ENV: &str = "RR3_INTERACT_CONFIG";

/// 面板状态页的链路卡片数据源（0.0.4 批C 只做订阅缓存，发布方 rr3-connect 随批D 上线）
const RR3_NET_LINK_TOPIC: &str = "rr3-connect/net/link";

/// 收帧等待上界：无点名时空转一圈的代价，值不敏感（心跳在独立线程）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

/// 表情帧动画节拍：~250ms 一帧（批F3 sim 档；多帧态 blink/talk 由此循环）。
/// 按墙上时钟节拍推进，帧速率不随总线流量漂移。
const RR3_FACE_TICK: Duration = Duration::from_millis(250);

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
    // 注入的策略面在启动自述里可见（I-11：改数据即改行为，日志是第一证据）
    println!(
        "config 就绪: face non_pointable={}",
        rr3_face_non_pointable_join(&cfg.face.non_pointable)
    );

    let (mut sink, backend) = rr3_hal_audio_sink();
    sink.open(RR3_AUDIO_CFG_16K_MONO16)
        .map_err(|e| format!("打开播放面({backend})失败: {e}"))?;
    println!("播放面就绪: backend={backend} rate=16000 ch=1");

    let mut face = Rr3Face::default();
    let mut face_tick_at = std::time::Instant::now();

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
    let panel_shared = Arc::new(Rr3PanelShared {
        bus: bus.clone(),
        bus_path: std::env::var(RR3_BUS_PATH_ENV)
            .unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string()),
        actions: act_tx,
        link: Arc::clone(&link),
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
                Rr3PayloadType::Event => {
                    if body.get("topic").and_then(Value::as_str) == Some(RR3_NET_LINK_TOPIC) {
                        *link.lock().unwrap_or_else(|p| p.into_inner()) = Some(body);
                    }
                }
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
                        player::RR3_TTS_TOPIC => {
                            rr3_handle_tts(&bus, sink.as_mut(), &mut face, &args)
                        }
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
        // 面板动作排空：beep/face 主循环串行代执行（与 TTS 点名天然互斥，无锁）
        while let Ok((action, tx)) = act_rx.try_recv() {
            let result = match action {
                Rr3PanelAction::Beep => {
                    player::rr3_tts_play(sink.as_mut(), "beep").map(|ms| json!({ "played_ms": ms }))
                }
                Rr3PanelAction::Face { expression } => face
                    .rr3_face_set(&expression, &cfg.face.non_pointable)
                    .map(|(c, p)| rr3_face_result(c, p)),
            };
            let _ = tx.send(result);
        }
        // 表情帧动画一拍（批F3 sim 档）：~250ms 节拍推进帧指针，多帧态
        // （blink/talk）由此循环；批F5a③ 接显示 trait——出帧交 HAL 渲染
        // （sim 后端内存记录，真件档 MAX7219 SPI 换后端零编排改动）。显示是
        // 妆面件，draw 失败只 eprintln 不退出（同 led worker 口径）
        if face_tick_at.elapsed() >= RR3_FACE_TICK {
            face_tick_at = std::time::Instant::now();
            let (expr, ix) = face.rr3_face_tick();
            let frame = rr3_face_frames(expr)[ix];
            display
                .draw(&frame)
                .unwrap_or_else(|e| eprintln!("rr3-interact-face: 出帧失败: {e}"));
        }
    }
}

/// TTS 点名：校验参数 → 表情切 talk → 播放 → 恢复表情 → 回 played_ms。
fn rr3_handle_tts(
    _bus: &Rr3BusClient,
    sink: &mut dyn rr_hal::Rr3AudioSink,
    face: &mut Rr3Face,
    args: &Value,
) -> Result<Value, String> {
    let text = rr3_tts_parse_args(args)?;
    let suspended = face.rr3_face_begin_talk();
    let played = player::rr3_tts_play(sink, &text);
    face.rr3_face_end_talk();
    let played_ms = played?;
    println!(
        "tts: {} 字符播了 {played_ms}ms（talk 挂起 {suspended}）",
        text.chars().count()
    );
    Ok(rr3_tts_result(&text, played_ms))
}

/// 表情点名：校验并切换，回 `{"expression","previous"}`。`non_pointable` = 本形态
/// 不可点名集（I-11：config 注入，透传给件面判定）。
fn rr3_handle_face(face: &mut Rr3Face, args: &Value, non_pointable: &[&str]) -> Result<Value, String> {
    let expr = rr3_face_parse_args(args)?;
    let (current, previous) = face.rr3_face_set(&expr, non_pointable)?;
    Ok(rr3_face_result(current, previous))
}

// ---------- 载荷组装面（0.0.5 批I I-8：上移承载层，ADR-0019 修订 4） ----------
// 件的对外契约面 = 结构体 + 判定函数；点名下钻（parse_args）与应答体（result）
// 的 Value 生产归本文件，线上字节逐字不变（用例随迁继续守字节面）。

/// TTS 点名下钻：`text` 必填且须非空字符串。校验失败返回 `Err`，由调用方折成
/// error 应答（请求-应答语义，不当致命错误）。
fn rr3_tts_parse_args(args: &Value) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} 缺 string 参数 text", player::RR3_TTS_TOPIC))?;
    if text.is_empty() {
        return Err(format!("{} 的 text 不能为空", player::RR3_TTS_TOPIC));
    }
    Ok(text.to_string())
}

/// `rr3-interact/tts/text` 的应答体：实际播出的毫秒数与计费字符数
/// （封顶后 played_ms < chars×60 时两者对不上，一并回给调用方对表）。
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
    face: Rr3FaceCfg,
}

/// config `[rr3-interact.face]` 的形状（`non_pointable` = 本形态不可点名集，I-11）。
/// 值取 `&'static str`（经白名单对账），件面判定直接吃 `&[&str]`。
#[derive(Debug, Clone)]
struct Rr3FaceCfg {
    non_pointable: Vec<&'static str>,
}

/// TOML 文本 → Rr3InteractCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读。
fn rr3_interact_config_parse(text: &str) -> Result<Rr3InteractCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
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
    Ok(Rr3InteractCfg {
        face: Rr3FaceCfg { non_pointable },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 点名下钻与应答体形状（用例自件面随迁，I-8：承载层守字节面）。
    #[test]
    fn args_and_result_shapes() {
        assert_eq!(rr3_tts_parse_args(&json!({ "text": "hi" })).unwrap(), "hi");
        for bad in [
            json!({}),
            json!({ "text": 3 }),
            json!({ "text": "" }),
            json!({ "text": null }),
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

    /// 仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-interact.face]
non_pointable = ["talk"]
"#;
        let cfg = rr3_interact_config_parse(text).unwrap();
        assert_eq!(cfg.face.non_pointable, vec!["talk"], "不可点名集注入");
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
