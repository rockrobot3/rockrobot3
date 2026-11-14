//! config schema 解析：`[rr3-act.*]` 四段下钻与逐键点名。

use crate::led::{Rr3LedEffect, RR3_LED_EFFECT_VALUES};
use crate::reflex::Rr3ReflexCfg;
use crate::relay::RR3_RELAY_SET_TOPIC;
use crate::servo::{Rr3ServoCfg, RR3_SERVO_COMMAND_TOPIC};

/// 本模块 config 的汇总形状（led/intent 带注入集合 → 去 Copy）
#[derive(Debug, Clone)]
pub(crate) struct Rr3ActCfg {
    pub(crate) servo: Rr3ServoCfg,
    pub(crate) led: Rr3LedCfg,
    pub(crate) intent: Rr3IntentCfg,
    pub(crate) reflex: Rr3ReflexCfg,
}

/// config `[rr3-act.led]` 的形状（`effects` = 本形态灯效白名单子集，A-9）
#[derive(Debug, Clone)]
pub(crate) struct Rr3LedCfg {
    pub(crate) blink_period_ms: u64,
    pub(crate) effects: Vec<Rr3LedEffect>,
}

/// config `[rr3-act.intent]` 的形状（`open_topics` = 意图执行面开放集，A-9）。
/// 组合件读入后透传给 `intent.rs`——组合件层无 ops 校验机制（⑧-c 只扫
/// `atoms/*/assemble.toml`），故本策略落 config 键而非装配清单 ops 槽。
#[derive(Debug, Clone)]
pub(crate) struct Rr3IntentCfg {
    pub(crate) open_topics: Vec<String>,
}

/// 反射环节拍上界（Hz）：`1_000_000 / loop_hz` 微秒取整，超过 1kHz 的配置
/// 在生产里没有物理意义，却会让循环周期归零（满转自旋）。
const RR3_MAX_LOOP_HZ: u32 = 1_000;

/// 饱和通告窗上界（毫秒，本轮静态审计）：`rr3_reflex_plan` 以 250ms 窗折算
/// 窗数 `div_ceil(… ) as u32`——无上界的巨值会静默截断成任意窗数（饱和通告
/// 语义失真）；超 10min 的连续饱和通告窗在生产无意义，超界即配置错点名拒。
const RR3_MAX_NOTICE_MS: u64 = 600_000;

/// TOML 文本 → Rr3ActCfg。手工下钻不引 serde derive：字段个位数，
/// 点名缺字段的文案比派生宏的默认报错可读。
pub(crate) fn rr3_act_config_parse(text: &str) -> Result<Rr3ActCfg, String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    rr3_act_cfg_build(&v)
}

/// 四个段址齐取后逐段解析（取址次序与解析次序分开，便于各自定位）。
fn rr3_act_cfg_build(v: &toml::Value) -> Result<Rr3ActCfg, String> {
    let servo_sect = rr3_act_cfg_sect(v, "servo")?;
    let led_sect = rr3_act_cfg_sect(v, "led")?;
    let intent_sect = rr3_act_cfg_sect(v, "intent")?;
    let reflex_sect = rr3_act_cfg_sect(v, "reflex")?;
    rr3_act_cfg_parts(servo_sect, led_sect, intent_sect, reflex_sect)
}

/// 段 → 汇总形状（字段按声明序求值）。
fn rr3_act_cfg_parts(
    servo_sect: &toml::Value,
    led_sect: &toml::Value,
    intent_sect: &toml::Value,
    reflex_sect: &toml::Value,
) -> Result<Rr3ActCfg, String> {
    Ok(Rr3ActCfg {
        servo: rr3_act_servo(servo_sect)?,
        led: rr3_act_led(led_sect)?,
        intent: Rr3IntentCfg {
            open_topics: rr3_act_cfg_topics(intent_sect)?,
        },
        reflex: rr3_act_reflex(reflex_sect)?,
    })
}

/// `[rr3-act.<名>]` 段取址：缺段点名段名（解析面唯一入口）。
fn rr3_act_cfg_sect<'a>(v: &'a toml::Value, name: &str) -> Result<&'a toml::Value, String> {
    v.get("rr3-act")
        .and_then(|s| s.get(name))
        .ok_or_else(|| format!("config 缺 [rr3-act.{name}] 段"))
}

