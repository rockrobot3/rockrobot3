// SPDX-License-Identifier: Apache-2.0
//! linux（I2C）SSD1306 OLED 显示后端：128x64 单色屏（W5-3 Pi 显示面换装）。
//!
//! 编译门是单重的：`target_os = "linux"`——依赖 libc 仅取 `ioctl`/`write`/`close`
//! 的 extern 声明（纯 Rust 壳，musl 静态链接零额外 C 库依赖，R13 友好），无
//! feature 门（与 [`crate::linux_display`] 同形）。主机 CI（macOS）不编译本文件；
//! 宿主侧验证路径 = aarch64 musl 交叉 check（R13）+ 板上 `cargo test -p rr-hal`
//! （真 linux 目标跑单测：帧缓冲/init 序列纯函数 + temp 树普通文件的错误传播
//! 路径，不触真硬件）。
//!
//! 接口面：I2C chardev UAPI（`/dev/i2c-N`）——`I2C_SLAVE` 定从地址，控制字节
//! 0x00（命令流）/ 0x40（显存数据流）。SSD1306 显存是页式 8 页 × 128 列；本档
//! 用水平寻址模式（0x20 0x00）并用列/页窗口命令（0x21/0x22）把每次整帧写限定
//! 在 0..127 × 0..7，显存指针跨事务自动递增——与 MAX7219 的"一次 CS 一包"
//! 级联锁存语义不同，分块写安全。
//!
//! 帧面契约：`Rr3FaceFrame` 8x8 行主序、bit7 最左（rr-hal-api 定案档）按 8 倍
//! 最近邻放大成 64x64，水平居中（左右各留 [`RR3_OLED_X0`] 列）。表情的"形状"
//! 归模块侧（rr3-interact 表情机），HAL 只把帧如实铺到屏上，不引入字体/动画语义。
//!
//! 写事务分块：i2c-dev 单次 write 的载荷上限随适配器而异（BCM2835 充足），统一
//! [`RR3_OLED_CHUNK`] 数据字节 + 1 控制字节分块——对任意适配器安全；1024 字节
//! 整帧 = 64 笔事务，1 Hz 表情刷新下总线占用可忽略。
//!
//! 开机 init 序列（datasheet 惯例：先关显示、配时钟/复用/电荷泵/寻址，最后开
//! 显示）：DISPLAYOFF → CLOCK → MUX → OFFSET → STARTLINE → CHARGEPUMP →
//! MEMORYMODE(horizontal) → SEGREMAP → COMSCANDEC → COMPINS → CONTRAST →
//! PRECHARGE → VCOMH → RESUME → NORMAL → DISPLAYON。亮度取
//! [`RR3_SSD1306_CONTRAST_DEFAULT`]（0x7F，模块常见出厂档），不留 env 旋钮——
//! 旋钮应急键是"物理绑定"口径，对比度是体验参数。
//!
//! boot 语义：选择函数（[`crate::backend::rr3_hal_display`]）不触设备：构造只存
//! 根/总线/地址，打开总线 + `I2C_SLAVE` + init 序列延迟到首次 trait 调用
//! （ensure，与 gpio/pwm/alsa/spidev 后端 open 延迟同形）。设备不在位/地址无
//! 应答/权限不足在此显式报错，rr3-interact 开机探活按"能力缺失显式回错"
//! exit(1)，不静默退化。
//!
//! [`Rr3Display::frame`] 的"读回 == 上次 draw 的帧"由 `last` 快照承载
//! （SSD1306 的 GDDRAM 回读依 I2C 读时序，本档不做回读——快照是探活与对账的
//! 真值，与 MAX7219 档同语义）。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{Rr3Display, Rr3ErrText, Rr3FaceFrame, Rr3HalError};

/// I2C chardev 设备根（构造参数是测试/夹具的根目录接缝；真实路径 = `{root}/i2c{N}`）
pub const RR3_OLED_I2C_ROOT_DEFAULT: &str = "/dev";

