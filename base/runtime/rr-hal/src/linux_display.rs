//! linux（spidev UAPI）MAX7219 LED 点阵显示后端：真件档（0.0.6 批F5a③ R11③
//! trait 立面，本档随链④真件到位实装）。
//!
//! 编译门是单重的：`target_os = "linux"`——依赖 libc 仅取 `ioctl` 的 extern
//! 声明（纯 Rust 壳，musl 静态链接零额外 C 库依赖，R13 友好），无 feature 门
//! （与 [`crate::linux_gpio`] 同形）。主机 CI（macOS）不编译本文件；宿主侧
//! 验证路径 = aarch64 musl 交叉 check（R13）+ 板上真件实测（SPI ioctl 面无
//! 法假树模拟，确定性单测 = ioctl 号/包组装纯函数 + temp 树普通文件的错误
//! 传播路径，不触真硬件）。
//!
//! 接口面：spidev 用户态 API（`/dev/spidevB.C`，SPI 模式 0 / MSB first /
//! 8 bit/字 / 1 MHz——飞线杜邦的保守速率）。CS 包络由内核 spidev 驱动管理：
//! 一次 `write()` 内 CS 保持低、结束后拉高，恰是 MAX7219 的 16-bit 包锁存
//! 语义（DIN 上升沿锁存到行寄存器），HAL 无需再摸 GPIO。
//!
//! 级联形状：参考机是双位模块（两片 MAX7219 级联，`RR3_MAX7219_CHIPS = 2`），
//! 同帧双片同显——发送顺序 = 先发的 16-bit 包落在链尾那片，两片同值时顺序
//! 无关紧要；单位模块换装时只改片数常量并如实重测，帧面契约（[`Rr3FaceFrame`]
//! 8x8 行主序、bit7 最左、行寄存器直排，rr-hal-api 定案档）不动。
//!
//! 开机 init 序列（datasheet 惯例：先关显示测试、配扫描/解码/亮度，最后进
//! normal 工作态）：DISPLAYTEST=off → SCANLIMIT=7 → DECODE=off →
//! INTENSITY → SHUTDOWN=normal。亮度缺省 [`RR3_MAX7219_INTENSITY_DEFAULT`]
//! （0x03 = 3/31 占空比，近看不刺眼；现场可调走重装，不留 env 旋钮——旋钮
//! 应急键是"物理绑定"口径，亮度是体验参数）。
//!
//! boot 语义：选择函数（[`crate::backend::rr3_hal_display`]）不触设备：构造
//! 只存路径，打开 + ioctl 配置 + init 序列延迟到首次 trait 调用（ensure，与
//! gpio/pwm/alsa 后端 open 延迟同形）。设备不在位/权限不足/非字符设备在此
//! 显式报错，rr3-interact 开机探活按"能力缺失显式回错" exit(1)，不静默退化。

use std::path::PathBuf;
use std::sync::Mutex;

use crate::{Rr3Display, Rr3ErrText, Rr3FaceFrame, Rr3HalError};

/// 级联片数（参考机 = 双位模块，见模块头注记）
const RR3_MAX7219_CHIPS: usize = 2;

/// 亮度寄存器缺省值（0x00..=0x0F 占空比档位）
const RR3_MAX7219_INTENSITY_DEFAULT: u8 = 0x03;

/// spidev ioctl 的 ioctl 号族（linux/spi/spidev.h：`SPI_IOC_MAGIC = 0x6B`），
/// 方向 _IOW（asm-generic/ioctl.h 通用式，aarch64/x86_64 同构）。
const SPI_IOC_MAGIC: u32 = 0x6B;
/// generic-abi 的 _IOW 方向位（DIRSHIFT=30）
const IOC_WRITE: u32 = 1;

/// _IOW 编码（asm-generic/ioctl.h 通用式；与 [`crate::linux_gpio`] 的
/// _IOWR 同族，方向位不同）。
const fn rr3_iow(ty: u32, nr: u32, size: u32) -> u32 {
    (IOC_WRITE << 30) | (size << 16) | (ty << 8) | nr
}

