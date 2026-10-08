//! rr3-connect 组合件实现（组合件五件套③，0.0.5 批C④ 原子重铺）。
//!
//! 组合件 = wifi ⊗ bt（composite.toml 的 ⊗ 链；⊗=装配期、执行单元内函数调用，
//! ADR-0019 裁定 2）。本文件承载装配与运行机制：config 解析（上报周期/漫游
//! 探针/TLS 下限/超时经 config 数据化注入，机制件零策略载荷）、HAL 链路面
//! 开机探活、事务面四 exposes 的点名分发与主循环；session 云会话事务面留
//! 组合件（0.0.5 方案 §12.2——request 事务接入/运行面成分，session.rs）。
//!
//! 真入口语义（自原入口 rr3_run 逐字迁入，0.0.2 T4c + 0.0.4 批D 配网面）：
//! net/link 状态机周期流 + cloud/session 应答面 + wifi/scan|connect|forget 三 RPC。
//!
//! 启动序列与 rr3-act/rr3-power 同构：读 `RR3_MANIFEST`（rrd spawn 注入的
//! manifest 绝对路径，supervisor.rs）→ [`Rr3BusClient::rr3_connect`] → 读
//! config.toml → 选 HAL 链路面并**开机探活**（Unsupported 即 exit(1)：能力缺失
//! 显式回错，不静默带病常驻）→ 主循环。主循环做两件事：
//! 1. 应答本模块 exposes 的四个点名：`cloud/session`（action=open；链路未就绪
//!    折 error 应答回话）与配网面三 RPC（0.0.4 批D：scan 回 sim 模拟列表、
//!    connect/forget 改 [`wifi::Rr3Provision`] 状态机与内存凭据——业务性失败折
//!    error 应答回话，请求-应答语义不当致命错误）；
//! 2. 链路拍：每圈读一次 HAL 链路状态——翻转（`Rr3LinkEdge`）立即发
//!    `net/link` 不等周期；同态到点才发（周期保底，流是摘要不是逐拍镜像）；
//!    事件体带 `mode` 字段（配网态 ap|station，随流发布）。
//!
//! 失败语义与 sense/interact/act/power 同一口径：HAL 读数失败、发布/应答写不
//! 出去都是传输性/设备级，`eprintln` + `exit(1)` 交 supervisor 按
//! restart_policy=always 重启，不装"活着"。订阅面为空是**设计**（module.toml
//! 同条注）：本模块没有可订阅的命令流，广播帧只排空。
//!
//! net.bt 格位由 bt 原子 provide：蓝牙链路状态机骨架在 atoms/bt（0.0.4 批E，
//! D5/Q5）——转移纯函数单测钉死，sim 初值 env 注入；主题不登记（btctl 语义
//! 已被日志环 + rr3-ctl 承接），启用事件面前须先改 contracts/topics.toml。

#[path = "../atoms/bt/bt.rs"]
mod bt;
#[path = "session.rs"]
mod session;
#[path = "../atoms/wifi/wifi.rs"]
mod wifi;

use std::time::{Duration, Instant};

use rr_bus::Rr3BusClient;
use rr_hal::{backend::rr3_hal_link, rr3_block_on};
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

use session::Rr3SessionCfg;
use wifi::Rr3LinkCfg;

/// rrd spawn 时注入的 manifest 绝对路径（supervisor.rs；rr-ipc 尚无常量，模块侧本地定义）
const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml）
const RR3_CONNECT_CONFIG_ENV: &str = "RR3_CONNECT_CONFIG";

/// 收帧等待上界兼链路拍节拍下界：翻转检测每圈做，sim 读数廉价；值不敏感
const RR3_POLL_WAIT: Duration = Duration::from_millis(100);

