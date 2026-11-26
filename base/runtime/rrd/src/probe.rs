// SPDX-License-Identifier: Apache-2.0
//! 装配链硬件探测面：探测 fail-closed——探测不到与探测自身故障同判 `false`（缺格），
//! 装配链对点名模块"绝不带病起"。拿不到答案不得当作在位。boot 前消费一次，运行期
//! 不重排，不阻塞启动横幅。
//!
//! 后端三档：[`Rr3LinuxProbe`] 生产 linux 真值探测——sysfs PWM / GPIO line / I2C /
//! SPI / ALSA 五面在位性 + I2C 格位器件级探测（总线在 ≠ 器件在）；探测根可注入，
//! 生产传 `/`、单测传假根，判据主机可测。[`Rr3AbsentProbe`] 非 linux 生产面——
//! 有硬件面的格位恒缺，无硬件面可反证者判在位。[`Rr3SimProbe`] 显式档，只在 test、
//! `RR3_PROBE=sim` 与显式注入时用，不再当缺省构造。
//!
//! 选择口 [`rr3_probe_default`] 与 `RR3_HAL_BACKEND` 同纪律：env 显式优先
//! （`RR3_PROBE=sim|linux|absent`，取值不认点名后回落），否则按目标分流（test→sim；
//! linux→真值；其余生产面→恒缺）。
//!
//! 格位→面映射 [`rr3_slot_probe_face`]：有硬件面按面探；`cognition.*` 软件格、
//! `net.wifi`/`net.bt` 运行期格与基础层回"在位"——"没有反证"与"拿不到答案"不同判。

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 一台机器的"格位在位吗"探测面。装配链每 boot 构造一次、逐格询问。
pub trait Rr3Probe: Send + Sync {
    fn rr3_probe_slot(&self, slot: &str) -> bool;

    /// 器件级探测（L-14，增补轮六批N1）：I2C 格位在装机档 `[pins]` 在册
    /// `(bus, addr)` 时，在位判据升为"内核已为该从机地址注册器件节点"
    /// （sysfs `devices/<bus>-<addr4hex>`），总线级判据降为**前置条件**——
    /// "总线在 ≠ 器件在"（L-14：只看 `/dev/i2c-*` 存在会让器件没接也恒绿）。
    /// `bus`/`addr` 任一缺席（装配单未声明 `[pins]` / 该格无 I2C 绑定）→
    /// 回落五面判据原样（无 [pins] 形态的兼容面；缺格点名仍走装配链"缺格"）。
    /// 读不到答案一律 `false`（fail-closed 与 [`Rr3Probe::rr3_probe_slot`] 同判）。
    fn rr3_probe_slot_bound(&self, slot: &str, _bus: Option<u32>, _addr: Option<&str>) -> bool {
        self.rr3_probe_slot(slot)
    }
}

/// 探测面五面（MC-4）：格位归到哪张硬件面，决定探什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ProbeFace {
    /// sysfs PWM 通道（`/sys/class/pwm/pwmchip*`）
    Pwm,
    /// chardev/sysfs GPIO（`/sys/class/gpio/gpiochip*`）
    Gpio,
    /// I2C 设备节点（`/dev/i2c-*`）
    I2c,
    /// SPI 设备节点（`/dev/spidev*`）
    Spi,
    /// ALSA 声卡注册（`/proc/asound/cards`）
    Alsa,
}

/// 目录型探测面表：面 → (根下目录, 条目前缀)。
const RR3_FACE_DIRS: &[(Rr3ProbeFace, &str, &str)] = &[
    (Rr3ProbeFace::Pwm, "sys/class/pwm", "pwmchip"),
    (Rr3ProbeFace::Gpio, "sys/class/gpio", "gpiochip"),
    (Rr3ProbeFace::I2c, "dev", "i2c-"),
    (Rr3ProbeFace::Spi, "dev", "spidev"),
];

/// 格位 → 探测面映射表（同一面可由多个格位共用；表内格位唯一）。
const RR3_SLOT_FACES: &[(&str, Rr3ProbeFace)] = &[
    ("act.servo", Rr3ProbeFace::Pwm),
    ("act.led", Rr3ProbeFace::Gpio),
    ("act.relay", Rr3ProbeFace::Gpio),
    ("ui.face", Rr3ProbeFace::Spi),
    ("power.charger", Rr3ProbeFace::I2c),
    ("sensor.env", Rr3ProbeFace::I2c),
    ("sensor.imu", Rr3ProbeFace::I2c),
    ("audio.capture", Rr3ProbeFace::Alsa),
    ("audio.playback", Rr3ProbeFace::Alsa),
    ("audio.vad", Rr3ProbeFace::Alsa),
];

