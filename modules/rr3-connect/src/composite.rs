//! rr3-connect 组合件实现（组合件五件套③，0.0.5 批C④ 原子重铺）。
//!
//! 组合件 = wifi ⊗ bt（composite.toml 的 ⊗ 链；⊗=装配期、执行单元内函数调用，
//! ADR-0019 裁定 2）。本文件承载装配与运行机制：config 解析（上报周期/nmcli
//! 子进程超时/蓝牙初值经 config 数据化注入，机制件零策略载荷）、HAL 链路面
//! 开机探活、事务面四 exposes 的点名分发与主循环；session 云会话事务面留
//! 组合件（0.0.5 方案 §12.2——request 事务接入/运行面成分，session.rs）；
//! 载荷组装与参数下钻面同在此处（0.0.5 批I N-7，ADR-0019 修订 4：wifi 件的
//! `*_body`/`*_result`/`*_parse_args` 上移，件面 serde_json 清零）。
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
//!    connect/forget 改 [`wifi::Rr3Provision`] 状态机与**后端承载的凭据**（MC-6：
//!    先过 `Rr3WifiStore` 落盘/真删，失败折 error 且状态机不动——业务性失败折
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
use rr_hal::backend::{rr3_hal_link, rr3_hal_wifi_store};
use rr_hal::provision_store::Rr3WifiStore;
use rr_hal::Rr3LinkState;
use rr_ipc::{opcode, Rr3PayloadType};
use serde_json::{json, Value};

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
        "config 就绪: link report={}ms nmcli timeout={}ms",
        cfg.0.link_report_period_ms, cfg.0.timeout_ms
    );

    // HAL 链路面选择 + 开机探活：Unsupported 即致命——显式回错不静默退化
    // （rr-hal 占位件的契约），也不给"没有链路读数还常驻"的假活。
    // nmcli 子进程超时由 config timeout_ms 传入（0.0.5 批I N-2：最坏翻转
    // 延迟由配置管住，不再由子进程自由决定）
    let (dev, backend) = rr3_hal_link(Duration::from_millis(cfg.0.timeout_ms));
    let first = dev
        .state()
        .map_err(|e| format!("链路面探活失败({backend}): {e}"))?;
    println!(
        "链路面就绪: {backend} up={} ssid={:?} rssi={:?}",
        first.up, first.ssid, first.rssi_dbm
    );

    // 主循环状态：状态机 + 流序 + 周期拍点（首拍由状态机的基线分支保证必发）
    // + 配网状态机（凭据态由后端承载持久化，net/link 的 mode 词汇随流发布）。
    // 凭据存储后端（MC-6）：与链路面同档子进程超时；构造不触设备，首次调用才起
    // nmcli。开机查在册 —— 在册则经 rr3_provision_restored 直接起 station
    // （重启仍在册），失败即致命（fail-closed，不静默当"没配过网"）。
    let (store, store_backend) = rr3_hal_wifi_store(Duration::from_millis(cfg.0.timeout_ms));
    let mut edge = wifi::Rr3LinkEdge::rr3_link_edge_new();
    let mut prov = match store
        .rr3_wifi_store_ssid()
        .map_err(|e| format!("配网凭据在册查询失败（{store_backend}）: {e}"))?
    {
        Some(ssid) => {
            let ssid = ssid.rr3_ssid_text_as_str().to_string();
            println!("配网面就绪: {store_backend} 在册 ssid={ssid:?}（重启恢复 station）");
            wifi::Rr3Provision::rr3_provision_restored(ssid)
        }
        None => {
            println!("配网面就绪: {store_backend} 无在册凭据（出厂 ap）");
            wifi::Rr3Provision::rr3_provision_new()
        }
    };
    let mut link_seq: u64 = 0;
    let mut next_report = Instant::now();

    // 蓝牙骨架（0.0.4 批E）：开机初值经 config 数据化注入（N-1：env 注入
    // 退役，缺省关=出厂不自动开蓝牙）。Off → Discoverable 的上电由构造初值
    // 承接（不走转移矩阵——装配期注入）。本进程暂无蓝牙事件面（主题不登记），
    // 相位只进开机自述——真后端接线接 Phase 时才有可发布语义。
    let bt_state = bt::Rr3BtState::rr3_bt_state_new(cfg.1);
    println!(
        "蓝牙面: phase={}（骨架，真后端板上后置）",
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
                        rr3_handle_wifi_connect(dev.as_ref(), store.as_ref(), &mut prov, &args)
                    }
                    wifi::RR3_WIFI_FORGET_TOPIC => {
                        rr3_handle_wifi_forget(store.as_ref(), &mut prov, &args)
                    }
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

        // 链路拍：每圈读一次 HAL（sim 廉价；实机 nmcli 子进程有 timeout_ms 守护，
        // N-2——读数失败 = 设备/传输级，exit(1)，与 power 的电池读数同口径）。
        let now = Instant::now();
        let st = dev
            .state()
            .map_err(|e| format!("链路读数失败（{backend}）: {e}"))?;
        if edge.rr3_link_edge_cross(st.up) || now >= next_report {
            link_seq += 1;
            let mode = prov.rr3_provision_mode();
            bus.rr3_publish(wifi::RR3_LINK_TOPIC, rr3_link_body(link_seq, &st, mode))?;
            next_report = now + Duration::from_millis(cfg.0.link_report_period_ms);
        }
    }
}

