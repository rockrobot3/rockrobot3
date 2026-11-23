// SPDX-License-Identifier: Apache-2.0
//! 配网凭据存储后端（0.0.5 批M M-γ，MC-6：凭据持久化）。
//!
//! 分工铁律（工单 MC-6 原文）：**station 侧凭据由后端承载持久化**（linux =
//! NetworkManager 连接配置），**件内不自写文件**；[`crate::provision_store`]
//! 的消费方（rr3-connect 的 [`Rr3Provision`](模块侧配网状态机)）只管态不管存。
//! 件内不出现落盘路径字面量——持久化策略归 NM 本身（连接配置目录由 NM
//! daemon 管辖），本后端只经 nmcli 子进程下单，不碰任何路径。
//!
//! 连接名锚 [`RR3_NM_PROVISION_CON`]：SSID 是用户数据、连接名是本后端的
//! 稳定锚（在册查询/更新/删除都以它定位；换网 = 同名 profile 改 ssid/psk，
//! 不为每个 SSID 建连接——那会让"在册吗"变成全表扫）。
//!
//! nmcli 序列（配网是 runbook §5.5.2 件 4 既定口径，全部落在 nmcli 上）：
//! - 在册查询：`nmcli -t -f NAME con show` 逐行找锚名；
//! - connect：在册 → `con modify <锚> 802-11-wireless.ssid S …psk P …key-mgmt wpa-psk`；
//!   不在册 → `con add type wifi con-name <锚> 802-11-wireless.ssid S …`——
//!   add/modify 同属性路径，key-mgmt 显式 wpa-psk（connect 参数语义恒 WPA，
//!   密码 ≥8 位由模块侧 legacy 语义裁决，承载层不做策略重复校验）；
//! - forget：在册 → `con delete <锚>`；不在册 → 直接 Ok（未连先忘幂等回 ap
//!   是裁定语义，delete 一个不存在的 profile 不是错误）；
//! - 在册 SSID 读回：`nmcli -t -f 802-11-wireless.ssid con show <锚>`。
//!
//! psk 经 argv 传递：与 `nmcli dev wifi connect … password` 既定口径一致
//! （同机用户可经 ps 观测 argv 是 nmcli 全家共有的暴露面），passwd-file
//! 收敛留待立项时随 runbook 改口径，此处不另发明。
//!
//! 编译门 `cfg(any(target_os = "linux", test))`：linux 全编译；macOS test 面
//! 编译——"connect → 重启 → 仍在册"判据要求**主机可测**：假 nmcli 脚本以
//! `NMCONF_HOME` 为连接配置目录模拟 NM 的持久化语义（脚本自 export，测试
//! 进程零 set_var 竞态），同夹具下重新构造后端 = 重启后查在册。
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use crate::nmcli_support::{rr3_nm_field, rr3_nm_run, rr3_nm_unescape};
use crate::{Rr3ErrText, Rr3HalError, Rr3SsidText};

/// 配网连接的固定 profile 名（本后端的在册锚，见模块 doc）
pub const RR3_NM_PROVISION_CON: &str = "rr3-provision";

/// 配网凭据形制校验（fail-closed）：nmcli 用 GOption 解析 argv，值位置以 `-`
/// 开头的串会被当选项——这是本后端唯一"用户数据进 argv"的面，必须在入口挡。
/// 其余规则取 802.11/WPA2-PSK 物理上限（SSID ≤32 字节、psk ≤63 字符）与
/// 控制字符拒（argv 里 NUL 会截断）。密码强度（≥8）归模块侧 legacy 语义，
/// 本层不重复策略。
fn rr3_wifi_cred_check(ssid: &str, pass: &str) -> Result<(), Rr3HalError> {
    let bad = |why: &str| Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(why)));
    if ssid.is_empty() {
        return bad("SSID 为空（配网凭据拒存）");
    }
    if ssid.len() > 32 {
        return bad("SSID 超 32 字节（802.11 上限，配网凭据拒存）");
    }
    if pass.is_empty() {
        return bad("密码为空（配网凭据拒存）");
    }
    if pass.chars().count() > 63 {
        return bad("密码超 63 字符（WPA2-PSK 上限，配网凭据拒存）");
    }
    for (what, v) in [("SSID", ssid), ("密码", pass)] {
        if v.starts_with('-') {
            return bad(&format!("{what} 以 '-' 开头会被 nmcli 当选项（拒存）"));
        }
        if v.chars().any(|c| c.is_control()) {
            return bad(&format!("{what} 含控制字符（拒存）"));
        }
    }
    Ok(())
}

/// 配网凭据存储面（MC-6）：三动作——connect 落盘、forget 真删、在册查询。
/// 全部由后端承载（linux = NM 连接配置）；实现方构造不触设备（选择函数
/// 口径），首次调用才起子进程。
pub trait Rr3WifiStore: Send + Sync {
    /// 存凭据进持久化承载（在册则更新，不在册则新建；调用方在成功后自行
    /// 转移配网态——失败时状态机不动，fail-closed）
    fn rr3_wifi_connect(&self, ssid: &str, pass: &str) -> Result<(), Rr3HalError>;
    /// 删除在册凭据（不在册 = 幂等 Ok；删除失败显式回错——"forget 后配置
    /// 真消失"要求失败不得静默装成功）
    fn rr3_wifi_forget(&self) -> Result<(), Rr3HalError>;
    /// 在册 SSID（None = 不在册）：重启后模块据此恢复配网态
    fn rr3_wifi_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError>;
}

