// SPDX-License-Identifier: Apache-2.0
//! 模块运行时执行环的机制核切片（ADR-0018 裁定 1/3）。
//!
//! 执行环 = **连接 → 注册 → 心跳 → 分发 → 节拍**的决策逻辑；本模块只装
//! 平台无关的那一半——状态转移与拍点判定，全部纯函数、时钟由调用方注入
//! （裁定 5：不读墙上时钟、不睡眠、不碰 io）。四类平台供给（裁定 3）不在
//! 这里：单调时基、事件循环、transport 实现由平台（std 驱动器 = `rr-bus`
//! 的 client；裸机 = 运行环）注入。
//!
//! - [`Rr3ModuleRuntime`]：连接态转移（[`Rr3LinkState`]）+ 心跳拍点调度；
//!   `net.online` 格判定落位在 [`Rr3ModuleRuntime::rr3_link_online`]；
//! - [`rr3_dispatch_class`]：应答帧按 JSON-RPC id 路由到等待者，其余（Event /
//!   Notification / 点名 Request / Stream）进 inbox 由模块轮询；
//! - [`Rr3GuardBuf`] / [`Rr3GuardEvent`]：自足档**本地守护输出行**（UART/RTT）
//!   起动态 + 喂拍 + 安全核触发/拒动原因三类行型（行文法注释即契约）。
//!
//! 节拍语义钉死为**相对节拍**（每拍 = 上一拍发出时刻 + heartbeat_ms，drift
//! 而非 catch-up）——与 std 心跳线程既有行为逐拍一致，两侧不得漂移。

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
    pub fn rr3_beat_wait_ms(&self, now_ms: u64) -> Option<u64> {
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

// ---------------------------------------------------------------------------
// 自足档本地守护输出行（0.0.5 批K H6，C15"本地守护输出的格式定形"）
// ---------------------------------------------------------------------------

/// 守护行容量上界：最长行 `RR3G boot online task=`(22) + u64 十进制最长 20 位 = 42，
/// 取 48 定容——定长栈缓冲，不假设全局分配器（裁定 3：内存策略是平台供给）。
pub const RR3_GUARD_LINE_CAP: usize = 48;

/// 安全核触发/拒动的原因码（封闭枚举，零策略载荷——码表只含机制语义，
/// 不含板名/厂商/业务阈值；新增原因码须改这里并同步对拍用例）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3GuardReason {
    /// 限位钳位生效（[`crate::control::rr3_travel_clamp`] 把出行角钳回行程内）
    TravelClamp,
    /// 低电边沿触发（[`crate::control::Rr3LowEdge`]，battery/low 兜底）
    LowEdge,
    /// 反射环饱和通告（[`crate::control::Rr3SaturationDetector`]）
    Saturation,
    /// 限速流拒发（[`crate::control::Rr3RateGate`] 拒动）
    RateDenied,
}

impl Rr3GuardReason {
    /// 行内原因码（小写连字符，ASCII）。
    pub fn rr3_guard_code(self) -> &'static str {
        match self {
            Rr3GuardReason::TravelClamp => "travel-clamp",
            Rr3GuardReason::LowEdge => "low-edge",
            Rr3GuardReason::Saturation => "saturation",
            Rr3GuardReason::RateDenied => "rate-denied",
        }
    }
}

