//! ESP32 外设桥后端（0.0.5 增补轮五 批C）：舵机/显示两能力面的 UART 能力帧承载。
//!
//! 路线二裁定（批C，Q1–Q4 对表）：板上外设**不进城**——不注册总线身份，宿主 HAL
//! 桥后端直连 UART。"进城"读作"进传输层帧面 + 在册主题形状"：帧面 JSON-RPC
//! `method` 恒 `rr3.call`、点名主题在 `params.topic`（conn.rs rr3_route_call 转发
//! 形状，哨兵 handle_request 同款分发依据），主题复用在册
//! `rr3-act/servo/command` 与 `rr3-interact/face/expression`，payload 形状与模块
//! 应答体逐字同源（contracts/events/act-servo_command.schema.json、
//! events/interact-face_expression.schema.json）——topics.toml/rrd/桥三个已收官
//! 面零改动（`topics.toml` 全轮只许 P1 那一次改动的防重规则不受扰动）。
//!
//! 连接模型：**长连接**。首调 `Rr3UartConn::open_with_baud`（rr-ipc 排他开口：
//! flock 账本，跨进程第二个 opener 当场被拒——两个模块进程同抢一块板是 fail-closed
//! 点名，不是静默截流；open 自带 TCIFLUSH 开口即净，批D1 敞口的正式处置）。
//! macOS CP2102 的 DTR/RTS 翻转会复位板子（E7 实锤），open 后等
//! [`RR3_OPEN_SETTLE_MS`] 启动窗口再发首帧。io 失败 drop 连接（close + flock 退账）
//! 下次调用重开。读侧是阻塞语义：板侧对合法 Request 确定性应答，拔线时写侧先报
//! 错——"板死无应答则本侧阻塞"是已知敞口（rr3-bridge 中继批E 统一收口，本档
//! 不在 rr-ipc 加读超时机制）。
//!
//! task 位形（帧头约定）：宿主下行 `src=1`（rrd 位形，与 `rr-core::RR3_TASK_RRD`
//! 同值——机制真值单点在 rr-core，本 crate 不加依赖边、本地锚定常量）；板上
//! `dst=2`（外设执行面位形，与首模块池位形同值，[`RR3_PERIPH_TASK`]）。应答帧
//! 板侧回 `src=2/dst=1`、`trace_id` 原样回带，本侧按 `dst==1` 门 + id 回显校验。
//!
//! 慢速事件级接口纪律（ADR-0007）：set_angle/draw 都是 round-trip ~26ms 量级
//! （115200 下 300B 往返），远低于 100Hz 事务上限，不碰反射环边界。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_uart_baud_speed, Rr3Frame, Rr3FrameBuf, Rr3Header,
    Rr3PayloadType, Rr3TransportConn, Rr3UartConn, RR3_BOARD_BINDING_ADC_RAIL_PIN,
    RR3_BOARD_BINDING_I2C_SCL, RR3_BOARD_BINDING_I2C_SDA, RR3_METHOD_SET_BINDINGS,
    RR3_UART_BAUD_DEFAULT,
};
use serde_json::{json, Map, Value};

use crate::descriptor::{rr3_hal_bindings_from_text, Rr3HalBindings, RR3_HAL_BINDINGS_ENV};
use crate::{Rr3Display, Rr3FaceFrame, Rr3HalError, Rr3PwmServo};

/// 板上外设执行面的 task 号（批C 帧头约定：外设固件帧面占首模块池位形 2；
/// 不进城，无 registry 冲突面。与 rr-core::RR3_TASK_FIRST_MODULE 同值——真值
/// 单点在 rr-core，本 crate 不加依赖边、本地锚定）。
pub const RR3_PERIPH_TASK: u64 = 2;

/// 宿主下行 task 号（rrd 位形；与 rr-core::RR3_TASK_RRD 同值，锚定注释见上）。
const RR3_TASK_HOST: u64 = 1;

/// 直连 trace（与 rr-core::RR3_TRACE_ID_DIRECT 同值 0——桥后端不经 rrd 路由）。
const RR3_TRACE_DIRECT: u64 = 0;