/// 组合件入口：装配原子件并跑主循环（原 main.rs `rr3_run` 逐字迁入，打印序列不变）。
pub fn rr3_connect_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    println!("rr3-connect 0.0.1 (0.0.2 T4c 实装)");
    for line in wifi::SURFACE
        .iter()
        .copied()
        .chain(session::SURFACE.iter().copied())
        .chain(bt::SURFACE.iter().copied())
    {
        println!("  {line}");
    }

    // 总线接入：manifest 路径是 rrd spawn 的契约环境变量，直跑（开发调试）须自行设置
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let bus = Rr3BusClient::rr3_connect(&manifest_text)?;
    println!("总线就绪: name={} task={}", bus.rr3_name(), bus.rr3_task());

    let cfg = rr3_config_load(&manifest_path)?;
    println!(
        "config 就绪: link report={}ms roam_probe={} tls={} timeout={}ms",
        cfg.0.link_report_period_ms,
        cfg.0.roam_probe_enabled,
        cfg.1.tls_min_version,
        cfg.1.timeout_ms
    );

    // HAL 链路面选择 + 开机探活：Unsupported 即致命——显式回错不静默退化
    // （rr-hal 占位件的契约），也不给"没有链路读数还常驻"的假活
    let (dev, backend) = rr3_hal_link();
    let first = rr3_block_on(dev.state()).map_err(|e| format!("链路面探活失败({backend}): {e}"))?;
    println!(
        "链路面就绪: {backend} up={} ssid={:?} rssi={:?}",
        first.up, first.ssid, first.rssi_dbm
    );

    // 主循环状态：状态机 + 流序 + 周期拍点（首拍由状态机的基线分支保证必发）
    // + 配网状态机（凭据内存态，net/link 的 mode 词汇随流发布）
    let mut edge = wifi::Rr3LinkEdge::rr3_link_edge_new();
    let mut prov = wifi::Rr3Provision::rr3_provision_new();
    let mut link_seq: u64 = 0;
    let mut next_report = Instant::now();

    // 蓝牙骨架（0.0.4 批E）：sim 初值由 env 定死（缺省关，出厂不自动开）；
    // Off → Discoverable 在出厂态必成功。本进程暂无蓝牙事件面（主题不登记），
    // 相位只进开机自述——真后端（BlueZ 板上后端）接 Phase 时才有可发布语义。
    let mut bt_state = bt::Rr3BtState::rr3_bt_state_new();
    if bt::rr3_bt_backend_up_default() {
        bt_state.rr3_bt_power_on();
    }
    println!(
        "蓝牙面: phase={}（骨架，BlueZ 板上后置）",
        bt_state.rr3_bt_phase().rr3_bt_phase_name()
    );

    loop {
        // 总线死亡探针：断开后 poll_wait 立即返回空 Vec，与"暂时没帧"不可区分——
        // 不探测，总线一死本循环就满转成僵尸。假了就走"不装活着"的退出语义。
        if !bus.rr3_connected() {
            return Err("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定".into());
        }
        for frame in bus.rr3_poll_wait(RR3_POLL_WAIT) {
            if frame.header.opcode != opcode::SEND {
                continue;
            }
            // 事务面：rr3.call 转发帧（Request，trace_id=调用方 task，顺原路应答）
            if let Ok((Rr3PayloadType::Request, body)) = Rr3BusClient::rr3_frame_body(&frame) {
                if body.get("method").and_then(Value::as_str) != Some(rr_bus::RR3_METHOD_CALL) {
                    continue;
                }
                let id = body.get("id").and_then(Value::as_u64).unwrap_or(0);
                let params = body.get("params").cloned().unwrap_or(Value::Null);
                let topic = params.get("topic").and_then(Value::as_str).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or(Value::Null);

                // 业务性失败折 error 应答（请求-应答语义），不当致命错误；
                // HAL 读数失败也按业务性折 error（会话/配网本来就是问"现在能不能"）
                let reply = match topic {
                    session::RR3_SESSION_TOPIC => rr3_handle_session(dev.as_ref(), &args),
                    wifi::RR3_WIFI_SCAN_TOPIC => rr3_handle_wifi_scan(&args),
                    wifi::RR3_WIFI_CONNECT_TOPIC => {
                        rr3_handle_wifi_connect(dev.as_ref(), &mut prov, &args)
                    }
                    wifi::RR3_WIFI_FORGET_TOPIC => rr3_handle_wifi_forget(&mut prov, &args),
                    other => Err(format!(
                        "rr3-connect 只 exposes {} 与 wifi/scan|connect|forget，收到 {other}",
                        session::RR3_SESSION_TOPIC
                    )),
                };
                let reply = reply.unwrap_or_else(|e| json!({ "error": e }));
                bus.rr3_reply_call(frame.header.trace_id, id, reply)?; // 传输性失败 → exit(1)
                continue;
            }
            // 订阅面为空：广播（Event/Notification/Stream）只排空
        }

        // 链路拍：每圈读一次 HAL（sim 廉价；实机读 NetworkManager 同为廉价读数）。
        // 读数失败 = 设备/传输级，exit(1)——与 power 的电池读数同口径。
        let now = Instant::now();
        let st =
            rr3_block_on(dev.state()).map_err(|e| format!("链路读数失败（{backend}）: {e}"))?;
        if edge.rr3_link_edge_cross(st.up) || now >= next_report {
            link_seq += 1;
            let mode = prov.rr3_provision_mode();
            bus.rr3_publish(
                wifi::RR3_LINK_TOPIC,
                wifi::rr3_link_body(link_seq, &st, mode),
            )?;
            next_report = now + Duration::from_millis(cfg.0.link_report_period_ms);
        }
    }
}

/// cloud/session 点名：参数裁决 → 链路裁决 → mock 会话应答
fn rr3_handle_session(dev: &dyn rr_hal::Rr3Link, args: &Value) -> Result<Value, String> {
    session::rr3_session_parse_args(args)?;
    let st = rr3_block_on(dev.state()).map_err(|e| format!("链路读数失败: {e}"))?;
    let v = session::rr3_session_result(&st)?;
    println!("session: opened ssid={:?}", v["ssid"]);
    Ok(v)
}

