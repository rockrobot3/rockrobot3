//! linux（NetworkManager）链路读数后端：R18 扩围（0.0.6 方案 §5.4，判据 13）。
//!
//! 编译门是单重的：`target_os = "linux"`。承载选 **nmcli 子进程轮询**而非
//! D-Bus 直连——nmcli 是方案 runbook §5.5.2 件 4 的既定口径（配网/对拍全套
//! 命令都落在它上），std::process::Command 零新依赖，不破"libc 声明壳纯
//! Rust、musl 静态"纪律（D-Bus 需引 zbus 级大库且 async 耦合）；轮询节拍由
//! 模块侧 connect 的 `link_report_period_ms` 控制，HAL 每次 `state()` 现读
//! 现报，不持 fd 无 ensure（与 pwm/gpio/power 的设备 fd 面不同构——这里是
//! 进程面不是设备面）。
//!
//! 一轮读数两次调用（terse 模式 `-t`，冒号分隔、值内 `:`/`\` 转义）：
//! 1. `nmcli -t -f GENERAL.STATE,GENERAL.CONNECTION,GENERAL.TYPE dev show
//!    <ifname>` —— STATE 数值 100 即 up；CONNECTION 是活动连接 profile 名
//!    （可改名，仅作 SSID 回落）；TYPE 区分 wifi / 有线（非 wifi 无 SSID/
//!    RSSI 语义，如实 None）。
//! 2. up 且 wifi 时：`nmcli -t -f IN-USE,SSID,SIGNAL dev wifi list ifname
//!    <ifname> --rescan no` —— 取 `*` 行拿真实广播 SSID 与信号质量。SIGNAL
//!    是 0–100 质量百分比不是 dBm，按经验线性 dBm ≈ q/2 − 100 折算（NM 不
//!    出 dBm，折算是近似面，注释钉死）；`--rescan no` 只读缓存不触发扫描
//!    （扫描是秒级阻塞，不许进周期拍）。缓存空/调用失败 → SSID 回落
//!    CONNECTION、RSSI None，不编造。
//!
//! 分工口径与 sim 同一条（sim.rs 模块注释）：NM 承载配网与断链重连
//! （autoconnect 是 NM 自身行为），模块侧只读翻转上报；HAL 只如实报数。
//! 断链语义对齐 sim：`up=false` 时 ssid/rssi 强制 `None`——真设备断链后
//! 拿不到这些读数，模块侧不许从"down 还带着 ssid"里发明语义。

use std::path::PathBuf;
use std::process::Command;

use async_trait::async_trait;

use crate::{Rr3HalError, Rr3Link, Rr3LinkState};

/// terse 值的反转义：NM terse 模式把值里的 `:` 与 `\` 分别转义成 `\:`
/// 与 `\\`（否则冒号分隔表无法承载含冒号的 SSID）。
fn rr3_nm_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(':') => out.push(':'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push(other);
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// terse 输出里取一个字段值：逐行找 `key:` 前缀，冒号后整段返回
/// （值内可能有转义冒号，不能再 split；取值后调用方自行反转义）。
fn rr3_nm_field<'a>(out: &'a str, key: &str) -> Option<&'a str> {
    out.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
}

/// GENERAL.STATE 值 → 在链：取首个空格前的数字段 == 100（NM terse 形如
/// `100 (connected)` / `30 (disconnected)`）。解析失败一律 false——不认的
/// 状态不编造成在线。
fn rr3_nm_state_up(value: &str) -> bool {
    value
        .split_whitespace()
        .next()
        .and_then(|t| t.parse::<u8>().ok())
        .map(|n| n == 100)
        .unwrap_or(false)
}

/// GENERAL.CONNECTION 值 → SSID 候选：`--`（NM 空值）或空串 → None。
fn rr3_nm_ssid_opt(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() || v == "--" {
        None
    } else {
        Some(rr3_nm_unescape(v))
    }
}

/// `dev wifi list` 一行 → (ssid, 质量百分比)。IN-USE 是行首段（`*` 表示
/// 当前连接的 AP），SIGNAL 是行尾数字段，中间整段是 SSID（可能含转义冒号，
/// 两端夹逼中间不拆）。IN-USE 非 `*` 返回 None（调用方逐行找当前 AP）。
fn rr3_nm_wifi_row(line: &str) -> Option<(String, u8)> {
    let (in_use, rest) = line.split_once(':')?;
    if in_use != "*" {
        return None;
    }
    let (ssid_raw, sig_raw) = rest.rsplit_once(':')?;
    let q = sig_raw.trim().parse::<u8>().ok()?;
    Some((rr3_nm_unescape(ssid_raw), q))
}

/// 质量百分比 → dBm（经验线性 dBm ≈ q/2 − 100；NM 不出 dBm，近似折算注释
/// 钉死）。q=0（无信号读数）→ None，不报假的 −100。
fn rr3_nm_signal_to_dbm(q: u8) -> Option<i8> {
    if q == 0 {
        return None;
    }
    let dbm = (q as i32 / 2) - 100;
    Some(dbm.clamp(-100, -1) as i8)
}

