//! 音频输入/输出面选择（ALSA、macOS 真件、sim 与 ESP32 旁路）。

use crate::{Rr3AudioSink, Rr3AudioSource, Rr3UnavailableSource};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::acts::rr3_periph_uart_path;
#[cfg(all(target_os = "linux", feature = "alsa"))]
use super::selector::rr3_bindings_eff_log;
use super::*;

fn rr3_hal_sim_wave() -> crate::sim::Rr3SimWave {
    std::env::var(RR3_HAL_SIM_WAVE_ENV)
        .ok()
        .and_then(|v| crate::sim::Rr3SimWave::rr3_sim_wave_parse(&v))
        .unwrap_or(crate::sim::Rr3SimWave::Sine)
}

/// linux/alsa 回放面：生效绑定给设备名，缺键返回 `None` 交调用方落占位。
#[cfg(all(target_os = "linux", feature = "alsa"))]
fn rr3_sink_linux() -> Option<(Box<dyn Rr3AudioSink>, &'static str)> {
    rr3_bindings_eff_log("音频回放").and_then(|b| {
        b.alsa_device.map(|dev| {
            (
                Box::new(crate::linux::Rr3AlsaAudioSink::rr3_alsa_sink_new(&dev))
                    as Box<dyn Rr3AudioSink>,
                RR3_BACKEND_LINUX,
            )
        })
    })
}

/// macOS 回放面（显式 `mac` 才进）。
#[cfg(target_os = "macos")]
fn rr3_sink_mac() -> (Box<dyn Rr3AudioSink>, &'static str) {
    (
        Box::new(crate::macaudio::Rr3MacAudioSink::rr3_mac_sink_new()) as Box<dyn Rr3AudioSink>,
        RR3_BACKEND_MAC,
    )
}

/// 按环境选音频源。解析失败（环境变量取值不认）或显式 `placeholder` 时返回
/// 占位后端——它会在使用处以 `Unsupported` 显式回错，**不静默换后端**。
/// linux 档的设备名来自生效绑定（[`crate::descriptor::rr3_bindings_eff`]：
/// 描述符为底、env 应急覆盖）；键缺失/合成失败 → 占位显式回错（fail-closed，批D④）。
pub fn rr3_audio_source() -> (Box<dyn Rr3AudioSource>, &'static str) {
    let (src, _, backend) = rr3_audio_source_cmd();
    (src, backend)
}

