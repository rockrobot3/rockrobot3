//! 真值集记账推导出的写口路径集与槽位查册。

use super::*;

/// 写口路径集（真值集记账推导，零新增全局）：记账成员判据 = 成员相对名（基-40
/// 成员判据 = "装载期读的文件"——写口的读与写打同一批文件，路径与 boot 记账
/// 同源即无第二真值点）。任一必需成员缺 = 记账形状破，拒（fail-closed）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3DomainWritePaths {
    /// 装配单（`profiles/<stem>.toml` 记账成员）
    pub profile: PathBuf,
    pub slots: PathBuf,
    pub roles: PathBuf,
    pub devices: PathBuf,
    /// 机型接线档（`hardware/<stem>/wiring.toml` 记账成员；axis 落点的目录锚）
    pub wiring: PathBuf,
    /// 平台事实表目录（`contracts/platform/<chip>.toml` 记账成员的父目录）
    pub platform_dir: PathBuf,
}

/// 记账成员相对名 → 写口路径位（基-40 记账七成员里写口要用的六路）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rr3PathSlot {
    Profile,
    Slots,
    Roles,
    Devices,
    Wiring,
    Platform,
}

/// 成员分类（纯函数）：不认识 = None（topics.toml 等非写口成员不取）。
/// 分类单点在这里，主循环只做"放位"，成员口径变化只改这一处。
fn rr3_path_slot(member: &str) -> Option<Rr3PathSlot> {
    // 前缀/后缀成对判据（表驱动：成员口径变化只改这张表）
    let rules = [
        ("profiles/", ".toml", Rr3PathSlot::Profile),
        ("hardware/", "/wiring.toml", Rr3PathSlot::Wiring),
    ];
    for (prefix, suffix, slot) in rules {
        if member.starts_with(prefix) && member.ends_with(suffix) {
            return Some(slot);
        }
    }
    let Some(rest) = member.strip_prefix("contracts/platform/") else {
        return rr3_slot_book(member, &crate::profile::RR3_CONTRACT_BOOKS);
    };
    rest.ends_with(".toml").then_some(Rr3PathSlot::Platform)
}

/// 契约三册（slots/roles/devices）成员 → 写口槽位。
fn rr3_slot_book(member: &str, books: &[&str; 4]) -> Option<Rr3PathSlot> {
    for (idx, slot) in [
        (1, Rr3PathSlot::Slots),
        (2, Rr3PathSlot::Roles),
        (3, Rr3PathSlot::Devices),
    ] {
        if member.strip_prefix("contracts/") == Some(books[idx]) {
            return Some(slot);
        }
    }
    None
}

/// 从真值集记账行推导写口路径集（跨平台纯函数）。topics.toml 不取（装载链
/// 内部按 contracts 目录读）。
pub fn rr3_write_paths(truth_set: &[Rr3TruthSetEntry]) -> Result<Rr3DomainWritePaths, String> {
    Ok(Rr3DomainWritePaths {
        profile: rr3_path_at(truth_set, Rr3PathSlot::Profile, "profiles/<stem>.toml")?,
        slots: rr3_path_at(truth_set, Rr3PathSlot::Slots, "contracts/slots.toml")?,
        roles: rr3_path_at(truth_set, Rr3PathSlot::Roles, "contracts/roles.toml")?,
        devices: rr3_path_at(truth_set, Rr3PathSlot::Devices, "contracts/devices.toml")?,
        wiring: rr3_path_at(
            truth_set,
            Rr3PathSlot::Wiring,
            "hardware/<stem>/wiring.toml",
        )?,
        platform_dir: rr3_path_at(
            truth_set,
            Rr3PathSlot::Platform,
            "contracts/platform/<chip>.toml",
        )?,
    })
}

/// 单槽取必需路径：分类/歧义/缺员判据全在下游单点，此处只做"取 → 点名"。
fn rr3_path_at(
    truth_set: &[Rr3TruthSetEntry],
    slot: Rr3PathSlot,
    what: &str,
) -> Result<PathBuf, String> {
    rr3_req(rr3_slot_path(truth_set, slot)?, what)
}

/// 按槽取路径（B10 自 write_paths 抽出；本轮审计加歧义门）：分类单点仍是
/// [`rr3_path_slot`]；同一槽多成员记账 = 记账形状破，**拒**（写口不猜哪个是
/// 真目标——静默取首条会把写落到错误文件）。
fn rr3_slot_path(
    truth_set: &[Rr3TruthSetEntry],
    slot: Rr3PathSlot,
) -> Result<Option<PathBuf>, String> {
    let hits: Vec<&Rr3TruthSetEntry> = truth_set
        .iter()
        .filter(|e| rr3_path_slot(&e.member) == Some(slot))
        .collect();
    let Some(e) = rr3_slot_one(&hits)? else {
        return Ok(None);
    };
    // platform 槽登记的是 chip 表文件本身，写口要的是目录（兄弟表落位同目录）
    Ok(rr3_slot_pick(e, slot))
}

/// 同槽命中数裁定：0 条 = 该槽无成员，1 条 = 唯一目标，多条 = 歧义拒。
fn rr3_slot_one<'a>(hits: &[&'a Rr3TruthSetEntry]) -> Result<Option<&'a Rr3TruthSetEntry>, String> {
    match hits.len() {
        0 => Ok(None),
        1 => Ok(Some(hits[0])),
        n => Err(rr3_slot_dup_err(hits, n)),
    }
}

/// 同槽多条记账 = 路径歧义：点名前三条成员，拒（写口不猜哪个是真目标）。
fn rr3_slot_dup_err(hits: &[&Rr3TruthSetEntry], n: usize) -> String {
    let names: Vec<&str> = hits.iter().take(3).map(|e| e.member.as_str()).collect();
    format!(
        "真值集同类记账成员重复 {n} 条（{} 等）——写口路径歧义，拒",
        names.join(", ")
    )
}

fn rr3_slot_pick(e: &Rr3TruthSetEntry, slot: Rr3PathSlot) -> Option<PathBuf> {
    if slot == Rr3PathSlot::Platform {
        e.path.parent().map(PathBuf::from)
    } else {
        Some(e.path.clone())
    }
}

/// 缺员点名（B10 自 write_paths 抽出）：真值集记账七成员里写口必需六路，
/// 缺哪条点哪条，不静默合并。
fn rr3_req(v: Option<PathBuf>, what: &str) -> Result<PathBuf, String> {
    let Some(p) = v else {
        return Err(format!("真值集记账缺 {what} 成员（记账形状破），写口拒"));
    };
    Ok(p)
}
