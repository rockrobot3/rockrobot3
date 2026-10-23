//! rr3-brain 机制层注册面（批N32 迁移件1 立树；迁移件2 起 cloud 三客户端在列）。
//!
//! [`Rr3BrainDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）。本件落
//! 事务面 on_rpc 三臂（module.toml [rpc].exposes 三条全接线）：`llm/completion`
//! 已激活（[`cloud::Rr3LlmClient`]，mock 档回显/real 档带历史真调；业务失败 Err
//! 折 `{"error": …}` 应答——Python try/except RuntimeError 同 wire）、`ops/list`
//! 快照（[`rpc::rr3_ops_snapshot`] 白名单组体）、`ops/set` 写臂（[`rpc::rr3_ops_set_arm`]
//! 全语义：白名单/类型域校验 → live 内存生效不落盘 → 按组重建受影响客户端——
//! asr/tts/llm 三组各重建、dialog 组无客户端动作）；未知 topic 返 Err 由 serve 折
//! `{"error": …}` 应答（Python 主循环兜底分支同 wire）。事件面 on_event／节拍面
//! on_tick 空实现是迁移中间态——pcm/vad 订阅消费与 utterance_expired 判停编排随
//! 迁移件四落（ADR-0016 增补裁定 2：ASR/LLM/TTS 编排挂 vad_start offset 臂＋
//! expired 臂两路共用）。
//!
//! 错误语义两档（rr3mod 同款）：on_rpc 返 Err = 业务性失败折 `{"error": …}` 应答；
//! on_event/on_tick 返 Err = 传输性/设备级失败上抛致命（迁移件四的编排面按同一
//! 契约承载：云端 4xx/5xx/空文本/被调方超时臂内弃轮返 Ok，发布/应答写不出去
//! 上抛，模块不装"活着"，死交 supervisor 按 restart_policy 裁定）。

use rr3mod::{BusHandle, Dispatcher};
use serde_json::{json, Value};

use crate::boot::{rr3_cloud_cfg_of, Rr3BrainCfg};
use crate::cloud::{Rr3AsrClient, Rr3LlmClient, Rr3TtsClient};
use crate::rpc::{rr3_ops_set_arm, rr3_ops_snapshot, rr3_py_str, rr3_model_row_of};

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

/// cloud 三客户端整体（批N32 迁移件2：Python composite 装配段同形状——asr/tts
/// 读 cloud 段、llm 读模型行实效；ops/set 写后按组重建的重建目标）。
pub(crate) struct Rr3CloudClients {
    pub(crate) asr: Rr3AsrClient,
    pub(crate) llm: Rr3LlmClient,
    pub(crate) tts: Rr3TtsClient,
}

impl Rr3CloudClients {
    /// 装配构造（main.rs 装配段与测试共用）：cloud 段四键＋模型行实效入表。
    /// boot 校验保证在册，取数失败 panic 点名（不可达路径 fail-closed）。
    pub(crate) fn from_cfg(cfg: &Rr3BrainCfg) -> Self {
        let cloud = rr3_cloud_cfg_of(&cfg.doc).expect("config [rr3-brain.cloud] 段 boot 校验在册");
        Self {
            asr: Rr3AsrClient::new(&cloud.base_url, &cloud.asr_model, cloud.timeout_s),
            llm: Rr3LlmClient::new(
                &cfg.model.base_url,
                &cfg.model.model_id,
                cloud.timeout_s,
                cfg.model.key_ref.clone(),
                cfg.model.mode,
            ),
            tts: Rr3TtsClient::new(&cloud.base_url, &cloud.tts_model, cloud.timeout_s),
        }
    }
}

/// 按组重建受影响客户端（自 Python composite.py ops/set 分支的重建段逐语义迁入）：
/// asr/tts 组读 cloud 段现值、llm 组重解析模型行实效（超时恒读 cloud 段 timeout_s
/// ——Python float(cloud["timeout_s"]) 同形状）；dialog 组无客户端动作（参数消费
/// 随迁移件四）。取数失败返 Err——Python 同路径 RuntimeError 逃逸主循环 exit(1)，
/// 此处由 on_rpc 折 error 应答（写已生效；该路径写面已拦结构错误、实际不可达，
/// 收敛宽一笔如实登记）。
fn rr3_rebuild_clients(
    cfg: &Rr3BrainCfg,
    group: &str,
    clients: &mut Rr3CloudClients,
) -> Result<(), String> {
    match group {
        "asr" => {
            let cloud = rr3_cloud_cfg_of(&cfg.doc)?;
            clients.asr = Rr3AsrClient::new(&cloud.base_url, &cloud.asr_model, cloud.timeout_s);
        }
        "tts" => {
            let cloud = rr3_cloud_cfg_of(&cfg.doc)?;
            clients.tts = Rr3TtsClient::new(&cloud.base_url, &cloud.tts_model, cloud.timeout_s);
        }
        "llm" => {
            let row = rr3_model_row_of(cfg)?;
            let timeout_s = rr3_cloud_cfg_of(&cfg.doc)?.timeout_s;
            clients.llm = Rr3LlmClient::new(
                &row.base_url,
                &row.model_id,
                timeout_s,
                row.key_ref,
                row.mode,
            );
        }
        _ => {} // dialog（rr3_ops_write 白名单保证四组，其余即 dialog）
    }
    Ok(())
}