/// `I2C_SLAVE`（i2c-dev.h `_IO(0x07, 0x03)` = 0x0703）：设从地址。与
/// [`crate::linux_power`] 同一常量（libc 未导出 I2C 常量组，i2c-dev.h 定值
/// 不随内核版本漂移）。
const I2C_SLAVE_IOCTL: u32 = 0x0703;

/// 屏宽（列）
const RR3_OLED_W: usize = 128;
/// 页数（每页 8 像素高；128x64 = 8 页 × 128 列）
const RR3_OLED_PAGES: usize = 8;
/// 8x8 帧 → 64x64 的放大倍数
const RR3_OLED_SCALE: usize = 8;
/// 64x64 在 128 宽屏上的水平起点（居中）
const RR3_OLED_X0: usize = (RR3_OLED_W - 8 * RR3_OLED_SCALE) / 2;

/// 控制字节：后续字节为命令流
const SSD1306_CTRL_CMD: u8 = 0x00;
/// 控制字节：后续字节为显存数据流
const SSD1306_CTRL_DATA: u8 = 0x40;
/// 单次数据事务的数据字节上限（+1 控制字节）
const RR3_OLED_CHUNK: usize = 16;
/// 对比度缺省值（0x00..=0xFF）
const RR3_SSD1306_CONTRAST_DEFAULT: u8 = 0x7F;

/// 开机 init 序列（见模块头注记顺序；ensure 一笔命令流写出）
const RR3_SSD1306_INIT_SEQ: [u8; 25] = [
    0xAE, // DISPLAYOFF
    0xD5,
    0x80, // CLOCKDIV（缺省振荡/分频）
    0xA8,
    0x3F, // MUXRATIO（64 行）
    0xD3,
    0x00, // DISPLAYOFFSET 0
    0x40, // STARTLINE 0
    0x8D,
    0x14, // CHARGEPUMP enable
    0x20,
    0x00, // MEMORYMODE horizontal
    0xA1, // SEGREMAP（列 127→SEG0）
    0xC8, // COMSCANDEC
    0xDA,
    0x12, // COMPINS（128x64 交替 COM）
    0x81,
    RR3_SSD1306_CONTRAST_DEFAULT, // CONTRAST
    0xD9,
    0xF1, // PRECHARGE
    0xDB,
    0x40, // VCOMH
    0xA4, // DISPLAYALLON_RESUME
    0xA6, // NORMALDISPLAY
    0xAF, // DISPLAYON
];

/// 整帧窗口命令：列 0..127、页 0..7（水平寻址模式下数据流按此窗口写满 1024B）
const RR3_SSD1306_WINDOW: [u8; 6] = [0x21, 0x00, 0x7F, 0x22, 0x00, 0x07];

/// 8x8 帧 → 页式显存（1024B，页 p = 帧第 p 行放大 8 倍）。
///
/// 纯函数：放大/居中规则可单测，不触设备。帧行主序、bit7 最左；每个置位点
/// 铺成 8x8 实心块（最近邻放大，点阵屏的机械形状保持）。
fn rr3_oled_fb(frame: &Rr3FaceFrame) -> [u8; RR3_OLED_W * RR3_OLED_PAGES] {
    let mut fb = [0u8; RR3_OLED_W * RR3_OLED_PAGES];
    for (row, bits) in frame.iter().enumerate() {
        for col in 0..8 {
            if *bits & (0x80 >> col) == 0 {
                continue;
            }
            let x = RR3_OLED_X0 + col * RR3_OLED_SCALE;
            let base = row * RR3_OLED_W + x;
            for px in &mut fb[base..base + RR3_OLED_SCALE] {
                *px = 0xFF;
            }
        }
    }
    fb
}

