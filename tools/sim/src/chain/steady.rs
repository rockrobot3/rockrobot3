//! `--hold-ms` 稳态段：`rr3-ctl health` 逐拍聚合（纯逻辑，单测可钉）。
//!
//! 判据（实时性基线的操作口径，不做"看起来在测"的阈值游戏）：
//! - 四个产品模块在每拍都在表内（观察者是夹具，跳过不计）；
//! - 末拍全部 `running`（保持期间掉线/重启即失败）；
//! - 各模块 `since_beat_ms` 峰值不触自己的 `watchdog_ms`（触线 = 判活输入已
//!   到边缘，rrd 下一轮就可能收尸）。
//!
//! 报告给出每模块的 since_beat 峰值与心跳/看门狗配置，数值归 CI/本机回执，
//! 不在此设"良好/不良"档位——档位需要跨环境基线，先收数。

use std::collections::BTreeMap;

use crate::drill::{RR3_SIM_MODULE_A, RR3_SIM_MODULE_B, RR3_SIM_MODULE_BRAIN};

use super::{RR3_CHAIN_OBS, RR3_SIM_MODULE_ACT};

/// 稳态判据覆盖的产品模块（与 `rr3_chain_mods_up` 的等待集合同源）。
pub(crate) const RR3_STEADY_MODS: [&str; 4] = [
    RR3_SIM_MODULE_A,
    RR3_SIM_MODULE_B,
    RR3_SIM_MODULE_BRAIN,
    RR3_SIM_MODULE_ACT,
];

/// 单模块聚合：心跳间隔峰值、配置心跳/看门狗、末拍相位。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Rr3SteadyMod {
    pub(crate) max_since_ms: u64,
    pub(crate) heartbeat_ms: u64,
    pub(crate) watchdog_ms: u64,
    pub(crate) phase: String,
}

/// 全段聚合：采样拍数与逐模块统计。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Rr3SteadyAgg {
    pub(crate) samples: u32,
    pub(crate) mods: BTreeMap<String, Rr3SteadyMod>,
}

impl Rr3SteadyAgg {
    /// 喂一拍 `rr3-ctl health` 表（表头 + 六列；观察者行跳过）。
    pub(crate) fn rr3_steady_feed(&mut self, table: &str) -> Result<(), String> {
        self.samples += 1;
        let mut rows = 0u32;
        for line in table.lines() {
            if line.starts_with("NAME") || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 6 {
                return Err(format!("health 行不是六列（{line}）"));
            }
            if f[0] == RR3_CHAIN_OBS {
                continue;
            }
            let hb = rr3_steady_num(f[3], line, "heartbeat_ms")?;
            let wd = rr3_steady_num(f[4], line, "watchdog_ms")?;
            let since = rr3_steady_num(f[5], line, "since_beat_ms")?;
            let e = self.mods.entry(f[0].to_string()).or_default();
            e.max_since_ms = e.max_since_ms.max(since);
            e.heartbeat_ms = hb;
            e.watchdog_ms = wd;
            e.phase = f[2].to_string();
            rows += 1;
        }
        if rows == 0 {
            return Err("health 表没有任何模块行（rrd 在跑吗？）".to_string());
        }
        Ok(())
    }

    /// 稳态判据：四模块在表、末拍 running、since_beat 峰值不触看门狗。
    pub(crate) fn rr3_steady_check(&self) -> Result<(), String> {
        for m in RR3_STEADY_MODS {
            let Some(e) = self.mods.get(m) else {
                return Err(format!("稳态段缺模块 {m}（保持期间未注册或已退场）"));
            };
            if e.phase != "running" {
                return Err(format!("稳态段末拍 {m} 不在 running: {}", e.phase));
            }
            if e.max_since_ms > e.watchdog_ms {
                return Err(format!(
                    "{m} 心跳间隔峰值 {}ms 触看门狗 {}ms（保持期间掉拍）",
                    e.max_since_ms, e.watchdog_ms
                ));
            }
        }
        Ok(())
    }

    /// 报告文本（模块名字典序；列宽固定，便于回执对读）。
    pub(crate) fn rr3_steady_text(&self, hold_ms: u64) -> String {
        let mut out = format!("稳态段：hold {hold_ms}ms，采样 {} 拍\n", self.samples);
        out.push_str("模块  最大 since_beat(ms)  心跳(ms)  看门狗(ms)  末拍\n");
        for (name, e) in &self.mods {
            out.push_str(&format!(
                "{name}  {}  {}  {}  {}\n",
                e.max_since_ms, e.heartbeat_ms, e.watchdog_ms, e.phase
            ));
        }
        out
    }
}

/// 六列里的数字列：非数字即点名（`?` 占位不许静默当 0）。
fn rr3_steady_num(s: &str, line: &str, field: &str) -> Result<u64, String> {
    s.parse()
        .map_err(|_| format!("health 行 {field} 非数字（{line}）"))
}
