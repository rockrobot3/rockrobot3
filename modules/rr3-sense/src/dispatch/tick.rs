//! 采集拍：读帧、判向窗积累、下混发布、VAD 边沿与 pacing。

use std::time::Instant;

use rr3mod::BusHandle;
use rr_hal::Rr3PcmStream;
use serde_json::Value;

use crate::capture;
use crate::capture::Rr3DoaFix;
use crate::doa_ring::rr3_ring_feed;
use crate::rpc;
use crate::vad::{self, Rr3VadEdge};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use rr_hal::backend::Rr3PeriphCmd;

use super::nod::rr3_reflex_nod_plan;
use super::pan::rr3_reflex_pan;
use super::pcm::rr3_pcm_downmix;
use super::Rr3SenseDispatcher;
use super::{RR3_PCM_EMPTY_FRAMES_KEY, RR3_PCM_FRAMES_KEY, RR3_VAD_ONSET_GATED_KEY};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::{RR3_REFLEX_PAN_CMDS_KEY, RR3_REFLEX_TILT_CENTER_DEG};

impl Rr3SenseDispatcher {
    /// 门控喂帧：`ms` 缺席即整帧忽略（帧体适配口径与上游同款）。
    pub(super) fn rr3_gate_feed(&mut self, body: &Value) {
        let Some(ms) = body.get("ms").and_then(Value::as_u64) else {
            return;
        };
        self.tts_gate.rr3_tts_gate_feed(
            Instant::now(),
            ms,
            self.cfg.reflex.gate_gap_ms,
            self.cfg.reflex.gate_tail_ms,
        );
    }

    /// 拍尾段：VAD 节拍 → 点头手势推进 → 节奏（失败原样上抛）。
    pub(super) fn rr3_tick_tail(
        &mut self,
        pcm: &Rr3PcmStream,
        now: Instant,
        bus: &BusHandle,
    ) -> Result<(), String> {
        self.rr3_vad_beat(pcm, now, bus)?;

        // 点头手势推进（0.0.8 批）：与 VAD 边沿无关，每拍看时刻——蓄势/上仰
        // 两段按 config 时序到点下发（宿主桥档专用，espidf 档无旁路自然跳过）
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        self.rr3_nod_step(now, bus);

        self.rr3_pace(now);
        Ok(())
    }

    /// 读一拍：从缓冲池取"帧×声道"采样点（池里有足够容量就不分配），欠载按
    /// 实填数截断；缓冲随 [`Rr3PcmStream`] 走完本拍后在 `on_tick` 尾归还。
    pub(super) fn rr3_read_frame(&mut self) -> Result<(Rr3PcmStream, usize), String> {
        let need = self.frames_per_tick * self.cfg.audio.channels as usize;
        let mut buf = self.read_pool.rr3_take(need);
        let done = self
            .source
            .read(&mut buf)
            .map_err(|e| format!("读帧失败({}): {e}", self.backend))?;
        buf.truncate(done);
        Ok((
            Rr3PcmStream {
                cfg: self.open_cfg,
                samples: buf,
            },
            done,
        ))
    }

    /// 判向窗积累 + 下混入总线 + 采集新鲜度记账（自 on_tick 拆出的发布步）：
    /// 原始双通道帧在下混**前**入 ring（双麦原始通道只在此一用）；判向本身挪到
    /// onset 边沿整窗回看。空帧另账（欠载饿死信号），帧账在发布成功后 +1——记账
    /// = 真上了总线的帧数（发布失败即 exit，临死一帧不入账）。返回总线 mono 帧。
    pub(super) fn rr3_tick_pub(
        &mut self,
        raw: Rr3PcmStream,
        done: usize,
        bus: &BusHandle,
    ) -> Result<Rr3PcmStream, String> {
        rr3_ring_feed(self.doa_ring.as_mut(), &raw.samples);
        // 多通道下混 mono 入总线（R14）：双麦原始多通道只供 sense 内部（DoA 随批F2），
        // 总线承载恒 mono——brain 消费面零改动；单麦链路 channels=1 时零拷贝等价
        let pcm = rr3_pcm_downmix(raw);
        self.rr3_frame_book(bus, done);
        self.rr3_pcm_pub(bus, &pcm)?;
        Ok(pcm)
    }

    /// 采集新鲜度记账：空帧另账（欠载饿死信号），流序自增（单点续号）。
    fn rr3_frame_book(&mut self, bus: &BusHandle, done: usize) {
        if done == 0 {
            bus.counter_bump(RR3_PCM_EMPTY_FRAMES_KEY, 1);
        }
        self.seq += 1;
    }

    /// 帧发布 + 成帧计数（seq 已在调用方续号）。
    fn rr3_pcm_pub(&self, bus: &BusHandle, pcm: &Rr3PcmStream) -> Result<(), String> {
        bus.publish(
            capture::RR3_PCM_TOPIC,
            rpc::rr3_pcm_event_body(self.seq, pcm),
        )?;
        bus.counter_bump(RR3_PCM_FRAMES_KEY, 1);
        Ok(())
    }

    /// 自 pacing：补足本节拍剩余时间（sim read 即时返回的关键；抖动可接受——
    /// v0.0.1 的 pcm 流只供联调，时序对齐挂 M5）。
    fn rr3_pace(&self, now: Instant) {
        let spent = now.elapsed();
        if spent < self.tick {
            std::thread::sleep(self.tick - spent);
        }
    }

