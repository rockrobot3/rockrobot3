//! servo 原子件机制主体（五件套③，自原 src/servo.rs 逐字迁入，逻辑零改动）。
//!
//! servo/command 的 Value 下钻（缺参/类型错/越 u16）已上移承载层（0.0.5 批I
//! A-8，ADR-0019 修订 4）：行程（[`rr3_servo_check_travel`]）与时限
//! （[`rr3_servo_check_timeout`]，缺 move_ms 回落 config 上限——实机后端按它
//! 限时驱动，堵转由反射环兜）的值域判定留件，文案契约由编排层渲染、逐字
//! 保持（0.0.8 批N24 件三零堆化，见下）。servo/state 限速门：状态变化必发布，
//! 但两次发布至少隔 min_interval
//! ——被限掉的这一次不缓存不补发，流是摘要不是逐拍镜像（命令面本就 <100Hz，
//! 限速只挡突发连发）。
//!
//! `Rr3RateGate` 真值在 rr-core `control`（批D₃，ADR-0018 裁定 5：机制核纯
//! 函数 + 注入时钟）——本文件 re-export 保既有路径，时钟由调用方给 ms。
//!
//! 错误面零堆化（0.0.8 批N24 件三，L-3 形状清理）：行程/时限判定的失败交
//! [`Rr3ServoTravelError`]/[`Rr3ServoTimeoutError`] 零堆枚举（动态数据——角度/
//! 毫秒/边界——进枚举字段，机制层不落 `String` 堆），人读文案由编排层渲染
//! （`src/composite.rs` 的 `rr3_servo_parse_args`，与改前逐字一致——act 册 §9
//! 契约字面量被单测逐字断言，面板与 legacy 依赖字面）。

use super::Rr3ServoCfg;
use rr_hal_api::Rr3ActMotion;

pub use rr_core::control::Rr3RateGate;

/// servo/command 动作形态判定（批N3 件三，值域留件）：词缺席 = auto（保守——
/// 不申报按自动）。词表解析走 rr-hal-api 单真值点，运行命令面白名单窄化到
/// {jog, auto}——calib-* 两词是标定面形态（判据 rr-hal-api 在册），其运行受理
/// 随批N2 逐域电流采样到位后开（全案 §6 路线 C 自动段）；N2 收口前标定只准
/// 单步点动＋人工确认，标定产物的落盘走 rrd 的 axis_write 写入面（批N3 件四），
/// 不经运行命令面。形态合法性（未标定拒自动等）在
/// [`rr_hal_api::rr3_act_motion_allowed`]，调用方持轴状态判，本判定只管词面。
///
/// 错误零堆化（批N34 件二，批N24 件三同件族收尾）：词不进运行命令面交
/// [`Rr3ServoMotionError`]（词本身是动态数据，借用进字段不落堆），人读文案由
/// 编排层渲染（`src/rpc.rs` 的 `rr3_servo_motion_reject`，与改前逐字一致——
/// error wire 契约字面量，面板与 legacy 依赖字面）。
pub fn rr3_servo_check_motion(word: Option<&str>) -> Result<Rr3ActMotion, Rr3ServoMotionError<'_>> {
    match word {
        None => Ok(Rr3ActMotion::Auto),
        Some(s) => match Rr3ActMotion::rr3_act_motion_parse(s) {
            Some(m @ (Rr3ActMotion::Jog | Rr3ActMotion::Auto)) => Ok(m),
            _ => Err(Rr3ServoMotionError::UnknownWord(s)),
        },
    }
}

/// [`rr3_servo_check_motion`] 的零堆错误（批N34 件二，批N24 件三同件族）：
/// 词表外词与 calib-* 标定面词统一一变体（运行命令面只认 jog/auto），词本身
/// 借用进字段（渲染点名的数据源），人读文案在编排层渲染（契约字面量不变）。
#[derive(Debug, PartialEq, Eq)]
pub enum Rr3ServoMotionError<'a> {
    /// 词不进运行命令面：携带原词
    UnknownWord(&'a str),
}

/// [`rr3_servo_check_travel`] 的零堆错误（批N24 件三）：越界动态值进字段，
/// 人读文案在编排层渲染（契约字面量不变）。
#[derive(Debug, PartialEq, Eq)]
pub enum Rr3ServoTravelError {
    /// 越行程：携带命令角与行程区间（渲染点名区间）
    OutOfRange {
        /// 命令角（度）
        angle_deg: u16,
        /// 行程下限（度）——档案 lo
        min_deg: u16,
        /// 行程上限（度）——档案 hi
        max_deg: u16,
    },
}