/// 定从地址（`I2C_SLAVE` ioctl）：失败点名总线路径与地址（errno 就地取出）。
fn rr3_oled_slave(fd: std::os::raw::c_int, path: &Path, addr: u8) -> Result<(), Rr3HalError> {
    // SAFETY：fd 合法，I2C_SLAVE 是 i2c-dev.h 定值 ioctl（0x0703），参数为
    // 7-bit 从地址。errno 在 File drop 前取出（close 可能改写 errno）。
    let ret = unsafe { libc::ioctl(fd, I2C_SLAVE_IOCTL as _, addr as u32) };
    if ret < 0 {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        return Err(rr3_oled_err(
            path,
            &format!("设从地址 {addr:#04x} 失败（器件不在位或地址无应答）"),
            errno,
        ));
    }
    Ok(())
}

/// 一笔 I2C 写（控制字节 + 载荷）：短写/errno 当场点名。
fn rr3_oled_send(
    fd: std::os::raw::c_int,
    payload: &[u8],
    path: &Path,
    what: &str,
) -> Result<(), Rr3HalError> {
    // SAFETY：fd 合法，payload 指针与长度配对
    let wr = unsafe { libc::write(fd, payload.as_ptr() as *const libc::c_void, payload.len()) };
    if wr != payload.len() as isize {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        return Err(rr3_oled_err(
            path,
            &format!("{what}写失败（{}B，地址无应答或 NACK）", payload.len()),
            errno,
        ));
    }
    Ok(())
}

/// 命令流写（控制字节 0x00 + 命令字节；init 与窗口命令共用）。
fn rr3_oled_cmd(fd: std::os::raw::c_int, cmds: &[u8], path: &Path) -> Result<(), Rr3HalError> {
    let mut buf = [0u8; 1 + 32];
    let Some(dst) = buf.get_mut(..1 + cmds.len()) else {
        return Err(rr3_oled_text(path, "命令流超单笔上限（32B）"));
    };
    dst[0] = SSD1306_CTRL_CMD;
    dst[1..].copy_from_slice(cmds);
    rr3_oled_send(fd, dst, path, "命令")
}

/// 整帧数据流写（控制字节 0x40 + 分块数据；显存指针自动递增）。
fn rr3_oled_data(
    fd: std::os::raw::c_int,
    fb: &[u8; RR3_OLED_W * RR3_OLED_PAGES],
    path: &Path,
) -> Result<(), Rr3HalError> {
    let mut buf = [0u8; 1 + RR3_OLED_CHUNK];
    for chunk in fb.chunks(RR3_OLED_CHUNK) {
        buf[0] = SSD1306_CTRL_DATA;
        buf[1..1 + chunk.len()].copy_from_slice(chunk);
        rr3_oled_send(fd, &buf[..1 + chunk.len()], path, "帧数据")?;
    }
    Ok(())
}

/// errno → HAL 错误（I2C 面统一 InvalidParam 带点名）。
fn rr3_oled_err(path: &Path, what: &str, errno: libc::c_int) -> Rr3HalError {
    let detail = std::io::Error::from_raw_os_error(errno);
    rr3_oled_text(path, &format!("{what}: {detail}（errno {errno}）"))
}

/// 无 errno 的静态错误（编译期常量越界这类装配错误，不带系统 errno）。
fn rr3_oled_text(path: &Path, what: &str) -> Rr3HalError {
    Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
        "oled {:?} {what}",
        path.display()
    )))
}

/// linux I2C SSD1306 OLED 显示（[`crate::Rr3Display`] 的 linux 后端）。
///
/// `last` 是探活/对账快照（无回读通道）；`dev` 是惰性开口槽（None = 尚未
/// ensure；Some = 总线已打开、init 已落，Drop 随 File 关闭）。
pub struct Rr3Ssd1306I2cDisplay {
    dev_root: PathBuf,
    bus: u32,
    addr: u8,
    dev: Mutex<Option<std::fs::File>>,
    last: Mutex<Rr3FaceFrame>,
}

