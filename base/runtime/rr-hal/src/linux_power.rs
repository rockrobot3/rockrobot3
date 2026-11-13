//! linux（I2C）电池读数后端：INA219 总线电压（R18 扩围，0.0.6 方案 §5.4/
//! 判据 12，BOM #5 定案在前——INA219@0x40，"电阻分压 + ADS1115"为存档备选）。
//!
//! 编译门是单重的：`target_os = "linux"`——依赖 libc 仅取 `ioctl`/`read`/`write`
//! /`close` 的 extern 声明（纯 Rust 壳，musl 静态零额外 C 库依赖，R13），无
//! feature 门。主机 CI（macOS）不编译本文件；宿主侧验证路径 = aarch64 musl
//! 交叉 check（R13）+ 板上 `cargo test -p rr-hal`（真 linux 目标跑单测：换算
//! 纯函数 + temp 树假设备错误传播路径，不触真硬件）。真件档验收（判据 12：
//! `i2cget` 原始值与万用表实测两点标定、偏差 ≤2%；0x00 读回与分流校准后的
//! 电流符号实录）留外设到位随批F5a 真件档。
//!
//! 接口面：I2C chardev UAPI（`/dev/i2c-N`，i2c-dev.h）——`I2C_SLAVE` 定从地址，
//! 原生寄存器读 = 写 1 字节寄存器号 + 读 2 字节（INA219 原生大端序，高字节
//! 在前；注意与 `i2cget w` 的 SMBus 小端序字不同，对拍时先 `i2cget` 字节交换）。
//! INA219 寄存器：0x00 = 配置（上电复位值 0x399F，即
//! `contracts/devices.toml` 的 `id_reg.expect` 判据源）；0x02 = 总线电压
//! （bits[15:3] 有效 13-bit，LSB 4mV，量程 0–32V）；0x04 = 电流（i16，符号 =
//! 方向）；0x05 = 校准（写 16-bit CAL 后电流寄存器才有物理意义）。打开总线
//! 后先读 0x00 校验器件，不符点名拒（应答了但不是该器件/已被重配）；装配侧
//! 给了分流参数（[`Rr3InaCal`]）才算 CAL 并写 0x05，**校准写入成功前 charging
//! 恒 false**——不把"没校过"冒充"没在充"，与"设备只报数、策略不在这里"同条。
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

/// INA219 寄存器：配置（0x00，上电复位值即契约 `id_reg.expect`）
const INA219_REG_CONFIG: u8 = 0x00;
/// INA219 寄存器：总线电压（read-only，bits[15:3] 有效，LSB 4mV）
const INA219_REG_BUS_VOLTAGE: u8 = 0x02;
/// INA219 寄存器：电流（i16 有符号，符号 = 方向；calibration 未配恒 0）
const INA219_REG_CURRENT: u8 = 0x04;
/// INA219 寄存器：校准（写 16-bit 校准值后电流寄存器的读数才有物理意义）
const INA219_REG_CALIBRATION: u8 = 0x05;

/// 器件在位的机器判据：`contracts/devices.toml` INA219 的
/// `id_reg = { addr = 0x00, expect = 0x399F }`（配置寄存器上电复位值，datasheet
/// 定值）。契约是不可运行时的静态文件，此处按机器判据抄值并以用例钉死。
pub const RR3_INA219_ID_EXPECT: u16 = 0x399F;

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

/// 分流校准参数：`r_mohm` = 分流电阻（毫欧，装配事实），`i_lsb_ua` = 电流
/// LSB（微安/位，量程策略）。两值都来自装配侧，缺一即不校准（不猜）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3InaCal {
    pub r_mohm: u32,
    pub i_lsb_ua: u32,
}

