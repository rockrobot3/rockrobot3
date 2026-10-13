//! # rr3-mkmod
//!
//! 生成 `modules/rr3-<名>/` 骨架：manifest + config + 空 Rust 程序 + README。
//! 生成物当场用 `rr-registry` 的契约校验，不合规即中止（architecture.md §7）。
//!
//! 0.0.5 批B② 扩产（ADR-0019）：原子件五件套生成（`rr3_mkmod_generate_atom`）——
//! ① `atom.toml`（Atom manifest，契约=`rr-registry/src/atom.schema.toml`）
//! ② 入口（`<名>.rs`，只声明公开面）· ③ 实现（`mech.rs`，机制主体）
//! ④ 装配清单（`assemble.toml`，注入面声明）· ⑤ 唯一私有头（`private.rs`）。
//! 产物当场回灌 `rr3_registry_validate_atom`（字段契约 + slots 查册门 + 板无关断言）。

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Rr3MkmodError {
    InvalidName(String),
    AlreadyExists(PathBuf),
    Io(std::io::Error),
    Manifest(Vec<String>),
}

impl From<std::io::Error> for Rr3MkmodError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl std::fmt::Display for Rr3MkmodError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName(n) => write!(
                f,
                "名字非法: {n}（模块要求 rr3-[a-z]+，词干 2..=24 个小写字母；原子要求 ^[a-z][a-z_]*$，2..=24 字符）"
            ),
            Self::AlreadyExists(p) => write!(f, "目录已存在: {}", p.display()),
            Self::Io(e) => write!(f, "IO 失败: {e}"),
            Self::Manifest(errs) => write!(f, "生成的 manifest 不合规:\n  {}", errs.join("\n  ")),
        }
    }
}

const RR3_NAME_HEAD: &str = "rr3-";

/// 归一化：允许 `audio` 或 `rr3-audio`，产出 `rr3-audio`
/// 词干必须匹配方案 §4 的 `^rr3-[a-z]+$`（小写字母，不含数字/连字符），长度 2..=24
fn rr3_mkmod_stem(name: &str) -> Result<String, Rr3MkmodError> {
    let stem = name.strip_prefix(RR3_NAME_HEAD).unwrap_or(name);
    let ok = stem.len() >= 2 && stem.len() <= 24 && stem.chars().all(|c| c.is_ascii_lowercase());
    if !ok {
        return Err(Rr3MkmodError::InvalidName(name.to_string()));
    }
    Ok(format!("{RR3_NAME_HEAD}{stem}"))
}

fn module_toml(id: &str) -> String {
    format!(
        r#"[manifest]
schema_version = 1
name = "{id}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "{id}"

[provides]
capabilities = []

[depends]
required = ["bus.publish"]
optional = []

[events]
publishes = []
subscribes = []

[resources]
restart_policy = "on-failure"
mem_limit_mb = 32
start_priority = 100

[health]
heartbeat_ms = 2000
watchdog_timeout_ms = 10000

# 按需补充（缺省即合规，ADR-0007 决策二）：
# [upgrades]
# min_from_semver = "0.0.1"
# staging = true
"#
    )
}

fn config_toml(id: &str) -> String {
    format!(
        r#"# {id} 运行期配置（由模块自己读取；secret 一律走环境变量，不落盘）
[{id}]
log_level = "info"
"#
    )
}

fn crate_toml(id: &str) -> String {
    format!(
        r#"[package]
name = "{id}"
version = "0.0.1"
edition = "2021"
license = "UNLICENSED"
publish = false

# 独立工程：modules/ 不在 base/runtime workspace 内
[workspace]

[[bin]]
name = "{id}"
path = "src/main.rs"

[dependencies]
# M2 接入总线时启用（path 依赖，勿用 crates.io 同名包）：
# rr-ipc = {{ path = "../../base/runtime/rr-ipc" }}
# rr-bus = {{ path = "../../base/runtime/rr-bus" }}
"#
    )
}