// ── 载荷组装与参数下钻面（0.0.5 批I N-7，ADR-0019 修订 4：json! 组装与 Value
// 下钻上移承载层，件面 serde_json 清零；错误文案逐字，每句落在唯一一处）──

/// net/link 事件体：seq 是本流自己的计数（envelope 惯例，从 1 起）。
/// ssid/rssi 断链时是 null——设备面（Rr3SimLink）已抹掉，这里如实透传。
/// `mode` 是配网状态机（[`wifi::Rr3Provision`]）的当前模式，0.0.4 批D append——
/// 契约只增不改：既有 seq/up/ssid/rssi_dbm 四字段原样保留。
/// ssid 序列化面：契约值类型 `Rr3SsidText` 出定长缓冲取 &str（serde_json 不
/// 进契约类型，F1 定形）。（自 wifi 原子件上移）
pub fn rr3_link_body(seq: u64, st: &Rr3LinkState, mode: &str) -> Value {
    json!({
        "seq": seq,
        "up": st.up,
        "ssid": st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
        "rssi_dbm": st.rssi_dbm,
        "mode": mode,
    })
}

/// wifi/scan 点名参数裁决：无参动作，`null` 与空对象放行，多余字段点名拒。
/// （自 wifi 原子件上移）
pub fn rr3_wifi_scan_parse_args(args: &Value) -> Result<(), String> {
    match args {
        Value::Null => Ok(()),
        Value::Object(m) if m.is_empty() => Ok(()),
        _ => Err("wifi/scan 无参数（收到多余字段）".into()),
    }
}

/// wifi/scan 结果（sim 后端）：确定性三行模拟列表，元素形状 = legacy panel.c
/// h_scan 的 `{"ssid","rssi","auth"}`（auth=是否加密，bool）；`backend:"sim"`
/// 诚实标注——sim 列表只保证形状与确定性，不保证"周围真有这些网"，板上换
/// NetworkManager 真扫描（cfg 门，方案 P4）。（自 wifi 原子件上移；假 AP 表
/// 整表迁 rr-hal sim 后端——0.0.5 批I N-9 落地，此处只组装形状，真值单点在
/// HAL 侧，首行 SSID 与链路 sim 默认 SSID 合流一处）
pub fn rr3_wifi_scan_result() -> Value {
    json!({
        "backend": "sim",
        "items": rr_hal::sim::rr3_sim_link_scan_list()
            .iter()
            .map(|(ssid, rssi, auth)| json!({ "ssid": ssid, "rssi": rssi, "auth": auth }))
            .collect::<Vec<Value>>(),
    })
}

/// wifi/connect 点名参数裁决（legacy panel.c h_connect 语义照搬）：ssid 非空、
/// 密码至少 8 位，文案点名（"密码至少 8 位"是 legacy 原词，配网页依赖它）。
/// 字段名沿用 legacy 的 `pass`——面板表单同名透传，零翻译。（自 wifi 原子件上移）
pub fn rr3_wifi_connect_parse_args(args: &Value) -> Result<(String, String), String> {
    let ssid = args
        .get("ssid")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let pass = args.get("pass").and_then(Value::as_str).unwrap_or("");
    if ssid.is_empty() {
        return Err("缺 ssid（非空字符串）".into());
    }
    if pass.len() < 8 {
        return Err("密码至少 8 位（legacy 配网语义）".into());
    }
    Ok((ssid.to_string(), pass.to_string()))
}