impl Rr3Ssd1306I2cDisplay {
    /// 指定设备根/总线号/从地址构造（选择函数从生效物理绑定取 `i2c_bus` +
    /// `oled_addr`，批D④ fail-closed——没有代码缺省；测试用 temp 树同走这里，
    /// 不触真硬件）。
    pub fn rr3_ssd1306_new(dev_root: &Path, bus: u32, addr: u8) -> Self {
        Self {
            dev_root: dev_root.to_path_buf(),
            bus,
            addr,
            dev: Mutex::new(None),
            last: Mutex::new([0; 8]),
        }
    }

    /// 总线设备路径（i2c-dev UAPI 名 = `/dev/i2c-<bus>`，连字符不可省）
    fn bus_dev_path(&self) -> PathBuf {
        self.dev_root.join(format!("i2c-{}", self.bus))
    }

    /// 首次使用：RDWR 开总线 + 定从地址 + init 序列。`slot` 由调用方在持锁
    /// 状态下传入——本函数只在 ensure 路径上跑一次；失败时 File 就地 drop
    /// （close），不把"验不过的器件"留在已打开状态。
    fn ensure(&self, slot: &mut Option<std::fs::File>) -> Result<(), Rr3HalError> {
        if slot.is_some() {
            return Ok(());
        }
        let path = self.bus_dev_path();
        let dev = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                    "oled 打开 {:?} 失败: {e}（总线不在位或权限不足——raspi-config 需开 I2C）",
                    path.display()
                )))
            })?;
        let fd = std::os::fd::AsRawFd::as_raw_fd(&dev);
        rr3_oled_slave(fd, &path, self.addr)?;
        rr3_oled_cmd(fd, &RR3_SSD1306_INIT_SEQ, &path)?;
        *slot = Some(dev);
        Ok(())
    }
}

impl Drop for Rr3Ssd1306I2cDisplay {
    fn drop(&mut self) {
        // File drop 即 close（内核释放总线 fd）；last 快照随之失效——显示件
        // 重开时探活帧回到黑屏，与"新进程无历史"同语义。
        self.dev.get_mut().unwrap_or_else(|p| p.into_inner()).take();
    }
}

impl Rr3Display for Rr3Ssd1306I2cDisplay {
    fn draw(&mut self, frame: &Rr3FaceFrame) -> Result<(), Rr3HalError> {
        let fb = rr3_oled_fb(frame);
        let path = self.bus_dev_path();
        let mut guard = self.dev.lock().unwrap_or_else(|p| p.into_inner());
        self.ensure(&mut guard)?;
        let Some(dev) = guard.as_ref() else {
            return Err(rr3_oled_text(&path, "总线 fd 未 open（ensure 未执行）"));
        };
        let fd = std::os::fd::AsRawFd::as_raw_fd(dev);
        rr3_oled_cmd(fd, &RR3_SSD1306_WINDOW, &path)?;
        rr3_oled_data(fd, &fb, &path)?;
        drop(guard);
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = *frame;
        Ok(())
    }