/// [`rr3_servo_check_timeout`] 的零堆错误（批N24 件三）：零时限与超上限各一
/// 变体，动态值进字段，人读文案在编排层渲染（契约字面量不变）。
#[derive(Debug, PartialEq, Eq)]
pub enum Rr3ServoTimeoutError {
    /// 零时限拒收——"超时即停，不给堵转留机会"
    ZeroMs,
    /// 超上限：携带命令毫秒与 config 上限（渲染点名上限）
    BeyondLimit {
        /// 命令时限（毫秒）
        ms: u64,
        /// 上限（毫秒）——config move_timeout_ms
        limit: u64,
    },
}

/// servo/command 行程判定（值域留件，批I A-8 拆层）：越行程携带命令角与区间。
/// 缺参/类型错/越 u16 的下钻文案在承载层（每句落在唯一一处）。
pub fn rr3_servo_check_travel(angle: u16, cfg: &Rr3ServoCfg) -> Result<(), Rr3ServoTravelError> {
    if !(cfg.angle_min_deg..=cfg.angle_max_deg).contains(&angle) {
        return Err(Rr3ServoTravelError::OutOfRange {
            angle_deg: angle,
            min_deg: cfg.angle_min_deg,
            max_deg: cfg.angle_max_deg,
        });
    }
    Ok(())
}

