//! WiFi 面：底层配置手抄、AP/STA 装配、有界等连与配网事务、boot 起栈链。
//!
//! 全部是 unsafe 壳层（IDF 全局状态）：调用时序纪律逐函数写在 `# Safety` 段
//! ——起栈只在配置腿线程一次，写口事务由 httpd 任务串行调用。
//!
//! 判据要点：等连成功 = **DHCP 得 IP**（`esp_netif_get_ip_info` 非 0），不用
//! `esp_wifi_sta_get_ap_info`（关联瞬态即 OK，坏凭据会被误判成功→不回退→砖）。
//!
//! 拆分动因：`serve.rs` 583 行，WiFi 面与 HTTP 面分属两簇；本文件不碰请求对象。

use std::time::Duration;

use super::*;

/// STA netif 句柄（起栈时写一次——等连判据 `esp_netif_get_ip_info` 查 IP 用；
/// 存 `usize` 绕裸指针非 Send，解引用处均在 unsafe 集中段）。
static STA_NETIF: OnceLock<usize> = OnceLock::new();

/// AP netif 句柄（起栈时写一次）：AP-only 档"网络面在位"判据（W3-3 net 心跳）。
static AP_NETIF: OnceLock<usize> = OnceLock::new();

/// v5.2.2 `WIFI_INIT_CONFIG_DEFAULT()` 宏展开手抄（[`wifi_init_config_t`]）——
/// 宏不进绑定，裸 sys 必须手抄；字段真值逐项对表**本项目生成 sdkconfig**
/// （target …/esp-idf-sys out/sdkconfig，版本组合已钉 v5.2.2）＋ esp_wifi.h
/// `:252` 宏体，升级 IDF 版本须同批重抄（版本钉死登记表口径）。
/// # Safety: 返回的配置含 IDF 静态全局裸指针，只作只读输入交 esp_wifi_init。
pub(super) unsafe fn wifi_default_config() -> wifi_init_config_t {
    wifi_init_config_t {
        // &g_wifi_osi_funcs（esp_wifi.h :253）
        osi_funcs: std::ptr::addr_of_mut!(g_wifi_osi_funcs),
        // g_wifi_default_wpa_crypto_funcs（:254）
        wpa_crypto_funcs: g_wifi_default_wpa_crypto_funcs,
        static_rx_buf_num: 10,     // CONFIG_ESP_WIFI_STATIC_RX_BUFFER_NUM
        dynamic_rx_buf_num: 32,    // CONFIG_ESP_WIFI_DYNAMIC_RX_BUFFER_NUM
        tx_buf_type: 1,            // CONFIG_ESP_WIFI_TX_BUFFER_TYPE（动态）
        static_tx_buf_num: 0,      // CONFIG_ESP_WIFI_STATIC_TX_BUFFER 未设（动态模式，宏 → 0）
        dynamic_tx_buf_num: 32,    // CONFIG_ESP_WIFI_DYNAMIC_TX_BUFFER_NUM
        rx_mgmt_buf_type: 0,       // CONFIG_ESP_WIFI_DYNAMIC_RX_MGMT_BUF=0（静态）
        rx_mgmt_buf_num: 5,        // CONFIG_ESP_WIFI_RX_MGMT_BUF_NUM_DEF
        cache_tx_buf_num: 0,       // ESP32 无 CACHE_TX（宏 → 0）
        csi_enable: 0,             // CONFIG_ESP_WIFI_CSI_ENABLED 未设
        ampdu_rx_enable: 1,        // CONFIG_ESP_WIFI_AMPDU_RX_ENABLED=y
        ampdu_tx_enable: 1,        // CONFIG_ESP_WIFI_AMPDU_TX_ENABLED=y
        amsdu_tx_enable: 0,        // CONFIG_ESP_WIFI_AMSDU_TX_ENABLED 未设
        nvs_enable: 1,             // CONFIG_ESP_WIFI_NVS_ENABLED=y
        nano_enable: 0,            // CONFIG_ESP_WIFI_NANO_FORMAT_ENABLED 未设
        rx_ba_win: 6,              // CONFIG_ESP_WIFI_RX_BA_WIN
        wifi_task_core_id: 0,      // CONFIG_ESP_WIFI_TASK_PINNED_TO_CORE_0=y（宏 :196）
        beacon_max_len: 752,       // CONFIG_ESP_WIFI_SOFTAP_BEACON_MAX_LEN
        mgmt_sbuf_num: 32,         // CONFIG_ESP_WIFI_MGMT_SBUF_NUM
        feature_caps: 1,           // WIFI_FEATURE_CAPS = WPA3_SAE(1<<0)｜SPIRAM 0｜FTM×2 0
        sta_disconnected_pm: true, // CONFIG_ESP_WIFI_STA_DISCONNECTED_PM_ENABLE=y
        espnow_max_encrypt_num: 7, // CONFIG_ESP_WIFI_ESPNOW_MAX_ENCRYPT_NUM
        magic: WIFI_INIT_CONFIG_MAGIC as i32, // 0x1F2F3F4F——esp_wifi_init 魔法校验
    }
}