/// 桥后端**缺省**波特率：保持 115200（Q1=(a) 裁定——宿主缺省不变，高速档只
/// 显式选择）。板固件自批D② 起钉 921600（带宽账：音频上行需 >512kbps），会话
/// 须以 [`RR3_HAL_PERIPH_BAUD_ENV`] 或 `_with_baud` 构造显式同档；两侧档位不
/// 一致即线路乱码（fail-visible：帧层 BadMagic 当场点名，不静默）。
pub const RR3_BRIDGE_BAUD: u32 = RR3_UART_BAUD_DEFAULT;

/// open 后的启动窗口：DTR/RTS 翻转复位板子（E7），ESP32 上电到帧面就绪的
/// 等待（esp-idf 启动 + OLED init 的实测时长量级，1.2s 留裕量）。
const RR3_OPEN_SETTLE_MS: u64 = 1200;

/// ESP32 档设备路径 env 键（应急覆盖口径与 [`crate::backend`] 其余键一致；
/// 描述符键 `periph_uart` 随真件装配批补入 profiles——本档先落 env 正门）。
pub const RR3_HAL_PERIPH_UART_ENV: &str = "RR3_HAL_PERIPH_UART";

/// ESP32 档波特率 env 键（批D②）：取值白名单见 [`rr3_periph_baud_parse`]，缺省
/// [`RR3_BRIDGE_BAUD`]（115200）。板固件已钉 921600，会话须显式给 921600。
pub const RR3_HAL_PERIPH_BAUD_ENV: &str = "RR3_HAL_PERIPH_BAUD";

/// ESP32 档舵机轴 env 键：`pan` / `tilt`（板上双轴云台 ↔ 宿主单轴 trait 的
/// 映射面，缺省 `pan`；板侧帧面另有 `axis` 参数可逐调用覆盖）。真值单点已上移
/// 契约层（rr-hal-api `RR3_PERIPH_AXIS_ENV`，批N3 件五——rrd 装配链同键选轴档案），
/// 本地 re-export 保既有路径。
pub use rr_hal_api::RR3_PERIPH_AXIS_ENV as RR3_HAL_PERIPH_AXIS_ENV;

/// 舵机点名主题（在册 request L1；payload 形状事实源
/// contracts/events/act-servo_command.schema.json）。
pub const RR3_SERVO_TOPIC: &str = "rr3-act/servo/command";

/// 表情点名主题（在册 request L1；payload 形状事实源
/// contracts/events/interact-face_expression.schema.json）。
pub const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";

/// 轴 env 取值裁决：只认 `pan` / `tilt`（trim 后）。独立纯函数：裁决可单测。
pub fn rr3_periph_axis_parse(s: &str) -> Option<&'static str> {
    match s.trim() {
        "pan" => Some("pan"),
        "tilt" => Some("tilt"),
        _ => None,
    }
}

/// 波特率 env 取值裁决（批D②）：十进制解析后过 rr-ipc 档位表
/// （[`rr3_uart_baud_speed`]）——档位真值单点在 rr-ipc，本侧不抄第二份白名单。
pub fn rr3_periph_baud_parse(s: &str) -> Option<u32> {
    let baud = s.trim().parse::<u32>().ok()?;
    rr3_uart_baud_speed(baud).ok()?;
    Some(baud)
}

/// 墙钟纳秒（帧头 timestamp；取不到 epoch 时落 0——时间戳不参与校验，只作
/// 诊断参考，不猜不造）。
fn rr3_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// JSON-RPC 请求体（通用 method 面）：method 与 params 原样内嵌——`rr3.call` 与
/// `rr3.set_bindings`（批N4 件七(b) 基-24）同走本信封。独立纯函数：形状可单测。
fn rr3_bridge_request_body_params(id: u64, method: &str, params: &Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

/// JSON-RPC 请求体（`method` 恒 `rr3.call`、点名主题在 `params.topic`——rrd
/// 转发形状，与模块侧/哨兵侧的分发依据同源）。独立纯函数：形状可单测。
fn rr3_bridge_request_body(id: u64, topic: &str, args: &Value) -> String {
    rr3_bridge_request_body_params(id, "rr3.call", &json!({ "topic": topic, "args": args }))
}

/// 应答解析（纯函数）：payload 类型必须 Response、`jsonrpc` 必须 2.0、id 必须
/// 回显一致、result 在位；`result.error`（板侧业务性失败折形状，模块面同款）
/// 折 Err 点名。独立纯函数：门序可单测。
fn rr3_bridge_parse_response(
    got_ty: Rr3PayloadType,
    body: &[u8],
    want_id: u64,
) -> Result<Value, String> {
    if got_ty != Rr3PayloadType::Response {
        return Err(format!("payload 类型非 Response: {got_ty:?}"));
    }
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("应答体非 JSON: {e}"))?;
    if v.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("jsonrpc 必须为 \"2.0\"".into());
    }
    if v.get("id").and_then(Value::as_u64) != Some(want_id) {
        return Err(format!("id 回显不一致（要 {want_id}）"));
    }
    let result = v.get("result").cloned().ok_or("缺 result")?;
    if let Some(e) = result.get("error").and_then(Value::as_str) {
        return Err(format!("板侧业务失败: {e}"));
    }
    Ok(result)
}

