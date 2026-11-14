//! 拒链与降级点名：装载期校验族、证据核验、装配扫描与预算判据。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rr_registry::{
    Rr3AssembleDemand, Rr3AssembleFacts, Rr3DeviceTable, Rr3PlatformTable, Rr3RoleTable,
    Rr3SlotTable,
};

use super::super::model::*;
use crate::profile_checks::{
    rr3_check_budgets, rr3_check_claims, rr3_check_domains, rr3_check_hw_keys, rr3_check_pins,
    rr3_check_rails, rr3_check_slot_refs, rr3_check_tier, rr3_scan_modules,
};
use crate::profile_pins::{rr3_pin_capabilities, rr3_pin_occupancy};

/// 装载期校验族（①③④⑤⑥⑦⑧⑨⑩ 与 ⑥-e 文件半边）：逐条点名，追加序即判据序。
/// 返回 (错误清单, 引脚派生, 装配段坏件点名)——坏件点名不是 fatal（G4 该格 Degraded）。
pub(super) type Rr3BootChecks = (Vec<String>, BTreeMap<String, String>, Vec<(String, String)>);

#[allow(clippy::too_many_arguments)]
pub(super) fn rr3_boot_checks(
    profile: &Rr3Profile,
    table: &Rr3SlotTable,
    roles: &Rr3RoleTable,
    devices: &Rr3DeviceTable,
    platform: &Rr3PlatformTable,
    wiring: &crate::wiring::Rr3Wiring,
    root: &Path,
    pin_ids: &BTreeSet<String>,
) -> Rr3BootChecks {
    let mut errs: Vec<String> = Vec::new();
    errs.extend(rr3_check_slot_refs(profile, table));
    errs.extend(rr3_check_tier(profile, table));
    errs.extend(rr3_check_claims(profile));
    // 装载期校验①（增补轮六 §5，后果 = 拒）：[[pin]] 逐条校验 + host 档词形派生
    let (pin_errs, derived) = rr3_check_pins(profile, roles, table, platform);
    errs.extend(pin_errs);
    // 门 n（§8 注入路径唯一）：[hardware] 不得出现引脚侧派生键
    errs.extend(rr3_check_hw_keys(profile, &derived));
    // 装载期校验⑤-a/⑤-b/⑧-b（增补轮六 §5，后果 = 拒；Task 4）：方向/能力/词表
    // 三面接平台表——判据真值只有 contracts/platform/<chip>.toml 一册，不推断、
    // 不运行期回退；表在调用方已 fail-closed 装载，此处只判。
    errs.extend(rr3_pin_capabilities(profile, roles, platform));
    // 装载期校验③-a/③-b/③-c + ④-b（增补轮六 §5，后果 = 拒；Task 5）：三个资源
    // 空间各自不双占 + 平台 conflicts 反推互斥（组名取自平台表，漏声明即红）。
    errs.extend(rr3_pin_occupancy(profile, platform, devices));
    // 装载期校验⑥（增补轮六 §5，后果 = 拒）：有轨、轨在册、绑定归一。
    // ⑥-a pins.rail ∈ supplies.domain.names；⑥-b kind=power 角色必有在册轨
    // （"none" 只对 signal 侧控制线合法）；⑥-c armed=true 必有测点且龄期上界 > 0
    // （无龄期快照不得充当拒动依据，§19 Y-4）；⑥-d 域的三个绑定引用位写 pin_id
    // 且能在 [pins] 取到（F-2：第二套语法一条校验都打不到）。
    let domain_names: BTreeSet<String> = profile
        .supplies
        .domains
        .iter()
        .map(|d| d.name.clone())
        .collect();
    errs.extend(rr3_check_rails(profile, roles, &domain_names));
    errs.extend(rr3_check_domains(profile));
    // 装载期校验⑦（增补轮六 §5，后果 = 拒）：接线闭合与额定加总。⑦-a wire.signal
    // ⊆ pin_id ∪ {vcc,gnd}；⑦-b wire.net ⊆ domains；⑦-d wire.mate ∈ devices.toml
    // 的 mate 集；闭合面（feeds 位号集）在接线档装载时已判。额定加总两位（§22
    // 实-4 拆分）装载期均不预支：⑦-c1（器件侧，判据对象 = devices.toml peak_ma）
    // 的自检时点判已随 assembly.rs 额定加总族先行（批N5 件五）；⑦-c2（件侧）的
    // 判据对象是件 demands 申报（n1-7 才立数据面），批N2 同链补。
    let mate_set = devices.rr3_dev_mates();
    errs.extend(wiring.rr3_wiring_check(pin_ids, &domain_names, &mate_set));

    // 门 c 装载执法（批N2 本体 ⑨-c）：装配扫描一次两用——形状/注入缺错误进
    // assemble_broken（G4 该格 Degraded，不 fatal），校验⑨⑩ 的预算判据进 errs
    // （boot 拒）。如实注记：assemble.toml 坏形的件无法供 demands facts（坏形已进
    // assemble_broken 点名），预算加总不含它——洞的可见面 = Degraded 点名。
    let (scanned, assemble_broken) = rr3_scan_modules(root, profile);
    errs.extend(rr3_check_budgets(profile, roles, &scanned));
    // 装载期校验⑥-e 文件半边（批N21 件二）：armed=true 域的证据引用四道核
    // （存在/frontmatter/form 一致；形制与成对形状半边已在 parse 层与 ⑥ 校验段拒）。
    // 证据容器根复用 modules-root 同一条 <root>/profiles/ 布局纪律 → <root>/docs/evidence；
    // 装配单不在仓布局时容器缺＋有武装申报即红（不静默放行），无武装申报（evidence
    // 恒 "none"）零波及。
    errs.extend(rr3_evidence_check(&root.join("docs/evidence"), profile));
    (errs, derived, assemble_broken)
}

