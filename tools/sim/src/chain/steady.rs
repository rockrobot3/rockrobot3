//! `--hold-ms` 稳态段：`rr3-ctl health` 逐拍聚合 + 收尾一次 `rr3-ctl status`
//! （纯逻辑，单测可钉）。
//!
//! 判据（实时性基线与阻塞面的操作口径，不做"看起来在测"的阈值游戏）：
//! - 四个产品模块在每拍都在表内（观察者是夹具，跳过不计）；
//! - 末拍全部 `running`（保持期间掉线/重启即失败）；
//! - 各模块 `since_beat_ms` 与 `beat_peak_ms` 峰值不触自己的 `watchdog_ms`
//!   （触线 = 判活输入已到边缘，rrd 下一轮就可能收尸），且各模块 `fan_dropped`
//!   为 0（丢帧点名到具体模块，便于直接查它的读循环）；
//! - 收尾 `rr3-ctl status` 的三个**单调**计数为 0：`fan_dropped`（广播积压丢帧）、
//!   `fan_write_timeout`（对端 5s 不收）与 `log_write slow`（单次落盘 ≥50ms）——
//!   单调计数收尾为 0 即证明全程未发生；同时把 `log_sink_max_us`（全程峰值）
//!   原样入报告。
//!
//! 报告给出每模块峰值与心跳/看门狗配置、以及全局阻塞面读数，数值归 CI/本机回执，
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

/// 单模块聚合：心跳间隔峰值（点读 + 续命拍峰值）、冷启动基线、配置心跳/看门狗、末拍相位。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Rr3SteadyMod {
    pub(crate) max_since_ms: u64,
    pub(crate) max_peak_ms: u64,
    /// 注册完成距 daemon 起点的毫秒数（`?` = 基准未定格）
    pub(crate) boot_ms: Option<u64>,
    pub(crate) heartbeat_ms: u64,
    pub(crate) watchdog_ms: u64,
    pub(crate) phase: String,
    /// 该模块广播积压丢帧累计（health 表 FAN_DROP 列；慢消费者点名）
    pub(crate) fan_dropped: u64,
}

/// 全段聚合：采样拍数与逐模块统计 + 收尾 status 的全局阻塞面读数。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Rr3SteadyAgg {
    pub(crate) samples: u32,
    pub(crate) mods: BTreeMap<String, Rr3SteadyMod>,
    /// 广播积压丢帧总数（单调；None = 未采到 status）
    pub(crate) fan_dropped: Option<u64>,
    /// 写超时累计（单调；None = 未采到 status）
    pub(crate) fan_wr_timeout: Option<u64>,
    /// 单次落盘 ≥50ms 的次数（单调）
    pub(crate) log_slow: Option<u64>,
    /// 单次落盘耗时峰值（us，全程）
    pub(crate) log_max_us: Option<u64>,
    /// 冷启动基线峰值（ms；全模块 `?` 时 None）
    pub(crate) max_boot_ms: Option<u64>,
}

impl Rr3SteadyAgg {
    /// 喂一拍 `rr3-ctl health` 表（表头与观察者行跳过；多余列向后兼容）。
    pub(crate) fn rr3_steady_feed(&mut self, table: &str) -> Result<(), String> {
        self.samples += 1;
        let mut rows = 0u32;
        for line in table.lines() {
            let Some(r) = rr3_steady_row(line)? else {
                continue;
            };
            let e = self.mods.entry(r.name).or_default();
            e.max_since_ms = e.max_since_ms.max(r.since_ms);
            e.max_peak_ms = e.max_peak_ms.max(r.peak_ms);
            e.heartbeat_ms = r.heartbeat_ms;
            e.watchdog_ms = r.watchdog_ms;
            e.phase = r.phase;
            e.fan_dropped = e.fan_dropped.max(r.fan_dropped);
            if let Some(b) = r.boot_ms {
                self.max_boot_ms = Some(self.max_boot_ms.unwrap_or(0).max(b));
            }
            rows += 1;
        }
        if rows == 0 {
            return Err("health 表没有任何模块行（rrd 在跑吗？）".to_string());
        }
        Ok(())
    }

