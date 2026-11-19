//! 自治形态的网络时间（W3-2a）：开机 SNTP 对时，失败显式点名、不静默降级。
//!
//! 只在 `--features autonomy` 时编译；默认外设底座固件零行为变化。
//! TLS 证书校验依赖正确系统时间，因此云请求前必须先确认 `synced=true`；
//! 本模块只负责对时与状态上报，是否发云请求由 dialog 任务决定。
//!
//! IDF 5.2.2 `esp_sntp_*` 为 lwIP SNTP 的线程安全别名（`esp_sntp.h`）：
//! `esp_sntp_setoperatingmode(0)` = POLL、`esp_sntp_setservername`、
//! `esp_sntp_init`、`sntp_get_sync_status() == 1` = 已同步（`esp_sntp_*` 是宏别名，
//! 绑定面只生成底层 `sntp_get_sync_status`）。

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use esp_idf_sys::{
    esp_sntp_init, esp_sntp_setoperatingmode, esp_sntp_setservername, sntp_get_sync_status,
};

/// 对时服务器（公网 NTP 池；内网部署可由 W3-2b 的配置面覆盖）。
const SNTP_SERVER: &[u8] = b"pool.ntp.org\0";
/// 首次对时等待上限与轮询间隔。
const SNTP_WAIT_S: u64 = 30;
const SNTP_POLL_MS: u64 = 1_000;
/// `sntp_sync_status_t::SNTP_SYNC_STATUS_COMPLETED`（esp_sntp.h）。
const SNTP_SYNCED: i32 = 1;

/// 起对时线程（不阻塞 main）；结果只打一行，失败不 panic。
pub(crate) fn spawn_sntp() {
    let spawned = thread::Builder::new()
        .name("sntp".into())
        .stack_size(16 * 1024)
        .spawn(sntp_run);
    if let Err(e) = spawned {
        println!("[sntp] 起对时线程失败: {e:?}");
    }
}

/// 对时主体：启动 SNTP POLL → 有界等待 → 成功/失败各一行。
fn sntp_run() {
    // SAFETY：四个 esp_sntp_* 都是 lwIP 线程安全封装；参数为常量与栈上 NUL
    // 结尾字符串，调用期间有效；POLL 模式值为 0（esp_sntp.h 枚举首项）。
    unsafe {
        esp_sntp_setoperatingmode(0);
        esp_sntp_setservername(0, SNTP_SERVER.as_ptr() as *const _);
        esp_sntp_init();
    }
    println!("[sntp] 启动 POLL 模式 server=pool.ntp.org 等待上限={SNTP_WAIT_S}s");
    let polls = SNTP_WAIT_S * 1_000 / SNTP_POLL_MS;
    for i in 1..=polls {
        thread::sleep(Duration::from_millis(SNTP_POLL_MS));
        // SAFETY：无参只读查询，返回值为 lwIP 同步状态枚举。
        let synced = unsafe { sntp_get_sync_status() } as i32 == SNTP_SYNCED;
        if synced {
            let epoch = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            println!("[sntp] 对时成功（第 {i} 次轮询）epoch={epoch}s");
            return;
        }
    }
    println!("[sntp] 对时失败：{SNTP_WAIT_S}s 内未同步（查 WiFi/DNS/UDP123）；云请求必须显式拒绝");
}
