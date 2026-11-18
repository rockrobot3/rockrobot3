//! dispatch 单测入口：共享夹具（配置/客户端/记录型总线）与三组用例。

mod arms;
mod frames;
mod turn;

use super::*;
use crate::boot::rr3_brain_cfg_parse;
use rr_ipc::rr3_base64_decode;

/// env 动作的测试互斥（全 crate 一柄，与 cloud.rs tests 共享——RR3_BRAIN_CLOUD
/// 等键是进程级全局，并行测试互相污染）
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::cloud::RR3_TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 与仓内 config.toml 同形状的最小配置夹具
fn cfg() -> Rr3BrainCfg {
    rr3_brain_cfg_parse(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n",
        )
        .unwrap()
}

/// 带点名超时的配置夹具（B-13① 超时注入判据用）
fn cfg_call_timeout(s: f64) -> Rr3BrainCfg {
    rr3_brain_cfg_parse(&format!(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n[rr3-brain.bus]\ncall_timeout_s = {s}\n"
        ))
        .unwrap()
}

/// 唤醒词开启的配置夹具（2026-10-31；跟进窗可调，0 = 关窗）
fn cfg_wake(followup_ms: u64) -> Rr3BrainCfg {
    rr3_brain_cfg_parse(&format!(
            "[rr3-brain.cloud]\nbase_url = \"u\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n[rr3-brain.dialog]\n[rr3-brain.wake]\nenable = true\nphrases = [\"你好洛克\"]\nack = \"我在呢，有什么可以帮您？\"\nfollowup_ms = {followup_ms}\n"
        ))
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

/// mock 档整装 Dispatcher 夹具（编排面测试用：三客户端 mock 档＋config 话轮
/// 封顶入状态机）。锁柄随测试全程持有（env 纪律同 mock_clients）。
fn mock_dispatcher(cfg: Rr3BrainCfg) -> (Rr3BrainDispatcher, std::sync::MutexGuard<'static, ()>) {
    let g = env_lock();
    std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "mock");
    (Rr3BrainDispatcher::new(cfg), g)
}

/// rr3-sense/audio/pcm 事件体夹具（rr3_pcm_event_body 同形状）
fn pcm_event_body(payload: &[u8], ms: i64) -> Value {
    json!({
        "seq": 1,
        "format": "s16le",
        "rate": 16000,
        "channels": 1,
        "ms": ms,
        "pcm_b64": rr3_base64_encode(payload),
    })
}

/// 记录型整轮编排夹具（Python test_composite._FakeBus 同体例）：计数/发布/
/// 点名三动作记账，点名应答/报错可编程——拆账判据的观测面。账本 RefCell
/// 内可变（Rr3TurnBus 取 &self——生产 BusHandle 内部带锁同形状）。
struct FakeTurnBus {
    bumps: std::cell::RefCell<Vec<(String, u64)>>,
    published: std::cell::RefCell<Vec<(String, Value)>>,
    calls: std::cell::RefCell<Vec<(String, Value, Duration)>>,
    reply: Value,
    call_error: Option<String>,
}

impl FakeTurnBus {
    fn reply(reply: Value) -> Self {
        Self {
            bumps: Default::default(),
            published: Default::default(),
            calls: Default::default(),
            reply,
            call_error: None,
        }
    }

    fn failing_call(error: &str) -> Self {
        Self {
            bumps: Default::default(),
            published: Default::default(),
            calls: Default::default(),
            reply: Value::Null,
            call_error: Some(error.to_string()),
        }
    }

    fn bumps_of(&self, key: &str) -> u64 {
        self.bumps
            .borrow()
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, by)| by)
            .sum()
    }

    fn published_of(&self, topic: &str) -> Vec<Value> {
        self.published
            .borrow()
            .iter()
            .filter(|(t, _)| t == topic)
            .map(|(_, b)| b.clone())
            .collect()
    }
}

impl Rr3TurnBus for FakeTurnBus {
    fn counter_bump(&self, key: &str, by: u64) {
        self.bumps.borrow_mut().push((key.to_string(), by));
    }

    fn publish(&self, topic: &str, body: Value) -> Result<(), String> {
        self.published.borrow_mut().push((topic.to_string(), body));
        Ok(())
    }

    fn call(&self, topic: &str, args: &Value, timeout: Duration) -> Result<Value, String> {
        self.calls
            .borrow_mut()
            .push((topic.to_string(), args.clone(), timeout));
        match &self.call_error {
            Some(e) => Err(e.clone()),
            None => Ok(self.reply.clone()),
        }
    }
}

/// intent 发布体取用（整轮走通的观测面；恰一次发布）
fn dp_published_intent(bus: &FakeTurnBus) -> Value {
    let intents = bus.published_of(RR3_TOPIC_INTENT);
    assert_eq!(intents.len(), 1, "整轮恰发布一次 intent");
    intents.into_iter().next().unwrap()
}

/// 多轮用例取**最后一条** intent（前轮已发布过，不能用恰一次断言）
fn dp_last_intent(bus: &FakeTurnBus) -> Value {
    bus.published_of(RR3_TOPIC_INTENT)
        .last()
        .cloned()
        .expect("至少一轮 intent")
}

/// 确定性伪 PCM（s16le，字节值随下标变化，便于逐块断言原样切分）
fn pcm_bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}