/// SoftAP 配置（2026-10-31 独立形态配网面）：AP 常开，SSID/口令由编译期
/// `RR3_BOARD_AP_SSID`/`RR3_BOARD_AP_PASS` 回落缺省（"RR3-BOARD"/"rock3robot"，
/// 非秘密、仓内公开——目的只是局域网配网入口，不承载业务凭据）。
/// 返回 AP 实际 SSID 便于 boot 横幅点名；`Err(i32)` = 某步 ESP-IDF 拒。
///
/// # Safety
///
/// 只能在配置腿起栈线程、`esp_wifi_start` 之前调用；函数直接改写 IDF 全局
/// WiFi 配置，不得与其它 WiFi 配置操作并发。
pub(super) unsafe fn wifi_ap_configure() -> Result<&'static str, i32> {
    let ssid = option_env!("RR3_BOARD_AP_SSID").unwrap_or("RR3-BOARD");
    let pass = option_env!("RR3_BOARD_AP_PASS").unwrap_or("rock3robot");
    let s = ssid.as_bytes();
    let p = pass.as_bytes();
    // 形制：SSID ≤32B（IDF 硬限）、WPA2 口令 8..=63B
    if s.is_empty() || s.len() > 32 || p.len() < 8 || p.len() > 63 {
        return Err(-1);
    }
    let ap = esp_netif_create_default_wifi_ap();
    if ap.is_null() {
        return Err(-1);
    }
    let _ = AP_NETIF.set(ap as usize);
    let mut cfg: wifi_config_t = std::mem::zeroed();
    cfg.ap.ssid[..s.len()].copy_from_slice(s);
    cfg.ap.password[..p.len()].copy_from_slice(p);
    cfg.ap.ssid_len = s.len() as u8;
    cfg.ap.channel = 1;
    cfg.ap.authmode = wifi_auth_mode_t_WIFI_AUTH_WPA2_PSK;
    cfg.ap.max_connection = 4;
    let r = esp_wifi_set_config(wifi_interface_t_WIFI_IF_AP, &mut cfg);
    if r != ESP_OK {
        return Err(r);
    }
    Ok(ssid)
}

/// STA 凭据装配（start_httpd 与写口 apply/回退共用；union 起底——sta 臂其余
/// 字段零 = 默认；调用前形制已验 32/64 上限＝`sta.ssid[32]`/`sta.password[64]`
/// 数组尺寸，copy_from_slice 不越界）。返回 esp_err。
///
/// # Safety
///
/// 只能在配置腿线程/写口事务内调用；函数先自守 32/64 字节上限再定长复制，
/// 调用方仍须保证不与并发的 WiFi 配置操作交叉。
pub(super) unsafe fn wifi_sta_set_cred(ssid: &[u8], pass: &[u8]) -> i32 {
    // FFI 边界自守（静态审计 B16）：定长数组越界复制＝panic＝整板 abort，这里把
    // "上层忘了校形"折成错误码走事务回退，永不 panic（读窄缝已同域校验，本关是
    // 第二道墙）。
    if ssid.len() > crate::cred::RR3_CRED_SSID_MAX || pass.len() > crate::cred::RR3_CRED_PASS_MAX {
        return -1; // 形制域内错误（非 esp_err 码空间，同 cred 模块口径）
    }
    let mut wcfg: wifi_config_t = std::mem::zeroed();
    wcfg.sta.ssid[..ssid.len()].copy_from_slice(ssid);
    wcfg.sta.password[..pass.len()].copy_from_slice(pass);
    esp_wifi_set_config(wifi_interface_t_WIFI_IF_STA, &mut wcfg)
}

