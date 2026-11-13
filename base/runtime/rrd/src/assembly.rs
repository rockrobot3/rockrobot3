//! 装配快照（批N4 件九笔4，门 d 余红四路 pin/role/domain/axis 转绿的判据源）：
//! boot 装载链验收通过的不可变快照 + `rr3.ctl.wiring` 应答渲染 + [pins]×wiring
//! 全交叉逐条点名（全案 §7 ⑤ 门"出厂自检必测 [pins]×wiring.toml 全交叉"）。
//!
//! 为什么是"快照"而不是查询时重读：装载面（校验①—⑩）在 boot 一次性执法
//! （fail-closed），ctl 应答必须展示**验收通过的那份**——查询时重读会拿到装载面
//! 从未见过、甚至已被拒 boot 的中间态（boot 后文件被改）。快照 boot 时一次定格
//! （OnceLock 单点，日志环同款进程级静态），ctl 渲染只读它。
//!
//! 为什么判据在 rrd 不在面板：全交叉交叉一致性判据的真值单点放数据 owner（rrd）；
//! 面板（rr3-interact）只取数渲染——判据写面板侧即第二真值点，与"装载期校验⑦
//! 在 rrd、面板不写第二份"同一条纪律。数据通道走 ctl 而非新数据主题：`rr3-runtime/*`
//! 是守护进程的纯下行广播命名空间（registry/topics_load 双层拒冒充），守护进程
//! 自有数据的查询面就是 ADR-0010 的 `rr3.ctl.*`（面板既有 ctl 短连客户端零新通道）。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde_json::{json, Value};

use rr_registry::Rr3RoleTable;

use crate::axis::Rr3AxisFile;
use crate::profile::Rr3Profile;
use crate::wiring::{Rr3Wire, Rr3Wiring};

/// 一份装配快照（不可变）。成员全是 boot 链已装载并验收的原值（clone 语义），
/// 不做第二次解析——快照 = "本进程 boot 时装载面看到的世界"。PartialEq（无
/// Eq：`Rr3Wiring.rated_a` 是 f64）随 `Rr3ProfileBoot` 的判等面需要。
#[derive(Debug, Clone, PartialEq)]
pub struct Rr3AssemblySnapshot {
    /// 形态名（装配单文件 stem，如 "esp32-mini"）——应答顶层的"这是哪台机器"
    pub profile_name: String,
    /// 装配单本体（pins / supplies.domains / bindings / …，校验①—⑩ 验收后的值面）
    pub profile: Rr3Profile,
    /// 接线档（校验⑦-a/b/d 验收后的线组）
    pub wiring: Rr3Wiring,
    /// 器件表的 mate → peak_ma 索引（装载链同一份 `contracts/devices.toml` 派生，
    /// 非第二真值点）：额定加总族（校验⑦-c1·自检时点判，批N5 件五）的峰值事实源，
    /// 判据真值单点仍住 [`rr3_cross_checks`]。
    pub peak_by_mate: BTreeMap<String, i64>,
    /// 轴档案（`hardware/<形态>/axis/*.toml` 全量合法装载件，件二装载器产物；
    /// 非投影选轴——面板出图要看见全部轴，投影 RR3_ACT_AXIS_STATE 只属于注入面）
    pub axis: Vec<Rr3AxisFile>,
    /// 角色表（`contracts/roles.toml` 查册验收后的登记面）
    pub roles: Rr3RoleTable,
}

static RR3_ASSEMBLY_SNAPSHOT: OnceLock<Rr3AssemblySnapshot> = OnceLock::new();

/// boot 装载链成功后一次定格（daemon.rs 单点调用）。重复 set 首份胜出并回
/// false——快照是"本次进程 boot 的装载结果"，后来者无权覆盖（与 OnceLock
/// 语义一致；测试同进程多次 boot 也因此不 panic，首份在册即如实）。
pub fn rr3_assembly_set(snapshot: Rr3AssemblySnapshot) -> bool {
    RR3_ASSEMBLY_SNAPSHOT.set(snapshot).is_ok()
}

/// 快照只读口（conn.rs `rr3.ctl.wiring` 渲染源）。None = 未以 `--profile` 启动
/// （`--manifest` 调试路径无装配快照）——应答如实报不可得，不造假数据。
pub fn rr3_assembly_snap() -> Option<&'static Rr3AssemblySnapshot> {
    RR3_ASSEMBLY_SNAPSHOT.get()
}

