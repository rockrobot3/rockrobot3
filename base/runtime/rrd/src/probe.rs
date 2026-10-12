//! 装配链硬件探测面（0.0.2 方案 D2；0.0.5 批M MC-4 实装真值接线）。
//!
//! 探测 fail-closed（方案 §4、§20 第 4 条）：探测不到与探测自身故障同判——回
//! `false` 即缺格，装配链对点名模块走"绝不带病起"。**拿不到答案不得当作在位**：
//! 目录读失败、设备节点缺失一律 `false`。探测在 boot 前消费一次（ADR-0012 裁定 4：
//! 运行期不重排），不阻塞启动横幅。
//!
//! 后端三档（MC-4 台-1 清零）：
//! - [`Rr3LinuxProbe`]：**生产 linux 档的真值探测**，五面在位性（sysfs PWM 通道 /
//!   GPIO line / I2C 设备 / SPI 设备 / ALSA 卡名）。探测根可注入（`rr3_linux_probe_new`），
//!   生产传 `/`，单测传假根——判据因此主机可测。纯 std，无外部依赖，全平台编译。
//! - [`Rr3AbsentProbe`]：非 linux 生产面（本机 macOS 直跑 rrd 的场合）——有硬件面的
//!   格位恒缺（不冒充在位，fail-closed）；无硬件面可反证的格位与真值探测同判"在位"。
//! - [`Rr3SimProbe`]：**显式档，不再当缺省构造**——sim 机无硬件可探，只在 test 面、
//!   显式 env（`RR3_PROBE=sim`，e2e/开发机用）与显式注入
//!   （`rr3_profile_boot_inputs_with_probe`）里用。
//!
//! 选择口（[`rr3_probe_default`]）与 `RR3_HAL_BACKEND` 同一条纪律：**env 显式取值优先**
//! （`RR3_PROBE=sim|linux|absent`，取值不认当场 `eprintln!` 点名后回落平台默认），
//! 否则按目标分流（test → sim；linux → 真值探测；其余生产面 → 恒缺）。
//!
//! 格位 → 探测面映射（[`rr3_slot_probe_face`]）：有硬件面的格位按面探，无硬件面
//! 可反证的格位（`cognition.*` 软件格、`net.wifi`/`net.bt` 运行期格、基础层）回
//! "在位"——那是"没有反证"不是"探测拿不到答案"，两者不同判。

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 一台机器的"格位在位吗"探测面。装配链每 boot 构造一次、逐格询问。
pub trait Rr3Probe: Send + Sync {
    fn rr3_probe_slot(&self, slot: &str) -> bool;
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

/// 格位 → 探测面。`None` = 无硬件面可反证（软件格 / 运行期格 / 基础层），
/// 探测对它们回"在位"（见文件头末段）。
pub fn rr3_slot_probe_face(slot: &str) -> Option<Rr3ProbeFace> {
    match slot {
        "act.servo" => Some(Rr3ProbeFace::Pwm),
        "act.led" | "act.relay" => Some(Rr3ProbeFace::Gpio),
        "ui.face" => Some(Rr3ProbeFace::Spi),
        "power.charger" | "sensor.env" | "sensor.imu" => Some(Rr3ProbeFace::I2c),
        "audio.capture" | "audio.playback" | "audio.vad" => Some(Rr3ProbeFace::Alsa),
        // cognition.*（软件格）、net.wifi/net.bt（运行期/占位格）、基础层 bus.*/
        // net.online/power.battery：无硬件面，无反证 → 在位。
        _ => None,
    }
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

    /// 文件存在且非空（ALSA 卡表有内容 = 至少注册了一张卡）。
    fn rr3_file_nonempty(path: &Path) -> bool {
        std::fs::metadata(path)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    }

    /// 面在位性。任一面拿不到答案 → `false`（fail-closed）。
    fn rr3_face_present(&self, face: Rr3ProbeFace) -> bool {
        match face {
            Rr3ProbeFace::Pwm => {
                Self::rr3_dir_has_prefix(&self.root.join("sys/class/pwm"), "pwmchip")
            }
            Rr3ProbeFace::Gpio => {
                Self::rr3_dir_has_prefix(&self.root.join("sys/class/gpio"), "gpiochip")
            }
            Rr3ProbeFace::I2c => Self::rr3_dir_has_prefix(&self.root.join("dev"), "i2c-"),
            Rr3ProbeFace::Spi => Self::rr3_dir_has_prefix(&self.root.join("dev"), "spidev"),
            Rr3ProbeFace::Alsa => Self::rr3_file_nonempty(&self.root.join("proc/asound/cards")),
        }
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
        _ => {
            if cfg!(test) {
                Arc::new(Rr3SimProbe)
            } else if cfg!(target_os = "linux") {
                Arc::new(Rr3LinuxProbe::rr3_linux_probe_new("/"))
            } else {
                Arc::new(Rr3AbsentProbe)
            }
        }
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
            let known = matches!(v.as_str(), "sim" | "linux" | "absent");
            if !known {
                eprintln!(
                    "rrd: 环境变量 {RR3_PROBE_ENV} 取值不认: {v}（允许 sim/linux/absent），回落平台默认"
                );
            }
            rr3_probe_select(Some(&v))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 唯一 temp 目录（pid + 纳秒后缀，批H5 教训：只 pid 会撞上一轮遗留目录）
    fn rr3_test_probe_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rr3-probe-{}-{nanos}-{tag}", std::process::id()))
    }

    /// 格位 → 面映射：硬件格位逐条归面，软件格/运行期格/基础层归 None。
    #[test]
    fn slot_probe_face_maps_hardware_and_software_slots() {
        assert_eq!(rr3_slot_probe_face("act.servo"), Some(Rr3ProbeFace::Pwm));
        assert_eq!(rr3_slot_probe_face("act.led"), Some(Rr3ProbeFace::Gpio));
        assert_eq!(rr3_slot_probe_face("act.relay"), Some(Rr3ProbeFace::Gpio));
        assert_eq!(rr3_slot_probe_face("ui.face"), Some(Rr3ProbeFace::Spi));
        for s in ["power.charger", "sensor.env", "sensor.imu"] {
            assert_eq!(rr3_slot_probe_face(s), Some(Rr3ProbeFace::I2c), "{s}");
        }
        for s in ["audio.capture", "audio.playback", "audio.vad"] {
            assert_eq!(rr3_slot_probe_face(s), Some(Rr3ProbeFace::Alsa), "{s}");
        }
        for s in [
            "cognition.llm",
            "cognition.asr",
            "net.wifi",
            "net.bt",
            "net.online",
            "power.battery",
            "bus.publish",
            "",
        ] {
            assert_eq!(rr3_slot_probe_face(s), None, "无硬件面: {s}");
        }
    }

    /// 空假根：硬件格位全缺（fail-closed），软件/运行期格位在位（无反证）。
    #[test]
    fn linux_probe_on_empty_root_reports_hardware_absent() {
        let root = rr3_test_probe_root("empty");
        std::fs::create_dir_all(&root).unwrap();
        let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
        for s in [
            "act.servo",
            "act.led",
            "ui.face",
            "power.charger",
            "audio.capture",
        ] {
            assert!(!probe.rr3_probe_slot(s), "空根下硬件格位必须缺: {s}");
        }
        for s in ["cognition.llm", "net.wifi", "net.online", ""] {
            assert!(probe.rr3_probe_slot(s), "无硬件面格位在位: {s}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 假根摆上五面设备节点 → 对应格位全在位；未摆的面仍缺。这条是"真值接线"
    /// 的主判据（探测确实读了设备面，而不是恒真/恒假）。
    #[test]
    fn linux_probe_reads_device_faces_from_root() {
        let root = rr3_test_probe_root("faces");
        std::fs::create_dir_all(root.join("sys/class/pwm/pwmchip0")).unwrap();
        std::fs::create_dir_all(root.join("sys/class/gpio/gpiochip0")).unwrap();
        std::fs::create_dir_all(root.join("dev")).unwrap();
        std::fs::write(root.join("dev/i2c-1"), b"").unwrap();
        std::fs::create_dir_all(root.join("proc/asound")).unwrap();
        std::fs::write(root.join("proc/asound/cards"), b" 0 [Headphones ]: ...\n").unwrap();
        let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);

        assert!(probe.rr3_probe_slot("act.servo"), "pwmchip 在位");
        assert!(probe.rr3_probe_slot("act.led"), "gpiochip 在位");
        assert!(probe.rr3_probe_slot("power.charger"), "i2c 设备在位");
        assert!(probe.rr3_probe_slot("audio.capture"), "声卡表非空");
        assert!(
            !probe.rr3_probe_slot("ui.face"),
            "spidev 未摆 → ui.face 缺（fail-closed，不冒充在位）"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 空 ALSA 卡表（文件在但 0 字节）= 无卡注册 → 缺（"拿不到答案不当作在位"）。
    #[test]
    fn linux_probe_treats_empty_alsa_cards_as_absent() {
        let root = rr3_test_probe_root("alsa-empty");
        std::fs::create_dir_all(root.join("proc/asound")).unwrap();
        std::fs::write(root.join("proc/asound/cards"), b"").unwrap();
        let probe = Rr3LinuxProbe::rr3_linux_probe_new(&root);
        assert!(
            !probe.rr3_probe_slot("audio.playback"),
            "空卡表 = 无卡注册 → 缺"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// sim 显式档：任何格位回"在位"（装配单缺 binding 才是它表达的缺格）。
    #[test]
    fn sim_probe_reports_every_slot_present() {
        let probe = Rr3SimProbe;
        for slot in ["act.servo", "sensor.env", "cognition.llm", ""] {
            assert!(probe.rr3_probe_slot(slot), "sim 档必须全在位: {slot:?}");
        }
    }

    /// 恒缺档（非 linux 生产面）：有硬件面的格位全缺，无硬件面可反证的格位在位
    /// （与真值探测同判——"没有反证"不是"拿不到答案"）。
    #[test]
    fn absent_probe_reports_hardware_faces_missing_software_present() {
        let probe = Rr3AbsentProbe;
        for slot in ["act.servo", "act.led", "ui.face", "power.charger", "audio.capture"] {
            assert!(!probe.rr3_probe_slot(slot), "有硬件面格位必须缺: {slot}");
        }
        for slot in ["cognition.llm", "net.wifi", "net.bt", "net.online", ""] {
            assert!(probe.rr3_probe_slot(slot), "无硬件面格位在位: {slot}");
        }
    }

    /// 台-1 清零观察式：显式档优先 + 平台默认分流（用**纯选择函数**断言，不读 env——
    /// 单测进程 env 共享，读 env 会串别的用例；env 读取本身无分支逻辑）。
    #[test]
    fn probe_select_prefers_explicit_then_dispatches_by_target() {
        // 显式 sim：即便在 linux 上也选 sim（e2e/开发机的唯一出口，不再当缺省）
        assert!(rr3_probe_select(Some("sim")).rr3_probe_slot("act.servo"));
        // 显式 absent：全缺
        assert!(!rr3_probe_select(Some("absent")).rr3_probe_slot("act.servo"));
        // 显式 linux：linux 上真值探测（真机值不断言），非 linux 恒缺（fail-closed）
        let l = rr3_probe_select(Some("linux"));
        if !cfg!(target_os = "linux") {
            assert!(
                !l.rr3_probe_slot("act.servo"),
                "非 linux 显式 linux → 拿不到答案 → 恒缺"
            );
        }
        // 取值不认（含空串）：与"未给"同判 → 平台默认
        assert!(rr3_probe_select(Some("bogus")).rr3_probe_slot("act.servo"), "test 面回落 sim");
        // 未给：test 面默认 sim；非 test 且非 linux → 恒缺（分流观察式）
        let d = rr3_probe_select(None);
        if cfg!(test) {
            assert!(d.rr3_probe_slot("act.servo"), "test 面默认 sim");
        } else if !cfg!(target_os = "linux") {
            assert!(!d.rr3_probe_slot("act.servo"), "非 linux 生产面恒缺");
        }
    }
}