//! 装机接线档 `hardware/<形态>/wiring.toml`（增补轮六 §4.3）：机器可查、人照它插线。
//!
//! 接线档把接线变成**可检查的数据**：面板出图、出厂自检逐条点名、`rated_a` 与
//! 件 `demands.peak_ma` 互校的载体。它拦不住人插错线（坑 10 的直接原因是物理的）——
//! 诚实边界，但每条线组从此有判据可点。九字段 = `connector`/`hdr_pin`/`signal`/
//! `net`/`feeds`/`mate`/`keyed`/`rated_a`/`gnd_common`，全键必填（§5 缺失语义列 +
//! "合法不适用显式写"的全仓范式）：`feeds` 无下游线组显式写 `[]`；`mate`/`net`/
//! `gnd_common` 的合法不适值是 `"none"`；`keyed`/`rated_a` 无不适值可言，缺即红。
//!
//! 校验⑦（§5，后果 = 拒）的装载侧判据：
//! - ⑦-a `wire.signal ⊆ pin_id ∪ {vcc, gnd}`（F-3：signal 除供电/地外一律写
//!   `[pins]` 的 pin_id，不各造语法）
//! - ⑦-b `wire.net ⊆ domains`（`"none"` = 纯信号/地线组、不承载电力网络导体的
//!   合法显式值，非 none 逐一查域在册——F-5）
//! - ⑦-d `wire.mate ∈ devices.toml 的 mate 集`（§4.1b 门 o 的对账面）
//! - 闭合面：`feeds` 指向同文件其他线组的 `connector`（位号在本档内闭合、不得
//!   自环）——位号一连接器一组线，重复声明即红
//! - **⑦-c 装载期不预支**（额定加总两位，§22 实-4 拆分：⑦-c1 器件侧——判据对象
//!   是 `devices.toml` 的 `peak_ma`——其自检时点判已随 assembly.rs 额定加总族先行，
//!   批N5 件五；⑦-c2 件侧的判据对象是件 demands 申报，批N2 落数据面后同链判。
//!   feeds 一般环的装载期拒绝仍随其批，自检时点已由额定加总族兜住）
//!
//! `hdr_pin` 与 `signal` 按位序对应（`hdr_pin[k] ↔ signal[k]`，丝印序即信号序）；
//! 同一逻辑信号占多个导体（如 I2C 的 SDA/SCL 同属一个 pin_id）按导体数重复写。
//!
//! 接线路径推导（[`rr3_wiring_path`])与 modules-root 同一条布局纪律：
//! `<root>/profiles/<机>.toml` 的上两级即 modules-root，接线档在
//! `<root>/hardware/<机>/wiring.toml`——daemon 由 `--profile` 推导，单点真值。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// 一条线组（§4.3 九字段，不可变快照）
#[derive(Debug, Clone, PartialEq)]
pub struct Rr3Wire {
    /// 板上连接器位号（一连接器一组线，本档内唯一）
    pub connector: String,
    /// 排针物理序号（与 hdr 丝印一致），与 signal 位序对应
    pub hdr_pin: Vec<i64>,
    /// 每个导体的信号名：`vcc`/`gnd` 或 [pins] 的 pin_id
    pub signal: Vec<String>,
    /// 电力网络 → domains 外键（`"none"` = 纯信号/地线组的合法显式值）
    pub net: String,
    /// 本线承载的下游线组位号（F-10 的修法：额定电流按加总判，不按单角色判；
    /// 无下游显式写 []）
    pub feeds: Vec<String>,
    /// 对端连接器型号 → devices.toml 的 mate 外键（`"none"` = 排针/焊线直连）
    pub mate: String,
    /// 有无物理防呆（缺失即红，§5 缺失语义列）
    pub keyed: bool,
    /// 线材额定电流 A（校验⑦-c 的判据面，批N2 消费）
    pub rated_a: f64,
    /// 共地点标识（"单点共地铁律"的数据位；无地导体显式写 "none"）
    pub gnd_common: String,
}

/// 一份接线档（不可变快照）。声明序 = TOML 序，不携带顺序语义。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rr3Wiring {
    pub wires: Vec<Rr3Wire>,
}