/// `SPI_IOC_WR_MODE` = _IOW(0x6B, 1, u8)：写 SPI 模式（0 = CPOL=0/CPHA=0）
const SPI_IOC_WR_MODE: u32 = rr3_iow(SPI_IOC_MAGIC, 1, 1);
/// `SPI_IOC_WR_BITS_PER_WORD` = _IOW(0x6B, 3, u8)：写字宽（8）
const SPI_IOC_WR_BITS_PER_WORD: u32 = rr3_iow(SPI_IOC_MAGIC, 3, 1);
/// `SPI_IOC_WR_MAX_SPEED_HZ` = _IOW(0x6B, 4, u32)：写时钟上限（1 MHz）
const SPI_IOC_WR_MAX_SPEED_HZ: u32 = rr3_iow(SPI_IOC_MAGIC, 4, 4);

/// MAX7219 寄存器地址（Datasheet 表 2；行寄存器 DIGIT0-7 = 0x01..=0x08）
const RR3_MAX7219_REG_DIGIT0: u8 = 0x01;
/// 解码模式寄存器（0x09；全 no-decode = 直排位图）
const RR3_MAX7219_REG_DECODE: u8 = 0x09;
/// 亮度寄存器（0x0A）
const RR3_MAX7219_REG_INTENSITY: u8 = 0x0A;
/// 扫描上限寄存器（0x0B；7 = 8 行全扫）
const RR3_MAX7219_REG_SCANLIMIT: u8 = 0x0B;
/// 关断寄存器（0x0C；1 = normal 工作态）
const RR3_MAX7219_REG_SHUTDOWN: u8 = 0x0C;
/// 显示测试寄存器（0x0F；0 = off）
const RR3_MAX7219_REG_DISPLAYTEST: u8 = 0x0F;

/// 一条 MAX7219 命令的级联包：每片收一个 16-bit 包（reg, data），CS 拉低期间
/// 逐片移入。独立纯函数：包形状可单测。
fn rr3_max7219_cmd_payload(reg: u8, data: u8) -> [u8; RR3_MAX7219_CHIPS * 2] {
    let mut p = [0u8; RR3_MAX7219_CHIPS * 2];
    for chip in 0..RR3_MAX7219_CHIPS {
        p[chip * 2] = reg;
        p[chip * 2 + 1] = data;
    }
    p
}

/// 开机 init 序列（见模块头注记顺序）。独立纯函数：序列形状可单测。
fn rr3_max7219_init_payloads() -> Vec<u8> {
    [
        (RR3_MAX7219_REG_DISPLAYTEST, 0x00),
        (RR3_MAX7219_REG_SCANLIMIT, 0x07),
        (RR3_MAX7219_REG_DECODE, 0x00),
        (RR3_MAX7219_REG_INTENSITY, RR3_MAX7219_INTENSITY_DEFAULT),
        (RR3_MAX7219_REG_SHUTDOWN, 0x01),
    ]
    .iter()
    .flat_map(|&(reg, data)| rr3_max7219_cmd_payload(reg, data))
    .collect()
}

/// 整帧刷包：8 行 × 双片，行号 = [`RR3_MAX7219_REG_DIGIT0`] + n，行数据按
/// 帧面直排（bit7 最左，rr-hal-api 定案档，无位序变换）。一次 `write()` 发
/// 完（CS 包络整段覆盖 = 8 行一次刷新，整帧替换语义）。
fn rr3_max7219_frame_payload(frame: &Rr3FaceFrame) -> Vec<u8> {
    (0u8..8)
        .flat_map(|row| rr3_max7219_cmd_payload(RR3_MAX7219_REG_DIGIT0 + row, frame[row as usize]))
        .collect()
}

/// linux spidev MAX7219 显示（[`crate::Rr3Display`] 的 linux 后端）。
///
/// [`Rr3Display::frame`] 的"读回 == 上次 draw 的帧"语义由 `last` 快照承载
/// （MAX7219 无读回通道，快照是探活与对账的真值）；`spi` 是惰性开口槽
/// （None = 尚未 ensure；Some = 配置 + init 已落，Drop 随 File 关闭）。
pub struct Rr3Max7219SpiDisplay {
    dev_path: PathBuf,
    spi: Mutex<Option<std::fs::File>>,
    last: Mutex<Rr3FaceFrame>,
}

impl Rr3Max7219SpiDisplay {
    /// 指定 spidev 设备路径构造（选择函数从生效物理绑定取 `spi_device`，
    /// 批D④ fail-closed 同口径——设备路径没有代码缺省，描述符/env 都没给
    /// 就在选择面点名；测试用 temp 树也走这里，不触真硬件）
    pub fn rr3_max7219_display_new(dev_path: &std::path::Path) -> Self {
        Self {
            dev_path: dev_path.to_path_buf(),
            spi: Mutex::new(None),
            last: Mutex::new([0; 8]),
        }
    }

