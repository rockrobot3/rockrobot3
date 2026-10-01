//! linux（ALSA）音频后端：Buildroot 根文件系统内的采集/回放（architecture.md §5、ADR-0007）。
//!
//! 编译门是双重的：`target_os = "linux"` **且** `alsa` feature（Cargo.toml 的目标门控依赖）。
//! 板上真实设备（ES8388 codec 经 ALSA 暴露）的回放/采集验证属板上收口事项，主机 CI
//! （macOS）不编译本文件——这里是"代码与配置就绪"，真机验证见 docs/status.md 登记。
//!
//! 实现要点：`open` 用 hw_params 固定 16bit 交错 S16 / 声道数 / 采样率（与
//! [`crate::Rr3AudioCfg`] 一一对应），之后 `readi`/`writei` 阻塞搬运。运行期
//! underrun/overrun（errno EPIPE = -32）走 `recover` 后重试一次——这是 ALSA 回放
//! 的常规存活手段；设备名取 `RR3_HAL_ALSA_DEVICE`（默认 `default`，见 backend.rs）。

use std::sync::Mutex;

use async_trait::async_trait;

use crate::{Rr3AudioCfg, Rr3AudioSink, Rr3AudioSource, Rr3HalError, Rr3PcmStream};

/// ALSA underrun/overrun 的 errno（EPIPE）
const EPIPE: i32 = 32;

fn rr3_alsa_device() -> String {
    std::env::var(crate::backend::RR3_HAL_ALSA_DEVICE_ENV)
        .unwrap_or_else(|_| crate::backend::RR3_HAL_ALSA_DEVICE_DEFAULT.to_string())
}

/// open 参数门禁：与 sim 后端同一套要求（16bit 采样且 rate/channels 非零）
fn rr3_alsa_cfg_check(cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
    if cfg.sample_rate_hz == 0 || cfg.channels == 0 || cfg.bit_depth != 16 {
        return Err(Rr3HalError::InvalidParam(format!(
            "alsa 后端要求 16bit 采样且 rate/channels 非零，收到 {cfg:?}"
        )));
    }
    Ok(())
}

fn rr3_alsa_open(
    cfg: Rr3AudioCfg,
    dir: alsa::Direction,
    device: &str,
) -> Result<alsa::pcm::PCM, Rr3HalError> {
    let pcm = alsa::pcm::PCM::new(device, dir, false)
        .map_err(|e| Rr3HalError::InvalidParam(format!("alsa 打开设备 {device:?} 失败: {e}")))?;
    {
        let hwp = alsa::pcm::HwParams::any(&pcm)
            .map_err(|e| Rr3HalError::InvalidParam(format!("alsa hw_params 起步失败: {e}")))?;
        hwp.set_access(alsa::pcm::Access::RWInterleaved)
            .map_err(|e| Rr3HalError::InvalidParam(format!("alsa 设 access 失败: {e}")))?;
        hwp.set_format(alsa::pcm::Format::s16())
            .map_err(|e| Rr3HalError::InvalidParam(format!("alsa 设 s16 格式失败: {e}")))?;
        hwp.set_channels(cfg.channels as u32).map_err(|e| {
            Rr3HalError::InvalidParam(format!("alsa 设声道 {} 失败: {e}", cfg.channels))
        })?;
        hwp.set_rate(cfg.sample_rate_hz, alsa::ValueOr::Nearest)
            .map_err(|e| {
                Rr3HalError::InvalidParam(format!("alsa 设采样率 {} 失败: {e}", cfg.sample_rate_hz))
            })?;
        pcm.hw_params(&hwp)
            .map_err(|e| Rr3HalError::InvalidParam(format!("alsa hw_params 提交失败: {e}")))?;
    }
    Ok(pcm)
}

/// 运行期 IO 错误：先按 errno 尝试 recover（underrun/overrun）， recover 不了才上抛。
/// 返回 Ok(true) 表示已恢复、调用方应重试本次搬运。
fn rr3_alsa_io_recover(pcm: &alsa::pcm::PCM, e: &alsa::Error) -> bool {
    if let Some(err) = e.errno() {
        if err == -EPIPE || err == EPIPE {
            return pcm.recover(err, true).is_ok();
        }
    }
    false
}

