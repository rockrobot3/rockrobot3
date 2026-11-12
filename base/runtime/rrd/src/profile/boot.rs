//! 装配单装载链（B9 结构收口）：选册 → 查册 → 探测 → 校验 → boot 输入。
//!
//! 全部发生在模块起之前（ADR-0012 裁定 4）。真值集/选册（[`rr3_truth_set`]、
//! [`rr3_truth_locations`]）、boot 入口（[`rr3_boot_with_probe`]）、五册装载
//! 小件、装配扫描与预算判据同址；单条判据体归 `profile_checks.rs`。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rr_registry::{
    rr3_dev_parse, rr3_plat_parse, rr3_role_parse, rr3_slot_parse, Rr3AssembleDemand,
    Rr3AssembleFacts, Rr3DeviceTable, Rr3PlatformTable, Rr3RoleTable, Rr3SlotTable,
};

use super::model::*;
use super::parse::{rr3_profile_abs, rr3_profile_parse};
use crate::profile_checks::{
    rr3_axis_load, rr3_check_budgets, rr3_check_claims, rr3_check_domains, rr3_check_hw_keys,
    rr3_check_pins, rr3_check_rails, rr3_check_slot_refs, rr3_check_tier, rr3_hal_bindings,
    rr3_manifests, rr3_role_gaps, rr3_scan_modules, rr3_slot_gaps,
};
use crate::profile_pins::{rr3_pin_capabilities, rr3_pin_occupancy};

