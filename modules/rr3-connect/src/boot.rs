//! rr3-connect 接入层（0.0.8 W-A 批2 拆分，ADR-0017 五层表接入层）。
//!
//! config schema 解析（自原 composite.rs 迁入：TOML 下钻、缺字段点名、bt 段
//! 缺段回落——文案是装配观测面，逐字不变）＋ HAL 链路面选择与开机探活编排＋
//! 配网凭据存储编排（Unsupported / 在册查询失败即 Err 上抛：显式回错不静默
//! 带病常驻，也不给"没有链路读数还常驻"的假活）。本层零业务 match——策略量
//! 经 config 数据化注入（周期/超时/口令下限/bt 开机初值），机制件零策略载荷。
//! config 路径解析（RR3_CONNECT_CONFIG_ENV 覆盖 + manifest 同目录缺省）已随
//! 批0 并入 rr3mod `load_config_text` 单点，本层不再持有路径段。

use std::time::Duration;

use rr_hal::backend::{rr3_hal_link, rr3_hal_wifi_store};
use rr_hal::provision_store::Rr3WifiStore;
use rr_hal_api::Rr3Link;

use crate::wifi::{Rr3LinkCfg, Rr3Provision};

/// HAL 链路面选择 + 开机探活：首读即探活（Unsupported / 读数失败显式回错），
/// 「链路面就绪」回显归本编排（方案 §4.2 映射表字面）。nmcli 子进程超时由
/// config timeout_ms 传入（0.0.5 批I N-2：最坏翻转延迟由配置管住，不再由子
/// 进程自由决定）。返回二元——句柄半、后端名（"链路读数失败（{backend}）"
/// 文案复用）。
pub(crate) fn rr3_connect_probe(
    timeout_ms: u64,
) -> Result<(Box<dyn Rr3Link>, &'static str), String> {
    let (dev, backend) = rr3_hal_link(Duration::from_millis(timeout_ms));
    let first = dev
        .state()
        .map_err(|e| format!("链路面探活失败({backend}): {e}"))?;
    eprintln!(
        "链路面就绪: {backend} up={} ssid={:?} rssi={:?}",
        first.up, first.ssid, first.rssi_dbm
    );
    Ok((dev, backend))
}

/// HAL 配网凭据存储选择 + 在册查询编排（MC-6：凭据由后端承载持久化——linux =
/// NetworkManager 连接配置）：构造不触设备，首次调用才起 nmcli。开机查在册——
/// 在册则经 `rr3_provision_restored` 直接起 station（重启仍在册），失败即致命
/// （fail-closed，不静默当"没配过网"）。「配网面就绪」回显归本编排。返回二元
/// ——凭据存储（on_rpc connect/forget 运行期需要）与配网状态机初值。
pub(crate) fn rr3_connect_provision(
    timeout_ms: u64,
) -> Result<(Box<dyn Rr3WifiStore>, Rr3Provision), String> {
    let (store, store_backend) = rr3_hal_wifi_store(Duration::from_millis(timeout_ms));
    let prov = match store
        .rr3_wifi_store_ssid()
        .map_err(|e| format!("配网凭据在册查询失败（{store_backend}）: {e}"))?
    {
        Some(ssid) => {
            let ssid = ssid.rr3_ssid_text_as_str();
            eprintln!("配网面就绪: {store_backend} 在册 ssid={ssid:?}（重启恢复 station）");
            Rr3Provision::rr3_provision_restored(ssid)
        }
        None => {
            eprintln!("配网面就绪: {store_backend} 无在册凭据（出厂 ap）");
            Rr3Provision::rr3_provision_new()
        }
    };
    Ok((store, prov))
}

