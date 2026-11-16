//! CLI 共用参数面：用法错出口、用法全文与分离式 `--flag value` 取值。
//!
//! 各子命令只做自己的形制判据；"缺参/未知子命令"统一走 [`rr3_use`] 的
//! 退出码 2，业务/校验失败走调用方的退出码 1。用法全文是单点，子命令不另抄。

use std::process::ExitCode;

pub(crate) const RR3_USAGE: &str = "\
用法:
  rr3-ota-ctl slots
  rr3-ota-ctl switch <rr_a|rr_b> --bootconf <extlinux.conf 路径>
  rr3-ota-ctl factory --bootconf <extlinux.conf 路径>
  rr3-ota-ctl rollback arm --limit <次数> --altcmd <备援命令> --env <fw_env.config 路径>
  rr3-ota-ctl rollback ok --env <fw_env.config 路径>
  rr3-ota-ctl keypub --key-file <种子文件> [--out-pub <公钥文件>]
  rr3-ota-ctl manifest --image <镜像> --platform <平台> --board <板型> --version <版本>
                       --key-file <种子文件> --out <manifest 路径>
  rr3-ota-ctl sign --manifest <manifest> --key-file <种子文件> --sig-out <签名文件>
  rr3-ota-ctl verify --manifest <manifest> --sig <签名文件> --pub <公钥文件>
                      [--image <镜像>] [--platform <平台>] [--board <板型>]
                      [--current-version <版本>] [--allow-downgrade]
  rr3-ota-ctl install --manifest <manifest> --sig <签名文件> --pub <公钥文件>
                      --image <镜像> --target <普通文件> --confirm
                      [--platform <平台>] [--board <板型>]
                      [--current-version <版本>] [--allow-downgrade]

  slots         打印在册槽位与对合关系（读侧）
  switch        改写引导配置 root=LABEL，持久 A/B 切换
  factory       根定位切 rr_factory 做一次性出厂引导（rr_factory 分区本体不写）
  rollback arm  U-Boot 回滚链上膛（upgrade_available=1/bootcount=0/bootlimit/altbootcmd）
  rollback ok   回滚链解除（bootcount=0/upgrade_available=0；落点与上限保留）
  keypub        从私钥种子派生公钥与 key_id（私钥不输出）
  manifest      生成严格行式 manifest（sha256/bytes/key_id 由工具计算）
  sign          对 manifest 原始字节做 Ed25519 签名
  verify        严格验签 + 目标兼容 + 镜像摘要核对（不写槽位）
  install       验签通过后原子装到普通文件 staging 目标（块设备写入归 M6）";

/// 用法错统一出口：文案后附用法全文（一次输出，不两次打印）。
pub(crate) fn rr3_use(e: String) -> (String, ExitCode) {
    (format!("{e}\n{RR3_USAGE}"), ExitCode::from(2))
}

/// 分离式 `--<flag> <值>` 取值：整参缺席按 `why` 点名，有 flag 无值当场点名。
pub(crate) fn rr3_val_arg(args: &[String], flag: &str, why: &str) -> Result<String, String> {
    match rr3_opt_arg(args, flag)? {
        Some(v) => Ok(v),
        None => Err(why.to_owned()),
    }
}

/// 可选 `--<flag> <值>`：整参缺席 = `None`；有 flag 无值 = 当场点名。
pub(crate) fn rr3_opt_arg(args: &[String], flag: &str) -> Result<Option<String>, String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == flag {
            return it
                .next()
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("{flag} 缺参数值"));
        }
    }
    Ok(None)
}

/// 布尔开关：出现即真（重复出现不另计）。
pub(crate) fn rr3_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}