/// 由装配单路径推导接线档路径（daemon 的接线；与 modules-root 同一布局纪律，
/// 见模块头注释）。装配单必须已存在（canonicalize 同 [`crate::profile::
/// rr3_profile_abs`] 口径——装机现场读不到装配单，接线路径无从谈起）。
pub fn rr3_wiring_path(profile: &std::path::Path) -> Result<PathBuf, String> {
    let abs = crate::profile::rr3_profile_abs(profile)?;
    let stem = abs
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| format!("装配单 {} 无文件名", abs.display()))?;
    let root = abs
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| {
            format!(
                "装配单 {} 无法定位 hardware-root（须在 <root>/profiles/ 下）",
                abs.display()
            )
        })?;
    Ok(root.join("hardware").join(stem).join("wiring.toml"))
}

/// [[wire]] 九字段词表（§4.3 定形）：全键必填，未知键点名
const RR3_WIRE_KEYS: [&str; 9] = [
    "connector",
    "hdr_pin",
    "signal",
    "net",
    "feeds",
    "mate",
    "keyed",
    "rated_a",
    "gnd_common",
];

/// 解析接线档文本。形状错误全部收集完才返回（运维一次改对），逐条点名、
/// 不静默放行。空表（无 [[wire]]）是装配事故——接线闭合是校验⑦的前提，
/// 没有"无线的机器"。
pub fn rr3_wiring_from_text(text: &str) -> Result<Rr3Wiring, Vec<String>> {
    let root: toml::Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("接线档 TOML 解析失败: {e}")]),
    };
    let mut errs = Vec::new();
    let Some(entries) = root.get("wire").and_then(toml::Value::as_array) else {
        return Err(vec![
            "缺少 [[wire]]：接线档必须至少声明一条线组（接线闭合是校验⑦的前提）".to_string(),
        ]);
    };
    if entries.is_empty() {
        return Err(vec![
            "接线档为空（无 [[wire]] 条目）：必须至少声明一条线组".to_string()
        ]);
    }
    let mut wires: Vec<Rr3Wire> = Vec::new();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, item) in entries.iter().enumerate() {
        let Some(t) = item.as_table() else {
            errs.push(format!("wire #{i} 必须是 TOML 表（[[wire]]）"));
            continue;
        };
        rr3_wire_unknown(i, t, &mut errs);
        let connector = rr3_wire_ident(i, t, &mut seen, &mut errs);
        wires.push(rr3_wire_fields(i, t, connector, &mut errs));
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    // 闭合面：feeds 指向同文件其他线组的位号（位号集在本档内闭合），自环即红。
    // 一般环（A→B→A）的检测随批N2 ⑦-c 的闭包计算同链落，本批不预支。
    rr3_wire_close(&wires, &mut errs);
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Rr3Wiring { wires })
}

/// 未知键门：九字段封闭（§4.3），表内多一个键即点名
fn rr3_wire_unknown(i: usize, t: &toml::value::Table, errs: &mut Vec<String>) {
    for k in t.keys() {
        if !RR3_WIRE_KEYS.contains(&k.as_str()) {
            errs.push(format!("wire #{i} 未知键 {k}（九字段封闭，§4.3）"));
        }
    }
}

/// connector 身份门：非空 + 位号唯一（一连接器一组线，feeds 引用以位号为键）
fn rr3_wire_ident<'a>(
    i: usize,
    t: &'a toml::value::Table,
    seen: &mut BTreeMap<&'a str, usize>,
    errs: &mut Vec<String>,
) -> &'a str {
    let connector = t
        .get("connector")
        .and_then(toml::Value::as_str)
        .unwrap_or("");
    if connector.is_empty() {
        errs.push(format!("wire #{i}.connector 缺失或为空"));
    } else if let Some(prev) = seen.get(connector) {
        errs.push(format!(
            "wire #{i} 位号 {connector} 与 wire #{prev} 重复（一连接器一组线，feeds 引用以位号为键）"
        ));
    } else {
        seen.insert(connector, i);
    }
    connector
}

/// 位序对应破：两边都非空且长度不等（hdr_pin[k] ↔ signal[k]）。
fn rr3_wire_arity_bad(hdr_pin: &[i64], signal: &[String]) -> bool {
    !hdr_pin.is_empty() && !signal.is_empty() && hdr_pin.len() != signal.len()
}

