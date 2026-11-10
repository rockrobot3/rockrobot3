//! 树莓派 4B 板侧五端口后端（W4/P0-C，批N43）——**平台适配层**：
//!
//! 本文件只做"开设备、收帧/收行、分发、调机制核、回包"的装配与循环；
//! 帧面机制在 `periph-mech::frame`、OLED 编码在 `periph-mech::oled_codec`、
//! 舵机缓速/行程/轨压判定在 `periph-mech::periph_core`+`rail_judge`，
//! 五端口实现在 [`ports`]，策略量在 [`board`]。换板只改 `board`/`ports`，
//! 机制与协议字节零改动（U1/U2/U3 判据的板侧半边）。
//!
//! 运行形态：单线程控制面（UART 独占；I2C/PWM 慢动作在同一线程串行执行——
//! T3 无语音/无 WANDER/无并行音频，串行即可，且避免共享总线锁）。
//! 板侧双轴舵机的"机制件"按计划 §8.2 复用 `atoms/servo/mech.rs`（含生成行程表），
//! 面目几何复用 `atoms/face/mech.rs`——**同件复用，不抄第二份**。

mod board;
#[allow(dead_code)]
#[path = "../../esp32-periph-base/atoms/face/mech.rs"]
mod face;
mod oled;
mod ports;
#[allow(dead_code)]
#[path = "../../esp32-periph-base/atoms/servo/mech.rs"]
mod servo;
#[allow(dead_code)]
#[path = "../../esp32-periph-base/src/topics_gen.rs"]
mod topics_gen;

use std::time::Duration;

use periph_mech::periph_core::{rr3_servo_gate, rr3_slew_steps, Rr3ServoCore};
use periph_mech::rail_judge::{RailJudge, RailTier};
use rr_ipc::{
    rr3_board_bind_entry, Rr3BoardBindings, Rr3ByteStream, Rr3UartConn, RR3_METHOD_SET_BINDINGS,
};
use rr_periph_proto::ports::{Rr3ClockPort, Rr3PwmChannel, Rr3PwmPort};
use serde_json::{json, Value};

use crate::board::*;
use crate::face::{expr_eye_base, EyeStyle, EYE_IDLE};
use crate::servo::{axis_check, deg_to_duty, Axis};
use crate::topics_gen::{RR3_FACE_EXPRS, RR3_FACE_TOPIC, RR3_SERVO_TOPIC};

/// 帧面控制面适配（`pre` = 双面分流时已消耗的首字节；帧首读时为 Some(0x31)）。
struct UartIo<'a> {
    uart: &'a mut Rr3UartConn,
    pre: Option<u8>,
}

impl Rr3ByteStream for UartIo<'_> {
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
        if let Some(b) = self.pre.take() {
            if buf.is_empty() {
                return 0;
            }
            buf[0] = b;
            return 1;
        }
        // 帧内读与行面同一纪律：**字节间超时 + 无进展预算**。旧版直接走阻塞
        // `Read`（VMIN=1 无 VTIME），半帧断线时控制面永久停在 128B 头里——宿主
        // 进程被杀/误码丢字节后再不会有字节到达，只能整机重启。预算用尽折叠为
        // 0（缝上无错误通道，Closed 形状）→ `TornHeader/TornPayload` → 调用方
        // 残尾吞净后复位失步，与 ESP 板侧同名实现同一语义。
        let mut idle = 0u32;
        loop {
            match self.uart.rr3_read_timeout(buf, LINE_BYTE_TIMEOUT_MS) {
                Ok(0) => {
                    idle += 1;
                    if idle >= FRAME_GAP_BUDGET {
                        eprintln!("[periph] 帧内读超时（{FRAME_GAP_BUDGET} 拍无进展，判半帧断线）");
                        return 0;
                    }
                }
                Ok(n) => return n,
                Err(e) => {
                    eprintln!("[periph] 帧面 UART 读失败: {e}");
                    return 0;
                }
            }
        }
    }

    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
        self.uart.rr3_write_all_bytes(buf)
    }
}