/// 板侧绑定载荷（纯函数，批N4 件七(b) 基-24 桥上配置腿宿主半边）：描述符板侧三键
/// `Some` 者逐键入载荷，值保持字符串（描述符同形纪律：注入文本值全字符串，JSON
/// 侧不换形）；全缺 = `None`（无板侧配置可注）。独立纯函数：键位/换形可单测。
fn rr3_board_bindings_payload(b: &Rr3HalBindings) -> Option<Value> {
    let mut m = Map::new();
    if let Some(v) = b.i2c_sda {
        m.insert(RR3_BOARD_BINDING_I2C_SDA.to_string(), json!(v.to_string()));
    }
    if let Some(v) = b.i2c_scl {
        m.insert(RR3_BOARD_BINDING_I2C_SCL.to_string(), json!(v.to_string()));
    }
    if let Some(v) = b.adc_rail_pin {
        m.insert(
            RR3_BOARD_BINDING_ADC_RAIL_PIN.to_string(),
            json!(v.to_string()),
        );
    }
    if m.is_empty() {
        return None;
    }
    Some(Value::Object(m))
}

/// 板侧绑定载荷取环境（[`RR3_HAL_BINDINGS_ENV`] → 同形解析 → 载荷）。返回 `None`
/// 的三种形态都**不构成错误**——绑定只服务板侧自愈路径，执法点在板侧消费面
/// fail-closed（未注入即拒并点名），桥上腿的职责是"尽力送达、失败可见"：
/// - env 未设 = 无描述符形态（纯 sim/`--manifest` 调试后门）；
/// - 文本解析失败 = 配置坏，eprintln 点名后跳过（fail-visible；后端选择侧已
///   fail-closed，此处兜底点名不静默放行坏值）；
/// - 三键全缺 = 无板侧配置可注（host 档描述符本就无板侧键）。
fn rr3_board_bindings_payload_from_env() -> Option<Value> {
    let text = std::env::var(RR3_HAL_BINDINGS_ENV).ok()?;
    let b = match rr3_hal_bindings_from_text(&text) {
        Ok(b) => b,
        Err(errs) => {
            eprintln!(
                "rr-hal-esp32: {} 解析失败，跳过板侧绑定推送: {}",
                RR3_HAL_BINDINGS_ENV,
                errs.join("; ")
            );
            return None;
        }
    };
    rr3_board_bindings_payload(&b)
}

/// UART 长连接 + 请求 id 计数（servo/display 两实例面共用的连接盒）。
struct Rr3BridgeConn {
    path: PathBuf,
    /// 开口波特率（批D②：构造时定档，[`RR3_BRIDGE_BAUD`] 为缺省）。
    baud: u32,
    conn: Option<Rr3UartConn>,
    next_id: u64,
}

impl Rr3BridgeConn {
    /// 波特率显式档（批D②）：板固件 921600 时经 env/构造参数同档传入。
    fn rr3_bridge_conn_new_with_baud(path: PathBuf, baud: u32) -> Self {
        Self {
            path,
            baud,
            conn: None,
            next_id: 1,
        }
    }

