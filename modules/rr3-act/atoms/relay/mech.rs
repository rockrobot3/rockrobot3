//! relay 原子件机制主体（五件套③，自原 src/relay.rs 逐字迁入，逻辑零改动）。
//!
//! 通断应用 + 变化判定（0.0.5 批I A-7）+ 运维计数键。参数裁决（只认真布尔）
//! 与载荷组装（result/state_body 的 json!）已上移承载层（0.0.5 批I A-8，
//! ADR-0019 修订 4）：件面只交判定与应用，组装归 `src/composite.rs`，
//! 线上字节逐字不变。
//!
//! 错误面零堆化（0.0.8 批N24 件三，L-3 形状清理）：`rr3_relay_apply` 返回
//! [`Rr3RelayApplyError`] 零堆枚举（机制层交结构化变体，不落 `String` 堆），
//! 人读文案由编排层渲染（`src/composite.rs` 的 `rr3_relay_apply_error_text`，
//! 与改前逐字一致——act 册 §9 契约字面量，面板与 legacy 依赖字面）；HAL 句柄
//! 入参改泛型（`G: Rr3GpioOut + ?Sized`，调用点单态化不改行为）。

use rr_hal_api::{Rr3GpioOut, Rr3HalError};

/// 运维计数键（0.0.4 批B，方案 §4 归属表）：`rr3_relay_apply` 成功后按目标电平
/// 经 bus client 的 counters 通道自报，字面量只在这一个函数
pub fn rr3_relay_counter_key(on: bool) -> &'static str {
    if on {
        "relay_on"
    } else {
        "relay_off"
    }
}

/// [`rr3_relay_apply`] 的零堆错误（批N24 件三）：读/写两腿失败各占一变体，
/// HAL 错原样随行——结构化数据进枚举，`String` 文案在编排层渲染（机制与
/// 策略分离：机制层不落堆、不攒人读文案）。
#[derive(Debug, PartialEq, Eq)]
pub enum Rr3RelayApplyError {
    /// 读回前一电平失败（先读后写的"读"腿：读不到前一就不许盲写）
    ReadBack(Rr3HalError),
    /// 落电平失败（"写"腿）
    Write(Rr3HalError),
}

/// 应用通断并落电平，返回 `(生效, 前一)`（0.0.5 批I A-7：变化判定的依据）。
/// 先读回前一电平再写——HAL 读回失败按业务性失败折 error（与写失败同口径，
/// 读不到前一就不许盲写，relay 是执行件不是妆面件）。泛型 `?Sized`：调用点
/// 持 `&mut dyn Rr3GpioOut` 或具体类型都可直接传（单态化不改行为）。
pub fn rr3_relay_apply<G: Rr3GpioOut + ?Sized>(
    gpio: &mut G,
    on: bool,
) -> Result<(bool, bool), Rr3RelayApplyError> {
    let prev = gpio.level().map_err(Rr3RelayApplyError::ReadBack)?;
    gpio.set(on).map_err(Rr3RelayApplyError::Write)?;
    Ok((on, prev))
}