impl Rr3InaCal {
    /// 校准寄存器值：`CAL = trunc(0.04096 / (R_shunt × I_lsb))`（INA219 数据
    /// 手册 calibration 公式；手册的 R 单位 Ω、I 单位 A，本函数入参为毫欧/微安，
    /// 常数折算成 `40_960_000`）。零参除零与超 16 位都点名拒。
    pub fn rr3_cal_reg(&self) -> Result<u16, String> {
        if self.r_mohm == 0 || self.i_lsb_ua == 0 {
            return Err(format!(
                "分流校准参数含 0（R={} mΩ, I_lsb={} µA）——除零，拒",
                self.r_mohm, self.i_lsb_ua
            ));
        }
        let cal = 40_960_000u64 / (self.r_mohm as u64 * self.i_lsb_ua as u64);
        u16::try_from(cal).map_err(|_| {
            format!(
                "分流校准值 {cal} 超 16 位寄存器（R={} mΩ, I_lsb={} µA）——分流过小或 LSB 过细",
                self.r_mohm, self.i_lsb_ua
            )
        })
    }
}

/// 定从地址（`I2C_SLAVE` ioctl）：失败点名总线路径与地址（errno 就地取出）。
fn rr3_slave_set(fd: std::os::raw::c_int, bus_path: &Path, addr: u8) -> Result<(), Rr3HalError> {
    // SAFETY：fd 合法，I2C_SLAVE 是 i2c-dev.h 定值 ioctl（0x0703），参数
    // 为 7-bit 从地址。errno 在 File drop 前取出（close 可能改写 errno）。
    let ret = unsafe { libc::ioctl(fd, I2C_SLAVE_IOCTL as _, addr as u32) };
    if ret < 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        let detail = std::io::Error::from_raw_os_error(errno);
        return Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(
            &format!(
                "i2c {:?} 设从地址 {addr:#04x} 失败: {detail}（errno {errno}）",
                bus_path.display()
            ),
        )));
    }
    Ok(())
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
    /// 分流校准参数；None = 装配侧未给，电流面不可信（charging 恒 false）
    cal: Option<Rr3InaCal>,
    /// None = 尚未 ensure；Some = 总线已打开、器件已校验、校准状态已定
    bus_fd: Mutex<Option<Rr3Bus>>,
}

/// 已打开总线的器件状态：fd 所有权 + 电流读数是否已校准（charging 的唯一前件）。
struct Rr3Bus {
    fd: std::os::raw::c_int,
    calibrated: bool,
}

impl Rr3LinuxIna219 {
    /// 指定根/总线/地址/校准参数构造（选择函数从生效物理绑定取
    /// `i2c_bus`/`ina219_addr` 与分流两键，批D④ fail-closed——都没有代码缺省，
    /// 描述符/env 都没给就在选择面点名；测试用 temp 树也走这里，不触真硬件）
    pub fn rr3_linux_power_new(
        dev_root: &Path,
        bus: u32,
        addr: u8,
        cal: Option<Rr3InaCal>,
    ) -> Self {
        Self {
            dev_root: dev_root.to_path_buf(),
            bus,
            addr,
            cal,
            bus_fd: Mutex::new(None),
        }
    }

    fn bus_dev_path(&self) -> PathBuf {
        self.dev_root.join(format!("i2c{}", self.bus))
    }

    /// 首次使用：打开总线 + `I2C_SLAVE` 定从地址 + 器件校验/校准。`slot` 由
    /// 调用方在持锁状态下传入——本函数只在 ensure 路径上跑一次；返回电流面
    /// 是否可信用（校准写入成功才 true）。
    fn ensure(&self, slot: &mut Option<Rr3Bus>) -> Result<bool, Rr3HalError> {
        if let Some(bus) = slot {
            return Ok(bus.calibrated);
        }
        let file = self.rr3_bus_open()?;
        // 校验/校准失败时 File 就地 drop（close）：只有全过才移交 fd 所有权，
        // 不把"验不过的器件"留在已打开状态。
        let calibrated = self.rr3_dev_init(file.as_raw_fd())?;
        *slot = Some(Rr3Bus {
            fd: file.into_raw_fd(),
            calibrated,
        });
        Ok(calibrated)
    }

