//! rr3-power 机制层注册面（0.0.8 W-A 批1 拆分，ADR-0017 五层表机制层）。
//!
//! [`Rr3PowerDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）：
//! `on_tick` = battery/charger/rail 三节拍＋低电两沿（批N2 件五起三拍，方案 §4.1
//! 「双节拍」为快照过时表述，批N26 件一订正）；`on_rpc` = `rr3-power/ops/list`
//! 快照应答（批N4 件九二，真事务面——非方案快照所称"无 exposes 纯防御"）；
//! `on_event` = 订阅面为空是**设计**（module.toml 同条注：低压关断是本地决策，
//! 不依赖总线可达），广播只排空。载荷组装面（`*_body`，0.0.5 批I P-7 上移承载层）
//! 与轨压判定核切片（批N2 件五 P-9 归属搬家）随 composite.rs 消解迁本文件，
//! 机制不动、线上字节逐字不变。
//!
//! 错误语义两档（rr3mod 同款）：on_rpc 返 Err = 业务性失败折 `{"error": …}` 应答；
//! on_tick 返 Err = 传输性/设备级失败上抛致命——HAL 读数失败先 counter 后退出，
//! 发布写失败直接上抛，模块不装"活着"，死交给 supervisor 按 restart_policy 重启。

use std::time::{Duration, Instant};

use rr3mod::{BusHandle, Dispatcher};
use rr_hal_api::{rr3_rail_tier_effective, Rr3BatteryState, Rr3Power, Rr3Rail, Rr3RailState, Rr3RailTier};
use serde_json::{json, Value};

use crate::battery::{
    Rr3LowEdge, RR3_BATTERY_LOW_TOPIC, RR3_BATTERY_OK_TOPIC, RR3_BATTERY_STATE_TOPIC,
};
use crate::boot::Rr3PowerCfg;
use crate::charger::RR3_CHARGER_STATE_TOPIC;

/// 电源域电平观测主题（contracts/topics.toml 在册，批N2 件二落的）
const RR3_RAIL_STATE_TOPIC: &str = "rr3-power/rail/state";

/// 本模块 exposes 的 ops 槽快照读口（批N4 件九二，module.toml [rpc].exposes 同源）：
/// 白名单组体应答，槽名与 assemble.toml 槽表申报同源（门 d covers 申报的对账面）
const RR3_OPS_LIST_TOPIC: &str = "rr3-power/ops/list";

/// 主循环状态整体持有（自原 composite.rs 局部状态迁入，状态归属随分发走）：
/// 各流 seq 各自计数、各拍点、低电边沿机（首拍即武装）、轨压判定核。
pub(crate) struct Rr3PowerDispatcher {
    dev: Box<dyn Rr3Power>,
    /// "电池读数失败（{backend}）"文案复用（探活编排随 boot.rs，后端名随句柄走）
    backend: &'static str,
    rail_dev: Box<dyn Rr3Rail>,
    rail_backend: &'static str,
    cfg: Rr3PowerCfg,
    battery_seq: u64,
    charger_seq: u64,
    low_seq: u64,
    ok_seq: u64,
    rail_seq: u64,
    low_edge: Rr3LowEdge,
    rail_judge: Rr3RailJudge,
    next_battery: Instant,
    next_charger: Instant,
    next_rail: Instant,
    /// charger 拍派生源；首拍 battery 之前不发（读数还没着落）——探活首读作种子
    last: Option<Rr3BatteryState>,
}