/// 变化判定（0.0.5 批I A-7）：`relay/state` 只在电平**实际变化**时发布
/// （设计申报口径，act 册 §9 不变量）——重复同态命令不再续 seq 空转。
pub fn rr3_relay_state_due(prev: bool, on: bool) -> bool {
    prev != on
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::sim::Rr3SimGpioOut;

    /// 假件：读腿恒失败（先读后写的"读"腿注入）
    struct Rr3ReadFailGpio;
    impl Rr3GpioOut for Rr3ReadFailGpio {
        fn set(&mut self, _level: bool) -> Result<(), Rr3HalError> {
            unreachable!("读腿失败应先于写腿返回，不应触达写")
        }
        fn level(&self) -> Result<bool, Rr3HalError> {
            Err(Rr3HalError::Busy)
        }
    }

    /// 假件：读腿恒成功、写腿恒失败（"写"腿注入）
    struct Rr3WriteFailGpio;
    impl Rr3GpioOut for Rr3WriteFailGpio {
        fn set(&mut self, _level: bool) -> Result<(), Rr3HalError> {
            Err(Rr3HalError::Busy)
        }
        fn level(&self) -> Result<bool, Rr3HalError> {
            Ok(true)
        }
    }

    /// HAL 失败按腿分变体（批N24 件三）：读失败 → ReadBack、写失败 → Write，
    /// HAL 错原样随行——枚举映射是编排层渲染的依据，分错腿文案就张冠李戴。
    #[test]
    fn apply_maps_hal_failures_to_readback_and_write_variants() {
        assert_eq!(
            rr3_relay_apply(&mut Rr3ReadFailGpio, true).unwrap_err(),
            Rr3RelayApplyError::ReadBack(Rr3HalError::Busy),
            "读腿失败 → ReadBack（读不到前一不许盲写，先读后写）"
        );
        assert_eq!(
            rr3_relay_apply(&mut Rr3WriteFailGpio, true).unwrap_err(),
            Rr3RelayApplyError::Write(Rr3HalError::Busy),
            "写腿失败 → Write"
        );
    }

    /// apply 落电平并回 `(生效, 前一)`——通断语义没有第三态（应答体组装随 A-8
    /// 上移承载层；变化判定依据随 A-7 进返回值）
    #[test]
    fn apply_sets_level_and_reports_previous() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        assert_eq!(
            rr3_relay_apply(&mut gpio, true).unwrap(),
            (true, false),
            "吸合：生效 true，前一 false"
        );
        assert!(gpio.rr3_sim_gpio_level(), "吸合落电平");
        assert_eq!(
            rr3_relay_apply(&mut gpio, false).unwrap(),
            (false, true),
            "断开：生效 false，前一 true"
        );
        assert!(!gpio.rr3_sim_gpio_level(), "断开落电平");
    }

    /// 同态抑制判定（0.0.5 批I A-7）：翻转真、同态假
    #[test]
    fn state_due_only_on_level_change() {
        assert!(rr3_relay_state_due(false, true));
        assert!(rr3_relay_state_due(true, false));
        assert!(!rr3_relay_state_due(true, true), "同态不续发");
        assert!(!rr3_relay_state_due(false, false), "同态不续发");
    }

    /// 计数键两态钉死（0.0.4 批B）：键是模块内部运维词汇，字面量唯一落点
    #[test]
    fn counter_key_distinguishes_on_off() {
        assert_eq!(rr3_relay_counter_key(true), "relay_on");
        assert_eq!(rr3_relay_counter_key(false), "relay_off");
    }

    // ---------- 两腿对拍（architecture §4.0.3 判据②，0.0.5 批K H1） ----------
    // 腿一·确定性：§8 设计行输入序列 [true,false,false,true] 喂两套（件 + sim
    // gpio），(生效, 前一, 变化判定, 落地电平) 轨迹逐拍相等。腿二·正确性：
    // 期望轨迹引自裁定语义（act 册 §9：state 只在电平实际变化时发布——重复
    // 同态不得续发），不由件自产。

    /// 腿一：同序列两跑轨迹相等
    #[test]
    fn parity_leg1_relay_output_train_is_deterministic() {
        fn run() -> Vec<(bool, bool, bool, bool)> {
            let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
            let mut out = Vec::new();
            for on in [true, false, false, true] {
                let (applied, prev) = rr3_relay_apply(&mut gpio, on).unwrap();
                let due = rr3_relay_state_due(prev, applied);
                out.push((applied, prev, due, gpio.rr3_sim_gpio_level()));
            }
            out
        }
        assert_eq!(run(), run(), "同输入两跑逐拍相等（腿一·确定性）");
    }

    /// 腿二：due 序列 = [真, 真, 假, 真]——第三拍 F→F 同态不续发（裁定语义，
    /// A-7 缺陷的回归锚）；前一电平按写前真值显式跟踪钉住
    #[test]
    fn parity_leg2_relay_due_only_on_change() {
        let mut gpio = Rr3SimGpioOut::rr3_sim_gpio_new(false);
        let mut due_seq = Vec::new();
        let mut expect_prev = false; // sim gpio 初值 = 灭
        for on in [true, false, false, true] {
            let (applied, prev) = rr3_relay_apply(&mut gpio, on).unwrap();
            assert_eq!(applied, on, "生效 = 命令值（执行件，不是妆面件）");
            assert_eq!(prev, expect_prev, "前一 = 写前电平（先读回后写）");
            expect_prev = on;
            due_seq.push(rr3_relay_state_due(prev, applied));
        }
        assert_eq!(
            due_seq,
            vec![true, true, false, true],
            "裁定：重复同态不得续发（第三拍 F→F 判假）"
        );
    }
}