/// 格位 → 探测面。`None` = 无硬件面可反证（软件格 / 运行期格 / 基础层），
/// 探测对它们回"在位"（见文件头末段）。
pub fn rr3_slot_probe_face(slot: &str) -> Option<Rr3ProbeFace> {
    // cognition.*（软件格）、net.wifi/net.bt（运行期/占位格）、基础层 bus.*/
    // net.online/power.battery：无硬件面，无反证 → 在位。
    RR3_SLOT_FACES
        .iter()
        .find(|(name, _)| *name == slot)
        .map(|(_, face)| *face)
}

/// linux 参数真值探测（MC-4）：五面在位性，探测根可注入。
pub struct Rr3LinuxProbe {
    root: PathBuf,
}

impl Rr3LinuxProbe {
    /// 指定探测根构造：生产传 `/`（[`rr3_probe_default`] 的 linux 分支），
    /// 单测传假根——判据主机可测，且不硬编码任何板的引脚/总线号。
    pub fn rr3_linux_probe_new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 目录下存在任一以 `prefix` 开头的条目（设备节点/芯片目录的泛型探法——
    /// 不钉 `i2c-1`/`spidev0.0` 这类板专属号，换板不改代码）。
    fn rr3_dir_has_prefix(dir: &Path, prefix: &str) -> bool {
        // 读不到目录 = 探测不到 = 缺（fail-closed，不当作在位）
        let Ok(rd) = std::fs::read_dir(dir) else {
            return false;
        };
        rd.flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(prefix))
    }

    /// 文件内容非空（ALSA 卡表有内容 = 至少注册了一张卡）。**按内容不按 stat
    /// 尺寸**：procfs 文件 stat 尺寸恒 0（真板实测 `/proc/asound/cards` size=0
    /// 而 cat 有三卡），按 metadata.len() 判会让真板音频格恒判"探测不到"。
    fn rr3_file_nonempty(path: &Path) -> bool {
        // 读不到文件 = 探测不到 = 缺（fail-closed，与目录读失败同判）
        std::fs::read(path).map(|b| !b.is_empty()).unwrap_or(false)
    }

    /// 装机档 `addr` → sysfs 器件节点词形（内核约定：`<bus>-<7位地址4位小写16进制>`，
    /// 如 `1-0040`）。"0x" 前缀认、越出 7 位地址域（>0x7F）不认——词形解析失败
    /// = 拿不到答案 = 缺（fail-closed，不回落总线级冒充在位）。
    fn rr3_i2c_addr_word(addr: &str) -> Option<String> {
        let v = Self::rr3_i2c_addr_hex(addr)?;
        Some(format!("{v:04x}"))
    }

    fn rr3_i2c_addr_hex(addr: &str) -> Option<u32> {
        let hex = addr
            .strip_prefix("0x")
            .or_else(|| addr.strip_prefix("0X"))?;
        let v = u32::from_str_radix(hex, 16).ok()?;
        if v > 0x7F {
            return None;
        }
        Some(v)
    }

    fn rr3_i2c_node(&self, n: u32, a: &str) -> bool {
        let Some(word) = Self::rr3_i2c_addr_word(a) else {
            return false; // 词形解析失败 = 拿不到答案 = 缺（fail-closed）
        };
        // 前置条件：该具体总线的设备节点在（n 由装机档 controller 词形
        // i2c<N> 数据驱动，非硬编码板号）；器件级：sysfs 从机节点在。
        self.root.join("dev").join(format!("i2c-{n}")).exists()
            && self
                .root
                .join("sys/bus/i2c/devices")
                .join(format!("{n}-{word}"))
                .exists()
    }

    /// 面在位性。任一面拿不到答案 → `false`（fail-closed）。
    fn rr3_face_present(&self, face: Rr3ProbeFace) -> bool {
        if face == Rr3ProbeFace::Alsa {
            return Self::rr3_file_nonempty(&self.root.join("proc/asound/cards"));
        }
        RR3_FACE_DIRS
            .iter()
            .find(|(f, _, _)| *f == face)
            .is_some_and(|(_, dir, prefix)| Self::rr3_dir_has_prefix(&self.root.join(dir), prefix))
    }
}

impl Rr3Probe for Rr3LinuxProbe {
    fn rr3_probe_slot(&self, slot: &str) -> bool {
        match rr3_slot_probe_face(slot) {
            Some(face) => self.rr3_face_present(face),
            // 无硬件面可反证：在位（软件格/运行期格/基础层——见文件头末段）
            None => true,
        }
    }

