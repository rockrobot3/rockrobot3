//! rr3-act 接入层（0.0.8 W-A 批5 拆分，ADR-0017 五层表接入层）。
//!
//! config schema 解析（自原 composite.rs 迁入：TOML 下钻、缺字段点名——文案是
//! 装配观测面，逐字不变）＋ 执行轴状态装载（批N3 件三建、件五扩形：类型＋五键
//! 严格解析随迁，env 读＋坏形/缺席折保守缺省并点名，安全向不退）＋ HAL 执行面
//! 三面选择与开机探活编排（Unsupported / 探活读失败即 Err 上抛——显式回错不
//! 静默带病常驻，也不给"半个执行器"的假活）＋ 反射环宿主线程 spawn（0.0.8 方案
//! §4.6：该 BusHandle clone 只归反射环，反射环与 Dispatcher 零共享状态）。
//! config 路径解析（RR3_ACT_CONFIG_ENV 覆盖 + manifest 同目录缺省）已随批0 并入
//! rr3mod `load_config_text` 单点，本层不再持有路径段（RR3_MANIFEST_ENV 本地常量
//! 随批退役）。本层零业务 match——策略量经 config 数据化注入，机制件零策略载荷。

mod axis;
mod cfg;
mod probe;
mod report;
mod spawn;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "remote.rs"]
mod remote;

pub(crate) use axis::{rr3_axis_state_load, Rr3ActAxisState};
pub(crate) use cfg::{rr3_act_config_parse, Rr3ActCfg};
pub(crate) use probe::rr3_act_probe;
pub(crate) use report::{rr3_act_banner, rr3_intent_open};
pub(crate) use spawn::rr3_act_spawn_reflex;

#[cfg(test)]
mod tests {
    use super::probe::rr3_opt_face;
    // 桥共享判据只在宿主档存在（板档无跨进程命令面，探活臂同门），用例随其同门。
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use super::probe::rr3_shared_rule;
    use super::*;
    use crate::led::Rr3LedEffect;
    use rr_hal_api::Rr3HalError;

    /// 轴状态注入文本解析（批N3 件三建、件五扩五键）：正例键序无关；缺键/
    /// 坏布尔/词表外词/未知键/重复键/越 u16 逐条点名红——装配链注入的文本坏了
    /// 必须看得见，不许静默猜。
    #[test]
    fn axis_state_env_parses_strict() {
        let ok =
            Rr3ActAxisState::rr3_from_env("calibrated=true,hard_stop=yes,lo=60,hi=120,center=90")
                .unwrap();
        assert_eq!(
            ok,
            Rr3ActAxisState {
                calibrated: true,
                hard_stop: rr_hal_api::Rr3ActHardStop::Yes,
                lo_deg: 60,
                hi_deg: 120,
                center_deg: 90
            }
        );
        let ok = Rr3ActAxisState::rr3_from_env(
            "hard_stop=unknown,calibrated=false,lo=0,hi=180,center=90",
        )
        .unwrap();
        assert_eq!(
            ok,
            Rr3ActAxisState {
                calibrated: false,
                hard_stop: rr_hal_api::Rr3ActHardStop::Unknown,
                lo_deg: 0,
                hi_deg: 180,
                center_deg: 90
            },
            "键序无关"
        );
        for (bad, needle) in [
            (
                "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90,x=1",
                "未知键",
            ),
            (
                "calibrated=true,hard_stop=yes,center=90",
                "lo（档案行程下限）",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=60,hi=120",
                "center（档案中位锚）",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=60,center=90",
                "hi（档案行程上限）",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=60,hi=120,center=70000",
                "u16",
            ),
            (
                "calibrated=true,hard_stop=yes,lo=abc,hi=120,center=90",
                "u16",
            ),
        ] {
            let e = Rr3ActAxisState::rr3_from_env(bad).unwrap_err();
            assert!(e.contains(needle), "{bad:?} → {e}");
        }
    }

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红。
    /// 行程键不在 config（批N3 件五门 g 收敛），本夹具与仓内文件同形。
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-act.servo]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500
effects = ["on", "off", "blink"]