/// 主循环状态整体持有：config 文档树（ops/set live 真值）＋ boot 投影＋cloud 三
/// 客户端（completion 臂消费、ops/set 写后重建）。迁移件四在此挂 dialog 状态
/// （ops/set dialog 组就地改参）。
pub(crate) struct Rr3BrainDispatcher {
    cfg: Rr3BrainCfg,
    clients: Rr3CloudClients,
}

impl Rr3BrainDispatcher {
    /// 装配构造（main.rs 装配段调用）：config 解析产物整体入表＋三客户端装配
    pub(crate) fn new(cfg: Rr3BrainCfg) -> Self {
        let clients = Rr3CloudClients::from_cfg(&cfg);
        Self { cfg, clients }
    }
}

impl Dispatcher for Rr3BrainDispatcher {
    /// rr3.call 点名应答（事务面三臂，module.toml exposes 同源）。薄委托
    /// [`rr3_rpc_arm`]——三臂是纯内存面不触总线（bus 参数留给 trait 形状），
    /// 纯函数即测试真值入口（仓外侧无法凭空构造 BusHandle）。
    fn on_rpc(&mut self, topic: &str, args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        rr3_rpc_arm(&mut self.cfg, &mut self.clients, topic, args)
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
/// （[`Rr3LlmClient`]）＋ops/list 快照＋ops/set 写臂（写 doc → 按组重建受影响
/// 客户端）；未知 topic 返 Err 由 serve 折 `{"error": …}` 应答（与 Python 主循环
/// 兜底分支同 wire）。
pub(crate) fn rr3_rpc_arm(
    cfg: &mut Rr3BrainCfg,
    clients: &mut Rr3CloudClients,
    topic: &str,
    args: &Value,
) -> Result<Value, String> {
    match topic {
        // completion 臂：业务失败（缺密钥/HTTP 4xx-5xx/应答形状不符）Err 折 error
        // 应答——Python try/except RuntimeError 同 wire（传输异常已在 cloud 层折
        // 同型 Err，同一次云不可用结局只剩一种：弃轮/错误应答，进程不死）
        RR3_COMPLETION_TOPIC => {
            // Python: text = str(params.get("args", {}).get("text", ""))
            let text = args.get("text").map(rr3_py_str).unwrap_or_default();
            Ok(json!({ "text": clients.llm.rr3_complete(&text)? }))
        }
        RR3_OPS_LIST_TOPIC => Ok(json!({ "ops": rr3_ops_snapshot(cfg)? })),
        RR3_OPS_SET_TOPIC => {
            let reply = rr3_ops_set_arm(cfg, args)?;
            // Python: group = str(args.get("group", ""))（同 ops/set 臂的 coercion）
            let group = args.get("group").map(rr3_py_str).unwrap_or_default();
            rr3_rebuild_clients(cfg, &group, clients)?;
            Ok(reply)
        }
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

    /// env 动作的测试互斥（全 crate 一柄，与 cloud.rs tests 共享——RR3_BRAIN_CLOUD
    /// 等键是进程级全局，并行测试互相污染）
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::cloud::RR3_TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 与仓内 config.toml 同形状的最小配置夹具
    fn cfg() -> Rr3BrainCfg {
        rr3_brain_config_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n",
        )
        .unwrap()
    }

    /// mock 档三客户端夹具（completion 臂激活测试用——mock 不触网不读 key）。
    /// 返回锁柄随测试全程持有：mock 判据是现读 env（rr3_cloud_mode），释锁后
    /// 其他测试改档会让臂半途换档。
    fn mock_clients(cfg: &Rr3BrainCfg) -> (Rr3CloudClients, std::sync::MutexGuard<'static, ()>) {
        let g = env_lock();
        std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "mock");
        (Rr3CloudClients::from_cfg(cfg), g)
    }

    /// 未知 topic 兜底文案逐字（Python f"rr3-brain 只 exposes {RPC_EXPOSES}，收到
    /// {rpc_topic}" 同形——列表 repr 是 Python 形状）
    #[test]
    fn unknown_topic_error_names_exposes_list() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let e = rr3_rpc_arm(&mut c, &mut cl, "rr3-brain/other", &Value::Null).unwrap_err();
        assert_eq!(
            e,
            "rr3-brain 只 exposes ['rr3-brain/llm/completion', 'rr3-brain/ops/list', 'rr3-brain/ops/set']，收到 rr3-brain/other"
        );
    }

