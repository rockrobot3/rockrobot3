//! rr3-connect 机制层注册面（0.0.8 W-A 批2 拆分，ADR-0017 五层表机制层）。
//!
//! [`Rr3ConnectDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）：
//! `on_rpc` = 五臂点名分发（四 handler 在 [`crate::rpc`]＋ops/list 快照臂——
//! 现行五臂照顺序保留，方案快照「四 RPC match」漏记 ops/list 臂，批N27 件一
//! 笔2 落档）；`on_tick` = 链路拍（每拍读一次 HAL——翻转（`Rr3LinkEdge`）立即
//! 发 net/link 不等周期，同态到点才发（周期保底，流是摘要不是逐拍镜像），事件
//! 体带 mode 字段随流发布）；`on_event` = 订阅面为空是**设计**（module.toml
//! 同条注：本模块没有可订阅的命令流），广播只排空。
//!
//! 失败语义与 sense/interact/act/power 同一口径（rr3mod 两档）：on_rpc 返 Err =
//! 业务性失败折 `{"error": …}` 应答（请求-应答语义，不当致命错误；HAL 读数失败
//! 也按业务性折 error——会话/配网本来就是问"现在能不能"）；on_tick 返 Err =
//! 传输性/设备级失败上抛致命（与 power 的电池读数同口径），模块不装"活着"，
//! 死交给 supervisor 按 restart_policy=always 重启。

use std::time::{Duration, Instant};

use rr3mod::{BusHandle, Dispatcher};
use rr_hal::provision_store::Rr3WifiStore;
use rr_hal_api::Rr3Link;
use serde_json::Value;

use crate::rpc;
use crate::session;
use crate::wifi;

/// 本模块 exposes 的 ops 槽快照读口（批N4 件九二，module.toml [rpc].exposes 同源）：
/// 白名单组体应答，槽名与 assemble.toml 槽表申报同源（门 d covers 申报的对账面）
const RR3_OPS_LIST_TOPIC: &str = "rr3-connect/ops/list";

/// 主循环状态整体持有（自原 composite.rs 局部状态迁入，状态归属随分发走）：
/// 配网状态机（on_rpc connect/forget 转移＋on_tick mode 随流）、链路翻转边沿机、
/// 流 seq 与周期保底拍点。store/bt_up 为方案 §4.2 字段级规格的后补字段
/// （批N27 件一笔6：on_rpc connect/forget 运行期需要凭据存储；ops/list 组体
/// 需要 bt 开机初值——"无用状态不迁移"的反面：这两项都有读者）。
pub(crate) struct Rr3ConnectDispatcher {
    dev: Box<dyn Rr3Link>,
    /// "链路读数失败（{backend}）"文案复用（探活编排随 boot.rs，后端名随句柄走）
    backend: &'static str,
    store: Box<dyn Rr3WifiStore>,
    /// link_report_period_ms 是 on_tick 周期保底拍点；min_pass_len 是 connect
    /// 点名参数裁决下限（策略量经 config 数据化注入，机制零策略载荷）
    wifi_cfg: wifi::Rr3LinkCfg,
    /// ops/list 组体用（config [rr3-connect.bt] up 注入值；bt 相位自述留装配段）
    bt_up: bool,
    /// 凭据内存态：on_rpc connect/forget + on_tick mode 随流
    prov: wifi::Rr3Provision,
    edge: wifi::Rr3LinkEdge,
    link_seq: u64,
    next_report: Instant,
}

impl Rr3ConnectDispatcher {
    /// 装配构造（main.rs 装配段调用）：探活句柄与配网初值由 boot.rs 编排产出；
    /// 拍点构造即 `Instant::now()`（首拍由边沿机基线分支保证必发，与原主循环
    /// 语义一致）。
    pub(crate) fn new(
        dev: Box<dyn Rr3Link>,
        backend: &'static str,
        store: Box<dyn Rr3WifiStore>,
        wifi_cfg: wifi::Rr3LinkCfg,
        bt_up: bool,
        prov: wifi::Rr3Provision,
    ) -> Self {
        Self {
            dev,
            backend,
            store,
            wifi_cfg,
            bt_up,
            prov,
            edge: wifi::Rr3LinkEdge::rr3_link_edge_new(),
            link_seq: 0,
            next_report: Instant::now(),
        }
    }
}

impl Dispatcher for Rr3ConnectDispatcher {
    /// rr3.call 点名应答：五臂 match 照现行顺序（session→scan→connect→forget→
    /// ops/list→兜底点名）。业务性失败 Err 折 error 应答（serve 骨架承接），与
    /// 现行 `unwrap_or_else` wire 形状逐字等价；兜底文案「只 exposes …」在各自
    /// Dispatcher 里是**装配事实**不是机制重复（方案 §3 零改动观测面清单）。
    fn on_rpc(&mut self, topic: &str, args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        match topic {
            session::RR3_SESSION_TOPIC => {
                rpc::rr3_handle_session(self.dev.as_ref(), self.prov.rr3_provision_mode(), args)
            }
            wifi::RR3_WIFI_SCAN_TOPIC => rpc::rr3_handle_wifi_scan(args),
            wifi::RR3_WIFI_CONNECT_TOPIC => {
                rpc::rr3_handle_wifi_connect(
                    self.dev.as_ref(),
                    self.store.as_ref(),
                    &mut self.prov,
                    args,
                    self.wifi_cfg.min_pass_len,
                )
            }
            wifi::RR3_WIFI_FORGET_TOPIC => {
                rpc::rr3_handle_wifi_forget(self.store.as_ref(), &mut self.prov, args)
            }
            RR3_OPS_LIST_TOPIC => {
                Ok(rpc::rr3_connect_ops_snapshot(&self.wifi_cfg, self.bt_up))
            }
            other => Err(format!(
                "rr3-connect 只 exposes {} 与 wifi/scan|connect|forget|ops/list，收到 {other}",
                session::RR3_SESSION_TOPIC
            )),
        }
    }

    /// 订阅面为空是设计（module.toml 同条注）：本模块没有可订阅的命令流——
    /// 广播（Event/Notification/Stream）只排空（方案 §4.2 映射表「并入骨架」行，
    /// 注释语义迁文档）。
    fn on_event(&mut self, _topic: &str, _body: &Value, _bus: &BusHandle) -> Result<(), String> {
        Ok(())
    }

    /// 链路拍（自原 composite.rs 主循环体逐字迁入）：每拍读一次 HAL（sim 廉价；
    /// 实机 nmcli 子进程有 timeout_ms 守护，0.0.5 批I N-2——读数失败 = 设备/
    /// 传输级，Err 上抛致命）。翻转（`Rr3LinkEdge`）立即发不等周期；同态到点
    /// 才发（周期保底）。`now` 由 serve 每圈传入（帧无关节拍，与帧到达无关）。
    fn on_tick(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        let st = self
            .dev
            .state()
            .map_err(|e| format!("链路读数失败（{}）: {e}", self.backend))?;
        if self.edge.rr3_link_edge_cross(st.up) || now >= self.next_report {
            self.link_seq += 1;
            let mode = self.prov.rr3_provision_mode();
            bus.publish(
                wifi::RR3_LINK_TOPIC,
                rpc::rr3_link_body(self.link_seq, &st, mode),
            )?;
            self.next_report = now + Duration::from_millis(self.wifi_cfg.link_report_period_ms);
        }
        Ok(())
    }
}