    /// 首次使用：RDWR 开设备 + ioctl 三连（模式 0 / 字宽 8 / 1 MHz）+ init
    /// 序列。`slot` 由调用方在持锁状态下传入——本函数只在 ensure 路径上跑
    /// 一次。ioctl 号与用法配对见常量注记；errno 当场取出（无中间 libc 调用
    /// 窗口），失败点名设备路径（探活 exit(1) 的排障入口）。
    fn ensure(&self, slot: &mut Option<std::fs::File>) -> Result<(), Rr3HalError> {
        if slot.is_some() {
            return Ok(());
        }
        let dev = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.dev_path)
            .map_err(|e| {
                Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                    "display 打开 {:?} 失败: {e}（设备不在位或权限不足）",
                    self.dev_path.display()
                )))
            })?;
        for (req, value, what) in [
            (SPI_IOC_WR_MODE, 0u8, "写 SPI 模式失败"),
            (SPI_IOC_WR_BITS_PER_WORD, 8u8, "写字宽失败"),
        ] {
            // SAFETY：value 是合法可写指针，ioctl 号与 u8 载荷配对（单测钉死）
            let ret = unsafe {
                libc::ioctl(
                    std::os::fd::AsRawFd::as_raw_fd(&dev),
                    req as _,
                    &value as *const u8,
                )
            };
            if ret < 0 {
                let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                return Err(rr3_spidev_err(&self.dev_path, what, errno));
            }
        }
        let speed: u32 = 1_000_000;
        // SAFETY：speed 是合法可写指针，ioctl 号与 u32 载荷配对（单测钉死）
        let ret = unsafe {
            libc::ioctl(
                std::os::fd::AsRawFd::as_raw_fd(&dev),
                SPI_IOC_WR_MAX_SPEED_HZ as _,
                &speed as *const u32,
            )
        };
        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(rr3_spidev_err(&self.dev_path, "写时钟上限失败", errno));
        }
        use std::io::Write;
        (&dev)
            .write_all(&rr3_max7219_init_payloads())
            .map_err(|e| {
                Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                    "display {:?} init 序列写失败: {e}",
                    self.dev_path.display()
                )))
            })?;
        *slot = Some(dev);
        Ok(())
    }

    /// 对在位设备整包写出（ensure 已过，fd 必在位）。写失败统一 InvalidParam
    /// 点名——SPI 无"忙"语义（独占 fd 自持），硬件掉线/线松在此显式听见。
    fn write_all(&self, payload: &[u8], what: &str) -> Result<(), Rr3HalError> {
        use std::io::Write;
        let guard = self.spi.lock().unwrap_or_else(|p| p.into_inner());
        let dev = guard.as_ref().expect("ensure 已过，设备必在位");
        (&*dev).write_all(payload).map_err(|e| {
            Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                "display {:?} {what}: {e}",
                self.dev_path.display()
            )))
        })
    }
}

/// errno → HAL 错误（spidev ioctl 面）：统一 InvalidParam 带点名（设备路径 +
/// 现象 + errno）。SPI 无 EBUSY 特判语义（见 [`Rr3Max7219SpiDisplay::write_all`]）。
fn rr3_spidev_err(dev: &std::path::Path, what: &str, errno: libc::c_int) -> Rr3HalError {
    let detail = std::io::Error::from_raw_os_error(errno);
    Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
        "display {:?} {what}: {detail}（errno {errno}）",
        dev.display()
    )))
}

impl Drop for Rr3Max7219SpiDisplay {
    fn drop(&mut self) {
        // File drop 即 close（内核释放 spidev 独占）；last 快照随之失效——
        // 显示件重开时探活帧回到黑屏，与"新进程无历史"同语义。
        self.spi.get_mut().unwrap_or_else(|p| p.into_inner()).take();
    }
}

impl Rr3Display for Rr3Max7219SpiDisplay {
    fn draw(&mut self, frame: &Rr3FaceFrame) -> Result<(), Rr3HalError> {
        {
            let mut guard = self.spi.lock().unwrap_or_else(|p| p.into_inner());
            self.ensure(&mut guard)?;
        }
        self.write_all(&rr3_max7219_frame_payload(frame), "出帧写失败")?;
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = *frame;
        Ok(())
    }

