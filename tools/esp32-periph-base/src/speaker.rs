//! I2S TX 功放/扬声器上板验收（W4-1）：I2S1 单声道提示音，默认固件不启用。
//!
//! 只在 `--features spk-tone` 时编译与启动；默认外设底座固件零行为变化。
//! 硬件口径（2026-11-19 用户确认器件在位）：
//! - I2S 功放 + 8 Ω 扬声器为被动 I2S 从设备，三线 BCLK/WS/DIN + 5V 供电 + 共地；
//!   同一时间只接一块板，3.3V 逻辑电平，禁接 5V 逻辑。
//! - classic：BCLK=GPIO25、WS=GPIO16、DIN=GPIO17（16/17 自 UART2 迁 18/19 后空闲；
//!   25 自麦克风迁 27 后空闲）。
//! - S3 N16R8：BCLK=GPIO16、WS=GPIO17、DIN=GPIO18（均在 26–37 Flash/PSRAM 区外）。
//!
//! 机制与策略分离：本文件只做"驱动装配 + 正弦样本生成 + 一次写流"；
//! 采样率/音高/时长/幅度是验收参数，真实播放路径（云端 PCM）在 W4-2 接入。

use std::thread;

use esp_idf_hal::gpio::{AnyIOPin, InputPin, OutputPin};
use esp_idf_hal::i2s::config::{DataBitWidth, StdConfig};
use esp_idf_hal::i2s::{I2sDriver, I2sTx, I2S1};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_sys::TickType_t;

/// 提示音采样率（与语音链路一致的 16 kHz）。
const SPK_SR_HZ: u32 = 16_000;
/// 提示音频率与时长。
const SPK_TONE_HZ: u32 = 440;
const SPK_TONE_MS: u32 = 200;
/// 幅度（满幅 1/4，避免功放削顶与上电爆音）。
const SPK_AMP_Q15: i32 = 8_192;
/// 线程栈（与 mic/控制面同口径的 16KB 运行下限）。
const SPK_STACK: usize = 16_384;
/// I2S 写阻塞上限（portMAX_DELAY 同义）。
const SPK_TIMEOUT: TickType_t = u32::MAX;

/// 起一条提示音线程；装配失败只点名不 panic（板其余功能照常）。
pub(crate) fn spawn_tone(
    i2s1: impl Peripheral<P = I2S1> + Send + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
    dout: impl Peripheral<P = impl OutputPin> + Send + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + Send + 'static,
) {
    let spawned = thread::Builder::new()
        .name("spk-tone".into())
        .stack_size(SPK_STACK)
        .spawn(move || spk_run(i2s1, bclk, dout, ws));
    if let Err(e) = spawned {
        println!("[spk] 起 spk-tone 线程失败: {e:?}");
    }
}

/// 线程主体：I2S1 TX 装配 → 两次提示音 → 退出。
fn spk_run(
    i2s1: impl Peripheral<P = I2S1> + 'static,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
    dout: impl Peripheral<P = impl OutputPin> + 'static,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + 'static,
) {
    let cfg = StdConfig::philips(SPK_SR_HZ, DataBitWidth::Bits16);
    let opened: Result<I2sDriver<'static, I2sTx>, _> =
        I2sDriver::new_std_tx(i2s1, &cfg, bclk, dout, None::<AnyIOPin>, ws);
    let Ok(mut tx) =
        opened.map_err(|e| println!("[spk] I2S1 TX 驱动安装失败: {e:?}（查功放三线与供电/共地）"))
    else {
        return;
    };
    if tx.tx_enable().is_err() {
        println!("[spk] I2S1 tx_enable 失败（tx 通道未启动）");
        return;
    }
    let pcm = tone_pcm();
    println!(
        "[spk] I2S1 TX ✓ {}Hz {}ms pcm={}B（功放/扬声器上板验收，W4-1）",
        SPK_TONE_HZ,
        SPK_TONE_MS,
        pcm.len()
    );
    for round in 1..=2 {
        if let Err(e) = tx.write_all(&pcm, SPK_TIMEOUT) {
            println!("[spk] 第 {round}/2 次写入失败: {e:?}");
            return;
        }
        println!("[spk] 第 {round}/2 次写入完成");
        let silence = vec![0u8; (SPK_SR_HZ as usize / 10) * 4];
        if let Err(e) = tx.write_all(&silence, SPK_TIMEOUT) {
            println!("[spk] 间隔静音写入失败: {e:?}");
            return;
        }
    }
    println!("[spk] 提示音结束（若无声：查功放 SD 模式/增益脚、供电与扬声器）");
}

/// 200ms 440Hz 正弦，双声道交错 s16le（I2S std 缺省 stereo 槽位）。
fn tone_pcm() -> Vec<u8> {
    let n = (SPK_SR_HZ * SPK_TONE_MS / 1000) as usize;
    let mut out = Vec::with_capacity(n * 4);
    for i in 0..n {
        let phase = (i as f32) * (SPK_TONE_HZ as f32) * std::f32::consts::TAU / (SPK_SR_HZ as f32);
        let v = (phase.sin() * SPK_AMP_Q15 as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_pcm_has_expected_length_and_no_clipping() {
        let pcm = tone_pcm();
        assert_eq!(pcm.len(), (SPK_SR_HZ * SPK_TONE_MS / 1000) as usize * 4);
        let peak = pcm
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs())
            .max()
            .unwrap_or(0);
        assert!(peak <= SPK_AMP_Q15 as u16, "峰值 {peak} 超过幅度");
        assert!(peak > 0, "不得为全静音");
    }
}
