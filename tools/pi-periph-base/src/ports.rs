//! 五端口实现（Pi 4B 档）——**只做事务级动作，不做策略**：
//!
//! | 端口 | 实现 | 边界 |
//! |---|---|---|
//! | UART | 复用 [`rr_ipc::Rr3UartConn`]（raw termios + 排他开口） | 帧面/重试全在上层 |
//! | I2C | Linux `i2c-dev` + `I2C_RDWR` ioctl | 一笔写/一笔读；重试与 OLED 分块在上层 |
//! | PWM | sysfs `/sys/class/pwm/pwmchip0/pwm{0,1}` | 只换算并写 duty/enable；export/period 在 `ensure` |
//! | ADC | ADS1115（i2c1, 0x48）单端 A0，2:1 分压，±4.096V | 无器件如实 Err 且点名，不冒领读数 |
//! | Clock | `CLOCK_MONOTONIC`（Linux）/ `Instant`（非 Linux 编译面） | 帧头 timestamp 用 |
//!
//! 平台可编面：Linux 设备代码用 `#[cfg(target_os = "linux")]` 隔离；纯换算、
//! PWM sysfs 文件面与 ADS1115 字节协议在 macOS 上也跑单测（门禁 ④/⑥ 在宿主跑）。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use periph_mech::rail_judge::rr3_rail_mv_from_pin;
use rr_ipc::Rr3UartConn;
use rr_periph_proto::ports::{Rr3AdcPort, Rr3ClockPort, Rr3I2cPort, Rr3PwmChannel, Rr3PwmPort};

use crate::board::{
    env_str, env_u32, pwm_ticks_to_ns, ADS1115_CONFIG, ADS1115_RAIL_CHANNEL, ADS1115_WAIT_MS,
    ENV_PWM_ROOT, ENV_UART_BAUD, ENV_UART_DEV, PWM_CHANNELS, PWM_PERIOD_NS, PWM_ROOT,
    RAIL_DIVIDER_DEN, RAIL_DIVIDER_NUM, UART_BAUD, UART_DEV,
};

// ---------------------------------------------------------------------------
// UART
// ---------------------------------------------------------------------------

/// 打开板↔宿主串口（缺省 `/dev/serial0` @1.5M；env 应急覆盖）。
/// 实现体是 `rr-ipc` 的 [`Rr3UartConn`]：raw termios / 开口净化 / 排他 flock /
/// 空闲前导都在那一处；本档只选路径与波特率。
pub fn open_default_uart() -> Result<Rr3UartConn, String> {
    let path = env_str(ENV_UART_DEV, UART_DEV);
    let baud = env_u32(ENV_UART_BAUD, UART_BAUD);
    Rr3UartConn::open_with_baud(Path::new(&path), baud)
        .map_err(|e| format!("串口 {path} 打开失败（波特 {baud}）: {e}"))
}

// ---------------------------------------------------------------------------
// I2C：可注入传输（Linux i2c-dev / 测试假件）
// ---------------------------------------------------------------------------

/// I2C 传输事务（端口底层）：一笔写 + 一笔"写寄存器指针后读"。
/// `Rr3I2cPort` 只露写；ADS1115 需要组合读，故内部传输多一条 `read_reg`。
pub trait I2cTransport {
    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String>;
    fn read_reg(&mut self, addr: u8, reg: u8, buf: &mut [u8]) -> Result<(), String>;
}

/// 共享 I2C 总线（OLED 与 ADS1115 同一 `/dev/i2c-1`）：单线程 `Rc<RefCell<..>>`，
/// 不跨线程（板侧控制面独占 I2C）。
#[derive(Clone)]
pub struct SharedI2c {
    inner: Rc<RefCell<Box<dyn I2cTransport>>>,
}

impl SharedI2c {
    pub fn new<T: I2cTransport + 'static>(t: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Box::new(t))),
        }
    }

    /// 不可用总线（打开失败/非 Linux 宿主）：所有事务回 Err 并带原因，不 panic。
    pub fn null(reason: &str) -> Self {
        Self::new(NullI2c {
            reason: reason.to_string(),
        })
    }

    pub fn write(&self, addr: u8, bytes: &[u8]) -> Result<(), String> {
        self.inner.borrow_mut().write(addr, bytes)
    }

    pub fn read_reg(&self, addr: u8, reg: u8, buf: &mut [u8]) -> Result<(), String> {
        self.inner.borrow_mut().read_reg(addr, reg, buf)
    }
}

impl Rr3I2cPort for SharedI2c {
    fn rr3_i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String> {
        self.write(addr, bytes)
    }
}

struct NullI2c {
    reason: String,
}

