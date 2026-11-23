// SPDX-License-Identifier: Apache-2.0
//! Linux i2c-dev 后端（`/dev/i2c-1`）：ioctl `I2C_RDWR` 的定长消息面。
//!
//! 只在 `target_os = "linux"` 且 64 位宿主编译（父模块 `ports` 的 `mod`
//! 声明携带 cfg）；其余宿主走 `NullI2c`，错误可读且可编译。

use std::os::unix::io::AsRawFd;

use super::I2cTransport;

/// `i2c_msg`（linux/i2c-dev.h，16 字节：addr/flags/len + 指针）。
#[repr(C)]
struct I2cMsg {
    addr: u16,
    flags: u16,
    len: u16,
    buf: *mut u8,
}

/// `i2c_rdwr_ioctl_data`（linux/i2c-dev.h）。
#[repr(C)]
struct I2cRdwrData {
    msgs: *mut I2cMsg,
    nmsgs: u32,
}

// SAFETY：声明与 Linux uapi `ioctl(2)` 原型一致（request 为 unsigned long）；
// 调用点只传本进程打开的合法 fd 与已校验的请求码，错误由返回值判。
extern "C" {
    /// `ioctl(2)`：Linux aarch64/x86_64 的 request 是 `unsigned long`（u64）。
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

/// `I2C_RDWR`（linux/i2c-dev.h：`_IOWR('i', 0x07, struct i2c_rdwr_ioctl_data)`）。
const I2C_RDWR: u64 = 0x0707;
/// 读方向标记（linux/i2c.h：`I2C_M_RD`）。
const I2C_M_RD: u16 = 0x0001;

/// Linux i2c-dev 传输（`/dev/i2c-1`；总线速率由设备树/overlay 决定，端口不重配）。
pub struct LinuxI2c {
    file: std::fs::File,
    path: String,
}

impl LinuxI2c {
    pub fn open(path: &str) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("打开 {path} 失败: {e}"))?;
        Ok(Self {
            file,
            path: path.to_string(),
        })
    }

    fn transfer(&self, msgs: &mut [I2cMsg]) -> Result<(), String> {
        let mut data = I2cRdwrData {
            msgs: msgs.as_mut_ptr(),
            nmsgs: msgs.len() as u32,
        };
        // SAFETY：msgs 与 data 活到调用返回；fd 属 self；request 是 I2C_RDWR 定值。
        let rc = unsafe {
            ioctl(
                self.file.as_raw_fd(),
                I2C_RDWR,
                &mut data as *mut I2cRdwrData,
            )
        };
        if rc < 0 {
            return Err(format!(
                "{} I2C_RDWR 失败（{} 条消息）: {}",
                self.path,
                msgs.len(),
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl I2cTransport for LinuxI2c {
    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > u16::MAX as usize {
            return Err(format!("I2C 写长度越界: {}", bytes.len()));
        }
        let mut msg = I2cMsg {
            addr: addr as u16,
            flags: 0,
            len: bytes.len() as u16,
            buf: bytes.as_ptr() as *mut u8,
        };
        self.transfer(std::slice::from_mut(&mut msg))
    }

    fn read_reg(&mut self, addr: u8, reg: u8, buf: &mut [u8]) -> Result<(), String> {
        if buf.len() > u16::MAX as usize {
            return Err(format!("I2C 读长度越界: {}", buf.len()));
        }
        let mut reg_byte = reg;
        let mut msgs = [
            I2cMsg {
                addr: addr as u16,
                flags: 0,
                len: 1,
                buf: &mut reg_byte as *mut u8,
            },
            I2cMsg {
                addr: addr as u16,
                flags: I2C_M_RD,
                len: buf.len() as u16,
                buf: buf.as_mut_ptr(),
            },
        ];
        self.transfer(&mut msgs)
    }
}