impl Rr3PowerDispatcher {
    /// 装配构造（main.rs 装配段调用）：探活句柄与首读由 boot.rs 编排产出；
    /// 边沿机按 config 阈值武装，三拍点构造即 `Instant::now()`（首拍即采：
    /// 兜底链路不等第一个周期）。
    pub(crate) fn new(
        dev: Box<dyn Rr3Power>,
        backend: &'static str,
        rail_dev: Box<dyn Rr3Rail>,
        rail_backend: &'static str,
        cfg: Rr3PowerCfg,
        first: Rr3BatteryState,
    ) -> Self {
        Self {
            dev,
            backend,
            rail_dev,
            rail_backend,
            battery_seq: 0,
            charger_seq: 0,
            low_seq: 0,
            ok_seq: 0,
            rail_seq: 0,
            low_edge: Rr3LowEdge::rr3_low_edge_new(cfg.battery.low_voltage_mv),
            rail_judge: Rr3RailJudge::rr3_rail_judge_new(cfg.rail.mild_mv, cfg.rail.severe_mv),
            next_battery: Instant::now(),
            next_charger: Instant::now(),
            next_rail: Instant::now(),
            cfg,
            last: Some(first),
        }
    }
}

impl Dispatcher for Rr3PowerDispatcher {
    /// rr3.call 点名应答（批N4 件九二）：ops/list 快照；未知主题 Err 点名——
    /// serve 折 `{"error": e}` 应答，与现行 wire 形状逐字等价。
    fn on_rpc(&mut self, topic: &str, _args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        if topic == RR3_OPS_LIST_TOPIC {
            Ok(json!({ "ops": rr3_power_ops_snapshot(&self.cfg) }))
        } else {
            Err(format!("rr3-power 只 exposes {RR3_OPS_LIST_TOPIC}，收到 {topic}"))
        }
    }

    /// 订阅面为空是设计（module.toml 同条注）：低压关断是本地决策，不依赖总线
    /// 可达——广播只排空（方案 §4.1 映射表「并入骨架」行）。
    fn on_event(&mut self, _topic: &str, _body: &Value, _bus: &BusHandle) -> Result<(), String> {
        Ok(())
    }

    /// 三节拍（自原 composite.rs 主循环体逐字迁入，拍序不变：battery → charger →
    /// rail）。`now` 由 serve 每圈传入（帧无关节拍，与帧到达无关）。
    fn on_tick(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        // battery 拍：读数 → 周期流 → 低电边沿。HAL 读数失败 = 设备/传输级，上抛
        // 致命；失败先计入 counters（0.0.5 批I P-2：数据新鲜度可观测），再走退出语义。
        if now >= self.next_battery {
            let st = match self.dev.battery() {
                Ok(st) => st,
                Err(e) => {
                    bus.counter_bump("battery_read_fail", 1);
                    return Err(format!("电池读数失败（{}）: {e}", self.backend));
                }
            };
            self.battery_seq += 1;
            bus.publish(
                RR3_BATTERY_STATE_TOPIC,
                rr3_battery_state_body(self.battery_seq, st),
            )?;
            // 低电两个沿、同一采样值，喂入次序是契约（Rr3LowEdge doc 钉死）：
            // **先 recover 后 cross**——cross 的再武装分支会悄悄置回 armed，先喂它
            // 会把恢复沿吞掉（battery/ok 丢失）。恢复沿发布 = rrd 清全局低电旗、
            // 恢复新 spawn 的解除路径（批N5 件二笔3，§17 运维留痕）。
            if self.low_edge.rr3_low_edge_recover(st.voltage_mv) {
                self.ok_seq += 1;
                bus.publish(RR3_BATTERY_OK_TOPIC, rr3_battery_ok_body(self.ok_seq, st))?;
                eprintln!(
                    "battery/ok: {}% {}mV（回阈恢复首拍，段内不重发）",
                    st.percent, st.voltage_mv
                );
            }
            if self.low_edge.rr3_low_edge_cross(st.voltage_mv) {
                self.low_seq += 1;
                bus.publish(RR3_BATTERY_LOW_TOPIC, rr3_battery_low_body(self.low_seq, st))?;
                eprintln!(
                    "battery/low: {}% {}mV（越阈首拍，段内不重发）",
                    st.percent, st.voltage_mv
                );
            }
            self.last = Some(st);
            self.next_battery = now + Duration::from_millis(self.cfg.battery.sample_period_ms);
        }

        // charger 拍：由最近一次电池读数派生；首拍 battery 之前不发（读数还没着落）
        if now >= self.next_charger {
            if let Some(st) = self.last {
                self.charger_seq += 1;
                bus.publish(
                    RR3_CHARGER_STATE_TOPIC,
                    rr3_charger_state_body(self.charger_seq, st.charging),
                )?;
            }
            self.next_charger = now + Duration::from_millis(self.cfg.charger.state_period_ms);
        }

        // rail 拍（批N2 件五）：读域压 → ⑥-c2 按龄期折有效档位 → 判定核 → 周期流
        // ＋边沿守护行。HAL 读数失败同 battery_read_fail 纪律：counter 先行再退——
        // 失败是设备/传输级，上抛致命不装活着。
        if self.cfg.rail.up && now >= self.next_rail {
            let st = match self.rail_dev.rr3_read(&self.cfg.rail.domain) {
                Ok(st) => st,
                Err(e) => {
                    bus.counter_bump("rail_read_fail", 1);
                    return Err(format!("轨压读数失败（{}）: {e}", self.rail_backend));
                }
            };
            self.rail_seq += 1;
            let (tier, event) = rr3_rail_beat(&mut self.rail_judge, st, self.cfg.rail.max_age_ms);
            bus.publish(
                RR3_RAIL_STATE_TOPIC,
                rr3_rail_state_body(self.rail_seq, st.mv, st.age_ms, tier.rr3_rail_tier_name()),
            )?;
            if event == Some(true) {
                bus.counter_bump("rail_edge", 1);
                eprintln!(
                    "rail/edge: {}mV tier={}（越阈首拍，段内不重发）",
                    st.mv,
                    tier.rr3_rail_tier_name()
                );
            }
            self.next_rail = now + Duration::from_millis(self.cfg.rail.sample_period_ms);
        }
        Ok(())
    }
}

