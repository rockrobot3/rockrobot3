//! 板侧凭据存储对象（N-10 ② 主体半边，批N14 件一；connect 册 N-10 ②／全案 §3.6 家族 F 凭据行）。
//!
//! 工单原文三条要求，落法逐条对表：
//! 1. **读取窄缝**——凭据值唯一出入口是 [`rr3_cred_read`]（读时现取：NVS → 调用方，
//!    本模块不缓存不驻留），消费面只有两处：boot 读链（[`crate::httpd`] spawn 段）与
//!    配网写口（批N14 件二）。`Rr3Cred` 的 `pass` 字段私有，取值 accessor 不给
//!    `Debug`/`Display` 任何落点（值类型**不派生 `Debug`**——格式化路径编译期不可达，
//!    零回显的第一道机器墙）。
//! 2. **不落日志**——错误点名只出键名与长度，永不出值（同 host 面板"值零回显"纪律）。
//! 3. **零回显是契约要求**——观测面（`/api/board/state`/`bindings`）结构上不含凭据字段
//!    （httpd.rs JSON 形状单测钉死）；本模块任何返回值/日志路径不含凭据值。
//!
//! namespace [`RR3_CRED_NAMESPACE`] 单真值（"rr3"——全案 §3.6 凭据行 `cred_store =
//! "nvs:rr3"` 的板侧实现值；该键进 `[hardware]`/绑定域**缓批**：契约级三处同批＋桥上
//! 注入晚于 WiFi 起栈，boot 读链不可能消费桥上键——批N14 计划册 §1 不排项表）。
//! boot 读链（[`crate::httpd`]）：**NVS 优先 → 编译期 `option_env!` 回落 → 都缺＝配置腿
//! 缺席 fail-honest**——NVS 空时行为与批N13 逐字一致（零回归，真板会话链不感知）。

use std::ffi::CString;

use esp_idf_sys::{
    nvs_close, nvs_commit, nvs_erase_key, nvs_get_str, nvs_open, nvs_open_mode_t_NVS_READONLY,
    nvs_open_mode_t_NVS_READWRITE, nvs_set_str, nvs_handle_t, ESP_ERR_NVS_INVALID_LENGTH,
    ESP_ERR_NVS_NOT_FOUND, ESP_OK,
};

/// NVS namespace 单真值（全案 §3.6 凭据行 `nvs:rr3` 的板侧实现值；消费面禁止抄第二份）。
pub(crate) const RR3_CRED_NAMESPACE: &str = "rr3";
/// SSID 键（NVS 键名上限 15 字节，9 字节在册）。
pub(crate) const RR3_CRED_KEY_SSID: &str = "wifi.ssid";
/// 密码键（同上）。
pub(crate) const RR3_CRED_KEY_PASS: &str = "wifi.pass";

/// 形制上限（与编译期注入路径 spawn 侧 32/64 同口径——批N14 计划册 §1：板侧写口
/// 仅形制校验，legacy"至少 8 位"是宿主面板契约字面量不随批）。
pub(crate) const RR3_CRED_SSID_MAX: usize = 32;
pub(crate) const RR3_CRED_PASS_MAX: usize = 64;

/// 凭据值类型（零回显机器墙：**不派生 `Debug`/`Display`**——`#[derive]` 里没有它们，
/// 任何 `{:?}`/`{}` 格式化在编译期即红；`Clone` 仅供事务域内旧值暂驻，用后即弃）。
pub(crate) struct Rr3Cred {
    /// SSID（非机密面，观测/日志可点名——零回显契约只护凭据值）
    pub(crate) ssid: String,
    /// 密码（私有——读取窄缝：只有 WiFi STA 配置消费点经 [`Rr3Cred::pass`] 取）
    pass: String,
}

impl Rr3Cred {
    pub(crate) fn new(ssid: String, pass: String) -> Self {
        Self { ssid, pass }
    }

    /// 密码读取窄缝（消费点：WiFi STA `wifi_config_t` 装配——boot 起栈与写口重配置）。
    pub(crate) fn pass(&self) -> &str {
        &self.pass
    }
}