    fn frame(&self) -> Result<Rr3FaceFrame, Rr3HalError> {
        {
            let mut guard = self.dev.lock().unwrap_or_else(|p| p.into_inner());
            self.ensure(&mut guard)?;
        }
        Ok(*self.last.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// init 序列 golden：首条关显示、含电荷泵使能、末条开显示
    #[test]
    fn init_seq_order() {
        assert_eq!(RR3_SSD1306_INIT_SEQ[0], 0xAE, "先 DISPLAYOFF");
        assert!(
            RR3_SSD1306_INIT_SEQ
                .windows(2)
                .any(|w| w == [0x8D, 0x14].as_slice()),
            "必须使能电荷泵（0x8D, 0x14）"
        );
        assert_eq!(
            RR3_SSD1306_INIT_SEQ[RR3_SSD1306_INIT_SEQ.len() - 1],
            0xAF,
            "最后 DISPLAYON"
        );
        assert_eq!(RR3_SSD1306_INIT_SEQ.len(), 25);
    }

    /// 窗口命令形状：列 0..127、页 0..7（水平寻址模式的整帧窗口）
    #[test]
    fn window_full_panel() {
        assert_eq!(RR3_SSD1306_WINDOW, [0x21, 0x00, 0x7F, 0x22, 0x00, 0x07]);
    }

    /// 空帧 = 全黑（每页 128 列全 0）
    #[test]
    fn blank_frame_zero() {
        assert_eq!(rr3_oled_fb(&[0; 8]), [0u8; 1024]);
    }

    /// 全亮帧 = 页内 [32, 96) 全 0xFF、两侧留白（64x64 居中）
    #[test]
    fn full_frame_64x64() {
        let fb = rr3_oled_fb(&[0xFF; 8]);
        for page in 0..RR3_OLED_PAGES {
            let base = page * RR3_OLED_W;
            assert_eq!(fb[base + 31], 0x00, "page{page} 左留白");
            assert_eq!(fb[base + 32], 0xFF, "page{page} 左边界");
            assert_eq!(fb[base + 95], 0xFF, "page{page} 右边界");
            assert_eq!(fb[base + 96], 0x00, "page{page} 右留白");
        }
    }

    /// 单点放大：帧第 0 行 bit7（最左）只占 page0 的 [32, 40)，其余页全黑
    #[test]
    fn single_8x8_block() {
        let mut frame = [0u8; 8];
        frame[0] = 0x80;
        let fb = rr3_oled_fb(&frame);
        assert!(
            fb[32..40].iter().all(|b| *b == 0xFF),
            "page0 [32,40) 应全亮"
        );
        assert_eq!(fb[31], 0x00);
        assert_eq!(fb[40], 0x00);
        for page in 1..RR3_OLED_PAGES {
            assert!(
                fb[page * RR3_OLED_W..(page + 1) * RR3_OLED_W]
                    .iter()
                    .all(|b| *b == 0),
                "page{page} 应全黑"
            );
        }
    }

    /// 设备名形状（i2c-dev.h `i2c-%d`）：漏连字符会打开不存在的路径，真机首启即
    /// 以"总线不在位"暴露——纯函数钉死命名，不靠现场猜。
    #[test]
    fn bus_i2c_bus_naming() {
        let dev = Rr3Ssd1306I2cDisplay::rr3_ssd1306_new(Path::new("/dev"), 1, 0x3C);
        assert_eq!(dev.bus_dev_path(), PathBuf::from("/dev/i2c-1"));
    }

    /// 错误传播路径（确定性，不触真硬件）：temp 树放普通文件冒充 i2c 总线——
    /// open 成功但 I2C_SLAVE 非字符设备必 ENOTTY，ensure 显式回 InvalidParam
    /// 并点名总线路径；draw 同路径复核
    #[test]
    fn ensure_fail_loudly() {
        let root = std::env::temp_dir().join(format!("rr3-hal-oled-{}-err", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("i2c-1"), "").unwrap();
        let mut out = Rr3Ssd1306I2cDisplay::rr3_ssd1306_new(&root, 1, 0x3C);
        let err = out.frame().unwrap_err();
        assert!(
            matches!(err, Rr3HalError::InvalidParam(ref m)
                if m.rr3_err_text_as_str().contains("i2c-1")),
            "错误要点名总线路径: {err}"
        );
        let err = out.draw(&[0; 8]).unwrap_err();
        assert!(matches!(err, Rr3HalError::InvalidParam(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 占位实例（从未 ensure）Drop 不碰任何 fd
    #[test]
    fn drop_ensure_noop() {
        let root = std::env::temp_dir().join(format!("rr3-hal-oled-{}-drop", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let out = Rr3Ssd1306I2cDisplay::rr3_ssd1306_new(&root, 1, 0x3C);
        drop(out);
        let _ = std::fs::remove_dir_all(&root);
    }
}
