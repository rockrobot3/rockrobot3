//! # rr3mod —— 模块公共骨架（0.0.8 W-A 批0，ADR-0017 裁定 1）
//!
//! 五模块（批1–批5 收敛）与 brain 迁移件（ADR-0016）共用的**同构启动序列编排**：
//! [`bootstrap`]（读 manifest → 连总线 → 读 config → 解析）+ [`serve`]（死亡探针 →
//! 分帧 → RPC/订阅分发 → 节拍）+ [`Dispatcher`]（模块机制层注册面）。只编排不重写
//! ——全部语义真值在 rr-bus-api / rr-core / rr-ipc，本 crate 不给任何既有语义立
//! 第二份实现。
//!
//! # 定形要点
//!
//! - **Dispatcher 错误语义两档**（方案 §2.3）：[`Dispatcher::on_rpc`] 返 `Err` =
//!   业务性失败，serve 折 `{"error": …}` 应答回话；`on_event` / `on_tick` 返 `Err` =
//!   传输性/设备级失败，上抛致命（main `exit(1)` 交 supervisor）——与现行五模块
//!   逐字同款，本 crate 不发明新语义。
//! - **HAL 选择 + 探活不进 bootstrap**（方案 §2.4）：后端选择函数各模块不同，探活
//!   句柄强耦合模块语义——归各模块 boot.rs（接入层），`Unsupported` 即 `Err`。
//! - **SURFACE 启动自述不进 bootstrap**（ADR-0017 裁定 3）：自述留 main（装配面）。
//! - **刻意不提供 subscribe**（方案 §2.2）：线上无订阅注册帧（全量扇出 + 本地过滤），
//!   给"注册"形状即假 API；订阅过滤是 `on_event` 的本地职责。
//! - **六消费方同批改**（方案 §5 批0 附加纪律）：本 crate API 此后修订须五模块 +
//!   brain 迁移件同批改，不许骨架单方漂移。
//!
//! `#[cfg(unix)]` 门随 rr-bus-api 同形状（承载是平台事，UDS 承载只对 unix 目标
//! 在册；非 unix 目标本 crate 只余 [`RR3_MANIFEST_ENV`] 常量）。

/// rrd spawn 注入的 manifest 绝对路径（supervisor.rs 契约环境变量）。
/// 现五模块各自本地定义（rr3-act / rr3-sense / rr3-connect / rr3-power /
/// rr3-interact 的 main.rs 各一份）——收编单点（0.0.8 方案 §2.2），模块侧本地
/// 常量随批1–批5 拆分退役。
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