/// 有界等连（连上＝**DHCP 得 IP**，判据见函数内注记）：[`WIFI_CONNECT_WAIT_MS`]
/// 上限、[`WIFI_POLL_MS`] 步进；超窗即 false（调用方如实应答——不猜不造）。
/// # Safety
///
/// 必须在 `wifi_bringup` 建好 STA netif 之后调用；`STA_NETIF` 是启动期写一次的
/// 进程级单例，未写入时函数直接返回 false（时序防御），不 deref 空指针。
pub(super) unsafe fn wifi_sta_wait_conn() -> bool {
    // 判据＝**DHCP 得 IP**（`esp_netif_get_ip_info` 的 ip.addr 非 0），批N15 件二
    // 真板取证实锤的缺陷修复：原判据 `esp_wifi_sta_get_ap_info` 在关联瞬态
    // （auth/assoc→run，4-way 握手前）即返 ESP_OK——坏凭据被误判成功→不回退→
    // NVS 残留坏凭据＝真砖路径（真板波形：关联 run 后 3s 被 AP 踢回 init 反复）。
    // 密码错永远到不了 DHCP，本判据物理可靠。
    let netif = match STA_NETIF.get() {
        Some(p) => *p as *mut esp_netif_t,
        None => return false, // netif 未建（时序防御，常态不可达）
    };
    for _ in 0..(WIFI_CONNECT_WAIT_MS / WIFI_POLL_MS) {
        let mut info: esp_netif_ip_info_t = std::mem::zeroed();
        if esp_netif_get_ip_info(netif, &mut info) == ESP_OK && info.ip.addr != 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(WIFI_POLL_MS));
    }
    false
}

/// 配网事务（批N14 件二，fail-safe 方向——消灭"写坏凭据板掉网"砖路径）：
/// 读旧值 → 写 NVS → apply（disconnect→set→connect）→ 有界等连 → 败即回退
/// 旧值（Some→写回 / None→erase 回原空态）重连 → 如实应答。全程**不持 bus 锁**
/// （不碰 [`crate::state::PeriphState`]）；httpd 服务任务串行处理请求＝两写口
/// 天然互斥。回退写败＝砖路径残余（新凭据在 NVS 但未连——重启后读链装配新凭据
/// 重试，AP 不可达则掉网）：NVS 写败罕见链，如实点名 stage 不冒领。
///
/// # Safety
///
/// 只能由 httpd 任务串行调用（两写口天然互斥），不得与 `wifi_bringup` 并发；
/// 事务期间不持 bus 锁，不跨线程共享 Rust 引用。
pub(super) unsafe fn rr3_wifi_write_apply(ssid: &str, pass: &str) -> Rr3WifiWriteResult {
    let old = crate::cred::rr3_cred_read(); // None＝原空态（回退目标＝erase）
                                            // 写 NVS（cred 模块内复验形制——毒不双入；败＝NVS 未动无需回退）
    let true = crate::cred::rr3_cred_write(ssid, pass).is_ok() else {
        return Rr3WifiWriteResult::rejected("nvs");
    };
    // apply 新凭据（disconnect 返回值不看——未连态不阻断 apply 链；
    // set/connect 任一败一律进回退分支）
    esp_wifi_disconnect();
    let mut apply_ok = wifi_sta_set_cred(ssid.as_bytes(), pass.as_bytes()) == ESP_OK;
    apply_ok = apply_ok && esp_wifi_connect() == ESP_OK;
    let applied = apply_ok && wifi_sta_wait_conn();
    if applied {
        return Rr3WifiWriteResult {
            ok: true,
            connected: true,
            reverted: false,
            stage: "apply",
        };
    }
    // 新凭据未连——回退旧值（Some→写回；None→erase；cred 模块幂等语义在案）
    let revert_ok = old.as_ref().map_or_else(
        || crate::cred::rr3_cred_erase().is_ok(),
        |c| crate::cred::rr3_cred_write(&c.ssid, c.pass()).is_ok(),
    );
    let true = revert_ok else {
        return Rr3WifiWriteResult {
            ok: false,
            connected: false,
            reverted: false,
            stage: "revert",
        };
    };
    // 回退写成功——apply 旧凭据重连（原空态＝停在断态，不 connect）
    old.as_ref().map_or_else(
        || {
            esp_wifi_disconnect();
        },
        |c| {
            esp_wifi_disconnect();
            if wifi_sta_set_cred(c.ssid.as_bytes(), c.pass().as_bytes()) == ESP_OK {
                let _ = esp_wifi_connect();
            }
        },
    );
    let connected = wifi_sta_wait_conn();
    Rr3WifiWriteResult {
        ok: true,
        connected,
        reverted: true,
        stage: "reverted",
    }
}