    /// VAD 边沿拍（B9 自 on_tick 拆出；本轮按沿分臂）：本函数只做门判与编排，
    /// 发布/判向/反射环各自成件。判据全量入参（门控/DoA/时序）不变。
    fn rr3_vad_beat(
        &mut self,
        pcm: &Rr3PcmStream,
        now: Instant,
        bus: &BusHandle,
    ) -> Result<(), String> {
        let Some(edge) = self.vad.rr3_vad_feed(&pcm.samples) else {
            return Ok(());
        };
        // 拍时刻入参（`now`）统一用作门判基准：本拍内不二次读时钟，
        // 门判/节点计划与 tick 同一条时间轴（espidf 档计划面不落，
        // 旧版此处另取 `Instant::now()` 让 `now` 在该档成死参）。
        let gate_open = self.tts_gate.rr3_tts_gate_open(now);
        // 播放期门控（B批 b2）：门开只弃 onset 发布（扬声器自声不再触发听声
        // 转头），VAD 状态机照喂不中断；offset 与门无关照发
        if matches!(edge, Rr3VadEdge::Onset { .. }) && gate_open {
            bus.counter_bump(RR3_VAD_ONSET_GATED_KEY, 1);
            eprintln!("播放期门控: onset 弃发布（TTS 回声中）");
            return Ok(());
        }
        let doa_deg = self.rr3_vad_publish(&edge, bus)?;
        match edge {
            Rr3VadEdge::Onset { .. } => self.rr3_vad_pan(doa_deg, gate_open),
            Rr3VadEdge::Offset { .. } => self.rr3_vad_stop(now, bus),
        }
        Ok(())
    }

    /// 边沿发布（两沿同址）：DoA 仅 onset 判（offset 静音无方向可判），判向窗 =
    /// ring 线性化快照（最近 window_ms 的原始立体声）交机制纯函数；单声道链路
    /// ring 不分配 → 恒 None，事件体不带 doa（与旧版等价）。onset 时 VAD 已证
    /// ≥min_speech_ms 连续有声帧，能量门对 onset 路径恒过。返回判向角供反射环。
    fn rr3_vad_publish(
        &mut self,
        edge: &Rr3VadEdge,
        bus: &BusHandle,
    ) -> Result<Option<i32>, String> {
        let doa = self.rr3_vad_doa(edge);
        bus.publish(
            vad::RR3_VAD_START_TOPIC,
            rpc::rr3_vad_event_body(self.seq, *edge, doa.map(|d| rpc::rr3_doa_field(&d))),
        )?;
        Ok(doa.map(|d| d.deg))
    }

    /// 判向段：仅 onset 有方向可判，判向窗 = ring 线性化快照（单声道链路恒 None）。
    fn rr3_vad_doa(&self, edge: &Rr3VadEdge) -> Option<Rr3DoaFix> {
        match (edge, self.doa_ring.as_ref()) {
            (Rr3VadEdge::Onset { .. }, Some(ring)) => {
                let snap = ring.rr3_ring_snapshot();
                capture::rr3_doa_fix(&snap, self.cfg.audio.channels, &self.doa_cfg)
            }
            _ => None,
        }
    }

    /// onset 反射环（B批 b2 起 onset 判向；2026-10-21 EMI 避让改判停后动作）：
    /// 实际发布后才换算存 pending（被门弃掉的 onset 不进 pending），判停才
    /// 实际下发——激磁尖峰不落采集窗。判向缺档自然无目标，机制面零特判。
    fn rr3_vad_pan(&mut self, doa_deg: Option<i32>, gate_open: bool) {
        let Some(deg) = doa_deg else { return };
        self.pending_pan = rr3_reflex_pan(
            self.cfg.reflex.enable,
            self.cfg.reflex.deadband_deg,
            deg,
            gate_open,
        );
    }

    /// offset 反射环：判停（语音采集完毕）下发 pending pan 并排 nod 手势。旁路
    /// 下发是宿主桥档行为（Rr3PeriphCmd，rr-hal backend 宿主档门）；espidf 档
    /// pending 自然落空，take 即弃，不进旁路（批N8 件三）。
    fn rr3_vad_stop(&mut self, now: Instant, bus: &BusHandle) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        self.rr3_pan_apply(now, bus);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            self.pending_pan.take();
            let _ = (now, bus);
        }
    }

    /// 判停动作：pan 下发 + 记账，随后按 config 时序排点头手势。
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn rr3_pan_apply(&mut self, now: Instant, bus: &BusHandle) {
        let Some(target) = self.pending_pan.take() else {
            return;
        };
        let Some(tx) = self.cmd_tx.as_ref() else {
            return;
        };
        tx.rr3_cmd_push(Rr3PeriphCmd::ServoPan(target));
        bus.counter_bump(RR3_REFLEX_PAN_CMDS_KEY, 1);
        eprintln!("反射环: 判停后下发 pan={target}°");
        // 点头示意（0.0.8 批）：pan 之后接一手势，时序量全在 config；
        // nod_deg=0 时计划为 None
        let nod = rr3_reflex_nod_plan(
            RR3_REFLEX_TILT_CENTER_DEG,
            self.cfg.reflex.nod_deg,
            self.cfg.reflex.nod_delay_ms,
            self.cfg.reflex.nod_hold_ms,
            now,
        );
        self.pending_nod = nod;
    }
}