/// 全交叉逐条判果三态（F-31 判据词表：三态 + 逐条点名，整体红绿是派生量）。
pub const RR3_CHECK_PASS: &str = "pass";
pub const RR3_CHECK_FAIL: &str = "fail";
pub const RR3_CHECK_NA: &str = "not-applicable";

/// 一条交叉点名（机器可读）。`reason` 恒填：pass 说明判了什么、not-applicable
/// 说明为什么不适用、fail 给病灶——产线照单逐条对机器，光有判果没有理由没法对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AssemblyCheck {
    /// 点名位：`signal:<连接器>[<导体序>]:<信号>` / `net:<连接器>:<网络>` /
    /// `rated:<连接器>` / `pin:<pin_id>`（与 wiring 档九字段、[pins] 外键语法
    /// 同一套词）
    pub item: String,
    /// 三态判果（[`RR3_CHECK_PASS`] / [`RR3_CHECK_FAIL`] / [`RR3_CHECK_NA`]）
    pub pass: &'static str,
    /// 判据说明
    pub reason: String,
}

/// [pins]×wiring 全交叉逐条点名（纯函数，判据真值单点）。四族：
/// - **signal 族**（wiring→pins 方向，装载期⑦-a 的自检时点复演）：每线组每导体，
///   `vcc`/`gnd` 记 not-applicable（供电/地导体非 pin 引用，⑦-a 豁免面），其余
///   须在 pin_id 集——装载期已执法，这里复演防"快照与装载面漂移"（漂移即 fail，
///   正是自检要抓的）；
/// - **net 族**（⑦-b 复演）：每线组 net，`none` 记 not-applicable（纯信号/地线组），
///   其余须在 domains 名集；
/// - **额定加总族**（校验⑦-c1·自检时点判——装载期 ⑦-c1 待建，先在自检点判，
///   同落点族先例）：每线组，本线 + feeds 闭包各线组按 mate 查器件峰值加总
///   ≤ `rated_a × 1000`（§5 ⑦-c1 / F-10 拓扑加总，不逐角色判）；`net = "none"`
///   纯信号/地线组记 not-applicable（⑦-c1 无判据面）；feeds 一般环/悬空装载期
///   不判（闭合面只拒自环/悬空在册）——环即快照与装载面漂移，fail，正是自检
///   要抓的；
/// - **落点族**（pins→wiring 方向，装载期未判的自检新判据）：每 pin_id 数接线
///   引用导体数，≥1 记 pass；**零引用记 not-applicable**——现役合法态（ui.face.spi
///   物理走线总线钉死不占连接器维 / power.charger.meter 共读源 / env·imu 占位格 /
///   tilt 待板侧收敛，各见装配注记），判 fail 会与在册数据矛盾；fail 分支只留给
///   数据漂移（如未来装载面撤了⑦-a 而快照仍旧）。
pub fn rr3_cross_checks(
    profile: &Rr3Profile,
    wiring: &Rr3Wiring,
    peak_by_mate: &BTreeMap<String, i64>,
) -> Vec<Rr3AssemblyCheck> {
    let pin_ids: BTreeSet<String> = profile.pins.iter().map(|p| p.rr3_pin_id()).collect();
    let domain_names: BTreeSet<&str> = profile
        .supplies
        .domains
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    let mut out = Vec::new();
    // pin_id → 接线引用导体数（落点族判据面；signal 族顺路记账，一遍扫完两族）
    let mut refs: BTreeMap<&str, usize> = BTreeMap::new();
    // 额定加总族的闭包图：位号 → 线组（feeds 位号集装载期已判在本档内闭合）
    let by_conn: BTreeMap<&str, &Rr3Wire> = wiring
        .wires
        .iter()
        .map(|w| (w.connector.as_str(), w))
        .collect();
    for w in &wiring.wires {
        rr3_check_signal(w, &pin_ids, &mut refs, &mut out);
        rr3_check_net(w, &domain_names, &mut out);
        rr3_check_rated(w, peak_by_mate, &by_conn, &mut out);
    }
    rr3_check_pins(profile, &refs, &mut out);
    out
}

fn rr3_power_conductor(sig: &str) -> bool {
    sig == "vcc" || sig == "gnd"
}

