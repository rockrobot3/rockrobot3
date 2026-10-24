//! rr-periph-proto —— 板-宿主外设帧面协议共享层（ADR-0024）。
//!
//! ## 为什么单独成 crate
//!
//! 外设帧面（128B rr-ipc 头 + JSON-RPC 信封）此前由两端各自维护一份：宿主
//! `rr-hal::esp32_uart` / `rr-hal::esp32audio`，板侧 `esp32-periph-base::frame`。
//! 同一套 task 位形、主题串、信封形状与应答门序抄两遍 = 漂移靠人记得同步。
//! 本 crate 把**协议机制**收拢成单点真值，两端只留**平台供给面**（宿主：串口
//! 开口/连接盒；板侧：UART/I2C/PWM/ADC 端口实现）。
//!
//! ## 边界（依赖方向）
//!
//! 依赖仅 `rr-ipc`（字节契约、帧缓冲、body 前缀）与 `serde_json`（信封体），
//! **零 esp-idf 依赖**：宿主 `rr-hal`（linux/macos 档）与 xtensa 固件
//! （`tools/esp32-periph-base`）双向引用本 crate，任何一侧的平台依赖都会把
//! 另一侧拖进不兼容的构建面。
//!
//! ## 复用面（树莓派等后端）
//!
//! 协议层与"哪个平台实现 UART/I2C/PWM"解耦：新平台后端只需实现自己的端口
//! 层并复用本 crate 的组帧/解析，不必再抄一遍帧面。

pub mod cmd;
pub mod consts;
pub mod host_call;
pub mod request;
pub mod response;

pub use cmd::{rr3_board_frame_classify, rr3_esp32_cmd_frame, Rr3BoardFrame, Rr3PeriphCmd};
pub use consts::{
    RR3_BRIDGE_BAUD, RR3_FACE_TOPIC, RR3_HAL_PERIPH_BAUD_ENV, RR3_HAL_PERIPH_UART_ENV,
    RR3_MAGIC_FIRST, RR3_PERIPH_TASK, RR3_SERVO_TOPIC, RR3_TASK_HOST, RR3_TRACE_DIRECT,
};
pub use host_call::{
    rr3_bridge_call_frame, rr3_bridge_request_body_params, rr3_bridge_request_frame,
};
pub use request::{rr3_periph_request_parse, Rr3PeriphRequest};
pub use response::{rr3_bridge_parse_response, rr3_periph_response_frame};