/// [hardware] 段序列化成注入文本：每行 `键 = "值"`（值已字符串化，TOML 基本
/// 字符串转义只涉及反斜杠与双引号）。BTreeMap 升序 = 确定性；空表 → 空串
/// （调用方见空不注入，模块侧维持"无描述符"状态）。
pub(crate) fn rr3_bindings_text(hardware: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (k, v) in hardware {
        out.push_str(&format!(
            "{k} = \"{}\"\n",
            v.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    out
}

/// 轴档案全量装载（批N4 件九笔4 自 [`rr3_axis_state_text`] 拆出，零行为漂移）：
/// 扫 `axis_dir`/*.toml（升序 = 确定性），全部合法装载（件二
/// [`crate::axis::rr3_act_profile_load`]，外键 = [[pin]] 的 pin_id 集——与档案
/// `pin` 键同一语法，F-2：第二套语法一条校验都打不到），返回 (合法档案件,
/// 坏件逐条点名)。目录读不到同零份（无 axis 目录 = 无档案）。拆出理由：
/// 装配快照要的是**全量**合法档案件（面板出图要看见全部轴），投影函数只返回
/// 消歧后的 (state_text, notes)、清单被丢弃——装载与投影两段各自可测可复用。
/// axis 目录下的 `.toml` 档路径（读不到目录 = 零份），排序保确定性。
fn rr3_axis_paths(axis_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(axis_dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
        .collect();
    files.sort();
    files
}

pub(crate) fn rr3_axis_files_load(
    axis_dir: &Path,
    pin_ids: &BTreeSet<String>,
) -> (Vec<crate::axis::Rr3AxisFile>, Vec<String>) {
    let mut notes = Vec::new();
    let files = rr3_axis_paths(axis_dir);
    let mut found = Vec::new();
    for f in &files {
        rr3_axis_file_take(f, pin_ids, &mut found, &mut notes);
    }
    (found, notes)
}

/// 单份轴档归置：合法进 found，坏形逐条折进 notes（无档案是常态，不点名）。
fn rr3_axis_file_take(
    f: &Path,
    pin_ids: &BTreeSet<String>,
    found: &mut Vec<crate::axis::Rr3AxisFile>,
    notes: &mut Vec<String>,
) {
    match crate::axis::rr3_act_profile_load(f, pin_ids) {
        Ok(Some(file)) => found.push(file),
        Ok(None) => {}
        Err(errs) => notes.extend(
            errs.into_iter()
                .map(|e| format!("轴档案 {} 坏形: {e}", f.display())),
        ),
    }
}

/// 执行轴状态投影（批N3 件三建，件五收口消歧；批N4 件九笔4 起吃
/// [`rr3_axis_files_load`] 的装载结果做纯投影——boot 装载一次，投影与快照共用
/// 同一份清单，不重读目录造第二真值点）。
///
/// 消歧裁决（件五，收"多轴未消歧"挂账）：模块面是单轴 trait——rr-hal 桥后端按
/// `RR3_HAL_PERIPH_AXIS` env（缺省 [`rr_hal_api::RR3_PERIPH_AXIS_DEFAULT`]）选轴；
/// 本函数的 `axis` 参数即该 env 的读数（env 读取在 [`rr3_boot_inputs`]
/// 调用点，本函数吃值不吃全局态——cli.rs 先例，测试不碰进程环境），选 pin 末段
/// （instance）与之**相等**的恰一份合法档案投影——注入的行程真值必须属于被驱动
/// 的那根轴。单份也对轴：env 指了 tilt 而档案只有 pan 时，"注入 pan 行程、驱动
/// tilt 轴"是错配，宁可空串折保守缺省。同 pin 多份文件（副本）不是消歧对象是
/// 病灶——投影空串并点名（恰一份合法才投影的 boot 链防线，写侧同款拒面在
/// axis_write 件四）。
///
/// 投影文本 = `calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>`（件五
/// 扩形：行程三键随状态同路注入，模块侧行程改读档案——门 g"档案之外的行程
/// 读者数为 0"的 host 侧半边）。零份回空串且安静（无档案是常态起跑态）；坏件
/// 逐条点名但不拒 boot（档案装载是降级面，与缺格/缺角色同档——模块侧折保守
/// 缺省仍是安全向）；多份合法点名后同样空串。
pub(crate) fn rr3_axis_state_text(
    found: &[crate::axis::Rr3AxisFile],
    axis: &str,
) -> (String, Vec<String>) {
    let mut notes = Vec::new();
    // 副本病灶：同 pin 多份文件 → 不注入（恰一份合法才投影；先清理副本）
    let mut pin_counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for f in found {
        *pin_counts.entry(f.pin.as_str()).or_insert(0) += 1;
    }
    if let Some((pin, n)) = pin_counts.into_iter().find(|(_, n)| *n > 1) {
        notes.push(format!(
            "pin {pin} 有 {n} 份合法档案（副本）——恰一份合法才投影，先清理副本"
        ));
        return (String::new(), notes);
    }
    // 轴消歧（件五）：instance（pin 末段）与轴读数相等的恰一份才投影
    let selected: Vec<&crate::axis::Rr3AxisFile> = found
        .iter()
        .filter(|f| f.pin.rsplit('.').next() == Some(axis))
        .collect();
    match selected.as_slice() {
        [] => {
            if !found.is_empty() {
                notes.push(format!(
                    "轴档案合法装载 {} 份，但无一 instance = {axis:?}（{}/缺省）——不注入执行轴状态，模块按保守缺省起跑",
                    found.len(),
                    rr_hal_api::RR3_PERIPH_AXIS_ENV
                ));
            }
            (String::new(), notes)
        }
        [one] => {
            let p = &one.profile;
            (
                format!(
                    "calibrated={},hard_stop={},lo={},hi={},center={}",
                    p.calibrated,
                    p.hard_stop.rr3_hard_stop_name(),
                    p.lo_deg,
                    p.hi_deg,
                    p.center_deg
                ),
                notes,
            )
        }
        many => {
            notes.push(format!(
                "instance {axis:?} 命中 {} 份合法档案（同 instance 异 pin，装配单形貌异常）——不注入执行轴状态，模块按保守缺省起跑",
                many.len()
            ));
            (String::new(), notes)
        }
    }
}

/// 平台事实表文件名（选册格式化单点）：清单成员与 boot 选册（
/// [`rr3_boot_with_probe`] 的 `platform_dir` join）两处共用，防字面漂移。
fn rr3_platform_file(chip: &str) -> String {
    format!("{chip}.toml")
}

/// 契约四册名单（基-40：字面唯一份）。全仓"契约四册文件名"只在这里写字面——
/// 装载真值集（[`rr3_truth_set`]）、contracts.rs 的 `RR3_CONTRACT_FILES`、
/// 打包/装机等消费方一律引用本常量；新增/改名册只改这里，成员判等（基-40 单测
/// ＋规约 ⑨-z 门）随之生效。顺序＝topics/slots/roles/devices（Z-4 记账点名序）。
#[rustfmt::skip]
pub const RR3_CONTRACT_BOOKS: [&str; 4] = ["topics.toml", "slots.toml", "roles.toml", "devices.toml"];

/// 装载消费真值集清单（增补轮六 基-25／实-12：全仓唯一一份，三处消费方判等——
/// `rr3-pack` 打包 / buildroot `post-build.sh` 装机 / OTA 模块级更新）。清单由装载面
/// 定义，打包与装机面不得各写一份文件清单——装载新增册时只改这里，消费方判等随之
/// 生效（块-15 的病根就是打包面手抄四册、装载面加了必需册而打包面不知道）。
/// 成员判据＝rrd 装载期读的文件：装配单本体（`--profile`）、契约四册（topics/slots/
/// roles/devices）、平台事实表（按 `[platform].chip` 选册）、机型接线档；目录成员＝
/// 轴档案目录（在则整目录随装、缺目录＝零份合法，与 [`rr3_axis_files_load`] 同语义）。
/// 装载期不读的契约件（envelope/golden/face-expressions 等）不在清单——成员判据写死
/// 为"装载期读"，不随"契约在册数"漂移。
/// 输出相对路径以仓根／装机安装根为基准（两侧同名同布局，消费方逐条 join 即落位）。
#[derive(Debug, PartialEq, Eq)]
pub struct Rr3RuntimeTruthSet {
    /// 必需逐文件成员：缺任一册 rrd 装载即拒 boot，打包面缺任一册必须拒出包。
    pub files: Vec<String>,
    /// 可选目录成员：目录在则整目录随装（升序确定性由消费方保证），不在不红。
    pub dirs: Vec<String>,
}

/// 真值集成员名生成单点（基-40：相对名唯一生成处）。相对名以仓根／装机安装根为
/// 基准（包内布局口径），`rr3_truth_set`（装载面清单）与
/// [`rr3_truth_locations`]（boot 记账定位）都消费它——成员名两处各自
/// 手写即漂移温床，收在这里后成员集判等只差"函数返回值相等"一条单测（⑨-z 判据）。
fn rr3_truth_names(profile_stem: &str, chip: &str) -> Vec<String> {
    let mut names = vec![format!("profiles/{profile_stem}.toml")];
    names.extend(RR3_CONTRACT_BOOKS.iter().map(|b| format!("contracts/{b}")));
    names.push(format!("contracts/platform/{}", rr3_platform_file(chip)));
    names.push(format!("hardware/{profile_stem}/wiring.toml"));
    names
}

/// 按装配单解析结果与机型 stem 列装载消费真值集（清单真值点，见 [`Rr3RuntimeTruthSet`]）。
pub fn rr3_truth_set(p: &Rr3Profile, profile_stem: &str) -> Rr3RuntimeTruthSet {
    Rr3RuntimeTruthSet {
        files: rr3_truth_names(profile_stem, &p.platform_chip),
        dirs: vec![format!("hardware/{profile_stem}/axis")],
    }
}

/// boot 真值集记账行：成员相对名（[`rr3_truth_names`] 生成，包内布局
/// 口径）+ boot 实际定位路径（本机口径，自检复演重读的就是它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3TruthSetLocation {
    /// 相对成员名（如 `contracts/topics.toml`；ctl 应答 truth_set 段点名用）
    pub member: String,
    /// 本机定位路径（boot 臂输入映射产物；`--profile` 任意路径、`--contracts`
    /// 运行期落点都在这里如实记账，不假设包内布局）
    pub path: PathBuf,
}

/// boot 臂真值集定位派生（基-40 件一②，机制面）：按成员名单点生成七条相对名，
/// 再按 boot 臂输入映射本机实际路径——装配单本体 = `--profile` 定位、契约四册与
/// 平台事实表 = `--contracts` join、接线档 = 接线布局推导产物。stem 提取与
/// [`crate::wiring::rr3_wiring_path`] 同法（canonicalize 后取
/// file_stem），保证成员名 `profiles/<stem>.toml` 与 `hardware/<stem>/wiring.toml`
/// 的 stem 同源。任何一条定位失败都带因报 Err（调用点在 boot 链验收之后，正常
/// 到不了这里——失败即文件系统级异常，fail-closed 不带病记账）。
pub fn rr3_truth_locations(
    profile_path: &Path,
    contracts_dir: &Path,
    wiring_path: &Path,
    chip: &str,
) -> Result<Vec<Rr3TruthSetLocation>, String> {
    let abs = rr3_profile_abs(profile_path)?;
    let stem = abs
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| format!("装配单 {} 无文件名", abs.display()))?;
    let names = rr3_truth_names(stem, chip);
    // 与 names 同序逐条映射（成员名生成与定位映射一一对应，错位即成员集判等红）
    let paths = rr3_truth_paths(&abs, contracts_dir, wiring_path, chip);
    debug_assert_eq!(names.len(), paths.len(), "真值集成员名与定位映射错位");
    Ok(names
        .into_iter()
        .zip(paths)
        .map(|(member, path)| Rr3TruthSetLocation { member, path })
        .collect())
}

/// 真值集七成员的定位映射（与 [`rr3_truth_names`] 同序）：装配单本体 → 契约四册
/// → 平台事实表 → 接线档。
fn rr3_truth_paths(
    abs: &Path,
    contracts_dir: &Path,
    wiring_path: &Path,
    chip: &str,
) -> Vec<PathBuf> {
    [
        vec![abs.to_path_buf()],
        RR3_CONTRACT_BOOKS
            .iter()
            .map(|b| contracts_dir.join(b))
            .collect::<Vec<_>>(),
        vec![contracts_dir.join("platform").join(rr3_platform_file(chip))],
        vec![wiring_path.to_path_buf()],
    ]
    .concat()
}

/// 装配链 boot 输入装配（daemon 在 bind 之前调用，fatal 即非 0 退出、套接字不留残迹）。
/// 致命错误（读不到 / 形状不合 / 查册未命中 / 结构不自洽）打包成一条串返回；
/// 缺格**不是**致命项（P2：缺格只降点名模块，缺格≠拒 boot），进 [`Rr3ProfileBoot::missing`]；
/// 缺角色同样不是致命项（校验②后果 = 降），进 [`Rr3ProfileBoot::role_missing`]。
/// `slots_path`/`roles_path`/`devices_path`/`wiring_path` 由调用方给定（`--contracts`
/// 运行期定位或编译期仓内路径；接线档由 [`crate::wiring::rr3_wiring_path`]
/// 按 `hardware/<形态>/wiring.toml` 布局推导）——查册真值仍只有 rr-registry 那几册
/// 与接线档本体，这里只管从哪读。
pub fn rr3_boot_inputs(
    profile_path: &Path,
    slots_path: &Path,
    roles_path: &Path,
    devices_path: &Path,
    wiring_path: &Path,
    platform_dir: &Path,
) -> Result<Rr3ProfileBoot, String> {
    let probe = crate::probe::rr3_probe_default();
    rr3_boot_with_probe(
        profile_path,
        slots_path,
        roles_path,
        devices_path,
        wiring_path,
        platform_dir,
        probe.as_ref(),
    )
}

/// 同 [`rr3_boot_inputs`]，但探测件由调用方注入（MC-4 接缝）：生产走
/// [`crate::probe::rr3_probe_default`]（非 test 面按目标分流），门禁用例注入
/// 恒缺/假根探针钉"缺格 → Degraded 点名件与键"。探测是纯读面、无状态，注入
/// 不改变 boot 前的单次消费语义（ADR-0012 裁定 4）。
pub fn rr3_boot_with_probe(
    profile_path: &Path,
    slots_path: &Path,
    roles_path: &Path,
    devices_path: &Path,
    wiring_path: &Path,
    platform_dir: &Path,
    probe: &dyn crate::probe::Rr3Probe,
) -> Result<Rr3ProfileBoot, String> {
    let (abs, profile) = rr3_load_profile(profile_path)?;

    // 查册门（裁定 5②）：格位真值只有 rr-registry 那一册，CI 全集层与本入口共用。
    let table = rr3_load_slots(slots_path)?;

    // 角色契约装载（增补轮六 §4.1，校验①②判据源）：与格位契约同一 fail-closed
    // 口径——读不到 / 解析空 / 形状越界都是契约事故，拒启动（空角色表不是放行许可证）。
    let roles = rr3_load_roles(roles_path)?;

    // 器件词表装载（增补轮六 §4.1b，校验⑦-d 的 mate 集判定源）：与格位/角色契约
    // 同一 fail-closed 口径——空器件表是契约事故，不是放行许可证。装载期只查
    // mate 外键闭合，不认型号能力（D10a：addrs/baud_ok 是探测期/出厂自检判据）。
    let devices = rr3_load_devices(devices_path)?;

    // 接线档装载（增补轮六 §4.3，校验⑦ 判据面）：装机档必在——读不到/空表/
    // 形状不合/闭合面破都是装配事故，拒启动（接线闭合是校验⑦的前提，没有
    // "无线的机器"）。
    let (wiring_abs, wiring) = rr3_load_wiring(wiring_path)?;

    // 平台事实表装载（增补轮六 §4.6，批N1 基-22；校验③④⑤⑧的判据前件）：按
    // [platform].chip 在 platform_dir 下选册（daemon 传 contracts/platform/，与
    // roles/devices 同源）。fail-closed 与 roles 同口径但更严——platform.rs 解析本就
    // 回 Err 版（roles/devices 宽容回空表走"表为空"点名，那是集合词表；平台表宽容 =
    // ④⑤⑧ 静默不判，§18 F-6 同病），读不到/形状坏都逐条点名拒 boot，不降级。
    let platform = rr3_load_platform(platform_dir, &profile.platform_chip)?;

    let mut errs: Vec<String> = Vec::new();
    errs.extend(rr3_check_slot_refs(&profile, &table));
    errs.extend(rr3_check_tier(&profile, &table));
    errs.extend(rr3_check_claims(&profile));
    // 装载期校验①（增补轮六 §5，后果 = 拒）：[[pin]] 逐条校验 + host 档词形派生
    let (pin_errs, derived) = rr3_check_pins(&profile, &roles, &table, &platform);
    errs.extend(pin_errs);
    // 门 n（§8 注入路径唯一）：[hardware] 不得出现引脚侧派生键
    errs.extend(rr3_check_hw_keys(&profile, &derived));
    // 装载期校验⑤-a/⑤-b/⑧-b（增补轮六 §5，后果 = 拒；Task 4）：方向/能力/词表
    // 三面接平台表——判据真值只有 contracts/platform/<chip>.toml 一册，不推断、
    // 不运行期回退；表在上方已 fail-closed 装载，此处只判。
    errs.extend(rr3_pin_capabilities(&profile, &roles, &platform));
    // 装载期校验③-a/③-b/③-c + ④-b（增补轮六 §5，后果 = 拒；Task 5）：三个资源
    // 空间各自不双占 + 平台 conflicts 反推互斥（组名取自平台表，漏声明即红）。
    errs.extend(rr3_pin_occupancy(&profile, &platform, &devices));
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
    errs.extend(rr3_check_rails(&profile, &roles, &domain_names));
    errs.extend(rr3_check_domains(&profile));
    // 装载期校验⑦（增补轮六 §5，后果 = 拒）：接线闭合与额定加总。⑦-a wire.signal
    // ⊆ pin_id ∪ {vcc,gnd}；⑦-b wire.net ⊆ domains；⑦-d wire.mate ∈ devices.toml
    // 的 mate 集；闭合面（feeds 位号集）在接线档装载时已判。额定加总两位（§22
    // 实-4 拆分）装载期均不预支：⑦-c1（器件侧，判据对象 = devices.toml peak_ma）
    // 的自检时点判已随 assembly.rs 额定加总族先行（批N5 件五）；⑦-c2（件侧）的
    // 判据对象是件 demands 申报（n1-7 才立数据面），批N2 同链补。
    let pin_ids: BTreeSet<String> = profile.pins.iter().map(|p| p.rr3_pin_id()).collect();
    let mate_set = devices.rr3_dev_mates();
    errs.extend(wiring.rr3_wiring_check(&pin_ids, &domain_names, &mate_set));

    // 门 c 装载执法（批N2 本体 ⑨-c）：装配扫描一次两用——形状/注入缺错误进
    // assemble_broken（G4 该格 Degraded，不 fatal），校验⑨⑩ 的预算判据进 errs
    // （boot 拒）。如实注记：assemble.toml 坏形的件无法供 demands facts（坏形已进
    // assemble_broken 点名），预算加总不含它——洞的可见面 = Degraded 点名。
    let root = abs.parent().and_then(Path::parent).ok_or_else(|| {
        format!(
            "装配单 {} 无法定位 modules-root（须在 <root>/profiles/ 下）",
            abs.display()
        )
    })?;
    let (scanned, assemble_broken) = rr3_scan_modules(root, &profile);
    errs.extend(rr3_check_budgets(&profile, &roles, &scanned));
    // 装载期校验⑥-e 文件半边（批N21 件二）：armed=true 域的证据引用四道核
    // （存在/frontmatter/form 一致；形制与成对形状半边已在 parse 层与 ⑥ 校验段拒）。
    // 证据容器根复用 modules-root 同一条 <root>/profiles/ 布局纪律 → <root>/docs/evidence；
    // 装配单不在仓布局时容器缺＋有武装申报即红（不静默放行），无武装申报（evidence
    // 恒 "none"）零波及。
    errs.extend(rr3_evidence_check(&root.join("docs/evidence"), &profile));
    if !errs.is_empty() {
        return Err(errs.join("; "));
    }
    // 装载期校验②（后果 = 降不拒）：present ∩ enabled_slots 的缺角色清单
    let role_missing = rr3_role_gaps(&profile, &roles);

    // 缺格判定：enabled_slots 声明的非基础层格位没有指向本模块的有效 binding（或探测不到）。
    // 基础层恒在位，跳过（slots.toml 注记：net.online / power.battery boot 即在位、恒不降级）。
    let missing = rr3_slot_gaps(&profile, &table, &roles, probe);

    let manifests = rr3_manifests(&profile, root);
    let hal_bindings = rr3_hal_bindings(derived, &profile.hardware);
    let (axis_files, axis_state, axis_notes) = rr3_axis_load(&wiring_abs, &pin_ids);

    // G4 装配段校验（ADR-0019 修订 1 ③）已在门 c 段随装配扫描一次完成：缺文件/
    // 坏形/ops 注入缺逐条点名进 assemble_broken，该模块 boot 入 Degraded（不 fatal；
    // 修数据 + 重启是唯一解除路）。此处只消费上文的 assemble_broken。

    // 装配快照（批N4 件九笔4，`rr3.ctl.wiring` 判据源）：至此装配单/接线档/轴
    // 档案/角色表四路全部过完装载期校验（fatal 面早已逐条拒净），这是"验收通过的
    // 世界"唯一收账点——clone 进快照交给 daemon 定格 OnceLock；boot 函数自身不碰
    // 全局态（纯构造，住处由调用方拍）。快照与 boot 返回值同生命周期，装载面之后
    // 对文件的改动进不了本进程的 ctl 应答（快照语义，assembly.rs 模块头注）。
    let snapshot = crate::assembly::Rr3AssemblySnapshot {
        profile_name: abs
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string(),
        profile: profile.clone(),
        wiring: wiring.clone(),
        peak_by_mate: devices.rr3_dev_peak_by_mate(),
        axis: axis_files,
        roles: roles.clone(),
    };

    Ok(Rr3ProfileBoot {
        manifests,
        missing,
        hal_bindings,
        assemble_broken,
        role_missing,
        axis_state,
        axis_notes,
        log_ring_cap: profile.log_ring_cap,
        hal_backend: profile.hal_backend.clone(),
        audio_backend: profile.audio_backend.clone(),
        servo_backend: profile.servo_backend.clone(),
        display_backend: profile.display_backend.clone(),
        platform_chip: profile.platform_chip.clone(),
        platform_tier: profile.platform_tier.clone(),
        snapshot: Some(snapshot),
    })
}

/// 装配单装载：绝对化 → 读文本 → 解析（形状不合逐条点名，一条不静默放行）
fn rr3_load_profile(path: &Path) -> Result<(PathBuf, Rr3Profile), String> {
    let abs = rr3_profile_abs(path)?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| format!("读不到装配单 {}: {e}", abs.display()))?;
    let profile = rr3_profile_parse(&text).map_err(|errs| {
        errs.iter()
            .map(|e| format!("装配单 {} 形状不合: {e}", abs.display()))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    Ok((abs, profile))
}

/// 格位契约装载 + 空表/形状门（查册真值 = rr-registry 那一册，与 CI 全集层同源）
fn rr3_load_slots(path: &Path) -> Result<Rr3SlotTable, String> {
    let (abs, text) = rr3_boot_text(path, "格位契约")?;
    let table: Rr3SlotTable = rr3_slot_parse(&text);
    rr3_slots_gate(&abs, &table)?;
    Ok(table)
}

/// 格位契约的空表门与形状门（与 topics 装载同构：CI 全集层与运行期门两轴缺一即带病）。
fn rr3_slots_gate(abs: &Path, table: &Rr3SlotTable) -> Result<(), String> {
    if table.rr3_slot_is_empty() {
        return Err(format!(
            "格位契约 {} 解析后为空，拒绝启动（空格位表是契约事故）",
            abs.display()
        ));
    }
    let errs = table.rr3_slot_check();
    if !errs.is_empty() {
        return Err(format!(
            "格位契约 {} 形状不合，拒绝启动: {}",
            abs.display(),
            errs.join("; ")
        ));
    }
    Ok(())
}

/// 角色契约装载 + 空表/形状门（与格位契约同一 fail-closed 口径）
fn rr3_load_roles(path: &Path) -> Result<Rr3RoleTable, String> {
    let (abs, text) = rr3_boot_text(path, "角色契约")?;
    let roles: Rr3RoleTable = rr3_role_parse(&text);
    rr3_roles_gate(&abs, &roles)?;
    Ok(roles)
}

/// 角色契约的空表门与形状门（与格位契约同一 fail-closed 口径）。
fn rr3_roles_gate(abs: &Path, roles: &Rr3RoleTable) -> Result<(), String> {
    if roles.rr3_role_is_empty() {
        return Err(format!(
            "角色契约 {} 解析后为空，拒绝启动（空角色表是契约事故）",
            abs.display()
        ));
    }
    let errs = roles.rr3_role_check();
    if !errs.is_empty() {
        return Err(format!(
            "角色契约 {} 形状不合，拒绝启动: {}",
            abs.display(),
            errs.join("; ")
        ));
    }
    Ok(())
}

/// 器件词表装载 + 空表/形状门（装载期只查 mate 外键闭合，不认型号能力，D10a）
fn rr3_load_devices(path: &Path) -> Result<Rr3DeviceTable, String> {
    let (abs, text) = rr3_boot_text(path, "器件词表")?;
    let devices: Rr3DeviceTable = rr3_dev_parse(&text);
    rr3_devs_gate(&abs, &devices)?;
    Ok(devices)
}

/// 契约册装载前两步：绝对化 + 读文本（两处失败同一句"读不到{what}"）。
fn rr3_boot_text(path: &Path, what: &str) -> Result<(PathBuf, String), String> {
    let abs =
        std::fs::canonicalize(path).map_err(|e| format!("读不到{what} {}: {e}", path.display()))?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| format!("读不到{what} {}: {e}", abs.display()))?;
    Ok((abs, text))
}

/// 器件词表的空表门与形状门（与格位/角色同一 fail-closed 口径）。
fn rr3_devs_gate(abs: &Path, devices: &Rr3DeviceTable) -> Result<(), String> {
    if devices.rr3_dev_is_empty() {
        return Err(format!(
            "器件词表 {} 解析后为空，拒绝启动（空器件表是契约事故）",
            abs.display()
        ));
    }
    let errs = devices.rr3_dev_check();
    if !errs.is_empty() {
        return Err(format!(
            "器件词表 {} 形状不合，拒绝启动: {}",
            abs.display(),
            errs.join("; ")
        ));
    }
    Ok(())
}

/// 接线档装载（装机档必在：读不到/形状不合都拒启动；返回绝对路径供轴目录推导）
fn rr3_load_wiring(path: &Path) -> Result<(PathBuf, crate::wiring::Rr3Wiring), String> {
    let abs =
        std::fs::canonicalize(path).map_err(|e| format!("读不到接线档 {}: {e}", path.display()))?;
    let text = std::fs::read_to_string(&abs)
        .map_err(|e| format!("读不到接线档 {}: {e}", abs.display()))?;
    let wiring = crate::wiring::rr3_wiring_from_text(&text).map_err(|errs| {
        format!(
            "接线档 {} 形状不合，拒绝启动: {}",
            abs.display(),
            errs.join("; ")
        )
    })?;
    Ok((abs, wiring))
}

/// 平台事实表装载：按 [platform].chip 选册，读不到/形状坏逐条点名拒 boot（不降级）
fn rr3_load_platform(dir: &Path, chip: &str) -> Result<Rr3PlatformTable, String> {
    let path = dir.join(rr3_platform_file(chip));
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "读不到平台事实表 {}（[platform].chip = {chip} 的选册）: {e}",
            path.display()
        )
    })?;
    rr3_plat_parse(&text).map_err(|errs| {
        format!(
            "平台事实表 {} 形状不合，拒绝启动: {}",
            path.display(),
            errs.join("; ")
        )
    })
}