// ---------- 载荷组装面（0.0.5 批I P-7：上移承载层，ADR-0019 修订 4） ----------
// 件的对外契约面 = 结构体 + 判定函数；`*_body` 的 json! 组装归本文件，
// 线上字节逐字不变（schema/golden 用例随迁到本文件，继续守字节面）。

/// battery/state 流体：seq 是本流自己的计数（envelope 惯例，从 1 起）
fn rr3_battery_state_body(seq: u64, st: Rr3BatteryState) -> Value {
    json!({
        "seq": seq,
        "voltage_mv": st.voltage_mv,
        "percent": st.percent,
        "charging": st.charging,
    })
}

/// battery/low 事件体：形状钉在 contracts/events/power-battery-low.schema.json
/// （seq/percent/voltage_mv 三必填，additionalProperties=false——不加多余字段）。
fn rr3_battery_low_body(seq: u64, st: Rr3BatteryState) -> Value {
    json!({
        "seq": seq,
        "percent": st.percent,
        "voltage_mv": st.voltage_mv,
    })
}

/// battery/ok 事件体（批N5 件二笔3）：形状钉在
/// contracts/events/power-battery-ok.schema.json——与 low 同形状（三必填、
/// additionalProperties=false），载荷取恢复首拍的读数透传。
fn rr3_battery_ok_body(seq: u64, st: Rr3BatteryState) -> Value {
    json!({
        "seq": seq,
        "percent": st.percent,
        "voltage_mv": st.voltage_mv,
    })
}

/// charger/state 流体：charging 是最近一次 battery 读数的派生值
fn rr3_charger_state_body(seq: u64, charging: bool) -> Value {
    json!({ "seq": seq, "charging": charging })
}

/// `rr3-power/ops/list` 应答体（批N4 件九二，面板 /api/power/ops 读口）：ops 槽
/// 快照，**白名单组体**——槽名与 assemble.toml 槽表申报同源（battery/charger 两组，
/// 门 d covers 申报的对账面），rail 段键不在槽表不在快照面。cutoff_voltage_mv
/// 如实进快照（策略数据在册），"声明未生效"语义由启动日志与文档承载、不在此折算。
fn rr3_power_ops_snapshot(cfg: &Rr3PowerCfg) -> Value {
    json!({
        "battery": {
            "sample_period_ms": cfg.battery.sample_period_ms,
            "low_voltage_mv": cfg.battery.low_voltage_mv,
            "cutoff_voltage_mv": cfg.battery.cutoff_voltage_mv,
        },
        "charger": {
            "state_period_ms": cfg.charger.state_period_ms,
        },
    })
}