fn main_rs(id: &str) -> String {
    format!(
        r#"//! {id} 骨架。业务实现见 modules/{id}/README.md 的首个里程碑。

fn main() {{
    println!("{id} 0.0.1 (骨架，未接入总线)");
}}
"#
    )
}

fn readme(id: &str) -> String {
    format!(
        r#"# {id}

由 `tools/mkmodule`（`rr3-mkmod`）生成的模块骨架。

## 落位

生成器落盘**五个文件**（四类结构角色 + crate 清单，见 docs/architecture/naming.md §4）：

| 文件 | 作用 |
|---|---|
| `module.toml` | manifest，字段契约见 `base/runtime/rr-registry/src/module.schema.toml` |
| `config.toml` | 运行期配置（密钥不入库，见 docs/architecture/conventions.md） |
| `Cargo.toml` | 独立 crate，M2 起 path 依赖 `rr-ipc`/`rr-bus` |
| `src/main.rs` | 入口骨架 |
| `README.md` | 本文件：能力表 + 事件表 + 配置说明 |

## 下一步

1. 在 `contracts/topics.toml` 登记本模块的主题与 owner（改契约要走 review）
2. 打开 `Cargo.toml` 里的 `rr-ipc`/`rr-bus` 依赖注释，接入 UDS（M2）
3. 反射环（若有）留在本进程内，永不进总线（ADR-0007 裁定 1）
"#
    )
}

/// 在 `out_root` 下生成 `<id>/` 全套骨架；返回生成的文件列表。
///
/// 返回的路径均以调用方传入的 `out_root` 为根原样拼接：`out_root` 为绝对路径时
/// 返回绝对路径，为相对路径时返回相对路径（其解析依赖调用方当前工作目录）。
/// 本函数不会把 `out_root` 规范化为绝对路径。
pub fn rr3_mkmod_generate(name: &str, out_root: &Path) -> Result<Vec<PathBuf>, Rr3MkmodError> {
    let id = rr3_mkmod_stem(name)?;
    let target = out_root.join(&id);
    if target.exists() {
        return Err(Rr3MkmodError::AlreadyExists(target));
    }

    // 先写临时目录，manifest 校验通过后再 rename，避免留下半成品模块
    let staging = out_root.join(format!(".staging-{id}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("src"))?;

    let files: Vec<(&str, String)> = vec![
        ("module.toml", module_toml(&id)),
        ("config.toml", config_toml(&id)),
        ("Cargo.toml", crate_toml(&id)),
        ("src/main.rs", main_rs(&id)),
        ("README.md", readme(&id)),
    ];
    for (rel, body) in &files {
        let path = staging.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, body)?;
    }

    let report = rr_registry::rr3_registry_validate_manifest(&staging.join("module.toml"));
    if !report.ok() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(Rr3MkmodError::Manifest(report.errors));
    }

    std::fs::rename(&staging, &target)?;
    Ok(files.iter().map(|(rel, _)| target.join(rel)).collect())
}

/// 原子件名：`^[a-z][a-z_]*$`，2..=24 字符（atom.schema.toml 的 atom_name 规则同款）
fn rr3_mkmod_atom_stem(name: &str) -> Result<String, Rr3MkmodError> {
    let ok = name.len() >= 2
        && name.len() <= 24
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    if !ok {
        return Err(Rr3MkmodError::InvalidName(name.to_string()));
    }
    Ok(name.to_string())
}

fn atom_manifest_toml(name: &str, slot: &str) -> String {
    format!(
        r#"# {name} 原子件 manifest（三层 manifest 的 Atom 层，ADR-0019）
# 契约：base/runtime/rr-registry/src/atom.schema.toml；查册门查 contracts/slots.toml 同一张表
schema_version = 1

[atom]
name = "{name}"
version = "0.0.1"

[provides]
slots = ["{slot}"]   # 占哪个格——每格一件（ADR-0019 裁定 1：恰一格、不可再分）

[uses]
slots = []           # 装配期依赖的其他格（描述符注入，不见厂商/引脚）

[events]
publishes = []
subscribes = []
"#
    )
}