    /// 连接保障：未开则 open + 启动窗口等待。失败当场 eprintln 点名（批J G5
    /// "错误当场可见"口径）后折 Busy（HAL 错型无 io 载荷，细节落日志）。
    fn rr3_ensure(&mut self) -> Result<(), Rr3HalError> {
        if self.conn.is_some() {
            return Ok(());
        }
        match Rr3UartConn::open_with_baud(&self.path, self.baud) {
            Ok(c) => {
                std::thread::sleep(Duration::from_millis(RR3_OPEN_SETTLE_MS));
                // settle 后补冲：开口触发的 DTR/RTS 复位让板端 banner 在开口
                // flush 之后才进队（批C e2e 实证 BadMagic），首帧前必须净化。
                if let Err(e) = c.rr3_flush_input() {
                    eprintln!("rr-hal-esp32: 开口净化失败 {}: {e}", self.path.display());
                    return Err(Rr3HalError::Busy);
                }
                self.conn = Some(c);
                // 板侧绑定推送（批N4 件七(b) 基-24 桥上配置腿）：每次（重）开连接
                // 都推——板侧重启或连接重建后注入态随之恢复，板侧覆盖语义幂等。
                // 推送失败只点名不上抛（rr3_push_bindings 内部消化）：绑定仅服务
                // 板侧自愈路径，执法点在板侧消费面 fail-closed，不挡舵机/表情能力面。
                self.rr3_push_bindings();
                Ok(())
            }
            Err(e) => {
                eprintln!("rr-hal-esp32: 串口 open 失败 {}: {e}", self.path.display());
                Err(Rr3HalError::Busy)
            }
        }
    }

    /// 板侧绑定推送（`rr3.set_bindings`，载荷住 `params.args`——method 面信封，
    /// rr-ipc bindings 模块定形）。推送失败（io/协议/板侧拒收）只 eprintln 点名，
    /// 不上抛不重试：绑定推送不挡能力面（执法点在板侧消费面 fail-closed）；
    /// io 失败时连接已由 rr3_method_call 报废，下次重开重推。
    fn rr3_push_bindings(&mut self) {
        let Some(payload) = rr3_board_bindings_payload_from_env() else {
            return;
        };
        if let Err(e) = self.rr3_method_call(RR3_METHOD_SET_BINDINGS, &json!({ "args": payload })) {
            eprintln!(
                "rr-hal-esp32: 板侧绑定推送未达（板侧自愈路径将 fail-closed 拒并点名）: {e:?}"
            );
        }
    }

    /// 一次 `rr3.call` round-trip（主题点名形状，rrd 转发同源）。
    fn rr3_call(&mut self, topic: &str, args: &Value) -> Result<Value, Rr3HalError> {
        self.rr3_method_call("rr3.call", &json!({ "topic": topic, "args": args }))
    }

    /// 一次 method 面 round-trip：组帧 → 发 → 阻塞收 → 四道门（dst/前缀/
    /// jsonrpc+id/error）。io 或协议失败点名后折 Busy，连接报废下次重开。
    fn rr3_method_call(&mut self, method: &str, params: &Value) -> Result<Value, Rr3HalError> {
        self.rr3_ensure()?;
        let conn = match self.conn.as_mut() {
            Some(c) => c,
            None => return Err(Rr3HalError::Busy), // rr3_ensure 后必在，防御性兜底
        };
        let id = self.next_id;
        self.next_id += 1;
        let body = rr3_bridge_request_body_params(id, method, params);
        let payload = rr3_body_encode(Rr3PayloadType::Request, body.as_bytes());
        let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
        hdr.trace_id = RR3_TRACE_DIRECT;
        hdr.timestamp_ns = rr3_now_ns();
        hdr.src_task = RR3_TASK_HOST;
        hdr.dst_task = RR3_PERIPH_TASK;
        let req = Rr3Frame {
            header: hdr,
            payload,
        };
        if let Err(e) = conn.rr3_send_buf(Rr3FrameBuf::from(&req)) {
            eprintln!("rr-hal-esp32: Request 写出失败: {e:?}");
            self.conn = None;
            return Err(Rr3HalError::Busy);
        }
        // 阻塞读：板侧对合法 Request 确定性应答（模块注记敞口：板死无应答则
        // 本侧阻塞，批E 桥面统一收口）
        let buf = match conn.rr3_recv_buf() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rr-hal-esp32: 应答收帧失败: {e:?}");
                self.conn = None;
                return Err(Rr3HalError::Busy);
            }
        };
        let frame = match Rr3Frame::try_from(buf) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("rr-hal-esp32: 应答帧还原失败: {e:?}");
                self.conn = None;
                return Err(Rr3HalError::Busy);
            }
        };
        if frame.header.dst_task != RR3_TASK_HOST {
            eprintln!(
                "rr-hal-esp32: 应答 dst_task={} 非 {RR3_TASK_HOST}，拒收",
                frame.header.dst_task
            );
            self.conn = None;
            return Err(Rr3HalError::Busy);
        }
        let (ty, body) = match rr3_body_split(&frame.payload) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("rr-hal-esp32: 应答 payload 前缀非法: {e:?}");
                self.conn = None;
                return Err(Rr3HalError::Busy);
            }
        };
        rr3_bridge_parse_response(ty, &body, id).map_err(|e| {
            eprintln!("rr-hal-esp32: 应答门拒绝: {e}");
            Rr3HalError::Busy
        })
    }
}

