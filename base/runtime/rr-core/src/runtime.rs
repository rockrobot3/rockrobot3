//! 模块运行时执行环的机制核切片（0.0.5 第 II 轮批D₂，ADR-0018 裁定 1/3）。
//!
//! 执行环 = **连接 → 注册 → 心跳 → 分发 → 节拍**的决策逻辑；本模块只装
//! 平台无关的那一半——状态转移与拍点判定，全部纯函数、时钟由调用方注入
//! （裁定 5：不读墙上时钟、不睡眠、不碰 io）。四类平台供给（裁定 3）不在
//! 这里：单调时基、事件循环、transport 实现由平台（std 驱动器 = `rr-bus`
//! 的 client；裸机 = 运行环）注入。
//!
//! - [`Rr3ModuleRuntime`]：连接态转移（[`Rr3LinkState`]）+ 心跳拍点调度。
//!   `net.online` 格（contracts/slots.toml base 层）的机制核切片落位于
//!   [`Rr3LinkState::Online`] 的判定（[`Rr3ModuleRuntime::rr3_link_online`]）；
//! - [`rr3_dispatch_class`]：收帧分发的分类规则——应答帧按 JSON-RPC id 路由
//!   到等待者，其余（Event / Notification / 点名 Request / Stream）进 inbox
//!   由模块轮询。std 侧真值在 `rr-bus` client 的 reader 循环（消费本分类）。
//!
//! 节拍语义钉死为**相对节拍**（每拍 = 上一拍发出时刻 + heartbeat_ms，drift
//! 而非 catch-up）——与 std 心跳线程既有行为逐拍一致；改追拍语义须先改
//! std 驱动器并复验 e2e，两侧不得漂移。

use rr_ipc::Rr3PayloadType;

/// 执行单元视角的基座连接态（`net.online` 机制核切片的取值域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3LinkState {
    /// 已连上传输、注册握手进行中（注册前没有心跳、没有 task）
    Registering,
    /// 注册成功：rrd 已分配 task，心跳节拍运行中
    Online { task: u64 },
    /// 断开（传输错误 / 注册被拒 / 心跳失败）——执行单元"不装活着"，
    /// 交给监督者裁定（进程面 = supervisor restart_policy；裸机面 = 运行环复位）
    Down,
}

/// 帧分发分类（[`rr3_dispatch_class`] 的判定结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3Dispatch {
    /// 应答帧：按 JSON-RPC id 路由到 pending 表的等待者
    Pending,
    /// 非应答帧：进 inbox，由模块主循环轮询分派
    Inbox,
}

/// 模块运行时状态机：连接态 + 心跳拍点。时钟全部入参注入。
///
/// 构造时处于 [`Rr3LinkState::Registering`]；心跳间隔自动保底 ≥1ms
/// （与 std 驱动器既有 `hb_ms.max(1)` 同一条规则——0 间隔在这里被拦下，
/// 机制核不自毁）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3ModuleRuntime {
    heartbeat_ms: u64,
    link: Rr3LinkState,
    /// 下一拍到期时刻（ms，注入时钟同一时基）；不在 Online 态时无意义
    next_beat_at_ms: u64,
}

impl Rr3ModuleRuntime {
    /// 新建：Registering，无拍点。
    pub fn rr3_runtime_new(heartbeat_ms: u64) -> Self {
        Self {
            heartbeat_ms: heartbeat_ms.max(1),
            link: Rr3LinkState::Registering,
            next_beat_at_ms: 0,
        }
    }

    /// 当前连接态。
    pub fn rr3_link_state(&self) -> Rr3LinkState {
        self.link
    }

    /// `net.online` 机制核切片：注册成功且未断开 = 基座连接态在位。
    pub fn rr3_link_online(&self) -> bool {
        matches!(self.link, Rr3LinkState::Online { .. })
    }