/// linux NetworkManager 凭据存储（[`Rr3WifiStore`] 的 linux 后端，nmcli 子进程
/// 承载）。机制本体（spawn + 超时守护 + 管道读）与链路读数后端共用
/// [`crate::nmcli_support`] 一份真值。
pub struct Rr3NmWifiStore {
    nmcli: PathBuf,
    nmcli_timeout: Duration,
}

impl Rr3NmWifiStore {
    /// 指定 nmcli 程序路径与子进程超时构造（选择函数从模块 config 的
    /// `timeout_ms` 传入超时——与链路读数后端同一档，没有代码缺省；测试传
    /// 假脚本绝对路径注入编排，零全局环境污染）
    pub fn rr3_nm_wifi_new(nmcli: impl Into<PathBuf>, nmcli_timeout: Duration) -> Self {
        Self {
            nmcli: nmcli.into(),
            nmcli_timeout,
        }
    }

    /// 在册吗：`-t -f NAME con show` 逐行找锚名（NAME 值不含 terse 转义——
    /// 锚名是本后端常量，无冒号无反斜杠）
    fn in_register(&self) -> Result<bool, Rr3HalError> {
        let names = rr3_nm_run(
            &self.nmcli,
            &["-t", "-f", "NAME", "con", "show"],
            self.nmcli_timeout,
        )?;
        Ok(names.lines().any(|l| l.trim() == RR3_NM_PROVISION_CON))
    }

    /// add/modify 公共属性段：ssid + psk + key-mgmt（add 与 modify 同形状，
    /// 假脚本与真 nmcli 同一份属性解析）。返回 owned 串——argv 引用面在
    /// 调用点收敛（&str 混 'static 字面量与调用方借用会拉出多生命周期）。
    fn upsert_args(&self, verb: &str, ssid: &str, pass: &str) -> Vec<String> {
        let mut args: Vec<String> = vec!["con".into(), verb.into()];
        if verb == "modify" {
            args.push(RR3_NM_PROVISION_CON.into());
        } else {
            args.extend(["type", "wifi", "con-name"].iter().map(|s| s.to_string()));
            args.push(RR3_NM_PROVISION_CON.into());
        }
        args.extend(
            [
                "802-11-wireless.ssid",
                ssid,
                "802-11-wireless-security.psk",
                pass,
                "802-11-wireless-security.key-mgmt",
                "wpa-psk",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        args
    }
}

impl Rr3WifiStore for Rr3NmWifiStore {
    fn rr3_wifi_connect(&self, ssid: &str, pass: &str) -> Result<(), Rr3HalError> {
        rr3_wifi_cred_check(ssid, pass)?;
        let verb = if self.in_register()? { "modify" } else { "add" };
        let args = self.upsert_args(verb, ssid, pass);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        rr3_nm_run(&self.nmcli, &refs, self.nmcli_timeout).map(|_| ())
    }

    fn rr3_wifi_forget(&self) -> Result<(), Rr3HalError> {
        if !self.in_register()? {
            return Ok(()); // 未连先忘幂等：不在册时连 delete 都不用发
        }
        rr3_nm_run(
            &self.nmcli,
            &["con", "delete", RR3_NM_PROVISION_CON],
            self.nmcli_timeout,
        )
        .map(|_| ())
    }

    fn rr3_wifi_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError> {
        if !self.in_register()? {
            return Ok(None);
        }
        let out = rr3_nm_run(
            &self.nmcli,
            &[
                "-t",
                "-f",
                "802-11-wireless.ssid",
                "con",
                "show",
                RR3_NM_PROVISION_CON,
            ],
            self.nmcli_timeout,
        )?;
        // terse 单字段行 `802-11-wireless.ssid:<值>`（字段提取与链路读数共用
        // nmcli_support 一份真值）；NM 空值 `--`/空串 → None
        Ok(rr3_nm_ssid_of(&out))
    }
}

/// 在册 SSID 取值：NM 空值 `--`/空串 → None，其余反转义收编定长文本。
fn rr3_nm_ssid_of(out: &str) -> Option<Rr3SsidText> {
    let v = rr3_nm_field(out, "802-11-wireless.ssid")
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "--");
    v.map(|s| Rr3SsidText::rr3_ssid_text_new(&rr3_nm_unescape(s)))
}

/// sim 凭据存储（内存档）：sim 是显式档不是持久化承载——进程内存态即"在册"，
/// 进程重启即失（sim 档无"重启仍在"语义，如实，不装）。
#[derive(Default)]
pub struct Rr3SimWifiStore {
    ssid: Mutex<Option<String>>,
}

impl Rr3WifiStore for Rr3SimWifiStore {
    fn rr3_wifi_connect(&self, ssid: &str, _pass: &str) -> Result<(), Rr3HalError> {
        *self.ssid.lock().unwrap_or_else(|e| e.into_inner()) = Some(ssid.to_string());
        Ok(())
    }

    fn rr3_wifi_forget(&self) -> Result<(), Rr3HalError> {
        *self.ssid.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }

    fn rr3_wifi_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError> {
        Ok(self
            .ssid
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .map(|s| Rr3SsidText::rr3_ssid_text_new(&s)))
    }
}

/// 占位凭据存储：能力缺失显式回错（同 [`Rr3UnavailableSource`](rr-hal-api) 口径
/// ——不静默换后端，选择面落 placeholder 时使用处 Unsupported 点名）。
pub struct Rr3WifiStoreUnavailable;

impl Rr3WifiStore for Rr3WifiStoreUnavailable {
    fn rr3_wifi_connect(&self, _ssid: &str, _pass: &str) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/wifi-store"))
    }
    fn rr3_wifi_forget(&self) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/wifi-store"))
    }
    fn rr3_wifi_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/wifi-store"))
    }
}

#[cfg(test)]
mod tests;
