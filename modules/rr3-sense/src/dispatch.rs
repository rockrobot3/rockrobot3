//! rr3-sense 机制层注册面（0.0.8 W-A 批3 拆分，ADR-0017 五层表机制层）。
//!
//! [`Rr3SenseDispatcher`] 实现 rr3mod [`Dispatcher`]（serve 骨架回调）三入口：
//! `on_tick`＝采集循环体函数化（自原 composite.rs 主循环逐字迁；自 pacing 归一，
//! 0.0.8 方案 §2.6——read→判向窗积累→下混→publish→VAD 边沿→余量睡眠全在 tick
//! 内，poll_wait=ZERO 时序行为不变）；`on_event`＝播放期门控喂帧（B批 b2 订阅面，
//! tts_pcm 流帧；其余订阅帧与本件无关径直忽略）；`on_rpc`＝dump/ops/list/兜底
//! 三臂点名应答（兜底 Err 点名，serve 折 `{"error": …}` wire 等价）。
//!
//! 门控/判向窗两个纯机制切片已各自成件（[`crate::tts_gate`]/[`crate::doa_ring`]，
//! 批D1 迁出），本层只留反射环与编排：反射环吃 onset 判向换算 pan 目标（裁定①：
//! 快环落 sense，唯一持音频 conn 者）；门控吃 tts_pcm 帧流外推静音窗（裁定②：
//! 订阅既有主题零新契约）；判向窗 ring 是双麦链路的 TDOA 积累器（b3b：onset 时
//! 整窗回看交 capture::rr3_doa_fix）。策略量全在 config（Rr3ReflexConfig 等），
//! 此处只有判定与算术。
//!
//! 发布/读帧失败 Err 上抛致命：模块不装"活着"，死交给 supervisor 按
//! restart_policy=always 重启（rrd 的进程语义）。

mod nod;
mod pan;
mod pcm;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod servo_rpc;
mod state;
mod tick;

use std::time::{Duration, Instant};

use rr3mod::{BusHandle, Dispatcher};
use rr_hal_api::Rr3AudioCfg;
use rr_hal_api::Rr3AudioSource;
use serde_json::{json, Value};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use servo_rpc::rr3_sense_servo;

use crate::boot::Rr3SenseConfig;
use crate::capture;
use crate::doa_ring::Rr3DoaRing;
use crate::rpc;
use crate::tts_gate::Rr3TtsGate;
use crate::vad::Rr3VadState;

use nod::Rr3NodState;
use pcm::Rr3FrameBuf;

/// 本模块 exposes 的唯一事务面主题（module.toml [rpc].exposes 同源）
const RR3_DUMP_TOPIC: &str = "rr3-sense/capture/dump";

/// 本模块 exposes 的 ops 槽快照读口（批N4 件九二，module.toml [rpc].exposes 同源）：
/// 白名单组体应答，槽名与 assemble.toml 槽表申报同源（门 d covers 申报的对账面）
const RR3_OPS_LIST_TOPIC: &str = "rr3-sense/ops/list";

/// 舵机命令面（A9，桥单所有者）：`rr3-act` 的手动舵机命令经此 RPC 转交持桥的
/// 音频源旁路。仅在宿主桥档编译（espidf 板侧无跨进程桥冲突，不暴露此面）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
const RR3_SERVO_TOPIC: &str = "rr3-sense/servo/set";

/// 采集新鲜度运维计数键（批M M-β S-3）：`pcm_frames` 每实际上总线一帧 +1；
/// `pcm_empty_frames` 读回欠载空帧（read 实填 0 采样——"活着但不动"的饿死
/// 信号，MC-11 判据：卡死与静默停摆看得见）另账。经心跳自报 rrd 归并，
/// `rr3.ctl.counters` 读面键 `rr3-sense/pcm_frames` / `rr3-sense/pcm_empty_frames`。
const RR3_PCM_FRAMES_KEY: &str = "pcm_frames";
const RR3_PCM_EMPTY_FRAMES_KEY: &str = "pcm_empty_frames";

/// 播放期门控的订阅主题（B批 b2，contracts/topics.toml 在册）：brain 的 TTS
/// 回放流，帧体带 `ms`（块内样点时长）——门控外推的数据源，零新契约（裁定②）。
const RR3_TTS_PCM_TOPIC: &str = "rr3-brain/audio/tts_pcm";