/// 锁面：HAL trait 的 `&self` 读面（angle/frame）也要求触链路——Mutex 内部可
/// 变性同时覆盖读/写两面的连接重建。模块调用面单线程，无争用，锁只作形状。
fn lock<'a, T>(m: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------
// 舵机桥档：Rr3PwmServo 经 rr3-act/servo/command 帧
// ---------------------------------------------------------------------------

/// ESP32 舵机桥后端：`set_angle` 发 `{"angle_deg":N,"axis":axis}`（写），
/// `angle` 发 `{"axis":axis}`（无 angle_deg=回读——schema 的读形状）。
pub struct Rr3Esp32UartServo {
    conn: Mutex<Rr3BridgeConn>,
    axis: &'static str,
}

impl Rr3Esp32UartServo {
    pub fn rr3_esp32_uart_servo_new(path: PathBuf, axis: &'static str) -> Self {
        Self::rr3_esp32_uart_servo_new_with_baud(path, axis, RR3_BRIDGE_BAUD)
    }

    /// 波特率显式档（批D②）：env [`RR3_HAL_PERIPH_BAUD_ENV`] 的解析结果经此处入 conn。
    pub fn rr3_esp32_uart_servo_new_with_baud(
        path: PathBuf,
        axis: &'static str,
        baud: u32,
    ) -> Self {
        Self {
            conn: Mutex::new(Rr3BridgeConn::rr3_bridge_conn_new_with_baud(path, baud)),
            axis,
        }
    }
}

impl Rr3PwmServo for Rr3Esp32UartServo {
    fn set_angle(&mut self, deg: u16) -> Result<(), Rr3HalError> {
        let result = lock(&self.conn).rr3_call(
            RR3_SERVO_TOPIC,
            &json!({ "angle_deg": deg, "axis": self.axis }),
        )?;
        // 回显一致性：板侧生效角必须与设定角逐度一致（round-trip 判据的协议面）
        if result.get("angle_deg").and_then(Value::as_u64) != Some(deg as u64) {
            eprintln!(
                "rr-hal-esp32: 舵机回显不一致（设 {deg}，回 {:?}）",
                result.get("angle_deg")
            );
            return Err(Rr3HalError::Busy);
        }
        Ok(())
    }

    fn angle(&self) -> Result<u16, Rr3HalError> {
        let result = lock(&self.conn).rr3_call(RR3_SERVO_TOPIC, &json!({ "axis": self.axis }))?;
        let deg = result
            .get("angle_deg")
            .and_then(Value::as_u64)
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| {
                eprintln!("rr-hal-esp32: 舵机回读缺 angle_deg 或越 u16");
                Rr3HalError::Busy
            })?;
        Ok(deg)
    }
}

// ---------------------------------------------------------------------------
// 显示桥档：Rr3Display 经 rr3-interact/face/expression 帧（HAL 渲染变体）
// ---------------------------------------------------------------------------

/// ESP32 显示桥后端：`draw` 发 `{"frame":[8u8]}`（位图，板侧 8x8→64x64 放大
/// 渲染到 OLED 中央）；`frame` 返回本地缓存——探活语义见 impl 注记。
pub struct Rr3Esp32UartDisplay {
    conn: Mutex<Rr3BridgeConn>,
    last: Mutex<Rr3FaceFrame>,
    probed: Mutex<bool>,
}