/// signal 族（wiring→pins 方向，⑦-a 自检复演）：每导体一条，vcc/gnd 记 n-a
fn rr3_check_signal<'a>(
    w: &'a Rr3Wire,
    pin_ids: &BTreeSet<String>,
    refs: &mut BTreeMap<&'a str, usize>,
    out: &mut Vec<Rr3AssemblyCheck>,
) {
    for (k, sig) in w.signal.iter().enumerate() {
        let item = format!("signal:{}[{}]:{sig}", w.connector, k);
        rr3_signal_row(item, sig.as_str(), pin_ids, refs, out);
    }
}

fn rr3_signal_row<'a>(
    item: String,
    sig: &'a str,
    pin_ids: &BTreeSet<String>,
    refs: &mut BTreeMap<&'a str, usize>,
    out: &mut Vec<Rr3AssemblyCheck>,
) {
    if rr3_power_conductor(sig) {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_NA,
            reason: "供电/地导体，非 pin 引用（校验⑦-a 豁免面）".to_string(),
        });
    } else if pin_ids.contains(sig) {
        *refs.entry(sig).or_insert(0) += 1;
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_PASS,
            reason: "已解析到 [pins] 装载快照（校验⑦-a 复演）".to_string(),
        });
    } else {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_FAIL,
            reason: "signal 不在 [pins] pin_id 集（校验⑦-a 破——快照与装载面漂移）".to_string(),
        });
    }
}

/// net 族（⑦-b 复演）：none 记 n-a，其余须在 domains 名集
fn rr3_check_net(w: &Rr3Wire, domain_names: &BTreeSet<&str>, out: &mut Vec<Rr3AssemblyCheck>) {
    let item = format!("net:{}:{}", w.connector, w.net);
    if w.net == "none" {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_NA,
            reason: "纯信号/地线组，不承载电力网络导体（校验⑦-b 豁免面）".to_string(),
        });
    } else if domain_names.contains(w.net.as_str()) {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_PASS,
            reason: "已在 [[supplies.domain]] 在册（校验⑦-b 复演）".to_string(),
        });
    } else {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_FAIL,
            reason: "net 不在 domains（校验⑦-b 破——快照与装载面漂移）".to_string(),
        });
    }
}

/// 额定加总族（⑦-c1·自检时点判）：本线 + feeds 闭包按 mate 查峰值加总 ≤ rated_a×1000
fn rr3_check_rated<'a>(
    w: &'a Rr3Wire,
    peak_by_mate: &BTreeMap<String, i64>,
    by_conn: &BTreeMap<&str, &'a Rr3Wire>,
    out: &mut Vec<Rr3AssemblyCheck>,
) {
    let item = format!("rated:{}", w.connector);
    if w.net == "none" {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_NA,
            reason: "纯信号/地线组，不承载电力导体（校验⑦-c1 无判据面）".to_string(),
        });
        return;
    }
    // mate = none 直连无注册对端型号，负载未注册按 0 计（诚实边界）；mate 非 none
    // 必在器件 mate 集（装载期⑦-d 已判），查不到只会是契约坏形，按 0 计不炸。
    rr3_rated_body(w, peak_by_mate, by_conn, item, out);
}

/// 额定加总体：馈线闭包可判则加总，闭包破直接点名 FAIL。
fn rr3_rated_body<'a>(
    w: &'a Rr3Wire,
    peak_by_mate: &BTreeMap<String, i64>,
    by_conn: &BTreeMap<&str, &'a Rr3Wire>,
    item: String,
    out: &mut Vec<Rr3AssemblyCheck>,
) {
    match rated_closure(w, by_conn) {
        Err(fault) => out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_FAIL,
            reason: format!("feeds {fault}——额定加总不可判（快照与装载面漂移，正是自检要抓的）"),
        }),
        Ok(closure) => rr3_rated_ok(w, &closure, peak_by_mate, item, out),
    }
}