/// wifi/connect 结果：状态机已转 station；`connected` 如实取链路读数（sim 后端
/// 由 env 定死，不 fake"连上了"）——legacy"等连后回真实结果"的等待语义由
/// net/link 流承载，RPC 只回执状态机转移与当下读数。（自 wifi 原子件上移）
pub fn rr3_wifi_connect_result(st: &Rr3LinkState, ssid: &str) -> Value {
    json!({
        "ok": true,
        "connected": st.up,
        "mode": "station",
        "ssid": ssid,
    })
}

/// wifi/forget 点名参数裁决：无参动作（同 scan）。（自 wifi 原子件上移）
pub fn rr3_wifi_forget_parse_args(args: &Value) -> Result<(), String> {
    match args {
        Value::Null => Ok(()),
        Value::Object(m) if m.is_empty() => Ok(()),
        _ => Err("wifi/forget 无参数（收到多余字段）".into()),
    }
}

/// wifi/forget 结果：清凭据回 ap，回执转移后的模式。（自 wifi 原子件上移）
pub fn rr3_wifi_forget_result() -> Value {
    json!({ "ok": true, "mode": "ap" })
}

/// cloud/session 点名：参数裁决 → 链路裁决 → mock 会话应答
fn rr3_handle_session(dev: &dyn rr_hal::Rr3Link, args: &Value) -> Result<Value, String> {
    session::rr3_session_parse_args(args)?;
    let st = dev.state().map_err(|e| format!("链路读数失败: {e}"))?;
    let v = session::rr3_session_result(&st)?;
    println!("session: opened ssid={:?}", v["ssid"]);
    Ok(v)
}

/// wifi/scan 点名：无参裁决 → sim 模拟列表（backend=sim 诚实标注）
fn rr3_handle_wifi_scan(args: &Value) -> Result<Value, String> {
    rr3_wifi_scan_parse_args(args)?;
    Ok(rr3_wifi_scan_result())
}

/// wifi/connect 点名：参数裁决（legacy 语义）→ **先过凭据存储后端**（失败即
/// 折 error 应答、状态机不动，fail-closed）→ 存凭据转 station → connected
/// 如实取链路读数回执。
fn rr3_handle_wifi_connect(
    dev: &dyn rr_hal::Rr3Link,
    store: &dyn Rr3WifiStore,
    prov: &mut wifi::Rr3Provision,
    args: &Value,
) -> Result<Value, String> {
    let (ssid, pass) = rr3_wifi_connect_parse_args(args)?;
    let st = dev.state().map_err(|e| format!("链路读数失败: {e}"))?;
    store
        .rr3_wifi_store_connect(&ssid, &pass)
        .map_err(|e| format!("凭据持久化失败: {e}"))?;
    prov.rr3_provision_connect(ssid.clone(), pass);
    println!("wifi: connect ssid={ssid} mode=station（凭据由后端持久化，重启仍在册）");
    Ok(rr3_wifi_connect_result(&st, &ssid))
}

/// wifi/forget 点名：无参裁决 → **先真删后端配置**（失败折 error、状态机不动）
/// → 清凭据回 ap（恢复出厂 WiFi ≠ M6 整机恢复出厂）
fn rr3_handle_wifi_forget(
    store: &dyn Rr3WifiStore,
    prov: &mut wifi::Rr3Provision,
    args: &Value,
) -> Result<Value, String> {
    rr3_wifi_forget_parse_args(args)?;
    store
        .rr3_wifi_store_forget()
        .map_err(|e| format!("凭据删除失败: {e}"))?;
    prov.rr3_provision_forget();
    println!("wifi: forget mode=ap（连接配置已真删）");
    Ok(rr3_wifi_forget_result())
}