/// rail/state 流体（批N2 件五）：四键 `{seq, mv, age_ms, tier}`——tier 串走
/// [`Rr3RailTier::rr3_rail_tier_name`] 同源词表（fresh 拍 = 判定核分类值，Unknown
/// 拍 = "unknown" 原样，展示面不折）。无独立 schema json，四键不加戏。
fn rr3_rail_state_body(seq: u64, mv: u16, age_ms: u32, tier_name: &str) -> Value {
    json!({
        "seq": seq,
        "mv": mv,
        "age_ms": age_ms,
        "tier": tier_name,
    })
}

// ---------- 轨压档位机制切片（批N2 件五，P-9 归属搬家——机制不动） ----------
// 板侧真值 = tools/esp32-periph-base/src/power.rs 的 `tier_of`/`RailJudge`，逐字
// 对照迁入组合件层；板侧文件不改（rtos 档自持）。搬家后两条线同源一个判定核：
// 电平管档位（每拍分类），边沿管事件（每低电段首拍报一次），共用同一对阈值，
// "向下档变"与"边沿事件"逐拍等价。阈值策略量由 config 注入，机制零策略载荷。

/// 电平分类（纯 mV 比较，板侧 `tier_of` 对照）：`mv <= severe ⇒ Severe`、
/// `mv <= mild ⇒ Mild`、否则 Ok。不含迟滞/事件语义（事件走 [`Rr3RailJudge`]）。
fn rr3_rail_tier_of(mv: u16, mild_mv: u16, severe_mv: u16) -> Rr3RailTier {
    if mv <= severe_mv {
        Rr3RailTier::Severe
    } else if mv <= mild_mv {
        Rr3RailTier::Mild
    } else {
        Rr3RailTier::Ok
    }
}

/// 欠压判定核（板侧 `RailJudge` 对照，**一核两条线**）：mild/severe 两条迟滞边沿
/// 各由 [`Rr3LowEdge`] 一实例承载，不另写第二个边沿点（裁定 5：调用方只能喂电压
/// 不能改语义）。event = 两线任一低电段首拍，逐拍等价于向下档变（Ok→Mild 轻度线
/// 报；Ok→Severe / Mild→Severe 重度线报；回阈不报，只是再武装）。
struct Rr3RailJudge {
    mild: Rr3LowEdge,
    severe: Rr3LowEdge,
    mild_mv: u16,
    severe_mv: u16,
}

impl Rr3RailJudge {
    /// 装配：两线阈值取 config rail 段（重度 < 轻度，解析面已执法倒挂）
    fn rr3_rail_judge_new(mild_mv: u16, severe_mv: u16) -> Self {
        Self {
            mild: Rr3LowEdge::rr3_low_edge_new(mild_mv),
            severe: Rr3LowEdge::rr3_low_edge_new(severe_mv),
            mild_mv,
            severe_mv,
        }
    }

    /// 喂一拍轨压 mV：返回 `(本拍电平档位, 是否越阈首发事件)`
    fn rr3_rail_feed(&mut self, mv: u16) -> (Rr3RailTier, bool) {
        let mild_hit = self.mild.rr3_low_edge_cross(mv);
        let severe_hit = self.severe.rr3_low_edge_cross(mv);
        (
            rr3_rail_tier_of(mv, self.mild_mv, self.severe_mv),
            mild_hit || severe_hit,
        )
    }
}

