//! linux（chardev UAPI）GPIO 数字输出后端：LED / 继电器（R11②，0.0.6 方案
//! §5.4 R8 引脚预算表 GPIO4/17 软件路行）。
//!
//! 编译门是单重的：`target_os = "linux"`——依赖 libc 仅取 `ioctl`/`close` 的
//! extern 声明（纯 Rust 壳，musl 静态链接零额外 C 库依赖，R13 友好），无
//! feature 门（与 [`crate::linux`] 的 alsa 双门不同）。主机 CI（macOS）不编译
//! 本文件；宿主侧验证路径 = aarch64 musl 交叉 check（R13）+ 板上
//! `cargo test -p rr-hal`（真 linux 目标跑单测；ioctl 面无法假树模拟，确定性
//! 单测 = 常量/布局断言 + 对 temp 树普通文件的错误传播路径，不触真硬件）。
//! 真件档验收（真 LED/继电器到位后的电平实测）随 R9 矩阵对应行（方案 §5.2）。
//!
//! 接口面：GPIO chardev UAPI v1（`/dev/gpiochipN`，内核 4.8+ 现役 UAPI）——
//! sysfs gpio 已在 6.12 内核弃用（CONFIG_GPIO_SYSFS 可能未编），不采用。
//! 请求 `GPIO_GET_LINEHANDLE_IOCTL` 取 OUTPUT 句柄（v1 句柄独占 line），
//! `GPIOHANDLE_SET/GET_LINE_VALUES_IOCTL` 写/读电平。芯片经 `RR3_HAL_GPIO_CHIP`
//! 选（缺省 0）；线偏移按角色经 `RR3_HAL_GPIO_LED_LINE` / `RR3_HAL_GPIO_RELAY_LINE`
//! 选（缺省 4 / 17 = R8 预算表软件路行；接线手册未单列 LED/继电器行，真件档
//! 装配单如改脚位走 env 覆写，代码缺省不动）。
//!
//! boot 语义（R11②）：真件档装配单显式 `RR3_HAL_BACKEND=linux` 后，模块开机
//! 探活读 [`crate::Rr3GpioOut::level`] 触发首次 ensure——chip 不在位、线被
//! 其他进程占用（v1 句柄独占，EBUSY）在此显式报错，rr3-act 按"能力缺失显式
//! 回错" exit(1)，不静默退化。请求 OUTPUT 句柄时落初值 0（灭/断开）：与 sim
//! 后端"初始 false"同语义（探活读回一致，R11 换装判据"编排与契约零改动"），
//! 也避免沿用上一进程遗留电平起动。进程退出（Drop）close 句柄即释放 line
//! （内核语义），电平回落由硬件侧上下拉定，不在此约定。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{Rr3ErrText, Rr3GpioOut, Rr3HalError};

/// GPIO chardev 设备根（构造参数是测试/夹具的根目录接缝，不另设环境变量；
/// 真实路径 = `{root}/gpiochip{N}`）
pub const RR3_GPIO_DEV_ROOT_DEFAULT: &str = "/dev";

/// chardev UAPI 的 ioctl 族号（linux/gpio.h：`GPIO_IOC_MAGIC = 0xB4`）
const GPIO_IOC_MAGIC: u32 = 0xB4;
/// generic-abi 的 _IOC 方向位（aarch64 与 x86_64 同构：DIRSHIFT=30，RW=READ|WRITE）
const IOC_RW: u32 = 3;

/// `struct gpiohandle_request`（linux/gpio.h）：64 路偏移 + 标志 + 初值 +
/// consumer 标签 + 路数 + 返回句柄 fd。#[repr(C)] 布局是 ioctl 契约，
/// 字段顺序/大小/对齐有单测钉死，漂移即失败。
#[repr(C)]
struct Rr3GpioHandleRequest {
    lineoffsets: [u32; 64],
    flags: u32,
    default_values: [u8; 64],
    consumer_label: [u8; 32],
    lines: u32,
    fd: std::os::raw::c_int,
}

/// `struct gpiohandle_data`（linux/gpio.h）：读写电平的字节组
#[repr(C)]
struct Rr3GpioHandleData {
    values: [u8; 64],
}

/// `GPIOHANDLE_REQUEST_OUTPUT`（linux/gpio.h）：输出模式，初值取 default_values
const GPIOHANDLE_REQUEST_OUTPUT: u32 = 2;

/// _IOC 编码（asm-generic/ioctl.h 的通用式；aarch64/x86_64 同构）。
/// 单测把三个硬编码参考值对齐内核头文件常量，ABI 漂移在此暴露。
const fn rr3_ioc(dir: u32, ty: u32, nr: u32, size: u32) -> u32 {
    (dir << 30) | (size << 16) | (ty << 8) | nr
}