    /// 打开总线文件并 `I2C_SLAVE` 定从地址（失败点名路径与地址）。
    fn rr3_bus_open(&self) -> Result<std::fs::File, Rr3HalError> {
        let file = std::fs::File::open(self.bus_dev_path()).map_err(|e| {
            Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                "i2c 打开 {:?} 失败: {e}（总线不在位或权限不足——raspi-config 需开 I2C）",
                self.bus_dev_path().display()
            )))
        })?;
        rr3_slave_set(file.as_raw_fd(), &self.bus_dev_path(), self.addr)?;
        Ok(file)
    }

    /// 器件级初始化：先读 0x00 配置寄存器校验 ID（契约判据），有分流参数再写
    /// 0x05 校准。返回"电流读数是否可信"。
    fn rr3_dev_init(&self, fd: std::os::raw::c_int) -> Result<bool, Rr3HalError> {
        let got = u16::from_be_bytes(rr3_reg_read(fd, INA219_REG_CONFIG)?);
        if got != RR3_INA219_ID_EXPECT {
            return Err(rr3_bad_id(got, &self.bus_dev_path()));
        }
        let Some(cal) = self.cal else {
            return Ok(false);
        };
        let reg = cal
            .rr3_cal_reg()
            .map_err(|e| rr3_bad_text(&format!("INA219 分流校准拒写：{e}")))?;
        rr3_reg_write_u16(fd, INA219_REG_CALIBRATION, reg)?;
        Ok(true)
    }

    /// 原生寄存器读：写 1 字节寄存器号 + 读 2 字节（INA219 原生大端）。
    /// 持锁调用（fd 面串行），两步不完整即回错（I2C NACK / EIO）。
    fn read_register(&self, reg: u8) -> Result<u16, Rr3HalError> {
        let guard = self.bus_fd.lock().unwrap_or_else(|p| p.into_inner());
        let fd = guard.as_ref().expect("ensure 已过，总线 fd 必在位").fd;
        Ok(u16::from_be_bytes(rr3_reg_read(fd, reg)?))
    }
}

/// 统一错误构造：点名文案直进 `InvalidParam`（ioctl/IO 面共用同一出口）。
fn rr3_bad_text(msg: &str) -> Rr3HalError {
    Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(msg))
}

/// 0x00 读回与契约期望不符：read 已回错 = 总线无应答（没接/坏线）；读回值不符 =
/// 应答了但不是该器件或已被重配——两类故障文案分开，现场不用猜。
fn rr3_bad_id(got: u16, bus: &Path) -> Rr3HalError {
    rr3_bad_text(&format!(
        "INA219 器件校验失败（{:?}）：0x00 配置寄存器读回 {got:#06x}，契约期望 {RR3_INA219_ID_EXPECT:#06x}——型号不符，或已被其他驱动重配（判据取上电复位值）",
        bus.display()
    ))
}

/// 16-bit 寄存器值的原生大端帧（寄存器号 + 高字节 + 低字节）。
fn rr3_reg_frame(reg: u8, val: u16) -> [u8; 3] {
    [reg, (val >> 8) as u8, val as u8]
}

/// 写一帧（地址相位）：失败点名寄存器号、帧长与 errno。
fn rr3_reg_send(fd: std::os::raw::c_int, frame: &[u8]) -> Result<(), Rr3HalError> {
    // SAFETY：fd 合法，缓冲区与长度配对
    let wr = unsafe { libc::write(fd, frame.as_ptr() as *const libc::c_void, frame.len()) };
    if wr != frame.len() as isize {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        return Err(rr3_bad_text(&format!(
            "i2c 写寄存器 {:#04x}（{}B 帧）失败（errno {errno}——地址无应答或 NACK）",
            frame[0],
            frame.len()
        )));
    }
    Ok(())
}

/// 写 16-bit 寄存器值（INA219 校准寄存器用：指针 + 2 字节原生大端）。
fn rr3_reg_write_u16(fd: std::os::raw::c_int, reg: u8, val: u16) -> Result<(), Rr3HalError> {
    rr3_reg_send(fd, &rr3_reg_frame(reg, val))
}

/// 读 2 字节寄存器值：先写 1 字节寄存器号（地址相位）再读（原生大端）。
fn rr3_reg_read(fd: std::os::raw::c_int, reg: u8) -> Result<[u8; 2], Rr3HalError> {
    rr3_reg_send(fd, &[reg])?;
    let mut buf = [0u8; 2];
    // SAFETY：fd 合法，缓冲区与长度配对
    let rd = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, 2) };
    if rd != 2 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        return Err(rr3_bad_text(&format!(
            "i2c 读寄存器 {reg:#04x} 失败（{rd}/2 字节，errno {errno}——地址无应答或 NACK）"
        )));
    }
    Ok(buf)
}