/// 导体面：hdr_pin 非空整数数组 + signal 非空字符串数组 + 位序对应（等长）
fn rr3_wire_conductors(
    i: usize,
    t: &toml::value::Table,
    errs: &mut Vec<String>,
) -> (Vec<i64>, Vec<String>) {
    let hdr_pin = rr3_wire_pins(i, t, errs);
    let signal = rr3_wire_signals(i, t, errs);
    // 位序对应：hdr_pin[k] ↔ signal[k]（丝印序即信号序）
    if rr3_wire_arity_bad(&hdr_pin, &signal) {
        errs.push(format!(
            "wire #{i} 的 hdr_pin（{} 个）与 signal（{} 个）数目不等（位序对应：hdr_pin[k] ↔ signal[k]）",
            hdr_pin.len(),
            signal.len()
        ));
    }
    (hdr_pin, signal)
}

/// `hdr_pin`：非空整数数组（非整数元素静默跳过，与拆分前一致）。
fn rr3_wire_pins(i: usize, t: &toml::value::Table, errs: &mut Vec<String>) -> Vec<i64> {
    let hdr_pin: Vec<i64> = t
        .get("hdr_pin")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_integer).collect())
        .unwrap_or_default();
    if hdr_pin.is_empty() {
        errs.push(format!("wire #{i}.hdr_pin 必须是非空整数数组"));
    }
    hdr_pin
}

/// `signal`：非空字符串数组（非串元素静默跳过，与拆分前一致）。
fn rr3_wire_signals(i: usize, t: &toml::value::Table, errs: &mut Vec<String>) -> Vec<String> {
    let signal: Vec<String> = t
        .get("signal")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if signal.is_empty() {
        errs.push(format!("wire #{i}.signal 必须是非空字符串数组"));
    }
    signal
}

/// 单条线组的八字段解析（connector 已由身份门给出）；形状错逐条点名、不静默
fn rr3_wire_fields(
    i: usize,
    t: &toml::value::Table,
    connector: &str,
    errs: &mut Vec<String>,
) -> Rr3Wire {
    let (hdr_pin, signal) = rr3_wire_conductors(i, t, errs);
    // 判据按字段序执行，错误顺序与文案逐字保持（net → feeds → mate → keyed →
    // rated_a → gnd_common）。
    let net = rr3_wire_req_str(
        i,
        t,
        "net",
        "\"none\" 是纯信号/地线组的合法显式值，留空即红",
        errs,
    );
    let feeds = rr3_wire_feeds(i, t, errs);
    let mate = rr3_wire_req_str(
        i,
        t,
        "mate",
        "\"none\" 是排针/焊线直连的合法显式值，留空即红",
        errs,
    );
    let keyed = rr3_wire_keyed(i, t, errs);
    let rated_a = rr3_wire_rated(i, t, errs);
    let gnd_common = rr3_wire_req_str(
        i,
        t,
        "gnd_common",
        "共地点标识，\"none\" 是无地导体线组的合法显式值",
        errs,
    );
    Rr3Wire {
        connector: connector.to_string(),
        hdr_pin,
        signal,
        net,
        feeds,
        mate,
        keyed,
        rated_a,
        gnd_common,
    }
}

/// 必填字符串字段（net/mate/gnd_common）：缺失、非串、空串同一文案点名。
fn rr3_wire_req_str(
    i: usize,
    t: &toml::value::Table,
    key: &str,
    hint: &str,
    errs: &mut Vec<String>,
) -> String {
    let v = t.get(key).and_then(toml::Value::as_str).unwrap_or("");
    if v.is_empty() {
        errs.push(format!("wire #{i}.{key} 缺失或为空（{hint}）"));
    }
    v.to_string()
}

/// feeds：无下游线组显式写 []（缺失/类型错 → 红）。
fn rr3_wire_feeds(i: usize, t: &toml::value::Table, errs: &mut Vec<String>) -> Vec<String> {
    let feeds: Vec<String> = t
        .get("feeds")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if t.get("feeds").and_then(toml::Value::as_array).is_none() {
        errs.push(format!(
            "wire #{i}.feeds 缺失或不是字符串数组（无下游线组显式写 []，留空即红——§5 缺失语义列）"
        ));
    }
    feeds
}

