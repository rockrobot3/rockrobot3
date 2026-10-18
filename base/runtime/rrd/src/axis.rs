//! 执行件档案装载器（批N3 件二，全案 §4.4——`hardware/<形态>/axis/<instance>.toml`）。
//!
//! 档案是行程的真值点（§1 坑 8 的收敛终点：config 侧字面量与板侧分轴字面量都要
//! 改读这里，收敛接线在件五/件六）。本模块只做**装载机制**——白名单、全键必填
//! （F-6）、类型派发、外键对账（F-4）、错误全收集；键级域界与自洽判据是策略，
//! 逐键显式写在 [`rr3_act_profile_from_text`] 一处。角域 0..=180 这类**器件事实
//! 不硬编码进装载器**（F-5：文档示例值不进代码）。
//!
//! 键真值源 = `rr-hal-api` 的 [`rr_hal_api::RR3_ACT_PROFILE_FIELDS`] 十四键词表
//! （键名 / 值类型 / 读者三态一张表）：白名单、缺键点名、序列化键序都从本表走
//! ——键名第二份真值点不存在。加键 = 词表 append + 装载侧同批（提取面缺新键
//! 由编译期结构体字段与缺键扫描双面拦），契约级评审。
//!
//! 载体语义（§4.4 载体与写入语义①，Y-2/Y-7）：
//! - host 档档案是文件，写入必须复用仓内那一份原子序——[`crate::counters::
//!   rr3_atomic_store`]（tmp 写全量 → fsync → rename），本模块不出现第二条写路径；
//! - 读侧坏件口径照抄 counters：`NotFound` → [`rr3_act_profile_load`] 回
//!   `Ok(None)`（"无档案"是调用方的裁决输入，机制不代言缺省），形状错 →
//!   `Err` **且不许覆盖**——`secrets.rs` 那个"坏 JSON 恒 Err 且不能自愈"的
//!   形状就是这条要防的。
//!
//! 消费接线（批N3 件五现状）：执行轴状态注入面已接进 boot 链（rrd profile.rs 扫
//! `axis/`，按 `RR3_HAL_PERIPH_AXIS`（缺省契约常量）消歧——instance 与轴读数相等的
//! 恰一份合法档案才投影 `RR3_ACT_AXIS_STATE`，文本含行程三键 lo/hi/center，空 =
//! 模块侧折保守缺省）；行程收敛的注入半边在本文件（档案即行程真值源），config 与
//! 板侧字面量的撤除随件五同批推进；标定写入面随件四（axis_write）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rr_hal_api::{Rr3ActHardStop, Rr3ActHold, Rr3ActProfile, Rr3ActSource, RR3_ACT_PROFILE_FIELDS};

use crate::counters::rr3_atomic_store;

/// 一个轴的档案装载产物：装载域键 `pin`（F-4 外键，件五 boot 链拿它对
/// `[[pins]]` 的 pin_id）＋档案值面（`schema` 对账后不留存——只认 1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AxisFile {
    pub pin: String,
    pub profile: Rr3ActProfile,
}

/// int 键取值＋域判一体（词表 kind = Int 的键走这里）：缺键静默置 `None`
/// （病灶已由 F-6 扫描点名，不重复报）；非整数形状与越域当场点名。键级域界
/// 是策略、由调用点显式给定——机制只有"在场才判、越域即红"这一条。
fn rr3_int_field(
    v: &toml::Value,
    errs: &mut Vec<String>,
    path: &str,
    key: &str,
    lo: i64,
    hi: i64,
) -> Option<i64> {
    let raw = v.get(key)?;
    let Some(n) = raw.as_integer() else {
        errs.push(format!("{path}: {key} 应为整数"));
        return None;
    };
    if !(lo..=hi).contains(&n) {
        errs.push(format!("{path}: {key} = {n} 越域（合法域 {lo}..={hi}）"));
        return None;
    }
    Some(n)
}

/// text 键取值（词表 kind = Text 的键走这里）：非字符串形状当场点名置 `None`。
/// 词表内合法性（枚举词）由词键各自的 parse 面管，本函数不认词。
fn rr3_text_field<'a>(
    v: &'a toml::Value,
    errs: &mut Vec<String>,
    path: &str,
    key: &str,
) -> Option<&'a str> {
    let raw = v.get(key)?;
    match raw.as_str() {
        Some(s) => Some(s),
        None => {
            errs.push(format!("{path}: {key} 应为字符串"));
            None
        }
    }
}