    /// 注册成功转移：记下 rrd 分配的 task，排出**首拍** = `now_ms + heartbeat_ms`
    /// （std 驱动器现状：注册完成后心跳线程先睡满一个间隔再发第一拍）。
    pub fn rr3_mark_registered(&mut self, task: u64, now_ms: u64) {
        self.link = Rr3LinkState::Online { task };
        self.next_beat_at_ms = now_ms.saturating_add(self.heartbeat_ms);
    }

    /// 断开转移（幂等）：清拍点，Online 语义终结。
    pub fn rr3_mark_down(&mut self) {
        self.link = Rr3LinkState::Down;
        self.next_beat_at_ms = 0;
    }

    /// 心跳到期判定：仅 Online 态有节拍；`now_ms` 追上拍点即到期。
    pub fn rr3_heartbeat_due(&self, now_ms: u64) -> bool {
        matches!(self.link, Rr3LinkState::Online { .. }) && now_ms >= self.next_beat_at_ms
    }

    /// 一拍发出后的记账：**相对节拍**——下一拍 = 本拍发出时刻 + heartbeat_ms。
    /// 非 Online 态调用是无操作（状态机不允许在断开后还在记账）。
    pub fn rr3_heartbeat_sent(&mut self, now_ms: u64) {
        if matches!(self.link, Rr3LinkState::Online { .. }) {
            self.next_beat_at_ms = now_ms.saturating_add(self.heartbeat_ms);
        }
    }

    /// 距下一拍的等待时长（ms）：仅 Online 态给出（饱和下界 0）；
    /// 非 Online 返回 `None`——运行环据此停拍（std 心跳线程据此退出）。
    pub fn rr3_next_beat_wait_ms(&self, now_ms: u64) -> Option<u64> {
        if !matches!(self.link, Rr3LinkState::Online { .. }) {
            return None;
        }
        Some(self.next_beat_at_ms.saturating_sub(now_ms))
    }

    /// 心跳间隔（保底后的真值，供驱动器核对/测试）。
    pub fn rr3_heartbeat_ms(&self) -> u64 {
        self.heartbeat_ms
    }
}

