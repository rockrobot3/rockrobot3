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
    nvs_close, nvs_commit, nvs_erase_key, nvs_get_str, nvs_handle_t, nvs_open,
    nvs_open_mode_t_NVS_READONLY, nvs_open_mode_t_NVS_READWRITE, nvs_set_str,
    ESP_ERR_NVS_INVALID_LENGTH, ESP_ERR_NVS_NOT_FOUND, ESP_OK,
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

/// 读出值定形（纯函数，单测钉域）：NUL 截断 + 字长上限 + UTF-8 三关。**读侧与写侧
/// 同域**——NVS 里可能躺着旧固件/手改的超形值，直接放进 WiFi 定长数组
/// （`sta.ssid[32]`/`sta.password[64]`）的 `copy_from_slice` 就是越界 panic＝整板
/// abort（静态审计 B16）。空串合法（开放网络密码），空 SSID 由形制门另判。
fn rr3_cred_take(raw: &[u8], max: usize) -> Option<String> {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    if end > max {
        return None;
    }
    std::str::from_utf8(&raw[..end]).ok().map(str::to_string)
}

/// 读时现取（读取窄缝唯一出入口）：两键齐且**形制同门**才 `Some`（半份/超形凭据＝
/// 没配，不猜不造）。NVS 未初始化/命名空间不存在/键不存在/值超形——一律 `None`＋
/// 键名级点名（不出值）。
pub(crate) fn rr3_cred_read() -> Option<Rr3Cred> {
    let ns = CString::new(RR3_CRED_NAMESPACE).ok()?;
    let k_ssid = CString::new(RR3_CRED_KEY_SSID).ok()?;
    let k_pass = CString::new(RR3_CRED_KEY_PASS).ok()?;
    let mut h: nvs_handle_t = 0;
    // 命名空间不存在＝没配过（常态，非错误——静默回 None，boot 走编译期回落）。
    // 其余打开失败（无空闲页/分区版本不符/参数坏）= 存储面故障，必须点名：静默按
    // "没配过"处理会让固化凭据悄悄回落到编译期注入（设备连到另一张网），排障零线索。
    let open = unsafe { nvs_open(ns.as_ptr(), nvs_open_mode_t_NVS_READONLY, &mut h) };
    if open != ESP_OK {
        rr3_cred_open_fault(open);
        return None;
    }
    let r = unsafe { rr3_cred_pairs(h, &k_ssid, &k_pass) };
    unsafe { nvs_close(h) };
    r
}

/// NVS 打开失败的点名：命名空间不存在是常态（不报），其余按存储面故障报。
fn rr3_cred_open_fault(open: i32) {
    if open != ESP_ERR_NVS_NOT_FOUND {
        eprintln!(
            "[periph] 凭据 NVS 打不开（ns={RR3_CRED_NAMESPACE}，err={open:#x}）——按未配置处理（存储面故障，非未配网）"
        );
    }
}

/// 两键读取与同门复验（读口一次现取，不缓存）。
unsafe fn rr3_cred_pairs(h: nvs_handle_t, k_ssid: &CString, k_pass: &CString) -> Option<Rr3Cred> {
    let ssid = nvs_get_str_boxed(h, k_ssid, RR3_CRED_SSID_MAX)?;
    let pass = nvs_get_str_boxed(h, k_pass, RR3_CRED_PASS_MAX)?;
    // 写侧同门复验（空 SSID 等半份形制在此折缺席，读口不是"任何字节能进"
    if rr3_cred_form_check(&ssid, &pass).is_err() {
        eprintln!(
            "[periph] 凭据形制不合（键 {}）——按缺席处理",
            k_ssid.to_string_lossy()
        );
        return None;
    }
    Some(Rr3Cred::new(ssid, pass))
}

/// 单键读出（定长缓冲一次取——形制上限已知，两段式探测无必要；超形/坏 UTF-8＝
/// 不认，点名后按缺席处理——fail-honest 不静默造值）。
unsafe fn nvs_get_str_boxed(h: nvs_handle_t, key: &CString, max: usize) -> Option<String> {
    // +1 NUL；缓冲按**密码上限** 64 取（两键中最宽者），逐键上限由 take 判
    let mut buf = [0u8; RR3_CRED_PASS_MAX + 1];
    let mut len = buf.len();
    let r = nvs_get_str(h, key.as_ptr(), buf.as_mut_ptr() as *mut _, &mut len);
    let kname = key.to_string_lossy();
    if r == ESP_ERR_NVS_INVALID_LENGTH {
        eprintln!("[periph] 凭据键 {kname} 值超形（>{max}B）——按缺席处理");
        return None;
    }
    if r != ESP_OK {
        // NOT_FOUND = 该键没配（常态走 None）；其余错误是存储面故障，同上必须点名
        if r != ESP_ERR_NVS_NOT_FOUND {
            eprintln!("[periph] 凭据键 {kname} 读失败（err={r:#x}）——按未配置处理");
        }
        return None;
    }
    match rr3_cred_take(&buf, max) {
        Some(s) => Some(s),
        None => {
            eprintln!("[periph] 凭据键 {kname} 值非 UTF-8 或超形（>{max}B）——按缺席处理");
            None
        }
    }
}