/// 板侧共享状态（**不含硬件句柄**：控制面单线程串行改/读，硬件在 [`Board`]）。
struct State {
    servo: Rr3ServoCore,
    last_text: String,
    frame_mode: bool,
    expr_mode: bool,
    prev_expr: String,
    base: EyeStyle,
    rail_mv: u16,
    rail_raw: u16,
    rail_tier: RailTier,
    rail_valid: bool,
    rail_low_events: u32,
    binding_mismatch: u32,
    next_rail_ms: u64,
}

impl State {
    fn boot() -> Self {
        Self {
            servo: Rr3ServoCore::new(Axis::Pan.center_deg(), Axis::Tilt.center_deg()),
            last_text: String::from("(boot)"),
            frame_mode: false,
            expr_mode: false,
            prev_expr: String::from("none"),
            base: EYE_IDLE,
            rail_mv: 0,
            rail_raw: 0,
            rail_tier: RailTier::Ok,
            rail_valid: false,
            rail_low_events: 0,
            binding_mismatch: 0,
            next_rail_ms: 0,
        }
    }
}

/// 板侧装配体：五端口 + 策略量 + 共享状态。
struct Board {
    uart: Rr3UartConn,
    pwm: ports::PwmPort,
    oled: oled::OledRenderer,
    rail: ports::RailSensor,
    clock: ports::MonotonicClock,
    judge: RailJudge,
    state: State,
    rail_err_logged: bool,
}

impl Board {
    fn new(uart: Rr3UartConn, i2c: ports::SharedI2c) -> Self {
        let oled = oled::OledRenderer::new(i2c.clone());
        let rail = ports::RailSensor::probe(i2c, ADS1115_ADDR);
        Self {
            uart,
            pwm: ports::PwmPort::default_root(),
            oled,
            rail,
            clock: ports::MonotonicClock,
            judge: RailJudge::new(RAIL_MILD_MV, RAIL_SEVERE_MV),
            state: State::boot(),
            rail_err_logged: false,
        }
    }

    fn now_ms(&self) -> u64 {
        self.clock.rr3_now_ns() / 1_000_000
    }

    /// 上电面：OLED 初始化 + idle 表情；轨压首次采样（缺器件只点名不拦）。
    fn boot_surfaces(&mut self) {
        match self.oled.init() {
            Ok(()) => {
                if let Err(e) = self.render_expression("idle") {
                    eprintln!("[periph] OLED idle 渲染失败: {e}");
                }
            }
            Err(e) => eprintln!("[periph] OLED init 失败（面板命令将回 Err）: {e}"),
        }
        if let Err(e) = self.sample_rail() {
            eprintln!("[periph] 启动轨压采样失败（rail 请求将回 Err）: {e}");
            self.rail_err_logged = true;
        }
    }

    fn run(&mut self) -> ! {
        loop {
            self.maybe_sample_rail();
            let mut first = [0u8; 1];
            match self
                .uart
                .rr3_read_timeout(&mut first, FIRST_BYTE_TIMEOUT_MS)
            {
                Ok(0) => continue,
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[periph] UART 读失败，退避后重试: {e}");
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
            if first[0] == rr_periph_proto::RR3_MAGIC_FIRST {
                self.frame_round(first[0]);
            } else {
                match self.read_line(first[0]) {
                    Ok(line) => self.line_round(&line),
                    Err(e) => {
                        eprintln!("[periph] 收行失败: {e}");
                        self.reply(&format!("ERR {e}"));
                    }
                }
            }
        }
    }

    /// 帧面一轮：收帧 → 协议门 → 分发 → 应答；坏帧丢弃 + 残尾吞净。
    fn frame_round(&mut self, first: u8) {
        let recv = {
            let mut io = UartIo {
                uart: &mut self.uart,
                pre: Some(first),
            };
            periph_mech::frame::frame_recv(&mut io)
        };
        let req = match recv {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[periph] 帧面丢弃: {e}");
                self.drain_uart();
                return;
            }
        };
        println!("[periph] 帧面 id={} topic=\"{}\"", req.id, req.topic);
        let result = self.dispatch(&req);
        if let Err(e) = &result {
            eprintln!("[periph] 帧面业务失败 id={}: {e}", req.id);
        }
        let mut io = UartIo {
            uart: &mut self.uart,
            pre: None,
        };
        let clock = &self.clock;
        if let Err(e) = periph_mech::frame::frame_send_response(&mut io, &req, result, clock) {
            eprintln!("[periph] 应答发送失败: {e}");
        }
    }

