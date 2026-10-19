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
use crate::wiring::Rr3Wiring;

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
pub fn rr3_assembly_snapshot_set(snapshot: Rr3AssemblySnapshot) -> bool {
    RR3_ASSEMBLY_SNAPSHOT.set(snapshot).is_ok()
}

/// 快照只读口（conn.rs `rr3.ctl.wiring` 渲染源）。None = 未以 `--profile` 启动
/// （`--manifest` 调试路径无装配快照）——应答如实报不可得，不造假数据。
pub fn rr3_assembly_snapshot() -> Option<&'static Rr3AssemblySnapshot> {
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
    /// `pin:<pin_id>`（与 wiring 档九字段、[pins] 外键语法同一套词）
    pub item: String,
    /// 三态判果（[`RR3_CHECK_PASS`] / [`RR3_CHECK_FAIL`] / [`RR3_CHECK_NA`]）
    pub pass: &'static str,
    /// 判据说明
    pub reason: String,
}

/// [pins]×wiring 全交叉逐条点名（纯函数，判据真值单点）。三族：
/// - **signal 族**（wiring→pins 方向，装载期⑦-a 的自检时点复演）：每线组每导体，
///   `vcc`/`gnd` 记 not-applicable（供电/地导体非 pin 引用，⑦-a 豁免面），其余
///   须在 pin_id 集——装载期已执法，这里复演防"快照与装载面漂移"（漂移即 fail，
///   正是自检要抓的）；
/// - **net 族**（⑦-b 复演）：每线组 net，`none` 记 not-applicable（纯信号/地线组），
///   其余须在 domains 名集；
/// - **落点族**（pins→wiring 方向，装载期未判的自检新判据）：每 pin_id 数接线
///   引用导体数，≥1 记 pass；**零引用记 not-applicable**——现役合法态（ui.face.spi
///   物理走线总线钉死不占连接器维 / power.charger.meter 共读源 / env·imu 占位格 /
///   tilt 待板侧收敛，各见装配注记），判 fail 会与在册数据矛盾；fail 分支只留给
///   数据漂移（如未来装载面撤了⑦-a 而快照仍旧）。
pub fn rr3_assembly_cross_checks(
    profile: &Rr3Profile,
    wiring: &Rr3Wiring,
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
    for w in &wiring.wires {
        for (k, sig) in w.signal.iter().enumerate() {
            let item = format!("signal:{}[{}]:{sig}", w.connector, k);
            if sig == "vcc" || sig == "gnd" {
                out.push(Rr3AssemblyCheck {
                    item,
                    pass: RR3_CHECK_NA,
                    reason: "供电/地导体，非 pin 引用（校验⑦-a 豁免面）".to_string(),
                });
            } else if pin_ids.contains(sig) {
                *refs.entry(sig.as_str()).or_insert(0) += 1;
                out.push(Rr3AssemblyCheck {
                    item,
                    pass: RR3_CHECK_PASS,
                    reason: "已解析到 [pins] 装载快照（校验⑦-a 复演）".to_string(),
                });
            } else {
                out.push(Rr3AssemblyCheck {
                    item,
                    pass: RR3_CHECK_FAIL,
                    reason: "signal 不在 [pins] pin_id 集（校验⑦-a 破——快照与装载面漂移）"
                        .to_string(),
                });
            }
        }
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
    for pin in &profile.pins {
        let id = pin.rr3_pin_id();
        let item = format!("pin:{id}");
        match refs.get(id.as_str()) {
            Some(n) => out.push(Rr3AssemblyCheck {
                item,
                pass: RR3_CHECK_PASS,
                reason: format!("接线引用 {n} 导体（[pins]×wiring 交叉命中）"),
            }),
            None => out.push(Rr3AssemblyCheck {
                item,
                pass: RR3_CHECK_NA,
                reason: "零引用（接线档未收敛该 pin——总线钉死/共读源/占位格/待收敛类合法态，见装配注记）"
                    .to_string(),
            }),
        }
    }
    out
}

/// `rr3.ctl.wiring` 应答渲染（纯函数）。字段面 = 门 d 分母四路的字段级全集
/// （pin 十一键 / domain 十一键 / axis 十四键 / role requires 逐项）——covers
/// 申报的"真实渲染"承诺在这里兑现，渲染完整性由本文件测试区用门 d 同源词表
/// （`RR3_PIN_KEYS` / `RR3_DOMAIN_KEYS` / `RR3_ACT_PROFILE_FIELDS`）钉死。
/// 不含 `available` 位：那是全局单例语义（快照在不在册），归
/// [`rr3_assembly_wiring_json`] 的壳——纯函数才能用局部快照无竞态地单测。
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
                "hard_stop": a.hard_stop.rr3_act_hard_stop_name(),
                "calibrated": a.calibrated,
                "source": a.source.rr3_act_source_name(),
            })
        }).collect::<Vec<Value>>(),
        "roles": s.roles.rr3_role_table_names().iter().map(|name| json!({
            "name": name,
            "slot": s.roles.rr3_role_table_slot(name),
            "requires": s.roles.rr3_role_table_requires(name),
            "derives": s.roles.rr3_role_table_derives(name),
            "direction": s.roles.rr3_role_table_direction(name),
        })).collect::<Vec<Value>>(),
        "checks": rr3_assembly_cross_checks(p, &s.wiring).iter().map(|c| {
            json!({ "item": c.item, "pass": c.pass, "reason": c.reason })
        }).collect::<Vec<Value>>(),
    })
}

