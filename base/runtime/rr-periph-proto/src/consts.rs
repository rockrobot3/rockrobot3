//! 帧面共享常量：task 位形、主题、magic、波特率缺省、宿主 env 名。
//!
//! 这些值此前在宿主 `rr-hal::esp32_uart` 与板侧 `board.rs` 各写一遍（"两端常量
//! 自证 + 注释锚定"），本 crate 收成单点；板侧生成物（topics_gen.rs）与
//! [`RR3_SERVO_TOPIC`] / [`RR3_FACE_TOPIC`] 的同值关系由固件的编译期断言兜底。

/// 板上外设执行面的 task 号（帧头 `dst` 门）：外设固件占首模块池位形 2。
///
/// 与 `rr-core::RR3_TASK_FIRST_MODULE` 同值——机制真值单点在 rr-core，本 crate
/// 不加依赖边（rr-core 会牵总线栈），本地锚定 + 单测防静默分叉。
pub const RR3_PERIPH_TASK: u64 = 2;

/// 宿主下行 task 号（帧头 `src`）：rrd 位形，与 `rr-core::RR3_TASK_RRD` 同值。
pub const RR3_TASK_HOST: u64 = 1;

/// 直连 trace id（桥后端不经 rrd 路由；与 `rr-core::RR3_TRACE_ID_DIRECT` 同值）。
pub const RR3_TRACE_DIRECT: u64 = 0;

/// 帧头 magic "1ERA" 的 LE 首字节（板侧双面分流窥视判据：行面首字节全为字母，
/// 与 0x31 无交集，分流无歧义）。
pub const RR3_MAGIC_FIRST: u8 = rr_ipc::RR3_MAGIC_LE[0];

/// 桥后端**定档**波特率：1.5M（2026-10-30 板固件 `UART2_BAUD_HZ` 定档）。
///
/// 板与宿主共用本常量——桥链是同一根 UART 的两端，缺省值不允许各写一份：
/// 板端定档后宿主缺省仍漂在 115200 会让整链静默失联（A8 误码观测的前件）。
/// `rr_ipc::RR3_UART_BAUD_DEFAULT` 仍是**通用**传输缺省（115200），桥面不得
/// 复用它。非宿主平台（xtensa 固件）不载 rr-ipc，本常量无平台门、无第二真值。
pub const RR3_BRIDGE_BAUD: u32 = 1_500_000;

/// 舵机点名主题（在册 request L1；payload 形状事实源
/// contracts/events/act-servo_command.schema.json）。
pub const RR3_SERVO_TOPIC: &str = "rr3-act/servo/command";

/// 表情点名主题（在册 request L1；payload 形状事实源
/// contracts/events/interact-face_expression.schema.json）。
pub const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";

/// ESP32 档设备路径 env 键（宿主侧应急覆盖口径）。
pub const RR3_HAL_PERIPH_UART_ENV: &str = "RR3_HAL_PERIPH_UART";

/// ESP32 档波特率 env 键（宿主侧；缺省即 [`RR3_BRIDGE_BAUD`]（1.5M），本键仅覆盖）。
pub const RR3_HAL_PERIPH_BAUD_ENV: &str = "RR3_HAL_PERIPH_BAUD";

#[cfg(test)]
mod tests {
    use super::*;

    /// 真值锚定：位形/直连 trace/缺省波特率与 rr-core、rr-ipc 逐值一致，
    /// magic 首字节与帧头 LE 线上字节一致（漂移即红）。
    #[test]
    fn consts_match_protocol_truth() {
        assert_eq!(RR3_TASK_HOST, 1, "宿主位形=rrd（rr-core::RR3_TASK_RRD）");
        assert_eq!(
            RR3_TRACE_DIRECT, 0,
            "直连 trace（rr-core::RR3_TRACE_ID_DIRECT）"
        );
        assert_eq!(RR3_PERIPH_TASK, 2, "板上执行面占首模块池位形");
        assert_eq!(RR3_MAGIC_FIRST, 0x31, "magic \"1ERA\" 的 LE 首字节");
        assert_eq!(RR3_MAGIC_FIRST, rr_ipc::RR3_MAGIC_LE[0]);
        // 桥面定档 1.5M（板固件 UART2_BAUD_HZ 同源）；通用传输缺省 115200
        // 仍归 rr-ipc——两者是不同域，不得再锚成相等。
        assert_eq!(RR3_BRIDGE_BAUD, 1_500_000, "桥面定档=板固件同值");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert_ne!(RR3_BRIDGE_BAUD, rr_ipc::RR3_UART_BAUD_DEFAULT);
        assert!(!RR3_SERVO_TOPIC.is_empty());
        assert!(!RR3_FACE_TOPIC.is_empty());
    }
}