/// 装载期校验⑥-e 文件半边（批N21 件二，D11 界面门的装载期落点）：对 profile 中
/// 全部 `armed = true` 域的 evidence 引用跑
/// [`rr_ipc::evidence::rr3_evidence_verify`](rr_ipc::evidence::rr3_evidence_verify)
/// 四道核（形制 → 文件存在 → frontmatter 六键 → form 与目录一致；形制半边已在
/// [`rr3_profile_parse`] 的 ⑥-e 拒，此处只补需要文件系统的后三道）。`root` =
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

// ── TOML 键行手术面（批N22 件一；批N44 起机制迁 [`crate::profile_edit`]）──────
// 本文件只留原路径转出（写面调用与文档锚点不变），手术判据与实现单点在
// `profile_edit`——超长文件按"装载 / 校验 / 手术"三分的首刀。
pub use crate::profile_edit::{rr3_domain_apply, rr3_key_line_apply};

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
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            out.push(format!("件 {atom} 缺 assemble.toml（五件套④，装配段点名）"));
            return None;
        }
    };
    let facts = match rr_registry::rr3_assemble_parse(&text) {
        Ok(f) => f,
        Err(errs) => {
            for e in errs {
                out.push(format!("件 {atom} 装配清单坏形: {e}"));
            }
            return None;
        }
    };
    if !facts.ops.is_empty() {
        rr3_atom_inject(module_dir, atom, &facts, out);
    }
    Some((atom.to_string(), facts))
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
    for (owner, demands) in declared {
        for d in demands.iter().filter(|d| d.critical && d.rail == dname) {
            owners.insert(owner.as_str());
            match d.concurrency_group.as_slice() {
                [g] => {
                    let slot = groups.entry(g.as_str()).or_insert(0);
                    *slot = (*slot).max(d.peak_ma);
                }
                _ => worst += d.peak_ma,
            }
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
            let mut instances: i64 = 0;
            for pin in &profile.pins {
                let served = roles
                    .rr3_role_iface(&pin.role)
                    .is_some_and(|i| traits.contains(i.as_str()));
                let bound = roles
                    .rr3_role_slot(&pin.role)
                    .and_then(|s| profile.bindings.get(&s))
                    .is_some_and(|m| m == module);
                if served && bound {
                    instances += 1;
                }
            }
            let n = instances.max(1);
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