/// wifi/scan 点名：无参裁决 → sim 模拟列表（backend=sim 诚实标注）
fn rr3_handle_wifi_scan(args: &Value) -> Result<Value, String> {
    wifi::rr3_wifi_scan_parse_args(args)?;
    Ok(wifi::rr3_wifi_scan_result())
}

/// wifi/connect 点名：参数裁决（legacy 语义）→ 存凭据转 station →
/// connected 如实取链路读数回执
fn rr3_handle_wifi_connect(
    dev: &dyn rr_hal::Rr3Link,
    prov: &mut wifi::Rr3Provision,
    args: &Value,
) -> Result<Value, String> {
    let (ssid, pass) = wifi::rr3_wifi_connect_parse_args(args)?;
    let st = rr3_block_on(dev.state()).map_err(|e| format!("链路读数失败: {e}"))?;
    prov.rr3_provision_connect(ssid.clone(), pass);
    println!("wifi: connect ssid={ssid} mode=station（凭据内存不落盘）");
    Ok(wifi::rr3_wifi_connect_result(&st, &ssid))
}

/// wifi/forget 点名：无参裁决 → 清凭据回 ap（恢复出厂 WiFi ≠ M6 整机恢复出厂）
fn rr3_handle_wifi_forget(prov: &mut wifi::Rr3Provision, args: &Value) -> Result<Value, String> {
    wifi::rr3_wifi_forget_parse_args(args)?;
    prov.rr3_provision_forget();
    println!("wifi: forget mode=ap");
    Ok(wifi::rr3_wifi_forget_result())
}

/// config.toml 解析：env 覆盖优先，缺省 manifest 同目录。
fn rr3_config_load(manifest_path: &str) -> Result<(Rr3LinkCfg, Rr3SessionCfg), String> {
    let path = std::env::var(RR3_CONNECT_CONFIG_ENV).unwrap_or_else(|_| {
        std::path::Path::new(manifest_path)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("config.toml")
            .to_string_lossy()
            .into_owned()
    });
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读 config {path} 失败: {e}"))?;
    rr3_connect_config_parse(&text)
}

/// TOML 文本 → (Rr3LinkCfg, Rr3SessionCfg)。手工下钻不引 serde derive：字段
/// 个位数，点名缺字段的文案比派生宏的默认报错可读（rr3-act/rr3-power 同款）。
fn rr3_connect_config_parse(text: &str) -> Result<(Rr3LinkCfg, Rr3SessionCfg), String> {
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
    let session_sect = sect("session")?.clone();
    let roam = wifi_sect
        .get("roam_probe_enabled")
        .and_then(toml::Value::as_bool)
        .ok_or_else(|| "config [rr3-connect.wifi] 缺 roam_probe_enabled（布尔）".to_string())?;
    let tls = session_sect
        .get("tls_min_version")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "config [rr3-connect.session] 缺 tls_min_version（字符串）".to_string())?
        .to_string();
    Ok((
        Rr3LinkCfg {
            link_report_period_ms: u64_of(&wifi_sect, "link_report_period_ms", "wifi")?.max(1),
            roam_probe_enabled: roam,
        },
        Rr3SessionCfg {
            tls_min_version: tls,
            timeout_ms: u64_of(&session_sect, "timeout_ms", "session")?.max(1),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-connect]
log_level = "info"

[rr3-connect.wifi]
link_report_period_ms = 5000
roam_probe_enabled = false

[rr3-connect.session]
tls_min_version = "1.3"
timeout_ms = 20000
"#;
        let (wifi, session) = rr3_connect_config_parse(text).unwrap();
        assert_eq!(wifi.link_report_period_ms, 5000);
        assert!(!wifi.roam_probe_enabled);
        assert_eq!(session.tls_min_version, "1.3");
        assert_eq!(session.timeout_ms, 20000);
    }

    #[test]
    fn config_parse_rejects_missing_sections_and_fields() {
        let e = rr3_connect_config_parse("[rr3-connect]\nlog_level = \"info\"\n").unwrap_err();
        assert!(e.contains("[rr3-connect.wifi]"), "缺段点名: {e}");
        let e = rr3_connect_config_parse(
            "[rr3-connect.wifi]\nlink_report_period_ms = 5000\n[rr3-connect.session]\ntls_min_version = \"1.3\"\ntimeout_ms = 20000\n",
        )
        .unwrap_err();
        assert!(e.contains("roam_probe_enabled"), "缺字段点名: {e}");
        let e = rr3_connect_config_parse("garbage =\n").unwrap_err();
        assert!(e.contains("TOML"), "坏文本点名: {e}");
    }
}
