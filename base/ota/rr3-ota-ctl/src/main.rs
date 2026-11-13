//! rr3-ota-ctl —— OTA 槽位查看/切换与回滚链 CLI（0.0.3 批D 主机面实装，
//! 0.0.8 批B61 增回滚链两子命令）。
//!
//! 子命令：
//!   slots                                打印在册槽位与对合关系（读侧，不碰文件）
//!   switch <rr_a|rr_b> --bootconf P      改写引导配置 root=LABEL 做持久 A/B 切换
//!   factory            --bootconf P      根定位切 rr_factory 做一次性出厂引导
//!   rollback arm --limit N --altcmd C --env P
//!                                        U-Boot 回滚链上膛（四变量）
//!   rollback ok        --env P           回滚链解除（两变量）
//!
//! 判定与改写逻辑全在 `rr-ota`（`bootconf` / `bootenv`，base/runtime workspace 内，
//! 随七步门 ③ 全量跑测试）；本 CLI 只做参数解析与落盘调用，且**不缺省碰任何
//! 路径**——四个写命令必须显式给 `--bootconf` / `--env`。退出码：0 成功 /
//! 1 业务拒或 IO 失败 / 2 用法错。

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rr_ota::bootconf;
use rr_ota::bootenv;
use rr_ota::{Rr3Slot, RR3_OTA_STAGING_PARTITION};

const ALL_SLOTS: [Rr3Slot; 3] = [Rr3Slot::SlotA, Rr3Slot::SlotB, Rr3Slot::Factory];

const RR3_USAGE: &str = "\
用法:
  rr3-ota-ctl slots
  rr3-ota-ctl switch <rr_a|rr_b> --bootconf <extlinux.conf 路径>
  rr3-ota-ctl factory --bootconf <extlinux.conf 路径>
  rr3-ota-ctl rollback arm --limit <次数> --altcmd <备援命令> --env <fw_env.config 路径>
  rr3-ota-ctl rollback ok --env <fw_env.config 路径>

  slots         打印在册槽位与对合关系（读侧）
  switch        改写引导配置 root=LABEL，持久 A/B 切换
  factory       根定位切 rr_factory 做一次性出厂引导（rr_factory 分区本体不写）
  rollback arm  U-Boot 回滚链上膛（upgrade_available=1/bootcount=0/bootlimit/altbootcmd）
  rollback ok   回滚链解除（bootcount=0/upgrade_available=0；落点与上限保留）";

/// 用法错统一出口：文案后附用法全文（一次输出，不两次打印）。
fn rr3_use(e: String) -> (String, ExitCode) {
    (format!("{e}\n{RR3_USAGE}"), ExitCode::from(2))
}

/// 目标标签解析：在册标签映射槽位；不在册标签属用法错（exit 2）
fn rr3_slot_arg(label: &str) -> Result<Rr3Slot, String> {
    Rr3Slot::rr3_slot_from_label(label)
        .ok_or_else(|| format!("未知槽位标签 {label:?}（在册：rr_a / rr_b / rr_factory）"))
}

/// 分离式 `--<flag> <值>` 取值：整参缺席按 `why` 点名，有 flag 无值当场点名。
fn rr3_val_arg(args: &[String], flag: &str, why: &str) -> Result<String, String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == flag {
            return it.next().cloned().ok_or_else(|| format!("{flag} 缺参数值"));
        }
    }
    Err(why.to_owned())
}

/// 抓取 `--bootconf <路径>`。无缺省值——本 CLI 不缺省碰任何路径。
fn rr3_bootconf_arg(args: &[String]) -> Result<PathBuf, String> {
    rr3_val_arg(
        args,
        "--bootconf",
        "switch/factory 必须显式传 --bootconf <路径>",
    )
    .map(PathBuf::from)
}

/// switch：首参即目标槽，缺参 / 不在册标签都属用法错（exit 2）。
fn rr3_parse_switch(args: &[String]) -> Result<(Rr3Slot, PathBuf), (String, ExitCode)> {
    let label = args
        .first()
        .ok_or_else(|| rr3_use("switch 缺目标槽位参数".into()))?;
    let target = rr3_slot_arg(label).map_err(rr3_use)?;
    let path = rr3_bootconf_arg(args).map_err(rr3_use)?;
    Ok((target, path))
}

