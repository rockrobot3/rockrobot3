//! rr3-act 机制层注册面（0.0.8 W-A 批5 拆分，ADR-0017 五层表机制层）。
//!
//! [`Rr3ActDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）：`on_rpc` =
//! 四臂点名分发（servo/command → led/effect → relay/set → ops/list → 兜底点名
//! ——现行四臂照顺序保留，方案快照「三点名」漏记 ops/list 臂，批N30 计划册
//! 取证笔4 落档）＋ servo/state 限速发布与 relay/state 变化即发在返回前经 bus
//! 完成（发布 → 应答的现行顺序不变）；`on_event` = 两臂（system/power 低电闸门
//! 臂先行＋intent 臂——两道归属门在 intent.rs 原位，本层只做 topic 过滤＋
//! actions 分派＋outcome 发布与日志，方案快照漏记 power 臂，取证笔5 落档）；
//! `on_tick` = led 节拍。原主循环 led tick 在死亡探针前、帧前；serve 骨架 on_tick
//! 在帧后——250ms 量级节拍一圈内毫秒级挪移不可观测，等价性由 SURFACE diff＋
//! 48 测＋chain 两跑承担（批N30 计划册取证笔6）。
//!
//! 失败语义与 connect/sense/interact/power 同一口径（rr3mod 两档）：on_rpc 返
//! Err = 业务性失败折 `{"error": …}` 应答（状态流发布失败走同一返 Err 面——
//! socket 死亡时随后的应答写同样失败经 serve `?` 上抛致命，死亡探针兜底，与
//! 现行 `exit(1)` 口径在可观测面等价）；on_event / on_tick 返 Err = 传输性上抛
//! 致命，模块不装"活着"，死交给 supervisor 按 restart_policy=always 重启。

use std::time::Instant;

use rr3mod::{BusHandle, Dispatcher};
use rr_bus_api::Rr3TimeBase;
use rr_core::control::Rr3LowGate;
use rr_hal_api::{Rr3GpioOut, Rr3PwmServo};
use serde_json::{json, Value};

use crate::boot::{Rr3ActAxisState, Rr3ActCfg};
use crate::intent;
use crate::led::{self, Rr3Led};
use crate::relay;
use crate::rpc;
use crate::servo::{self, Rr3RateGate};

/// 订阅面主题之一（module.toml [events].subscribes 同源）：意图面
const RR3_INTENT_TOPIC: &str = "rr3-brain/intent/result";

/// 订阅面主题之二（MCP-3 申报 2026-10-12）：rrd 的低电旗广播——低电禁动闸门的
/// **唯一触发源**（0.0.5 §20 ② ①：判定已在 power 侧、rrd 置旗已在，件内不再判定
/// 电压，避免第三真值点）。帧体 `{topic, reason:"battery-low", percent, voltage_mv}`。
const RR3_SYSTEM_POWER_TOPIC: &str = "rr3-runtime/system/power";

/// 主循环状态整体持有（自原 composite.rs 局部状态迁入，状态归属随分发走）：
/// 11 字段 = 方案 §4.5 批5 七字段（servo_dev/relay_gpio/cfg/led_ctrl/servo_seq/
/// relay_seq/servo_gate）＋ led_gpio/axis/low_gate/tb 四项（批N30 计划册取证笔2：
/// 方案定稿早于批C② 重铺与批N3/N5 演进，随批订正登记）。
pub(crate) struct Rr3ActDispatcher {
    servo_dev: Box<dyn Rr3PwmServo>,
    /// 裸句柄：led 节拍（on_tick）与 led/effect 点名（on_rpc）两路写
    led_gpio: Box<dyn Rr3GpioOut>,
    relay_gpio: Box<dyn Rr3GpioOut>,
    /// 策略量经 config 数据化注入（时限/限速/节拍/白名单集），机制零策略载荷
    cfg: Rr3ActCfg,
    /// 执行轴状态（批N3 件三）：拒动判据两路（on_rpc servo 臂＋intent 臂）＋行程真值
    axis: Rr3ActAxisState,
    led_ctrl: Rr3Led,
    /// 低电禁动闸门（MC-5）：点名闸（on_rpc）＋event trip＋intent 闸三路共用
    low_gate: Rr3LowGate,
    servo_gate: Rr3RateGate,
    servo_seq: u64,
    relay_seq: u64,
    /// 单调时基（0.0.5 批I A-2）：限速门/led 节拍/led 点名三处同锚 `rr3_now_ms`
    /// （Instant 单调毫秒，只看间隔不看绝对值；旧本地 SystemTime 实现自称"单调"
    /// 是误称（墙钟可回拨），已删）
    tb: Rr3TimeBase,
}

