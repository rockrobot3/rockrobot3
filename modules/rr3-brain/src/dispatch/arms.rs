//! 六臂应答（rr3.call）与 TTS 分帧。

use super::*;

impl Dispatcher for Rr3BrainDispatcher {
    /// rr3.call 点名应答（事务面六臂，module.toml exposes 同源）。薄委托
    /// [`rr3_rpc_arm`]——六臂是内存面不触总线（bus 参数留给 trait 形状），
    /// 纯函数即测试真值入口（仓外侧无法凭空构造 BusHandle）。
    fn on_rpc(&mut self, topic: &str, args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        rr3_rpc_arm(
            &mut self.cfg,
            &mut self.clients,
            &mut self.dialog,
            topic,
            args,
        )
    }

    /// 订阅面两臂（pcm 积累＋vad_start 判停触发整轮；module.toml subscribes
    /// 两条是申报面，事件过滤在臂内本地做）。
    fn on_event(&mut self, topic: &str, body: &Value, bus: &BusHandle) -> Result<(), String> {
        self.rr3_event_arm(topic, body, bus)
    }

    /// 节拍面一臂（utterance_expired 时长判停触发整轮）。
    fn on_tick(&mut self, _now: Instant, bus: &BusHandle) -> Result<(), String> {
        self.rr3_tick_arm(bus)
    }
}

/// 事务面分派表的臂签名：`(cfg, clients, dialog, args)` 入、应答体出（六臂是
/// 内存面不触总线，bus 参数留给 trait 形状）。
type Rr3RpcArm =
    fn(&mut Rr3BrainCfg, &mut Rr3CloudClients, &mut Rr3Dialog, &Value) -> Result<Value, String>;

/// 事务面六臂（自 Python composite.py 主循环 RPC 分支逐语义迁入）：completion
/// （[`Rr3LlmClient`]）＋ops/list 快照＋ops/set 写臂（写 doc → 按组重建受影响
/// 客户端——dialog 组就地改参）＋模型行三臂；未知 topic 返 Err 由 serve 折
/// `{"error": …}` 应答（与 Python 主循环兜底分支同 wire）。
pub(crate) fn rr3_rpc_arm(
    cfg: &mut Rr3BrainCfg,
    clients: &mut Rr3CloudClients,
    dialog: &mut Rr3Dialog,
    topic: &str,
    args: &Value,
) -> Result<Value, String> {
    // 表项次序即 exposes 申报次序（兜底文案仍按 RR3_RPC_EXPOSES 列举）
    let arms: [(&str, Rr3RpcArm); 6] = [
        // completion 臂：业务失败（缺密钥/HTTP 4xx-5xx/应答形状不符）Err 折 error
        // 应答——Python try/except RuntimeError 同 wire（传输异常已在 cloud 层折
        // 同型 Err，同一次云不可用结局只剩一种：弃轮/错误应答，进程不死）
        (RR3_COMPLETION_TOPIC, |_, clients, _, args| {
            // Python: text = str(params.get("args", {}).get("text", ""))
            let text = args.get("text").map(rr3_py_str).unwrap_or_default();
            Ok(json!({ "text": clients.llm.rr3_complete(&text)? }))
        }),
        (RR3_OPS_LIST_TOPIC, |cfg, _, _, _| {
            Ok(json!({ "ops": rr3_ops_snapshot(cfg)? }))
        }),
        (RR3_OPS_SET_TOPIC, |cfg, clients, dialog, args| {
            let reply = rr3_ops_set_arm(cfg, args)?;
            // Python: group = str(args.get("group", ""))（同 ops/set 臂的 coercion）；
            // new 是写臂类型域校验产物（dialog 组消费其整数值）
            let group = args.get("group").map(rr3_py_str).unwrap_or_default();
            let new = reply.get("new").cloned().unwrap_or(Value::Null);
            rr3_rebuild_clients(cfg, &group, &new, dialog, clients)?;
            Ok(reply)
        }),
        // 模型行三臂（ADR-0023）：列表只读；set 落盘后按 llm 组重建客户端
        // （写面已自守结构门，重建取数不可达失败面如实上抛）；test 只探测一次，
        // 业务失败用 `ok:false` 表达（面板要区分"接口通"与"RPC 出错"）
        (RR3_MODEL_LIST_TOPIC, |cfg, _, _, _| rr3_model_list(cfg)),
        (RR3_MODEL_SET_TOPIC, |cfg, clients, dialog, args| {
            let reply = rr3_model_set_arm(cfg, args)?;
            rr3_rebuild_clients(cfg, "llm", &Value::Null, dialog, clients)?;
            Ok(reply)
        }),
        (RR3_MODEL_TEST_TOPIC, |cfg, _, _, args| {
            rr3_model_test_arm(cfg, args)
        }),
    ];
    let Some((_, arm)) = arms.iter().find(|(t, _)| *t == topic) else {
        let list: Vec<String> = RR3_RPC_EXPOSES.iter().map(|t| format!("'{t}'")).collect();
        return Err(format!(
            "rr3-brain 只 exposes [{}]，收到 {topic}",
            list.join(", ")
        ));
    };
    arm(cfg, clients, dialog, args)
}

/// 把一轮 TTS 的 PCM 分帧为 `rr3-brain/audio/tts_pcm` 的载荷体列表（纯函数，自
/// composite.py `rr3_tts_pcm_frames` 逐语义迁入）。
///
/// 逐字段复用上行 `rr3-sense/audio/pcm` 已定的形状（`rr3_pcm_event_body` 为形状
/// 事实源）：`{seq, format:"s16le", rate, channels, ms, pcm_b64}`，20ms 一块
/// （640 字节）。`seq` 从入参水位起逐帧 +1（**模块级单调，不随轮次复位**）；`ms`
/// 由块内样点数导出（尾块不足 20ms 时按实算）。`topic` 不含（由发布并入）。返回
/// `(frames, 新 seq 水位)`——总线发布与跨语言一致性用例共用同一份组装。
pub(crate) fn rr3_tts_pcm_frames(pcm: &[u8], mut seq: u64) -> (Vec<Value>, u64) {
    let mut frames = Vec::new();
    for chunk in pcm.chunks(RR3_TTS_PCM_FRAME_BYTES) {
        seq += 1;
        let samples = chunk.len() as u64 / 2;
        frames.push(json!({
            "seq": seq,
            "format": "s16le",
            "rate": RR3_TTS_SAMPLE_RATE,
            "channels": 1,
            "ms": samples * 1000 / RR3_TTS_SAMPLE_RATE as u64,
            "pcm_b64": rr3_base64_encode(chunk),
        }));
    }
    (frames, seq)
}