/// `GPIO_GET_LINEHANDLE_IOCTL` = _IOWR(0xB4, 0x03, gpiohandle_request)
const GPIO_GET_LINEHANDLE_IOCTL: u32 = rr3_ioc(IOC_RW, GPIO_IOC_MAGIC, 0x03, 364);
/// `GPIOHANDLE_GET_LINE_VALUES_IOCTL` = _IOWR(0xB4, 0x08, gpiohandle_data)
const GPIOHANDLE_GET_LINE_VALUES_IOCTL: u32 = rr3_ioc(IOC_RW, GPIO_IOC_MAGIC, 0x08, 64);
/// `GPIOHANDLE_SET_LINE_VALUES_IOCTL` = _IOWR(0xB4, 0x09, gpiohandle_data)
const GPIOHANDLE_SET_LINE_VALUES_IOCTL: u32 = rr3_ioc(IOC_RW, GPIO_IOC_MAGIC, 0x09, 64);

/// errno → HAL 错误：line 被其他进程持有（v1 句柄独占，EBUSY）是
/// [`Rr3HalError::Busy`]，其余（chip 不在位 / 权限不足 / line 越界）是
/// [`Rr3HalError::InvalidParam`] 带点名。
fn rr3_gpio_ioctl_err(context: &str, errno: libc::c_int) -> Rr3HalError {
    if errno == libc::EBUSY {
        Rr3HalError::Busy
    } else {
        let detail = std::io::Error::from_raw_os_error(errno);
        Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
            "{context}: {detail}（errno {errno}）"
        )))
    }
}

/// linux chardev GPIO 数字输出（[`crate::Rr3GpioOut`] 的 linux 后端）。
///
/// 选择函数（[`crate::backend::rr3_hal_gpio_out`]）不触设备：构造只算路径，
/// 打开芯片与请求 OUTPUT 句柄延迟到首次 trait 调用（与 pwm/alsa 后端 open
/// 延迟同形）。
pub struct Rr3LinuxGpioOut {
    dev_root: PathBuf,
    chip: u32,
    line: u32,
    /// consumer 标签（请求里的 consumer_label，lsof/fuser 可见，运维面）
    role: String,
    /// None = 尚未 ensure；Some(fd) = OUTPUT 句柄已在位（Drop close 释放 line）
    handle_fd: Mutex<Option<std::os::raw::c_int>>,
}

impl Rr3LinuxGpioOut {
    /// 指定根/芯片/线/角色构造（选择函数从生效物理绑定取 `gpio_chip` 与该角色的
    /// `gpio_<role>_line`，批D④ fail-closed——芯片/线偏移没有代码缺省，描述符/
    /// env 都没给就在选择面点名；测试用 temp 树也走这里，不触真硬件）
    pub fn rr3_linux_gpio_out_new(dev_root: &Path, chip: u32, line: u32, role: &str) -> Self {
        Self {
            dev_root: dev_root.to_path_buf(),
            chip,
            line,
            role: role.to_string(),
            handle_fd: Mutex::new(None),
        }
    }

    fn chip_dev_path(&self) -> PathBuf {
        self.dev_root.join(format!("gpiochip{}", self.chip))
    }

    /// 首次使用：打开芯片设备 + 请求 OUTPUT 句柄（初值 0）。`slot` 由调用方
    /// 在持锁状态下传入——本函数只在 ensure 路径上跑一次。
    fn ensure(&self, slot: &mut Option<std::os::raw::c_int>) -> Result<(), Rr3HalError> {
        if slot.is_some() {
            return Ok(());
        }
        // open 走 std::fs::File（Rust 面管关闭）；句柄 fd 是内核另行分配的
        // 新 fd（request.fd 回填），chip 的 fd 用完即随 File drop 关闭。
        let chip_file = std::fs::File::open(self.chip_dev_path()).map_err(|e| {
            Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(&format!(
                "gpio 打开 {:?} 失败: {e}（芯片不在位或权限不足）",
                self.chip_dev_path().display()
            )))
        })?;
        let mut req = Rr3GpioHandleRequest {
            lineoffsets: [0; 64],
            flags: GPIOHANDLE_REQUEST_OUTPUT,
            default_values: [0; 64],
            consumer_label: [0; 32],
            lines: 1,
            fd: -1,
        };
        req.lineoffsets[0] = self.line;
        for (i, b) in self.role.bytes().take(32).enumerate() {
            req.consumer_label[i] = b;
        }
        // SAFETY：req 是合法可写指针，ioctl 号与结构体布局配对（单测钉死）；
        // 成功时内核回填 req.fd（新句柄 fd）。errno 必须在 File drop（close
        // 可能改写 errno）之前取出。
        let ret = unsafe {
            libc::ioctl(
                std::os::fd::AsRawFd::as_raw_fd(&chip_file),
                GPIO_GET_LINEHANDLE_IOCTL as _, // glibc 声明 c_ulong / musl 声明 c_int，按平台推断
                &mut req as *mut Rr3GpioHandleRequest,
            )
        };
        let errno = if ret < 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
        } else {
            0
        };
        drop(chip_file);
        if ret < 0 {
            return Err(rr3_gpio_ioctl_err(
                &format!(
                    "gpio 请求 {:?} line{} 输出句柄失败（line 被占或越界）",
                    self.chip_dev_path().display(),
                    self.line
                ),
                errno,
            ));
        }
        *slot = Some(req.fd);
        Ok(())
    }

    /// 对在位句柄发 GET/SET 电平 ioctl（ensure 已过，fd 必在位）。
    /// errno 同样在无中间 libc 调用的窗口内取出。
    fn line_values(
        &self,
        ioctl: u32,
        data: &mut Rr3GpioHandleData,
        what: &str,
    ) -> Result<(), Rr3HalError> {
        let fd = self
            .handle_fd
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .expect("ensure 已过，句柄必在位");
        // SAFETY：data 是合法可写指针，尺寸与 ioctl 号配对（单测钉死）
        let ret = unsafe { libc::ioctl(fd, ioctl as _, data as *mut Rr3GpioHandleData) };
        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(rr3_gpio_ioctl_err(what, errno));
        }
        Ok(())
    }
}