impl Rr3ActDispatcher {
    /// 装配构造（main.rs 装配段调用）：led_ctrl/low_gate/servo_gate/seq/tb 构造
    /// 即初始化（批N29 构造即初始化先例）。
    pub(crate) fn new(
        servo_dev: Box<dyn Rr3PwmServo>,
        mut led_gpio: Box<dyn Rr3GpioOut>,
        relay_gpio: Box<dyn Rr3GpioOut>,
        cfg: Rr3ActCfg,
        axis: Rr3ActAxisState,
    ) -> Self {
        // 主循环状态：限速门 + 流序 + 低电禁动闸门（MC-5）＋ 执行轴状态（main.rs
        // 装配段已解析，批N3 件三建、件五扩形——行程真值随路）。
        // 闸门复位保守默认（§6 复位语义④）：构造即闭锁未武装。host 档现无轨压样本流
        // （rr3-runtime/system/power 自批N5 件二笔3 起两值 reason——battery-low/battery-ok，
        // 解除沿已进场、e2e_power_ok 点名；battery-ok 到本侧只点名不解禁，见 on_event
        // 订阅臂——解禁只认轨压样本），起手武装 + 解禁是**窄化的诚实开局**：假设轨压
        // 健康，把"订阅 battery/state 判新鲜样本再武装"的喂入面挂账批N3 件七（D5 读数位）。
        // trip 随闭锁撤销武装，此后 host 无样本可再武装 → 拒动直至重启（battery-ok
        // 不构成样本，解除归喂入面）。
        let servo_gate = servo::Rr3RateGate::rr3_gate_new(cfg.servo.state_min_interval_ms);
        let mut low_gate = Rr3LowGate::rr3_low_gate_new();
        low_gate.rr3_low_gate_arm();
        low_gate.rr3_low_gate_release();
        let led_ctrl = Rr3Led::rr3_led_new(cfg.led.blink_period_ms);
        // 初始落灭态（原 worker 起手写 0 的语义上收组合件；写失败折日志不退——妆面件）
        led_gpio
            .set(false)
            .unwrap_or_else(|e| eprintln!("rr3-act-led: 初始写电平失败: {e}"));
        Self {
            servo_dev,
            led_gpio,
            relay_gpio,
            cfg,
            axis,
            led_ctrl,
            low_gate,
            servo_gate,
            servo_seq: 0,
            relay_seq: 0,
            tb: Rr3TimeBase::rr3_time_base_new(),
        }
    }
}