impl Rr3Esp32UartDisplay {
    pub fn rr3_esp32_uart_display_new(path: PathBuf) -> Self {
        Self::rr3_esp32_uart_display_new_with_baud(path, RR3_BRIDGE_BAUD)
    }

    /// 波特率显式档（批D②）：env [`RR3_HAL_PERIPH_BAUD_ENV`] 的解析结果经此处入 conn。
    pub fn rr3_esp32_uart_display_new_with_baud(path: PathBuf, baud: u32) -> Self {
        Self {
            conn: Mutex::new(Rr3BridgeConn::rr3_bridge_conn_new_with_baud(path, baud)),
            last: Mutex::new([0; 8]),
            probed: Mutex::new(false),
        }
    }
}

impl Rr3Display for Rr3Esp32UartDisplay {
    fn draw(&mut self, frame: &Rr3FaceFrame) -> Result<(), Rr3HalError> {
        let result = lock(&self.conn).rr3_call(RR3_FACE_TOPIC, &json!({ "frame": frame }))?;
        if result.get("applied").and_then(Value::as_bool) != Some(true) {
            eprintln!("rr-hal-esp32: 出帧应答缺 applied=true");
            return Err(Rr3HalError::Busy);
        }
        *lock(&self.last) = *frame;
        // draw 成功 = 链路已证（四道应答门真 round-trip），frame() 不必再探活
        // ——探活帧是全黑位图，板侧当真渲染会擦掉屏上现帧（批C e2e 实证）。
        *lock(&self.probed) = true;
        Ok(())
    }

    /// 读回当前帧（HAL 契约：读回 == 上次 draw 的帧）。探活语义：链路未建立
    /// （从未 draw 过）时先发一条全黑帧 round-trip（rr3-interact 开机探活的真
    /// 验证——板不在位则 Busy 显式回错，探活 exit(1) 是预期语义，R11③ 口径），
    /// 之后恒返回缓存（节拍 250ms 的 draw 持续真发，链路活性由 draw 面兜底）。
    fn frame(&self) -> Result<Rr3FaceFrame, Rr3HalError> {
        let need_probe = !*lock(&self.probed);
        if need_probe {
            let probe: Rr3FaceFrame = [0; 8];
            lock(&self.conn).rr3_call(RR3_FACE_TOPIC, &json!({ "frame": probe }))?;
            *lock(&self.probed) = true;
        }
        Ok(*lock(&self.last))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轴 env 取值裁决：两档内放行，trim 宽容；其余全拒（落缺省 pan 的正门）。
    #[test]
    fn periph_axis_parse_accepts_pan_tilt_only() {
        assert_eq!(rr3_periph_axis_parse("pan"), Some("pan"));
        assert_eq!(rr3_periph_axis_parse("tilt"), Some("tilt"));
        assert_eq!(rr3_periph_axis_parse(" pan "), Some("pan"));
        for bad in ["", "PAN", "roll", "yaw", "1"] {
            assert_eq!(rr3_periph_axis_parse(bad), None, "{bad}");
        }
    }

    /// 波特率 env 取值裁决（批D②）：三档内放行、trim 宽容；档位外/垃圾全拒
    /// （真值单点在 rr-ipc 档位表，本侧只做解析）。
    #[test]
    fn periph_baud_parse_accepts_whitelist_only() {
        assert_eq!(rr3_periph_baud_parse("115200"), Some(115200));
        assert_eq!(rr3_periph_baud_parse(" 921600 "), Some(921600));
        assert_eq!(rr3_periph_baud_parse("9600"), Some(9600));
        for bad in ["", "0", "230400", "921600x", "-1"] {
            assert_eq!(rr3_periph_baud_parse(bad), None, "{bad}");
        }
    }

    /// 请求体形状：method 恒 rr3.call、主题在 params.topic、args 原样内嵌
    /// （conn.rs 转发形状的分发依据；形状漂移此处即红）。
    #[test]
    fn bridge_request_body_is_rr3_call_with_topic_in_params() {
        let s = rr3_bridge_request_body(7, RR3_SERVO_TOPIC, &json!({ "angle_deg": 90 }));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["method"], "rr3.call");
        assert_eq!(v["params"]["topic"], RR3_SERVO_TOPIC);
        assert_eq!(v["params"]["args"]["angle_deg"], 90);
    }