fn atom_entry_rs(name: &str, slot: &str) -> String {
    format!(
        r#"//! {name} 原子件入口（五件套②）：只声明公开面，机制在 mech.rs。
//! 占格 {slot}（atom.toml）。组合件以 `#[path]` 引本文件为 mod；
//! 跨域只准 use 本文件 pub 项（五件套⑤纪律，内部共享落 private.rs，永不 re-export）。

#[path = "mech.rs"]
mod mech;
#[path = "private.rs"]
mod private;

pub use mech::*;
"#
    )
}

fn atom_mech_rs(name: &str) -> String {
    format!(
        r#"//! {name} 机制主体（五件套③）：零策略载荷（ADR-0019 裁定 5——厂商名/产品名/
//! 业务阈值不进机制件，策略经 ops 函数表 + manifest/config 数据化注入）。

#[cfg(test)]
mod tests {{
    /// 骨架自检：机制件可编译、可测。真实现落位后替换本测试。
    #[test]
    fn skeleton_compiles() {{
        assert_eq!(1 + 1, 2);
    }}
}}
"#
    )
}

fn atom_assemble_toml(name: &str) -> String {
    format!(
        r#"# {name} 装配清单（五件套④）：装配期注入面声明（ADR-0019 修订 1 ③——
# [assemble.handles] 声明件用哪个能力面 trait ← 绑描述符哪个键、[assemble.ops]
# 声明件暴露哪些参数槽 ← 取哪个 config 键；装配段校验缺项即该格 Degraded）。
schema_version = 2

[assemble]
handles = []          # 每条 {{ trait = "能力面", desc_key = "描述符键" }}（在册表见 rr-registry assemble.rs）
ops = []              # 每条 {{ slot = "参数槽", src_key = "config 点路径" }}
# 骨架期无注入面：明写 no_injection（L-5）。件填实后删本行，按上面两数组申报真实句柄/参数槽；
# 两数组均空而缺本行即判红（"漏填"与"确无注入面"不可区分）。
no_injection = true
"#
    )
}

fn atom_private_rs(name: &str) -> String {
    format!(
        r#"//! {name} 唯一私有头（五件套⑤）：原子件内部共享的类型与常量。
//! 私有性纪律：本文件项不进入口 re-export，跨域不可见（architecture.md §4.0.4）。
"#
    )
}