/// TOML 文本 → (Rr3LinkCfg, bt 开机初值)。手工下钻不引 serde derive：字段
/// 个位数，点名缺字段的文案比派生宏的默认报错可读（rr3-act/rr3-power 同款）。
/// 0.0.5 批I N-6：无读者的 roam_probe_enabled / tls_min_version / 总段 log_level
/// 删除；N-2：timeout_ms 迁 wifi 段，生效点 = nmcli 子进程超时。
pub(crate) fn rr3_connect_config_parse(text: &str) -> Result<(Rr3LinkCfg, bool), String> {
    let v: toml::Value = toml::from_str(text).map_err(|e| format!("config TOML 解析失败: {e}"))?;
    let sect = |name: &str| -> Result<&toml::Value, String> {
        v.get("rr3-connect")
            .and_then(|s| s.get(name))
            .ok_or_else(|| format!("config 缺 [rr3-connect.{name}] 段"))
    };
    let u64_of = |sect: &toml::Value, key: &str, sect_name: &str| -> Result<u64, String> {
        sect.get(key)
            .and_then(toml::Value::as_integer)
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
            .ok_or_else(|| format!("config [rr3-connect.{sect_name}] 缺 {key}（非负整数）"))
    };
    let wifi_sect = sect("wifi")?.clone();
    // bt 开机初值（N-1：env 注入退役改 config 数据化注入）：缺段回落 false
    // =出厂不自动开蓝牙，等价旧 env 未设行为（已部署旧 config 兼容）；
    // 有段则 up 必须是布尔（类型错点名拒，不静默吞）
    let bt_up = v
        .get("rr3-connect")
        .and_then(|s| s.get("bt"))
        .map(|sect| {
            sect.get("up")
                .and_then(toml::Value::as_bool)
                .ok_or_else(|| "config [rr3-connect.bt] 缺 up（布尔）".to_string())
        })
        .transpose()?
        .unwrap_or(false);
    Ok((
        Rr3LinkCfg {
            link_report_period_ms: u64_of(&wifi_sect, "link_report_period_ms", "wifi")?.max(1),
            timeout_ms: u64_of(&wifi_sect, "timeout_ms", "wifi")?.max(1),
            // 配网口令下限（N-11①/M-6，批N4 件五数据化）：缺键回落 8 = legacy
            // 语义（已部署旧 config 兼容，同 bt 段缺段回落口径）；显式给值时
            // 必须 ≥1（0 = 无下限是策略漏洞，不当合法配置放行）
            min_pass_len: wifi_sect
                .get("min_pass_len")
                .and_then(toml::Value::as_integer)
                .filter(|n| *n >= 0)
                .map(|n| n as u64)
                .unwrap_or(8)
                .max(1),
        },
        bt_up,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-connect.wifi]
link_report_period_ms = 5000
timeout_ms = 20000
min_pass_len = 8

[rr3-connect.bt]
up = false
"#;
        let (wifi, bt_up) = rr3_connect_config_parse(text).unwrap();
        assert_eq!(wifi.link_report_period_ms, 5000);
        assert_eq!(wifi.timeout_ms, 20000);
        assert_eq!(wifi.min_pass_len, 8);
        assert!(!bt_up, "未写 bt 段 = 出厂不自动开蓝牙");
    }

    /// 配网口令下限（批N4 件五 N-11①）：缺键回落 8 = legacy 语义（旧 config
    /// 兼容，同 bt 缺段回落口径）；显式值生效；显式 0 按 1 拒放（0 = 无下限
    /// 是策略漏洞，不当合法配置）
    #[test]
    fn config_min_pass_len_defaults_to_legacy_eight() {
        let base = "[rr3-connect.wifi]\nlink_report_period_ms = 5000\ntimeout_ms = 20000\n";
        assert_eq!(rr3_connect_config_parse(base).unwrap().0.min_pass_len, 8);
        let text = format!("{base}min_pass_len = 6\n");
        assert_eq!(rr3_connect_config_parse(&text).unwrap().0.min_pass_len, 6);
        let text = format!("{base}min_pass_len = 0\n");
        assert_eq!(
            rr3_connect_config_parse(&text).unwrap().0.min_pass_len,
            1,
            "显式 0 按 1 拒放（无下限不是合法配置）"
        );
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_connect_config_parse("[rr3-connect]\n").unwrap_err();
        assert!(e.contains("[rr3-connect.wifi]"), "缺段点名: {e}");
        let e = rr3_connect_config_parse("[rr3-connect.wifi]\nlink_report_period_ms = 5000\n")
            .unwrap_err();
        assert!(e.contains("timeout_ms"), "缺字段点名: {e}");
        let e = rr3_connect_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }

    /// bt 开机初值注入（N-1）：缺段回落 false（旧 config 兼容）、up=true
    /// 生效、类型错点名拒（不静默吞）。
    #[test]
    fn config_bt_up_defaults_false_and_type_checks() {
        let base = "[rr3-connect.wifi]\nlink_report_period_ms = 5000\ntimeout_ms = 20000\n";
        // 缺段回落 false
        assert!(!rr3_connect_config_parse(base).unwrap().1);
        // up=true 生效
        let text = format!("{base}[rr3-connect.bt]\nup = true\n");
        assert!(rr3_connect_config_parse(&text).unwrap().1);
        // 类型错点名
        let text = format!("{base}[rr3-connect.bt]\nup = \"yes\"\n");
        let e = rr3_connect_config_parse(&text).unwrap_err();
        assert!(
            e.contains("[rr3-connect.bt]") && e.contains("up"),
            "类型错点名: {e}"
        );
    }
}
