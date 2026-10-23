//! rr3-connect 入口（0.0.8 W-A 批2 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → [`rr3mod::bootstrap`]（收敛序列前四段）→
//! config 就绪行 → 链路面/配网面探活（[`boot`]）→ bt 相位自述（装配段，方案
//! §4.2「自述属装配」字面）→ Dispatcher 构造 → [`rr3mod::serve`] 主循环。
//! 机制在 [`dispatch`]（链路拍＋五臂点名分发），协议面在 [`rpc`]（四 handler＋
//! 载荷组装面），原子件在 `atoms/` 原位不动；session 云会话事务面留本模块
//! （0.0.5 方案 §12.2——request 事务接入/运行面成分）。

#[path = "../atoms/bt/bt.rs"]
mod bt;
#[path = "../atoms/wifi/wifi.rs"]
mod wifi;
mod boot;
mod dispatch;
mod rpc;
mod session;

use std::time::Duration;

use rr3mod::{bootstrap, serve, Rr3ModConfig};

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_CONNECT_CONFIG_ENV: &str = "RR3_CONNECT_CONFIG";

/// 收帧等待上界兼链路拍节拍下界：翻转检测每圈做，sim 读数廉价；值不敏感
const RR3_POLL_WAIT: Duration = Duration::from_millis(100);

fn main() {
    if let Err(e) = rr3_connect_run() {
        eprintln!("rr3-connect 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自原 composite.rs `rr3_connect_run` 迁入，打印序列逐字不变）。
fn rr3_connect_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    eprintln!("rr3-connect 0.0.1 (0.0.2 T4c 实装)");
    for line in wifi::SURFACE
        .iter()
        .copied()
        .chain(session::SURFACE.iter().copied())
        .chain(bt::SURFACE.iter().copied())
    {
        eprintln!("  {line}");
    }

    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字；
    // 路径段 RR3_CONNECT_CONFIG_ENV 覆盖 + manifest 同目录缺省归 load_config_text）
    let (bus, cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_CONNECT_CONFIG_ENV),
        parse_config: boot::rr3_connect_config_parse,
    })?;
    eprintln!(
        "config 就绪: link report={}ms nmcli timeout={}ms",
        cfg.0.link_report_period_ms, cfg.0.timeout_ms
    );

    // HAL 链路面探活 + 配网凭据在册编排（接入层，Unsupported / 在册查询失败即
    // 致命——fail-closed 显式回错，不静默带病常驻也不假活）
    let (dev, backend) = boot::rr3_connect_probe(cfg.0.timeout_ms)?;
    let (store, prov) = boot::rr3_connect_provision(cfg.0.timeout_ms)?;

    // 蓝牙骨架（0.0.4 批E）：Off → Discoverable 的上电由构造初值承接（不走
    // 转移矩阵——装配期注入）。本进程暂无蓝牙事件面（主题不登记），相位只进
    // 开机自述——真后端接线接 Phase 时才有可发布语义。
    let bt_state = bt::Rr3BtState::rr3_bt_state_new(cfg.1);
    eprintln!(
        "蓝牙面: phase={}（骨架，真后端板上后置）",
        bt_state.rr3_bt_phase().rr3_bt_phase_name()
    );

    // 机制层注册面构造 + 主循环骨架（rr3mod serve：死亡探针/分帧/RPC/订阅/节拍）
    let mut dispatcher =
        dispatch::Rr3ConnectDispatcher::new(dev, backend, store, cfg.0, cfg.1, prov);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
