//! deb 面：control/postinst 文本与 dpkg-deb 出包。

use super::*;

/// 发版维护者标识缺省值：`.invalid` 是 RFC 2606 保留域，源码仓不写个人/单位
/// 身份；发行方用 [`RR3_DEB_MAINTAINER_ENV`] 覆盖（debian/control 的必填字段
/// 不能空，缺省必须是可解析的合法地址）。
pub(super) const RR3_DEB_MAINTAINER: &str =
    "RockRobot3 Maintainers <maintainers@rockrobot3.invalid>";
/// 维护者覆盖环境变量（发版流水线注入真实身份，源码零泄漏）。
pub(super) const RR3_DEB_MAINTAINER_ENV: &str = "RR3_DEB_MAINTAINER";

/// control 文本（字段序按 dpkg 惯例）。
pub(super) fn rr3_control_text(arch: &str) -> String {
    let maintainer = std::env::var(RR3_DEB_MAINTAINER_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RR3_DEB_MAINTAINER.to_string());
    format!(
        "Package: {RR3_DEB_PACKAGE}\nVersion: {}\nSection: net\nPriority: optional\nArchitecture: {arch}\nMaintainer: {maintainer}\nDescription: RockRobot3 运行时 payload（rrd + 模块 + systemd 单元，发行版快路轨）\n",
        rr3_deb_version()
    )
}

/// postinst：systemd 在位才 enable——容器冒烟里 dpkg -i 时没有 systemd，不得因此失败。
/// enable 不带 --now：启动时机归 systemd 策略，装机脚本不擅自拉起守护。
pub(super) fn rr3_postinst_text() -> &'static str {
    "#!/bin/sh\n# RockRobot3 deb 后置脚本（rr3-pack 生成）\n\
     # 运维计数持久化目录（RR3_DATA_DIR，0.0.6 批E' R3：unit 注入同名环境变量，\n\
     # 缺目录时 rrd 回写建 tmp 失败、boots 每次开机重置）\n\
     mkdir -p /var/lib/rockrobot3\nchmod 755 /var/lib/rockrobot3\nchown root:root /var/lib/rockrobot3\n\
     # ctl socket 属主组（0.0.6 批F5b R12，ADR-0010 权限模型增补）：rrd bind 后\n\
     # chmod 0o660 + chgrp 到本组，运维用户装机现场入组（deb 不动用户）\n\
     addgroup --system rockrobot3 2>/dev/null || true\n\
     if [ -d /run/systemd/system ]; then\n    systemctl enable rrd.service\nfi\n"
}

/// postinst 必须可执行否则 dpkg 不跑它。staging 在 Unix 上落，权限语义也只对 Unix 有意义。
#[cfg(unix)]
pub(super) fn rr3_make_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
}
#[cfg(not(unix))]
pub(super) fn rr3_make_executable(_p: &Path) {}

/// dpkg-deb 出包。缺席给可操作的指引（macOS 本机没有 dpkg 是常态，不是故障）。
pub(super) fn rr3_build_deb(stage: &Path, out: &Path) -> Result<(), String> {
    let outp = rr3_dpkg_build(stage, out)?;
    if !outp.status.success() {
        return Err(rr3_deb_fail(&outp.stderr));
    }
    Ok(())
}

/// dpkg-deb 失败文案（stderr 原样带回）。
fn rr3_deb_fail(stderr: &[u8]) -> String {
    format!("dpkg-deb 失败: {}", String::from_utf8_lossy(stderr))
}

/// 可用性检查 + 调 dpkg-deb 出包（失败文案带 stderr）。
fn rr3_dpkg_build(stage: &Path, out: &Path) -> Result<std::process::Output, String> {
    rr3_dpkg_ready()?;
    Command::new("dpkg-deb")
        .args(["--root-owner-group", "--build"])
        .arg(stage)
        .arg(out)
        .output()
        .map_err(|e| format!("调 dpkg-deb 失败: {e}"))
}

/// dpkg-deb 可用性（缺席给可操作指引）。
fn rr3_dpkg_ready() -> Result<(), String> {
    if Command::new("dpkg-deb").arg("--version").output().is_err() {
        return Err(
            "dpkg-deb 不可用（本机多半没装 dpkg）：用 --stage-only 只出 staging 树，真装交给 CI distro-smoke 容器"
                .to_string(),
        );
    }
    Ok(())
}
