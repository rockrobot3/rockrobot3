//! 控制原语与安全核的机制核切片（0.0.5 第 II 轮批D₃，ADR-0018 裁定 5）。
//!
//! 裁定 5 的两句话是本模块的宪法：
//! 1. **控制原语与安全核是不读墙上时钟、不睡眠、时钟与输入由外部注入的纯计算**——
//!    同一张控制图在 Linux 的实时环里跑、在裸机的定时器里跑，行为逐拍一致
//!    （执行局部性是机制，行为图是策略）；
//! 2. **安全核不可被策略改写，永远包住任何控制图**——限位钳位在这里是最后
//!    一道（上游裁决已被绕过时仍不越行程），低电边沿与饱和通告的语义钉死在
//!    状态机内，调用方只能喂输入不能改语义。
//!
//! 四件原语的真值自模块面迁入（0.0.5 批D₃；模块侧 re-export 保既有路径）：
//! `Rr3RateGate`（rr3-act servo/state 限速流）、`Rr3LowEdge`（rr3-power
//! battery/low 边沿）、饱和检测三件（rr3-act 反射环：`Rr3ReflexCfg`/
//! `rr3_reflex_window_plan`/`Rr3SaturationDetector`）+ 限位钳位
//! [`rr3_travel_clamp`]（安全核新原语，rr3-act 两路落角前接线）。
//! 时钟一律 `u64` 毫秒注入（std `Instant` 到不了机制核）；零策略载荷。

/// servo/state 限速门：状态变化必发布，但两次发布至少隔 `min_interval_ms`。
/// 被限掉的这一次**不缓存不补发**——下一次发布自然携带最新角度：流是摘要，
/// 不是逐拍镜像（命令面本就 <100Hz，限速只挡突发连发）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3RateGate {
    min_interval_ms: u64,
    last_ms: Option<u64>,
}

impl Rr3RateGate {
    pub fn rr3_gate_new(min_interval_ms: u64) -> Self {
        Self {
            min_interval_ms,
            last_ms: None,
        }
    }

    /// `now_ms` 注入而非内部取时钟（裁定 5）。放行即记账（返回 true 的同时
    /// 刷新 last）。
    pub fn rr3_gate_allow(&mut self, now_ms: u64) -> bool {
        if let Some(t) = self.last_ms {
            if now_ms.saturating_sub(t) < self.min_interval_ms {
                return false;
            }
        }
        self.last_ms = Some(now_ms);
        true
    }
}

/// 低电边沿机：armed 时电压跌破阈值 → 报一次并解除武装；电压回到阈值**之上**
/// （严格大于，恰好等于阈值仍算低电段内）→ 重新武装。状态只有这一位。
/// 安全核语义：武装本身**不是**事件——[`Rr3LowEdge::rr3_low_edge_cross`]
/// 返回 true 的唯一含义是"发布 battery/low"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3LowEdge {
    low_voltage_mv: u16,
    armed: bool,
}

impl Rr3LowEdge {
    pub fn rr3_low_edge_new(low_voltage_mv: u16) -> Self {
        Self {
            low_voltage_mv,
            armed: true,
        }
    }

    /// 每拍采样喂一次电压。返回 true = 此刻应发布 battery/low（本次低电段的首拍）。
    pub fn rr3_low_edge_cross(&mut self, voltage_mv: u16) -> bool {
        if self.armed && voltage_mv <= self.low_voltage_mv {
            self.armed = false;
            return true;
        }
        if voltage_mv > self.low_voltage_mv {
            self.armed = true;
        }
        false
    }
}

/// 反射环 config `[rr3-act.reflex]` 的形状（机制核只认参数不认来源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ReflexCfg {
    pub loop_hz: u32,
    /// 连续饱和持续多久才发一次通告（episode 内不重复）
    pub saturate_notice_ms: u64,
}