fn rr3_rated_ok(
    w: &Rr3Wire,
    closure: &[&Rr3Wire],
    peak_by_mate: &BTreeMap<String, i64>,
    item: String,
    out: &mut Vec<Rr3AssemblyCheck>,
) {
    let sum_ma: i64 = closure.iter().map(|g| rr3_peak_ma(g, peak_by_mate)).sum();
    if sum_ma as f64 > w.rated_a * 1000.0 {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_FAIL,
            reason: format!(
                "馈线峰值加总 {sum_ma}mA > 本线额定 {}mA（校验⑦-c1 破——F-10：本线 + feeds 闭包 {} 组按拓扑加总，不逐角色判）",
                w.rated_a * 1000.0,
                closure.len()
            ),
        });
    } else {
        out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_PASS,
            reason: format!(
                "馈线峰值加总 {sum_ma}mA ≤ 本线额定 {}mA（校验⑦-c1 额定加总·自检时点判——F-10 拓扑加总；mate=none 直连负载未注册按 0 计）",
                w.rated_a * 1000.0
            ),
        });
    }
}

/// 单线组对端型号峰值（mate=none 直连按 0 计；查不到按 0，契约坏形不炸）。
fn rr3_peak_ma(g: &Rr3Wire, peak_by_mate: &BTreeMap<String, i64>) -> i64 {
    if g.mate == "none" {
        0
    } else {
        peak_by_mate.get(g.mate.as_str()).copied().unwrap_or(0)
    }
}

/// 落点族（pins→wiring）：每 pin_id 数引用导体，≥1 pass、零引用 n-a（合法态）
fn rr3_check_pins(
    profile: &Rr3Profile,
    refs: &BTreeMap<&str, usize>,
    out: &mut Vec<Rr3AssemblyCheck>,
) {
    for pin in &profile.pins {
        let id = pin.rr3_pin_id();
        let item = format!("pin:{id}");
        rr3_pin_ref_row(item, &id, refs, out);
    }
}

fn rr3_pin_ref_row(
    item: String,
    id: &str,
    refs: &BTreeMap<&str, usize>,
    out: &mut Vec<Rr3AssemblyCheck>,
) {
    match refs.get(id) {
        Some(n) => out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_PASS,
            reason: format!("接线引用 {n} 导体（[pins]×wiring 交叉命中）"),
        }),
        None => out.push(Rr3AssemblyCheck {
            item,
            pass: RR3_CHECK_NA,
            reason:
                "零引用（接线档未收敛该 pin——总线钉死/共读源/占位格/待收敛类合法态，见装配注记）"
                    .to_string(),
        }),
    }
}

/// feeds 自反闭包（含起点——本线组对端器件的峰值是本线的主负载，F-10 的
/// "该线承载之和"）+ 环检测：三色迭代 DFS，gray 重逢 = 环，black 重逢 = 菱形
/// 分叉（两条路汇到同一线组，只收账一次，不误报）。Err = 闭包不可判的漂移：
/// "环"（装载期闭合面只拒自环，一般环漏到这里 = 快照与装载面漂移）或"悬空"
/// （feeds 指向不在册位号——装载期已拒，同理漂移；防御分支保纯函数对任意
/// 输入不炸）。
fn rated_closure<'a>(
    start: &'a Rr3Wire,
    by_conn: &BTreeMap<&str, &'a Rr3Wire>,
) -> Result<Vec<&'a Rr3Wire>, &'static str> {
    let mut color: BTreeMap<&str, u8> = BTreeMap::new(); // 1 = 在栈，2 = 已收账
    let mut stack: Vec<(&Rr3Wire, usize)> = vec![(start, 0)];
    let mut acc: Vec<&Rr3Wire> = Vec::new();
    color.insert(start.connector.as_str(), 1);
    while let Some((w, i)) = stack.pop() {
        if i < w.feeds.len() {
            let next = w.feeds[i].as_str();
            stack.push((w, i + 1));
            match color.get(next) {
                Some(&1) => return Err("环"),
                // 2 = 已收账（black 重逢——菱形分叉只收账一次，不误报环）；
                // 其余值按构造不可达（只写 1/2）
                Some(_) => {}
                None => {
                    let nw = by_conn.get(next).copied().ok_or("悬空")?;
                    color.insert(next, 1);
                    stack.push((nw, 0));
                }
            }
        } else {
            color.insert(w.connector.as_str(), 2);
            acc.push(w);
        }
    }
    Ok(acc)
}