/// bool 键取值（词表 kind = Bool 的键走这里）：非布尔形状当场点名置 `None`。
fn rr3_bool_field(v: &toml::Value, errs: &mut Vec<String>, path: &str, key: &str) -> Option<bool> {
    let raw = v.get(key)?;
    match raw.as_bool() {
        Some(b) => Some(b),
        None => {
            errs.push(format!("{path}: {key} 应为布尔"));
            None
        }
    }
}

/// 从档案文本装载一个轴（**机制面**：白名单 + F-6 全键必填 + 类型派发 + 外键
/// 对账 + 错误全收集）。`path` 只进错误点名文案。红 = 返回全量错误清单
/// （一次装载点名所有病灶，不挤牙膏）。
pub fn rr3_act_profile_from_text(
    text: &str,
    path: &str,
    pin_ids: &BTreeSet<String>,
) -> Result<Rr3AxisFile, Vec<String>> {
    let mut errs: Vec<String> = Vec::new();

    let v: toml::Value = match text.parse() {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("{path}: 不是合法 TOML: {e}")]),
    };
    let Some(table) = v.as_table() else {
        return Err(vec![format!(
            "{path}: 应为键值平表（十四键平铺，无嵌套小节）"
        )]);
    };

    // 白名单 + F-6 全键必填：一次遍历两查——词表外键点名（加键走契约级评审），
    // 词表内键登记已见；随后按词表反向扫缺键（"合法不适用"必须显式写，留空即红）
    let mut seen = BTreeSet::new();
    for k in table.keys() {
        if RR3_ACT_PROFILE_FIELDS.iter().any(|f| f.key == k) {
            seen.insert(k.as_str());
        } else {
            errs.push(format!(
                "{path}: 未知键 {k}（十四键词表见 rr-hal-api RR3_ACT_PROFILE_FIELDS，加键走契约级评审）"
            ));
        }
    }
    for f in RR3_ACT_PROFILE_FIELDS {
        if !seen.contains(f.key) {
            errs.push(format!(
                "{path}: 缺键 {}（F-6：全键必填，\"合法不适用\"必须显式写，留空即红）",
                f.key
            ));
        }
    }

    // 键级域判据（策略逐键显式）：schema 只认 1（升版走契约级评审，对账即弃）；
    // 行程键非负且进 u16；dir_sign ∈ {±1}（0 = 物理矛盾，单独点名）；slew /
    // 电流上限须正；ms 键进 u32。域界红都不进产物。
    let _ = rr3_int_field(&v, &mut errs, path, "schema", 1, 1);
    let center = rr3_int_field(&v, &mut errs, path, "center_deg", 0, i64::from(u16::MAX));
    let lo = rr3_int_field(&v, &mut errs, path, "lo_deg", 0, i64::from(u16::MAX));
    let hi = rr3_int_field(&v, &mut errs, path, "hi_deg", 0, i64::from(u16::MAX));
    let dir = match rr3_int_field(&v, &mut errs, path, "dir_sign", -1, 1) {
        Some(0) => {
            errs.push(format!(
                "{path}: dir_sign = 0（方向符号只有 ±1，0 是物理矛盾）"
            ));
            None
        }
        other => other,
    };
    let slew = rr3_int_field(&v, &mut errs, path, "slew_deg_s", 1, i64::from(u16::MAX));
    let settle = rr3_int_field(&v, &mut errs, path, "settle_ms", 0, i64::from(u32::MAX));
    let warmup = rr3_int_field(&v, &mut errs, path, "warmup_ms", 0, i64::from(u32::MAX));
    let ceil_ma = rr3_int_field(
        &v,
        &mut errs,
        path,
        "current_ceil_ma",
        1,
        i64::from(u16::MAX),
    );

    // 词键走在册词表解析（词形真值在 rr-hal-api 各 enum 的 impl，此处不认第二份）
    let hold = match rr3_text_field(&v, &mut errs, path, "hold") {
        Some(s) => match Rr3ActHold::rr3_act_hold_parse(s) {
            Some(h) => Some(h),
            None => {
                errs.push(format!(
                    "{path}: hold = \"{s}\" 不在册（只认 hold | release）"
                ));
                None
            }
        },
        None => None,
    };
    let hard_stop = match rr3_text_field(&v, &mut errs, path, "hard_stop") {
        Some(s) => match Rr3ActHardStop::rr3_act_hard_stop_parse(s) {
            Some(h) => Some(h),
            None => {
                errs.push(format!(
                    "{path}: hard_stop = \"{s}\" 不在册（只认 unknown | yes | no）"
                ));
                None
            }
        },
        None => None,
    };
    let source = match rr3_text_field(&v, &mut errs, path, "source") {
        Some(s) => match Rr3ActSource::rr3_act_source_parse(s) {
            Some(src) => Some(src),
            None => {
                errs.push(format!(
                    "{path}: source = \"{s}\" 不在册（只认 manual | fixture | openloop-recover | stall-search）"
                ));
                None
            }
        },
        None => None,
    };
    let calibrated = rr3_bool_field(&v, &mut errs, path, "calibrated");

    // pin 外键对账（F-4）：档案与绑定之间必须有外键，否则档案改名而 [pins]
    // 不动，一条校验都不红——档案侧的对账半边在此，绑定侧归 profile 装载域
    let pin = match rr3_text_field(&v, &mut errs, path, "pin") {
        Some(s) if pin_ids.contains(s) => Some(s.to_string()),
        Some(s) => {
            errs.push(format!(
                "{path}: pin = \"{s}\" 未命中 [pins] 的 pin_id 集（F-4：档案外键与绑定必须对账）"
            ));
            None
        }
        None => None,
    };

    // 自洽判据（§4.4）：行程窗口必须包住中位锚——lo < center < hi（三键都在场
    // 才判，缺键病灶已各自点名，这里不重复报）
    if let (Some(lo), Some(center), Some(hi)) = (lo, center, hi) {
        if !(lo < center && center < hi) {
            errs.push(format!(
                "{path}: 行程窗口自洽破：lo_deg = {lo}, center_deg = {center}, hi_deg = {hi}（须 lo < center < hi）"
            ));
        }
    }

    if !errs.is_empty() {
        return Err(errs);
    }

    // 判据全绿后十四键必在场：上面每个 None 分支都已同步点名（缺键 / 形状错 /
    // 越域 / 词表外 / 外键未命中）——errs 空 ⟺ 全 Some，重组只是换承载形状
    let profile = (|| {
        Some(Rr3ActProfile {
            center_deg: center? as u16,
            lo_deg: lo? as u16,
            hi_deg: hi? as u16,
            dir_sign: dir? as i8,
            slew_deg_s: slew? as u16,
            settle_ms: settle? as u32,
            hold: hold?,
            current_ceil_ma: ceil_ma? as u16,
            warmup_ms: warmup? as u32,
            hard_stop: hard_stop?,
            calibrated: calibrated?,
            source: source?,
        })
    })()
    .expect("逐键判据全绿后十二键必在场（errs 已判空）");
    let pin = pin.expect("逐键判据全绿后 pin 必在场（errs 已判空）");

    Ok(Rr3AxisFile { pin, profile })
}

