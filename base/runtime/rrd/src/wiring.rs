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
    let Some(stem) = abs.file_stem().and_then(std::ffi::OsStr::to_str) else {
        return Err(format!("装配单 {} 无文件名", abs.display()));
    };
    let root = rr3_hardware_root(&abs)?;
    Ok(root.join("hardware").join(stem).join("wiring.toml"))
}

/// 装配单的两级父目录（`<root>/profiles/` 的 root），推不出即点名。
fn rr3_hardware_root(abs: &std::path::Path) -> Result<&std::path::Path, String> {
    let Some(root) = abs.parent().and_then(std::path::Path::parent) else {
        return Err(format!(
            "装配单 {} 无法定位 hardware-root（须在 <root>/profiles/ 下）",
            abs.display()
        ));
    };
    Ok(root)
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
    rr3_wire_collect(entries, &mut wires, &mut seen, &mut errs);
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

/// 逐条 [[wire]]：非表项点名，表项走未知键/身份/字段三段。
fn rr3_wire_collect<'a>(
    entries: &'a [toml::Value],
    wires: &mut Vec<Rr3Wire>,
    seen: &mut BTreeMap<&'a str, usize>,
    errs: &mut Vec<String>,
) {
    for (i, item) in entries.iter().enumerate() {
        let Some(t) = item.as_table() else {
            errs.push(format!("wire #{i} 必须是 TOML 表（[[wire]]）"));
            continue;
        };
        rr3_wire_unknown(i, t, errs);
        let connector = rr3_wire_ident(i, t, seen, errs);
        wires.push(rr3_wire_fields(i, t, connector, errs));
    }
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
    match rr3_rated_parse(t) {
        Some(a) if a > 0.0 => a,
        _ => {
            errs.push(format!(
                "wire #{i}.rated_a 缺失或不是正数（线材额定电流，缺失即红——§5 缺失语义列）"
            ));
            0.0
        }
    }
}

/// rated_a 取值（浮点或整数皆收）：缺失或非数值 = None，域判归调用方。
fn rr3_rated_parse(t: &toml::value::Table) -> Option<f64> {
    t.get("rated_a")
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
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
        rr3_close_one(w, f, connectors, errs);
    }
}

/// 单条 feeds 两判：自环 / 不在本档位号集。
fn rr3_close_one(w: &Rr3Wire, f: &str, connectors: &BTreeSet<&str>, errs: &mut Vec<String>) {
    if f == w.connector.as_str() {
        errs.push(format!(
            "wiring[{}].feeds 自环（本线组不能承载自己）",
            w.connector
        ));
    } else if !connectors.contains(&f) {
        errs.push(format!(
            "wiring[{}].feeds = {f} 未在本接线档的位号集（接线闭合：feeds 指向同文件其他线组的位号）",
            w.connector
        ));
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
        rr3_signal_one(w, s, pin_ids, errs);
    }
}

/// 单 signal 元素：vcc/gnd 免检，其余一律须是 [pins] 的 pin_id。
fn rr3_signal_one(w: &Rr3Wire, s: &str, pin_ids: &BTreeSet<String>, errs: &mut Vec<String>) {
    if s == "vcc" || s == "gnd" {
        return;
    }
    if !pin_ids.contains(s) {
        errs.push(format!(
            "wiring[{}].signal 元素 {s} 不在 [pins] 的 pin_id 集（校验⑦-a：signal 除 vcc/gnd 外一律写 pin_id，不各造语法——F-3）",
            w.connector
        ));
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
mod tests;