/// switch：读 → 判定改写 → 原子写回。业务拒（同槽幂等拒 / rr_factory 目标 /
/// 形状漂移）一律 (文案, 1)——文案全部来自 rr-ota::bootconf，本层不重抄语义。
fn rr3_cmd_switch(target: Rr3Slot, path: &Path) -> Result<String, (String, ExitCode)> {
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

/// 原子写回引导配置（失败文案统一）。
fn rr3_conf_store(path: &Path, out: &str) -> Result<(), (String, ExitCode)> {
    bootconf::rr3_bootconf_store(path, out)
        .map_err(|e| (format!("原子落盘失败: {e}"), ExitCode::from(1)))
}

/// rollback：arm 上膛 / ok 解除（`base/ota/partition-layout.md` 标准序第 4/6 步）。
fn rr3_cmd_rollback(args: &[String]) -> Result<String, (String, ExitCode)> {
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
fn rr3_cmd_rb_arm(args: &[String]) -> Result<String, (String, ExitCode)> {
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
fn rr3_cmd_rb_ok(args: &[String]) -> Result<String, (String, ExitCode)> {
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
fn rr3_env_dev(cfg: &Path) -> Result<(bootenv::Rr3EnvDev, usize), (String, ExitCode)> {
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

/// slots：只读打印，不碰任何路径。
fn rr3_print_slots() {
    for slot in ALL_SLOTS {
        println!(
            "{}  peer={}",
            slot.rr3_slot_label(),
            slot.rr3_slot_peer().rr3_slot_label()
        );
    }
    println!("{RR3_OTA_STAGING_PARTITION}  (模块级暂存，无对侧)");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("slots") => {
            rr3_print_slots();
            return ExitCode::SUCCESS;
        }
        Some("switch") => {
            rr3_parse_switch(&args[1..]).and_then(|(target, path)| rr3_cmd_switch(target, &path))
        }
        Some("factory") => rr3_bootconf_arg(&args[1..])
            .map_err(rr3_use)
            .and_then(|path| rr3_cmd_factory(&path)),
        Some("rollback") => rr3_cmd_rollback(&args[1..]),
        Some(other) => Err(rr3_use(format!(
            "未知子命令 {other:?}（在册：slots / switch / factory / rollback）"
        ))),
        None => Err(rr3_use(
            "缺子命令（在册：slots / switch / factory / rollback）".into(),
        )),
    };
    match outcome {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err((msg, code)) => {
            eprintln!("{msg}");
            code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 打印的标签必须与 rr-ota 常量逐一对应（否则文档与代码就分家了）
    #[test]
    fn slot_labels_come_from_rr_ota_constants() {
        assert_eq!(
            ALL_SLOTS.map(|s| s.rr3_slot_label()),
            ["rr_a", "rr_b", "rr_factory"]
        );
        assert_eq!(rr_ota::RR3_OTA_SLOT_NAMES, ["rr_a", "rr_b"]);
    }

    /// 写侧实装后，"恒拒"面由"无效目标拒"取代：不在册标签在用法层拒
    /// （exit 2）；业务规则（同槽 / factory 目标 / 形状漂移）归 rr-ota::bootconf
    #[test]
    fn slot_arg_maps_known_labels_and_rejects_unknown_ones() {
        assert_eq!(rr3_slot_arg("rr_a"), Ok(Rr3Slot::SlotA));
        assert_eq!(rr3_slot_arg("rr_b"), Ok(Rr3Slot::SlotB));
        assert_eq!(rr3_slot_arg("rr_factory"), Ok(Rr3Slot::Factory));
        let e = rr3_slot_arg("nonsense").unwrap_err();
        assert!(e.contains("未知槽位标签"), "不在册标签必须拒: {e}");
    }

    /// CLI 壳端到端：临时文件 读 → 改写 → 原子写回；业务拒（同槽）透传 rr-ota 文案
    #[test]
    fn switch_command_rewrites_file_and_rejects_same_slot() {
        let dir = std::env::temp_dir().join(format!("rr3-ota-ctl-{}-e2e", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("extlinux.conf");
        std::fs::write(
            &path,
            "label rockrobot3\n    append root=LABEL=rr_a rootwait\n",
        )
        .unwrap();

        let msg = rr3_cmd_switch(Rr3Slot::SlotB, &path).unwrap();
        assert!(msg.contains("rr_b"), "成功文案须点名新槽: {msg}");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("root=LABEL=rr_b"),
            "文件必须真的落了新标签"
        );

        // 同槽再切：业务拒（退出码 1），文件不被重写
        let err = rr3_cmd_switch(Rr3Slot::SlotB, &path).unwrap_err();
        assert_eq!(err.1, ExitCode::from(1), "业务拒退出码是 1");
        assert!(err.0.contains("拒绝切换"), "拒因须点名: {}", err.0);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("root=LABEL=rr_b"),
            "被拒的请求不得改动文件"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// --bootconf 是硬门槛：缺参必须在用法层拒，不得缺省碰任何路径
    #[test]
    fn bootconf_arg_is_mandatory_and_never_defaulted() {
        let args: Vec<String> = vec!["switch".into(), "rr_b".into()];
        assert!(rr3_bootconf_arg(&args).is_err(), "缺 --bootconf 必须拒");
        let args: Vec<String> = vec!["switch".into(), "rr_b".into(), "--bootconf".into()];
        assert!(
            rr3_bootconf_arg(&args).is_err(),
            "--bootconf 缺路径值必须拒"
        );
        let args: Vec<String> = vec![
            "switch".into(),
            "rr_b".into(),
            "--bootconf".into(),
            "/tmp/x.conf".into(),
        ];
        assert_eq!(
            rr3_bootconf_arg(&args).unwrap(),
            PathBuf::from("/tmp/x.conf")
        );
    }

    /// 回滚链夹具：合法 raw env（一条 bootcmd）+ 单行 fw_env.config。
    /// 普通文件顶替块设备（同 rr-ota::bootenv 测试口径），返回 (config 路径, 目录)。
    #[cfg(unix)]
    fn rb_fixture(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rr3-ota-rb-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dev = dir.join("env.bin");
        let mut env = bootenv::Rr3BootEnv::default();
        env.rr3_env_set("bootcmd", "run distro_bootcmd").unwrap();
        std::fs::write(&dev, env.rr3_env_render(128).unwrap()).unwrap();
        let cfg = dir.join("fw_env.config");
        std::fs::write(&cfg, format!("{} 0x0 0x80\n", dev.display())).unwrap();
        (cfg, dir)
    }

    /// arm 写四变量、ok 写两变量：都经真实 fs 读改写，读回逐键等值。
    #[cfg(unix)]
    #[test]
    fn rollback_arm_then_ok_cycles_through_env_file() {
        let (cfg, dir) = rb_fixture("cycle");
        let envp = vec!["--env".into(), cfg.to_string_lossy().into_owned()];
        let mut arm = vec!["--limit".into(), "3".into(), "--altcmd".into()];
        arm.push("run rr3_alt".into());
        arm.extend(envp.iter().cloned());

        let msg = rr3_cmd_rb_arm(&arm).unwrap();
        assert!(msg.contains("bootlimit=3"), "成功文案须点名上限: {msg}");
        assert!(
            msg.contains("upgrade_available=1"),
            "成功文案须点名上膛: {msg}"
        );

        let dev = rr3_env_dev(&cfg).unwrap().0;
        let env = bootenv::rr3_env_read(&dev).unwrap();
        assert_eq!(env.rr3_env_get("upgrade_available"), Some("1"));
        assert_eq!(env.rr3_env_get("bootcount"), Some("0"));
        assert_eq!(env.rr3_env_get("bootlimit"), Some("3"));
        assert_eq!(env.rr3_env_get("altbootcmd"), Some("run rr3_alt"));

        let msg = rr3_cmd_rb_ok(&envp).unwrap();
        assert!(msg.contains("upgrade_available=0"), "{msg}");
        let env = bootenv::rr3_env_read(&dev).unwrap();
        assert_eq!(env.rr3_env_get("upgrade_available"), Some("0"));
        assert_eq!(env.rr3_env_get("bootcount"), Some("0"));
        // 落点与上限保留（排障可见、下次上膛少写两笔）
        assert_eq!(env.rr3_env_get("bootlimit"), Some("3"));
        assert_eq!(env.rr3_env_get("altbootcmd"), Some("run rr3_alt"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 三条业务拒（altcmd 空 / limit=0 / env 缺位）+ 两条用法错（缺参 / 非数）
    /// 各自独立成立：文案点名对象，退出码分 1（业务）与 2（用法）。
    #[cfg(unix)]
    #[test]
    fn rollback_error_faces_are_named() {
        let (cfg, dir) = rb_fixture("err");
        let envp = cfg.to_string_lossy().into_owned();
        let mk = |limit: &str, alt: &str| -> Vec<String> {
            vec![
                "--limit".into(),
                limit.into(),
                "--altcmd".into(),
                alt.into(),
                "--env".into(),
                envp.clone(),
            ]
        };

        let (m, c) = rr3_cmd_rb_arm(&mk("3", "  ")).unwrap_err();
        assert_eq!(c, ExitCode::from(1), "空落点是业务拒");
        assert!(m.contains("拒绝上膛") && m.contains("altbootcmd"), "{m}");

        let (m, c) = rr3_cmd_rb_arm(&mk("0", "run x")).unwrap_err();
        assert_eq!(c, ExitCode::from(1), "零上限是业务拒");
        assert!(m.contains("bootlimit"), "{m}");

        let (m, c) = rr3_cmd_rb_arm(&mk("x", "run x")).unwrap_err();
        assert_eq!(c, ExitCode::from(2), "非数是用法错");
        assert!(m.contains("--limit") && m.contains("无符号整数"), "{m}");

        let no_env: Vec<String> = mk("3", "run x")[..4].to_vec();
        let (m, c) = rr3_cmd_rb_arm(&no_env).unwrap_err();
        assert_eq!(c, ExitCode::from(2), "缺 --env 是用法错");
        assert!(m.contains("--env"), "{m}");

        let missing = vec![
            "--limit".into(),
            "3".into(),
            "--altcmd".into(),
            "run x".into(),
            "--env".into(),
            dir.join("absent.config").to_string_lossy().into_owned(),
        ];
        let (m, c) = rr3_cmd_rb_arm(&missing).unwrap_err();
        assert_eq!(c, ExitCode::from(1), "配置读不到是业务面");
        assert!(m.contains("读 env 配置"), "{m}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