/// `rr3.ctl.wiring` 应答渲染（纯函数）。字段面 = 门 d 分母四路的字段级全集
/// （pin 十一键 / domain 十一键 / axis 十四键 / role requires 逐项）——covers
/// 申报的"真实渲染"承诺在这里兑现，渲染完整性由本文件测试区用门 d 同源词表
/// （`RR3_PIN_KEYS` / `RR3_DOMAIN_KEYS` / `RR3_ACT_PROFILE_FIELDS`）钉死。
/// 不含 `available` 位：那是全局单例语义（快照在不在册），归
/// [`rr3_assembly_json`] 的壳——纯函数才能用局部快照无竞态地单测。
pub fn rr3_assembly_render(s: &Rr3AssemblySnapshot) -> Value {
    let p = &s.profile;
    json!({
        "profile": s.profile_name,
        "pins": p.pins.iter().map(|pin| json!({
            "pin_id": pin.rr3_pin_id(),
            "role": pin.role,
            "instance": pin.instance,
            "controller": pin.controller,
            "channel": pin.channel,
            "physical": pin.physical,
            "direction": pin.direction,
            "rail": pin.rail,
            "addr": pin.addr,
            "device": pin.device,
            "failsafe": pin.failsafe,
            "mux_group": pin.mux_group,
        })).collect::<Vec<Value>>(),
        "domains": p.supplies.domains.iter().map(|d| json!({
            "name": d.name,
            "nominal_mv": d.nominal_mv,
            "mild_mv": d.mild_mv,
            "severe_mv": d.severe_mv,
            "current_cap_ma": d.current_cap_ma,
            "sense": d.sense,
            "current_sense": d.current_sense,
            "switch": d.switch,
            "switchable": d.switchable,
            "armed": d.armed,
            "max_age_ms": d.max_age_ms,
            "evidence": d.evidence,
        })).collect::<Vec<Value>>(),
        "wires": s.wiring.wires.iter().map(|w| json!({
            "connector": w.connector,
            "hdr_pin": w.hdr_pin,
            "signal": w.signal,
            "net": w.net,
            "feeds": w.feeds,
            "mate": w.mate,
            "keyed": w.keyed,
            "rated_a": w.rated_a,
            "gnd_common": w.gnd_common,
        })).collect::<Vec<Value>>(),
        "axis": s.axis.iter().map(|f| {
            let a = &f.profile;
            // schema 十四键之一：装载面只认 1（对账后不留存），快照里必为 1
            json!({
                "pin": f.pin,
                "schema": 1,
                "center_deg": a.center_deg,
                "lo_deg": a.lo_deg,
                "hi_deg": a.hi_deg,
                "dir_sign": a.dir_sign,
                "slew_deg_s": a.slew_deg_s,
                "settle_ms": a.settle_ms,
                "hold": a.hold.rr3_act_hold_name(),
                "current_ceil_ma": a.current_ceil_ma,
                "warmup_ms": a.warmup_ms,
                "hard_stop": a.hard_stop.rr3_hard_stop_name(),
                "calibrated": a.calibrated,
                "source": a.source.rr3_act_source_name(),
            })
        }).collect::<Vec<Value>>(),
        "roles": s.roles.rr3_role_names().iter().map(|name| json!({
            "name": name,
            "slot": s.roles.rr3_role_slot(name),
            "requires": s.roles.rr3_role_requires(name),
            "derives": s.roles.rr3_role_derives(name),
            "direction": s.roles.rr3_role_direction(name),
        })).collect::<Vec<Value>>(),
        "checks": rr3_cross_checks(p, &s.wiring, &s.peak_by_mate).iter().map(|c| {
            json!({ "item": c.item, "pass": c.pass, "reason": c.reason })
        }).collect::<Vec<Value>>(),
    })
}

/// 全局壳（conn.rs 单点调用）：快照在册 → 渲染 + `available:true`；未以
/// `--profile` 启动（`--manifest` 调试路径无装配快照）→ 如实报不可得，不造假
/// 数据。None 分支不单测——OnceLock 全局单例语义下，cargo test 同二进制并行
/// 线程里"未 set"无法与"已 set"的用例共存（首份胜出的一锤子买卖），该分支由
/// `--manifest` 路径的端到端用例覆盖。
pub fn rr3_assembly_json() -> Value {
    match rr3_assembly_snap() {
        Some(s) => {
            let mut v = rr3_assembly_render(s);
            v["available"] = json!(true);
            v
        }
        None => json!({
            "available": false,
            "reason": "未以 --profile 启动（--manifest 调试路径无装配快照），接线图不可得",
        }),
    }
}

#[cfg(test)]
mod tests;