/// 段内非负整数字段：缺键/类型错/负数一律点名 `[rr3-act.<段>] 缺 <键>`。
fn rr3_act_cfg_u64(sect: &toml::Value, key: &str, name: &str) -> Result<u64, String> {
    sect.get(key)
        .and_then(toml::Value::as_integer)
        .filter(|n| *n >= 0)
        .map(|n| n as u64)
        .ok_or_else(|| format!("config [rr3-act.{name}] 缺 {key}（非负整数）"))
}

/// servo 段：只管时限/限速。行程键不在 config（批N3 件五门 g 收敛）：Rr3ServoCfg
/// 的 angle_min/max_deg 由执行轴状态注入填值（RR3_ACT_AXIS_STATE 的 lo/hi，
/// 档案真值），config 出现任何行程键 = 读者零的死键，装配期（rrd assemble scan）
/// 与 CI 门 ⑨-e 双面执法，此处不再解析。
fn rr3_act_servo(sect: &toml::Value) -> Result<Rr3ServoCfg, String> {
    Ok(Rr3ServoCfg {
        // 行程占位（批N3 件五）：真值由装配段 rr3_act_run 用执行轴状态注入
        // 覆填（cfg.servo.angle_min/max_deg = axis.lo/hi_deg），此处不解析
        angle_min_deg: 0,
        angle_max_deg: 180,
        move_timeout_ms: rr3_act_cfg_u64(sect, "move_timeout_ms", "servo")?,
        state_min_interval_ms: rr3_act_cfg_u64(sect, "state_min_interval_ms", "servo")?,
    })
}

/// led 段：眨眼周期下限 1ms（0 为无意义自旋），灯效白名单子集见 `rr3_act_cfg_effects`。
fn rr3_act_led(sect: &toml::Value) -> Result<Rr3LedCfg, String> {
    Ok(Rr3LedCfg {
        blink_period_ms: rr3_act_cfg_u64(sect, "blink_period_ms", "led")?.max(1),
        effects: rr3_act_cfg_effects(sect)?,
    })
}

/// reflex 段：节拍走越界裁决（`rr3_act_cfg_loop`），饱和提示阈值同域裁决。
fn rr3_act_reflex(sect: &toml::Value) -> Result<Rr3ReflexCfg, String> {
    let notice = rr3_act_notice(sect)?;
    Ok(Rr3ReflexCfg {
        loop_hz: rr3_act_cfg_loop(sect)?,
        saturate_notice_ms: notice,
    })
}

/// 饱和通告窗阈：读数 + 上界裁决。
fn rr3_act_notice(sect: &toml::Value) -> Result<u64, String> {
    let notice = rr3_act_cfg_u64(sect, "saturate_notice_ms", "reflex")?;
    if notice > RR3_MAX_NOTICE_MS {
        return Err(format!(
            "config [rr3-act.reflex] saturate_notice_ms 须在 0..={RR3_MAX_NOTICE_MS}，收到 {notice}（窗数以 u32 折算，超界会静默截断）"
        ));
    }
    Ok(notice)
}

/// reflex 环节拍：越界拒 boot（0.0.5 批I A-3）——`as u32` 裸截断会把 2^32+1
/// 之类值偷偷收成 1，节拍面静默变形；上界补 1kHz（`1_000_000/loop_hz` 在极值
/// 时取整为 0µs，反射环会退化为满转自旋）。
fn rr3_act_cfg_loop(sect: &toml::Value) -> Result<u32, String> {
    let raw = rr3_act_loop_raw(sect)?;
    let hz = u32::try_from(raw)
        .map_err(|_| format!("config [rr3-act.reflex] loop_hz={raw} 超出 u32 范围，拒 boot"))?;
    rr3_loop_cap(hz)?;
    Ok(hz)
}

/// 环节拍上界（1..=上限）。
fn rr3_loop_cap(hz: u32) -> Result<(), String> {
    if hz > RR3_MAX_LOOP_HZ {
        return Err(format!(
            "config [rr3-act.reflex] loop_hz 须在 1..={RR3_MAX_LOOP_HZ}，收到 {hz}（更大值会退化为满转自旋）"
        ));
    }
    Ok(())
}

