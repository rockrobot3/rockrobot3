//! rr3-pack —— 发行版快路 deb 打包器（0.0.2 T5 / ADR-0013 裁定 5）。
//!
//! 输入两份真值，装配各自薄装（保底轨 post-build.sh 读同一注入格式，两轨不互抄装配）：
//! - `--payload` 注入产物（两轨同源约定，与 buildroot post-build.sh 同一口径）：
//!   `rrd`、`rr3-ctl`；rust 模块 `rr3-modules/<name>/{bin,manifest.toml}`；
//!   python 模块 `rr3-modules/<name>/{pkg/,manifest.toml}`（`pkg/` = 包树，
//!   如 `rr3_brain/`；entry 装机产物是打包器生成的 shim，见下）；
//! - `--repo` 仓根：`profiles/<name>.toml`（`--profile` 选机型，缺省 distro——H4/C2 批K
//!   参数化，出包链不再只认一份装配单）、`contracts/{topics,slots,roles,devices}.toml`、
//!   `contracts/platform/<chip>.toml`（按装配单 `[platform].chip` 选册，批N1 基-22）、
//!   `hardware/<name>/wiring.toml`、`hardware/<name>/axis/`（轴档案目录，在则整目录
//!   随装）、`modules/<name>/config.toml`（config 是源真值，不是构建产物，不进注入
//!   格式）。前六册 + 轴档案是 rrd 装载门的 boot 真值集，成员清单的真值只有 rrd
//!   `rr3_truth_set` 一份（增补轮六 基-25／实-12：装载面定义清单，打包面照单
//!   收口、不自带第二份——deb 缺任一必需册 rrd 即拒 boot，打包面缺任一册拒出包；
//!   打包先按装载同判据预验装配单，坏单不得出包）。
//!
//! staging 装配是纯 Rust（macOS 可跑可单测），deb 布局：
//! `/usr/bin/{rrd,rr3-ctl}` + `/usr/lib/rockrobot3/{profiles,contracts,modules,hardware}` +
//! `/usr/lib/systemd/system/rrd.service` + `DEBIAN/{control,postinst}`。
//!
//! **内容改写只有两处**（除此外字节零差异）：①entry 装机改写（modules/README.md：
//! "entry 在仓相对、装机绝对"）：manifest 只把 entry 的值换成装机绝对路径，其余字节
//! 原样。为什么必须改：线上注册门禁（rrd conn.rs）要求 manifest.entry 绝对路径——模块
//! 开机读的就是装机里这份 manifest 原文上报，相对值必被 `AIRY_EINVAL` 拒；装配单磁盘
//! 装载那侧绝对/相对皆可，改写成绝对两头都过。错行不改、少行不改：除 entry 引号内
//! 一个值外零差异。
//!
//! ②rrd.service 的 --profile 装机位改写（0.0.5 批K 深挖收口）：单元模板 ExecStart
//! 锚定缺省机型 distro.toml 的装机位，而非 distro 机型出包若照抄，systemd 拉起的 rrd
//! 找不到装配单必败——装机位 profiles/<name>.toml 只装 --profile 指的那一份，故
//! ExecStart 的 profile 路径随 --profile 同步改写。缺省 distro 恒等改写字节零差异；
//! 模板锚点缺席即报错（fail-closed），不静默照抄。
//!
//! **python 模块的 entry 装机**（`lang = "python"`，批F1）：产物不是 ELF 而是 shim 脚本，
//! 且 shim 里的 `PYTHONPATH`/`RR3_BRAIN_CONFIG` 必须是**装机绝对路径**（只有打包器知道
//! `RR3_INSTALL_ROOT`），故 shim 由打包器生成而非注入（`bin` 那个位置对 python 模块换成
//! `pkg/` 包树）。机制按 manifest.lang 数据位分派（批N32 件四起仓内六模块全 Rust，
//! python 路径是机制保留位，仓内暂无消费模块）：
//! `#!/bin/sh\nRR3_BRAIN_CONFIG=<装机 config> PYTHONPATH=<装机包树> exec python3 -m <包名>\n`
//! （包名 = 目录名连字符转下划线；config env 走 brain `rr3_config_load` 的文档化优先口，
//! python 模块 config 契约当前即 brain 契约）。
//! 运行期语言不分支（rrd 只看 entry 可执行），`lang` 只在本打包器与演练侧做产物路径推导。

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use rrd::manifest::rr3_manifest_parse;

