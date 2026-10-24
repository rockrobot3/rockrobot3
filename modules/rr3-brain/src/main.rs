//! rr3-brain 入口（批N32 迁移件1 立树，ADR-0017 五层表装配层，≤80 行）。
//! 装配只做启动编排：SURFACE 自述（[`dispatch::rr3_brain_surface`]）→
//! [`rr3mod::bootstrap`]（收敛序列前四段——启动序遵固定序，Python 侧「config 先于
//! 总线连接」的更早 fail-closed 不保留，批N32 增补裁定 4）→ config 就绪行
//! （`connect_timeout_s` 段随键退役撤除）→ Dispatcher 构造 → [`rr3mod::serve`] 主循环
//! （poll_wait 250ms 对位 Python 主循环 0.25s）。config 解析与路径投影在 [`boot`]，
//! ops 读写两面在 [`rpc`]，事务面三臂与语音主链编排（pcm/vad 订阅消费＋
//! ASR→LLM→TTS→播放→intent）在 [`dispatch`]；话轮边界状态机在 [`dialog`]、意图
//! 词表在 [`intent`]；云端三家客户端与共享原语在 [`cloud`]，件四五件套拆分——
//! asr/llm/tts 各件纯机制经 `#[path]` 原位挂载。

#[path = "../atoms/dialog/dialog.rs"]
mod dialog;
#[path = "../atoms/asr/asr.rs"]
mod asr;
#[path = "../atoms/llm/llm.rs"]
mod llm;
#[path = "../atoms/tts/tts.rs"]
mod tts;
mod boot;
mod cloud;
mod config_store;
mod dispatch;
mod intent;
mod rpc;

use std::time::Duration;

use rr3mod::{bootstrap, serve, Rr3ModConfig};

/// config 覆盖入口（缺省取 manifest 同目录的 config.toml；契约环境变量名不变）
const RR3_BRAIN_CONFIG_ENV: &str = "RR3_BRAIN_CONFIG";

/// 收帧等待上界兼循环节拍（Python 主循环 rr3_poll_wait(0.25) 对位，批N32 增补裁定 2）
const RR3_POLL_WAIT: Duration = Duration::from_millis(250);

fn main() {
    if let Err(e) = rr3_brain_run() {
        eprintln!("rr3-brain 退出: {e}");
        std::process::exit(1);
    }
}

/// 装配序列（自 Python composite.py `rr3_brain_run` 迁入，打印序列逐字不变；
/// config 就绪行 connect 段随键退役撤除——批N32 增补裁定 4）。
fn rr3_brain_run() -> Result<(), String> {
    dispatch::rr3_brain_surface();
    // 总线接入 + config 读取解析（收敛序列前四段在 rr3mod bootstrap，文案逐字）
    let (bus, mut cfg) = bootstrap(Rr3ModConfig {
        config_env: Some(RR3_BRAIN_CONFIG_ENV),
        parse_config: boot::rr3_brain_config_parse,
    })?;
    // config 路径投影（ADR-0023 模型行落盘）：与 bootstrap 实读同一解析单点
    cfg.config_path = Some(boot::rr3_brain_config_path(Some(RR3_BRAIN_CONFIG_ENV))?);
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