/// 运维计数键（B批 b2 冒烟观测面）：门开期被弃的 onset 数 / 反射环实际下发的
/// pan 指令数。经心跳自报 rrd 归并（与 pcm_frames 同一通路）。
const RR3_VAD_ONSET_GATED_KEY: &str = "vad_onset_gated";
// 反射计数键与旁路下发点同径（宿主桥档专用，批N8 件三）
#[cfg(any(target_os = "linux", target_os = "macos"))]
const RR3_REFLEX_PAN_CMDS_KEY: &str = "reflex_pan_cmds";
/// 点头示意下发的 tilt 指令数（0.0.8 批"转完点头"，与 pan 计数分开记账：
/// 手势动是否真发生一眼可见）。
#[cfg(any(target_os = "linux", target_os = "macos"))]
const RR3_REFLEX_TILT_CMDS_KEY: &str = "reflex_tilt_cmds";

/// 反射环行程常量（机制面，策略量在 config）：pan 中位 = boot 姿态基准，
/// 单侧摆幅上限是板侧行程 fail-closed 之外的第二道软件限幅。
const RR3_REFLEX_PAN_CENTER_DEG: i32 = 90;
const RR3_REFLEX_PAN_SWING_DEG: i32 = 30;
/// tilt 中位（板侧行程 [80,100] 的中位 90）：点头手势的基准位——先回中位
/// 蓄势，再上仰 `nod_deg`（绝对角 = 中位 + nod_deg，板侧 fail-closed 裁决）。
/// 仅宿主桥档消费（espidf 档无命令旁路）；保留定义供测试与机制面同源。
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
const RR3_REFLEX_TILT_CENTER_DEG: i32 = 90;

/// 主循环状态整体持有（自原 composite.rs 局部状态迁入，状态归属随分发走）：
/// 音频源与采集形状、VAD 状态机、流 seq、门控/反射环/判向窗三切片状态。
/// cmd_tx/tts_gate/pending_pan/doa_cfg/doa_ring/open_cfg 为方案 §4.3 字段级
/// 规格的后补字段（批N28 件一笔5：B批 b2/b3b 后于方案快照——门控/反射环/
/// 判向窗三面与开源形状都有读者，"无用状态不迁移"的反面）。
pub(crate) struct Rr3SenseDispatcher {
    source: Box<dyn Rr3AudioSource>,
    /// "读帧失败({backend})"文案复用（探活编排随 boot.rs，后端名随句柄走）
    backend: &'static str,
    /// 采集拍构造 `Rr3PcmStream` 复用（boot.rs probe 按 config 真值开）
    open_cfg: Rr3AudioCfg,
    cfg: Rr3SenseConfig,
    /// capture::rr3_frames_per_tick 预计算一次（方案 §4.3 字段级规格字面）
    frames_per_tick: usize,
    vad: Rr3VadState,
    /// frame_ms——自 pacing 余量睡眠基准
    tick: Duration,
    /// pcm / vad 共用流序（现况）
    seq: u64,
    /// 反射环命令旁路（None = 后端无旁路；探测与警告随 boot.rs probe）。
    /// 仅宿主桥档读（espidf 档无旁路，字段保留以维持构造形状）。
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    cmd_tx: Option<rr_hal::backend::Rr3AudioCmdProducer>,
    /// 播放期门控状态机（B批 b2；on_event 喂帧、on_tick onset 臂读门）
    tts_gate: Rr3TtsGate,
    /// 反射环 pending（EMI 避让）：onset 判向换算的目标在此挂起，判停后才实际
    /// 下发——激磁尖峰不落采集窗（见模块文档第 4 条）。连发 onset 覆盖旧值
    /// （最新意图优先，板侧 audio_q 同款背压口径）。
    pending_pan: Option<u16>,
    /// 点头手势时序（0.0.8 批"转完点头"）：pan 下发后按 nod_delay_ms 先回 tilt
    /// 中位（蓄势位），再按 nod_hold_ms 上仰到中位 + nod_deg（示意"听到了"）。
    /// None = 无手势在途（nod_deg=0 或尚未判停）。
    /// 仅宿主桥档读写（espidf 档无旁路，字段保留以维持构造形状）。
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    pending_nod: Option<Rr3NodState>,
    /// 判向策略量一次收进 Rr3DoaCfg（B批 b3b：能量门/几何/档界整体注入机制件）
    doa_cfg: capture::Rr3DoaCfg,
    /// 判向窗 ring（B批 b3b）：双麦链路才分配（单声道恒无方向可判，不分配零开销）
    doa_ring: Option<Rr3DoaRing>,
    /// 采集帧缓冲池：稳态复用同一块 Vec（mono 链路每拍零分配，见 [`Rr3FrameBuf`]）
    read_pool: Rr3FrameBuf,
}

