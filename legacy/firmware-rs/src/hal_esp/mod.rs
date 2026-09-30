//! hal_esp：ESP32 硬件适配层（平台绑定层）
//!
//! 迁移边界（RK3566 重写区）：本目录下所有模块在迁移时整体用
//! Linux 栈重写（ALSA / gpiod / NetworkManager / BlueZ），
//! 但各模块的公开 API 契约保持不变，供 cloud/dialog/main 复用。
//! 本目录内代码允许使用 esp-idf-svc/hal 类型。

pub mod audio;
pub mod bt;
pub mod relay;
pub mod wifi;