/// deb 包名/版本/装机根。版本缺省跟方案批次走（0.0.6）；发版用
/// [`RR3_DEB_VERSION_ENV`] 覆盖，包名与控制字段同源。
const RR3_DEB_PACKAGE: &str = "rockrobot3";
const RR3_DEB_VERSION: &str = "0.0.6";
const RR3_INSTALL_ROOT: &str = "/usr/lib/rockrobot3";
/// 版本覆盖环境变量：发版流水线与部署脚本同读一处，避免"包名 0.0.6 / control
/// 0.0.5"这类两处真值漂移。
const RR3_DEB_VERSION_ENV: &str = "RR3_DEB_VERSION";

/// 本次出包版本（环境变量优先，缺省值为本地/演练档）。
fn rr3_deb_version() -> String {
    std::env::var(RR3_DEB_VERSION_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RR3_DEB_VERSION.to_string())
}

/// systemd 单元实体在 base/os/distro/units/rrd.service，include_str! 编译期嵌入：
/// 文件缺席直接编译失败（比运行期读 --repo 再报错更早），打包器对 repo 布局的依赖
/// 只剩 --repo 下的 profiles/contracts/hardware 三个数据目录与各模块 config.toml。
/// 路径以 CARGO_MANIFEST_DIR 构建期注入（基-32：不手写跨仓层数）。
const RR3_SERVICE_UNIT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../base/os/distro/units/rrd.service"
));

/// 打包配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3PackConfig {
    payload: PathBuf,
    repo: PathBuf,
    out: PathBuf,
    /// deb Architecture 字段。缺省 arm64（出厂目标 aarch64）；CI 容器冒烟按
    /// `dpkg --print-architecture` 传 amd64——架构跟着真产物走，不硬编码。
    arch: String,
    /// 装机装配单名（不带 .toml）。缺省 distro 不破坏既有调用方（CI distro-smoke、
    /// pack-arm64.sh）；换机型 = `--profile <name>`，staging 装机位 profiles/<name>.toml
    /// 零改写落位——出包链不再只认 distro 一份（H4/C2，0.0.5 批K）。
    /// rrd.service ExecStart 的装机位随名同步改写（缺省 distro 恒等，见模块头注②）。
    profile: String,
    stage_only: bool,
}

// 入口是 crate root：子模块按 src/ 解析，故显式给路径（模块体在 pack/ 下）。
#[path = "pack/args.rs"]
mod args;
#[path = "pack/copy.rs"]
mod copy;
#[path = "pack/deb.rs"]
mod deb;
#[path = "pack/stage.rs"]
mod stage;
#[cfg(test)]
mod tests;

use args::*;
use copy::*;
use deb::*;
use stage::*;

fn rr3_pack(cfg: &Rr3PackConfig) -> Result<String, String> {
    let stage = rr3_stage(cfg)?;
    rr3_pack_from(cfg, &stage)
}

/// staging 树就绪后的两条出口（`--stage-only` 直接收兵，否则出 deb）。
fn rr3_pack_from(cfg: &Rr3PackConfig, stage: &Path) -> Result<String, String> {
    if cfg.stage_only {
        return Ok(format!(
            "staging 树就绪（--stage-only，未调 dpkg-deb）: {}",
            stage.display()
        ));
    }
    rr3_pack_deb(cfg, stage)
}

/// 出 deb 并给出产物与 staging 树路径。
fn rr3_pack_deb(cfg: &Rr3PackConfig, stage: &Path) -> Result<String, String> {
    rr3_build_deb(stage, &cfg.out)?;
    Ok(format!(
        "deb 就绪: {}（staging 树: {}）",
        cfg.out.display(),
        stage.display()
    ))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = rr3_pack_cfg(&args);
    match rr3_pack(&cfg) {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("rr3-pack: {e}");
            std::process::exit(1);
        }
    }
}

/// 参数解析失败即退 2（用法错误）。
fn rr3_pack_cfg(args: &[String]) -> Rr3PackConfig {
    match rr3_pack_parse(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rr3-pack: {e}");
            std::process::exit(2);
        }
    }
}
