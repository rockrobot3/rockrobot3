//! 配置腿起栈面（自 serve.rs 抽出）：NVS 凭据读链、起栈线程、httpd 注册与起栈打印。
//!
//! unsafe 壳层仍集中于此；HTTP 原语与 endpoint 留在父模块。

#[cfg(not(feature = "no-http"))]
use std::ffi::CString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(not(feature = "no-http"))]
use esp_idf_sys::{
    heap_caps_get_largest_free_block, http_method_HTTP_GET, http_method_HTTP_POST, httpd_config_t,
    httpd_handle_t, httpd_register_uri_handler, httpd_start, httpd_uri_t, MALLOC_CAP_DEFAULT,
};
use esp_idf_sys::{nvs_flash_init, ESP_OK};

use super::wifi::{self, wifi_bringup};
#[cfg(not(feature = "no-http"))]
use super::{rr3_httpd_dispatch, HTTPD_BUS, HTTPD_SERVER_PORT, HTTPD_STACK_SIZE};
#[cfg(not(feature = "no-http"))]
use crate::httpd::RR3_BOARD_ROUTES;
use crate::state::Bus;

/// 起板侧配置腿（main 挂载点；**fail-honest**——任何一步失败点名后返回，
/// 配置腿缺席不阻塞既有链路；凭据未装配同口径）。
///
/// 凭据读链（批N14 件一，存储对象见 [`crate::cred`]）：**NVS 优先 → 编译期
/// `option_env!` 回落 → 都缺＝缺席照常**（NVS 空时与批N13 逐字零回归）。
/// NVS init 前移到本段（读链前件）——失败＝起栈链无健康前件，点名缺席
/// （不 erase——不毁既有分区）；真机 env 永远缺席的机制订正见模块文档凭据节。
pub(crate) fn rr3_httpd_spawn(bus: &Arc<Mutex<Bus>>) {
    let bus = Arc::clone(bus);
    if !nvs_ready() {
        return;
    }
    match cred_read() {
        Ok((cred_source, ssid, pass)) => thread_spawn(bus, cred_source, ssid, pass),
        Err(e) => println!("[periph] 配置腿装配失败（{e}）——缺席照常"),
    }
}

/// NVS init 前件（批N14 件一）：凭据读链前件，兼 WiFi 校准数据存储。
fn nvs_ready() -> bool {
    // SAFETY：nvs_flash_init 是 IDF 的一次性初始化，配置腿线程起栈前调用；
    // 失败即返回，不重复调用、不 erase 既有分区。
    if unsafe { nvs_flash_init() } != ESP_OK {
        println!("[periph] 配置腿未装配（nvs_flash_init 失败）——N-13 服务面缺席，板其余功能照常");
        return false;
    }
    true
}

/// 凭据读链：NVS 优先（读取窄缝 [`crate::cred::rr3_cred_read`]）→ 编译期回落 →
/// 都缺转 AP-only；形制校验两源同门（错误不出值只出长度）。返回（来源点名、SSID、密码）。
fn cred_read() -> Result<(&'static str, Vec<u8>, Vec<u8>), String> {
    let (ssid, pass, cred_source) = crate::cred::rr3_cred_read()
        .map(|c| {
            let pass = c.pass().to_string(); // 先经窄缝取密码（借用），再 move ssid
            (c.ssid, pass, "nvs")
        })
        // 编译期回落（option_env! 需字面量——构建期烘焙 .rodata，值不落仓）
        .or_else(|| {
            option_env!("RR3_BOARD_WIFI_SSID")
                .filter(|s| !s.is_empty())
                .map(|s| {
                    (
                        s.to_string(),
                        option_env!("RR3_BOARD_WIFI_PASS")
                            .unwrap_or_default()
                            .to_string(),
                        "编译期",
                    )
                })
        })
        // **2026-10-31 SoftAP 配网面**：两源都缺时改用 AP-only 起栈——原实现直接
        // 返回（配置腿缺席），板因此无法自身上线（配网入口要网络，构成鸡生蛋死锁）。
        // 无凭据也起 SoftAP（SSID 见 [`wifi_ap_configure`]），手机/笔记本连上板自己
        // 的面板即可配网；来源如实点名 "ap-only"，永不冒领已配网。
        .unwrap_or_else(|| {
            println!(
                "[periph] 无 WiFi 凭据（NVS 空且 RR3_BOARD_WIFI_SSID 未注入）→ 转 SoftAP 配网模式（AP-only，配置腿照常可达）"
            );
            (String::new(), String::new(), "ap-only")
        });
    if !ssid.is_empty() {
        if let Err(e) = crate::cred::rr3_cred_form_check(&ssid, &pass) {
            return Err(format!("{e}：SSID {}B/密码 {}B", ssid.len(), pass.len()));
        }
    }
    Ok((cred_source, ssid.into_bytes(), pass.into_bytes()))
}

