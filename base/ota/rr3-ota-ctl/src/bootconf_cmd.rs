// SPDX-License-Identifier: Apache-2.0
//! 引导配置与回滚链子命令（switch / factory / rollback arm|ok）。
//!
//! 判定与改写逻辑全在 `rr-ota`（`bootconf` / `bootenv`）；本文件只做参数形制、
//! 路径读取与原子落盘调用。所有写命令都要求显式路径，不缺省碰任何文件。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rr_ota::bootconf;
use rr_ota::bootenv;
use rr_ota::Rr3Slot;

use crate::args::{rr3_use, rr3_val_arg};

/// 目标标签解析：在册标签映射槽位；不在册标签属用法错（exit 2）
pub(crate) fn rr3_slot_arg(label: &str) -> Result<Rr3Slot, String> {
    Rr3Slot::rr3_slot_from_label(label)
        .ok_or_else(|| format!("未知槽位标签 {label:?}（在册：rr_a / rr_b / rr_factory）"))
}

/// 抓取 `--bootconf <路径>`。无缺省值——本 CLI 不缺省碰任何路径。
pub(crate) fn rr3_bootconf_arg(args: &[String]) -> Result<PathBuf, String> {
    rr3_val_arg(
        args,
        "--bootconf",
        "switch/factory 必须显式传 --bootconf <路径>",
    )
    .map(PathBuf::from)
}

/// switch 参数面：首参即目标槽，缺参 / 不在册标签都属用法错（exit 2）。
fn rr3_parse_switch(args: &[String]) -> Result<(Rr3Slot, PathBuf), (String, ExitCode)> {
    let label = args
        .first()
        .ok_or_else(|| rr3_use("switch 缺目标槽位参数".into()))?;
    let target = rr3_slot_arg(label).map_err(rr3_use)?;
    let path = rr3_bootconf_arg(args).map_err(rr3_use)?;
    Ok((target, path))
}

/// 引导面子命令分派：switch / factory / rollback。
pub(crate) fn rr3_dispatch(cmd: &str, args: &[String]) -> Result<String, (String, ExitCode)> {
    match cmd {
        "switch" => rr3_parse_switch(args).and_then(|(target, path)| rr3_cmd_switch(target, &path)),
        "factory" => rr3_bootconf_arg(args)
            .map_err(rr3_use)
            .and_then(|path| rr3_cmd_factory(&path)),
        "rollback" => rr3_cmd_rollback(args),
        other => Err(rr3_use(format!("未知引导子命令 {other:?}"))),
    }
}

/// switch：读 → 判定改写 → 原子写回。业务拒（同槽幂等拒 / rr_factory 目标 /
/// 形状漂移）一律 (文案, 1)——文案全部来自 rr-ota::bootconf，本层不重抄语义。
pub(crate) fn rr3_cmd_switch(target: Rr3Slot, path: &Path) -> Result<String, (String, ExitCode)> {
    let (text, from) = rr3_conf_slot(path, "拒绝切换")?;
    let out = bootconf::rr3_bootconf_switch(&text, target)
        .map_err(|e| (format!("拒绝切换：{e}"), ExitCode::from(1)))?;
    rr3_conf_store(path, &out)?;
    Ok(format!(
        "引导配置已改写：root=LABEL={} → {}（持久切换，重启后生效）",
        from.rr3_slot_label(),
        target.rr3_slot_label()
    ))
}

/// factory：同上，但目标是 rr_factory 的一次性出厂引导（分区本体不写）。
fn rr3_cmd_factory(path: &Path) -> Result<String, (String, ExitCode)> {
    let (text, from) = rr3_conf_slot(path, "拒绝恢复出厂")?;
    let out = bootconf::rr3_bootconf_factory(&text)
        .map_err(|e| (format!("拒绝恢复出厂：{e}"), ExitCode::from(1)))?;
    rr3_conf_store(path, &out)?;
    Ok(format!(
        "引导配置已改写：root=LABEL={} → rr_factory（一次性出厂引导，重启后生效；rr_factory 分区本体不写）",
        from.rr3_slot_label()
    ))
}

/// 读引导配置 + 判当前槽：`why` 是业务拒文案前缀（两子命令各自口径）。
fn rr3_conf_slot(path: &Path, why: &str) -> Result<(String, Rr3Slot), (String, ExitCode)> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| (format!("读引导配置 {path:?} 失败: {e}"), ExitCode::from(1)))?;
    let from = bootconf::rr3_bootconf_slot(&text)
        .map_err(|e| (format!("{why}：{e}"), ExitCode::from(1)))?;
    Ok((text, from))
}

/// 原子写回引导配置（失败点名路径与原因）。
fn rr3_conf_store(path: &Path, out: &str) -> Result<(), (String, ExitCode)> {
    bootconf::rr3_bootconf_store(path, out).map_err(|e| {
        (
            format!("引导配置 {path:?} 原子落盘失败: {e}"),
            ExitCode::from(1),
        )
    })
}

