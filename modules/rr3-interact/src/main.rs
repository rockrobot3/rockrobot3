//! rr3-interact 入口（0.0.8 W-A 批4 拆分，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → [`rr3mod::bootstrap`]（收敛序列前四段）→
//! config 就绪行 → 播放面/显示面探活（[`boot`]）→ 面板线程宿主（panel 一行
//! 装配）→ Dispatcher 构造 → [`rr3mod::serve`] 主循环。机制在 [`dispatch`]
//! （收流缓冲＋beep＋动作排空＋表情帧节拍），协议面在 [`rpc`]（载荷组装＋错误
//! 文案＋点名执行）；face/player 两原子与 panel 三件原位不动，功能守恒零改动。

#[path = "../atoms/face/face.rs"]
mod face;
#[path = "panel/mod.rs"]
mod panel;
#[path = "../atoms/player/player.rs"]
mod player;
mod boot;
mod dispatch;
mod rpc;

use std::time::Duration;

use rr3mod::{bootstrap, serve, Rr3ModConfig};

use face::rr3_face_non_pointable_join;

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_INTERACT_CONFIG_ENV: &str = "RR3_INTERACT_CONFIG";

/// 收帧等待上界：无点名时空转一圈的代价，值不敏感（心跳在独立线程）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

fn main() {
    if let Err(e) = rr3_interact_run() {
        eprintln!("rr3-interact 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自原 composite.rs `rr3_interact_run` 迁入，打印序列逐字不变）。
fn rr3_interact_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的一行说明
    eprintln!("rr3-interact 0.0.1 (M3 实装)");
    for line in face::SURFACE.iter().copied().chain(player::SURFACE.iter().copied()) {
        eprintln!("  {line}");
    }
    // panel 自述是推导文案（端点数由 RR3_PANEL_ROUTES 推导，MC-8③），String 面单独成段
    for line in panel::surface() {
        eprintln!("  {line}");
    }

    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字；
    // 路径段 RR3_INTERACT_CONFIG_ENV 覆盖 + manifest 同目录缺省归 load_config_text）
    let (bus, cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_INTERACT_CONFIG_ENV),
        parse_config: boot::rr3_interact_config_parse,
    })?;
    // config 就绪行（注入的策略面在启动自述里可见：I-11 改数据即改行为，
    // 日志是第一证据；I-17/I-18②/F-31 新增键随行）
    eprintln!(
        "config 就绪: face non_pointable={} tick_ms={} default={} player prebuffer_ms={} volume_pct={} panel max_conns={} selftest face_expect={} expect[speaker={},relay={},face={},wiring={},contracts={}]",
        rr3_face_non_pointable_join(&cfg.face.non_pointable), cfg.face.face_tick_ms,
        cfg.face.default_expr, cfg.player.prebuffer_ms, cfg.player.volume_pct,
        cfg.panel.max_conns, cfg.selftest.face_expect, cfg.selftest.expect.speaker,
        cfg.selftest.expect.relay, cfg.selftest.expect.face, cfg.selftest.expect.wiring,
        cfg.selftest.expect.contracts
    );

    // 播放面 + 显示面选择与开机探活（打开/探活失败 Err 上抛——fail-closed
    // 显式回错，不静默带病常驻）
    let (sink, display) = boot::rr3_interact_probe()?;
    // 面板线程宿主（0.0.4 批C Q3 裁定：独立 HTTP 线程 + 动作通道；批4 函数化
    // 一行装配——link 缓存与动作收端返回给 Dispatcher）
    let (act_rx, link) = panel::server::spawn_host(&bus, &cfg.panel, &cfg.selftest)?;
    // 机制层注册面构造 + 主循环骨架（rr3mod serve：死亡探针/分帧/RPC/订阅/节拍）
    let mut dispatcher = dispatch::Rr3InteractDispatcher::new(sink, display, cfg, link, act_rx);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
