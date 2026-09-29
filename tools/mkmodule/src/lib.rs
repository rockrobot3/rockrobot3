//! # rr3-mkmod
//!
//! 生成 `modules/rr3-<名>/` 骨架：manifest + config + 空 Rust 程序 + README。
//! 生成物当场用 `rr-registry` 的契约校验，不合规即中止（architecture.md §7）。

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
            Self::InvalidName(n) => write!(f, "模块名非法: {n}（要求 rr3-[a-z]+，词干 2..=24 个小写字母）"),
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
    let ok = stem.len() >= 2
        && stem.len() <= 24
        && stem.chars().all(|c| c.is_ascii_lowercase());
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

| 文件 | 作用 |
|---|---|
| `module.toml` | manifest，字段契约见 `base/runtime/rr-registry/src/module.schema.toml` |
| `config.toml` | 运行期配置（密钥不入库，见 docs/conventions.md） |
| `Cargo.toml` | 独立 crate，M2 起 path 依赖 `rr-ipc`/`rr-bus` |
| `src/main.rs` | 入口骨架 |

## 下一步

1. 在 `contracts/topics.toml` 登记本模块的主题与 owner（改契约要走 review）
2. 打开 `Cargo.toml` 里的 `rr-ipc`/`rr-bus` 依赖注释，接入 UDS（M2）
3. 反射环（若有）留在本进程内，永不进总线（ADR-0007 裁定 1）
"#
    )
}

/// 在 `out_root` 下生成 `<id>/` 全套骨架；返回生成的文件列表（绝对路径）
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
        let path = staging.join(rel.to_string());
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
    Ok(files.iter().map(|(rel, _)| target.join(rel.to_string())).collect())
}