/// 饱和判定的节拍方案（纯函数）：
/// 速率窗 250ms 粒度（低于 saturate_notice_ms，单拍抖动在窗内被平均掉；
/// notice 更小时压到与它同宽，保证"连续 need 窗"不越过 notice 承诺）；
/// 窗内实跑 tick 数低于半拍才算饱和（sleep 粒度的系统性超期不算）。
/// 返回 (window_ms, need_windows, expect_ticks_per_window)。
pub fn rr3_reflex_window_plan(cfg: &Rr3ReflexCfg) -> (u64, u32, u64) {
    let loop_hz = cfg.loop_hz.max(1) as u64;
    let window_ms = 250u64.min(cfg.saturate_notice_ms.max(1));
    let need = cfg.saturate_notice_ms.max(1).div_ceil(window_ms) as u32;
    let expect = (loop_hz * window_ms / 1000).max(1);
    (window_ms, need, expect)
}

/// 一窗的饱和判定（纯函数）：实跑 tick 数低于期望半拍即饱和。
pub fn rr3_reflex_window_saturated(ticks: u64, expect_ticks: u64) -> bool {
    ticks < expect_ticks / 2
}

/// 窗内实测速率折算（纯函数）：window_ms=0 视为无有效窗口，报 0——畸形输入
/// 往"不触发动作"方向收敛（安全核不得自毁，也不得虚报速率）。
pub fn rr3_reflex_rate_hz(ticks: u64, window_ms: u64) -> u64 {
    if window_ms == 0 {
        return 0;
    }
    ticks * 1000 / window_ms
}

/// 饱和 episode 状态机（纯逻辑，无时钟依赖）：
/// 连续 need 个饱和窗 → 恰好翻出一次 true（episode 首次）；episode 内后续
/// 饱和窗不再重复；一个非饱和窗即清零复位，再饱和重新计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3SaturationDetector {
    need: u32,
    run: u32,
    fired: bool,
}

impl Rr3SaturationDetector {
    pub fn rr3_detector_new(need_windows: u32) -> Self {
        Self {
            need: need_windows.max(1),
            run: 0,
            fired: false,
        }
    }

    /// feed 一窗的饱和判定结果，返回"此刻是否要发通告"。
    pub fn rr3_detector_feed(&mut self, saturated: bool) -> bool {
        if !saturated {
            self.run = 0;
            self.fired = false;
            return false;
        }
        self.run += 1;
        if !self.fired && self.run >= self.need {
            self.fired = true;
            return true;
        }
        false
    }
}

