//! 链路面与配网凭据存储面选择（NetworkManager / sim / 占位）。

use crate::provision_store::Rr3WifiStore;
use crate::Rr3UnavailableSource;

#[cfg(target_os = "linux")]
use super::selector::rr3_bindings_eff_log;
use super::*;

/// 按环境选网络链路读数面（与 [`rr3_audio_source`] 同一套规则）。linux
/// 分支走 NetworkManager 后端（批F5a⑤，`linux_link.rs` nmcli 子进程承载；
/// 配网/断链重连是 NM 自身行为，HAL 只如实报数），接口名来自生效绑定（描述符
/// `link_ifname`，`RR3_HAL_LINK_IFNAME` env 应急覆盖）；键缺失或合成失败 →
/// 占位件使用时以 `Unsupported` 回错（fail-closed）；显式 `linux` 但非 linux
/// 平台时同落占位件，不静默换 sim。
///
/// `nmcli_timeout`（0.0.5 批I N-2）：nmcli 子进程超时，由模块 config 的
/// `timeout_ms` 传入——没有代码缺省（同 `link_ifname` 的 fail-closed 口径），
/// sim/placeholder 分支忽略该值。最坏翻转延迟自此由配置管住，不再由子进程
/// 自由决定。
pub fn rr3_hal_link(
    // 非 linux 平台 linux 分支被剔除、sim/placeholder 分支不消费超时——参数在
    // 那些平台编译面上未使用是结构性的，不是疏漏（linux 上它进子进程守护）
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    nmcli_timeout: std::time::Duration,
) -> (Box<dyn crate::Rr3Link>, &'static str) {
    match rr3_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return rr3_bindings_eff_log("链路读数")
                .and_then(|b| {
                    b.link_ifname.map(|ifname| {
                        (
                            Box::new(crate::linux_link::Rr3LinuxNmLink::rr3_linux_link_new(
                                ifname,
                                "nmcli",
                                nmcli_timeout,
                            )) as Box<dyn crate::Rr3Link>,
                            RR3_BACKEND_LINUX,
                        )
                    })
                })
                .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER));
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_SIM => {
            let up = std::env::var(RR3_HAL_SIM_LINK_UP_ENV)
                .ok()
                .and_then(|v| rr3_sim_up_parse(&v))
                .unwrap_or(true);
            let ssid = std::env::var(RR3_HAL_SIM_LINK_SSID_ENV)
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| RR3_HAL_SIM_LINK_SSID_DEFAULT.to_string());
            let rssi = std::env::var(RR3_HAL_SIM_LINK_RSSI_ENV)
                .ok()
                .and_then(|v| rr3_sim_rssi_parse(&v))
                .unwrap_or(RR3_HAL_SIM_LINK_RSSI_DEFAULT);
            let dev = crate::sim::Rr3SimLink::rr3_sim_link_new(up, &ssid, rssi);
            (Box::new(dev), RR3_BACKEND_SIM)
        }
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}

/// 按环境选配网凭据存储面（0.0.5 批M M-γ，MC-6：凭据由后端承载持久化）。
/// linux 分支走 NetworkManager 连接配置后端（[`crate::provision_store::
/// Rr3NmWifiStore`]，nmcli 子进程承载），子进程超时由模块 config 的
/// `timeout_ms` 传入（与链路读数后端同一档，无代码缺省）；sim 分支内存档
/// （进程内存态即"在册"，sim 无"重启仍在"语义）；其余落占位（使用时以
/// `Unsupported` 回错，fail-closed 不静默换）。
///
/// 构造不触设备（选择函数口径）；凭据落盘策略归 NM 本身，件内不自写文件
/// ——`Rr3Provision` 只管态不管存（工单 MC-6 红线）。
pub fn rr3_hal_wifi_store(
    // 非 linux 平台 linux 分支被剔除、sim/placeholder 分支不消费超时——参数在
    // 那些平台编译面上未使用是结构性的，不是疏漏（linux 上它进子进程守护）
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    store_timeout: std::time::Duration,
) -> (Box<dyn Rr3WifiStore>, &'static str) {
    match rr3_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(target_os = "linux") => {
            #[cfg(target_os = "linux")]
            return (
                Box::new(crate::provision_store::Rr3NmWifiStore::rr3_nm_wifi_new(
                    "nmcli",
                    store_timeout,
                )),
                RR3_BACKEND_LINUX,
            );
            #[cfg(not(target_os = "linux"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        RR3_BACKEND_SIM => (
            Box::new(crate::provision_store::Rr3SimWifiStore::default()),
            RR3_BACKEND_SIM,
        ),
        _ => (
            Box::new(crate::provision_store::Rr3WifiStoreUnavailable),
            RR3_BACKEND_PLACEHOLDER,
        ),
    }
}