/// 音频源 + 板外设命令旁路产面（B 批 b1）：esp32 档返 `Some`——flock 单消费者
/// 纪律下板外设命令下行唯一通路 = 持 conn 的音频源旁路（esp32audio.rs）；
/// 其余档 `None`（无旁路面，消费方按 None fail-closed 关闭反射环）。
pub fn rr3_audio_source_cmd() -> (
    Box<dyn Rr3AudioSource>,
    Option<Rr3AudioCmdProducer>,
    &'static str,
) {
    // 采集面走能力面覆盖键（[`RR3_HAL_AUDIO_BACKEND_ENV`]）：组合形态下音频源
    // 独占 UART，舵机/显示面不能同吃 esp32 档（见该键文档）。
    match rr3_audio_backend().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        RR3_BACKEND_LINUX if cfg!(all(target_os = "linux", feature = "alsa")) => {
            #[cfg(all(target_os = "linux", feature = "alsa"))]
            return rr3_bindings_eff_log("音频采集")
                .and_then(|b| {
                    b.alsa_device.map(|dev| {
                        (
                            Box::new(crate::linux::Rr3AlsaAudioSource::rr3_alsa_source_new(&dev))
                                as Box<dyn Rr3AudioSource>,
                            None,
                            RR3_BACKEND_LINUX,
                        )
                    })
                })
                .unwrap_or((
                    Box::new(Rr3UnavailableSource),
                    None,
                    RR3_BACKEND_PLACEHOLDER,
                ));
            #[cfg(not(all(target_os = "linux", feature = "alsa")))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        // 显式 linux 但 alsa 后端没编进（无 alsa 的板上构建）：占位显式回错，
        // 不静默落 sim（resolve 订正为平台门后此分支结构上可达）
        RR3_BACKEND_LINUX => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
        // macOS 真麦（显式 mac 才进；缺省仍 sim——CI/无头不真开设备）
        RR3_BACKEND_MAC if cfg!(target_os = "macos") => {
            #[cfg(target_os = "macos")]
            return (
                Box::new(crate::macaudio::Rr3MacAudioSource::rr3_mac_source_new())
                    as Box<dyn Rr3AudioSource>,
                None,
                RR3_BACKEND_MAC,
            );
            #[cfg(not(target_os = "macos"))]
            unreachable!("上面 guard 已保证只在本分支编译")
        }
        // 显式 mac 但平台不是 macOS：占位显式回错，不静默落 sim
        RR3_BACKEND_MAC => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
        // ESP32 板面双麦流（A 批 c3，esp32audio.rs）：设备/波特来自应急 env
        // （与舵机/显示面同键）；缺 env → 点名 + placeholder（fail-closed，
        // 不猜设备不静默换 sim）。
        // 宿主档门（批N8 件三）：同 [`rr3_hal_servo`] 的 esp32 臂。
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        RR3_BACKEND_ESP32 => match rr3_periph_uart_path() {
            Some(p) => {
                let baud = std::env::var(crate::esp32_uart::RR3_HAL_PERIPH_BAUD_ENV)
                    .ok()
                    .and_then(|v| crate::esp32_uart::rr3_baud_parse(&v))
                    .unwrap_or(crate::esp32_uart::RR3_BRIDGE_BAUD);
                let (src, producer) =
                    crate::esp32audio::Rr3Esp32UartAudioSource::rr3_esp32_source_cmd(
                        std::path::PathBuf::from(p),
                        baud,
                    );
                (
                    Box::new(src) as Box<dyn Rr3AudioSource>,
                    Some(producer),
                    RR3_BACKEND_ESP32,
                )
            }
            None => {
                eprintln!(
                        "rr-hal: esp32 档缺 UART 路径（装配单 [hardware].periph_uart 或 env {}），音频采集面落占位（使用时以 Unsupported 回错）",
                        crate::esp32_uart::RR3_HAL_PERIPH_UART_ENV
                    );
                (
                    Box::new(Rr3UnavailableSource),
                    None,
                    RR3_BACKEND_PLACEHOLDER,
                )
            }
        },
        RR3_BACKEND_PLACEHOLDER => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
        RR3_BACKEND_SIM => {
            let doa = std::env::var(RR3_HAL_SIM_DOA_DEG_ENV)
                .ok()
                .and_then(|v| rr3_sim_doa_parse(&v))
                .and_then(crate::sim::Rr3SimDoa::rr3_sim_doa_new);
            (
                Box::new(crate::sim::Rr3SimAudioSource::rr3_sim_source_doa(
                    rr3_hal_sim_wave(),
                    doa,
                )),
                None,
                RR3_BACKEND_SIM,
            )
        }
        // `&str` 模式编译器裁定无法穷举（E0004）：白名单收敛在
        // [`rr3_backend_resolve`] 单点，此臂不可达；防御性落占位
        // （使用时 Unsupported 显式回错），不静默换 sim。
        _ => (
            Box::new(Rr3UnavailableSource),
            None,
            RR3_BACKEND_PLACEHOLDER,
        ),
    }
}

/// 按环境选音频播放面（与 [`rr3_audio_source`] 同一套规则）
pub fn rr3_hal_audio_sink() -> (Box<dyn Rr3AudioSink>, &'static str) {
    match rr3_backend_resolve().unwrap_or(RR3_BACKEND_PLACEHOLDER) {
        // linux/alsa 编进 → 真件；显式 linux 但 alsa 没编进 → 占位显式回错，
        // 不静默落 sim。
        RR3_BACKEND_LINUX => {
            #[cfg(all(target_os = "linux", feature = "alsa"))]
            {
                rr3_sink_linux()
                    .unwrap_or((Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER))
            }
            #[cfg(not(all(target_os = "linux", feature = "alsa")))]
            {
                (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
            }
        }
        // macOS 真扬声器（显式 mac 才进；缺省仍 sim——CI/无头不真开设备）；
        // 显式 mac 但平台不是 macOS：占位显式回错，不静默落 sim。
        RR3_BACKEND_MAC => {
            #[cfg(target_os = "macos")]
            {
                rr3_sink_mac()
            }
            #[cfg(not(target_os = "macos"))]
            {
                (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
            }
        }
        // 板无扬声器（A 批 c3）：esp32 档回放面显式落占位——TTS 回放恒宿主
        // （mac AudioQueue / sim 尾窗），fail-closed 不静默换。
        RR3_BACKEND_ESP32 | RR3_BACKEND_PLACEHOLDER => {
            (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER)
        }
        RR3_BACKEND_SIM => (
            Box::new(crate::sim::Rr3SimAudioSink::rr3_sim_sink_new(16000 * 10)),
            RR3_BACKEND_SIM,
        ),
        // 同 audio_source：白名单收敛在 resolve 单点，此臂不可达；防御性落占位
        _ => (Box::new(Rr3UnavailableSource), RR3_BACKEND_PLACEHOLDER),
    }
}