/// 装载期校验⑥-e 文件半边（批N21 件二，D11 界面门的装载期落点）：对 profile 中
/// 全部 `armed = true` 域的 evidence 引用跑
/// [`rr_ipc::evidence::rr3_evidence_verify`](rr_ipc::evidence::rr3_evidence_verify)
/// 四道核（形制 → 文件存在 → frontmatter 六键 → form 与目录一致；形制半边已在
/// [`crate::profile::rr3_profile_parse`] 的 ⑥-e 拒，此处只补需要文件系统的后三道）。`root` =
/// 证据容器根（仓内上下文为 `docs/evidence`）。armed=false 域零波及（evidence 恒为
/// 显式 `"none"`，形状半边已保证成对）。返回逐条点名文案，调用方 fail-closed
/// （任一条即拒装载，无豁免通道——§6.1 校验纪律）。
pub fn rr3_evidence_check(root: &Path, profile: &Rr3Profile) -> Vec<String> {
    let mut errs = Vec::new();
    for d in &profile.supplies.domains {
        rr3_armed_ev(d, root, &mut errs);
    }
    errs
}

/// 单域证据门：未武装域零波及，武装域引用须过四道核。
fn rr3_armed_ev(d: &Rr3Domain, root: &Path, errs: &mut Vec<String>) {
    if !d.armed {
        return;
    }
    if let Err(e) = rr_ipc::evidence::rr3_evidence_verify(root, &d.evidence) {
        errs.push(format!(
            "supplies.domains[{}] armed = true 的证据引用核验不过（校验⑥-e）: {e}",
            d.name
        ));
    }
}