    /// L-14 器件级探测：I2C 格位 + (bus, addr) 在册 → 在位 = 总线节点在（前置
    /// 条件，收窄到该具体总线）**且** sysfs 器件节点在（`devices/<bus>-<addr4>`）。
    /// 器件节点不存在 = 内核没为该地址注册从机 = 总线在、器件不在 → 缺。
    /// 寄存器 ID 读回比对（INA219 0x00 = 0x399F）需 i2c-dev 事务、会碰器件，
    /// 归器件与真板批（status 敞口在案）——本判据不冒充它是读回。
    fn rr3_probe_slot_bound(&self, slot: &str, bus: Option<u32>, addr: Option<&str>) -> bool {
        match (rr3_slot_probe_face(slot), bus, addr) {
            (Some(Rr3ProbeFace::I2c), Some(n), Some(a)) => self.rr3_i2c_node(n, a),
            _ => self.rr3_probe_slot(slot),
        }
    }
}

/// sim 后端：全在位。**显式档**——sim 机无硬件可探，缺格只由装配单缺 binding
/// 表达；MC-4 起不再是缺省构造（`rr3_probe_default` 非 test 面按目标分流），
/// 只在 test 面与显式注入里用。
pub struct Rr3SimProbe;

impl Rr3Probe for Rr3SimProbe {
    fn rr3_probe_slot(&self, _slot: &str) -> bool {
        true
    }
}

/// 非 linux 生产面（本机 macOS 直跑 rrd）：**没有可用的硬件探测面**——有硬件面的
/// 格位恒缺（拿不到答案**不当作**在位，fail-closed）；无硬件面可反证的格位（软件格
/// `cognition.*` / 运行期格 `net.wifi`/`net.bt` / 基础层）与真值探测**同判"在位"**
/// ——那是"没有反证"，不是"探测拿不到答案"，两者不同判（见文件头末段）。
pub struct Rr3AbsentProbe;

impl Rr3Probe for Rr3AbsentProbe {
    fn rr3_probe_slot(&self, slot: &str) -> bool {
        rr3_slot_probe_face(slot).is_none()
    }
}

/// 探测档显式选择环境变量（与 `RR3_HAL_BACKEND` 同一条纪律：显式优先、取值不认点名回落）。
pub const RR3_PROBE_ENV: &str = "RR3_PROBE";

/// 探测件选择（纯函数，env 读取只在 [`rr3_probe_default`]）：`explicit` 显式档优先——
/// `sim` → [`Rr3SimProbe`]（e2e/开发机显式选，不再当缺省）；`linux` → 真值探测
/// （非 linux 平台拿不到 → 恒缺，fail-closed，不冒充在位）；`absent` → [`Rr3AbsentProbe`]。
/// 未给（`None`）或取值不认 → 平台默认：test → sim；linux → 真值探测；其余生产面 → 恒缺。
pub fn rr3_probe_select(explicit: Option<&str>) -> Arc<dyn Rr3Probe> {
    match explicit {
        Some("sim") => Arc::new(Rr3SimProbe),
        Some("absent") => Arc::new(Rr3AbsentProbe),
        Some("linux") => {
            if cfg!(target_os = "linux") {
                Arc::new(Rr3LinuxProbe::rr3_linux_probe_new("/"))
            } else {
                // 非 linux 上显式要 linux 探测 = 拿不到答案 → 恒缺（fail-closed）
                Arc::new(Rr3AbsentProbe)
            }
        }
        // 未给 / 取值不认：平台默认（取值不认的点名由 rr3_probe_default 出）
        _ => rr3_probe_platform(),
    }
}

/// 平台默认探测件：test → sim；linux → 真值探测；其余生产面 → 恒缺。
fn rr3_probe_platform() -> Arc<dyn Rr3Probe> {
    if cfg!(test) {
        Arc::new(Rr3SimProbe)
    } else if cfg!(target_os = "linux") {
        Arc::new(Rr3LinuxProbe::rr3_linux_probe_new("/"))
    } else {
        Arc::new(Rr3AbsentProbe)
    }
}

/// 探测后端选择（MC-4 台-1 清零观察式）：`RR3_PROBE` 显式取值优先（取值不认当场
/// `eprintln!` 点名后回落平台默认——与批J G5 的"错误当场可见"同一条），否则**在非
/// test 面按目标分流**——linux → 五面真值探测（根 `/`）；其余生产面 → 恒缺。test 面
/// 显式保留 sim（主机可测编排；生产路径不经此分支）。
pub fn rr3_probe_default() -> Arc<dyn Rr3Probe> {
    match std::env::var_os(RR3_PROBE_ENV) {
        None => rr3_probe_select(None),
        Some(v) => {
            let v = v.to_string_lossy().into_owned();
            rr3_probe_warn(&v);
            rr3_probe_select(Some(&v))
        }
    }
}

/// 未知取值留痕（不拦启动，回落平台默认）。
fn rr3_probe_warn(v: &str) {
    let known = matches!(v, "sim" | "linux" | "absent");
    if !known {
        eprintln!(
            "rrd: 环境变量 {RR3_PROBE_ENV} 取值不认: {v}（允许 sim/linux/absent），回落平台默认"
        );
    }
}

#[cfg(test)]
mod tests;