/// 形制校验（纯函数，单测钉域）：SSID 1..=32B（空＝没配 SSID）、密码 ≤64B
/// （**空合法**＝开放网络，与编译期路径 `unwrap_or_default()` 同口径）。
pub(crate) fn rr3_cred_form_check(ssid: &str, pass: &str) -> Result<(), &'static str> {
    if ssid.is_empty() {
        return Err("SSID 空");
    }
    if ssid.len() > RR3_CRED_SSID_MAX {
        return Err("SSID 超位形 32B");
    }
    if pass.len() > RR3_CRED_PASS_MAX {
        return Err("密码超位形 64B");
    }
    Ok(())
}

/// 读时现取（读取窄缝唯一出入口）：两键齐才 `Some`（半份凭据＝没配，不猜不造）。
/// NVS 未初始化/命名空间不存在/键不存在/值超形——一律 `None`＋键名级点名（不出值）。
pub(crate) fn rr3_cred_read() -> Option<Rr3Cred> {
    let ns = CString::new(RR3_CRED_NAMESPACE).ok()?;
    let k_ssid = CString::new(RR3_CRED_KEY_SSID).ok()?;
    let k_pass = CString::new(RR3_CRED_KEY_PASS).ok()?;
    let mut h: nvs_handle_t = 0;
    // 命名空间不存在＝没配过（常态，非错误——静默回 None，boot 走编译期回落）
    if unsafe { nvs_open(ns.as_ptr(), nvs_open_mode_t_NVS_READONLY, &mut h) } != ESP_OK {
        return None;
    }
    let r = unsafe {
        (|| {
            let ssid = nvs_get_str_boxed(h, &k_ssid)?;
            let pass = nvs_get_str_boxed(h, &k_pass)?;
            Some(Rr3Cred::new(ssid, pass))
        })()
    };
    unsafe { nvs_close(h) };
    r
}