impl Dispatcher for Rr3ActDispatcher {
    /// rr3.call 点名应答：四臂 match 照现行顺序（servo→led→relay→ops/list→兜底
    /// 点名）。业务性失败 Err 直接返——serve 骨架折 `{"error": …}` 应答，与现行
    /// `json!({ "error": e })` wire 形状逐字等价（批N26-29 同款）；兜底文案
    /// 「只 exposes …」在这里是**装配事实**不是机制重复（方案 §3 零改动观测面）。
    fn on_rpc(&mut self, topic: &str, args: &Value, bus: &BusHandle) -> Result<Value, String> {
        // 业务性失败折 error 应答（请求-应答语义），不当致命错误。
        // 低电禁动闸门（MC-5 §20 ② ②）：只辖**执行面** servo/relay（点名与
        // 意图两路同判），**妆面 led 不禁**（禁动它只增加卡死面积）。
        let blocked = self.low_gate.rr3_low_gate_is_blocked();
        let outcome = match topic {
            servo::RR3_SERVO_COMMAND_TOPIC => {
                rpc::rr3_handle_servo(
                    &mut *self.servo_dev,
                    &self.cfg.servo,
                    args,
                    blocked,
                    &self.axis,
                )
            }
            led::RR3_LED_EFFECT_TOPIC => rpc::rr3_handle_led(
                &mut self.led_ctrl,
                &mut *self.led_gpio,
                args,
                self.tb.rr3_now_ms(),
                &self.cfg.led.effects,
            ),
            relay::RR3_RELAY_SET_TOPIC => {
                if blocked {
                    Err(rpc::rr3_low_power_reject(relay::RR3_RELAY_SET_TOPIC))
                } else {
                    rpc::rr3_relay_parse_args(args)
                        .and_then(|on| {
                            relay::rr3_relay_apply(&mut *self.relay_gpio, on)
                                .map_err(|e| rpc::rr3_relay_apply_error_text(&e))
                        })
                        .map(|(on, prev)| (rpc::rr3_relay_result(on), None, Some((on, prev))))
                }
            }
            rpc::RR3_OPS_LIST_TOPIC => Ok((
                json!({ "ops": rpc::rr3_act_ops_snapshot(&self.cfg) }),
                None,
                None,
            )),
            other => Err(format!(
                "rr3-act 只 exposes {}/{}/{}/{}，收到 {other}",
                servo::RR3_SERVO_COMMAND_TOPIC,
                led::RR3_LED_EFFECT_TOPIC,
                relay::RR3_RELAY_SET_TOPIC,
                rpc::RR3_OPS_LIST_TOPIC
            )),
        };
        let (reply, state_angle, relay_applied) = outcome?;
        // servo/state 限速流先于应答上路：被限速挡掉的发布不补发，
        // 下一次命令自然携带最新角度（流是摘要不是逐拍镜像）
        if let Some(angle) = state_angle {
            if self.servo_gate.rr3_gate_allow(self.tb.rr3_now_ms()) {
                self.servo_seq += 1;
                bus.publish(
                    servo::RR3_SERVO_STATE_TOPIC,
                    rpc::rr3_servo_state_body(self.servo_seq, angle),
                )?;
            }
        }
        // relay/state 变化即发（0.0.5 批I A-7）：只在电平**真变**时续
        // seq 并发布/计数——同态命令不产生状态帧（§9 不变量；
        // counters 与发布同门，保持键值 = 实际翻转数）
        if let Some((on, prev)) = relay_applied {
            if relay::rr3_relay_state_due(prev, on) {
                self.relay_seq += 1;
                bus.counter_bump(relay::rr3_relay_counter_key(on), 1);
                bus.publish(
                    relay::RR3_RELAY_STATE_TOPIC,
                    rpc::rr3_relay_state_body(self.relay_seq, on),
                )?;
            }
        }
        Ok(reply)
    }