    /// 收尾一次 `rr3-ctl status`：取三个**单调**计数与落盘峰值。
    /// 缺行即点名——静默当 0 会把"没采到"伪装成"很健康"。
    pub(crate) fn rr3_steady_tail(&mut self, status: &str) -> Result<(), String> {
        let (fd, wr, slow, max_us) = rr3_steady_scan(status)?;
        self.fan_dropped = Some(fd);
        self.fan_wr_timeout = Some(wr);
        self.log_slow = Some(slow);
        self.log_max_us = max_us;
        Ok(())
    }

    /// 稳态判据：四模块在表、末拍 running、两路峰值不触看门狗、无模块级丢帧、
    /// 全局阻塞面为零。
    pub(crate) fn rr3_steady_check(&self) -> Result<(), String> {
        let mut drops: Vec<String> = Vec::new();
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
            if e.max_peak_ms > e.watchdog_ms {
                return Err(format!(
                    "{m} 续命拍间隔峰值 {}ms 触看门狗 {}ms（稳态长尾越界）",
                    e.max_peak_ms, e.watchdog_ms
                ));
            }
            if e.fan_dropped > 0 {
                drops.push(format!("{m}={}", e.fan_dropped));
            }
        }
        if !drops.is_empty() {
            return Err(format!(
                "稳态段模块级丢帧：{}（该模块读速跟不上广播积压；查其读循环与订阅面）",
                drops.join(" ")
            ));
        }
        match (self.fan_dropped, self.fan_wr_timeout, self.log_slow) {
            (Some(0), Some(0), Some(0)) => Ok(()),
            (Some(fd), Some(wr), Some(slow)) => Err(format!(
                "稳态阻塞面非零：fan_dropped={fd} fan_write_timeout={wr} log_sink_slow={slow}\
                 （积压丢帧 / 对端不收 / 单次落盘 ≥50ms；按订阅者积压与磁盘抖动两条线归因）"
            )),
            _ => Err("稳态段未采到 status 阻塞面（判据缺失，不放行）".to_string()),
        }
    }

    /// 报告文本（模块名字典序；列宽固定，便于回执对读）。
    pub(crate) fn rr3_steady_text(&self, hold_ms: u64) -> String {
        let mut out = format!("稳态段：hold {hold_ms}ms，采样 {} 拍\n", self.samples);
        out.push_str("模块  最大 since_beat(ms)  续命峰值(ms)  心跳(ms)  看门狗(ms)  丢帧  末拍\n");
        for (name, e) in &self.mods {
            out.push_str(&format!(
                "{name}  {}  {}  {}  {}  {}  {}\n",
                e.max_since_ms,
                e.max_peak_ms,
                e.heartbeat_ms,
                e.watchdog_ms,
                e.fan_dropped,
                e.phase
            ));
        }
        out.push_str(&format!(
            "阻塞面：fan_dropped={} fan_write_timeout={} log_sink_slow={} log_sink_max_us={}\n",
            rr3_steady_opt(self.fan_dropped),
            rr3_steady_opt(self.fan_wr_timeout),
            rr3_steady_opt(self.log_slow),
            rr3_steady_opt(self.log_max_us)
        ));
        out.push_str(&format!("稳态摘要：{}\n", self.rr3_steady_summary()));
        out
    }

    /// CI 可读摘要（单行，机器可 grep）：`sim-chain.sh` 把它打成 check-run
    /// annotation——job 日志匿名 403，注解是唯一无需凭据的取数通道。
    pub(crate) fn rr3_steady_summary(&self) -> String {
        let mut since = 0u64;
        let mut peak = 0u64;
        let mut wd = u64::MAX;
        for e in self.mods.values() {
            since = since.max(e.max_since_ms);
            peak = peak.max(e.max_peak_ms);
            wd = wd.min(e.watchdog_ms);
        }
        format!(
            "since_max={since}ms peak_max={peak}ms wd_min={}ms fd={} wr={} slow={} log_max_us={} boot_max={}",
            if wd == u64::MAX { 0 } else { wd },
            rr3_steady_opt(self.fan_dropped),
            rr3_steady_opt(self.fan_wr_timeout),
            rr3_steady_opt(self.log_slow),
            rr3_steady_opt(self.log_max_us),
            rr3_steady_opt(self.max_boot_ms),
        )
    }
}

/// 未采到的读数渲染为 `?`（不拿 0 冒充）。
fn rr3_steady_opt(v: Option<u64>) -> String {
    v.map_or("?".to_string(), |n| n.to_string())
}

