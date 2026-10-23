//! rr3-brain 入口（批N32 迁移件1 立树，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述 → [`rr3mod::bootstrap`]（收敛序列前四段——
//! 启动序遵固定序，Python 侧「config 先于总线连接」的更早 fail-closed 不保留，
//! 批N32 增补裁定 4）→ config 就绪行（`connect_timeout_s` 段随键退役撤除）→
//! Dispatcher 构造 → [`rr3mod::serve`] 主循环（poll_wait 250ms 对位 Python 主
//! 循环 0.25s）。config 解析在 [`boot`]，ops 读写两面在 [`rpc`]，事务面三臂在
//! [`dispatch`]；语音主链（pcm/vad 订阅消费＋ASR→LLM→TTS 编排）随迁移件四落。

mod boot;
mod cloud;
mod dispatch;
mod rpc;

use std::time::Duration;

use rr3mod::{bootstrap, serve, Rr3ModConfig};

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_BRAIN_CONFIG_ENV: &str = "RR3_BRAIN_CONFIG";

/// 收帧等待上界兼循环节拍（Python 主循环 rr3_poll_wait(0.25) 对位，批N32 增补裁定 2）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

/// 申报发布面（与 module.toml [events].publishes 同源双写，composite.py PUBLISHES 同义）
const RR3_PUBLISHES: [&str; 2] = ["rr3-brain/intent/result", "rr3-brain/audio/tts_pcm"];

/// 申报订阅面（与 module.toml [events].subscribes 同源双写，C14 实然面：
/// pcm + vad_start 是语音主链入口；消费编排随迁移件四落）
const RR3_SUBSCRIBES: [&str; 2] = ["rr3-sense/audio/pcm", "rr3-sense/audio/vad_start"];

fn main() {
    if let Err(e) = rr3_brain_run() {
        eprintln!("rr3-brain 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自 Python composite.py `rr3_brain_run` 迁入，打印序列逐字不变；
/// config 就绪行 connect 段随键退役撤除——批N32 增补裁定 4）。
fn rr3_brain_run() -> Result<(), String> {
    // 启动自述保留骨架的 SURFACE 行：架构面"这个模块管什么"的说明（版本随
    // Python __version__ 逐字）
    eprintln!("rr3-brain 0.1.0 (M4 实装)");
    for topic in RR3_PUBLISHES {
        eprintln!("  publish   {topic}");
    }
    for topic in RR3_SUBSCRIBES {
        eprintln!("  subscribe {topic}");
    }
    for topic in dispatch::RR3_RPC_EXPOSES {
        eprintln!("  rpc       {topic}");
    }

    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字）
    let (bus, cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_BRAIN_CONFIG_ENV),
        parse_config: boot::rr3_brain_config_parse,
    })?;
    eprintln!(
        "config 就绪: cloud timeout_s={} bus call={}s dialog max_utterance_ms={}",
        boot::rr3_py_float_repr(cfg.cloud_timeout_s),
        boot::rr3_py_float_repr(cfg.call_timeout_s),
        cfg.max_utterance_ms,
    );

    // 机制层注册面构造 + 主循环骨架（rr3mod serve：死亡探针/分帧/RPC/订阅/节拍）
    let mut dispatcher = dispatch::Rr3BrainDispatcher::new(cfg);
    serve(bus, &mut dispatcher, RR3_POLL_WAIT)
}