/// 在 `out_root` 下生成原子件 `atoms 形状` 的 `<name>/` 五件套；返回生成的文件列表。
///
/// `out_root` 语义 = 原子根目录（惯例 `modules/<组合件>/atoms`），产物落
/// `<out_root>/<name>/`。生成后当场回灌 `rr3_registry_validate_atom`（字段契约 +
/// slots 查册门 + 板无关断言）：`out_root` 须位于仓内以向上定位 `contracts/slots.toml`，
/// 校验不过即整体丢弃（staging→rename，不留半成品）。
pub fn rr3_mkmod_generate_atom(
    name: &str,
    slot: &str,
    out_root: &Path,
) -> Result<Vec<PathBuf>, Rr3MkmodError> {
    let stem = rr3_mkmod_atom_stem(name)?;
    let target = out_root.join(&stem);
    if target.exists() {
        return Err(Rr3MkmodError::AlreadyExists(target));
    }

    let staging = out_root.join(format!(".staging-{stem}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    let files: Vec<(String, String)> = vec![
        ("atom.toml".into(), atom_manifest_toml(&stem, slot)),
        (format!("{stem}.rs"), atom_entry_rs(&stem, slot)),
        ("mech.rs".into(), atom_mech_rs(&stem)),
        ("assemble.toml".into(), atom_assemble_toml(&stem)),
        ("private.rs".into(), atom_private_rs(&stem)),
    ];
    for (rel, body) in &files {
        std::fs::write(staging.join(rel), body)?;
    }

    // 回灌校验（②b 同款纪律）：manifest + 装配清单生成即自检，不合规即中止。
    // G4：装配清单走 rr-registry 形状级真值（模板空数组必过；填实后坏形当场听见）。
    let mut errs = rr_registry::rr3_registry_validate_atom(&staging.join("atom.toml"));
    let assemble_body = files
        .iter()
        .find(|(rel, _)| rel == "assemble.toml")
        .map(|(_, body)| body.as_str())
        .expect("五件套必含 assemble.toml");
    if let Err(asm_errs) = rr_registry::rr3_registry_assemble_from_text(assemble_body) {
        errs.extend(asm_errs);
    }
    if !errs.is_empty() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(Rr3MkmodError::Manifest(errs));
    }

    std::fs::rename(&staging, &target)?;
    Ok(files.iter().map(|(rel, _)| target.join(rel)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仓内临时根（target/ 不入库且向上可定位 contracts/slots.toml，回灌门可走通）
    fn scratch_root(tag: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/mkmod-test")
            .join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 原子五件套：五文件齐全 + atom.toml 回灌过查册门与板无关断言
    #[test]
    fn atom_generation_passes_registry_validation() {
        let root = scratch_root("ok");
        let files = rr3_mkmod_generate_atom("sample_atom", "power.battery", &root)
            .expect("合法生成必须成功");
        assert_eq!(files.len(), 5, "五件套缺一不可");
        for rel in [
            "sample_atom/atom.toml",
            "sample_atom/sample_atom.rs",
            "sample_atom/mech.rs",
            "sample_atom/assemble.toml",
            "sample_atom/private.rs",
        ] {
            assert!(root.join(rel).is_file(), "缺 {rel}");
        }
        // 生成物独立再过一遍校验器（回灌发生在 staging 期，rename 后幂等）
        let errs = rr_registry::rr3_registry_validate_atom(&root.join("sample_atom/atom.toml"));
        assert!(errs.is_empty(), "{errs:?}");
    }

    /// C16 件面形状 × C3 模板成对（0.0.5 批I）：生成五件套不得出现 serde_json——
    /// 件面只留纯机制（结构体+判定函数），载荷组装归组合件承载层（ADR-0019 修订 4）。
    /// 模板漂移反向断言（模块设计索引 X-5 教训：模板不改，新件一生成就违规）。
    #[test]
    fn generated_atom_face_carries_no_serde_json() {
        let root = scratch_root("closure");
        rr3_mkmod_generate_atom("sample_atom", "power.battery", &root).unwrap();
        for rel in [
            "sample_atom/atom.toml",
            "sample_atom/sample_atom.rs",
            "sample_atom/mech.rs",
            "sample_atom/assemble.toml",
            "sample_atom/private.rs",
        ] {
            let text = std::fs::read_to_string(root.join(rel)).unwrap();
            assert!(!text.contains("serde_json"), "{rel} 不得出现 serde_json");
        }
    }

    /// 非法输入三类：坏原子名、已存在目录、未登记格位（查册门回灌拒绝）
    #[test]
    fn atom_generation_rejects_bad_inputs() {
        let root = scratch_root("bad");
        assert!(matches!(
            rr3_mkmod_generate_atom("Bad-Atom", "power.battery", &root),
            Err(Rr3MkmodError::InvalidName(_))
        ));
        rr3_mkmod_generate_atom("dupe", "power.battery", &root).unwrap();
        assert!(matches!(
            rr3_mkmod_generate_atom("dupe", "power.battery", &root),
            Err(Rr3MkmodError::AlreadyExists(_))
        ));
        let e = rr3_mkmod_generate_atom("nosuch", "power.solar", &root).unwrap_err();
        let Rr3MkmodError::Manifest(errs) = e else {
            panic!("未登记格位必须由回灌拒绝")
        };
        assert!(
            errs.iter().any(|e| e.contains("未登记格位 power.solar")),
            "{errs:?}"
        );
        // 失败不留半成品
        assert!(!root.join(".staging-nosuch").exists());
        assert!(!root.join("nosuch").exists());
    }
}