/// loop_hz 原始值（非负整数，下限 1）。
fn rr3_act_loop_raw(sect: &toml::Value) -> Result<u64, String> {
    Ok(sect
        .get("loop_hz")
        .and_then(toml::Value::as_integer)
        .filter(|n| *n >= 0)
        .map(|n| n as u64)
        .ok_or("config [rr3-act.reflex] 缺 loop_hz（非负整数）")?
        .max(1))
}

/// 灯效白名单子集（A-9）：数组逐元素必须是**在册契约值**，非在册值或非字符串
/// 逐条点名拒 boot（fail-closed）；键在而数组为空 = 本形态一条点名都不认（合法）。
fn rr3_act_cfg_effects(sect: &toml::Value) -> Result<Vec<Rr3LedEffect>, String> {
    let arr = sect
        .get("effects")
        .and_then(toml::Value::as_array)
        .ok_or("config [rr3-act.led] 缺 effects（灯效白名单数组）")?;
    rr3_effects_of(arr)
}

/// 灯效数组逐项落位（顺序与下标文案随项）。
fn rr3_effects_of(arr: &[toml::Value]) -> Result<Vec<Rr3LedEffect>, String> {
    arr.iter()
        .enumerate()
        .map(|(i, e)| rr3_act_effect_at(i, e))
        .collect()
}

/// 单项灯效：必为在册契约值的字符串（越界值点名在册值）。
fn rr3_act_effect_at(i: usize, e: &toml::Value) -> Result<Rr3LedEffect, String> {
    let s = rr3_act_effect_str(i, e)?;
    Rr3LedEffect::rr3_led_effect_parse(s).ok_or_else(|| {
        format!(
            "config [rr3-act.led] effects[{i}] 非在册灯效 {s:?}（在册值 {}）",
            RR3_LED_EFFECT_VALUES.join("/")
        )
    })
}

/// 单项灯效必须是字符串（非字符串逐条点名）。
fn rr3_act_effect_str(i: usize, e: &toml::Value) -> Result<&str, String> {
    e.as_str()
        .ok_or_else(|| format!("config [rr3-act.led] effects[{i}] 必须是字符串，收到 {e}"))
}

/// 意图执行面开放集（A-9）：只认本模块**有执行臂**的在册意图主题
/// （relay/set、servo/command），越界值逐条点名拒 boot（fail-closed）；空集合法。
fn rr3_act_cfg_topics(sect: &toml::Value) -> Result<Vec<String>, String> {
    let arr = sect
        .get("open_topics")
        .and_then(toml::Value::as_array)
        .ok_or("config [rr3-act.intent] 缺 open_topics（意图开放主题数组）")?;
    rr3_topics_of(arr)
}

/// 主题数组逐项落位（顺序与下标文案随项）。
fn rr3_topics_of(arr: &[toml::Value]) -> Result<Vec<String>, String> {
    arr.iter()
        .enumerate()
        .map(|(i, t)| rr3_act_topic_at(i, t))
        .collect()
}

/// 单项主题：字符串且在本模块执行臂内（只认 relay/set 与 servo/command）。
fn rr3_act_topic_at(i: usize, t: &toml::Value) -> Result<String, String> {
    let s = rr3_act_topic_str(i, t)?;
    if !rr3_topic_known(s) {
        return Err(format!(
            "config [rr3-act.intent] open_topics[{i}] 非本模块可执行的意图主题 {s:?}（只认 {}/{}）",
            RR3_RELAY_SET_TOPIC, RR3_SERVO_COMMAND_TOPIC
        ));
    }
    Ok(s.to_string())
}

/// 本模块有执行臂的意图主题（relay/set 与 servo/command）。
fn rr3_topic_known(s: &str) -> bool {
    s == RR3_SERVO_COMMAND_TOPIC || s == RR3_RELAY_SET_TOPIC
}

/// 单项必须是字符串（非字符串逐条点名）。
fn rr3_act_topic_str(i: usize, t: &toml::Value) -> Result<&str, String> {
    t.as_str()
        .ok_or_else(|| format!("config [rr3-act.intent] open_topics[{i}] 必须是字符串，收到 {t}"))
}