/// linux NetworkManager 链路读数（[`crate::Rr3Link`] 的 linux 后端）。
///
/// 选择函数（[`crate::backend::rr3_hal_link`]）不触设备：构造只记接口名与
/// nmcli 程序路径，首次 `state()` 才起子进程（构造零副作用同 pwm/gpio/power
/// 后端口径）。`nmcli` 程序路径是构造参数：缺省 `nmcli` 走 PATH，测试传假
/// 脚本绝对路径注入编排，零全局环境污染。
pub struct Rr3LinuxNmLink {
    ifname: String,
    nmcli: PathBuf,
}

impl Rr3LinuxNmLink {
    /// 板上缺省构造：接口取 `RR3_HAL_LINK_IFNAME`（缺省 `wlan0`——树莓派
    /// 板载 WiFi 常见名，runbook 件 4 口径），nmcli 程序 = `nmcli`（PATH）。
    pub fn rr3_linux_link_default() -> Self {
        let ifname = std::env::var(crate::backend::RR3_HAL_LINK_IFNAME_ENV)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "wlan0".to_string());
        Self::rr3_linux_link_new(ifname, "nmcli")
    }

    /// 指定接口与 nmcli 程序路径构造（测试传假脚本走这里）
    pub fn rr3_linux_link_new(ifname: impl Into<String>, nmcli: impl Into<PathBuf>) -> Self {
        Self {
            ifname: ifname.into(),
            nmcli: nmcli.into(),
        }
    }

    /// 起一次 nmcli 子进程（terse 模式固定），非零退出/不可执行显式回错。
    fn nm(&self, args: &[&str]) -> Result<String, Rr3HalError> {
        let out = Command::new(&self.nmcli).args(args).output().map_err(|e| {
            Rr3HalError::InvalidParam(format!(
                "起 nmcli 失败: {e}（NetworkManager/nmcli 不在位？）"
            ))
        })?;
        if !out.status.success() {
            return Err(Rr3HalError::InvalidParam(format!(
                "nmcli 退出非零（{:?}）: {}",
                args,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[async_trait]
impl Rr3Link for Rr3LinuxNmLink {
    async fn state(&self) -> Result<Rr3LinkState, Rr3HalError> {
        let show = self.nm(&[
            "-t",
            "-f",
            "GENERAL.STATE,GENERAL.CONNECTION,GENERAL.TYPE",
            "dev",
            "show",
            &self.ifname,
        ])?;
        let up = rr3_nm_field(&show, "GENERAL.STATE")
            .map(rr3_nm_state_up)
            .unwrap_or(false);
        // 断链强制 None（同 sim 断链语义）；非 wifi 设备无 SSID/RSSI 词汇
        let mut ssid = if up {
            rr3_nm_field(&show, "GENERAL.CONNECTION").and_then(rr3_nm_ssid_opt)
        } else {
            None
        };
        let mut rssi_dbm = None;
        let is_wifi = rr3_nm_field(&show, "GENERAL.TYPE") == Some("wifi");
        if up && is_wifi {
            // 只读缓存不触发扫描（扫描秒级阻塞，不进周期拍）；失败回落
            // CONNECTION 的 SSID、RSSI None，不编造
            if let Ok(list) = self.nm(&[
                "-t",
                "-f",
                "IN-USE,SSID,SIGNAL",
                "dev",
                "wifi",
                "list",
                "ifname",
                &self.ifname,
                "--rescan",
                "no",
            ]) {
                if let Some((pick_ssid, q)) = list.lines().find_map(rr3_nm_wifi_row) {
                    ssid = Some(pick_ssid);
                    rssi_dbm = rr3_nm_signal_to_dbm(q);
                }
            }
        }
        Ok(Rr3LinkState {
            up,
            ssid,
            rssi_dbm,
        })
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::rr3_block_on;

    /// terse 反转义：`\:`→`:`、`\\`→`\`、无转义原样、孤立反斜杠吞掉不炸
    #[test]
    fn nm_unescape_roundtrip() {
        assert_eq!(rr3_nm_unescape("My\\:WiFi"), "My:WiFi");
        assert_eq!(rr3_nm_unescape("a\\\\b"), "a\\b");
        assert_eq!(rr3_nm_unescape("plain"), "plain");
        assert_eq!(rr3_nm_unescape("tail\\"), "tail");
    }

    /// 字段提取：`KEY:value` 前缀行取值；值内转义冒号不拆（整段返回）；
    /// 无此键 None
    #[test]
    fn nm_field_extracts_value_keeps_escape() {
        let out = "GENERAL.STATE:100 (connected)\nGENERAL.CONNECTION:Home\\:5G\n";
        assert_eq!(rr3_nm_field(out, "GENERAL.STATE"), Some("100 (connected)"));
        assert_eq!(rr3_nm_field(out, "GENERAL.CONNECTION"), Some("Home\\:5G"));
        assert_eq!(rr3_nm_field(out, "GENERAL.TYPE"), None);
    }

    /// STATE 裁决：100 在链，其余（30/40/50/垃圾）一律不在——不认不编造
    #[test]
    fn nm_state_up_only_full_connectivity() {
        assert!(rr3_nm_state_up("100 (connected)"));
        assert!(rr3_nm_state_up("100"));
        assert!(!rr3_nm_state_up("30 (disconnected)"));
        assert!(!rr3_nm_state_up("40 (connecting)"));
        assert!(!rr3_nm_state_up("xx"));
        assert!(!rr3_nm_state_up(""));
    }

    /// SSID 候选裁决：NM 空值 `--` 与空串 → None，其余反转义返回
    #[test]
    fn nm_ssid_rejects_nm_empty_marker() {
        assert_eq!(rr3_nm_ssid_opt("--"), None);
        assert_eq!(rr3_nm_ssid_opt(""), None);
        assert_eq!(rr3_nm_ssid_opt(" A "), Some("A".to_string()), "trim 空白");
        assert_eq!(rr3_nm_ssid_opt("a\\:b"), Some("a:b".to_string()));
    }

    /// wifi 行两端夹逼：IN-USE `*` 才选中，SIGNAL 取行尾数字，SSID 中段
    /// 保留转义反转义；`--rescan` 不会让非选中行混入
    #[test]
    fn nm_wifi_row_picks_star_line_only() {
        assert_eq!(rr3_nm_wifi_row("*:Home:87"), Some(("Home".into(), 87)));
        assert_eq!(rr3_nm_wifi_row("*:Home\\:5G:72"), Some(("Home:5G".into(), 72)));
        assert_eq!(rr3_nm_wifi_row(":Neighbour:40"), None, "非 `*` 行不选");
        assert_eq!(rr3_nm_wifi_row("*:NoSig:0"), Some(("NoSig".into(), 0)));
        assert_eq!(rr3_nm_wifi_row("*:Garbage:x"), None, "SIGNAL 非数字不认");
        assert_eq!(rr3_nm_wifi_row("*:NoSignalField"), None, "缺信号段不认");
    }

    /// 质量百分比 → dBm：q=0 → None（不报假 −100），经验线性 q/2−100，
    /// 域钉在 −100..=−1
    #[test]
    fn nm_signal_dbm_zero_means_unknown() {
        assert_eq!(rr3_nm_signal_to_dbm(0), None);
        assert_eq!(rr3_nm_signal_to_dbm(100), Some(-50));
        assert_eq!(rr3_nm_signal_to_dbm(87), Some(-56));
        assert_eq!(rr3_nm_signal_to_dbm(1), Some(-100));
        assert_eq!(rr3_nm_signal_to_dbm(255), Some(-1), "u8 上界 clamp 锚");
    }

    /// 假 nmcli 编排（temp 脚本注入，零 PATH 污染）：dev show + wifi list
    /// 两次调用读数组装成 Rr3LinkState——SSID 取 IN-USE 行（广播真名），
    /// RSSI 换算落位
    #[test]
    fn state_assembles_from_two_nmcli_calls() {
        let dir = std::env::temp_dir().join(format!("rr3-hal-link-{}-ok", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-nmcli");
        std::fs::write(
            &script,
            "#!/bin/sh\n\
             for a in \"$@\"; do\n\
             \x20 if [ \"$a\" = \"show\" ]; then\n\
             \x20   printf 'GENERAL.STATE:100 (connected)\\nGENERAL.CONNECTION:rr3-home\\nGENERAL.TYPE:wifi\\n'; exit 0\n\
             \x20 fi\n\
             done\n\
             printf '*:Home\\:5G:87\\n:Neighbour:40\\n'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let dev = Rr3LinuxNmLink::rr3_linux_link_new("wlan0", &script);
        let st = rr3_block_on(dev.state()).unwrap();
        assert!(st.up);
        assert_eq!(st.ssid.as_deref(), Some("Home:5G"), "SSID 取 IN-USE 行真名");
        assert_eq!(st.rssi_dbm, Some(-56));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 假 nmcli 退出非零：显式回错点名参数，不静默编造状态
    #[test]
    fn nmcli_nonzero_exit_fails_loudly() {
        let dir = std::env::temp_dir().join(format!("rr3-hal-link-{}-err", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-nmcli-fail");
        std::fs::write(&script, "#!/bin/sh\necho no wifi dev >&2\nexit 4\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let dev = Rr3LinuxNmLink::rr3_linux_link_new("wlan0", &script);
        let err = rr3_block_on(dev.state()).unwrap_err();
        assert!(
            matches!(err, Rr3HalError::InvalidParam(ref m) if m.contains("nmcli")),
            "错误要点名 nmcli: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