    /// 广播事件（订阅面）：两臂照现行序——system/power 低电闸门臂先行；intent 臂
    /// 两道归属门在 intent.rs 原位，本层只做 topic 过滤＋actions 分派＋outcome
    /// 发布与日志；其余 topic 忽略。
    fn on_event(&mut self, topic: &str, body: &Value, bus: &BusHandle) -> Result<(), String> {
        // 低电禁动闸门（MC-5 §20 ② ①）：触发源唯一 = rrd 低电旗广播
        // `rr3-runtime/system/power`（判定已在 power 侧、rrd 置旗已在，件内
        // 不再判定电压）。置位即闭锁执行面；trip 自带去抖（重复广播不翻转）。
        // 批N5 件二笔3 波及订正：该主题现两值 reason——battery-low 闭锁；
        // **battery-ok 是解除广播，不闭锁也不解禁**（解禁只认轨压样本，复位
        // 语义④；低电段中本件重启会被解除广播误闭锁至重启，缺 reason/未知值
        // 按 fail-closed 闭锁安全向）。
        if topic == RR3_SYSTEM_POWER_TOPIC {
            let reason = body.get("reason").and_then(Value::as_str).unwrap_or("");
            if reason == "battery-ok" {
                eprintln!("低电禁动：battery-ok 解除广播不闭锁（本侧解禁只认轨压样本，喂入面挂账批N3 件七）");
            } else if self.low_gate.rr3_low_gate_trip() {
                eprintln!(
                    "低电禁动：执行面（servo/relay）闭锁，妆面不受辖（{RR3_SYSTEM_POWER_TOPIC} reason={reason}）"
                );
            }
            return Ok(());
        }
        // 订阅面：广播帧只认 `rr3-brain/intent/result`（Q1 裁定：brain 产出意图、
        // 执行器广播认领）。actions 结构化动作经 intent.rs 两道归属门逐条执行，
        // 生效即发 relay/state 摘要；拒执行/非法载荷点名折日志（业务性失败）；
        // 无 actions 字段即纯对话轮，记一行日志。
        if topic != RR3_INTENT_TOPIC {
            return Ok(());
        }
        let trace = body.get("trace").and_then(Value::as_u64).unwrap_or(0);
        match body.get("actions") {
            None | Some(Value::Null) => {
                eprintln!("intent: trace={trace}（纯对话轮，无动作）");
            }
            Some(actions) => {
                for r in intent::rr3_intent_actions_handle(
                    actions,
                    &mut *self.relay_gpio,
                    &mut *self.servo_dev,
                    &self.cfg.servo,
                    self.low_gate.rr3_low_gate_is_blocked(),
                    &self.axis,
                    &self.cfg.intent.open_topics,
                ) {
                    match r {
                        Ok(intent::Rr3IntentOutcome::Relay(on, prev)) => {
                            // 变化即发（0.0.5 批I A-7）：同态命令折日志不发布
                            if relay::rr3_relay_state_due(prev, on) {
                                self.relay_seq += 1;
                                bus.counter_bump(relay::rr3_relay_counter_key(on), 1);
                                bus.publish(
                                    relay::RR3_RELAY_STATE_TOPIC,
                                    rpc::rr3_relay_state_body(self.relay_seq, on),
                                )?;
                                eprintln!(
                                    "intent: trace={trace} relay on={on}，state#{} 已发",
                                    self.relay_seq
                                );
                            } else {
                                eprintln!(
                                    "intent: trace={trace} relay on={on}（同态不续发）"
                                );
                            }
                        }
                        Ok(intent::Rr3IntentOutcome::Servo(angle)) => {
                            // servo/state 限速门同 RPC 路（两条驱动路共用一条状态流）；
                            // 被限速挡掉的发布不补发，下一次动作自然携带最新角度
                            if self.servo_gate.rr3_gate_allow(self.tb.rr3_now_ms()) {
                                self.servo_seq += 1;
                                bus.publish(
                                    servo::RR3_SERVO_STATE_TOPIC,
                                    rpc::rr3_servo_state_body(self.servo_seq, angle),
                                )?;
                                eprintln!(
                                    "intent: trace={trace} servo angle={angle}，state#{} 已发",
                                    self.servo_seq
                                );
                            } else {
                                eprintln!(
                                    "intent: trace={trace} servo angle={angle}（state 被限速挡下）"
                                );
                            }
                        }
                        Err(e) => eprintln!("intent: trace={trace} 拒执行: {e}"),
                    }
                }
            }
        }
        Ok(())
    }

    /// 帧无关节拍：led 节拍（自原 composite.rs 主循环逐字迁入，批I A-1 线程上收
    /// 组合件）：每圈先推状态机，眨眼到点即落电平（写失败折日志不退——妆面件）。
    /// `now`/`bus` 实参 act 不消费——tb 字段承担毫秒时基（限速门/led 节拍/led
    /// 点名三处同锚 `rr3_now_ms`，Instant 换算引入漂移违零改动，批N30 计划册裁定 5）。
    fn on_tick(&mut self, _now: Instant, _bus: &BusHandle) -> Result<(), String> {
        if let Some(level) = led::rr3_led_tick(self.tb.rr3_now_ms(), &mut self.led_ctrl) {
            self.led_gpio
                .set(level)
                .unwrap_or_else(|e| eprintln!("rr3-act-led: 翻转写电平失败: {e}"));
        }
        Ok(())
    }
}