/// 限位钳位（安全核原语）：任何角度进执行器前钳进行程 `[travel_min, travel_max]`。
/// 即使上游裁决被绕过（裸机控制图直接算出角度），输出也不越行程——安全核
/// 永远包住控制图（裁定 5）。行程端点颠倒时仍收敛到有效区间（取两者较小值
/// 为下界），**饱和到最保守端点**，绝不 panic（安全核不得自毁）。
pub fn rr3_travel_clamp(angle_deg: u16, travel_min: u16, travel_max: u16) -> u16 {
    let (lo, hi) = if travel_min <= travel_max {
        (travel_min, travel_max)
    } else {
        (travel_max, travel_min)
    };
    angle_deg.clamp(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// 限速门：首拍必放；窗内连发被限；恰好到窗放行；窗从上次放行起算。
    /// now 全部合成注入（原 rr3-act servo 用例语义随迁，时钟改 ms）。
    #[test]
    fn rate_gate_admits_first_blocks_within_window_reopens_after() {
        let mut gate = Rr3RateGate::rr3_gate_new(50);
        assert!(gate.rr3_gate_allow(1_000), "首拍必放");
        assert!(!gate.rr3_gate_allow(1_010));
        assert!(!gate.rr3_gate_allow(1_049));
        assert!(gate.rr3_gate_allow(1_050), "恰好到窗放行");
        assert!(!gate.rr3_gate_allow(1_060));
        assert!(gate.rr3_gate_allow(1_100), "窗从上次放行起算");
    }

    /// 低电边沿主语义：越阈第一拍恰报一次，段内后续拍不再报
    /// （原 rr3-power battery 用例语义随迁）。
    #[test]
    fn low_edge_fires_once_per_episode() {
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(!e.rr3_low_edge_cross(3900), "高电平不触发");
        assert!(e.rr3_low_edge_cross(3300), "恰好等于阈值算越阈（≤）");
        assert!(!e.rr3_low_edge_cross(3250), "段内第二拍不再报");
        assert!(!e.rr3_low_edge_cross(3200), "段内继续跌也不再报");
    }

    /// 严格大于才重新武装；武装本身不是事件（原 rr3-power 用例语义随迁）
    #[test]
    fn low_edge_rearms_only_above_threshold() {
        let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
        assert!(e.rr3_low_edge_cross(3290));
        assert!(!e.rr3_low_edge_cross(3300), "等于阈值不恢复");
        assert!(
            !e.rr3_low_edge_cross(3301),
            "严格大于才重新武装——武装本身不触发，否则发出高电压的'低电'事件"
        );
        assert!(!e.rr3_low_edge_cross(3301), "武装后高电平拍照旧不触发");
        assert!(e.rr3_low_edge_cross(3200), "重新武装已生效：再次越阈再报");
        assert!(!e.rr3_low_edge_cross(3200));
    }

    /// 节拍方案三例（原 rr3-act reflex 用例语义随迁）
    #[test]
    fn window_plan_derives_window_need_and_expect() {
        let cfg = Rr3ReflexCfg {
            loop_hz: 1000,
            saturate_notice_ms: 500,
        };
        assert_eq!(rr3_reflex_window_plan(&cfg), (250, 2, 250));
        let cfg = Rr3ReflexCfg {
            loop_hz: 1000,
            saturate_notice_ms: 100,
        };
        assert_eq!(rr3_reflex_window_plan(&cfg), (100, 1, 100));
        let cfg = Rr3ReflexCfg {
            loop_hz: 100,
            saturate_notice_ms: 500,
        };
        assert_eq!(rr3_reflex_window_plan(&cfg), (250, 2, 25));
    }

    /// episode 语义：连续 need 窗饱和才翻 true 且只翻一次；非饱和窗复位
    /// （原 rr3-act reflex 用例语义随迁）
    #[test]
    fn detector_fires_once_per_saturation_episode() {
        let mut det = Rr3SaturationDetector::rr3_detector_new(2);
        assert!(!det.rr3_detector_feed(true), "第 1 个饱和窗：还在积累");
        assert!(
            det.rr3_detector_feed(true),
            "第 2 个连续饱和窗：episode 首报"
        );
        assert!(!det.rr3_detector_feed(true), "episode 内不重复");
        assert!(!det.rr3_detector_feed(true), "episode 内不重复（继续）");
        assert!(!det.rr3_detector_feed(false), "恢复窗复位且不发");
        assert!(!det.rr3_detector_feed(true), "新 episode 重新积累");
        assert!(det.rr3_detector_feed(true), "新 episode 再次首报");
    }

    /// 孤立饱和窗夹恢复窗永不触发——抖动不是饱和
    #[test]
    fn isolated_saturated_window_never_fires() {
        let mut det = Rr3SaturationDetector::rr3_detector_new(2);
        for _ in 0..10 {
            assert!(!det.rr3_detector_feed(true));
            assert!(!det.rr3_detector_feed(false));
        }
    }

    /// 窗结算判定与速率折算的边界（半拍线 / 零除防御）
    #[test]
    fn window_saturated_and_rate_hz_boundaries() {
        assert!(!rr3_reflex_window_saturated(125, 250), "恰半拍不算饱和");
        assert!(rr3_reflex_window_saturated(124, 250), "低于半拍即饱和");
        assert!(rr3_reflex_window_saturated(0, 250));
        assert_eq!(rr3_reflex_rate_hz(250, 250), 1000);
        assert_eq!(rr3_reflex_rate_hz(0, 250), 0);
        assert_eq!(rr3_reflex_rate_hz(10, 0), 0, "window_ms=0 不炸除零");
    }

    /// 限位钳位：界内恒等 / 钳两端 / 端点颠倒收敛且饱和到最保守端
    #[test]
    fn travel_clamps_into_range_and_survives_inverted_range() {
        assert_eq!(rr3_travel_clamp(90, 0, 180), 90, "界内恒等");
        assert_eq!(rr3_travel_clamp(0, 0, 180), 0, "下界恒等");
        assert_eq!(rr3_travel_clamp(180, 0, 180), 180, "上界恒等");
        assert_eq!(rr3_travel_clamp(5, 45, 135), 45, "钳到下界");
        assert_eq!(rr3_travel_clamp(200, 45, 135), 135, "钳到上界");
        assert_eq!(rr3_travel_clamp(10, 135, 45), 45, "颠倒行程收敛不 panic");
        assert_eq!(
            rr3_travel_clamp(200, 135, 45),
            135,
            "颠倒行程仍收敛到有效区间 [45,135]：200 钳到最近端点"
        );
    }

    /// 逐拍一致性（批D₃ 判据：同一控制图注入时钟逐拍一致）——同一张图
    /// （限速门 + 限位钳位 + 低电边沿）喂两遍同一合成时钟序列，输出序列
    /// 逐拍相等：机制核纯函数无隐藏状态、无隐藏时钟，Linux 环与裸机定时器
    /// 跑同一图必然同拍。
    #[test]
    fn same_control_graph_beat_for_beat_identical() {
        // 控制图：第 k 拍时刻 now=1000+10k，角度在 [0,90,180,270] 轮转，
        // 电压在 [3400, 3290, 3310, 3200] 轮转
        let run = || {
            let mut gate = Rr3RateGate::rr3_gate_new(50);
            let mut edge = Rr3LowEdge::rr3_low_edge_new(3300);
            let mut out = Vec::new();
            for k in 0..24u64 {
                let now = 1_000 + 10 * k;
                let angle = rr3_travel_clamp([0u16, 90, 180, 270][k as usize % 4], 0, 180);
                let published = gate.rr3_gate_allow(now);
                let low = edge.rr3_low_edge_cross([3400u16, 3290, 3310, 3200][k as usize % 4]);
                out.push((now, angle, published, low));
            }
            out
        };
        let a = run();
        let b = run();
        assert_eq!(a, b, "同一图同一输入必须逐拍一致");
        // 图本身的行为抽查：限速 50ms/拍 10ms → 每 5 拍放 1 次；低电 3290 首报、
        // 3310（>3300）重新武装、3200 再报
        assert!(a[0].2);
        assert!(!a[1].2);
        assert!(a[5].2, "1_050 距 1_000 恰 50ms，放行");
        assert!(a[1].3, "3290 越阈首报");
        assert!(!a[2].3, "3310 重新武装但不是事件");
        assert!(a[3].3, "3200 再次越阈再报");
    }
}

// ---------------------------------------------------------------------------
// 批G G1：安全核逐拍对拍向量（点验矩阵 ⑤ 列载体，architecture §12.6；
// 0.0.7 方案 §3/§4 批G 行——⑤ 判据：板上真跑输出与宿主面 bit-for-bit 相同，
// 用例带两条腿：**确定性腿** = 板输出 == 宿主输出（同一输入序列，宿主在
// harness 里用同一 runner 跑），**契约期望腿** = 期望值来自裁定语义（下方
// [`parity::RR3_PARITY_EXPECTED`] 手推，语义即本文件上方各原语的单元用例 +
// 模块头"裁定 5"两句话）。
//
// 单源纪律：输入序列与期望向量只在这里一份；宿主（rr3-sentinel-harness）与
// 板上（espidf-sentinel）各自消费同一 runner [`parity::rr3_parity_run`] 跑
// **真实原语**——板上固件不得直接回显 EXPECTED，那等于自欺（⑤ 列要的是行为
// 证据，不是复读机）。
//
// 全整数、时钟 u64 ms 注入——Rust 整数语义在 xtensa LX6 与宿主一致，无浮点
// 无平台差，bit-for-bit 可比成立的前提即裁定 5 的"不读墙上时钟、不睡眠、
// 纯计算"。定长数组零堆，bare/rtos 档同吃（机制核无全局分配器假设）。
// ---------------------------------------------------------------------------
pub mod parity {
    //! 安全核逐拍对拍套件（⑤ 列）：见文件尾总注记。固件/宿主侧序列化各做
    //! （serde_json::json! 手工展开——机制核零 serde 依赖是 ⑧-b 依赖闭包门硬项）。

    use super::{
        rr3_reflex_rate_hz, rr3_reflex_window_plan, rr3_reflex_window_saturated, rr3_travel_clamp,
        Rr3LowEdge, Rr3RateGate, Rr3ReflexCfg, Rr3SaturationDetector,
    };

    /// 套件版本（响应体 `v` 字段；向量/语义改动即 bump，防新旧错配误对拍）。
    pub const RR3_PARITY_VERSION: u32 = 1;

    /// 一轮全量输出（定长数组，no_std 兼容）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Rr3ParityOutputs {
        /// 限速门放行序列（[`RR3_RATE_NOW_MS`] × [`RR3_RATE_MIN_INTERVAL_MS`]）
        pub rate: [bool; 8],
        /// 低电边沿越阈序列（[`RR3_LOW_VOLTAGES`] × [`RR3_LOW_THRESHOLD_MV`]）
        pub low: [bool; 8],
        /// 饱和 episode 通告序列（[`RR3_SAT_WINDOWS`]，need 由用例一的 plan 派生
        /// ——与 rr3-act 反射环装配同构：先 plan 再 detector）
        pub sat: [bool; 6],
        /// 节拍方案三例：(window_ms, need, expect) × [`RR3_SAT_CFGS`]
        pub plan: [(u64, u32, u64); 3],
        /// 窗结算判定三例（[`RR3_WSAT_TICKS`]）
        pub wsat: [bool; 3],
        /// 速率折算三例（[`RR3_RHZ_CASES`]）
        pub rhz: [u64; 3],
        /// 限位钳位五例（含端点颠倒，[`RR3_CLAMP_CASES`]）
        pub clamp: [u16; 5],
    }

    // ----- 输入序列（每条注释即语义出处：本文件上方单元用例） -----
    /// 限速门：首拍必放 / 窗内限 / 恰到窗放 / 窗从上次放行起算（50ms 档）。
    pub const RR3_RATE_MIN_INTERVAL_MS: u64 = 50;
    pub const RR3_RATE_NOW_MS: [u64; 8] = [1_000, 1_010, 1_049, 1_050, 1_060, 1_100, 1_101, 1_150];
    /// 低电边沿：恰等阈值算越阈（≤）/ 段内不重报 / 严格大于才重新武装。
    pub const RR3_LOW_THRESHOLD_MV: u16 = 3_300;
    pub const RR3_LOW_VOLTAGES: [u16; 8] = [3_900, 3_300, 3_250, 3_200, 3_300, 3_301, 3_200, 3_200];
    /// 饱和 episode：连续 need 窗才首报一次 / episode 内不重复 / 非饱和窗复位。
    pub const RR3_SAT_WINDOWS: [bool; 6] = [true, true, true, false, true, true];
    /// 节拍方案三例（window 粒度 250ms 上限 / need 向上取整 / expect 半拍线）。
    pub const RR3_SAT_CFGS: [Rr3ReflexCfg; 3] = [
        Rr3ReflexCfg {
            loop_hz: 1000,
            saturate_notice_ms: 500,
        },
        Rr3ReflexCfg {
            loop_hz: 1000,
            saturate_notice_ms: 100,
        },
        Rr3ReflexCfg {
            loop_hz: 100,
            saturate_notice_ms: 500,
        },
    ];
    /// 窗结算：恰半拍不算饱和 / 低于半拍即饱和 / 0 tick 必饱和。
    pub const RR3_WSAT_TICKS: [(u64, u64); 3] = [(125, 250), (124, 250), (0, 250)];
    /// 速率折算：常规 / 0 tick / window=0 防除零（畸形输入向"不动作"收敛）。
    pub const RR3_RHZ_CASES: [(u64, u64); 3] = [(250, 250), (0, 250), (10, 0)];
    /// 钳位：界内恒等 / 钳下界 / 钳上界 / 端点颠倒收敛最保守端 ×2。
    pub const RR3_CLAMP_CASES: [(u16, u16, u16); 5] = [
        (90, 0, 180),
        (5, 45, 135),
        (200, 45, 135),
        (10, 135, 45),
        (200, 135, 45),
    ];

    /// **契约期望腿**：手推自裁定语义（与上方单元用例同源），不是跑出来的。
    /// runner 实跑 ≠ 此向量 = 期望推错或原语语义漂移，宿主先红（守门）。
    pub const RR3_PARITY_EXPECTED: Rr3ParityOutputs = Rr3ParityOutputs {
        rate: [true, false, false, true, false, true, false, true],
        low: [false, true, false, false, false, false, true, false],
        sat: [false, true, false, false, false, true],
        plan: [(250, 2, 250), (100, 1, 100), (250, 2, 25)],
        wsat: [false, true, true],
        rhz: [1000, 0, 0],
        clamp: [90, 45, 135, 45, 135],
    };

    /// 真跑一轮（宿主与板上同一份代码）：各原语按输入序列过一遍，返回实际输出。
    pub fn rr3_parity_run() -> Rr3ParityOutputs {
        let mut gate = Rr3RateGate::rr3_gate_new(RR3_RATE_MIN_INTERVAL_MS);
        let mut rate = [false; 8];
        for (i, now) in RR3_RATE_NOW_MS.iter().enumerate() {
            rate[i] = gate.rr3_gate_allow(*now);
        }
        let mut edge = Rr3LowEdge::rr3_low_edge_new(RR3_LOW_THRESHOLD_MV);
        let mut low = [false; 8];
        for (i, mv) in RR3_LOW_VOLTAGES.iter().enumerate() {
            low[i] = edge.rr3_low_edge_cross(*mv);
        }
        let (window_ms, need, _expect) = rr3_reflex_window_plan(&RR3_SAT_CFGS[0]);
        let _ = window_ms;
        let mut det = Rr3SaturationDetector::rr3_detector_new(need);
        let mut sat = [false; 6];
        for (i, s) in RR3_SAT_WINDOWS.iter().enumerate() {
            sat[i] = det.rr3_detector_feed(*s);
        }
        let mut plan = [(0u64, 0u32, 0u64); 3];
        for (i, cfg) in RR3_SAT_CFGS.iter().enumerate() {
            plan[i] = rr3_reflex_window_plan(cfg);
        }
        let mut wsat = [false; 3];
        for (i, (ticks, expect)) in RR3_WSAT_TICKS.iter().enumerate() {
            wsat[i] = rr3_reflex_window_saturated(*ticks, *expect);
        }
        let mut rhz = [0u64; 3];
        for (i, (ticks, window)) in RR3_RHZ_CASES.iter().enumerate() {
            rhz[i] = rr3_reflex_rate_hz(*ticks, *window);
        }
        let mut clamp = [0u16; 5];
        for (i, (angle, lo, hi)) in RR3_CLAMP_CASES.iter().enumerate() {
            clamp[i] = rr3_travel_clamp(*angle, *lo, *hi);
        }
        Rr3ParityOutputs {
            rate,
            low,
            sat,
            plan,
            wsat,
            rhz,
            clamp,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 期望腿守门：runner 实跑必须逐位等于契约期望（宿主面上的第一道门，
        /// 板上对拍之前的预检——期望推错或原语语义漂移都在这里先红）。
        #[test]
        fn parity_runner_matches_contract_expected() {
            assert_eq!(
                rr3_parity_run(),
                RR3_PARITY_EXPECTED,
                "实跑必须逐位等于契约期望（期望腿守门）"
            );
        }
    }
}