/// keyed：有无物理防呆必须显式声明（缺 → 红）。
fn rr3_wire_keyed(i: usize, t: &toml::value::Table, errs: &mut Vec<String>) -> bool {
    match t.get("keyed").and_then(toml::Value::as_bool) {
        Some(b) => b,
        None => {
            errs.push(format!(
                "wire #{i}.keyed 缺失或不是布尔（有无物理防呆必须显式声明——§5 缺失语义列）"
            ));
            false
        }
    }
}

/// rated_a：正数（0 额定线不存在；⑦-c 的判据面）。
fn rr3_wire_rated(i: usize, t: &toml::value::Table, errs: &mut Vec<String>) -> f64 {
    let rated = t
        .get("rated_a")
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)));
    match rated {
        Some(a) if a > 0.0 => a,
        _ => {
            errs.push(format!(
                "wire #{i}.rated_a 缺失或不是正数（线材额定电流，缺失即红——§5 缺失语义列）"
            ));
            0.0
        }
    }
}

/// 闭合面：feeds 必须指向本档其他线组位号（自环即红）
fn rr3_wire_close(wires: &[Rr3Wire], errs: &mut Vec<String>) {
    let connectors: BTreeSet<&str> = wires.iter().map(|w| w.connector.as_str()).collect();
    for w in wires {
        rr3_close_wire(w, &connectors, errs);
    }
}

fn rr3_close_wire(w: &Rr3Wire, connectors: &BTreeSet<&str>, errs: &mut Vec<String>) {
    for f in &w.feeds {
        if f == &w.connector {
            errs.push(format!(
                "wiring[{}].feeds 自环（本线组不能承载自己）",
                w.connector
            ));
        } else if !connectors.contains(f.as_str()) {
            errs.push(format!(
                "wiring[{}].feeds = {f} 未在本接线档的位号集（接线闭合：feeds 指向同文件其他线组的位号）",
                w.connector
            ));
        }
    }
}

impl Rr3Wiring {
    pub fn rr3_wiring_is_empty(&self) -> bool {
        self.wires.is_empty()
    }

    /// 装载期校验⑦ 的跨档判据（⑦-a/b/d，后果 = 拒）：pin_id 集来自 [pins]、
    /// 域名集来自 [[supplies.domain]]、mate 集来自 contracts/devices.toml——
    /// 三份真值各归其主，由调用方（boot 装配链）携带进来，本函数不读盘。
    /// ⑦-c（额定加总）待件 demands 数据面（批N2）同链补。
    pub fn rr3_wiring_check(
        &self,
        pin_ids: &BTreeSet<String>,
        domain_names: &BTreeSet<String>,
        mate_set: &BTreeSet<String>,
    ) -> Vec<String> {
        let mut errs = Vec::new();
        for w in &self.wires {
            rr3_wire_signal(w, pin_ids, &mut errs);
            rr3_wire_net(w, domain_names, &mut errs);
            rr3_wire_mate(w, mate_set, &mut errs);
        }
        errs
    }
}

/// ⑦-a：signal 除 vcc/gnd 外一律写 pin_id（F-3）
fn rr3_wire_signal(w: &Rr3Wire, pin_ids: &BTreeSet<String>, errs: &mut Vec<String>) {
    for s in &w.signal {
        if s == "vcc" || s == "gnd" {
            continue;
        }
        if !pin_ids.contains(s) {
            errs.push(format!(
                "wiring[{}].signal 元素 {s} 不在 [pins] 的 pin_id 集（校验⑦-a：signal 除 vcc/gnd 外一律写 pin_id，不各造语法——F-3）",
                w.connector
            ));
        }
    }
}

/// ⑦-b：net 是 domains 外键（F-5）；"none" = 纯信号/地线组
fn rr3_wire_net(w: &Rr3Wire, domain_names: &BTreeSet<String>, errs: &mut Vec<String>) {
    if w.net != "none" && !domain_names.contains(&w.net) {
        errs.push(format!(
            "wiring[{}].net = {} 未在 [[supplies.domain]] 在册（校验⑦-b：wire.net 是 domains 外键——F-5）",
            w.connector, w.net
        ));
    }
}