    /// method 面请求体信封：method/params 原样内嵌（rr3.set_bindings 的协议形状
    /// 锚——载荷住 params.args，rr-ipc bindings 模块定形）。
    #[test]
    fn bridge_request_body_params_is_generic_method_envelope() {
        let s = rr3_bridge_request_body_params(9, "rr3.set_bindings", &json!({ "args": {} }));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 9);
        assert_eq!(v["method"], "rr3.set_bindings");
        assert_eq!(v["params"]["args"], json!({}));
    }

    /// 板侧绑定载荷（纯函数）：三键 Some 者逐键入载荷、值保持字符串（描述符同形
    /// 纪律）；全缺 = None（无板侧配置可注）；部分注入只带已有键。
    #[test]
    fn board_bindings_payload_carries_string_values_only() {
        let mut b = Rr3HalBindings::default();
        assert_eq!(rr3_board_bindings_payload(&b), None, "三键全缺 = 无载荷");
        b.i2c_sda = Some(21);
        b.adc_rail_pin = Some(34);
        let p = rr3_board_bindings_payload(&b).expect("有键必须有载荷");
        assert_eq!(p["i2c_sda"], "21", "值保持字符串（描述符同形纪律）");
        assert_eq!(p["adc_rail_pin"], "34");
        assert!(p.get("i2c_scl").is_none(), "未注入键不入载荷");
    }

    /// 应答门序：Response + id 回显 + result 在位放行；类型错/id 错/缺 result/
    /// result.error 四路全点名（板侧业务性失败折形状的协议面锚）。
    #[test]
    fn bridge_parse_response_gates() {
        let ok = json!({"jsonrpc": "2.0", "id": 3, "result": {"angle_deg": 90}});
        let (ty, raw) = (
            Rr3PayloadType::Response,
            rr3_body_encode(Rr3PayloadType::Response, ok.to_string().as_bytes()),
        );
        let v = rr3_bridge_parse_response(ty, &raw[2..], 3).unwrap();
        assert_eq!(v["angle_deg"], 90);

        // 类型错
        let ev = rr3_body_encode(Rr3PayloadType::Event, ok.to_string().as_bytes());
        assert!(rr3_bridge_parse_response(Rr3PayloadType::Event, &ev[2..], 3).is_err());
        // id 错
        assert!(rr3_bridge_parse_response(ty, &raw[2..], 4).is_err());
        // 缺 result
        let no_res = json!({"jsonrpc": "2.0", "id": 3});
        let nr = rr3_body_encode(ty, no_res.to_string().as_bytes());
        assert!(rr3_bridge_parse_response(ty, &nr[2..], 3).is_err());
        // result.error 折 Err
        let err_body =
            json!({"jsonrpc": "2.0", "id": 3, "result": {"error": "角度超范围 0-180: 200"}});
        let eb = rr3_body_encode(ty, err_body.to_string().as_bytes());
        let e = rr3_bridge_parse_response(ty, &eb[2..], 3).unwrap_err();
        assert!(e.contains("板侧业务失败"), "{e}");
        assert!(e.contains("角度超范围"), "{e}");
    }

    /// 真值锚定：宿主/板上 task 位形与 rr-core 的机制真值逐值一致（本 crate
    /// 不加依赖边的补偿断言——漂移即红，防两处常量静默分叉）。
    #[test]
    fn task_and_trace_constants_match_rr_core_truth() {
        assert_eq!(RR3_TASK_HOST, 1, "宿主位形=rrd（rr-core::RR3_TASK_RRD）");
        assert_eq!(
            RR3_TRACE_DIRECT, 0,
            "直连 trace（rr-core::RR3_TRACE_ID_DIRECT）"
        );
        assert_eq!(RR3_PERIPH_TASK, 2, "板上执行面占首模块池位形");
        assert_eq!(
            RR3_BRIDGE_BAUD, 115200,
            "缺省档保持 115200（Q1=(a)：宿主缺省不变）"
        );
    }
}