/// rollback：arm 上膛 / ok 解除（`docs/design/分区布局与OTA槽位.md` 标准序第 4/6 步）。
pub(crate) fn rr3_cmd_rollback(args: &[String]) -> Result<String, (String, ExitCode)> {
    match args.first().map(String::as_str) {
        Some("arm") => rr3_cmd_rb_arm(&args[1..]),
        Some("ok") => rr3_cmd_rb_ok(&args[1..]),
        Some(other) => Err(rr3_use(format!(
            "未知 rollback 子命令 {other:?}（在册：arm / ok）"
        ))),
        None => Err(rr3_use("rollback 缺子命令（在册：arm / ok）".into())),
    }
}

/// rollback arm：`--limit` 只收无符号整数（非零判据归 rr-ota::bootenv）。
fn rr3_limit_arg(args: &[String]) -> Result<u32, String> {
    let raw = rr3_val_arg(args, "--limit", "rollback arm 必须显式传 --limit <次数>")?;
    raw.parse::<u32>()
        .map_err(|_| format!("--limit 只收无符号整数（收到 {raw:?}）"))
}

/// rollback arm：上膛四变量（判据在 rr-ota::bootenv，本层只做参数形制）。
pub(crate) fn rr3_cmd_rb_arm(args: &[String]) -> Result<String, (String, ExitCode)> {
    let limit = rr3_limit_arg(args).map_err(rr3_use)?;
    let altcmd = rr3_val_arg(
        args,
        "--altcmd",
        "rollback arm 必须显式传 --altcmd <备援命令>",
    )
    .map_err(rr3_use)?;
    let (dev, copies) = rr3_env_mutate(&rr3_env_arg(args)?, "上膛", |env| {
        env.rr3_rollback_arm(limit, &altcmd)
    })?;
    Ok(format!(
        "回滚链已上膛{}：upgrade_available=1 bootcount=0 bootlimit={limit} altbootcmd={altcmd:?}（{} offset=0x{:x}）",
        rr3_copies_note(copies),
        dev.rr3_dev_path(),
        dev.rr3_dev_offset()
    ))
}

/// rollback ok：解除两变量；`bootlimit` / `altbootcmd` 保留给人工排障。
pub(crate) fn rr3_cmd_rb_ok(args: &[String]) -> Result<String, (String, ExitCode)> {
    let (dev, copies) = rr3_env_mutate(&rr3_env_arg(args)?, "解除", |env| env.rr3_rollback_ok())?;
    Ok(format!(
        "回滚链已解除{}：upgrade_available=0 bootcount=0（{} offset=0x{:x}）",
        rr3_copies_note(copies),
        dev.rr3_dev_path(),
        dev.rr3_dev_offset()
    ))
}

/// 抓取 `--env <fw_env.config 路径>`（回滚链两子命令共用）。
fn rr3_env_arg(args: &[String]) -> Result<PathBuf, (String, ExitCode)> {
    rr3_val_arg(
        args,
        "--env",
        "rollback 必须显式传 --env <fw_env.config 路径>",
    )
    .map(PathBuf::from)
    .map_err(rr3_use)
}

/// env 区读改写的公共腿：读 → `f` 变更 → 整区写回；三步失败各自点名动作。
fn rr3_env_mutate(
    cfg: &Path,
    act: &str,
    f: impl FnOnce(&mut bootenv::Rr3BootEnv) -> Result<(), String>,
) -> Result<(bootenv::Rr3EnvDev, usize), (String, ExitCode)> {
    let (dev, copies) = rr3_env_dev(cfg)?;
    let mut env = bootenv::rr3_env_read(&dev)
        .map_err(|e| (format!("{act}：读 env 区失败：{e}"), ExitCode::from(1)))?;
    f(&mut env).map_err(|e| (format!("拒绝{act}：{e}"), ExitCode::from(1)))?;
    bootenv::rr3_env_write(&dev, &env)
        .map_err(|e| (format!("{act}：写回 env 区失败：{e}"), ExitCode::from(1)))?;
    Ok((dev, copies))
}

/// 读 fw_env.config 取主副本：首条即主副本，多副本的同步写不在本工具范围
/// （见 rr-ota::bootenv 模块头"不做"）；返回设备与 config 条数（供注记）。
pub(crate) fn rr3_env_dev(cfg: &Path) -> Result<(bootenv::Rr3EnvDev, usize), (String, ExitCode)> {
    let text = std::fs::read_to_string(cfg)
        .map_err(|e| (format!("读 env 配置 {cfg:?} 失败: {e}"), ExitCode::from(1)))?;
    let mut devs = bootenv::rr3_env_cfg_parse(&text)
        .map_err(|e| (format!("拒绝 env 配置 {cfg:?}：{e}"), ExitCode::from(1)))?;
    let copies = devs.len();
    Ok((devs.remove(0), copies))
}

/// 多副本注记：config 多于一条时明说只写首条主副本（不静默丢副本）。
fn rr3_copies_note(copies: usize) -> String {
    if copies > 1 {
        format!("（config 共 {copies} 条，只写首条主副本）")
    } else {
        String::new()
    }
}