impl Drop for Rr3LinuxIna219 {
    fn drop(&mut self) {
        let bus = self
            .bus_fd
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(bus) = bus {
            unsafe {
                libc::close(bus.fd);
            }
        }
    }
}

impl Rr3Power for Rr3LinuxIna219 {
    fn battery(&self) -> Result<Rr3BatteryState, Rr3HalError> {
        let mut guard = self.bus_fd.lock().unwrap_or_else(|p| p.into_inner());
        let calibrated = self.ensure(&mut guard)?;
        drop(guard);
        let bus_mv = ina219_bus_mv(self.read_register(INA219_REG_BUS_VOLTAGE)?);
        // 电流符号 = 方向；未校准（0x05 未写）时电流寄存器恒 0、读数不可信，
        // charging 一律 false——不把"没校过"冒充"没在充"。
        let current_raw = self.read_register(INA219_REG_CURRENT)? as i16;
        Ok(Rr3BatteryState {
            voltage_mv: bus_mv as u16,
            percent: cell_percent(bus_mv),
            charging: calibrated && current_raw < 0,
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
        assert_eq!(
            ina219_bus_mv(0x07D0),
            1000,
            "0x07D0>>3=250 → 250×4mV=1000mV"
        );
        // 4.0V 单芯典型态：raw = (4000mV/4mV)<<3 = 1000<<3 = 0x1F40
        assert_eq!(ina219_bus_mv(0x1F40), 4000);
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
        let dev = Rr3LinuxIna219::rr3_linux_power_new(&root, 1, 0x40, None);
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
        let dev = Rr3LinuxIna219::rr3_linux_power_new(&root, 1, 0x40, None);
        drop(dev);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 器件 ID 判据钉死 `contracts/devices.toml` 的 `id_reg`（0x00 / 0x399F）：
    /// 契约与代码分家时这条先红
    #[test]
    fn id_judge_matches_devices_contract() {
        assert_eq!(INA219_REG_CONFIG, 0x00, "id_reg.addr");
        assert_eq!(RR3_INA219_ID_EXPECT, 0x399F, "id_reg.expect（上电复位值）");
    }

    /// 校准公式算例（手册式 CAL = 0.04096/(R×I_lsb) 的毫欧/微安折算）：
    /// 0.1Ω × 100µA → 0x1000；2mΩ × 1mA → 0x5000
    #[test]
    fn cal_reg_nominal_examples() {
        let cal = Rr3InaCal {
            r_mohm: 100,
            i_lsb_ua: 100,
        };
        assert_eq!(cal.rr3_cal_reg().unwrap(), 0x1000);
        let cal = Rr3InaCal {
            r_mohm: 2,
            i_lsb_ua: 1000,
        };
        assert_eq!(cal.rr3_cal_reg().unwrap(), 0x5000);
    }

    /// 零参（除零）与超 16 位各点名拒——不静默截断成"看起来能用"的值
    #[test]
    fn cal_reg_rejects_zero_and_overflow() {
        for cal in [
            Rr3InaCal {
                r_mohm: 0,
                i_lsb_ua: 100,
            },
            Rr3InaCal {
                r_mohm: 100,
                i_lsb_ua: 0,
            },
        ] {
            let e = cal.rr3_cal_reg().unwrap_err();
            assert!(e.contains("含 0"), "{e}");
        }
        let cal = Rr3InaCal {
            r_mohm: 1,
            i_lsb_ua: 1,
        };
        let e = cal.rr3_cal_reg().unwrap_err();
        assert!(e.contains("超 16 位"), "{e}");
    }

    /// 校准寄存器帧 = 指针 + 原生大端两字节（写相位编码单测）
    #[test]
    fn cal_frame_is_pointer_then_big_endian() {
        assert_eq!(INA219_REG_CALIBRATION, 0x05);
        assert_eq!(
            rr3_reg_frame(INA219_REG_CALIBRATION, 0x1000),
            [0x05, 0x10, 0x00]
        );
        assert_eq!(
            rr3_reg_frame(INA219_REG_CALIBRATION, 0x5000),
            [0x05, 0x50, 0x00]
        );
    }
}
