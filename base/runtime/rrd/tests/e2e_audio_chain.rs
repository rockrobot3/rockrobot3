#![cfg(unix)]
// SPDX-License-Identifier: Apache-2.0
//! M3 音频链三真进程端到端：rrd + rr3-sense(sim) + rr3-interact(sim) 全是**真二进制**。
//!
//! 前面的 e2e 用的都是 sim_peer 夹具（行为脚本）；这一组把仓里两个真模块拉进同一条总线，
//! 钉的是音频链的整体形状：
//! 1. boot spawn → 注册 → running（supervisor 的进程语义对真 manifest 成立）；
//! 2. `rr3-sense/audio/vad_start` 事件上总线（sim 正弦必过 VAD 起音阈值，恰一次 onset）；
//! 3. `rr3-sense/audio/pcm` stream 有序——seq 从旁观连接看到的任意一点起逐帧 +1
//!    （旁观者看到的是流的**连续后缀**，起点不是 1 是正形，起点错序才是 bug）；
//! 4. `rr3.call` 回路穿过真进程：旁观连接点名 `rr3-interact/tts/text`（本链无
//!    brain、无 tts_pcm 流——`pcm_bytes` 对账必拒，MC-1：无流不静默，拒因折
//!    result.error）与 `rr3-interact/face/expression`（应答体 expression/previous
//!    由 interact 进程里的表情机真实算出）——模块侧 rr3_reply_call 的 trace_id
//!    语义在这条线上被端到端复证。
//!
//! 模块二进制**由测试现场 cargo build**（独立工程，`CARGO_BIN_EXE` 够不着它们）：
//! 增量编译下缓存命中，代价只在首次全量。
//!
//! 阶段体在 [`phases`]（自本文件按阶段抽出，判据不变）：主用例只保留顺序编排。

mod support;

#[path = "e2e_audio_chain/phases.rs"]
mod phases;

use std::path::{Path, PathBuf};

use support::{rr3_build_module_bin, rr3_module_manifest_text, SimManifest};

/// --manifest 直驱的落盘薄封装：渲染文本来自 support 的 [`rr3_module_manifest_text`]，
/// 这里只负责 pid+tag 唯一的 /tmp 路径（Global Constraints 8）。
fn rr3_module_manifest(tag: &str, name: &str, bin: &Path) -> SimManifest {
    let rendered = rr3_module_manifest_text(name, bin);
    let path = PathBuf::from(format!(
        "/tmp/rr3-m3-module-{}-{tag}-{name}.toml",
        std::process::id()
    ));
    std::fs::write(&path, rendered)
        .unwrap_or_else(|e| panic!("写夹具 manifest {} 失败: {e}", path.display()));
    SimManifest { path }
}

#[test]
fn audio_chain_sense_to_interact_end_to_end() {
    let sense_bin = rr3_build_module_bin("rr3-sense");
    let interact_bin = rr3_build_module_bin("rr3-interact");
    let (m_sense, m_inter, cfg, inter_cfg) = phases::stage(&sense_bin, &interact_bin);
    // ① boot 两模块 + 旁观连接先就位（onset 是一次性边沿）
    let mut rrd = phases::start(&m_sense, &m_inter, &cfg, &inter_cfg);
    let (mut obs, obs_task, sense_task, _inter_task) = phases::register(&mut rrd);
    // ②–③ vad 边沿与 pcm 流的连续后缀
    phases::vad_edge(&mut obs, sense_task);
    let last = phases::pcm_ordered(&mut obs);
    // ④ rr3.call 回路：TTS 对账拒、表情切换、坏参数折 error、流仍活
    phases::tts_reject(&mut obs, obs_task);
    phases::face_expr(&mut obs, obs_task);
    phases::bad_args(&mut obs, obs_task);
    phases::stream_alive(&mut obs, last);
    // ⑤ 采集新鲜度记账
    phases::counters(&rrd);
}

/// 双麦判向 e2e 的 config 守卫：channels=2 的 sense 配置落 /tmp（pid 唯一路），Drop 清理
/// ——断言失败会 panic，自己删的那一行跑不到（SimManifest 同一条教训）。
struct CfgGuard {
    path: PathBuf,
}

impl Drop for CfgGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 双麦 config 夹具：与仓内 `modules/rr3-sense/config.toml` 同形状，唯 channels=2——
/// DoA 注入只在 ch_n>=2 时被 sim 源采纳（backend.rs 同一纪律），单麦链路无方向语义。
/// VAD 在下混后的 mono 上判，阈值与仓内 config 同值。
const DOA_CONFIG: &str = r#"
[rr3-sense]
log_level = "info"

[rr3-sense.audio]
sample_rate = 16000
channels = 2
frame_ms = 20

[rr3-sense.vad]
rms_start = 1500        # 起音阈值
silence_tail_ms = 700   # 判停尾静音
min_speech_ms = 200

[rr3-sense.doa]
bin_threshold_deg = 30  # DoA 三档边界（S-5 注入面形状 pin）
window_ms = 1000        # 判向窗长（B批 b3b TDOA：onset 时 ring 回看窗整窗互相关）
window_rms_min = 800    # 窗能量门（B批 b3b 注入面判据显式化：与代码缺省同值落档）
mic_baseline_mm = 150   # 双麦间距（b3a 实测安装同源；sim 注入基线常量与此同源，
                        # 改任一侧必须同批改另一侧——e2e ±2° 容差即两处一致判据）
"#;

#[test]
fn audio_chain_doa_stereo_env_injection_end_to_end() {
    let sense_bin = rr3_build_module_bin("rr3-sense");
    let (m_sense, cfg, cfg_path) = phases::doa_stage(&sense_bin);
    let _cfg = CfgGuard { path: cfg_path };
    // ① boot：DOA_DEG=-60（左）经 rrd 进程继承传给真 rr3-sense
    let mut rrd = phases::doa_start(&m_sense, &cfg);
    let (mut obs, sense_task) = phases::doa_register(&mut rrd);
    // ② onset 带 doa（sense 内部反演 + ±30° 分档）；③ pcm 总线恒 mono
    phases::doa_onset(&mut obs, sense_task);
    phases::doa_mono(&mut obs);
}
