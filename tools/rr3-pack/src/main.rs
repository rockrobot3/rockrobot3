// SPDX-License-Identifier: Apache-2.0
//! rr3-pack —— 发行版快路 deb 打包器（ADR-0013 裁定 5）：两份真值 → 一个 deb。
//!
//! 输入：`--payload` 注入产物（rrd / rr3-ctl / rr3-ota-ctl / 模块 bin 或包树 +
//! manifest）；`--repo` 仓根（装配单、contracts 六册、`hardware/<机型>/` 与轴档案、
//! 模块 config）。前六册 + 轴档案是 rrd 装载门的 boot 真值集，成员清单真值只有
//! rrd `rr3_truth_set` 一份——打包面照单收口，不收第二份；打包前按装载同判据预验，
//! 坏单不出包。
//!
//! 布局：`/usr/bin/{rrd,rr3-ctl,rr3-ota-ctl}` + `/usr/lib/rockrobot3/{profiles,
//! contracts,modules,hardware}` + `rrd.service` + `DEBIAN/{control,postinst}`；
//! staging 是纯 Rust（macOS 可跑可单测）。
//!
//! 内容改写只有两处（其余字节零差异）：① entry 装机改写——manifest.entry 换成
//! 装机绝对路径（线上注册门禁要求绝对路径），只改引号内一个值；② `rrd.service`
//! 的 `--profile` 装机位随 `--profile` 同步改写（模板锚定 distro）——锚点缺席即
//! fail-closed。
//!
//! python 模块（`lang = "python"`）产物是 shim 脚本而非 ELF：`PYTHONPATH` 与
//! `config` 必须装机绝对路径，故 shim 由本器生成；仓内六模块当前全 Rust（保留位）。

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use rrd::manifest::rr3_manifest_parse;

/// deb 包名/版本/装机根。版本唯一真源 = 仓根 `VERSION`（编译期嵌入，树内不落
/// 第二份字面量；2026-11-23 部署链实录：缺省值曾长期落后板上版本，裸跑产出的包
/// 被判降级并静默回退）。发版用 [`RR3_DEB_VERSION_ENV`] 覆盖，包名与控制字段同源。
const RR3_DEB_PACKAGE: &str = "rockrobot3";
const RR3_PRODUCT_VERSION: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../VERSION"));
const RR3_INSTALL_ROOT: &str = "/usr/lib/rockrobot3";
/// 版本覆盖环境变量：发版流水线与部署脚本同读一处，避免"包名 0.0.6 / control
/// 0.0.5"这类两处真值漂移。
const RR3_DEB_VERSION_ENV: &str = "RR3_DEB_VERSION";

/// 本次出包版本（环境变量优先，缺省值为本地/演练档）。
fn rr3_deb_version() -> String {
    std::env::var(RR3_DEB_VERSION_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RR3_PRODUCT_VERSION.trim().to_string())
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
    /// 直接落到一棵已存在的目标根（Buildroot TARGET_DIR）：只装 usr/bin 三件 +
    /// 模块树 + 装载真值集，不写 DEBIAN/systemd、不调 dpkg-deb。保底轨
    /// post-build.sh 与快路轨共用同一套 entry 改写/真值集裁定。
    install_root: Option<PathBuf>,
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
    let res = if cfg.install_root.is_some() {
        rr3_install_tree(&cfg).map(|root| format!("目标根装配就绪: {}", root.display()))
    } else {
        rr3_pack(&cfg)
    };
    match res {
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
