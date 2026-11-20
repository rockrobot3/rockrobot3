//! 板侧绑定单点（`rr3.set_bindings` 注入态，批N4 件七(c) / 批N9 件五）：宿主经桥上
//! method 下发的引脚真值（esp32-mini `[hardware]` 十键 = 批N4 三键自愈/观测面 +
//! 批N9 件五七键主驱动对账面 → rrd RR3_HAL_BINDINGS → supervisor env → 桥），落
//! 此进程级单例。引脚号是芯片级物理事实，且 I2C 自愈的裸寄存器路径在深层自由函数
//! （[`crate::oled`]，够不到 `Bus`）——故不随 `PeriphState` 走，单点存取。
//! **消费面 fail-closed：未注入（None）即拒不动作**，观测面在调用方
//! （boot 解卡点名 / 运行面 `selfheal_skipped` 计数 / 注入到达时装配对账
//! `binding_mismatch` 计数，见 [`crate::board::rr3_bindings_check`]）。

use std::sync::Mutex;

use rr_ipc::Rr3BoardBindings;

/// 板侧绑定单点（`None` = 未注入）。毒锁不失效（同 [`crate::state::lock_bus`] 纪律）。
static BOARD_BINDINGS: Mutex<Option<Rr3BoardBindings>> = Mutex::new(None);

/// 覆盖写板侧绑定单点（set_bindings 分发面用；全量覆盖语义，与宿主
/// `rr3_push_bindings` 每次连接重推全量一致）。
pub(crate) fn rr3_board_binds_set(b: Rr3BoardBindings) {
    *BOARD_BINDINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(b);
}

/// 读 I2C 引脚对 `(sda, scl)`（自愈路径消费点用；缺任一键或未注入回 None）。
pub(crate) fn rr3_i2c_binds_pair() -> Option<(u8, u8)> {
    BOARD_BINDINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(Rr3BoardBindings::rr3_board_i2c_binds)
}

/// 自愈路径的 I2C 引脚对：**注入优先，缺席回编译期装配锚**（批N41 件一）。
///
/// 独立部署形态（音频独占 UART、无 act 桥）下宿主永远不会推 `rr3.set_bindings`；
/// boot 解卡与运行面自愈若继续 fail-closed，就会永久停摆（真板实录 `selfheal_skipped`
/// 293176）——OLED 一旦在冷启动/舵机噪声中丢拍就再也回不来。本函数只服务**自愈动作**：
/// 编译期装配常量是当前 bin 的板级真值（classic 21/22、S3 8/9），注入值仍优先；
/// 消费面（帧面绑定/读数）的 fail-closed 语义不变。
pub(crate) fn rr3_i2c_recovery() -> (u8, u8) {
    rr3_i2c_binds_pair().unwrap_or((
        crate::board::RR3_BIND_I2C_SDA,
        crate::board::RR3_BIND_I2C_SCL,
    ))
}

/// 读全量绑定快照（批N13 件一配置腿 `/api/board/bindings` 读数用；Clone 出锁，
/// 不持锁组 JSON——与 [`rr3_i2c_binds_pair`] 同一单点）。
pub(crate) fn rr3_board_binds_snap() -> Option<Rr3BoardBindings> {
    BOARD_BINDINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}