    fn drain_uart(&mut self) {
        for _ in 0..FRAME_DRAIN_MAX {
            let mut b = [0u8; 1];
            match self.uart.rr3_read_timeout(&mut b, 2) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }

    /// 收一行（半行超时丢弃；'\\r' 剥离；超长吞尾）。
    fn read_line(&mut self, first: u8) -> Result<String, String> {
        let mut buf: Vec<u8> = Vec::with_capacity(LINE_MAX);
        if first == b'\n' {
            return Ok(String::new());
        }
        buf.push(first);
        let mut idle = 0u32;
        loop {
            let mut byte = [0u8; 1];
            match self.uart.rr3_read_timeout(&mut byte, LINE_BYTE_TIMEOUT_MS) {
                Ok(0) => {
                    idle += 1;
                    if idle >= 2 {
                        return Err("line timeout（半行无进展丢弃）".into());
                    }
                    continue;
                }
                Ok(_) => {
                    idle = 0;
                    match byte[0] {
                        b'\n' => {
                            while matches!(buf.last(), Some(b'\r')) {
                                buf.pop();
                            }
                            return Ok(String::from_utf8_lossy(&buf).into_owned());
                        }
                        b => {
                            if buf.len() >= LINE_MAX {
                                self.drain_line_tail();
                                return Err("line too long".into());
                            }
                            buf.push(b);
                        }
                    }
                }
                Err(e) => return Err(format!("uart read 失败: {e}")),
            }
        }
    }

    fn drain_line_tail(&mut self) {
        for _ in 0..(LINE_MAX * 4) {
            let mut b = [0u8; 1];
            match self.uart.rr3_read_timeout(&mut b, LINE_BYTE_TIMEOUT_MS) {
                Ok(0) => return,
                Ok(_) => {
                    if b[0] == b'\n' {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }

    fn reply(&mut self, line: &str) {
        let bytes = format!("{line}\r\n");
        let n = self.uart.rr3_write_all_bytes(bytes.as_bytes());
        if n < bytes.len() {
            eprintln!("[periph] 应答丢字节 {n}/{}: {line}", bytes.len());
        }
    }

    fn dispatch(&mut self, req: &periph_mech::frame::FrameReq) -> Result<Value, String> {
        if req.method == RR3_METHOD_SET_BINDINGS {
            return self.handle_set_bindings(&req.args);
        }
        if req.method != "rr3.call" {
            return Err(format!("method 非法: \"{}\"（恒 rr3.call）", req.method));
        }
        match req.topic.as_str() {
            RR3_SERVO_TOPIC => self.handle_servo(&req.args),
            RR3_FACE_TOPIC => self.handle_face(&req.args),
            other => Err(format!("未知主题: \"{other}\"")),
        }
    }

    /// 绑定注入（控制面）：键域/值域走 rr-ipc 单点；板侧逐键对账只计数不拒收。
    fn handle_set_bindings(&mut self, args: &Value) -> Result<Value, String> {
        let obj = args
            .as_object()
            .ok_or_else(|| format!("args 非法: {args}（须对象）"))?;
        let mut nb = Rr3BoardBindings::default();
        for (k, v) in obj {
            let s = v
                .as_str()
                .ok_or_else(|| format!("{k} 值非法: {v}（须字符串）"))?;
            rr3_board_bind_entry(&mut nb, k, s)?;
        }
        println!(
            "[periph] 绑定注入 ✓ i2c_sda={} i2c_scl={} uart2_tx={} uart2_rx={}",
            nb.i2c_sda.map_or("-".into(), |v| v.to_string()),
            nb.i2c_scl.map_or("-".into(), |v| v.to_string()),
            nb.uart2_tx.map_or("-".into(), |v| v.to_string()),
            nb.uart2_rx.map_or("-".into(), |v| v.to_string()),
        );
        let bad = reconcile_bindings(&nb);
        self.state.binding_mismatch = self.state.binding_mismatch.saturating_add(bad);
        Ok(json!({ "applied": true, "mismatch": bad }))
    }

    fn handle_servo(&mut self, args: &Value) -> Result<Value, String> {
        let axis = parse_axis(args.get("axis"))?;
        match args.get("angle_deg") {
            None | Some(Value::Null) => {
                let deg = self.state.servo.deg(channel_of(axis));
                Ok(json!({ "angle_deg": deg, "move_ms": 0 }))
            }
            Some(v) => {
                let deg = v
                    .as_u64()
                    .ok_or_else(|| format!("angle_deg 非法: {v}（须 0-180 整数）"))?
                    as u32;
                let d = self.servo_move(axis, deg)?;
                Ok(json!({ "angle_deg": d, "move_ms": 0 }))
            }
        }
    }

    fn handle_face(&mut self, args: &Value) -> Result<Value, String> {
        if let Some(v) = args.get("expression") {
            let expr = v
                .as_str()
                .ok_or_else(|| format!("expression 非法: {v}（须字符串）"))?;
            if !RR3_FACE_EXPRS.contains(&expr) {
                return Err(format!("expression 不在白名单: \"{expr}\""));
            }
            let base = expr_eye_base(expr);
            // 先渲染后提交状态：渲染失败不污染内部视图（效应确认才记账，
            // 否则状态报"已生效"而屏幕未变，排障时视图与现场不一致）
            self.oled
                .capsules(base)
                .map_err(|e| format!("OLED 表情渲染失败: {e}"))?;
            let previous = std::mem::replace(&mut self.state.prev_expr, expr.to_string());
            self.state.base = base;
            self.state.expr_mode = true;
            self.state.frame_mode = false;
            Ok(json!({ "expression": expr, "previous": previous }))
        } else if let Some(v) = args.get("frame") {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("frame 非法: {v}（须 8 字节数组）"))?;
            if arr.len() != 8 {
                return Err(format!("frame 长度非法: {}（须 8）", arr.len()));
            }
            let mut f = [0u8; 8];
            for (i, item) in arr.iter().enumerate() {
                let b = item
                    .as_u64()
                    .ok_or_else(|| format!("frame[{i}] 非法: {item}"))?;
                if b > 255 {
                    return Err(format!("frame[{i}] 超 u8: {b}"));
                }
                f[i] = b as u8;
            }
            // 同上：点阵上屏成功后才切换模式
            self.oled
                .frame(&f)
                .map_err(|e| format!("OLED 点阵渲染失败: {e}"))?;
            self.state.frame_mode = true;
            self.state.expr_mode = false;
            Ok(json!({ "applied": true }))
        } else {
            Err("缺 expression/frame 之一".into())
        }
    }

    /// 舵机移动（机制核：行程校验 + 轨压闸 + 缓速步进；本面只管写端口与等待）。
    fn servo_move(&mut self, axis: Axis, deg: u32) -> Result<u32, String> {
        let d = axis_check(axis, deg)?;
        rr3_servo_gate(RAIL_GATE_ARMED, self.state.rail_tier, axis.name(), d)?;
        let ch = channel_of(axis);
        for step in rr3_slew_steps(self.state.servo.deg(ch), d, SERVO_SLEW_STEP_DEG) {
            let ticks = deg_to_duty(step, DUTY_MIN_TICKS, DUTY_MAX_TICKS);
            self.pwm.rr3_pwm_duty(ch, ticks)?;
            if step != d {
                std::thread::sleep(Duration::from_millis(SERVO_SLEW_STEP_MS));
            }
        }
        self.state.servo.commit(ch, d);
        self.redraw_text();
        Ok(d)
    }

    fn render_expression(&mut self, expr: &str) -> Result<(), String> {
        let base = expr_eye_base(expr);
        // 先渲染后提交状态（同 handle_face：效应确认才记账）
        self.oled
            .capsules(base)
            .map_err(|e| format!("OLED 表情渲染失败: {e}"))?;
        self.state.base = base;
        self.state.expr_mode = true;
        self.state.frame_mode = false;
        self.state.prev_expr = expr.to_string();
        Ok(())
    }

    /// 文本模式重绘（帧/表情模式占满 8 页时跳过，不凿位图）。
    fn redraw_text(&mut self) {
        if self.state.frame_mode || self.state.expr_mode {
            return;
        }
        let text = self.state.last_text.clone();
        let pan = self.state.servo.deg(Rr3PwmChannel::Pan);
        let tilt = self.state.servo.deg(Rr3PwmChannel::Tilt);
        if let Err(e) = self.oled.show_text(&text, pan, tilt) {
            eprintln!("[periph] OLED 文本渲染失败: {e}");
        }
    }

    fn maybe_sample_rail(&mut self) {
        let now = self.now_ms();
        if now < self.state.next_rail_ms {
            return;
        }
        self.state.next_rail_ms = now.saturating_add(RAIL_SAMPLE_PERIOD_MS);
        if let Err(e) = self.sample_rail() {
            if !self.rail_err_logged {
                eprintln!("[periph] rail 周期采样失败（后续静默，RAIL 请求仍回 Err）: {e}");
                self.rail_err_logged = true;
            }
        }
    }

    fn sample_rail(&mut self) -> Result<(u16, u16), String> {
        let (mv, raw) = self.rail.sample()?;
        let (tier, edge) = self.judge.feed(mv);
        if edge {
            self.state.rail_low_events = self.state.rail_low_events.saturating_add(1);
            eprintln!("[periph] 5V 轨越阈: {mv}mV tier={}", tier.name());
        }
        self.state.rail_mv = mv;
        self.state.rail_raw = raw;
        self.state.rail_tier = tier;
        self.state.rail_valid = true;
        self.rail_err_logged = false;
        Ok((mv, raw))
    }

    fn line_round(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let mut parts = line.split_whitespace();
        let head = parts.next().unwrap_or("");
        let result: Result<String, String> = match head {
            "HELP" => Ok("CMDS: OLED <text> | FACE <expr> | PAN <deg> | TILT <deg> | SERVO <pan> <tilt> | RAIL | INFO | HELP".into()),
            "INFO" => Ok(self.info_line()),
            "RAIL" => self
                .sample_rail()
                .map(|(mv, raw)| {
                    format!(
                        "OK rail={mv}mV raw={raw} tier={}",
                        self.state.rail_tier.name()
                    )
                }),
            "OLED" => self.oled_text(line[head.len()..].trim_start()),
            "FACE" => self.face_expr(parts.next()),
            "PAN" => parse_deg(parts.next()).and_then(|deg| {
                self.servo_move(Axis::Pan, deg)
                    .map(|d| format!("OK pan={d}"))
            }),
            "TILT" => parse_deg(parts.next()).and_then(|deg| {
                self.servo_move(Axis::Tilt, deg)
                    .map(|d| format!("OK tilt={d}"))
            }),
            "SERVO" => self.servo_pair(parse_deg(parts.next()), parse_deg(parts.next())),
            "WANDER" => Err("WANDER 不支持（T3 档无自主扫摆；INFO 恒报 wander=off）".into()),
            "SCAN" => Err("SCAN 不支持（用 PAN/TILT/SERVO 逐点）".into()),
            "AUDIO" => Err("AUDIO 不支持（T3 档明确不做语音/DoA）".into()),
            other => Err(format!("unknown cmd \"{other}\"")),
        };
        match result {
            Ok(s) => self.reply(&s),
            Err(e) => self.reply(&format!("ERR {e}")),
        }
    }

    /// OLED 文本臂：非空 + 长度上限（行数×列数），设置后重绘。
    fn oled_text(&mut self, rest: &str) -> Result<String, String> {
        if rest.is_empty() {
            Err("OLED 缺文本参数".into())
        } else if rest.len() > OLED_COLS * (OLED_ROWS - 1) {
            Err(format!(
                "OLED 文本超长（≤{} 字符 = {} 行 × {} 列）",
                OLED_COLS * (OLED_ROWS - 1),
                OLED_ROWS - 1,
                OLED_COLS
            ))
        } else {
            self.state.last_text = rest.to_string();
            self.state.frame_mode = false;
            self.state.expr_mode = false;
            self.redraw_text();
            Ok(format!("OK oled \"{rest}\""))
        }
    }

    /// FACE 表情臂：白名单内才进渲染。
    fn face_expr(&mut self, expr: Option<&str>) -> Result<String, String> {
        match expr {
            Some(expr) if RR3_FACE_EXPRS.contains(&expr) => self
                .render_expression(expr)
                .map(|()| format!("OK face={expr}")),
            Some(expr) => Err(format!("FACE 表情不在白名单: \"{expr}\"")),
            None => Err("FACE 缺表情参数".into()),
        }
    }

    /// SERVO 双轴臂：两角度同序解析（任一非法即拒），pan 先动 tilt 后动。
    fn servo_pair(
        &mut self,
        p: Result<u32, String>,
        t: Result<u32, String>,
    ) -> Result<String, String> {
        match (p, t) {
            (Ok(p), Ok(t)) => self
                .servo_move(Axis::Pan, p)
                .and_then(|p| self.servo_move(Axis::Tilt, t).map(|t| (p, t)))
                .map(|(p, t)| format!("OK servo pan={p} tilt={t}")),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }

    fn info_line(&self) -> String {
        info_of(&self.state)
    }
}

fn info_of(state: &State) -> String {
    let pan = state.servo.deg(Rr3PwmChannel::Pan);
    let tilt = state.servo.deg(Rr3PwmChannel::Tilt);
    let (plo, phi) = Axis::Pan.range();
    let (tlo, thi) = Axis::Tilt.range();
    let rail = if state.rail_valid {
        format!(
            "rail={}mV raw={} rail_tier={}",
            state.rail_mv,
            state.rail_raw,
            state.rail_tier.name()
        )
    } else {
        String::from("rail=absent")
    };
    format!(
        "INFO pan={pan:03} tilt={tilt:03} lim=pan[{plo},{phi}]/tilt[{tlo},{thi}] \
         wander=off phase=0 {rail} low_events={} binding_mismatch={} oled_last=\"{}\"",
        state.rail_low_events, state.binding_mismatch, state.last_text
    )
}

fn channel_of(axis: Axis) -> Rr3PwmChannel {
    match axis {
        Axis::Pan => Rr3PwmChannel::Pan,
        Axis::Tilt => Rr3PwmChannel::Tilt,
    }
}

fn parse_axis(v: Option<&Value>) -> Result<Axis, String> {
    match v {
        None | Some(Value::Null) => Ok(Axis::Pan),
        Some(v) => match v.as_str() {
            Some("pan") => Ok(Axis::Pan),
            Some("tilt") => Ok(Axis::Tilt),
            _ => Err(format!("axis 非法: {v}（pan|tilt）")),
        },
    }
}

fn parse_deg(v: Option<&str>) -> Result<u32, String> {
    v.ok_or_else(|| "缺角度参数".to_string())?
        .parse::<u32>()
        .map_err(|_| format!("角度非法: {v:?}"))
}

/// 绑定对账：Pi 板只消费四个总线/桥键（其余键在 Pi 上不适用，不判等）。
fn reconcile_bindings(b: &Rr3BoardBindings) -> u32 {
    let mut bad = 0u32;
    for (key, got, want) in [
        ("i2c_sda", b.i2c_sda, BIND_I2C_SDA),
        ("i2c_scl", b.i2c_scl, BIND_I2C_SCL),
        ("uart2_tx", b.uart2_tx, BIND_UART2_TX),
        ("uart2_rx", b.uart2_rx, BIND_UART2_RX),
    ] {
        if let Some(v) = got {
            if v != want {
                eprintln!("[periph] 绑定对账 ✗ {key}: 注入 {v} ≠ 装配 {want}");
                bad = bad.saturating_add(1);
            }
        }
    }
    bad
}

fn main() {
    let uart = match ports::open_default_uart() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[periph] 启动失败: {e}");
            std::process::exit(1);
        }
    };
    let i2c = ports::open_default_i2c();
    let mut board = Board::new(uart, i2c);
    println!("READY periph-base(pi) v0.1.0 五端口（UART/I2C/PWM/ADS1115/CLOCK_MONOTONIC）");
    board.boot_surfaces();
    board.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_axis_and_degree_are_fail_closed() {
        assert!(parse_axis(None).is_ok());
        assert!(parse_axis(Some(&json!("pan"))).is_ok());
        assert!(parse_axis(Some(&Value::Null)).is_ok());
        match parse_axis(Some(&json!("yaw"))) {
            Err(e) => assert_eq!(e, "axis 非法: \"yaw\"（pan|tilt）"),
            Ok(_) => panic!("未知轴必须拒"),
        }
        assert_eq!(parse_deg(Some("90")), Ok(90));
        assert!(parse_deg(None).is_err());
        assert!(parse_deg(Some("-1")).is_err());
        assert!(parse_deg(Some("x")).is_err());
    }

    /// 绑定对账：四键失配逐条计数，未注入不判、其余键不参与（Pi 不适用）。
    #[test]
    fn reconcile_counts_only_pi_bus_keys() {
        let mut b = Rr3BoardBindings::default();
        assert_eq!(reconcile_bindings(&b), 0, "未注入不判");
        b.i2c_sda = Some(BIND_I2C_SDA);
        b.i2c_scl = Some(BIND_I2C_SCL);
        b.uart2_tx = Some(BIND_UART2_TX);
        b.uart2_rx = Some(BIND_UART2_RX);
        assert_eq!(reconcile_bindings(&b), 0, "四键齐且值对");
        b.i2c_sda = Some(9);
        b.uart2_rx = Some(20);
        assert_eq!(reconcile_bindings(&b), 2, "两键失配逐条计数");
    }

    /// INFO 对无 ADS 的板子必须显式 rail=absent，不得把默认 0mV/Ok 冒领成有效采样。
    #[test]
    fn info_marks_absent_rail_explicitly() {
        let absent = info_of(&State::boot());
        assert!(absent.contains("rail=absent"), "{absent}");
        assert!(!absent.contains("rail=0mV"), "{absent}");

        let mut ok = State::boot();
        ok.rail_valid = true;
        ok.rail_mv = 5_240;
        ok.rail_raw = 20_960;
        let line = info_of(&ok);
        assert!(
            line.contains("rail=5240mV raw=20960 rail_tier=ok"),
            "{line}"
        );
    }
}