/// config.toml 解析：env 覆盖优先，缺省 manifest 同目录。
fn rr3_config_load(manifest_path: &str) -> Result<(Rr3LinkCfg, bool), String> {
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

/// TOML 文本 → (Rr3LinkCfg, bt 开机初值)。手工下钻不引 serde derive：字段
/// 个位数，点名缺字段的文案比派生宏的默认报错可读（rr3-act/rr3-power 同款）。
/// 0.0.5 批I N-6：无读者的 roam_probe_enabled / tls_min_version / 总段 log_level
/// 删除；N-2：timeout_ms 迁 wifi 段，生效点 = nmcli 子进程超时。
fn rr3_connect_config_parse(text: &str) -> Result<(Rr3LinkCfg, bool), String> {
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
        },
        bt_up,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_hal::Rr3SsidText;

    fn st(up: bool) -> Rr3LinkState {
        Rr3LinkState {
            up,
            ssid: up.then(|| Rr3SsidText::rr3_ssid_text_new("rock-sim")),
            rssi_dbm: up.then_some(-50),
        }
    }

    /// 与仓内 config.toml 同形状的最小配置必须解析成功——漂移当场红
    #[test]
    fn config_parse_covers_repo_shape() {
        let text = r#"
[rr3-connect.wifi]
link_report_period_ms = 5000
timeout_ms = 20000

[rr3-connect.bt]
up = false
"#;
        let (wifi, bt_up) = rr3_connect_config_parse(text).unwrap();
        assert_eq!(wifi.link_report_period_ms, 5000);
        assert_eq!(wifi.timeout_ms, 20000);
        assert!(!bt_up, "未写 bt 段 = 出厂不自动开蓝牙");
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

    // ── 载荷组装与下钻用例（随 N-7 自件面迁入：件测"输入→结构体/判定"，
    // 承载层测"输入→字节"）──

    /// net/link 事件体逐字对齐契约：seq 自计数、断链时 ssid/rssi 必为 null
    /// （§9 红线：up=false 不许从死链读身份）、0.0.4 批D append 的 mode 随流
    #[test]
    fn link_body_carries_full_state_with_own_seq() {
        let v = rr3_link_body(3, &st(true), "station");
        assert_eq!(v["seq"], 3);
        assert_eq!(v["up"], true);
        assert_eq!(v["ssid"], "rock-sim");
        assert_eq!(v["rssi_dbm"], -50);
        assert_eq!(v["mode"], "station", "0.0.4 批D append：mode 随流发布");
        let v = rr3_link_body(4, &st(false), "ap");
        assert_eq!(v["up"], false);
        assert_eq!(v["mode"], "ap");
        assert!(
            v["ssid"].is_null() && v["rssi_dbm"].is_null(),
            "断链时 ssid/rssi 必须是 null（设备面已抹掉）: {v}"
        );
    }

    /// wifi/connect 参数裁决（legacy 语义）：合法放行；缺 ssid / 空 ssid /
    /// 缺 pass / 密码 <8 位 全部点名拒，"密码至少 8 位"是 legacy 原词。
    /// 等号 8 位是边界（§8 打折过的边界用例，本批补上）
    #[test]
    fn wifi_connect_args_parse_legacy_semantics() {
        let (ssid, pass) =
            rr3_wifi_connect_parse_args(&json!({"ssid": " home ", "pass": "password8"})).unwrap();
        assert_eq!(
            (ssid.as_str(), pass.as_str()),
            ("home", "password8"),
            "ssid 去首尾空白"
        );
        let (ssid, _) =
            rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "12345678"})).unwrap();
        assert_eq!(ssid, "home", "恰好 8 位放行（等号边界）");
        let e = rr3_wifi_connect_parse_args(&json!({"pass": "password8"})).unwrap_err();
        assert!(e.contains("ssid"), "缺 ssid 点名: {e}");
        let e =
            rr3_wifi_connect_parse_args(&json!({"ssid": "  ", "pass": "password8"})).unwrap_err();
        assert!(e.contains("ssid"), "空 ssid 点名: {e}");
        let e = rr3_wifi_connect_parse_args(&json!({"ssid": "home"})).unwrap_err();
        assert!(e.contains("8 位"), "缺 pass 点名: {e}");
        let e = rr3_wifi_connect_parse_args(&json!({"ssid": "home", "pass": "short"})).unwrap_err();
        assert!(
            e.contains("密码至少 8 位"),
            "短密码点名（legacy 原词）: {e}"
        );
    }

    /// wifi/scan|forget 无参裁决 + 结果形状：sim 诚实标注、items 三行、
    /// 元素三字段（legacy panel.c h_scan 形状）；forget 回执 ap。
    #[test]
    fn wifi_scan_and_forget_shapes() {
        assert_eq!(rr3_wifi_scan_parse_args(&Value::Null), Ok(()));
        assert_eq!(rr3_wifi_scan_parse_args(&json!({})), Ok(()));
        assert!(
            rr3_wifi_scan_parse_args(&json!({"n": 5})).is_err(),
            "多余字段点名拒"
        );
        assert_eq!(rr3_wifi_forget_parse_args(&json!({})), Ok(()));
        assert!(rr3_wifi_forget_parse_args(&json!({"x": 1})).is_err());

        let v = rr3_wifi_scan_result();
        assert_eq!(v["backend"], "sim", "sim 后端诚实标注");
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        for it in items {
            assert!(it.get("ssid").and_then(Value::as_str).is_some());
            assert!(it.get("rssi").and_then(Value::as_i64).is_some());
            assert!(it.get("auth").and_then(Value::as_bool).is_some());
        }
        assert_eq!(rr3_wifi_forget_result()["mode"], "ap");
    }

    /// wifi/connect 结果：connected 如实透传链路读数（不 fake"连上了"），
    /// mode 恒 station、ssid 回显。
    #[test]
    fn wifi_connect_result_reports_link_honestly() {
        let v = rr3_wifi_connect_result(&st(true), "home");
        assert_eq!(v["ok"], true);
        assert_eq!(v["connected"], true);
        assert_eq!(v["mode"], "station");
        assert_eq!(v["ssid"], "home");
        let v = rr3_wifi_connect_result(&st(false), "home");
        assert_eq!(v["connected"], false, "链路 down 时 connected 如实 false");
    }

    // ── MC-6 配网凭据持久化：handler 编排（sim 链路 + sim 存储）──

    /// connect 先落后端再转态、forget 真删后端——handler 编排两腿与状态机
    /// 同向；"重启仍在册"的承载半边在 rr-hal::provision_store 用例钉（本处只
    /// 钉编排顺序与状态机同向）。
    #[test]
    fn connect_and_forget_handlers_move_store_and_state_together() {
        let dev = rr_hal::sim::Rr3SimLink::rr3_sim_link_new(true, "rock-sim", -50);
        let store = rr_hal::provision_store::Rr3SimWifiStore::default();
        let mut prov = wifi::Rr3Provision::rr3_provision_new();

        let v = rr3_handle_wifi_connect(
            &dev,
            &store,
            &mut prov,
            &json!({"ssid": "home", "pass": "password8"}),
        )
        .unwrap();
        assert_eq!(v["mode"], "station");
        assert_eq!(prov.rr3_provision_mode(), "station", "状态机随存储同向转");
        assert_eq!(
            store
                .rr3_wifi_store_ssid()
                .unwrap()
                .map(|s| s.rr3_ssid_text_as_str().to_string()),
            Some("home".to_string()),
            "凭据已落后端"
        );

        let v = rr3_handle_wifi_forget(&store, &mut prov, &json!({})).unwrap();
        assert_eq!(v["mode"], "ap");
        assert_eq!(prov.rr3_provision_mode(), "ap");
        assert!(
            store.rr3_wifi_store_ssid().unwrap().is_none(),
            "forget 后端配置真删"
        );
    }

    /// fail-closed：存储失败时状态机**不动**、错误点名——不许"存储没成功却
    /// 当作配上网了"（存储是持久化真值，状态机跟着它走）。
    #[test]
    fn store_failure_leaves_provision_state_untouched() {
        struct Rr3FailingStore;
        impl Rr3WifiStore for Rr3FailingStore {
            fn rr3_wifi_store_connect(
                &self,
                _ssid: &str,
                _pass: &str,
            ) -> Result<(), rr_hal::Rr3HalError> {
                Err(rr_hal::Rr3HalError::Unsupported("test/failing-store"))
            }
            fn rr3_wifi_store_forget(&self) -> Result<(), rr_hal::Rr3HalError> {
                Err(rr_hal::Rr3HalError::Unsupported("test/failing-store"))
            }
            fn rr3_wifi_store_ssid(
                &self,
            ) -> Result<Option<rr_hal::Rr3SsidText>, rr_hal::Rr3HalError> {
                Err(rr_hal::Rr3HalError::Unsupported("test/failing-store"))
            }
        }
        let dev = rr_hal::sim::Rr3SimLink::rr3_sim_link_new(true, "rock-sim", -50);
        let store = Rr3FailingStore;
        let mut prov = wifi::Rr3Provision::rr3_provision_new();

        let e = rr3_handle_wifi_connect(
            &dev,
            &store,
            &mut prov,
            &json!({"ssid": "home", "pass": "password8"}),
        )
        .unwrap_err();
        assert!(e.contains("凭据持久化失败"), "存储失败点名: {e}");
        assert_eq!(
            prov.rr3_provision_mode(),
            "ap",
            "存储没成功，状态机不许转 station（fail-closed）"
        );

        let e = rr3_handle_wifi_forget(&store, &mut prov, &json!({})).unwrap_err();
        assert!(e.contains("凭据删除失败"), "删除失败点名: {e}");
    }
}