/// net 任务心跳线程（W3-3）：只读网络面在位真值（[`wifi::rr3_net_up`]），
/// 每 1 s 一次；起线程失败只点名——net 任务随后按 timeout 判死，健康面可见。
#[cfg(feature = "autonomy")]
fn net_watch_spawn() {
    let spawned = std::thread::Builder::new()
        .name("net-watch".into())
        .stack_size(crate::board::THREAD_STACK)
        .spawn(|| loop {
            std::thread::sleep(Duration::from_secs(1));
            // SAFETY：rr3_net_up 只读启动期写一次的 netif 单例（见 wifi 模块）。
            if unsafe { wifi::rr3_net_up() } {
                crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Net);
            }
        });
    if let Err(e) = spawned {
        println!("[periph] net 监控线程起失败: {e:?}（net 任务将按 timeout 判死）");
    }
}

/// 起栈线程（与控制/运行面同 16KB 口径；短命——起完 httpd 即退；
/// `cred_source`＝凭据来源点名串（"nvs"/"编译期"），boot 日志用，永不出值）。
fn thread_spawn(bus: Arc<Mutex<Bus>>, cred_source: &'static str, ssid: Vec<u8>, pass: Vec<u8>) {
    std::thread::Builder::new()
        .name("periph-httpd".into())
        .stack_size(crate::board::THREAD_STACK)
        // SAFETY：闭包体整段是 unsafe 壳层调用（start_httpd），线程独占起栈链；
        // 捕获的 Arc/Vec 均 Send，无裸指针跨线程传递。
        .spawn(move || unsafe {
            if let Err(e) = start_httpd(&bus, cred_source, &ssid, &pass) {
                println!("[periph] 配置腿启动失败 esp_err={e}（缺席照常；负值＝IDF 错误码）");
            }
        })
        .expect("起配置腿线程失败");
}

/// httpd 起 + 白名单注册（手填 config：HTTPD_DEFAULT_CONFIG 真值——宏默认也是
/// false/null/0 的字段由全零起底等价覆盖，差异字段逐项对表 esp_http_server.h :53；
/// OnceLock 先写——handler 可达即 bus 在位；method 映射随表行 GET/POST，表内词表
/// 由 covers_wordform_shape 单测钉死，词表外 panic 不可达）。
///
/// # Safety
///
/// 只能在 `wifi_bringup` 成功后调用一次；`HTTPD_BUS` 先写后注册 handler，
/// 保证回调可达时 bus 必在位；注册表路径 CString 活到函数返回。
#[cfg(not(feature = "no-http"))]
unsafe fn httpd_register(bus: &Arc<Mutex<Bus>>) -> Result<(), i32> {
    let mut h: httpd_handle_t = std::ptr::null_mut();
    let conf = httpd_config_t {
        task_priority: 6, // tskIDLE_PRIORITY+5
        stack_size: HTTPD_STACK_SIZE,
        core_id: 0x7fff_ffff, // tskNO_AFFINITY
        server_port: HTTPD_SERVER_PORT,
        ctrl_port: 32768, // ESP_HTTPD_DEF_CTRL_PORT
        max_open_sockets: 7,
        // W4-2b 起白名单最多 9 条（tts 档）；留余量到 12，避免加端点时静默注册失败。
        max_uri_handlers: 12,
        max_resp_headers: 8,
        backlog_conn: 5,
        recv_wait_timeout: 5,
        send_wait_timeout: 5,
        ..Default::default()
    };
    let r = httpd_start(&mut h, &conf);
    crate::mem::rr3_heap_mark(
        "httpd",
        if r == ESP_OK {
            "start-ok"
        } else {
            "start-fail"
        },
    );
    let true = r == ESP_OK else { return Err(r) };
    let _ = HTTPD_BUS.set(Arc::clone(bus));
    for (method, path, _, _, _) in RR3_BOARD_ROUTES {
        let cpath = CString::new(*path).expect("白名单 path 不含 NUL");
        let m = match *method {
            "GET" => http_method_HTTP_GET,
            "POST" => http_method_HTTP_POST,
            _ => panic!("白名单 method 词表外: {method}"),
        };
        let uri = httpd_uri_t {
            uri: cpath.as_ptr(),
            method: m,
            handler: Some(rr3_httpd_dispatch),
            user_ctx: std::ptr::null_mut(),
        };
        let r = httpd_register_uri_handler(h, &uri);
        let true = r == ESP_OK else { return Err(r) };
    }
    Ok(())
}