/// ⑥-c2 消费执法可测点（纯函数，批N2 件五）：先按采样龄期折有效档位，再过判定核。
///
/// - 有效档 = Unknown ⇒ `(Unknown, None)`：**不喂判定核**（执行面保持置位），
///   产出侧已折的 Unknown 原样透传——mv 再好也不许从 mv 重分类出 Ok；
/// - 否则 `(tier, Some(event)) = judge.rr3_rail_feed(st.mv)`：判定核 tier 与后端
///   `st.tier` 在同域阈值下一致（机制产出）；0.0.7 真件对账归敞口 12 行。
fn rr3_rail_beat(
    judge: &mut Rr3RailJudge,
    st: Rr3RailState,
    max_age_ms: u32,
) -> (Rr3RailTier, Option<bool>) {
    let effective = rr3_rail_tier_effective(st.tier, Some(st.age_ms), max_age_ms);
    if effective == Rr3RailTier::Unknown {
        return (Rr3RailTier::Unknown, None);
    }
    let (tier, event) = judge.rr3_rail_feed(st.mv);
    (tier, Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(mv: u16, percent: u8) -> Rr3BatteryState {
        Rr3BatteryState {
            voltage_mv: mv,
            percent,
            charging: false,
        }
    }

    #[test]
    fn state_body_carries_full_summary_with_own_seq() {
        let v = rr3_battery_state_body(7, st(3700, 50));
        assert_eq!(v["seq"], 7);
        assert_eq!(v["voltage_mv"], 3700);
        assert_eq!(v["percent"], 50);
        assert_eq!(v["charging"], false);
    }

    /// battery/low 体与 schema 逐字段对齐：三必填，且**没有**多余字段
    /// （additionalProperties=false，多带 charging 会直接违反契约）。
    #[test]
    fn low_body_matches_schema_exactly() {
        let v = rr3_battery_low_body(1, st(3250, 5));
        assert_eq!(v["seq"], 1);
        assert_eq!(v["percent"], 5);
        assert_eq!(v["voltage_mv"], 3250);
        assert_eq!(
            v.as_object().unwrap().len(),
            3,
            "schema additionalProperties=false，字段超集即违约: {v}"
        );
    }

    /// battery/ok 体与 schema 逐字段对齐（批N5 件二笔3）：三必填，且**没有**多余
    /// 字段——与 low 同形状（additionalProperties=false）。
    #[test]
    fn ok_body_matches_schema_exactly() {
        let v = rr3_battery_ok_body(2, st(3310, 8));
        assert_eq!(v["seq"], 2);
        assert_eq!(v["percent"], 8);
        assert_eq!(v["voltage_mv"], 3310);
        assert_eq!(
            v.as_object().unwrap().len(),
            3,
            "schema additionalProperties=false，字段超集即违约: {v}"
        );
    }

    #[test]
    fn charger_body_carries_own_seq_and_charging() {
        let v = rr3_charger_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["charging"], true);
        assert_eq!(v.as_object().unwrap().len(), 2, "两字段，不加戏: {v}");
    }

    // ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------
    // battery/charger 的载荷 body fn 随 0.0.8 批1 拆分落 dispatch.rs（原 composite.rs；
    // 对拍计数门扫描面随批N26 件二扩四路），用例随载荷落本文件。

    #[test]
    fn parity_leg1_battery_output_train_is_deterministic() {
        // 腿一·确定性：同一电压序列喂两跑，(state 体, low 触发) 输出轨迹逐拍相等
        fn run() -> Vec<(Value, bool)> {
            let mut edge = Rr3LowEdge::rr3_low_edge_new(3300);
            [3700u16, 3310, 3300, 3290, 3295, 3400, 3299]
                .iter()
                .map(|&mv| {
                    let st = Rr3BatteryState {
                        voltage_mv: mv,
                        percent: 50,
                        charging: false,
                    };
                    let low = edge.rr3_low_edge_cross(mv);
                    (rr3_battery_state_body(1, st), low)
                })
                .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_battery_matches_ruling_semantics() {
        // 腿二·正确性：低电边沿裁定语义独立重写（真值 = rr-core Rr3LowEdge）
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(!e.rr3_low_edge_cross(3301), "严格大于阈值：不触发");
        assert!(e.rr3_low_edge_cross(3300), "等号算低：v == threshold 触发");
        assert!(!e.rr3_low_edge_cross(3290), "段内不重发（武装已解除）");
        assert!(!e.rr3_low_edge_cross(3300), "段内等号也不重发");
        assert!(!e.rr3_low_edge_cross(3310), "回充不返回 true（只是再武装）");
        assert!(e.rr3_low_edge_cross(3299), "再武装后再次越阈才再发");
        // state 体四字段、low 体三必填无加戏（schema additionalProperties=false）
        let v = rr3_battery_state_body(9, st(3700, 50));
        assert_eq!(v["seq"], 9);
        assert_eq!(v["voltage_mv"], 3700);
        assert_eq!(v["percent"], 50);
        assert_eq!(v["charging"], false);
        assert_eq!(v.as_object().unwrap().len(), 4, "state 四字段: {v}");
        let low = rr3_battery_low_body(1, st(3300, 10));
        assert_eq!(low["percent"], 10);
        assert_eq!(low["voltage_mv"], 3300);
        assert_eq!(low.as_object().unwrap().len(), 3, "low 三必填无加戏: {low}");
    }

    #[test]
    fn parity_leg1_charger_output_train_is_deterministic() {
        // 腿一·确定性：同一 charging 派生序列两跑，(seq, charging) 流逐拍相等
        fn run() -> Vec<(u64, bool)> {
            let mut seq = 0;
            [true, true, false, false, true]
                .iter()
                .map(|&charging| {
                    seq += 1;
                    (
                        seq,
                        rr3_charger_state_body(seq, charging)["charging"]
                            .as_bool()
                            .unwrap(),
                    )
                })
                .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_charger_matches_ruling_semantics() {
        // 腿二·正确性：seq 本流自计数（与 battery/state 的 seq 独立）+ 两字段体
        // + 派生语义（charging 位来自最近一次 battery 读数，body 原样透传）
        let v = rr3_charger_state_body(3, true);
        assert_eq!(v["seq"], 3);
        assert_eq!(v["charging"], true, "charging 位是 battery 读数的派生透传");
        assert_eq!(v.as_object().unwrap().len(), 2, "两字段不加戏: {v}");
        assert_ne!(
            rr3_charger_state_body(1, false)["charging"],
            rr3_charger_state_body(2, true)["charging"],
            "派生值随读数变，不受 seq 影响"
        );
    }

    // ---------- 轨压档位对拍（批N2 件四，⑥-c2 真值面） ----------
    // rr_hal_api 的 rr3_rail_tier_effective 是契约层纯函数（无原子件 mech.rs，⑥-c2
    // 判据单真值点），对拍随其运行期唯一消费模块 rr3-power 落本文件。power 不是
    // atom 名（本模块原子 = battery/charger），⑧-a 计数门不数这两条（不占原子门
    // 分母，命名前缀照模块名走）。

    #[test]
    fn parity_leg1_power_rail_tier_effective_train_is_deterministic() {
        // 腿一·确定性：同一 (档位, 龄期, 上界) 合成序列喂两跑，输出档位序列逐拍相等
        use rr_hal_api::{rr3_rail_tier_effective, Rr3RailTier};
        fn run() -> Vec<Rr3RailTier> {
            let max = 250u32;
            [
                (Rr3RailTier::Ok, Some(0u32)),
                (Rr3RailTier::Ok, Some(max)),     // 边界：不折
                (Rr3RailTier::Ok, Some(max + 1)), // 越上界：折
                (Rr3RailTier::Mild, Some(max)),
                (Rr3RailTier::Mild, Some(max + 1)),
                (Rr3RailTier::Severe, Some(9)),
                (Rr3RailTier::Severe, None),     // 取不到基准：折
                (Rr3RailTier::Unknown, Some(0)), // 已是 Unknown：原样
                (Rr3RailTier::Unknown, None),
            ]
            .iter()
            .map(|&(tier, age)| rr3_rail_tier_effective(tier, age, max))
            .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_power_rail_tier_effective_matches_ruling_semantics() {
        // 腿二·正确性：裁定语义逐条点名（真值 = rr_hal_api rr3_rail_tier_effective，
        // 全案 §6 T-6 运行期读数可信度 + Y-4 如实申报）
        use rr_hal_api::{rr3_rail_tier_effective, Rr3RailTier};
        let max = 250u32;
        // 取不到基准（None）⇒ Unknown，与档位无关——新鲜 Ok 也折不出"可信"
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, None, max),
            Rr3RailTier::Unknown,
            "时基取不到基准 ⇒ Unknown（Y-4：None 是如实申报，不许编龄期冒充新鲜）"
        );
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, None, max),
            Rr3RailTier::Unknown,
            "None 折档与输入档位无关"
        );
        // 越上界 ⇒ Unknown：陈旧的 Severe 与新鲜的 Severe 必须可区分（T-6）
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, Some(max + 1), max),
            Rr3RailTier::Unknown,
            "age > max ⇒ Unknown（陈旧 Severe ≠ 新鲜 Severe，T-6）"
        );
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, Some(max + 1), max),
            Rr3RailTier::Unknown
        );
        // 边界 age == max 不折（新鲜度上界是闭区间内）；龄期内原样透传
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, Some(max), max),
            Rr3RailTier::Severe,
            "age == max 边界不折（闭区间内）"
        );
        assert_eq!(
            rr3_rail_tier_effective(Rr3RailTier::Ok, Some(0), max),
            Rr3RailTier::Ok,
            "新鲜读数原样透传"
        );
        // 档名串同源：第四态 "unknown" 不冒充 "severe"（守护输出行 INFO rail_tier=
        // 与板侧 RailTier::name 同一词表——展示面原样的数据基础）
        assert_eq!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "unknown");
        assert_ne!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "severe");
        // 消费纪律的类型基础（本函数只折档不判处置，执法归⑥-c2 消费侧）：Unknown
        // 是独立第四态（执行面 match 得到独立臂按 Severe 那一侧处置），且折档结果
        // 不许再折成 Ok（展示面原样）
        assert_ne!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, None, max),
            Rr3RailTier::Severe,
            "折出的 Unknown 与 Severe 可区分（执行面独立处置的判据基础）"
        );
        assert_ne!(
            rr3_rail_tier_effective(Rr3RailTier::Severe, None, max),
            Rr3RailTier::Ok,
            "Unknown 不许折算成 Ok（展示面原样不折）"
        );
    }

    // ---------- 轨压监测归属搬家用例（批N2 件五，P-9：机制不动、归属搬家） ----------
    // 判定核切片（rr3_rail_tier_of / Rr3RailJudge）与 ⑥-c2 消费执法点（rr3_rail_beat）
    // 的用例落本文件；板侧 tools/esp32-periph-base 的 tier_of/RailJudge 不改，语义逐字对照。

    #[test]
    fn rail_state_body_carries_four_keys() {
        let v = rr3_rail_state_body(5, 4500, 120, "mild");
        assert_eq!(v["seq"], 5);
        assert_eq!(v["mv"], 4500);
        assert_eq!(v["age_ms"], 120);
        assert_eq!(v["tier"], "mild");
        assert_eq!(v.as_object().unwrap().len(), 4, "四键不加戏: {v}");
    }

    #[test]
    fn rail_judge_feed_matches_board_semantics() {
        // 板侧 RailJudge::feed 逐字对照（mild=4600/severe=4400）：档位 = tier_of 纯
        // mV 分类，event = 两线任一低电段首拍。4500 对 severe 线是"严格回阈 ⇒ 再
        // 武装"（Rr3LowEdge 语义），故 4300 是新段首发再报——与"向下档变 ⇔ 边沿
        // 事件"一致，不是同段重发。
        let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
        let seq = [
            (4700u16, Rr3RailTier::Ok, false),
            (4600, Rr3RailTier::Mild, true),   // 轻度线首发（Ok→Mild）
            (4500, Rr3RailTier::Mild, false),  // 段内不重发
            (4400, Rr3RailTier::Severe, true), // 重度线首发（Mild→Severe）
            (4500, Rr3RailTier::Mild, false),  // 回阈不报（severe 线再武装）
            (4300, Rr3RailTier::Severe, true), // severe 新段首发（4500 已严格回阈）
            (4700, Rr3RailTier::Ok, false),    // 回 Ok（mild 线再武装）
            (4600, Rr3RailTier::Mild, true),   // 再武装后越阈再发
        ];
        for (mv, want_tier, want_event) in seq {
            let (tier, event) = j.rr3_rail_feed(mv);
            assert_eq!(tier, want_tier, "mv={mv} 档位");
            assert_eq!(event, want_event, "mv={mv} 事件");
        }
    }

    #[test]
    fn parity_leg1_power_rail_judge_train_is_deterministic() {
        // 腿一·确定性：同一 mV 序列喂两跑，(档位, 事件) 轨迹逐拍相等
        fn run() -> Vec<(Rr3RailTier, bool)> {
            let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
            [4700u16, 4600, 4500, 4400, 4500, 4300, 4700, 4600]
                .iter()
                .map(|&mv| j.rr3_rail_feed(mv))
                .collect()
        }
        assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
    }

    #[test]
    fn parity_leg2_power_rail_judge_edge_equivalent_to_downward_transition() {
        // 腿二·正确性：边沿事件 ⇔ 向下档变（板侧 RailJudge 类型注的逐拍等价性）。
        // 档序 Ok=0/Mild=1/Severe=2，prev 初值 Ok：event ⇔ rank 增；回阈（rank 降）
        // 不报。与 rail_judge_feed_matches_board_semantics 同一序列、判据独立重写。
        fn rank(t: Rr3RailTier) -> u8 {
            match t {
                Rr3RailTier::Ok => 0,
                Rr3RailTier::Mild => 1,
                Rr3RailTier::Severe => 2,
                Rr3RailTier::Unknown => 3,
            }
        }
        let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
        let mut prev = rank(Rr3RailTier::Ok);
        for mv in [4700u16, 4600, 4500, 4400, 4500, 4300, 4700, 4600] {
            let (tier, event) = j.rr3_rail_feed(mv);
            assert_eq!(event, rank(tier) > prev, "mv={mv}: 事件 ⇔ 向下档变");
            prev = rank(tier);
        }
    }

    #[test]
    fn rail_beat_unknown_not_refed_and_passthrough() {
        // ⑥-c2 消费执法两条（P-15 末列"执行面按保守侧处置、展示面原样显示"的落点）：
        // ① 产出侧已折的 Unknown 原样透传——mv 再好也不许从 mv 重分类出 Ok（不喂核，
        //    执行面保持置位）；
        let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
        let st = Rr3RailState {
            mv: 4700,
            tier: Rr3RailTier::Unknown,
            age_ms: 0,
        };
        let (tier, event) = rr3_rail_beat(&mut j, st, 250);
        assert_eq!(tier, Rr3RailTier::Unknown, "Unknown 原样透传，不折 Ok");
        assert_eq!(event, None, "Unknown 不喂判定核");
        // ② 陈旧读数（age > max）折 Unknown——同样不喂核（T-6：陈旧 Ok ≠ 新鲜 Ok）
        let st = Rr3RailState {
            mv: 4700,
            tier: Rr3RailTier::Ok,
            age_ms: 251,
        };
        let (tier, event) = rr3_rail_beat(&mut j, st, 250);
        assert_eq!(tier, Rr3RailTier::Unknown, "陈旧 Ok 折 Unknown（T-6）");
        assert_eq!(event, None);
        // 新鲜读数（age == max 边界不折）正常喂核：档位 = 判定核分类值（机制产出）
        let st = Rr3RailState {
            mv: 4300,
            tier: Rr3RailTier::Severe,
            age_ms: 250,
        };
        let (tier, event) = rr3_rail_beat(&mut j, st, 250);
        assert_eq!(tier, Rr3RailTier::Severe);
        assert_eq!(event, Some(true), "fresh 拍喂核，severe 线首发");
    }
}
