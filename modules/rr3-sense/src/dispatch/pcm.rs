//! 采集帧缓冲池与流级单声道化。

use rr_hal::Rr3PcmStream;
use rr_hal_api::Rr3AudioCfg;

use crate::capture;

/// 采集帧缓冲池：把"每拍新分配一块 Vec"改成"稳态复用同一块"。
///
/// `take` 出的缓冲随本拍 `Rr3PcmStream` 走完读帧→发布→VAD，拍尾 `put` 归还；
/// 容量够下一拍就继续用（mono 链路稳态零分配），不够就丢弃由下次 `take`
/// 重新分配。语义与逐拍 `vec![0; need]` 等价：每拍拿到的都是全零、长度
/// `need` 的可写缓冲。
#[derive(Default)]
pub(super) struct Rr3FrameBuf {
    spare: Vec<i16>,
}

impl Rr3FrameBuf {
    /// 取一块长度恰为 `need` 的全零缓冲（池里有富余容量的块就不分配）。
    pub(super) fn rr3_take(&mut self, need: usize) -> Vec<i16> {
        let mut buf = std::mem::take(&mut self.spare);
        buf.clear();
        buf.resize(need, 0);
        buf
    }

    /// 归还本拍缓冲：容量够下一拍才留下（不足不占坑，由下次 take 重分配）。
    pub(super) fn rr3_put(&mut self, samples: Vec<i16>, need: usize) {
        if samples.capacity() >= need {
            self.spare = samples;
        }
    }
}

/// 流级单声道化（R14 + 2026-10-31 实机订正）：`cfg.channels` 归 1、rate/bit_depth
/// 不动、samples 收缩为**能量最高通道**（不平均——双麦平均下混实测梳状滤波，
/// ASR 质量显著劣于单通道）；`channels <= 1` 时原样返回（零改动）。自 capture/
/// mech.rs 迁入（0.0.5 批I 深挖收口：`Rr3PcmStream` 是承载层数据类型，件面只留
/// 纯切片机制——C7 白名单外件面不得引用 rr_hal 具体类型）；纯切片选道
/// [`capture::rr3_pcm_best_channel`] 仍在件内。
pub(super) fn rr3_pcm_downmix(pcm: Rr3PcmStream) -> Rr3PcmStream {
    if pcm.cfg.channels <= 1 {
        return pcm;
    }
    Rr3PcmStream {
        cfg: Rr3AudioCfg {
            channels: 1,
            ..pcm.cfg
        },
        samples: capture::rr3_pcm_best_channel(&pcm.samples, pcm.cfg.channels),
    }
}