/// 扫 status 文本取四读数（缺行点名）。
fn rr3_steady_scan(status: &str) -> Result<(u64, u64, u64, Option<u64>), String> {
    let mut fd = None;
    let mut wr = None;
    let mut slow = None;
    let mut max_us = None;
    for line in status.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("fan_dropped = ") {
            fd = Some(rr3_steady_num(v, line, "fan_dropped")?);
        } else if let Some(v) = line.strip_prefix("fan_write_timeout = ") {
            wr = Some(rr3_steady_num(v, line, "fan_write_timeout")?);
        } else if let Some((max, s)) = rr3_steady_log_line(line)? {
            max_us = Some(max);
            slow = Some(s);
        }
    }
    Ok((
        fd.ok_or("status 缺 fan_dropped 行")?,
        wr.ok_or("status 缺 fan_write_timeout 行")?,
        slow.ok_or("status 缺 log_write 行")?,
        max_us,
    ))
}

/// `log_write = max_us=N slow=M` 一行；不是该行回 None。
fn rr3_steady_log_line(line: &str) -> Result<Option<(u64, u64)>, String> {
    let Some(v) = line.strip_prefix("log_write = max_us=") else {
        return Ok(None);
    };
    let (max, slow) = v
        .split_once(" slow=")
        .ok_or_else(|| format!("log_write 行缺 slow（{line}）"))?;
    Ok(Some((
        rr3_steady_num(max, line, "log_sink_max_us")?,
        rr3_steady_num(slow, line, "log_sink_slow")?,
    )))
}

/// 一行 health 的解析结果（表头/观察者返回 None）。
struct Rr3SteadyRow {
    name: String,
    phase: String,
    heartbeat_ms: u64,
    watchdog_ms: u64,
    since_ms: u64,
    peak_ms: u64,
    boot_ms: Option<u64>,
    fan_dropped: u64,
}

/// 非模块行：表头、空行与运行级汇总（`restart_exhausted=… watchdog_overruns=…`）。
fn rr3_steady_skip(line: &str) -> bool {
    line.starts_with("NAME") || line.starts_with("restart_exhausted=") || line.trim().is_empty()
}

/// 解析一行 health；`beat_peak_ms` 列缺位按 0（旧格式兼容，不静默当健康）。
fn rr3_steady_row(line: &str) -> Result<Option<Rr3SteadyRow>, String> {
    if rr3_steady_skip(line) {
        return Ok(None);
    }
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() < 6 {
        return Err(format!("health 行字段不足（{line}）"));
    }
    if f[0] == RR3_CHAIN_OBS {
        return Ok(None);
    }
    let peak_ms = rr3_steady_opt_num(&f, 6, line, "beat_peak_ms")?.unwrap_or(0);
    let boot_ms = rr3_steady_opt_num(&f, 7, line, "boot_ms")?;
    // FAN_DROP 列缺位按 0（旧格式兼容；数值错仍报错）
    let fan_dropped = rr3_steady_opt_num(&f, 8, line, "fan_dropped")?.unwrap_or(0);
    Ok(Some(Rr3SteadyRow {
        name: f[0].to_string(),
        phase: f[2].to_string(),
        heartbeat_ms: rr3_steady_num(f[3], line, "heartbeat_ms")?,
        watchdog_ms: rr3_steady_num(f[4], line, "watchdog_ms")?,
        since_ms: rr3_steady_num(f[5], line, "since_beat_ms")?,
        peak_ms,
        boot_ms,
        fan_dropped,
    }))
}

/// 可缺列（`?` = 未定格）的解析：缺列与 `?` 都回 None，其余必须是数字。
fn rr3_steady_opt_num(
    f: &[&str],
    at: usize,
    line: &str,
    field: &str,
) -> Result<Option<u64>, String> {
    match f.get(at) {
        Some(&"?") | None => Ok(None),
        Some(v) => Ok(Some(rr3_steady_num(v, line, field)?)),
    }
}

/// 六列里的数字列：非数字即点名（`?` 占位不许静默当 0）。
fn rr3_steady_num(s: &str, line: &str, field: &str) -> Result<u64, String> {
    s.parse()
        .map_err(|_| format!("health 行 {field} 非数字（{line}）"))
}
