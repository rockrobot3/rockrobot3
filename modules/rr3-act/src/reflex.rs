//! 反射环宿主进程内的 1k–10kHz 闭环。
//!
//! **永不进总线**（ADR-0007 裁定 1）：闭环数据本身不过 IPC；本文件唯一出总线的
//! 东西是异常/饱和通告 `rr3-act/motor/reflex`（notification）——它是异常面，
//! 不是数据流。v0.0.1 的最小环：按 loop_hz 节拍空转的宿主线程——1kHz 闭环的
//! "宿主位"真实存在（0.0.2 方案 §4：M5 只要求宿主位与命令流存在），环内控制律
//! （FOC/PID）随电机实装进入同一节拍。
//!
//! 饱和检测真值批D₃ 迁 rr-core `control`（ADR-0018 裁定 5：控制原语是注入
//! 时钟的纯计算）——cfg 形状/节拍方案/episode 状态机及窗结算、速率折算两纯
//! 函数的真值在 rr-core，本文件 re-export 保既有路径；std 主循环只做平台事
//! （sleep 节拍 + Instant 取时 + 总线发布）。

use std::time::{Duration, Instant};

use rr_bus_api::Rr3BusClient;
use serde_json::json;

pub use rr_core::control::{
    rr3_reflex_rate_hz, rr3_reflex_window_plan, rr3_reflex_window_saturated, Rr3ReflexCfg,
    Rr3SaturationDetector,
};

pub const RR3_REFLEX_NOTICE_TOPIC: &str = "rr3-act/motor/reflex";

/// 反射环 tick 运维计数键（批M M-β A-6）：每速率窗结算一次，值 = 窗内实际
/// 节拍数。1kHz 闭环的"宿主位真实存在"由此从日志宣言升级为可点验的记账线
/// （经心跳自报 rrd 归并，`rr3.ctl.counters` 读面键 `rr3-act/reflex_ticks`）。
pub const RR3_REFLEX_TICKS_KEY: &str = "reflex_ticks";

/// 启动自述：本子模块的面一行说明（闭环本体在进程内，总线只见异常通告）
pub const SURFACE: &[&str] =
    &["reflex: 1k-10kHz 闭环宿主（进程内，永不进总线）| 饱和通告 -> rr3-act/motor/reflex"];

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
            let saturated = rr3_reflex_window_saturated(ticks, expect);
            if det.rr3_detector_feed(saturated) {
                seq += 1;
                let rate_hz = rr3_reflex_rate_hz(ticks, window_ms);
                let _ = bus.rr3_publish(
                    RR3_REFLEX_NOTICE_TOPIC,
                    json!({ "seq": seq, "reason": "saturate", "rate_hz": rate_hz }),
                );
            }
            // 窗结算记账（批M M-β A-6）：值 = 窗内实跑节拍数，重置前落账；
            // bump 失败不进总线发布面（异常通告语义），静默容忍——计数是运维账
            bus.rr3_counter_bump(RR3_REFLEX_TICKS_KEY, ticks);
            ticks = 0;
            window_start = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 饱和检测真值批D₃ 迁 rr-core `control`（节拍方案/episode/孤立窗/下限
    /// 用例随迁）——本地只钉"re-export 可用"：cfg→窗方案→喂窗→结算走通
    /// rr-core 面。
    #[test]
    fn saturation_truth_is_rr_core_reexport() {
        let cfg = Rr3ReflexCfg {
            loop_hz: 1000,
            saturate_notice_ms: 500,
        };
        let (window_ms, need, expect) = rr3_reflex_window_plan(&cfg);
        assert_eq!(
            (window_ms, need, expect),
            (250, 2, 250),
            "节拍方案（rr-core 判定）"
        );
        let mut det = Rr3SaturationDetector::rr3_detector_new(need);
        assert!(
            !det.rr3_detector_feed(true),
            "第 1 个饱和窗：还在积累（rr-core 判定）"
        );
        assert!(
            det.rr3_detector_feed(true),
            "第 2 个连续饱和窗：episode 首报（rr-core 判定）"
        );
        assert!(
            rr3_reflex_window_saturated(124, 250) && !rr3_reflex_window_saturated(125, 250),
            "半拍线（rr-core 判定）"
        );
        assert_eq!(rr3_reflex_rate_hz(250, 250), 1000);
    }
}