    /// completion 臂激活（迁移件2）：mock 档回显＋{"text": …} 应答体（Python
    /// result = {"text": llm.complete(text)} 同 wire）
    #[test]
    fn completion_arm_mocks_echo_reply() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let v = rr3_rpc_arm(&mut c, &mut cl, RR3_COMPLETION_TOPIC, &serde_json::json!({"text": "在吗"}))
            .unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）在吗"}));
    }

    /// completion 臂 text 参数 str() coercion（Python str(args.get("text", ""))：
    /// 缺键回落空串、非串折词——mock 回显即观测面）
    #[test]
    fn completion_arm_coerces_text_like_python_str() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let v = rr3_rpc_arm(&mut c, &mut cl, RR3_COMPLETION_TOPIC, &serde_json::json!({})).unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）"}), "缺 text 回落空串");
        let v = rr3_rpc_arm(&mut c, &mut cl, RR3_COMPLETION_TOPIC, &serde_json::json!({"text": 5}))
            .unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）5"}), "str(5)='5' coercion");
    }

    /// completion 臂 real 档缺密钥：业务失败 Err 折 error 应答（Python try/except
    /// RuntimeError 同 wire——文案点名环境变量，不静默降级）
    #[test]
    fn completion_arm_real_without_key_folds_error() {
        let mut c = cfg();
        let _g = env_lock();
        let mut cl = Rr3CloudClients::from_cfg(&c);
        std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "real");
        std::env::remove_var(crate::cloud::RR3_API_KEY_ENV);
        let e = rr3_rpc_arm(&mut c, &mut cl, RR3_COMPLETION_TOPIC, &serde_json::json!({"text": "hi"}))
            .unwrap_err();
        assert_eq!(e, "缺少环境变量 RR3_CLOUD_API_KEY，云端链路不可用");
        std::env::remove_var(crate::cloud::RR3_CLOUD_MODE_ENV);
    }

    /// ops/list 臂经 rr3_rpc_arm 全链路可达（快照组体在 rpc.rs 逐键钉，此处钉臂位）
    #[test]
    fn ops_list_arm_reachable_via_rpc_arm() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let v = rr3_rpc_arm(&mut c, &mut cl, RR3_OPS_LIST_TOPIC, &Value::Null).unwrap();
        assert_eq!(v["ops"]["dialog"]["max_utterance_ms"], 8000);
    }

    /// ops/set 臂写后按组重建：llm 组写 model_id 后重建不红（重建取数在
    /// rpc.rs model_row_of_reflects_live_write 钉真值），mock 档 complete 照走
    #[test]
    fn ops_set_arm_rebuilds_llm_client_after_write() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let v = rr3_rpc_arm(
            &mut c,
            &mut cl,
            RR3_OPS_SET_TOPIC,
            &serde_json::json!({"group": "llm", "slot": "llm_model", "value": "m2"}),
        )
        .unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["new"], "m2");
        // 重建后 completion 臂照常可用（mock 档回显不依赖模型行，行为连续性观测）
        let v = rr3_rpc_arm(&mut c, &mut cl, RR3_COMPLETION_TOPIC, &serde_json::json!({"text": "hi"}))
            .unwrap();
        assert_eq!(v, serde_json::json!({"text": "（mock 回复）hi"}));
    }

    /// ops/set dialog 组写后重建无客户端动作（白名单四组全臂位可达）
    #[test]
    fn ops_set_arm_dialog_group_rebuild_is_noop() {
        let mut c = cfg();
        let (mut cl, _g) = mock_clients(&c);
        let v = rr3_rpc_arm(
            &mut c,
            &mut cl,
            RR3_OPS_SET_TOPIC,
            &serde_json::json!({"group": "dialog", "slot": "max_utterance_ms", "value": 9000}),
        )
        .unwrap();
        assert_eq!(v["new"], 9000);
        assert_eq!(
            c.doc["rr3-brain"]["dialog"]["max_utterance_ms"],
            serde_json::json!(9000),
            "doc 真值已写（dialog 参数消费随迁移件四接线）"
        );
    }
}