/// 单键读出（定长缓冲一次取——形制上限已知，两段式探测无必要；超形/坏 UTF-8＝
/// 不认，点名后按缺席处理——fail-honest 不静默造值）。
unsafe fn nvs_get_str_boxed(h: nvs_handle_t, key: &CString) -> Option<String> {
    // +1 NUL；65 容密码形制上限 64——超形值读出即 INVALID_LENGTH，点名缺席
    let mut buf = [0u8; RR3_CRED_PASS_MAX + 1];
    let mut len = buf.len();
    let r = nvs_get_str(h, key.as_ptr(), buf.as_mut_ptr() as *mut _, &mut len);
    let kname = key.to_string_lossy();
    if r == ESP_ERR_NVS_INVALID_LENGTH {
        eprintln!("[periph] 凭据键 {kname} 值超形（>{RR3_CRED_PASS_MAX}B）——按缺席处理");
        return None;
    }
    if r != ESP_OK {
        // NVS_NOT_FOUND 等＝该键没配（常态走 None）；其余错误同样键名级点名不出值
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    match std::str::from_utf8(&buf[..end]) {
        Ok(s) => Some(s.to_string()),
        Err(_) => {
            eprintln!("[periph] 凭据键 {kname} 值非 UTF-8——按缺席处理");
            None
        }
    }
}

/// 写入（配网写口消费点，批N14 件二 [crate::httpd::rr3_wifi_write_apply]——apply
/// 与回退两路共用）：形制校验前件（调用方已校验，此处复验——毒不双入）→
/// 两键写 → commit。错误只回码，点名在调用方（键名级，不出值）。
pub(crate) fn rr3_cred_write(ssid: &str, pass: &str) -> Result<(), i32> {
    rr3_cred_form_check(ssid, pass).map_err(|_| -1)?; // -1：形制域内错误（非 esp_err 码空间）
    unsafe fn raw(h: nvs_handle_t, key: &CString, v: &str) -> Result<(), i32> {
        let cv = CString::new(v).map_err(|_| -1)?;
        if nvs_set_str(h, key.as_ptr(), cv.as_ptr()) != ESP_OK {
            return Err(-2);
        }
        Ok(())
    }
    let ns = match CString::new(RR3_CRED_NAMESPACE) {
        Ok(s) => s,
        Err(_) => return Err(-1),
    };
    let k_ssid = match CString::new(RR3_CRED_KEY_SSID) {
        Ok(s) => s,
        Err(_) => return Err(-1),
    };
    let k_pass = match CString::new(RR3_CRED_KEY_PASS) {
        Ok(s) => s,
        Err(_) => return Err(-1),
    };
    let mut h: nvs_handle_t = 0;
    unsafe {
        if nvs_open(ns.as_ptr(), nvs_open_mode_t_NVS_READWRITE, &mut h) != ESP_OK {
            return Err(-3);
        }
        let r = (|| -> Result<(), i32> {
            raw(h, &k_ssid, ssid)?;
            raw(h, &k_pass, pass)?;
            if nvs_commit(h) != ESP_OK {
                return Err(-4);
            }
            Ok(())
        })();
        nvs_close(h);
        r
    }
}

/// 擦除（事务回退到"原空态"消费点，批N14 件二 [crate::httpd::rr3_wifi_write_apply]）：
/// 两键 erase + commit；键不存在不算错（幂等——回退目标是"读不到"，erase 一个
/// 不存在的键语义已达成）。
pub(crate) fn rr3_cred_erase() -> Result<(), i32> {
    let ns = match CString::new(RR3_CRED_NAMESPACE) {
        Ok(s) => s,
        Err(_) => return Err(-1),
    };
    let k_ssid = match CString::new(RR3_CRED_KEY_SSID) {
        Ok(s) => s,
        Err(_) => return Err(-1),
    };
    let k_pass = match CString::new(RR3_CRED_KEY_PASS) {
        Ok(s) => s,
        Err(_) => return Err(-1),
    };
    let mut h: nvs_handle_t = 0;
    unsafe {
        if nvs_open(ns.as_ptr(), nvs_open_mode_t_NVS_READWRITE, &mut h) != ESP_OK {
            return Err(-3);
        }
        let r = (|| -> Result<(), i32> {
            // ESP_ERR_NVS_NOT_FOUND＝键本就不在——幂等语义，不算错
            let e1 = nvs_erase_key(h, k_ssid.as_ptr());
            let e2 = nvs_erase_key(h, k_pass.as_ptr());
            let bad = [e1, e2].into_iter().any(|e| e != ESP_OK && e != ESP_ERR_NVS_NOT_FOUND);
            if bad {
                return Err(-5);
            }
            if nvs_commit(h) != ESP_OK {
                return Err(-4);
            }
            Ok(())
        })();
        nvs_close(h);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 形制域钉死：SSID 空/33B 红、32B 绿；密码 64B 绿、65B 红、空绿（开放网络）。
    #[test]
    fn form_check_domain() {
        let ok_ssid = "a".repeat(RR3_CRED_SSID_MAX);
        let bad_ssid = "a".repeat(RR3_CRED_SSID_MAX + 1);
        let ok_pass = "p".repeat(RR3_CRED_PASS_MAX);
        let bad_pass = "p".repeat(RR3_CRED_PASS_MAX + 1);
        assert_eq!(rr3_cred_form_check(&ok_ssid, ""), Ok(()));
        assert_eq!(rr3_cred_form_check(&ok_ssid, &ok_pass), Ok(()));
        assert!(rr3_cred_form_check("", "x").is_err(), "SSID 空必须红");
        assert!(rr3_cred_form_check(&bad_ssid, "x").is_err(), "SSID 33B 必须红");
        assert!(rr3_cred_form_check(&ok_ssid, &bad_pass).is_err(), "密码 65B 必须红");
    }

    /// namespace/键名单真值（全案 §3.6 `nvs:rr3` 的板侧实现值——键半边进绑定域
    /// 时与本表对账，禁抄第二份）。
    #[test]
    fn namespace_and_keys_single_truth() {
        assert_eq!(RR3_CRED_NAMESPACE, "rr3");
        assert_eq!(RR3_CRED_KEY_SSID, "wifi.ssid");
        assert_eq!(RR3_CRED_KEY_PASS, "wifi.pass");
        // NVS 键名上限 15 字节（idf nvs.h 硬约束——键名超限写入口即红）
        for k in [RR3_CRED_KEY_SSID, RR3_CRED_KEY_PASS] {
            assert!(k.len() <= 15, "NVS 键名超 15B: {k}");
            assert!(!k.contains('\0'));
        }
    }

    /// 零回显机器墙形制：值类型不带 `Debug`/`Display` 的可检面——pass 字段私有
    /// （编译期窄缝），这里钉 accessor 与字段形状（结构性防"顺手加 derive"的
    /// review 锚；真正的机器墙是 derive 缺席本身）。
    #[test]
    fn cred_type_shape() {
        let c = Rr3Cred::new("ssid".into(), "pass".into());
        assert_eq!(c.ssid, "ssid");
        assert_eq!(c.pass(), "pass");
    }
}