/// 全局壳（conn.rs 单点调用）：快照在册 → 渲染 + `available:true`；未以
/// `--profile` 启动（`--manifest` 调试路径无装配快照）→ 如实报不可得，不造假
/// 数据。None 分支不单测——OnceLock 全局单例语义下，cargo test 同二进制并行
/// 线程里"未 set"无法与"已 set"的用例共存（首份胜出的一锤子买卖），该分支由
/// `--manifest` 路径的端到端用例覆盖。
pub fn rr3_assembly_wiring_json() -> Value {
    match rr3_assembly_snapshot() {
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
mod tests {
    use super::*;
    use rr_hal_api::{Rr3ActHardStop, Rr3ActHold, Rr3ActProfile, Rr3ActSource};
    use rr_registry::rr3_registry_role_table_from_text;

    /// 最小快照：一 pin（有落点）+ 一 pin（零引用）+ 一域 + 一线组 + 一轴 + 一角色
    fn snapshot() -> Rr3AssemblySnapshot {
        let profile = Rr3Profile {
            platform_tier: "host".into(),
            platform_chip: "rk3566".into(),
            supplies: crate::profile::Rr3Supplies {
                domains: vec![crate::profile::Rr3Domain {
                    name: "rail5v".into(),
                    nominal_mv: 5000,
                    mild_mv: 4600,
                    severe_mv: 4400,
                    current_cap_ma: 2000,
                    sense: "none".into(),
                    current_sense: "none".into(),
                    switch: "none".into(),
                    switchable: false,
                    armed: false,
                    max_age_ms: 0,
                }],
                ..Default::default()
            },
            pins: vec![
                crate::profile::Rr3PinEntry {
                    role: "act.axis.pwm".into(),
                    instance: "pan".into(),
                    controller: "pwm0".into(),
                    channel: Some(0),
                    physical: vec![32],
                    direction: "out".into(),
                    rail: "rail5v".into(),
                    addr: "none".into(),
                    device: String::new(),
                    failsafe: "release".into(),
                    mux_group: vec![],
                },
                crate::profile::Rr3PinEntry {
                    role: "ui.face.spi".into(),
                    instance: String::new(),
                    controller: "spidev0".into(),
                    channel: Some(0),
                    physical: vec![],
                    direction: "out".into(),
                    rail: "none".into(),
                    addr: "none".into(),
                    device: String::new(),
                    failsafe: "release".into(),
                    mux_group: vec![],
                },
            ],
            ..Default::default()
        };
        let wiring = crate::wiring::Rr3Wiring {
            wires: vec![crate::wiring::Rr3Wire {
                connector: "J3".into(),
                hdr_pin: vec![2, 3, 4],
                signal: vec!["vcc".into(), "act.axis.pwm.pan".into(), "gnd".into()],
                net: "rail5v".into(),
                feeds: vec![],
                mate: "none".into(),
                keyed: true,
                rated_a: 1.0,
                gnd_common: "star-1".into(),
            }],
        };
        let axis = vec![Rr3AxisFile {
            pin: "act.axis.pwm.pan".into(),
            profile: Rr3ActProfile {
                center_deg: 90,
                lo_deg: 60,
                hi_deg: 120,
                dir_sign: 1,
                slew_deg_s: 100,
                settle_ms: 200,
                hold: Rr3ActHold::Release,
                current_ceil_ma: 750,
                warmup_ms: 0,
                hard_stop: Rr3ActHardStop::Yes,
                calibrated: true,
                source: Rr3ActSource::Manual,
            },
        }];
        let roles = rr3_registry_role_table_from_text(
            "[[role]]\nname = \"act.axis.pwm\"\nslot = \"act.servo\"\nrequires = [\"controller\", \"channel\"]\nderives = [\"pwm_chip\"]\n",
        );
        Rr3AssemblySnapshot {
            profile_name: "test形态".into(),
            profile,
            wiring,
            axis,
            roles,
        }
    }

    #[test]
    fn snapshot_set_is_first_wins_and_readable() {
        // OnceLock 进程级单点：首份胜出、二份被拒、只读口取首份
        let first = snapshot();
        let mut second = snapshot();
        second.profile_name = "另一形态".into();
        assert!(rr3_assembly_snapshot_set(first));
        assert!(!rr3_assembly_snapshot_set(second));
        assert_eq!(
            rr3_assembly_snapshot().expect("首份应可读").profile_name,
            "test形态"
        );
    }

    #[test]
    fn cross_checks_cover_three_families_with_three_states() {
        let s = snapshot();
        let checks = rr3_assembly_cross_checks(&s.profile, &s.wiring);
        // signal 族 3 导体：vcc/gnd 记 not-applicable，pin 导体记 pass
        let sig = |item: &str| checks.iter().find(|c| c.item == item).expect("点名位必在");
        assert_eq!(sig("signal:J3[0]:vcc").pass, RR3_CHECK_NA);
        assert_eq!(sig("signal:J3[1]:act.axis.pwm.pan").pass, RR3_CHECK_PASS);
        assert_eq!(sig("signal:J3[2]:gnd").pass, RR3_CHECK_NA);
        // net 族：在册域 pass（none 形状归独立用例）
        assert_eq!(sig("net:J3:rail5v").pass, RR3_CHECK_PASS);
        // 落点族：pan 有 1 导体引用 pass；face.spi 零引用记 not-applicable（合法态）
        assert_eq!(sig("pin:act.axis.pwm.pan").pass, RR3_CHECK_PASS);
        assert_eq!(sig("pin:ui.face.spi").pass, RR3_CHECK_NA);
        assert!(sig("pin:ui.face.spi").reason.contains("零引用"));
        // fail 分支：signal 越出 pin_id 集（⑦-a 破）与 net 越出 domains（⑦-b 破）
        let broken = crate::wiring::Rr3Wiring {
            wires: vec![crate::wiring::Rr3Wire {
                connector: "JX".into(),
                hdr_pin: vec![1],
                signal: vec!["act.axis.pwm.tilt".into()],
                net: "rail24".into(),
                feeds: vec![],
                mate: "none".into(),
                keyed: false,
                rated_a: 0.5,
                gnd_common: "none".into(),
            }],
        };
        let bad = rr3_assembly_cross_checks(&s.profile, &broken);
        assert_eq!(bad[0].pass, RR3_CHECK_FAIL);
        assert!(bad[0].reason.contains("⑦-a"));
        assert_eq!(bad[1].pass, RR3_CHECK_FAIL);
        assert!(bad[1].reason.contains("⑦-b"));
    }

    #[test]
    fn wiring_json_renders_every_denominator_field() {
        // 门 d 分母四路的字段级全集必须逐键出现在应答（covers 申报"真实渲染"
        // 的兑现判据）——词表自门 d 同源常量导出，不抄第二份清单。
        // 测纯函数 + 局部快照：OnceLock 全局态与 cargo test 并行线程有竞态，
        // 纯函数口才可测（全局壳只做 available 位与 None 分支，语义见函数注）。
        let s = snapshot();
        let v = rr3_assembly_render(&s);
        assert_eq!(v["profile"], json!("test形态"));
        let pin = &v["pins"][0];
        for k in crate::profile::RR3_PIN_KEYS {
            assert!(
                pin.get(k).is_some(),
                "pin 应答缺分母字段 {k}（covers 申报 pin/{k} 的渲染面）"
            );
        }
        let dom = &v["domains"][0];
        for k in crate::profile::RR3_DOMAIN_KEYS {
            assert!(
                dom.get(k).is_some(),
                "domain 应答缺分母字段 {k}（covers 申报 domain/{k} 的渲染面）"
            );
        }
        let axis = &v["axis"][0];
        for f in rr_hal_api::RR3_ACT_PROFILE_FIELDS {
            assert!(
                axis.get(f.key).is_some(),
                "axis 应答缺分母字段 {}（covers 申报 axis/{} 的渲染面）",
                f.key,
                f.key
            );
        }
        assert_eq!(axis["hold"], json!("release"));
        assert_eq!(axis["hard_stop"], json!("yes"));
        assert_eq!(axis["source"], json!("manual"));
        // role 族：requires 逐项在应答（门 d role/<req> 的渲染面）
        let role = &v["roles"][0];
        assert_eq!(role["name"], json!("act.axis.pwm"));
        assert_eq!(role["requires"], json!(["controller", "channel"]));
        // checks 逐条进应答
        assert!(v["checks"].as_array().expect("checks 应为数组").len() >= 5);
    }
}