/// 收帧分发分类：Response 路由等待者，其余进 inbox。
/// 与 std 驱动器 reader 循环既有判定逐条对齐（功能守恒面）。
pub fn rr3_dispatch_class(ty: Rr3PayloadType) -> Rr3Dispatch {
    match ty {
        Rr3PayloadType::Response => Rr3Dispatch::Pending,
        Rr3PayloadType::Request
        | Rr3PayloadType::Event
        | Rr3PayloadType::Stream
        | Rr3PayloadType::Notification => Rr3Dispatch::Inbox,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// 构造即 Registering：无心跳、无拍点、net.online 为假
    #[test]
    fn new_starts_registering_with_no_heartbeat() {
        let rt = Rr3ModuleRuntime::rr3_runtime_new(50);
        assert_eq!(rt.rr3_link_state(), Rr3LinkState::Registering);
        assert!(!rt.rr3_link_online());
        assert!(!rt.rr3_heartbeat_due(1_000));
        assert_eq!(rt.rr3_next_beat_wait_ms(1_000), None);
        assert_eq!(rt.rr3_heartbeat_ms(), 50);
    }

    /// 心跳间隔保底 ≥1ms（std 驱动器 `hb_ms.max(1)` 同一条规则进机制核）
    #[test]
    fn heartbeat_interval_floors_at_one_ms() {
        assert_eq!(Rr3ModuleRuntime::rr3_runtime_new(0).rr3_heartbeat_ms(), 1);
        assert_eq!(Rr3ModuleRuntime::rr3_runtime_new(50).rr3_heartbeat_ms(), 50);
    }

    /// 注册转移：Online + 首拍 = 注册时刻 + 间隔（拍点序列与 std 心跳线程逐拍一致）
    #[test]
    fn registered_schedules_first_beat_one_interval_out() {
        let mut rt = Rr3ModuleRuntime::rr3_runtime_new(50);
        rt.rr3_mark_registered(7, 1_000);
        assert_eq!(rt.rr3_link_state(), Rr3LinkState::Online { task: 7 });
        assert!(rt.rr3_link_online());
        assert!(!rt.rr3_heartbeat_due(1_049), "到期前一毫秒不算到期");
        assert!(rt.rr3_heartbeat_due(1_050), "拍点整点到期");
        assert_eq!(rt.rr3_next_beat_wait_ms(1_000), Some(50));
        assert_eq!(rt.rr3_next_beat_wait_ms(1_030), Some(20));
        assert_eq!(rt.rr3_next_beat_wait_ms(1_050), Some(0));
    }

    /// 相对节拍：下一拍 = 本拍发出时刻 + 间隔（drift 语义，与 std 行为一致；
    /// 迟到的一拍不追拍）
    #[test]
    fn heartbeat_sent_advances_relative_beat() {
        let mut rt = Rr3ModuleRuntime::rr3_runtime_new(50);
        rt.rr3_mark_registered(7, 1_000);
        rt.rr3_heartbeat_sent(1_060); // 迟到 10ms 才发出
        assert!(!rt.rr3_heartbeat_due(1_060));
        assert!(rt.rr3_heartbeat_due(1_110));
        rt.rr3_heartbeat_sent(1_110);
        assert!(rt.rr3_heartbeat_due(1_160));
    }

    /// 断开转移（幂等）：Down 后无节拍、net.online 翻假、sent 是无操作
    #[test]
    fn mark_down_is_terminal_and_idempotent() {
        let mut rt = Rr3ModuleRuntime::rr3_runtime_new(50);
        rt.rr3_mark_registered(7, 1_000);
        rt.rr3_mark_down();
        rt.rr3_mark_down();
        assert_eq!(rt.rr3_link_state(), Rr3LinkState::Down);
        assert!(!rt.rr3_link_online());
        assert!(!rt.rr3_heartbeat_due(9_999));
        assert_eq!(rt.rr3_next_beat_wait_ms(9_999), None);
        rt.rr3_heartbeat_sent(9_999); // 无操作：断开后不得再排拍
        assert_eq!(rt.rr3_next_beat_wait_ms(9_999), None);
    }

    /// 逐拍一致性：注入时钟推进，拍点序列 = 注册点 + k·间隔（std 心跳线程的
    /// sleep 间隔序列的机制核镜像）
    #[test]
    fn beat_train_is_periodic_under_injected_clock() {
        let mut rt = Rr3ModuleRuntime::rr3_runtime_new(20);
        rt.rr3_mark_registered(7, 0);
        let mut fired = Vec::new();
        let mut now = 0u64;
        while now < 100 {
            if rt.rr3_heartbeat_due(now) {
                fired.push(now);
                rt.rr3_heartbeat_sent(now);
            }
            now += 1;
        }
        assert_eq!(fired, vec![20, 40, 60, 80], "拍点必须严格等距");
    }

    /// 分发分类表：Response → 等待者路由；其余四类 → inbox
    #[test]
    fn dispatch_class_routes_only_response_to_pending() {
        assert_eq!(
            rr3_dispatch_class(Rr3PayloadType::Response),
            Rr3Dispatch::Pending
        );
        assert_eq!(
            rr3_dispatch_class(Rr3PayloadType::Request),
            Rr3Dispatch::Inbox
        );
        assert_eq!(
            rr3_dispatch_class(Rr3PayloadType::Event),
            Rr3Dispatch::Inbox
        );
        assert_eq!(
            rr3_dispatch_class(Rr3PayloadType::Stream),
            Rr3Dispatch::Inbox
        );
        assert_eq!(
            rr3_dispatch_class(Rr3PayloadType::Notification),
            Rr3Dispatch::Inbox
        );
    }
}