/// 扫描单个模块目录的装配清单（G4/ADR-0019 修订 1 ③）：`atoms/<件>/` 必有
/// assemble.toml（五件套④），形状校验走 rr-registry 共用真值；ops 非空时对模块
/// config.toml 做注入缺点路径游走。无 atoms/ 目录的模块（夹具树/两件式形态）返回空。
/// 门 c（⑨-c）起同时返回解析成功的 facts（(件名, facts) 升序）供校验⑨ 消费——
/// 形状错误照旧走错误串（该件 Degraded），坏形件的 demands 不进预算加总（可见面 =
/// assemble_broken 点名，不 claim 全覆盖）。
pub(crate) fn rr3_assemble_scan(
    module_dir: &Path,
) -> (Vec<(String, Rr3AssembleFacts)>, Vec<String>) {
    let atoms_dir = module_dir.join("atoms");
    let Ok(entries) = std::fs::read_dir(&atoms_dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();
    let mut facts_out: Vec<(String, Rr3AssembleFacts)> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for atom in names {
        if let Some(one) = rr3_atom_scan(module_dir, &atoms_dir, &atom, &mut out) {
            facts_out.push(one);
        }
    }
    (facts_out, out)
}

/// 单件装配清单扫描（五件套④）：缺件/坏形只点名不回，可进预算加总的件原样返回。
fn rr3_atom_scan(
    module_dir: &Path,
    atoms_dir: &Path,
    atom: &str,
    out: &mut Vec<String>,
) -> Option<(String, Rr3AssembleFacts)> {
    let path = atoms_dir.join(atom).join("assemble.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        out.push(format!("件 {atom} 缺 assemble.toml（五件套④，装配段点名）"));
        return None;
    };
    let Ok(facts) =
        rr_registry::rr3_assemble_parse(&text).map_err(|errs| rr3_atom_bad(atom, errs, out))
    else {
        return None;
    };
    if !facts.ops.is_empty() {
        rr3_atom_inject(module_dir, atom, &facts, out);
    }
    Some((atom.to_string(), facts))
}

/// 装配清单坏形逐条点名（Err 仅作信号，故返回 ()）。
fn rr3_atom_bad(atom: &str, errs: Vec<String>, out: &mut Vec<String>) {
    for e in errs {
        out.push(format!("件 {atom} 装配清单坏形: {e}"));
    }
}

/// ops 注入缺点路径游走：声明的参数槽必须能从模块 config.toml 按点路径取到。
fn rr3_atom_inject(module_dir: &Path, atom: &str, facts: &Rr3AssembleFacts, out: &mut Vec<String>) {
    match std::fs::read_to_string(module_dir.join("config.toml")) {
        Ok(cfg) => rr3_atom_gaps(atom, facts, &cfg, out),
        Err(e) => out.push(format!(
            "件 {atom} 参数槽来源不可证：config.toml 读不到 ({e})"
        )),
    }
}

/// 注入缺点路径游走（config 可读的那一半）：逐条缺点路径点名。
fn rr3_atom_gaps(atom: &str, facts: &Rr3AssembleFacts, cfg: &str, out: &mut Vec<String>) {
    for gap in rr_registry::rr3_injection_gaps(facts, cfg) {
        out.push(format!("件 {atom} {gap}"));
    }
}

/// 校验⑨ 判据实体（批N2 首件基-36 落、门 c ⑨-c 接链，全案 §5 判据⑨）：
/// `Σ demands.peak_ma (critical=true) ≤ domains[rail].current_cap_ma`。fail-closed
/// 逐条点名：
/// - demand.rail 不在 [[supplies.domain]] ⇒ 红（域缺 = `current_cap_ma` 缺，预算无分母）；
/// - per rail 扩算后最坏并发 Σ 超域 cap ⇒ 红点名（域 + 最坏值 + cap + 申报者清单）。
///
/// 加总口径（F-12）：**缺省 = 全部 critical=true 同时（最坏）**；收窄只认数据——
/// `concurrency_group` **恰一组**归属的条目同组互斥（至多一个同时取流）⇒ 组内取
/// max；显式空数组（不分组）与**多组**归属按不收窄计（宁超算不漏算），不许运行期
/// 推断。收窄是申报者的自证声明，判据只认数据不验物理——错报收窄即假绿，归申报者。
/// `critical=false` 不进 Σ；输入条目应已经 [`rr3_demand_expand`] 按角色
/// 实例数扩算（每实例语义，assemble.rs 头注同源）。
/// 域预算超限点名（校验⑨）：最坏并发与申报者一并给出。
fn rr3_budget_over(dm: &Rr3Domain, worst: i64, owners: &BTreeSet<&str>) -> String {
    format!(
        "域 {} 电流预算超限（校验⑨）：扩算后 Σ demands.peak_ma (critical=true) 最坏并发 = {worst} mA > current_cap_ma = {} mA（申报者：{}；缺省 = 全部同时，显式 concurrency_group 按组互斥取 max——F-12 不重叠必须是数据）",
        dm.name,
        dm.current_cap_ma,
        owners.iter().copied().collect::<Vec<_>>().join("、")
    )
}

pub(crate) fn rr3_demand_budget(
    declared: &[(String, Vec<Rr3AssembleDemand>)],
    domains: &[Rr3Domain],
) -> Vec<String> {
    let mut errs = Vec::new();
    rr3_rail_known(declared, domains, &mut errs);
    // per rail 预算加总：不分组/多组归属条目全额进最坏值，恰一组归属的条目按组
    // 互斥取组内 max（BTreeMap 保确定性，红例文案顺序稳定）
    for dm in domains {
        let (worst, owners) = rr3_rail_sum(declared, &dm.name);
        if worst > dm.current_cap_ma {
            errs.push(rr3_budget_over(dm, worst, &owners));
        }
    }
    errs
}

/// 域在册性先行：rail 不在 domains ⇒ 该条需求无分母可比，逐条点名。
fn rr3_rail_known(
    declared: &[(String, Vec<Rr3AssembleDemand>)],
    domains: &[Rr3Domain],
    errs: &mut Vec<String>,
) {
    for (owner, demands) in declared {
        for d in demands {
            if rr3_rail_unlisted(d, domains) {
                errs.push(format!(
                    "件 {owner} 申报 rail = {} 不在 [[supplies.domain]] 在册域集（校验⑨：域缺即 current_cap_ma 缺，预算无分母）",
                    d.rail
                ));
            }
        }
    }
}

/// 需求申报的 rail 是否不在域在册集（缺分母）。
fn rr3_rail_unlisted(d: &Rr3AssembleDemand, domains: &[Rr3Domain]) -> bool {
    !domains.iter().any(|dm| dm.name == d.rail)
}

/// 单域最坏并发电流与申报者集：不分组/多组归属全额加总，恰一组归属按组互斥取 max。
fn rr3_rail_sum<'a>(
    declared: &'a [(String, Vec<Rr3AssembleDemand>)],
    dname: &str,
) -> (i64, BTreeSet<&'a str>) {
    let mut worst = 0i64;
    let mut owners: BTreeSet<&str> = BTreeSet::new();
    let mut groups: BTreeMap<&str, i64> = BTreeMap::new();
    let hits = declared
        .iter()
        .flat_map(|(owner, demands)| demands.iter().map(move |d| (owner.as_str(), d)))
        .filter(|(_, d)| d.critical && d.rail == dname);
    for (owner, d) in hits {
        owners.insert(owner);
        if let [g] = d.concurrency_group.as_slice() {
            let slot = groups.entry(g.as_str()).or_insert(0);
            *slot = (*slot).max(d.peak_ma);
        } else {
            worst += d.peak_ma;
        }
    }
    for peak in groups.values() {
        worst += peak;
    }
    (worst, owners)
}