/// WiFi 起栈链（netif/事件循环/宏配置/APSTA/AP/STA/start/connect），返回 AP 实际
/// SSID 供 boot 横幅点名；逐步 fail-honest（错误码由调用方点名）。
///
/// # Safety
///
/// 每 boot 只调用一次，且只在配置腿线程；函数按序初始化 IDF netif/事件循环/
/// WiFi 驱动，重复或并发调用会与 IDF 全局状态冲突。
pub(super) unsafe fn wifi_bringup(ssid: &[u8], pass: &[u8]) -> Result<&'static str, i32> {
    // ① netif + 事件循环 + STA 缺省 netif（句柄留存——等连判据查 IP 用）
    let r = esp_netif_init();
    let true = r == ESP_OK else { return Err(r) };
    let r = esp_event_loop_create_default();
    let true = r == ESP_OK else { return Err(r) };
    let _ = STA_NETIF.set(esp_netif_create_default_wifi_sta() as usize);
    // ② WiFi 起栈（手抄宏配置见 wifi_default_config）
    let cfg = wifi_default_config();
    let r = esp_wifi_init(&cfg);
    let true = r == ESP_OK else { return Err(r) };
    // **模式必须先于接口配置**（2026-10-31 真板排查）：IDF 的 esp_wifi_set_config
    // 按接口查"该接口是否已在当前模式里启用"，模式仍是缺省 STA 时配 AP 接口返回
    // ESP_ERR_INVALID_ARG——表现为配置腿整段起不来、板侧 AP 不可见，而其它功能
    // 全部正常（板活着但不配网，排查极易误判成 SoftAP 支持未编入）。
    let r = esp_wifi_set_mode(wifi_mode_t_WIFI_MODE_APSTA);
    let true = r == ESP_OK else { return Err(r) };
    // AP 常开（SoftAP 配网面，见 wifi_ap_configure 注）：netif 与 config 须在
    // esp_wifi_start 前就位——IDF 在 start 时才拉起 AP/DHCP 服务。
    let ap_ssid = wifi_ap_configure()?;
    // STA 凭据（helper 共用——写口 apply/回退同款；位形已在 spawn 前验）；
    // ap-only 档空凭据不配置 STA，只起 AP
    if !ssid.is_empty() {
        let r = wifi_sta_set_cred(ssid, pass);
        let true = r == ESP_OK else { return Err(r) };
    }
    let r = esp_wifi_start();
    let true = r == ESP_OK else { return Err(r) };
    // STA 关联发起（批N15 件二真板取证实锤的缺陷修复：esp_wifi_start 只起驱动
    // 不自动连 AP——缺此步板永不关联、无 got ip；写口 apply/回退两路本就有
    // connect，唯 boot 路漏）。ap-only 档无 STA 目标，跳过。
    if !ssid.is_empty() {
        let r = esp_wifi_connect();
        let true = r == ESP_OK else { return Err(r) };
    }
    Ok(ap_ssid)
}

/// 网络面在位判据（W3-3 net 任务心跳真值）：STA 已得 IP（DHCP 完成）**或**
/// SoftAP 在跑（AP-only 配网面可达）即为真；两者皆无＝网络服务确实不可达。
///
/// 与 [`wifi_sta_wait_conn`] 分工不同：那里判"新 STA 凭据是否连上"，这里判
/// "板侧网络服务是否可达"——AP-only 档必须为真，否则无凭据板会把 net 判死。
///
/// # Safety
///
/// 只读 IDF netif 状态；两 handle 均为启动期写一次的进程级单例，未写入即
/// 返回 false（时序防御），不解引用空指针。
#[cfg(feature = "autonomy")]
pub(super) unsafe fn rr3_net_up() -> bool {
    netif_has_ip(STA_NETIF.get()) || netif_has_ip(AP_NETIF.get())
}

/// # Safety: 同 [`rr3_net_up`]；`esp_netif_get_ip_info` 只读传入 netif。
#[cfg(feature = "autonomy")]
unsafe fn netif_has_ip(h: Option<&usize>) -> bool {
    let Some(p) = h else {
        return false;
    };
    let mut info: esp_netif_ip_info_t = std::mem::zeroed();
    esp_netif_get_ip_info(*p as *mut esp_netif_t, &mut info) == ESP_OK && info.ip.addr != 0
}