/// HTTP 起栈的最小连续堆（httpd 任务栈 16KB + endpoint/连接结构余量）。
#[cfg(not(feature = "no-http"))]
const HTTPD_MIN_LARGEST: usize = 40 * 1024;

/// HTTP 配置面内存门：**WiFi/net 已在前面起好**，本门只决定 httpd 是否起栈。
/// 2026-11-21 classic 真板实测：`httpd_start` 的 16KB 任务分配在碎片化堆上失败
/// （0xB008），失败窗口又让播放线程 16KB 栈起不来、监督壳整机复位。最大连续块
/// 不足即降级关闭 HTTP（等同 no-http），保住播放/云链，不再用复位换配置面。
#[cfg(not(feature = "no-http"))]
fn httpd_mem_ok() -> bool {
    // SAFETY：无参只读的 IDF 堆账查询。
    let largest = unsafe { heap_caps_get_largest_free_block(MALLOC_CAP_DEFAULT) } as usize;
    if largest >= HTTPD_MIN_LARGEST {
        return true;
    }
    println!(
        "[periph] HTTP 配置面降级关闭：最大连续堆 {largest}B < {HTTPD_MIN_LARGEST}B（WiFi/net 已起；等同 no-http，播放/云链优先）"
    );
    false
}

/// 配置腿起栈成功点名（SSID 与白名单端点；端点表真值在 `RR3_BOARD_ROUTES`）。
#[cfg(not(feature = "no-http"))]
fn rr3_httpd_ready(cred_source: &str, ap_ssid: &str) {
    println!(
        "[periph] 配置腿起（N-13，凭据源={cred_source}）: SoftAP SSID={ap_ssid}（WPA2，口令见 wifi_ap_configure 注）+ http://0.0.0.0:{}/api/board/*（白名单 {} 端点，端点表＝RR3_BOARD_ROUTES）",
        HTTPD_SERVER_PORT,
        RR3_BOARD_ROUTES.len()
    );
}

/// WiFi 起栈 + httpd 起与注册（unsafe 集中段；NVS init 已前移到
/// [`rr3_httpd_spawn`]——本段只做起栈链与服务工作面）。
///
/// # Safety
///
/// 只能由配置腿线程调用一次；整段是 IDF 起栈链，重复或并发调用会与 IDF
/// 全局状态冲突（本函数不提供幂等保证）。
unsafe fn start_httpd(
    bus: &Arc<Mutex<Bus>>,
    cred_source: &str,
    ssid: &[u8],
    pass: &[u8],
) -> Result<(), i32> {
    let ap_ssid = wifi_bringup(ssid, pass)?;
    // ---- W3-3 net 任务接线：网络可达即心跳（1 s 一拍），**先于 httpd 起栈**。
    // 配置腿在 classic 上会因 httpd 分配失败（W7 实测 0xB008=ESP_ERR_HTTPD_ALLOC_MEM），但网络本身已起；
    // 心跳若绑在 httpd 成功之后，net 任务会被监督壳判死并整机复位（classic 实测
    // 8 次 SW_CPU_RESET）。HTTP 是配置面，不是网络在位的判据。
    #[cfg(feature = "autonomy")]
    net_watch_spawn();
    #[cfg(not(feature = "no-http"))]
    {
        if httpd_mem_ok() {
            httpd_register(bus)?;
            rr3_httpd_ready(cred_source, ap_ssid);
        }
    }
    #[cfg(feature = "no-http")]
    {
        let _ = bus;
        println!(
            "[periph] 全链档 no-http：WiFi 已起（AP SSID={ap_ssid}，凭据源={cred_source}），HTTP 配置面关闭（ADR-0030）"
        );
    }
    Ok(())
}