    fn frame(&self) -> Result<Rr3FaceFrame, Rr3HalError> {
        {
            let mut guard = self.spi.lock().unwrap_or_else(|p| p.into_inner());
            self.ensure(&mut guard)?;
        }
        Ok(*self.last.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// ioctl 号与内核 linux/spi/spidev.h 常量硬编码对齐（aarch64/x86_64
    /// generic-abi）：_IOW(0x6B, 1, 1) 等——编码式写错在此最先暴露
    #[test]
    fn ioctl_numbers_match_kernel_uapi() {
        assert_eq!(SPI_IOC_WR_MODE, 0x4001_6B01);
        assert_eq!(SPI_IOC_WR_BITS_PER_WORD, 0x4001_6B03);
        assert_eq!(SPI_IOC_WR_MAX_SPEED_HZ, 0x4004_6B04);
    }

    /// 级联命令包：每片同值（reg, data）×2——双片同显语义
    #[test]
    fn cmd_payload_is_per_chip_duplicated() {
        assert_eq!(
            rr3_max7219_cmd_payload(0x0C, 0x01),
            [0x0C, 0x01, 0x0C, 0x01]
        );
    }

    /// init 序列 golden：五条命令按头注记顺序，各带双片同值包
    #[test]
    fn init_payload_matches_datasheet_order() {
        assert_eq!(
            rr3_max7219_init_payloads(),
            vec![
                0x0F, 0x00, 0x0F, 0x00, // DISPLAYTEST off
                0x0B, 0x07, 0x0B, 0x07, // SCANLIMIT 7
                0x09, 0x00, 0x09, 0x00, // DECODE off
                0x0A, 0x03, 0x0A, 0x03, // INTENSITY 缺省
                0x0C, 0x01, 0x0C, 0x01, // SHUTDOWN normal
            ]
        );
    }

    /// 整帧刷包 golden：行号直排（DIGIT0..7）、行数据 bit7 最左原样搬运、
    /// 双片同值——黑帧全 0、特征帧逐行落位
    #[test]
    fn frame_payload_maps_rows_in_order() {
        // 黑帧：寄存器地址字节恒在位（DIGIT1..8），数据字节全 0
        assert_eq!(
            rr3_max7219_frame_payload(&[0; 8]),
            (1u8..=8).flat_map(|r| [r, 0, r, 0]).collect::<Vec<u8>>()
        );
        let frame = [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01];
        let p = rr3_max7219_frame_payload(&frame);
        for row in 0..8usize {
            let off = row * 4;
            assert_eq!(p[off], (RR3_MAX7219_REG_DIGIT0 + row as u8));
            assert_eq!(p[off + 1], frame[row], "行 {row} 数据直排");
            assert_eq!(p[off + 2], (RR3_MAX7219_REG_DIGIT0 + row as u8));
            assert_eq!(p[off + 3], frame[row], "双片同显");
        }
    }

    /// 错误传播路径（确定性，不触真硬件）：temp 树放普通文件冒充 spidev——
    /// open 成功但 ioctl 非字符设备必 ENOTTY，ensure 显式回 InvalidParam 并
    /// 点名设备路径；设备 fd 随 File drop 关闭（无泄漏面）
    #[test]
    fn ensure_on_non_chardev_fails_loudly() {
        let root = std::env::temp_dir().join(format!("rr3-hal-spi-{}-err", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let dev = root.join("spidev0.0");
        std::fs::write(&dev, "").unwrap();
        let mut out = Rr3Max7219SpiDisplay::rr3_max7219_display_new(&dev);
        // 探活读面 &self；写面 draw 是 &mut self，绑定取 mut
        let err = out.frame().unwrap_err();
        assert!(
            matches!(err, Rr3HalError::InvalidParam(ref m)
                if m.rr3_err_text_as_str().contains("spidev0.0")),
            "错误要点名设备路径: {err}"
        );
        // draw 同路径：同一实例第二次调用仍在 ensure 处回错（开口未建立）
        let err = out.draw(&[0; 8]).unwrap_err();
        assert!(matches!(err, Rr3HalError::InvalidParam(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 占位实例（从未 ensure）Drop 不碰任何 fd：构造后直接丢弃不炸
    #[test]
    fn drop_without_ensure_is_noop() {
        let root = std::env::temp_dir().join(format!("rr3-hal-spi-{}-drop", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let out = Rr3Max7219SpiDisplay::rr3_max7219_display_new(&root.join("spidev0.0"));
        drop(out);
        let _ = std::fs::remove_dir_all(&root);
    }
}
