//! rr3-brain 机制层注册面（批N32 迁移件1 立树，ADR-0017 五层表机制层）。
//!
//! [`Rr3BrainDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）。本件落
//! 事务面 on_rpc 三臂（module.toml [rpc].exposes 三条全接线）：`llm/completion`
//! 兜底（cloud 客户端随迁移件三激活）、`ops/list` 快照（[`rpc::rr3_ops_snapshot`]
//! 白名单组体）、`ops/set` 写臂（[`rpc::rr3_ops_set_arm`] 全语义：白名单/类型域
//! 校验 → live 内存生效不落盘）；未知 topic 返 Err 由 serve 折 `{"error": …}`
//! 应答（Python 主循环兜底分支同 wire）。事件面 on_event／节拍面 on_tick 空实现
//! 是迁移中间态——pcm/vad 订阅消费与 utterance_expired 判停编排随迁移件四落
//! （ADR-0016 增补裁定 2：ASR/LLM/TTS 编排挂 vad_start offset 臂＋expired 臂两路
//! 共用）。
//!
//! 错误语义两档（rr3mod 同款）：on_rpc 返 Err = 业务性失败折 `{"error": …}` 应答；
//! on_event/on_tick 返 Err = 传输性/设备级失败上抛致命（迁移件四的编排面按同一
//! 契约承载：云端 4xx/5xx/空文本/被调方超时臂内弃轮返 Ok，发布/应答写不出去
//! 上抛，模块不装"活着"，死交 supervisor 按 restart_policy 裁定）。

use rr3mod::{BusHandle, Dispatcher};
use serde_json::Value;

use crate::boot::Rr3BrainCfg;
use crate::rpc::{rr3_ops_set_arm, rr3_ops_snapshot};

/// 本模块 exposes 的 completion 点名（module.toml [rpc].exposes 同源）
pub(crate) const RR3_COMPLETION_TOPIC: &str = "rr3-brain/llm/completion";

/// ops 槽快照读口（批N4 件六二 B-12）
pub(crate) const RR3_OPS_LIST_TOPIC: &str = "rr3-brain/ops/list";

/// ops 槽写口（批N4 件九三 配置面）
pub(crate) const RR3_OPS_SET_TOPIC: &str = "rr3-brain/ops/set";

/// exposes 申报面（module.toml [rpc].exposes 同源三件；SURFACE 自述与未知 topic
/// 兜底文案共此一源——列表 repr 是 Python RPC_EXPOSES 的形状）
pub(crate) const RR3_RPC_EXPOSES: [&str; 3] = [
    "rr3-brain/llm/completion",
    "rr3-brain/ops/list",
    "rr3-brain/ops/set",
];

/// 主循环状态整体持有：config 文档树（ops/set live 真值）＋ boot 投影。
/// 迁移件三在此挂 cloud 三客户端（asr/llm/tts，ops/set 后重建）、迁移件四挂
/// dialog 状态（ops/set dialog 组就地改参）。
pub(crate) struct Rr3BrainDispatcher {
    cfg: Rr3BrainCfg,
}

impl Rr3BrainDispatcher {
    /// 装配构造（main.rs 装配段调用）：config 解析产物整体入表
    pub(crate) fn new(cfg: Rr3BrainCfg) -> Self {
        Self { cfg }
    }
}

impl Dispatcher for Rr3BrainDispatcher {
    /// rr3.call 点名应答（事务面三臂，module.toml exposes 同源）。薄委托
    /// [`rr3_rpc_arm`]——三臂是纯内存面不触总线（bus 参数留给 trait 形状），
    /// 纯函数即测试真值入口（仓外侧无法凭空构造 BusHandle）。
    fn on_rpc(&mut self, topic: &str, args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        rr3_rpc_arm(&mut self.cfg, topic, args)
    }

    /// 订阅面迁移中间态：pcm 积累＋vad_start 判停触发整轮随迁移件四落
    /// （module.toml subscribes 两条是申报面，事件过滤在臂内本地做）。
    fn on_event(&mut self, _topic: &str, _body: &Value, _bus: &BusHandle) -> Result<(), String> {
        Ok(())
    }

    /// 节拍面迁移中间态：utterance_expired 时长判停触发整轮随迁移件四落。
    fn on_tick(&mut self, _now: std::time::Instant, _bus: &BusHandle) -> Result<(), String> {
        Ok(())
    }
}

/// 事务面三臂（自 Python composite.py 主循环 RPC 分支逐语义迁入）：completion
/// 兜底＋ops/list 快照＋ops/set 写臂；未知 topic 返 Err 由 serve 折 `{"error": …}`
/// 应答（与 Python 主循环兜底分支同 wire）。
pub(crate) fn rr3_rpc_arm(
    cfg: &mut Rr3BrainCfg,
    topic: &str,
    args: &Value,
) -> Result<Value, String> {
    match topic {
        // completion 臂兜底：cloud LLM 客户端随迁移件三激活——届时此臂调
        // llm.complete（业务失败 Err 折 error 应答，与 Python try/except 同 wire）
        RR3_COMPLETION_TOPIC => Err(
            "rr3-brain llm/completion 暂不可用：云端客户端随迁移件三接入（迁移中间态）"
                .to_string(),
        ),
        RR3_OPS_LIST_TOPIC => Ok(serde_json::json!({ "ops": rr3_ops_snapshot(cfg)? })),
        RR3_OPS_SET_TOPIC => rr3_ops_set_arm(cfg, args),
        _ => {
            let list: Vec<String> = RR3_RPC_EXPOSES.iter().map(|t| format!("'{t}'")).collect();
            Err(format!(
                "rr3-brain 只 exposes [{}]，收到 {topic}",
                list.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::rr3_brain_config_parse;

    /// 与仓内 config.toml 同形状的最小配置夹具
    fn cfg() -> Rr3BrainCfg {
        rr3_brain_config_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n",
        )
        .unwrap()
    }

    /// 未知 topic 兜底文案逐字（Python f"rr3-brain 只 exposes {RPC_EXPOSES}，收到
    /// {rpc_topic}" 同形——列表 repr 是 Python 形状）
    #[test]
    fn unknown_topic_error_names_exposes_list() {
        let mut c = cfg();
        let e = rr3_rpc_arm(&mut c, "rr3-brain/other", &Value::Null).unwrap_err();
        assert_eq!(
            e,
            "rr3-brain 只 exposes ['rr3-brain/llm/completion', 'rr3-brain/ops/list', 'rr3-brain/ops/set']，收到 rr3-brain/other"
        );
    }

    /// completion 臂兜底 Err（迁移件三激活前的中间态声明）
    #[test]
    fn completion_arm_folds_error_until_piece3() {
        let mut c = cfg();
        let e = rr3_rpc_arm(&mut c, RR3_COMPLETION_TOPIC, &serde_json::json!({"text": "hi"}))
            .unwrap_err();
        assert!(e.contains("迁移件三"), "兜底文案点名中间态: {e}");
    }

    /// ops/list 臂经 rr3_rpc_arm 全链路可达（快照组体在 rpc.rs 逐键钉，此处钉臂位）
    #[test]
    fn ops_list_arm_reachable_via_rpc_arm() {
        let mut c = cfg();
        let v = rr3_rpc_arm(&mut c, RR3_OPS_LIST_TOPIC, &Value::Null).unwrap();
        assert_eq!(v["ops"]["dialog"]["max_utterance_ms"], 8000);
    }
}
