//! 反射环宿主进程内的 1k–10kHz 闭环。
//!
//! **永不进总线**（ADR-0007 裁定 1）：闭环数据本身不过 IPC；本文件唯一出总线的
//! 东西是异常/饱和通告 `rr3-act/motor/reflex`（notification）——它是异常面，
//! 不是数据流。v0.0.1 的最小环：按 loop_hz 节拍空转的宿主线程——1kHz 闭环的
//! "宿主位"真实存在（0.0.2 方案 §4：M5 只要求宿主位与命令流存在），环内控制律
//! （FOC/PID）随电机实装进入同一节拍。

use std::time::{Duration, Instant};

use rr_bus::Rr3BusClient;
use serde_json::json;

pub const RR3_REFLEX_NOTICE_TOPIC: &str = "rr3-act/motor/reflex";

/// 启动自述：本子模块的面一行说明（闭环本体在进程内，总线只见异常通告）
pub const SURFACE: &[&str] =
    &["reflex: 1k-10kHz 闭环宿主（进程内，永不进总线）| 饱和通告 -> rr3-act/motor/reflex"];

/// config `[rr3-act.reflex]` 的形状
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ReflexCfg {
    pub loop_hz: u32,
    /// 连续饱和持续多久才发一次通告（episode 内不重复）
    pub saturate_notice_ms: u64,
}

/// 饱和判定的节拍方案（纯函数，可测）：
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

/// 饱和 episode 状态机（纯逻辑，无时钟依赖）：
/// 连续 need 个饱和窗 → 恰好翻出一次 true（episode 首次）；episode 内后续
/// 饱和窗不再重复；一个非饱和窗即清零复位，再饱和重新计数。
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

    /// feed 一窗的饱和判定结果，返回"此刻是否要发通告"
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

/// 宿主线程主循环（不返回）：按 1/loop_hz 节拍空转；每速率窗结算一次实际速率，
/// 饱和 episode 触发时经总线发**恰好一条**通告。总线死了不重试不退出——
/// 主循环的死亡探针会先一步终结整个进程（restart_policy=always 兜底）。
pub fn rr3_reflex_loop(cfg: Rr3ReflexCfg, bus: Rr3BusClient) {
    let loop_hz = cfg.loop_hz.max(1);
    let period = Duration::from_micros(1_000_000 / loop_hz as u64);
    let (window_ms, need, expect) = rr3_reflex_window_plan(&cfg);
    let window = Duration::from_millis(window_ms);
    let mut det = Rr3SaturationDetector::rr3_detector_new(need);
    let mut seq: u64 = 0;
    let mut ticks: u64 = 0;
    let mut window_start = Instant::now();
    loop {
        // 环位：控制律的挂点。v0.0.1 最小环为空转，电机实装后同节拍换掉这一行
        std::thread::sleep(period);
        ticks += 1;
        let now = Instant::now();
        if now.duration_since(window_start) >= window {
            let saturated = ticks < expect / 2;
            if det.rr3_detector_feed(saturated) {
                seq += 1;
                let rate_hz = ticks * 1000 / window_ms;
                let _ = bus.rr3_publish(
                    RR3_REFLEX_NOTICE_TOPIC,
                    json!({ "seq": seq, "reason": "saturate", "rate_hz": rate_hz }),
                );
            }
            ticks = 0;
            window_start = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_plan_derives_window_need_and_expect() {
        // 常规：1kHz × 500ms notice → 250ms 窗、连续 2 窗、窗内期望 250 拍
        let cfg = Rr3ReflexCfg {
            loop_hz: 1000,
            saturate_notice_ms: 500,
        };
        assert_eq!(rr3_reflex_window_plan(&cfg), (250, 2, 250));
        // notice 小于窗粒度：窗压到与 notice 同宽，need 恒 1
        let cfg = Rr3ReflexCfg {
            loop_hz: 1000,
            saturate_notice_ms: 100,
        };
        assert_eq!(rr3_reflex_window_plan(&cfg), (100, 1, 100));
        // 低速率：窗内期望按比例折算且不低于 1
        let cfg = Rr3ReflexCfg {
            loop_hz: 100,
            saturate_notice_ms: 500,
        };
        assert_eq!(rr3_reflex_window_plan(&cfg), (250, 2, 25));
    }

    /// episode 语义：连续 need 窗饱和才翻 true 且只翻一次；非饱和窗复位，
    /// 再饱和从头计数——通告频率被 notice 承诺钉死，不随饱和时长线性增长。
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

    /// 单个孤立饱和窗夹在恢复窗之间永不触发——抖动不是饱和
    #[test]
    fn isolated_saturated_window_never_fires() {
        let mut det = Rr3SaturationDetector::rr3_detector_new(2);
        for _ in 0..10 {
            assert!(!det.rr3_detector_feed(true));
            assert!(!det.rr3_detector_feed(false));
        }
    }

    /// need 下限保护：0 或 1 都按"一窗即报"处理
    #[test]
    fn detector_need_floor_is_one() {
        for need in [0, 1] {
            let mut det = Rr3SaturationDetector::rr3_detector_new(need);
            assert!(det.rr3_detector_feed(true), "need={need} 一窗即报");
        }
    }
}