/// 门 c 申报分子构建（批N2 本体 ⑨-c）：把装配扫描 facts 扩算成校验⑨ 的加总输入。
/// **每实例语义**（assemble.rs 头注同源）：demand 条目是"该件服务的每个角色实例"的
/// 需求——实例数 = [pins] 中"角色 iface ∈ 件 handles.trait 集 且 角色 slot 绑到
/// 本模块"的 pin 数（归因链 pin→role→iface→件 handles）；归因打空的地板 = 1（模块
/// 启用件即运行，取流不因归因失配而消失，宁超算不漏算）；同模块多件声明同一 trait
/// 时各按全额计（互相不知情，fail-closed 向红）。owner = "模块/件"（红例点名到件）。
pub(crate) fn rr3_demand_expand(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    scanned: &[(String, Vec<(String, Rr3AssembleFacts)>)],
) -> Vec<(String, Vec<Rr3AssembleDemand>)> {
    let mut out = Vec::new();
    for (module, atoms) in scanned {
        for (atom, facts) in atoms {
            if facts.demands.is_empty() {
                continue;
            }
            let traits: BTreeSet<&str> = facts
                .handles
                .iter()
                .map(|h| h.trait_name.as_str())
                .collect();
            let n = rr3_atom_instances(profile, roles, &traits, module).max(1);
            let expanded: Vec<Rr3AssembleDemand> = facts
                .demands
                .iter()
                .map(|d| {
                    let mut e = d.clone();
                    e.peak_ma *= n;
                    e.sustained_ma *= n;
                    e
                })
                .collect();
            out.push((format!("{module}/{atom}"), expanded));
        }
    }
    out
}