/// servo/command 时限判定（值域留件，批I A-8 拆层）：`None` 回落 config 上限，
/// 零时限与超上限拒收——"超时即停，不给堵转留机会"。
pub fn rr3_servo_check_timeout(
    move_ms: Option<u64>,
    cfg: &Rr3ServoCfg,
) -> Result<u64, Rr3ServoTimeoutError> {
    match move_ms {
        None => Ok(cfg.move_timeout_ms),
        Some(0) => Err(Rr3ServoTimeoutError::ZeroMs),
        Some(ms) if ms > cfg.move_timeout_ms => Err(Rr3ServoTimeoutError::BeyondLimit {
            ms,
            limit: cfg.move_timeout_ms,
        }),
        Some(ms) => Ok(ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Rr3ServoCfg {
        Rr3ServoCfg {
            angle_min_deg: 0,
            angle_max_deg: 180,
            move_timeout_ms: 800,
            state_min_interval_ms: 50,
        }
    }

    #[test]
    fn check_travel_accepts_bounds_and_rejects_out_of_range() {
        assert!(rr3_servo_check_travel(0, &cfg()).is_ok(), "行程下界合法");
        assert!(rr3_servo_check_travel(180, &cfg()).is_ok(), "行程上界合法");
        assert_eq!(
            rr3_servo_check_travel(181, &cfg()).unwrap_err(),
            Rr3ServoTravelError::OutOfRange {
                angle_deg: 181,
                min_deg: 0,
                max_deg: 180
            },
            "越界携带命令角与行程区间（编排层渲染点名的数据源）"
        );
    }

    #[test]
    fn check_timeout_defaults_to_cfg_and_rejects_zero_beyond_limit() {
        assert_eq!(
            rr3_servo_check_timeout(None, &cfg()).unwrap(),
            800,
            "缺 move_ms 取 config 上限"
        );
        assert_eq!(rr3_servo_check_timeout(Some(200), &cfg()).unwrap(), 200);
        assert_eq!(rr3_servo_check_timeout(Some(800), &cfg()).unwrap(), 800);
        assert_eq!(
            rr3_servo_check_timeout(Some(801), &cfg()).unwrap_err(),
            Rr3ServoTimeoutError::BeyondLimit { ms: 801, limit: 800 },
            "超上限携带命令毫秒与上限（编排层渲染点名的数据源）"
        );
        assert_eq!(
            rr3_servo_check_timeout(Some(0), &cfg()).unwrap_err(),
            Rr3ServoTimeoutError::ZeroMs,
            "零时限拒收"
        );
    }

    /// 限速门判定真值随批D₃ 迁 rr-core `control`（逐拍/窗口用例随迁）——
    /// 本地只钉"re-export 可用"：同一构造 + 一次注入调用走通 rr-core 面。
    #[test]
    fn rate_gate_is_rr_core_reexport() {
        let mut gate = Rr3RateGate::rr3_gate_new(50);
        assert!(gate.rr3_gate_allow(1_000));
        assert!(!gate.rr3_gate_allow(1_010), "窗内连发被限（rr-core 判定）");
    }

    /// 动作形态判定（批N3 件三）：缺席 = auto 保守（不申报按自动）；jog/auto
    /// 在册放行；calib-* 标定面词与词表外词在运行命令面点名拒。
    #[test]
    fn check_motion_defaults_to_auto_and_narrows_to_runface_words() {
        assert_eq!(
            rr3_servo_check_motion(None).unwrap(),
            Rr3ActMotion::Auto,
            "词缺席 = auto（保守）"
        );
        assert_eq!(
            rr3_servo_check_motion(Some("jog")).unwrap(),
            Rr3ActMotion::Jog
        );
        assert_eq!(
            rr3_servo_check_motion(Some("auto")).unwrap(),
            Rr3ActMotion::Auto
        );
        assert_eq!(
            rr3_servo_check_motion(Some("calib-openloop")).unwrap_err(),
            Rr3ServoMotionError::UnknownWord("calib-openloop"),
            "标定面词不进运行命令面（词随枚举走，渲染逐字由编排层单测钉）"
        );
        assert_eq!(
            rr3_servo_check_motion(Some("walk")).unwrap_err(),
            Rr3ServoMotionError::UnknownWord("walk"),
            "词表外点名拒（词随枚举走）"
        );
    }

    // ---------- 两腿对拍（architecture §4.0.3 判据②，0.0.5 批K H1） ----------
    // 腿一·确定性：同一 (角度, 时限, 拍点) 序列喂两套判定（行程/时限/限速门），
    // 判定轨迹逐拍相等。腿二·正确性：期望值引自裁定语义（act 册 §8 servo 行：
    // 端点等号合法、越界点名区间、时限零值/超上限拒收、限速窗起放行/窗内被限/
    // 窗末等号放行），不由件自产。

    /// 对拍快照：((行程判定, 行程错型), (时限判定, 时限错型), 限速门放行)
    type ServoParitySnap = (
        (bool, Option<Rr3ServoTravelError>),
        (bool, Option<Rr3ServoTimeoutError>),
        bool,
    );

    /// 腿一：§8 设计行序列（端点/越界/零时限/超上限 + now 每 10ms 一拍）两跑相等
    #[test]
    fn parity_leg1_servo_judgement_train_is_deterministic() {
        fn run() -> Vec<ServoParitySnap> {
            let mut gate = Rr3RateGate::rr3_gate_new(50);
            let mut out = Vec::new();
            let mut now = 1_000u64;
            for (angle, ms) in [
                (0u16, None),
                (180, Some(800)),
                (181, Some(200)),
                (90, Some(0)),
                (45, Some(801)),
            ] {
                let travel = rr3_servo_check_travel(angle, &cfg());
                let timeout = rr3_servo_check_timeout(ms, &cfg());
                out.push((
                    (travel.is_ok(), travel.err()),
                    (timeout.is_ok(), timeout.err()),
                    gate.rr3_gate_allow(now),
                ));
                now += 10;
            }
            out
        }
        assert_eq!(run(), run(), "同输入两跑逐拍相等（腿一·确定性）");
    }

    /// 腿二：裁定语义逐点断言（含文案点名区间/上限——面板与 legacy 依赖字面）
    #[test]
    fn parity_leg2_servo_matches_ruling_semantics() {
        assert!(
            rr3_servo_check_travel(0, &cfg()).is_ok(),
            "行程下界等号合法"
        );
        assert!(
            rr3_servo_check_travel(180, &cfg()).is_ok(),
            "行程上界等号合法"
        );
        assert!(rr3_servo_check_travel(181, &cfg()).is_err(), "越界拒收");
        assert_eq!(
            rr3_servo_check_travel(181, &cfg()).unwrap_err(),
            Rr3ServoTravelError::OutOfRange {
                angle_deg: 181,
                min_deg: 0,
                max_deg: 180
            },
            "越界点名行程区间（区间随枚举走，渲染逐字由编排层单测钉）"
        );
        assert_eq!(
            rr3_servo_check_timeout(None, &cfg()).unwrap(),
            800,
            "缺 move_ms 回落 config 上限（裁定）"
        );
        assert!(
            rr3_servo_check_timeout(Some(0), &cfg()).is_err(),
            "零时限拒收（超时即停）"
        );
        assert!(
            rr3_servo_check_timeout(Some(801), &cfg()).is_err(),
            "超上限拒收（不给堵转留机会）"
        );
        assert_eq!(
            rr3_servo_check_timeout(Some(800), &cfg()).unwrap(),
            800,
            "上限等号合法"
        );
        // 限速门（rr-core 裁定）：窗起放行、窗末前一 ms 被限、窗末等号放行
        let mut gate = Rr3RateGate::rr3_gate_new(50);
        assert!(gate.rr3_gate_allow(1_000), "窗起放行");
        assert!(!gate.rr3_gate_allow(1_049), "窗内被限");
        assert!(gate.rr3_gate_allow(1_050), "窗末等号放行");
    }
}
