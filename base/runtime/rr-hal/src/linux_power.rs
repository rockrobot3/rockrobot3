//! linux（I2C）电池读数后端：INA219 总线电压（R18 扩围，0.0.6 方案 §5.4/
//! 判据 12，BOM #5 定案在前——INA219@0x40，"电阻分压 + ADS1115"为存档备选）。
//!
//! 编译门是单重的：`target_os = "linux"`——依赖 libc 仅取 `ioctl`/`read`/`write`
//! /`close` 的 extern 声明（纯 Rust 壳，musl 静态零额外 C 库依赖，R13），无
//! feature 门。主机 CI（macOS）不编译本文件；宿主侧验证路径 = aarch64 musl
//! 交叉 check（R13）+ 板上 `cargo test -p rr-hal`（真 linux 目标跑单测：换算
//! 纯函数 + temp 树假设备错误传播路径，不触真硬件）。真件档验收（判据 12：
//! `i2cget` 原始值与万用表实测两点标定、偏差 ≤2%）留外设到位随批F5a 真件档。
//!
//! 接口面：I2C chardev UAPI（`/dev/i2c-N`，i2c-dev.h）——`I2C_SLAVE` 定从地址，
//! 原生寄存器读 = 写 1 字节寄存器号 + 读 2 字节（INA219 原生大端序，高字节
//! 在前；注意与 `i2cget w` 的 SMBus 小端序字不同，对拍时先 `i2cget` 字节交换）。
//! INA219 寄存器：0x02 = 总线电压（bits[15:3] 有效 13-bit，LSB 4mV，量程
//! 0–32V）；0x04 = 电流（i16，符号 = 方向）。芯片上电默认 config 已使能连续
//! 测量，总线电压无需配置即可读；电流面默认 calibration=0 读回恒 0——
//! **charging 判定在真件档配好分流校准前恒 false**（方向语义也随真件档接线
//! 定案），此处如实报 0 不编造，与"设备只报数、策略不在这里"分工同条。
//!
//! percent 是派生面：单芯锂电池（TP4056 充电管理，接线 5）OCV 线性近似
//! 3200–4200mV ↔ 0–100%，与 sim 后端电压曲线同一条线（两后端观测对称，R11
//! 判据精神）；`voltage_mv` 永远原样报真读数（判据 12 只对电压字段），percent
//! 越界 clamp 不越权。boot 语义（R11）：显式 `RR3_HAL_BACKEND=linux` 后，
//! rr3-power 开机探活 `battery()`——bus 不在位/地址无应答在此显式回错，
//! exit(1)，不静默退化。

use std::os::fd::{AsRawFd, IntoRawFd};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{Rr3BatteryState, Rr3ErrText, Rr3HalError, Rr3Power};

/// I2C chardev 设备根（构造参数是测试/夹具的根目录接缝；真实路径 = `{root}/i2c{N}`）
pub const RR3_I2C_DEV_ROOT_DEFAULT: &str = "/dev";

/// `I2C_SLAVE`（i2c-dev.h `_IO(0x07, 0x03)` = 0x0703）：设从地址。libc 0.2
/// 的 musl 面未导出 I2C 常量组（glibc/musl 差异），照 GPIO 批先例自定义
/// 硬编码——i2c-dev.h 定值不随内核版本漂移，单测钉死。
const I2C_SLAVE_IOCTL: u32 = 0x0703;

/// INA219 寄存器：总线电压（read-only，bits[15:3] 有效，LSB 4mV）
const INA219_REG_BUS_VOLTAGE: u8 = 0x02;
/// INA219 寄存器：电流（i16 有符号，符号 = 方向；calibration 未配恒 0）
const INA219_REG_CURRENT: u8 = 0x04;

/// 单芯锂电 OCV 线性映射下界（与 sim 后端电压曲线同一条线）
const CELL_MV_MIN: u32 = 3200;
/// 单芯锂电 OCV 线性映射上界
const CELL_MV_MAX: u32 = 4200;

/// 总线电压原始值 → 毫伏：bits[15:3] 为 13-bit 有效位，LSB 4mV（INA219 数据
/// 手册 bus voltage register 布局；真件档 `i2cget` 对拍点，标定超差先查此换算）
fn ina219_bus_mv(raw: u16) -> u32 {
    ((raw >> 3) as u32) * 4
}