impl I2cTransport for NullI2c {
    fn write(&mut self, _addr: u8, _bytes: &[u8]) -> Result<(), String> {
        Err(format!("I2C 不可用: {}", self.reason))
    }

    fn read_reg(&mut self, _addr: u8, _reg: u8, _buf: &mut [u8]) -> Result<(), String> {
        Err(format!("I2C 不可用: {}", self.reason))
    }
}

/// 打开缺省 I2C 总线：Linux 走 i2c-dev；其余宿主回 Null 总线（可编译、错误可读）。
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
pub fn open_default_i2c() -> SharedI2c {
    let path = env_str(crate::board::ENV_I2C_DEV, crate::board::I2C_DEV);
    match linux_i2c::LinuxI2c::open(&path) {
        Ok(bus) => SharedI2c::new(bus),
        Err(e) => {
            eprintln!("[periph] I2C 打开失败: {e}");
            SharedI2c::null(&e)
        }
    }
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
pub fn open_default_i2c() -> SharedI2c {
    let path = env_str(crate::board::ENV_I2C_DEV, crate::board::I2C_DEV);
    SharedI2c::null(&format!(
        "非 Linux 64 位宿主：i2c-dev 不可用（目标设备 {path}）"
    ))
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod linux_i2c {
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
}

// ---------------------------------------------------------------------------
// PWM：sysfs（可注入根，单测用临时树）
// ---------------------------------------------------------------------------

/// sysfs PWM 端口：`root/pwm{0,1}` 两个通道；首次写即 export + period 配置。
pub struct PwmPort {
    root: PathBuf,
    ensured: [bool; 2],
}

impl PwmPort {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            ensured: [false; 2],
        }
    }

    pub fn default_root() -> Self {
        Self::new(env_str(ENV_PWM_ROOT, PWM_ROOT))
    }

    fn ensure(&mut self, idx: usize) -> Result<(), String> {
        if self.ensured[idx] {
            return Ok(());
        }
        let dir = self.root.join(PWM_CHANNELS[idx]);
        if !dir.exists() {
            let export = self.root.join("export");
            std::fs::write(&export, format!("{idx}\n")).map_err(|e| {
                format!(
                    "pwm export 通道 {idx} 写 {} 失败（dtoverlay=pwm-2chan 未开或权限不足）: {e}",
                    export.display()
                )
            })?;
            let t0 = Instant::now();
            while !dir.exists() {
                if t0.elapsed() >= Duration::from_millis(50) {
                    return Err(format!(
                        "pwm 导出后通道目录 {} 未在 50ms 内出现（内核 sysfs 异常）",
                        dir.display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if read_u64(&dir.join("period")).unwrap_or(0) != PWM_PERIOD_NS as u64 {
            write_u64(&dir.join("duty_cycle"), 0)?;
            write_u64(&dir.join("period"), PWM_PERIOD_NS as u64)?;
        }
        self.ensured[idx] = true;
        Ok(())
    }
}

impl Rr3PwmPort for PwmPort {
    fn rr3_pwm_duty(&mut self, channel: Rr3PwmChannel, duty: u32) -> Result<(), String> {
        let idx = match channel {
            Rr3PwmChannel::Pan => 0,
            Rr3PwmChannel::Tilt => 1,
        };
        self.ensure(idx)?;
        let dir = self.root.join(PWM_CHANNELS[idx]);
        let ns = pwm_ticks_to_ns(duty);
        write_u64(&dir.join("duty_cycle"), ns as u64)?;
        write_u64(&dir.join("enable"), if ns == 0 { 0 } else { 1 })
    }
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

fn write_u64(path: &Path, val: u64) -> Result<(), String> {
    std::fs::write(path, format!("{val}\n"))
        .map_err(|e| format!("写 {} = {val} 失败: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// ADC：ADS1115
// ---------------------------------------------------------------------------

/// ADS1115 单端 A0 采信：写配置寄存器 → 等转换 → 读转换寄存器 → mV 换算。
/// 无器件（NACK/总线不可用）由传输层回 Err，本层原样上抛并点名——**不冒领读数**。
pub struct Ads1115 {
    bus: SharedI2c,
    addr: u8,
}

impl Ads1115 {
    pub fn new(bus: SharedI2c, addr: u8) -> Self {
        Self { bus, addr }
    }

    /// 采一次：返回 `(5V 轨 mV, A0 引脚 raw)`。
    pub fn read_rail(&mut self) -> Result<(u16, u16), String> {
        self.bus
            .write(self.addr, &[0x01, ADS1115_CONFIG[0], ADS1115_CONFIG[1]])
            .map_err(|e| format!("ADS1115@{:#04x} 配置写失败: {e}", self.addr))?;
        std::thread::sleep(Duration::from_millis(ADS1115_WAIT_MS));
        let mut raw = [0u8; 2];
        self.bus
            .read_reg(self.addr, 0x00, &mut raw)
            .map_err(|e| format!("ADS1115@{:#04x} 转换读失败: {e}", self.addr))?;
        let raw_u = u16::from_be_bytes(raw);
        let raw_i = raw_u as i16;
        let pin_raw = raw_i.max(0) as u16;
        // ±4.096V 档：1 LSB = 4.096V / 32768 = 0.125mV；整数式先算 mV。
        let pin_mv = ((pin_raw as u32) * 4096 / 32768) as u16;
        let rail_mv = rr3_rail_mv_from_pin(pin_mv, RAIL_DIVIDER_NUM, RAIL_DIVIDER_DEN);
        Ok((rail_mv, pin_raw))
    }
}

/// 轨压传感器（ADS1115 或"缺席"态）：缺席也保留原因，消费面 Err 可读、不 panic。
pub enum RailSensor {
    Ads(Ads1115),
    Absent(String),
}

impl RailSensor {
    /// 上电探测：能读一次即认为在位；失败转 `Absent` 并点名（不阻断其余端口）。
    pub fn probe(bus: SharedI2c, addr: u8) -> Self {
        let mut ads = Ads1115::new(bus, addr);
        match ads.read_rail() {
            Ok(_) => Self::Ads(ads),
            Err(e) => {
                eprintln!("[periph] ADS1115 未应答（rail 请求将回 Err）: {e}");
                Self::Absent(e)
            }
        }
    }

    pub fn sample(&mut self) -> Result<(u16, u16), String> {
        match self {
            Self::Ads(ads) => ads.read_rail(),
            Self::Absent(reason) => Err(format!("ADS1115 未装配: {reason}")),
        }
    }
}

impl Rr3AdcPort for RailSensor {
    fn rr3_adc_read_mv(&mut self, pin: u8) -> Result<(u16, u16), String> {
        if pin != ADS1115_RAIL_CHANNEL {
            return Err(format!(
                "ADC 端口仅装配 ADS1115 A0（通道 {ADS1115_RAIL_CHANNEL}），拒绝通道 {pin}"
            ));
        }
        self.sample()
    }
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

/// 单调时钟端口（帧头 timestamp 诊断用；epoch 无关）。
pub struct MonotonicClock;

impl Rr3ClockPort for MonotonicClock {
    fn rr3_now_ns(&self) -> u64 {
        now_ns()
    }
}

#[cfg(target_os = "linux")]
fn now_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    extern "C" {
        fn clock_gettime(clk_id: i32, tp: *mut Timespec) -> i32;
    }
    /// Linux `CLOCK_MONOTONIC`（uapi linux/time.h）。
    const CLOCK_MONOTONIC: i32 = 1;
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY：ts 是本栈合法内存；只读单调时钟。
    let rc = unsafe { clock_gettime(CLOCK_MONOTONIC, &mut ts) };
    if rc == 0 {
        (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64
    } else {
        0
    }
}

#[cfg(not(target_os = "linux"))]
fn now_ns() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// 测试假件（ports 与 oled 单测共用）：记录写事务、可注入读回/错误。
#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{I2cTransport, SharedI2c};

    #[derive(Default)]
    pub(crate) struct FakeI2c {
        pub(crate) writes: Vec<(u8, Vec<u8>)>,
        pub(crate) reads: Vec<(u8, u8)>,
        pub(crate) read_data: Vec<u8>,
        pub(crate) write_err: Option<String>,
        pub(crate) read_err: Option<String>,
    }

    impl I2cTransport for Rc<RefCell<FakeI2c>> {
        fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), String> {
            let mut f = self.borrow_mut();
            f.writes.push((addr, bytes.to_vec()));
            match &f.write_err {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        }

        fn read_reg(&mut self, addr: u8, reg: u8, buf: &mut [u8]) -> Result<(), String> {
            let mut f = self.borrow_mut();
            f.reads.push((addr, reg));
            if let Some(e) = &f.read_err {
                return Err(e.clone());
            }
            assert_eq!(buf.len(), f.read_data.len(), "假件读长度须与注入数据同长");
            buf.copy_from_slice(&f.read_data);
            Ok(())
        }
    }

    pub(crate) fn fake() -> (SharedI2c, Rc<RefCell<FakeI2c>>) {
        let f = Rc::new(RefCell::new(FakeI2c::default()));
        let bus = SharedI2c::new(f.clone());
        (bus, f)
    }
}

#[cfg(test)]
mod tests;
