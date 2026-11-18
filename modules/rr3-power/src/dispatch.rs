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
use rr_hal_api::{
    rr3_rail_tier_fold, Rr3BatteryState, Rr3Power, Rr3Rail, Rr3RailState, Rr3RailTier,
};
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
            Ok(json!({ "ops": rr3_power_ops(&self.cfg) }))
        } else {
            Err(format!(
                "rr3-power 只 exposes {RR3_OPS_LIST_TOPIC}，收到 {topic}"
            ))
        }
    }

    /// 订阅面为空是设计（module.toml 同条注）：低压关断是本地决策，不依赖总线
    /// 可达——广播只排空（方案 §4.1 映射表「并入骨架」行）。
    fn on_event(&mut self, _topic: &str, _body: &Value, _bus: &BusHandle) -> Result<(), String> {
        Ok(())
    }

    /// 三节拍（自原 composite.rs 主循环体逐字迁入，拍序不变：battery → charger →
    /// rail）。`now` 由 serve 每圈传入（帧无关节拍，与帧到达无关）。
    /// B9 起每拍体各自成方法：拍序 = 策略面（本函数），拍体 = 机制面（`rr3_*_beat`）。
    fn on_tick(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        if now >= self.next_battery {
            self.rr3_battery_beat(now, bus)?;
        }
        if now >= self.next_charger {
            self.rr3_charger_beat(now, bus)?;
        }
        if self.cfg.rail.up && now >= self.next_rail {
            self.rr3_rail_tick(now, bus)?;
        }
        Ok(())
    }
}

impl Rr3PowerDispatcher {
    /// battery 拍：读数 → 周期流 → 低电边沿。HAL 读数失败 = 设备/传输级，上抛
    /// 致命；失败先计入 counters（0.0.5 批I P-2：数据新鲜度可观测），再走退出语义。
    fn rr3_battery_beat(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        let st = self.dev.battery().map_err(|e| {
            bus.counter_bump("battery_read_fail", 1);
            format!("电池读数失败（{}）: {e}", self.backend)
        })?;
        self.battery_seq += 1;
        bus.publish(
            RR3_BATTERY_STATE_TOPIC,
            rr3_battery_body(self.battery_seq, st),
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
            bus.publish(
                RR3_BATTERY_LOW_TOPIC,
                rr3_battery_low_body(self.low_seq, st),
            )?;
            eprintln!(
                "battery/low: {}% {}mV（越阈首拍，段内不重发）",
                st.percent, st.voltage_mv
            );
        }
        self.last = Some(st);
        self.next_battery = now + Duration::from_millis(self.cfg.battery.sample_period_ms);
        Ok(())
    }

    /// charger 拍：由最近一次电池读数派生；首拍 battery 之前不发（读数还没着落）
    fn rr3_charger_beat(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        if let Some(st) = self.last {
            self.rr3_charger_pub(bus, st)?;
        }
        self.next_charger = now + Duration::from_millis(self.cfg.charger.state_period_ms);
        Ok(())
    }

    /// charger/state 单帧（续 seq 后发布，charging 位由调用方透传）。
    fn rr3_charger_pub(&mut self, bus: &BusHandle, st: Rr3BatteryState) -> Result<(), String> {
        self.charger_seq += 1;
        bus.publish(
            RR3_CHARGER_STATE_TOPIC,
            rr3_charger_body(self.charger_seq, st.charging),
        )
    }

    /// rail 拍（批N2 件五）：读域压 → ⑥-c2 按龄期折有效档位 → 判定核 → 周期流
    /// ＋边沿守护行。HAL 读数失败同 battery_read_fail 纪律：counter 先行再退——
    /// 失败是设备/传输级，上抛致命不装活着。
    fn rr3_rail_tick(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        let st = self.rr3_rail_state(bus)?;
        self.rr3_rail_emit(bus, st)?;
        self.next_rail = now + Duration::from_millis(self.cfg.rail.sample_period_ms);
        Ok(())
    }

    /// 判定 + 周期流 + 越阈通告（续 seq 与通告同门）。
    fn rr3_rail_emit(&mut self, bus: &BusHandle, st: Rr3RailState) -> Result<(), String> {
        self.rail_seq += 1;
        let (tier, event) = rr3_rail_beat(&mut self.rail_judge, st, self.cfg.rail.max_age_ms);
        self.rr3_rail_pub(bus, st, tier)?;
        if event == Some(true) {
            Self::rr3_rail_notice(bus, &st, tier);
        }
        Ok(())
    }

    /// 周期流发布（seq 已在调用方续号）。
    fn rr3_rail_pub(
        &self,
        bus: &BusHandle,
        st: Rr3RailState,
        tier: Rr3RailTier,
    ) -> Result<(), String> {
        bus.publish(
            RR3_RAIL_STATE_TOPIC,
            rr3_rail_state_body(self.rail_seq, st.mv, st.age_ms, tier.rr3_rail_tier_name()),
        )?;
        Ok(())
    }

    /// 越阈首拍通告：计数 + 一行台账（段内不重发，由判定核保证）。
    fn rr3_rail_notice(bus: &BusHandle, st: &Rr3RailState, tier: Rr3RailTier) {
        bus.counter_bump("rail_edge", 1);
        eprintln!(
            "rail/edge: {}mV tier={}（越阈首拍，段内不重发）",
            st.mv,
            tier.rr3_rail_tier_name()
        );
    }

    /// 轨压读数：失败先记账再退（设备/传输级，上抛致命不装活着）。
    fn rr3_rail_state(&self, bus: &BusHandle) -> Result<Rr3RailState, String> {
        self.rail_dev
            .rr3_read(&self.cfg.rail.domain)
            .map_err(|e| rr3_rail_read_err(bus, self.rail_backend, e))
    }
}

/// 轨压读失败文案（记账先行，退因与后端名同句）。
fn rr3_rail_read_err(bus: &BusHandle, backend: &str, e: impl std::fmt::Display) -> String {
    bus.counter_bump("rail_read_fail", 1);
    format!("轨压读数失败（{backend}）: {e}")
}

// ---------- 载荷组装面（0.0.5 批I P-7：上移承载层，ADR-0019 修订 4） ----------
// 件的对外契约面 = 结构体 + 判定函数；`*_body` 的 json! 组装归本文件，
// 线上字节逐字不变（schema/golden 用例随迁到本文件，继续守字节面）。

/// battery/state 流体：seq 是本流自己的计数（envelope 惯例，从 1 起）
fn rr3_battery_body(seq: u64, st: Rr3BatteryState) -> Value {
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
fn rr3_charger_body(seq: u64, charging: bool) -> Value {
    json!({ "seq": seq, "charging": charging })
}

/// `rr3-power/ops/list` 应答体（批N4 件九二，面板 /api/power/ops 读口）：ops 槽
/// 快照，**白名单组体**——槽名与 assemble.toml 槽表申报同源（battery/charger 两组，
/// 门 d covers 申报的对账面），rail 段键不在槽表不在快照面。cutoff_voltage_mv
/// 如实进快照（策略数据在册），"声明未生效"语义由启动日志与文档承载、不在此折算。
fn rr3_power_ops(cfg: &Rr3PowerCfg) -> Value {
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
    let effective = rr3_rail_tier_fold(st.tier, Some(st.age_ms), max_age_ms);
    if effective == Rr3RailTier::Unknown {
        return (Rr3RailTier::Unknown, None);
    }
    let (tier, event) = judge.rr3_rail_feed(st.mv);
    (tier, Some(event))
}

#[cfg(test)]
mod tests;