/// 件的角色实例数：pin 的角色 iface ∈ traits 且该角色 slot 绑到本模块即计一。
fn rr3_atom_instances(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    traits: &BTreeSet<&str>,
    module: &str,
) -> i64 {
    profile
        .pins
        .iter()
        .filter(|pin| {
            let served = roles
                .rr3_role_iface(&pin.role)
                .is_some_and(|i| traits.contains(i.as_str()));
            let bound = roles
                .rr3_role_slot(&pin.role)
                .and_then(|s| profile.bindings.get(&s))
                .is_some_and(|m| m == module);
            served && bound
        })
        .count() as i64
}

/// power 角色的有效挂轨；非 power 或轨缺失/`none` 返回 None（⑥-b 已点名，不双报）。
fn rr3_power_rail<'a>(pin: &'a Rr3PinEntry, roles: &Rr3RoleTable) -> Option<&'a str> {
    if !rr3_power_kind(pin, roles) {
        return None;
    }
    if rr3_rail_unset(pin) {
        return None;
    }
    Some(pin.rail.as_str())
}

/// 挂轨缺失/显式 `none` 谓词（⑥-b 已点名，不双报）。
fn rr3_rail_unset(pin: &Rr3PinEntry) -> bool {
    pin.rail.is_empty() || pin.rail == "none"
}

/// 角色 kind = power 谓词（⑥-b 的有效挂轨前件）。
fn rr3_power_kind(pin: &Rr3PinEntry, roles: &Rr3RoleTable) -> bool {
    roles.rr3_role_kind(&pin.role).as_deref() == Some("power")
}

/// 门 c 覆盖判据（校验⑨ 缺失语义另一半，§5 ⑨ 行"件未申报 demands 而角色
/// kind=power → 红"）：角色 kind=power 的每个 pin，其绑定模块的件申报面必须覆盖
/// 该 pin 的 rail（≥1 条 rail 相同的申报，跨件合计）。绑定缺失由缺格/结构自洽判
/// 点名，此处不重复报；rail 空/none 由校验⑥-b 点名，此处跳过不双报。
/// 该模块的件申报面是否覆盖 rail（跨件合计，≥1 条即算）。
fn rr3_pow_covered(
    scanned: &[(String, Vec<(String, Rr3AssembleFacts)>)],
    module: &str,
    rail: &str,
) -> bool {
    scanned
        .iter()
        .filter(|(m, _)| m.as_str() == module)
        .flat_map(|(_, atoms)| atoms.iter())
        .flat_map(|(_, f)| f.demands.iter())
        .any(|d| d.rail == rail)
}