struct Rr3AlsaOpen {
    cfg: Rr3AudioCfg,
    pcm: alsa::pcm::PCM,
}

/// linux 音频源（ALSACapture，上行拾音）
pub struct Rr3AlsaAudioSource {
    device: String,
    open: Mutex<Option<Rr3AlsaOpen>>,
}

impl Rr3AlsaAudioSource {
    /// 设备名取 `RR3_HAL_ALSA_DEVICE`（未设则 `default`），见 backend.rs
    pub fn rr3_alsa_source_default() -> Self {
        Self {
            device: rr3_alsa_device(),
            open: Mutex::new(None),
        }
    }
}

#[async_trait]
impl Rr3AudioSource for Rr3AlsaAudioSource {
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        rr3_alsa_cfg_check(cfg)?;
        let pcm = rr3_alsa_open(cfg, alsa::Direction::Capture, &self.device)?;
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = Some(Rr3AlsaOpen { cfg, pcm });
        Ok(())
    }

    async fn read(&self, frames: usize) -> Result<Rr3PcmStream, Rr3HalError> {
        let mut guard = self.open.lock().unwrap_or_else(|p| p.into_inner());
        let Some(open) = guard.as_mut() else {
            return Err(Rr3HalError::InvalidParam("alsa 源 read 前必须 open".into()));
        };
        let ch = open.cfg.channels as usize;
        let mut buf = vec![0i16; frames * ch];
        {
            let mut io = open.pcm.io_i16().map_err(|e| Rr3HalError::Busy)?;
            let mut done = 0usize;
            while done < frames {
                match io.readi(&mut buf[done * ch..]) {
                    Ok(n) if n > 0 => done += n,
                    Ok(_) => continue,
                    Err(e) if rr3_alsa_io_recover(&open.pcm, &e) => continue,
                    Err(e) => return Err(Rr3HalError::Busy),
                }
            }
        }
        Ok(Rr3PcmStream {
            cfg: open.cfg,
            samples: buf,
        })
    }
}

/// linux 音频播放面（ALSA Playback，下行出话）
pub struct Rr3AlsaAudioSink {
    device: String,
    open: Mutex<Option<Rr3AlsaOpen>>,
}

impl Rr3AlsaAudioSink {
    /// 设备名取 `RR3_HAL_ALSA_DEVICE`（未设则 `default`），见 backend.rs
    pub fn rr3_alsa_sink_default() -> Self {
        Self {
            device: rr3_alsa_device(),
            open: Mutex::new(None),
        }
    }
}

#[async_trait]
impl Rr3AudioSink for Rr3AlsaAudioSink {
    async fn open(&self, cfg: Rr3AudioCfg) -> Result<(), Rr3HalError> {
        rr3_alsa_cfg_check(cfg)?;
        let pcm = rr3_alsa_open(cfg, alsa::Direction::Playback, &self.device)?;
        *self.open.lock().unwrap_or_else(|p| p.into_inner()) = Some(Rr3AlsaOpen { cfg, pcm });
        Ok(())
    }

    async fn write(&self, pcm_in: Rr3PcmStream) -> Result<(), Rr3HalError> {
        let mut guard = self.open.lock().unwrap_or_else(|p| p.into_inner());
        let Some(open) = guard.as_mut() else {
            return Err(Rr3HalError::InvalidParam(
                "alsa 扬声器 write 前必须 open".into(),
            ));
        };
        if pcm_in.cfg != open.cfg {
            return Err(Rr3HalError::InvalidParam(format!(
                "alsa 扬声器已按 {:?} 打开，write 的配置是 {:?}",
                open.cfg, pcm_in.cfg
            )));
        }
        let ch = open.cfg.channels as usize;
        let total_frames = pcm_in.samples.len() / ch;
        let mut io = open.pcm.io_i16().map_err(|e| Rr3HalError::Busy)?;
        let mut done = 0usize;
        while done < total_frames {
            match io.writei(&pcm_in.samples[done * ch..]) {
                Ok(n) if n > 0 => done += n,
                Ok(_) => continue,
                Err(e) if rr3_alsa_io_recover(&open.pcm, &e) => continue,
                Err(_) => return Err(Rr3HalError::Busy),
            }
        }
        Ok(())
    }
}
