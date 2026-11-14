//! 后端默认值、白名单裁决与能力面覆盖键，以及生效绑定获取。

use super::*;

/// 平台默认后端：linux + `alsa` feature 编进来了就是 `linux`，否则 `sim`。
/// macOS 平台缺省同为 `sim`（真麦走显式 `mac`——CI/无头不真开设备，见 [`RR3_BACKEND_MAC`]）。
pub const fn rr3_backend_default() -> &'static str {
    if cfg!(all(target_os = "linux", feature = "alsa")) {
        RR3_BACKEND_LINUX
    } else {
        RR3_BACKEND_SIM
    }
}

/// 解析生效的后端名：`RR3_HAL_BACKEND` 显式取值优先（取值不认则回落平台默认并返回
/// `Err` 点名），否则平台默认。`linux` 表示"linux 平台在位"（批F5a 订正：原按 alsa
/// feature 门判可用性，与舵机/GPIO 等无 alsa 依赖的 linux 后端错位——真件档板上
/// 显式 `RR3_HAL_BACKEND=linux` 会被误落 placeholder，act 探活 exit(1) 起不来）；
/// 各能力面是否编进了对应 linux 后端由各选择函数自判（音频按 `alsa` feature，
/// 舵机按 sysfs PWM 模块编译门），选了 `linux` 但该能力后端没编进 → 选择函数落
/// `placeholder`（使用时以 `Unsupported` 显式回错，不静默换 sim）。
pub fn rr3_backend_resolve() -> Result<&'static str, String> {
    let Some(v) = std::env::var_os(RR3_HAL_BACKEND_ENV) else {
        return Ok(rr3_backend_default());
    };
    rr3_backend_parse(RR3_HAL_BACKEND_ENV, &v.to_string_lossy())
}

/// 后端名白名单裁决（纯函数，单测可钉；`key` 只用于错误点名）。全局键与
/// 音频能力面覆盖键（[`RR3_HAL_AUDIO_BACKEND_ENV`]）共用同一条白名单与
/// 平台门语义——两键不各写一套，防静默分叉。
pub(super) fn rr3_backend_parse(key: &str, v: &str) -> Result<&'static str, String> {
    let known: [(&'static str, bool); 5] = [
        (RR3_BACKEND_SIM, true),
        (RR3_BACKEND_PLACEHOLDER, true),
        (RR3_BACKEND_ESP32, true),
        (RR3_BACKEND_LINUX, cfg!(target_os = "linux")),
        (RR3_BACKEND_MAC, cfg!(target_os = "macos")),
    ];
    if let Some((backend, native)) = known.iter().find(|(b, _)| *b == v) {
        return Ok(rr3_backend_os(backend, *native));
    }
    Err(format!(
        "环境变量 {key} 取值不认: {v}（允许 sim/linux/esp32/mac/placeholder）"
    ))
}

/// 平台门：本平台才认该后端，否则落占位（使用时回 Unsupported，不静默降级）。
fn rr3_backend_os(backend: &'static str, native: bool) -> &'static str {
    if native {
        backend
    } else {
        RR3_BACKEND_PLACEHOLDER
    }
}

/// 音频采集面生效后端：能力面覆盖键（[`RR3_HAL_AUDIO_BACKEND_ENV`]，未设/空白
/// 视为未设）优先，否则回落全局 [`rr3_backend_resolve`]。只此一处读覆盖键。
pub fn rr3_audio_backend() -> Result<&'static str, String> {
    rr3_backend_override(RR3_HAL_AUDIO_BACKEND_ENV)?.map_or_else(rr3_backend_resolve, Ok)
}

/// 舵机面生效后端（P0-4）：能力面覆盖键 [`RR3_HAL_SERVO_BACKEND_ENV`] 优先，否则
/// 回落全局。组合档舵机在 ESP32 板上时设 `esp32`，host-only 档不设回落全局。
pub fn rr3_servo_backend() -> Result<&'static str, String> {
    rr3_backend_override(RR3_HAL_SERVO_BACKEND_ENV)?.map_or_else(rr3_backend_resolve, Ok)
}

/// 显示面生效后端（P0-4）：能力面覆盖键 [`RR3_HAL_DISPLAY_BACKEND_ENV`] 优先，否则
/// 回落全局。组合档 OLED/点阵在 ESP32 板上时设 `esp32`，host-only 档不设回落全局。
pub fn rr3_display_backend() -> Result<&'static str, String> {
    rr3_backend_override(RR3_HAL_DISPLAY_BACKEND_ENV)?.map_or_else(rr3_backend_resolve, Ok)
}

/// 能力面覆盖键取值：未设/空白视为未设（`None` 交调用方回落全局）。
fn rr3_backend_override(env: &str) -> Result<Option<&'static str>, String> {
    let Some(v) = std::env::var_os(env) else {
        return Ok(None);
    };
    let s = v.to_string_lossy();
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    rr3_backend_parse(env, s).map(Some)
}

/// 描述符失败点名文案（R3-D3/0.0.5 批J G5）：裁决纯函数独立可单测。
/// 生产消费方只有 cfg(linux) 的 [`rr3_bindings_eff_log`]，故生产门
/// 收 linux；test 门留给文案形状单测（macOS 主机也要跑）。
#[cfg(any(target_os = "linux", test))]
pub(super) fn rr3_desc_fail_note(facet: &str, err: &str) -> String {
    format!("rr-hal: 描述符解析失败，{facet}落占位（使用时以 Unsupported 回错）: {err}")
}

/// 生效绑定获取（R3-D3/批J G5 收口）：各选择函数 linux 分支此前以 `.ok()` 取
/// 绑定，描述符解析失败（[hardware] 键名拼错/取值不认/应急 env 无效）被静默
/// 吞掉、只落占位，板上排障无从下手。改为失败当场 `eprintln!` 点名（rrd 系统
/// 实例 stderr 进 journal），随后照旧落占位——失败语义不变，只补"错误当场可见"。
/// 仅 linux 分支调用（非 linux 平台无人消费），故 cfg 门收进本平台。
#[cfg(target_os = "linux")]
pub(super) fn rr3_bindings_eff_log(facet: &str) -> Option<crate::descriptor::Rr3HalBindings> {
    match crate::descriptor::rr3_bindings_eff() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("{}", rr3_desc_fail_note(facet, &e));
            None
        }
    }
}