[rr3-act.intent]
open_topics = ["rr3-act/relay/set", "rr3-act/servo/command"]

[rr3-act.reflex]
loop_hz = 1000
saturate_notice_ms = 500
"#;
        let cfg = rr3_act_config_parse(text).unwrap();
        // 行程两键在 config 解析面恒为占位 0/180——真值由装配段 rr3_act_run 用
        // 执行轴状态覆填（门 g 收敛：档案是唯一真值点）
        assert_eq!(cfg.servo.angle_min_deg, 0);
        assert_eq!(cfg.servo.angle_max_deg, 180);
        assert_eq!(cfg.servo.move_timeout_ms, 800);
        assert_eq!(cfg.servo.state_min_interval_ms, 50);
        assert_eq!(cfg.led.blink_period_ms, 500);
        assert_eq!(cfg.led.effects, led_effects_all(), "灯效白名单子集注入");
        assert_eq!(
            cfg.intent.open_topics,
            vec![
                "rr3-act/relay/set".to_string(),
                "rr3-act/servo/command".to_string()
            ],
            "意图开放集注入"
        );
        assert_eq!(cfg.reflex.loop_hz, 1000);
        assert_eq!(cfg.reflex.saturate_notice_ms, 500);
    }

    /// 桥共享判据：只有舵机面与音频面同选 esp32 才归 sense 独占。
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn shared_rule_only_when_both_esp32() {
        assert!(rr3_shared_rule("esp32", "esp32"));
        assert!(!rr3_shared_rule("esp32", "linux"));
        assert!(!rr3_shared_rule("linux", "esp32"));
        assert!(!rr3_shared_rule("sim", "sim"));
    }

    /// 可缺面门：占位（装配单未启用该格）只点名不致命；Busy/参数错是真故障。
    #[test]
    fn opt_face_degrades_only_placeholder() {
        assert!(rr3_opt_face("LED", "placeholder", Err(Rr3HalError::Unsupported("x"))).is_ok());
        assert!(rr3_opt_face("LED", "linux", Err(Rr3HalError::Busy)).is_err());
        assert!(rr3_opt_face("继电器", "linux", Ok(true)).is_ok());
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_act_config_parse("").unwrap_err();
        assert!(e.contains("[rr3-act.servo]"), "缺段点名: {e}");
        // 行程键已撤（批N3 件五门 g），servo 节只剩时限/限速两必填键——
        // 全节齐备即应解析通过（同仓内 config.toml 形状）
        let ok = rr3_act_config_parse(
            "[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\neffects = [\"on\"]\n[rr3-act.intent]\nopen_topics = []\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n",
        );
        assert!(ok.is_ok(), "时限/限速两键齐备即完整: {:?}", ok.err());
        // 缺必填键照旧点名（换 move_timeout_ms 当靶）
        let e = rr3_act_config_parse(
            "[rr3-act.servo]\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\neffects = [\"on\"]\n[rr3-act.intent]\nopen_topics = []\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n",
        )
        .unwrap_err();
        assert!(e.contains("move_timeout_ms"), "缺字段点名: {e}");
        let e = rr3_act_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }

    /// loop_hz 越界拒 boot（0.0.5 批I A-3）：装不进 u32 的值点名拒绝，
    /// 不得被 `as u32` 裸截断成小值静默变形
    #[test]
    fn config_parse_rejects_loop_hz_overflow() {
        let text = r#"
[rr3-act.servo]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500
effects = ["on"]

[rr3-act.intent]
open_topics = []

[rr3-act.reflex]
loop_hz = 5000000000
saturate_notice_ms = 500
"#;
        let e = rr3_act_config_parse(text).unwrap_err();
        assert!(e.contains("loop_hz"), "越界点名: {e}");
        // 本轮审计：装得进 u32 但大到让 1_000_000/hz 归零的档位同样拒
        let text = text.replace("loop_hz = 5000000000", "loop_hz = 4000000000");
        let e = rr3_act_config_parse(&text).unwrap_err();
        assert!(
            e.contains("loop_hz") && e.contains("满转自旋"),
            "超大 loop_hz 点名: {e}"
        );
    }

    /// 饱和通告窗越界拒 boot（本轮静态审计）：`rr3_reflex_plan` 的窗数折算
    /// `div_ceil(…) as u32` 会把无上界巨值静默截断，语义失真且不可定位。
    #[test]
    fn config_parse_rejects_notice_overflow() {
        let text = r#"
[rr3-act.servo]
move_timeout_ms = 800
state_min_interval_ms = 50

[rr3-act.led]
blink_period_ms = 500
effects = ["on"]

[rr3-act.intent]
open_topics = []

[rr3-act.reflex]
loop_hz = 1000
saturate_notice_ms = 18446744073709551615
"#;
        let e = rr3_act_config_parse(text).unwrap_err();
        assert!(e.contains("saturate_notice_ms"), "越界点名: {e}");
        let ok = text.replace(
            "saturate_notice_ms = 18446744073709551615",
            "saturate_notice_ms = 600000",
        );
        assert_eq!(
            rr3_act_config_parse(&ok).unwrap().reflex.saturate_notice_ms,
            600_000,
            "上界本身合法"
        );
    }

    /// A-9 fail-closed：灯效白名单子集与意图开放集逐条点名——缺键/类型错/
    /// 越界值都拒 boot（空数组合法，另行覆盖）
    #[test]
    fn config_parse_rejects_bad_effects_and_open_topics() {
        // 基座（四段齐备），按需替换 led.effects / intent.open_topics 行
        let base = |led: &str, top: &str| {
            format!(
                "[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\n{led}\n[rr3-act.intent]\n{top}\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n"
            )
        };
        // 缺 effects 键
        let e = rr3_act_config_parse(&base("", "open_topics = []")).unwrap_err();
        assert!(e.contains("effects"), "缺键点名: {e}");
        // 非数组
        let e = rr3_act_config_parse(&base("effects = 1", "open_topics = []")).unwrap_err();
        assert!(e.contains("effects"), "非数组点名: {e}");
        // 元素非字符串
        let e = rr3_act_config_parse(&base("effects = [1]", "open_topics = []")).unwrap_err();
        assert!(e.contains("effects[0]"), "元素类型点名: {e}");
        // 越在册值
        let e =
            rr3_act_config_parse(&base("effects = [\"pulse\"]", "open_topics = []")).unwrap_err();
        assert!(e.contains("非在册灯效"), "越界值点名在册值: {e}");
        // 缺 intent 段
        let e = rr3_act_config_parse(
            "[rr3-act.servo]\nmove_timeout_ms = 800\nstate_min_interval_ms = 50\n[rr3-act.led]\nblink_period_ms = 500\neffects = [\"on\"]\n[rr3-act.reflex]\nloop_hz = 1000\nsaturate_notice_ms = 500\n",
        )
        .unwrap_err();
        assert!(e.contains("[rr3-act.intent]"), "缺段点名: {e}");
        // 缺 open_topics 键
        let e = rr3_act_config_parse(&base("effects = [\"on\"]", "")).unwrap_err();
        assert!(e.contains("open_topics"), "缺键点名: {e}");
        // 非数组
        let e = rr3_act_config_parse(&base("effects = [\"on\"]", "open_topics = 1")).unwrap_err();
        assert!(e.contains("open_topics"), "非数组点名: {e}");
        // 元素非字符串
        let e = rr3_act_config_parse(&base("effects = [\"on\"]", "open_topics = [1]")).unwrap_err();
        assert!(e.contains("open_topics[0]"), "元素类型点名: {e}");
        // 越界主题（不在本模块执行臂内）
        let e = rr3_act_config_parse(&base(
            "effects = [\"on\"]",
            "open_topics = [\"rr3-act/led/effect\"]",
        ))
        .unwrap_err();
        assert!(e.contains("非本模块可执行"), "越界主题点名: {e}");
    }

    /// 仓内默认灯效白名单全集（三值）——config 用例的统一断言基线
    fn led_effects_all() -> Vec<Rr3LedEffect> {
        vec![Rr3LedEffect::On, Rr3LedEffect::Off, Rr3LedEffect::Blink]
    }
}