pub(crate) fn rr3_power_gaps(
    profile: &Rr3Profile,
    roles: &Rr3RoleTable,
    scanned: &[(String, Vec<(String, Rr3AssembleFacts)>)],
) -> Vec<String> {
    let mut errs = Vec::new();
    for pin in &profile.pins {
        let Some(rail) = rr3_power_rail(pin, roles) else {
            continue; // 非 power / ⑥-b 已点名功率面缺轨，不双报
        };
        let Some(module) = roles
            .rr3_role_slot(&pin.role)
            .and_then(|s| profile.bindings.get(&s))
        else {
            continue;
        };
        let covered = rr3_pow_covered(scanned, module, rail);
        if !covered {
            errs.push(format!(
                "pins[{}] 角色 {} kind = power 挂轨 {rail}，但模块 {module} 的件申报面无 {rail} 需求（校验⑨ 缺失语义：件未申报 demands 而角色 kind=power → 红）",
                pin.rr3_pin_id(),
                pin.role,
                rail = rail
            ));
        }
    }
    errs
}

/// 校验⑩ 判据实体（批N2 n2c-v10，全案 §5 判据⑩ / F-13 / ADR-0021 修订 3）：
/// - ⑩-a `Σ task_stack_bytes ≤ [supplies.budget].task_stack_budget_bytes`
/// - ⑩-b `Σ heap_peak_bytes ≤ heap_total_bytes − heap_reserved_bytes`（可用堆 =
///   平台堆 − 常驻——F-13：UART2_TX_RING/AUDIO_Q_CAP/渲染缓冲/批N4 板侧配置
///   全是堆账，只加总栈会击穿半边）
///
/// `not_applicable = true`（host 档显式不适用）⇒ 绿（parse 层分档自洽已保证 host
/// 档 not_applicable 必真且三数键不并存）。fail-closed 逐条点名：栈超与堆超各一条、
/// 各自点名判据号与两侧数值——F-13 形状（堆值不计栈）下只红 ⑩-b 不级联 ⑩-a。
/// 分子是**件级直加**非每实例扩算（assemble.rs 头注同源：件全部实例共享一个任务
/// 栈/一条堆账）；输入由装载链对扫描成功的件直加，坏形件不进加总（可见面 =
/// assemble_broken 点名，不 claim 全覆盖，同校验⑨ 诚实注记）。
pub(crate) fn rr3_memory_budget(
    stack_sum: i64,
    heap_sum: i64,
    budget: &Rr3Budget,
    tier: &str,
) -> Vec<String> {
    let mut errs = Vec::new();
    if budget.not_applicable {
        return errs;
    }
    rr3_budget_cmp(stack_sum, heap_sum, budget, tier, &mut errs);
    errs
}

fn rr3_budget_cmp(
    stack_sum: i64,
    heap_sum: i64,
    budget: &Rr3Budget,
    tier: &str,
    errs: &mut Vec<String>,
) {
    if stack_sum > budget.task_stack_budget_bytes {
        errs.push(format!(
            "内存预算超限（校验⑩-a，{tier} 档）：Σ task_stack_bytes = {stack_sum} B > task_stack_budget_bytes = {} B（分子为件级直加非每实例扩算，分母住 [supplies].budget——ADR-0021 修订 3 三份数据三个主人）",
            budget.task_stack_budget_bytes
        ));
    }
    let heap_available = budget.heap_total_bytes - budget.heap_reserved_bytes;
    if heap_sum > heap_available {
        errs.push(format!(
            "内存预算超限（校验⑩-b，{tier} 档）：Σ heap_peak_bytes = {heap_sum} B > 可用堆 {heap_available} B（heap_total_bytes = {} − 常驻 heap_reserved_bytes = {}；F-13：渲染缓冲/队列/板侧配置全是堆账，只加总栈会击穿半边）",
            budget.heap_total_bytes, budget.heap_reserved_bytes
        ));
    }
}