impl Drop for Rr3LinuxGpioOut {
    fn drop(&mut self) {
        let fd = self
            .handle_fd
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(fd) = fd {
            // close 句柄 fd 即释放 line（内核语义）；错误无从上报，忽略——
            // 真件档验收关注板上实况
            unsafe {
                libc::close(fd);
            }
        }
    }
}

impl Rr3GpioOut for Rr3LinuxGpioOut {
    fn set(&mut self, level: bool) -> Result<(), Rr3HalError> {
        let mut guard = self.handle_fd.lock().unwrap_or_else(|p| p.into_inner());
        self.ensure(&mut guard)?;
        drop(guard);
        let mut data = Rr3GpioHandleData { values: [0; 64] };
        data.values[0] = level as u8;
        self.line_values(
            GPIOHANDLE_SET_LINE_VALUES_IOCTL,
            &mut data,
            "gpio 写电平失败",
        )
    }

    fn level(&self) -> Result<bool, Rr3HalError> {
        let mut guard = self.handle_fd.lock().unwrap_or_else(|p| p.into_inner());
        self.ensure(&mut guard)?;
        drop(guard);
        let mut data = Rr3GpioHandleData { values: [0; 64] };
        self.line_values(
            GPIOHANDLE_GET_LINE_VALUES_IOCTL,
            &mut data,
            "gpio 读电平失败",
        )?;
        Ok(data.values[0] != 0)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// ioctl 号与内核 linux/gpio.h 常量硬编码对齐（aarch64/x86_64 generic-abi）：
    /// _IOWR(0xB4, 0x03, 364) 等——布局漂移或编码式写错在此最先暴露
    #[test]
    fn ioctl_numbers_match_kernel_uapi() {
        assert_eq!(GPIO_GET_LINEHANDLE_IOCTL, 0xC16C_B403);
        assert_eq!(GPIOHANDLE_GET_LINE_VALUES_IOCTL, 0xC040_B408);
        assert_eq!(GPIOHANDLE_SET_LINE_VALUES_IOCTL, 0xC040_B409);
    }

    /// #[repr(C)] 结构体布局 = ioctl 契约：大小/对齐漂移即失败
    #[test]
    fn request_and_data_layout_matches_uapi() {
        // gpiohandle_request：64*4 + 4 + 64 + 32 + 4 + 4 = 364，对齐 4
        assert_eq!(std::mem::size_of::<Rr3GpioHandleRequest>(), 364);
        assert_eq!(std::mem::align_of::<Rr3GpioHandleRequest>(), 4);
        // gpiohandle_data：64 字节，对齐 1
        assert_eq!(std::mem::size_of::<Rr3GpioHandleData>(), 64);
        assert_eq!(std::mem::align_of::<Rr3GpioHandleData>(), 1);
    }

    /// 错误传播路径（确定性，不触真硬件）：temp 树放普通文件冒充 chip 设备——
    /// open 成功但 ioctl 非字符设备必 ENOTTY，ensure 显式回 InvalidParam，
    /// 且 chip fd 随 File drop 关闭（无泄漏面）
    #[test]
    fn ensure_on_non_chardev_fails_loudly() {
        let root = std::env::temp_dir().join(format!("rr3-hal-gpio-{}-err", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("gpiochip0"), "").unwrap();
        let mut out = Rr3LinuxGpioOut::rr3_linux_gpio_out_new(&root, 0, 4, "led");
        // 探活读面 &self；写面 set 是 &mut self，绑定取 mut
        let err = out.level().unwrap_err();
        assert!(
            matches!(err, Rr3HalError::InvalidParam(ref m)
                if m.rr3_err_text_as_str().contains("gpiochip0")),
            "错误要点名芯片路径: {err}"
        );
        // set 同路径：同一实例第二次调用仍在 ensure 处回错（句柄未建立）
        let err = out.set(true).unwrap_err();
        assert!(matches!(err, Rr3HalError::InvalidParam(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 占位实例（从未 ensure）Drop 不碰任何 fd：构造后直接丢弃不炸
    #[test]
    fn drop_without_ensure_is_noop() {
        let root = std::env::temp_dir().join(format!("rr3-hal-gpio-{}-drop", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let out = Rr3LinuxGpioOut::rr3_linux_gpio_out_new(&root, 0, 4, "relay");
        drop(out);
        let _ = std::fs::remove_dir_all(&root);
    }
}
