//! 装载计划与真值集：选册、轴档案装载投影、消费真值清单与定位。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::super::model::Rr3Profile;
use super::super::parse::rr3_profile_abs;

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
/// 本函数的 `axis` 参数即该 env 的读数（env 读取在 [`crate::profile::rr3_boot_inputs`]
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
    let text = rr3_axis_pick(found, axis, &mut notes);
    (text, notes)
}

/// 按轴读数消歧投影：恰一份合法才出注入文本，零份/多份空串并点名。
fn rr3_axis_pick(
    found: &[crate::axis::Rr3AxisFile],
    axis: &str,
    notes: &mut Vec<String>,
) -> String {
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
            String::new()
        }
        [one] => {
            let p = &one.profile;
            format!(
                "calibrated={},hard_stop={},lo={},hi={},center={}",
                p.calibrated,
                p.hard_stop.rr3_hard_stop_name(),
                p.lo_deg,
                p.hi_deg,
                p.center_deg
            )
        }
        many => {
            notes.push(format!(
                "instance {axis:?} 命中 {} 份合法档案（同 instance 异 pin，装配单形貌异常）——不注入执行轴状态，模块按保守缺省起跑",
                many.len()
            ));
            String::new()
        }
    }
}

/// 平台事实表文件名（选册格式化单点）：清单成员与 boot 选册（
/// [`crate::profile::rr3_boot_with_probe`] 的 `platform_dir` join）两处共用，防字面漂移。
pub(super) fn rr3_platform_file(chip: &str) -> String {
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
    let Some(stem) = abs.file_stem().and_then(std::ffi::OsStr::to_str) else {
        return Err(format!("装配单 {} 无文件名", abs.display()));
    };
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