impl Dispatcher for Rr3SenseDispatcher {
    /// 点名应答三臂（dump／ops/list／兜底）：现行主循环 Request 臂逐字迁——
    /// 成功臂 Ok(json!) 应答，兜底臂 Err 点名（serve 折 `{"error": e}`，wire
    /// 等价；请求-应答语义不当致命错误）。
    fn on_rpc(&mut self, topic: &str, _args: &Value, _bus: &BusHandle) -> Result<Value, String> {
        if topic == RR3_DUMP_TOPIC {
            Ok(rpc::rr3_dump_result(self.seq, self.backend, &self.cfg))
        } else if topic == RR3_OPS_LIST_TOPIC {
            Ok(json!({ "ops": rpc::rr3_sense_ops(&self.cfg) }))
        } else {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if topic == RR3_SERVO_TOPIC {
                return rr3_sense_servo(self.cmd_tx.as_ref(), _args);
            }
            Err(rr3_sense_unknown(topic))
        }
    }

    /// 播放期门控喂帧（B批 b2 订阅面）：tts_pcm 流帧喂门控，其余订阅帧与本件
    /// 无关径直忽略。门控轮分隔/外推用真实时刻（原主循环 poll 排空逐字语义）。
    fn on_event(&mut self, topic: &str, body: &Value, _bus: &BusHandle) -> Result<(), String> {
        if topic == RR3_TTS_PCM_TOPIC {
            self.rr3_gate_feed(body);
        }
        Ok(())
    }

    /// 采集拍（自原 composite.rs 主循环体逐字迁，状态归 self；自 pacing 归一
    /// ——余量睡眠在本拍尾，方案 §2.6）。读帧/发布失败 Err 上抛致命。
    fn on_tick(&mut self, now: Instant, bus: &BusHandle) -> Result<(), String> {
        // 读一拍：buf 按"帧×声道"分配采样点数（F1 定形 read 收采样点 buf，
        // 返回实填采样点数，欠载按实数截断）
        let (raw, done) = self.rr3_read_frame()?;
        let pcm = self.rr3_tick_pub(raw, done, bus)?;
        self.rr3_tick_tail(&pcm, now, bus)?;
        // 拍尾归还缓冲：mono 链路下一拍原样复用（稳态零分配）；下行混成 mono
        // 的双麦链路容量不够即弃（不为此多留一份整拍内存）
        let need = self.frames_per_tick * self.cfg.audio.channels as usize;
        self.read_pool.rr3_put(pcm.samples, need);
        Ok(())
    }
}

/// 兜底面点名：exposes 清单（宿主档另含舵机命令面）。
fn rr3_sense_unknown(topic: &str) -> String {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let servo = format!("/{RR3_SERVO_TOPIC}");
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let servo = String::new();
    format!("rr3-sense 只 exposes {RR3_DUMP_TOPIC}/{RR3_OPS_LIST_TOPIC}{servo}，收到 {topic}")
}

#[cfg(test)]
mod tests {
    use super::nod::rr3_reflex_nod_plan;
    use super::pan::rr3_reflex_pan;
    use super::pcm::{rr3_pcm_downmix, Rr3FrameBuf};
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use super::servo_rpc::{rr3_axis_deg, rr3_sense_servo};
    use super::*;
    use rr_hal::Rr3PcmStream;

    /// A9 舵机命令面：单轴门 + 非桥主 fail-closed（无 cmd_tx 不许另开 UART）。
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn servo_axis_gate_and_owner_gate() {
        assert_eq!(rr3_axis_deg(&json!({"pan": 90}), "pan").unwrap(), Some(90));
        assert_eq!(rr3_axis_deg(&json!({"tilt": 0}), "tilt").unwrap(), Some(0));
        assert_eq!(rr3_axis_deg(&json!({}), "pan").unwrap(), None);
        assert!(rr3_axis_deg(&json!({"pan": 181}), "pan").is_err());
        assert!(rr3_axis_deg(&json!({"tilt": "x"}), "tilt").is_err());
        assert!(rr3_sense_servo(None, &json!({})).is_err(), "缺两键拒");
        assert!(
            rr3_sense_servo(None, &json!({"pan": 90})).is_err(),
            "非桥主拒"
        );
    }