/// 写入（配网写口消费点，批N14 件二 [crate::httpd::rr3_wifi_write_apply]——apply
/// 与回退两路共用）：形制校验前件（调用方已校验，此处复验——毒不双入）→
/// 两键写 → commit。错误只回码，点名在调用方（键名级，不出值）。
pub(crate) fn rr3_cred_write(ssid: &str, pass: &str) -> Result<(), i32> {
    rr3_cred_form_check(ssid, pass).map_err(|_| -1)?; // -1：形制域内错误（非 esp_err 码空间）
    let keys = rr3_cred_keys()?;
    let mut h: nvs_handle_t = 0;
    unsafe {
        if nvs_open(keys.ns.as_ptr(), nvs_open_mode_t_NVS_READWRITE, &mut h) != ESP_OK {
            return Err(-3);
        }
        let r = rr3_cred_put(h, &keys, ssid, pass);
        nvs_close(h);
        r
    }
}

/// NVS 命名空间与两键名的 C 串（含 NUL 的常量不可能失败，失败即 -1 形制域）。
struct Rr3CredKeys {
    ns: CString,
    ssid: CString,
    pass: CString,
}

fn rr3_cred_keys() -> Result<Rr3CredKeys, i32> {
    let cstr = |s: &str| CString::new(s).map_err(|_| -1);
    Ok(Rr3CredKeys {
        ns: cstr(RR3_CRED_NAMESPACE)?,
        ssid: cstr(RR3_CRED_KEY_SSID)?,
        pass: cstr(RR3_CRED_KEY_PASS)?,
    })
}

/// 两键写入 + commit（-2 写失败 / -1 值含 NUL / -4 commit 失败）。
unsafe fn rr3_cred_put(
    h: nvs_handle_t,
    k: &Rr3CredKeys,
    ssid: &str,
    pass: &str,
) -> Result<(), i32> {
    rr3_cred_raw(h, &k.ssid, ssid)?;
    rr3_cred_raw(h, &k.pass, pass)?;
    if nvs_commit(h) != ESP_OK {
        return Err(-4);
    }
    Ok(())
}

unsafe fn rr3_cred_raw(h: nvs_handle_t, key: &CString, v: &str) -> Result<(), i32> {
    let cv = CString::new(v).map_err(|_| -1)?;
    if nvs_set_str(h, key.as_ptr(), cv.as_ptr()) != ESP_OK {
        return Err(-2);
    }
    Ok(())
}

/// 擦除（事务回退到"原空态"消费点，批N14 件二 [crate::httpd::rr3_wifi_write_apply]）：
/// 两键 erase + commit；键不存在不算错（幂等——回退目标是"读不到"，erase 一个
/// 不存在的键语义已达成）。
pub(crate) fn rr3_cred_erase() -> Result<(), i32> {
    let keys = rr3_cred_keys()?;
    let mut h: nvs_handle_t = 0;
    unsafe {
        if nvs_open(keys.ns.as_ptr(), nvs_open_mode_t_NVS_READWRITE, &mut h) != ESP_OK {
            return Err(-3);
        }
        let r = (|| -> Result<(), i32> {
            // ESP_ERR_NVS_NOT_FOUND＝键本就不在——幂等语义，不算错
            let e1 = nvs_erase_key(h, keys.ssid.as_ptr());
            let e2 = nvs_erase_key(h, keys.pass.as_ptr());
            let bad = [e1, e2]
                .into_iter()
                .any(|e| e != ESP_OK && e != ESP_ERR_NVS_NOT_FOUND);
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
        assert!(
            rr3_cred_form_check(&bad_ssid, "x").is_err(),
            "SSID 33B 必须红"
        );
        assert!(
            rr3_cred_form_check(&ok_ssid, &bad_pass).is_err(),
            "密码 65B 必须红"
        );
    }

    /// 读侧定形（静态审计 B16）：NVS 里的超形/坏 UTF-8 值不得流出读窄缝——
    /// 写侧形制门只拦新写入，旧固件/手改的存量值必须同域拒（否则定长数组
    /// `copy_from_slice` 越界即整板 abort）。空串合法（开放网络密码）。
    #[test]
    fn take_rejects_oversize_and_bad_utf8() {
        assert_eq!(
            rr3_cred_take(b"ap\0", RR3_CRED_SSID_MAX).as_deref(),
            Some("ap")
        );
        assert_eq!(
            rr3_cred_take(b"", RR3_CRED_PASS_MAX).as_deref(),
            Some(""),
            "空密码合法"
        );
        let max_ssid = "a".repeat(RR3_CRED_SSID_MAX);
        assert_eq!(
            rr3_cred_take(max_ssid.as_bytes(), RR3_CRED_SSID_MAX).as_deref(),
            Some(max_ssid.as_str()),
            "恰好 32B 合法（无 NUL 也按全长取）"
        );
        let over = "a".repeat(RR3_CRED_SSID_MAX + 1);
        assert_eq!(
            rr3_cred_take(over.as_bytes(), RR3_CRED_SSID_MAX),
            None,
            "33B SSID 必须缺席"
        );
        assert_eq!(
            rr3_cred_take(&[0xff, 0xfe], RR3_CRED_PASS_MAX),
            None,
            "非 UTF-8 必须缺席"
        );
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