/// ⑦-d：对端连接器型号与器件词表对账（§4.1b 门 o）
fn rr3_wire_mate(w: &Rr3Wire, mate_set: &BTreeSet<String>, errs: &mut Vec<String>) {
    if w.mate != "none" && !mate_set.contains(&w.mate) {
        errs.push(format!(
            "wiring[{}].mate = {} 不在 devices.toml 的 mate 集（校验⑦-d：对端连接器型号必须与器件词表对账，§4.1b）",
            w.connector, w.mate
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// 与生产档同构的最小合法线组（profile.rs 测试区 GOOD_WIRING 同一形状）
    const GOOD: &str = r#"
[[wire]]
connector = "J1"
hdr_pin = [1, 2]
signal = ["vcc", "gnd"]
net = "rail5v"
feeds = []
mate = "none"
keyed = true
rated_a = 1.0
gnd_common = "star-1"
"#;

    #[test]
    fn good_text_parses_field_by_field() {
        let w = rr3_wiring_from_text(GOOD).expect("应能解析");
        assert_eq!(w.wires.len(), 1);
        let wire = &w.wires[0];
        assert_eq!(wire.connector, "J1");
        assert_eq!(wire.hdr_pin, vec![1, 2]);
        assert_eq!(wire.signal, vec!["vcc", "gnd"]);
        assert_eq!(wire.net, "rail5v");
        assert!(wire.feeds.is_empty());
        assert_eq!(wire.mate, "none");
        assert!(wire.keyed);
        assert_eq!(wire.rated_a, 1.0);
        assert_eq!(wire.gnd_common, "star-1");
        assert!(!w.rr3_wiring_is_empty());
        // vcc/gnd-only 线组：signal/mate 空集即闭合；net 是 domains 外键——
        // 给 rail5v 命中域后全绿（"none" 显式值的同一判据在下方反例组钉）
        let mut domains = BTreeSet::new();
        domains.insert("rail5v".to_string());
        let errs = w.rr3_wiring_check(&BTreeSet::new(), &domains, &BTreeSet::new());
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// 缺 [[wire]] / 空表 / TOML 坏文逐条点名（fail-closed：没有"无线的机器"）
    #[test]
    fn empty_and_broken_text_are_fatal() {
        for (text, want) in [
            ("", "缺少 [[wire]]"),
            ("wire = []\n", "接线档为空"),
            ("[[wire\n", "TOML 解析失败"),
        ] {
            let errs = rr3_wiring_from_text(text).expect_err(text);
            assert!(errs[0].contains(want), "{text:?} => {errs:?}");
        }
    }

    /// 形状反例：缺键/坏类型/未知键/位序不等/位号重复一次收集完
    #[test]
    fn shape_errors_are_all_collected_and_named() {
        let text = format!(
            "{GOOD}\n[[wire]]\nhdr_pin = [1, 2]\nsignal = [\"vcc\"]\nfeeds = []\n\
             mate = \"none\"\nkeyed = \"yes\"\nrated_a = 0\ngnd_common = \"\"\nghost = 1\n\
             \n[[wire]]\nconnector = \"J1\"\n"
        );
        let errs = rr3_wiring_from_text(&text).expect_err("形状不合必须报错");
        for want in [
            "wire #1.connector 缺失或为空",
            "wire #1 的 hdr_pin（2 个）与 signal（1 个）数目不等",
            "wire #2.signal 必须是非空字符串数组",
            "wire #1.net 缺失或为空",
            "wire #1.keyed 缺失或不是布尔",
            "wire #1.rated_a 缺失或不是正数",
            "wire #1.gnd_common 缺失或为空",
            "wire #1 未知键 ghost",
            "wire #2 位号 J1 与 wire #0 重复",
        ] {
            assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
        }
    }

    /// feeds 缺键即红（§5 缺失语义列），显式 [] 合法
    #[test]
    fn feeds_missing_is_fatal_but_empty_array_is_legal() {
        let text = GOOD.replace("feeds = []\n", "");
        let errs = rr3_wiring_from_text(&text).expect_err("feeds 缺失必须报错");
        assert!(errs.iter().any(|e| e.contains("feeds 缺失")), "{errs:?}");
        assert!(rr3_wiring_from_text(GOOD).is_ok(), "显式 [] 是合法空集");
    }

    /// 闭合面：feeds 指向同文件位号；悬空引用与自环都红
    #[test]
    fn feeds_must_close_within_the_file() {
        let text = format!(
            "{GOOD}\n[[wire]]\nconnector = \"J2\"\nhdr_pin = [1, 2]\nsignal = [\"vcc\", \"gnd\"]\n\
             net = \"rail5v\"\nfeeds = [\"J9\"]\nmate = \"none\"\nkeyed = false\nrated_a = 1.0\n\
             gnd_common = \"star-1\"\n"
        );
        let errs = rr3_wiring_from_text(&text).expect_err("悬空 feeds 必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("wiring[J2].feeds = J9") && e.contains("位号集")),
            "{errs:?}"
        );

        let text = GOOD.replace("feeds = []", "feeds = [\"J1\"]");
        let errs = rr3_wiring_from_text(&text).expect_err("自环必须报错");
        assert!(errs.iter().any(|e| e.contains("自环")), "{errs:?}");
    }

    /// 校验⑦-a/b/d 反例：signal 越出 pin_id ∪ {vcc,gnd}、net 越出 domains、
    /// mate 越出 devices mate 集，逐条点名；合法显式值 none 不误报。
    /// 底稿 GOOD 的 net 先置换成 none（纯信号/地线组合法显式值）——反例集只携
    /// 三条越界噪声，条数断言才钉得住"一次收集、不短路"。
    #[test]
    fn wiring_check_names_each_cross_file_violation() {
        let base = GOOD.replace("net = \"rail5v\"", "net = \"none\"");
        let text = format!(
            "{base}\n[[wire]]\nconnector = \"J2\"\nhdr_pin = [1, 2, 3]\n\
             signal = [\"vcc\", \"act.relay.gpio\", \"gnd\"]\nnet = \"rail24\"\nfeeds = []\n\
             mate = \"JST-NOPE\"\nkeyed = false\nrated_a = 1.0\ngnd_common = \"star-1\"\n"
        );
        let w = rr3_wiring_from_text(&text).expect("形状层须先过（闭合面成立）");
        let errs = w.rr3_wiring_check(&BTreeSet::new(), &BTreeSet::new(), &BTreeSet::new());
        assert_eq!(errs.len(), 3, "{errs:?}");
        assert!(
            errs.iter()
                .any(|e| e.contains("校验⑦-a") && e.contains("act.relay.gpio")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("校验⑦-b") && e.contains("rail24")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("校验⑦-d") && e.contains("JST-NOPE")),
            "{errs:?}"
        );

        // 命中集：signal ∈ pin_id、net ∈ domains、mate ∈ mate 集全绿；
        // J2 的 vcc/gnd 与 none 显式值不误报
        let mut pins = BTreeSet::new();
        pins.insert("act.relay.gpio".to_string());
        let mut domains = BTreeSet::new();
        domains.insert("rail24".to_string());
        let mut mates = BTreeSet::new();
        mates.insert("JST-NOPE".to_string());
        let errs = w.rr3_wiring_check(&pins, &domains, &mates);
        assert!(errs.is_empty(), "{errs:?}");

        // "none" 是合法显式值（net/mate/gnd_common 三处），空集上不报 ⑦-b/⑦-d
        let text = GOOD
            .replace("net = \"rail5v\"", "net = \"none\"")
            .replace("gnd_common = \"star-1\"", "gnd_common = \"none\"");
        let w = rr3_wiring_from_text(&text).expect("应能解析");
        let errs = w.rr3_wiring_check(&BTreeSet::new(), &BTreeSet::new(), &BTreeSet::new());
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// 接线路径推导：profiles/<机>.toml → hardware/<机>/wiring.toml；
    /// 装配单不存在 = 路径不可推导（canonicalize 的 ENOENT 口径）
    #[test]
    fn wiring_path_follows_install_layout() {
        let dir = std::env::temp_dir().join(format!("rr3-wiring-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        let p = dir.join("profiles").join("mini.toml");
        std::fs::write(&p, "schema_version = 5\n").unwrap();
        // 比对基准必须是规范化后的根目录：推导走 canonicalize（macOS 的 /tmp 是
        // /private/tmp 的符号链接），拿夹具原始路径比对会红在系统布局上
        let root_canon = std::fs::canonicalize(&dir).unwrap();
        let want = root_canon.join("hardware").join("mini").join("wiring.toml");
        assert_eq!(rr3_wiring_path(&p).unwrap(), want);
        let e =
            rr3_wiring_path(Path::new("/nonexistent/nope.toml")).expect_err("读不到装配单必须致命");
        assert!(e.contains("无法解析为绝对路径"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