    /// 流级下混：channels 归 1、rate 不动、samples 收缩；单麦原样返回（零改动）。
    /// 随下混函数自 capture/mech.rs 迁入（0.0.5 批I 深挖收口）。
    #[test]
    fn downmix_stream_rewrites_cfg_to_mono() {
        let stereo = Rr3PcmStream {
            cfg: Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 2,
                bit_depth: 16,
            },
            samples: vec![100, 300, -400, -200],
        };
        let mono = rr3_pcm_downmix(stereo);
        assert_eq!(mono.cfg.channels, 1);
        assert_eq!(mono.cfg.sample_rate_hz, 16000);
        // 2026-10-31：单声道化取能量更高通道（ch0 = 100²+400² > ch1 = 300²+200²）
        assert_eq!(mono.samples, vec![100, -400]);
        let one = Rr3PcmStream {
            cfg: Rr3AudioCfg {
                sample_rate_hz: 16000,
                channels: 1,
                bit_depth: 16,
            },
            samples: vec![7, 8, 9],
        };
        assert_eq!(rr3_pcm_downmix(one.clone()), one);
    }

    // ---------- B批 b2：反射环用例 ----------

    /// 反射环全量判据：未启用 / 门开 / 死区内三路不转头；过判据的换算
    /// = 中位 90 + clamp(deg, ±30)，左右号直通、双端限幅。
    #[test]
    fn reflex_pan_target_respects_enable_gate_and_deadband() {
        assert_eq!(rr3_reflex_pan(false, 10, -60, false), None, "未启用");
        assert_eq!(rr3_reflex_pan(true, 10, -60, true), None, "门开");
        assert_eq!(rr3_reflex_pan(true, 10, 5, false), None, "死区内");
        assert_eq!(rr3_reflex_pan(true, 10, -9, false), None, "死区负侧");
        // 死区边界恰过：|deg| >= deadband 即动作
        assert_eq!(rr3_reflex_pan(true, 10, 10, false), Some(100));
        assert_eq!(rr3_reflex_pan(true, 10, -15, false), Some(75));
        assert_eq!(rr3_reflex_pan(true, 10, 60, false), Some(120), "右摆限幅");
        assert_eq!(rr3_reflex_pan(true, 10, -60, false), Some(60), "左摆限幅");
    }

    /// 点头计划：nod_deg=0 不生成（旧版逐点等价）；两段目标 = 中位 → 中位+deg，
    /// 时刻按 delay/hold 排定；越出舵机全域不生成（板侧行程仍 fail-closed 裁决）。
    #[test]
    fn reflex_nod_plan_derives_two_segments_and_skips_disabled() {
        let now = Instant::now();
        assert_eq!(rr3_reflex_nod_plan(90, 0, 500, 200, now), None, "手势关");
        let st = rr3_reflex_nod_plan(90, 8, 500, 200, now).expect("手势开必生成");
        assert_eq!(st.dip_deg, 90, "蓄势段回 tilt 中位");
        assert_eq!(st.lift_deg, 98, "上仰段 = 中位 + nod_deg");
        assert_eq!(st.dip_at, now + Duration::from_millis(500));
        assert_eq!(st.lift_at, now + Duration::from_millis(700));
        assert!(!st.dip_sent);
        assert_eq!(
            rr3_reflex_nod_plan(90, 120, 500, 200, now),
            None,
            "越出 0..=180 不生成"
        );
        // 负值 = 下仰（行程内合法时照常生成，板侧裁决）
        assert_eq!(
            rr3_reflex_nod_plan(90, -8, 500, 200, now).unwrap().lift_deg,
            82
        );
    }

    /// 采集缓冲池：每拍拿到的都是"长度 need、全零"的可写缓冲；容量够就复用
    /// （稳态零分配），不够则丢弃重分配——与逐拍 `vec![0; need]` 语义等价。
    #[test]
    fn frame_pool_reuses_capacity_and_hands_zeroed_buffer() {
        let mut pool = Rr3FrameBuf::default();
        let mut buf = pool.rr3_take(8);
        assert_eq!(buf.len(), 8, "长度 = 帧×声道");
        assert!(buf.iter().all(|&s| s == 0), "交出去的必须是全零缓冲");
        let cap = buf.capacity();
        buf[0] = 7;
        buf.truncate(5);
        pool.rr3_put(buf, 8);
        let buf = pool.rr3_take(8);
        assert_eq!(buf.capacity(), cap, "容量够必须复用同一块（零分配）");
        assert!(buf.iter().all(|&s| s == 0), "复用前必须清掉上一拍残值");

        // 容量不够（双麦下行混成 mono 的拍）就丢弃，下次按需重分配
        let mut pool = Rr3FrameBuf::default();
        let mut small = Vec::with_capacity(4);
        small.extend_from_slice(&[1, 2, 3, 4]);
        pool.rr3_put(small, 8);
        let buf = pool.rr3_take(8);
        assert_eq!(buf.len(), 8);
        assert!(buf.iter().all(|&s| s == 0));
    }
}