/// 本地守护输出事件（自足档 UART/RTT 行的三类行型，0.0.5 批K H6 定形）。
///
/// 三类行型与文法（唯一真源 = 本枚举 + [`Rr3GuardBuf::rr3_guard_format`]）：
/// - 起动态：`RR3G boot <registering | online task=<dec> | down>`——照抄
///   [`Rr3LinkState`] 状态机，退化用法（§12.4 预置 Online）同走此行；
/// - 喂拍：`RR3G feed seq=<dec>`——watchdog 喂狗第 n 拍（序号自 1 起，由
///   运行环记账；喂拍**产生策略**随 port，行格式是机制）；
/// - 安全核：`RR3G safe <trip | deny> <原因码>`——`trip` = 安全核动作已生效
///   （钳位/边沿/饱和通告），`deny` = 拒绝执行（如限速流拒发、低电闭锁拒动）。
///
/// 行内容全 ASCII、空格分字段、**不带时间戳**——机制核不读墙上时钟（裁定 5），
/// 时戳前缀是发落面（UART/RTT 写出方）的事。事件产生策略随 port，行格式唯一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3GuardEvent {
    /// 起动态：连接态转移
    Boot(Rr3LinkState),
    /// 喂拍：第 `seq` 次喂狗
    Feed { seq: u64 },
    /// 安全核触发（`trip = true`）或拒动（`trip = false`）+ 原因码
    Safety {
        /// true = 动作已生效（trip），false = 拒绝执行（deny）
        trip: bool,
        /// 原因码（封闭枚举）
        reason: Rr3GuardReason,
    },
}

/// 守护行定容缓冲（栈上 48 字节，覆盖式写入）——自足档没有 rrd 日志面，
/// 观察走 UART/RTT 原始行；本类型把"事件 → 行"做成**无格式化宏依赖的纯函数**
/// （`core::fmt` 的浮点/对齐机制在最小固件面是体积负担，十进制手排 20 位封顶）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3GuardBuf {
    buf: [u8; RR3_GUARD_LINE_CAP],
    len: usize,
}

impl Rr3GuardBuf {
    /// 空缓冲。
    pub fn rr3_guard_buf_new() -> Self {
        Self {
            buf: [0; RR3_GUARD_LINE_CAP],
            len: 0,
        }
    }

    /// 格式化一个守护事件（覆盖上一行），返回本行的 `&str`。
    /// 文法见 [`Rr3GuardEvent`]；文法保证最长 42 字节 ≤ 容量，越界写按容量
    /// 截断兜底（不 panic、不回绕——即便未来行型扩张也不烧栈）。
    pub fn rr3_guard_format(&mut self, ev: &Rr3GuardEvent) -> &str {
        self.len = 0;
        self.guard_push(b"RR3G ");
        match *ev {
            Rr3GuardEvent::Boot(state) => self.guard_boot(state),
            Rr3GuardEvent::Feed { seq } => {
                self.guard_push(b"feed seq=");
                self.guard_push_u64(seq);
            }
            Rr3GuardEvent::Safety { trip, reason } => {
                self.guard_push(b"safe ");
                self.guard_push(if trip { b"trip " } else { b"deny " });
                self.guard_push(reason.rr3_guard_code().as_bytes());
            }
        }
        self.rr3_guard_as_str()
    }

    /// 起动态行体：`boot <registering | online task=<dec> | down>`。
    fn guard_boot(&mut self, state: Rr3LinkState) {
        self.guard_push(b"boot ");
        match state {
            Rr3LinkState::Registering => self.guard_push(b"registering"),
            Rr3LinkState::Online { task } => {
                self.guard_push(b"online task=");
                self.guard_push_u64(task);
            }
            Rr3LinkState::Down => self.guard_push(b"down"),
        }
    }

    /// 当前缓冲内容（最近一次格式化的行；空缓冲 = 空串）。
    pub fn rr3_guard_as_str(&self) -> &str {
        // 文法只产 ASCII 字节（字面量 + 十进制数字），UTF-8 校验恒成立
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// 追加字节（容量饱和截断，不 panic）。
    fn guard_push(&mut self, s: &[u8]) {
        let room = RR3_GUARD_LINE_CAP - self.len;
        let n = s.len().min(room);
        self.buf[self.len..self.len + n].copy_from_slice(&s[..n]);
        self.len += n;
    }

    /// 追加 u64 十进制（手排，最多 20 位；容量饱和截断）。
    fn guard_push_u64(&mut self, mut v: u64) {
        let mut digits = [0u8; 20];
        let mut n = 0;
        loop {
            digits[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
            if v == 0 {
                break;
            }
        }
        for i in (0..n).rev() {
            if self.len >= RR3_GUARD_LINE_CAP {
                return;
            }
            self.buf[self.len] = digits[i];
            self.len += 1;
        }
    }
}

#[cfg(test)]
mod tests;