/// 毫伏 → 电量百分比（单芯线性，与 sim 曲线同一条线；越界 clamp）
fn cell_percent(mv: u32) -> u8 {
    if mv <= CELL_MV_MIN {
        return 0;
    }
    if mv >= CELL_MV_MAX {
        return 100;
    }
    ((mv - CELL_MV_MIN) / 10) as u8
}

/// linux I2C INA219 电池读数（[`crate::Rr3Power`] 的 linux 后端）。
///
/// 选择函数（[`crate::backend::rr3_hal_power`]）不触设备：构造只算路径，
/// 打开总线延迟到首次 trait 调用（与 pwm/gpio 后端 open 延迟同形）。
pub struct Rr3LinuxIna219 {
    dev_root: PathBuf,
    /// I2C 总线号（`/dev/i2c{N}`；树莓派硬件 I2C = GPIO2/3 = i2c1）
    bus: u32,
    /// INA219 从地址（7-bit，缺省 0x40）
    addr: u8,
    /// None = 尚未 ensure；Some(fd) = 总线已打开且从地址已定
    bus_fd: Mutex<Option<std::os::raw::c_int>>,
}

impl Rr3LinuxIna219 {
    /// 指定根/总线/地址构造（选择函数从生效物理绑定取 `i2c_bus`/`ina219_addr`，
    /// 批D④ fail-closed——总线/地址没有代码缺省，描述符/env 都没给就在选择面
    /// 点名；测试用 temp 树也走这里，不触真硬件）
    pub fn rr3_linux_power_new(dev_root: &Path, bus: u32, addr: u8) -> Self {
        Self {
            dev_root: dev_root.to_path_buf(),
            bus,
            addr,
            bus_fd: Mutex::new(None),
        }
    }

    fn bus_dev_path(&self) -> PathBuf {
        self.dev_root.join(format!("i2c{}", self.bus))
    }

    /// 首次使用：打开总线 + `I2C_SLAVE` 定从地址。`slot` 由调用方在持锁状态
    /// 下传入——本函数只在 ensure 路径上跑一次。
    fn ensure(&self, slot: &mut Option<std::os::raw::c_int>) -> Result<(), Rr3HalError> {
        if slot.is_some() {
            return Ok(());
        }
        let file = std::fs::File::open(self.bus_dev_path()).map_err(|e| {
            Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                "i2c 打开 {:?} 失败: {e}（总线不在位或权限不足——raspi-config 需开 I2C）",
                self.bus_dev_path().display()
            )))
        })?;
        let fd = file.as_raw_fd();
        // SAFETY：fd 合法，I2C_SLAVE 是 i2c-dev.h 定值 ioctl（0x0703），参数
        // 为 7-bit 从地址。errno 在 File drop 前取出（close 可能改写 errno）。
        let ret = unsafe { libc::ioctl(fd, I2C_SLAVE_IOCTL as _, self.addr as u32) };
        let errno = if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        } else {
            0
        };
        if ret < 0 {
            let detail = std::io::Error::from_raw_os_error(errno);
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                &format!(
                    "i2c 设从地址 {:#04x} 失败: {detail}（errno {errno}）",
                    self.addr
                ),
            )));
        }
        *slot = Some(file.into_raw_fd()); // fd 所有权移交 self，Drop close
        Ok(())
    }

    /// 原生寄存器读：写 1 字节寄存器号 + 读 2 字节（INA219 原生大端）。
    /// 持锁调用（fd 面串行），两步不完整即回错（I2C NACK / EIO）。
    fn read_register(&self, reg: u8) -> Result<u16, Rr3HalError> {
        let guard = self.bus_fd.lock().unwrap_or_else(|p| p.into_inner());
        let fd = guard.expect("ensure 已过，总线 fd 必在位");
        // SAFETY：fd 合法，缓冲区与长度配对
        let wr = unsafe { libc::write(fd, &reg as *const u8 as *const libc::c_void, 1) };
        if wr != 1 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                &format!("i2c 写寄存器号 {reg:#04x} 失败（errno {errno}）"),
            )));
        }
        let mut buf = [0u8; 2];
        // SAFETY：同上
        let rd = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, 2) };
        if rd != 2 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
                &format!(
                    "i2c 读寄存器 {reg:#04x} 失败（{rd}/2 字节，errno {errno}——地址无应答或 NACK）"
                ),
            )));
        }
        Ok(u16::from_be_bytes(buf))
    }
}