/// 序列化：键序 = 十四键词表序（与 [`RR3_ACT_PROFILE_FIELDS`] 同序发射，测试
/// 钉死）。平表、无注释——档案是机器读写的数据（面板 / 标定写入面），词表
/// 注释归 rr-hal-api，这里不复制第二份。
pub fn rr3_act_profile_to_toml(file: &Rr3AxisFile) -> String {
    let p = &file.profile;
    let lines = [
        "schema = 1".to_string(),
        format!("pin = \"{}\"", file.pin),
        format!("center_deg = {}", p.center_deg),
        format!("lo_deg = {}", p.lo_deg),
        format!("hi_deg = {}", p.hi_deg),
        format!("dir_sign = {}", p.dir_sign),
        format!("slew_deg_s = {}", p.slew_deg_s),
        format!("settle_ms = {}", p.settle_ms),
        format!("hold = \"{}\"", p.hold.rr3_act_hold_name()),
        format!("current_ceil_ma = {}", p.current_ceil_ma),
        format!("warmup_ms = {}", p.warmup_ms),
        format!("hard_stop = \"{}\"", p.hard_stop.rr3_act_hard_stop_name()),
        format!("calibrated = {}", p.calibrated),
        format!("source = \"{}\"", p.source.rr3_act_source_name()),
    ];
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// 落盘：走 [`rr3_atomic_store`]（tmp 写全量 → fsync → rename）——§4.4 载体
/// 语义①"必须复用仓内那一份原子序，不许出现第二条写路径"。tmp 同名加
/// `.tmp` 后缀（counters 的 `counters.json.tmp` 同形）。
pub fn rr3_act_profile_store(path: &Path, file: &Rr3AxisFile) -> Result<(), String> {
    let body = rr3_act_profile_to_toml(file);
    let mut os = path.as_os_str().to_os_string();
    os.push(".tmp");
    rr3_atomic_store(path, &PathBuf::from(os), body.as_bytes())
}

/// 启动/按需装载：`NotFound` → `Ok(None)`（"无档案"是调用方的裁决输入——机制
/// 不代言缺省）；其余读错 / 形状错 → `Err`（**不许覆盖**——坏档案上重写等于
/// 把病灶誊进下一轮，§4.4 载体语义①）。
pub fn rr3_act_profile_load(
    path: &Path,
    pin_ids: &BTreeSet<String>,
) -> Result<Option<Rr3AxisFile>, Vec<String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(vec![format!("读 {} 失败: {e}", path.display())]),
    };
    rr3_act_profile_from_text(&text, &path.display().to_string(), pin_ids).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rrd-axis-t-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// §4.4 示例档（pan 轴）：全部判据走绿的正例基底
    fn sample_text() -> String {
        [
            "schema = 1",
            "pin = \"act.axis.pwm.pan\"",
            "center_deg = 90",
            "lo_deg = 60",
            "hi_deg = 120",
            "dir_sign = 1",
            "slew_deg_s = 100",
            "settle_ms = 200",
            "hold = \"release\"",
            "current_ceil_ma = 750",
            "warmup_ms = 0",
            "hard_stop = \"unknown\"",
            "calibrated = false",
            "source = \"manual\"",
        ]
        .join("\n")
            + "\n"
    }

    fn sample_pin_ids() -> BTreeSet<String> {
        ["act.axis.pwm.pan", "act.axis.pwm.tilt"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// 解析 → 逐键值面断言；序列化键序 = 词表序；再装载 round-trip 逐键相等
    #[test]
    fn parse_and_serialize_round_trip_in_vocabulary_order() {
        let f = rr3_act_profile_from_text(&sample_text(), "axis.toml", &sample_pin_ids())
            .expect("示例档应全绿");
        assert_eq!(f.pin, "act.axis.pwm.pan");
        let p = &f.profile;
        assert_eq!((p.center_deg, p.lo_deg, p.hi_deg), (90, 60, 120));
        assert_eq!(p.dir_sign, 1);
        assert_eq!(p.slew_deg_s, 100);
        assert_eq!(p.settle_ms, 200);
        assert_eq!(p.hold, Rr3ActHold::Release);
        assert_eq!(p.current_ceil_ma, 750);
        assert_eq!(p.warmup_ms, 0);
        assert_eq!(p.hard_stop, Rr3ActHardStop::Unknown);
        assert!(!p.calibrated);
        assert_eq!(p.source, Rr3ActSource::Manual);

        let text = rr3_act_profile_to_toml(&f);
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|l| l.split('=').next())
            .map(|k| k.trim())
            .collect();
        let vocab: Vec<&str> = RR3_ACT_PROFILE_FIELDS.iter().map(|f| f.key).collect();
        assert_eq!(keys, vocab, "发射键序必须与十四键词表同序");
        let f2 =
            rr3_act_profile_from_text(&text, "axis.toml", &sample_pin_ids()).expect("回读应全绿");
        assert_eq!(f2, f, "round-trip 逐键一致");
    }

    /// 未知键 + 缺键同批全数点名（错误全收集，不挤牙膏；F-6 留空即红）
    #[test]
    fn unknown_and_missing_keys_are_all_collected() {
        let mut lines: Vec<String> = sample_text().lines().map(String::from).collect();
        lines.push("extra_key = 1".to_string());
        let idx = lines
            .iter()
            .position(|l| l.starts_with("calibrated"))
            .unwrap();
        lines.remove(idx);
        let errs = rr3_act_profile_from_text(&lines.join("\n"), "axis.toml", &sample_pin_ids())
            .expect_err("未知键 + 缺键应全数点名");
        assert_eq!(errs.len(), 2, "错误全收集：{errs:?}");
        assert!(errs.iter().any(|e| e.contains("未知键 extra_key")));
        assert!(errs.iter().any(|e| e.contains("缺键 calibrated")));
    }

    /// pin 外键未命中 [pins] 的 pin_id 集（F-4：档案与绑定之间必须有外键）
    #[test]
    fn pin_foreign_key_miss_is_red() {
        let text = sample_text().replace("act.axis.pwm.pan", "act.axis.pwm.unknown");
        let errs = rr3_act_profile_from_text(&text, "axis.toml", &sample_pin_ids())
            .expect_err("外键未命中应红");
        assert_eq!(errs.len(), 1);
        assert!(
            errs[0].contains("pin = \"act.axis.pwm.unknown\"") && errs[0].contains("F-4"),
            "{errs:?}"
        );
    }

    /// schema 版本对账：只认 1，升版走契约级评审
    #[test]
    fn schema_mismatch_is_red() {
        let text = sample_text().replace("schema = 1", "schema = 2");
        let errs = rr3_act_profile_from_text(&text, "axis.toml", &sample_pin_ids())
            .expect_err("schema 版本不对应红");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("schema"), "{errs:?}");
    }

    /// 行程窗口自洽判据：须严格 lo < center < hi（lo == center 也破）
    #[test]
    fn window_self_consistency_is_red() {
        let text = sample_text().replace("lo_deg = 60", "lo_deg = 90");
        let errs = rr3_act_profile_from_text(&text, "axis.toml", &sample_pin_ids())
            .expect_err("行程窗口自洽破应红");
        assert!(errs[0].contains("行程窗口自洽破"), "{errs:?}");
    }

    /// dir_sign = 0 是物理矛盾（单独点名）；dir_sign = 2 越域（±1 之外）
    #[test]
    fn dir_sign_zero_and_out_of_domain_are_red() {
        let text = sample_text().replace("dir_sign = 1", "dir_sign = 0");
        let errs = rr3_act_profile_from_text(&text, "axis.toml", &sample_pin_ids())
            .expect_err("dir_sign = 0 应红");
        assert!(errs[0].contains("dir_sign = 0"), "{errs:?}");
        let text = sample_text().replace("dir_sign = 1", "dir_sign = 2");
        let errs = rr3_act_profile_from_text(&text, "axis.toml", &sample_pin_ids())
            .expect_err("dir_sign = 2 应红");
        assert!(errs[0].contains("越域"), "{errs:?}");
    }

    /// 词键词表外即红（hold 词形真值在 rr-hal-api，装载侧不认第二份）
    #[test]
    fn hold_word_out_of_vocabulary_is_red() {
        let text = sample_text().replace("hold = \"release\"", "hold = \"auto\"");
        let errs = rr3_act_profile_from_text(&text, "axis.toml", &sample_pin_ids())
            .expect_err("词表外应红");
        assert!(errs[0].contains("hold = \"auto\" 不在册"), "{errs:?}");
    }

    /// 落盘 → 读回 round-trip；原子写序走完不得留 tmp 残留
    #[test]
    fn store_then_load_round_trips_without_tmp_leftover() {
        let dir = tmpdir("round");
        let path = dir.join("pan.toml");
        let f = rr3_act_profile_from_text(&sample_text(), "axis.toml", &sample_pin_ids()).unwrap();
        rr3_act_profile_store(&path, &f).expect("落盘应成功");
        assert!(
            !dir.join("pan.toml.tmp").exists(),
            "原子写序走完不得留 tmp 残留"
        );
        let loaded = rr3_act_profile_load(&path, &sample_pin_ids())
            .expect("读回应全绿")
            .expect("刚写完的档案必在场");
        assert_eq!(loaded, f);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 无档案 = None（机制不代言缺省）；坏件 = Err 且字节不动（不许覆盖）
    #[test]
    fn load_missing_is_none_and_broken_file_is_err_without_overwrite() {
        let dir = tmpdir("broken");
        let path = dir.join("pan.toml");
        assert_eq!(
            rr3_act_profile_load(&path, &sample_pin_ids()).unwrap(),
            None,
            "无档案 = None"
        );
        std::fs::write(&path, "schema = 1\npin = \"x\"\n").unwrap();
        let before = std::fs::read(&path).unwrap();
        let errs = rr3_act_profile_load(&path, &sample_pin_ids()).expect_err("坏件应红");
        assert!(errs.iter().any(|e| e.contains("缺键")), "{errs:?}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "装载失败不得动文件");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
