// SPDX-License-Identifier: Apache-2.0
//! # rr3mod —— 模块公共骨架（ADR-0017 裁定 1）：只编排，不重写。
//!
//! 五模块与 brain 迁移件共用同构启动序列：[`bootstrap`]（manifest → 总线 → config →
//! 解析）+ [`serve`]（死亡探针 → 分帧 → RPC/订阅分发 → 节拍）+ [`Dispatcher`]。
//! 语义真值全在 rr-bus-api / rr-core / rr-ipc，本 crate 不立第二份实现。
//!
//! 定形要点：`on_rpc` 返 `Err` = 业务失败折 `{"error": …}`；`on_event`/`on_tick` 返
//! `Err` = 传输/设备级失败上抛致命（main `exit(1)` 交 supervisor）。HAL 选择与探活
//! 归各模块 boot.rs，`Unsupported` 即 `Err`；SURFACE 启动自述留 main。
//! 刻意不提供 subscribe：线上无订阅注册帧（全量扇出 + 本地过滤）。本 crate API 修订
//! 须五模块 + brain 迁移件同批改，不许骨架单方漂移。
//!
//! `#[cfg(unix)]` 门与 rr-bus-api 同形状；非 unix 目标只余 [`RR3_MANIFEST_ENV`]。

/// rrd spawn 注入的 manifest 绝对路径（supervisor 契约；模块侧本地常量随批退役）。
pub const RR3_MANIFEST_ENV: &str = "RR3_MANIFEST";

#[cfg(unix)]
mod boot;
#[cfg(unix)]
mod dispatch;
#[cfg(unix)]
mod serve;

#[cfg(all(test, unix))]
pub(crate) mod test_support;

#[cfg(unix)]
pub use boot::{bootstrap, load_config_text, rr3_config_path};

#[cfg(unix)]
pub use dispatch::Dispatcher;

#[cfg(unix)]
pub use serve::serve;

/// 装配参数（模块装配段 main.rs 构造；0.0.8 方案 §2.4）。C = 模块 config 形状
/// （rr3-interact 无 config 解析面，用 `()` 空形状）。
#[cfg(unix)]
pub struct Rr3ModConfig<C> {
    /// config 覆盖环境变量名（现四模块各持一份：rr3-act `RR3_ACT_CONFIG_ENV` /
    /// rr3-connect `RR3_CONNECT_CONFIG_ENV` / rr3-power `RR3_POWER_CONFIG_ENV` /
    /// rr3-sense `RR3_SENSE_CONFIG_ENV`——随批1–批5 拆分退役）。None = 只认
    /// manifest 同目录 config.toml。
    pub config_env: Option<&'static str>,
    /// config 文本 → 模块配置形状。TOML schema 解析与缺字段点名文案**留模块侧**
    /// （各模块文案形状是装配观测面，不统一）；C = () 时传 `|_| Ok(())`。
    pub parse_config: fn(&str) -> Result<C, String>,
}

/// 总线薄封装：透传 Rr3BusClient，不加语义。Clone 语义与 rr-bus 一致
/// （同一连接的 Arc 共享句柄——rr3-interact 面板线程现用法）。`client` 字段
/// `pub(crate)`：serve 骨架的分帧/应答直用承载客户端（`rr3_poll_wait` /
/// `rr3_frame_body` / `rr3_reply_call` 是 serve 的机制面，不进 BusHandle 公开
/// API——方案 §2.2 刻意收窄的同一条纪律）。
#[cfg(unix)]
#[derive(Clone)]
pub struct BusHandle {
    pub(crate) client: rr_bus_api::Rr3BusClient,
}

#[cfg(unix)]
impl BusHandle {
    /// 显式包装构造（批N29 批4）：把既有 [`Rr3BusClient`] 包进句柄——面板线程
    /// 宿主（rr3-interact `panel::spawn_host`）与单测夹具（假 rrd 短连）持有的
    /// 是自建的 client，bootstrap 拿不到它们。**收窄纪律针对承载方法面**
    /// （rr3_poll_wait / rr3_reply_call 等不进公开 API），构造口只让已持有
    /// client 的人把既有连接包进句柄，不开任何承载方法洞。
    pub fn wrap(client: rr_bus_api::Rr3BusClient) -> Self {
        Self { client }
    }

    /// 业务发布（Event 帧）。透传 [`Rr3BusClient::rr3_publish`]。
    pub fn publish(&self, topic: &str, body: serde_json::Value) -> Result<(), String> {
        self.client.rr3_publish(topic, body)
    }

    /// 模块间请求（`rr3.call`）。透传 [`Rr3BusClient::rr3_call_topic`]；
    /// `timeout` 显式实参（批N25 件一裁定 2：超时是调用方策略，骨架零缺省值——
    /// 方案 §2.2 签名缺实参属规格疏漏，随批订正登记）。
    pub fn call(
        &self,
        topic: &str,
        args: &serde_json::Value,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, String> {
        self.client.rr3_call_topic(topic, args.clone(), timeout)
    }

    /// 运维计数自报。透传 [`Rr3BusClient::rr3_counter_bump`]。
    pub fn counter_bump(&self, key: &str, by: u64) {
        self.client.rr3_counter_bump(key, by)
    }

    /// 总线还活着吗（死亡探针）。透传 [`Rr3BusClient::rr3_connected`]。
    pub fn connected(&self) -> bool {
        self.client.rr3_connected()
    }
}