impl Drop for Rr3LinuxIna219 {
    fn drop(&mut self) {
        let fd = self
            .bus_fd
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(fd) = fd {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

impl Rr3Power for Rr3LinuxIna219 {
    fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError> {
        let mut guard = self.bus_fd.lock().unwrap_or_else(|p| p.into_inner());
        self.ensure(&mut guard)?;
        drop(guard);
        let bus_mv = ina219_bus_mv(self.read_register(INA219_REG_BUS_VOLTAGE)?);
        // 电流符号 = 方向（calibration 未配读回 0 → charging=false，如实不编造；
        // 方向语义随真件档接线定案）
        let current_raw = self.read_register(INA219_REG_CURRENT)? as i16;
        Ok(Rr3BatteryState {
            voltage_mv: bus_mv as u16,
            percent: cell_percent(bus_mv),
            charging: current_raw < 0,
        })
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// I2C_SLAVE ioctl 号钉死 i2c-dev.h 定值 `_IO(0x07, 0x03)` = 0x0703
    /// （libc 0.2 musl 面不导出，自定义常量的 ABI 参考锚）
    #[test]
    fn i2c_slave_ioctl_matches_i2c_dev_h() {
        assert_eq!(I2C_SLAVE_IOCTL, 0x0703);
        // _IO 编码自洽：无方向无 size，type=0x07 nr=0x03
        assert_eq!(I2C_SLAVE_IOCTL, (0x07 << 8) | 0x03);
    }

    /// INA219 bus voltage 换算钉死：13-bit [15:3] + LSB 4mV——真件档 i2cget
    /// 对拍的换算基准（标定超差先查这里）
    #[test]
    fn bus_mv_decodes_13bit_lsb4mv() {
        assert_eq!(ina219_bus_mv(0x0000), 0);
        assert_eq!(ina219_bus_mv(0x0008), 4, "LSB 4mV");
        assert_eq!(ina219_bus_mv(0x07D0), 4000, "0x07D0>>3=250 → 1000mV");
        // 4.0V 单芯典型态：raw = 4000/4 << 3 = 0x7D0<<3 = 0x3E80
        assert_eq!(ina219_bus_mv(0x3E80), 4000);
        // 满量程 32V：0xFFFF>>3 = 8191 → 32764mV
        assert_eq!(ina219_bus_mv(0xFFFF), 32764);
    }

    /// percent 与 sim 曲线同一条线：3200–4200mV ↔ 0–100，越界 clamp，
    /// 中点 3700 → 50
    #[test]
    fn cell_percent_matches_sim_voltage_curve() {
        assert_eq!(cell_percent(0), 0);
        assert_eq!(cell_percent(3199), 0);
        assert_eq!(cell_percent(3200), 0);
        assert_eq!(cell_percent(3700), 50);
        assert_eq!(cell_percent(4200), 100);
        assert_eq!(cell_percent(9999), 100, "越界 clamp 不越权");
        // 与 sim 的 voltage = 3200 + percent*10 反演恒等
        for p in [0u8, 1, 25, 50, 80, 99, 100] {
            let mv = 3200 + p as u32 * 10;
            assert_eq!(cell_percent(mv), p, "percent {p} 反演");
        }
    }

    /// 错误传播路径（确定性，不触真硬件）：temp 树放普通文件冒充 i2c 总线——
    /// open 成功但 ioctl 非字符设备必 ENOTTY，ensure 显式回 InvalidParam
    #[test]
    fn ensure_on_non_chardev_fails_loudly() {
        let root = std::env::temp_dir().join(format!("rr3-hal-pwr-{}-err", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("i2c1"), "").unwrap();
        let dev = Rr3LinuxIna219::rr3_linux_power_new(&root, 1, 0x40);
        let err = dev.battery().unwrap_err();
        assert!(
            matches!(err, Rr3HalError::InvalidParam(ref m)
                if m.rr3_err_text_as_str().contains("i2c1")),
            "错误要点名总线路径: {err}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 占位实例（从未 ensure）Drop 不碰任何 fd
    #[test]
    fn drop_without_ensure_is_noop() {
        let root = std::env::temp_dir().join(format!("rr3-hal-pwr-{}-drop", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let dev = Rr3LinuxIna219::rr3_linux_power_new(&root, 1, 0x40);
        drop(dev);
        let _ = std::fs::remove_dir_all(&root);
    }
}
