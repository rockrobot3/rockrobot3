//! linux（sysfs PWM）舵机后端：SG90 硬件 PWM（R22 定案档，0.0.6 方案 §5.4 R11①）。
//!
//! 编译门是单重的：`target_os = "linux"`——纯 std（sysfs 读写），无外部依赖、
//! 无 feature 门（与 [`crate::linux`] 的 alsa 双门不同）。主机 CI（macOS）不编译
//! 本文件；宿主侧验证路径 = aarch64 musl 交叉 check（R13）+ 板上
//! `cargo test -p rr-hal`（真 linux 目标跑全部单测，单测用临时目录假 sysfs 树，
//! 确定性、不触真硬件）。真件档验收（真舵机到位后的脉宽/抖动实测）留批F5a
//! 真件档，随 R9 矩阵"舵机"行验收（方案 §5.2）。
//!
//! 接口面：内核 pwm sysfs（`/sys/class/pwm/pwmchipN/{export,unexport,pwmM/…}`）。
//! SG90 定案参数（方案 §5.5.2 接线 1 / R22）：50Hz（20ms 周期），脉宽
//! 0.5–2.5ms 线性映射 0–180°；通道经 `RR3_HAL_PWM_CHIP` / `RR3_HAL_PWM_CHANNEL`
//! 选（缺省 pwmchip0/pwm0 = GPIO12，R8 引脚预算表的硬件 PWM 行）。
//!
//! boot 语义（R11①）：真件档装配单显式 `RR3_HAL_BACKEND=linux` 后，模块开机
//! 探活读 [`crate::Rr3PwmServo::angle`] 触发首次 ensure——芯片/通道不在位
//! （dtoverlay 未开、权限不足）在此显式报错，rr3-act 按"能力缺失显式回错"
//! exit(1)，不静默退化。取通道所有权即落定全量配置（50Hz + 中位 90° 脉宽 +
//! 使能）：与 sim 后端"初始角 90°"同语义（探活读回 90，两后端观测一致，
//! R11 换装判据"编排与契约零改动"），也避免沿用上一进程遗留的未知占空比
//! 起动。进程退出（Drop）停发脉冲 + 归还通道（仅限自己导出的），防舵机在
//! 无人驱动时维持抖动。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::{Rr3HalError, Rr3PwmServo};

/// 内核 pwm sysfs 挂载点（构造参数是测试/夹具的根目录接缝，不另设环境变量）
pub const RR3_PWM_SYSFS_ROOT_DEFAULT: &str = "/sys/class/pwm";

/// SG90 定案参数（R22，方案 §5.5.2 接线 1）：PWM 周期 20ms（50Hz）
pub const RR3_SERVO_PERIOD_NS: u32 = 20_000_000;
/// SG90 脉宽下界 0.5ms ↔ 0°
pub const RR3_SERVO_PULSE_MIN_NS: u32 = 500_000;
/// SG90 脉宽上界 2.5ms ↔ 180°
pub const RR3_SERVO_PULSE_MAX_NS: u32 = 2_500_000;
/// SG90 物理角域上限：越域显式回错、不静默 clamp（行程裁决在模块 config
/// `angle_limit_deg`，这里钉的是舵机物理域——两者都越才到得了这一层）
pub const RR3_SERVO_ANGLE_MAX_DEG: u16 = 180;

/// 取通道所有权时落定的初始角（与 sim 后端"初始角 90°（中位）"同语义）
const SERVO_NEUTRAL_DEG: u16 = 90;
/// export 写入后等 `pwmN` 目录出现的上限（内核 workqueue 异步建目录，板上实测毫秒级）
const PWM_EXPORT_TIMEOUT: Duration = Duration::from_millis(1000);
/// 目录出现轮询间隔
const PWM_EXPORT_POLL: Duration = Duration::from_millis(10);

/// 角度 → 脉宽（ns）：0.5–2.5ms 线性映射 0–180°，整数运算。
/// 越域回 [`Rr3HalError::InvalidParam`] 点名，不静默 clamp。
fn servo_duty_ns(deg: u16) -> Result<u32, Rr3HalError> {
    if deg > RR3_SERVO_ANGLE_MAX_DEG {
        return Err(Rr3HalError::InvalidParam(format!(
            "SG90 角域 0..={RR3_SERVO_ANGLE_MAX_DEG}，收到 {deg}°——越域显式回错不静默 clamp（行程裁决在模块 config angle_limit_deg）"
        )));
    }
    let span = RR3_SERVO_PULSE_MAX_NS - RR3_SERVO_PULSE_MIN_NS;
    Ok(RR3_SERVO_PULSE_MIN_NS + (deg as u32 * span) / RR3_SERVO_ANGLE_MAX_DEG as u32)
}

/// 脉宽（ns）→ 角度：[`servo_duty_ns`] 的精确反演（四舍五入整数式；
/// 对 `servo_duty_ns` 的全部输出往返恒等）。域外回错点名——通道被外部进程
/// 改写或尚未配置时诚实暴露，不编一个角。
fn servo_angle_of_duty(duty: u64) -> Result<u16, Rr3HalError> {
    let span = (RR3_SERVO_PULSE_MAX_NS - RR3_SERVO_PULSE_MIN_NS) as u64;
    let min = RR3_SERVO_PULSE_MIN_NS as u64;
    if !(min..=min + span).contains(&duty) {
        return Err(Rr3HalError::InvalidParam(format!(
            "pwm 占空比 {duty}ns 不在 SG90 脉宽映射域 [{RR3_SERVO_PULSE_MIN_NS}, {RR3_SERVO_PULSE_MAX_NS}]ns——通道被外部改写或尚未配置"
        )));
    }
    Ok((((duty - min) * RR3_SERVO_ANGLE_MAX_DEG as u64 + span / 2) / span) as u16)
}

fn pwm_read_u64(path: &Path) -> Result<u64, Rr3HalError> {
    let s = std::fs::read_to_string(path).map_err(|e| {
        Rr3HalError::InvalidParam(format!("pwm 读 {:?} 失败: {e}", path.display()))
    })?;
    s.trim().parse::<u64>().map_err(|e| {
        Rr3HalError::InvalidParam(format!(
            "pwm {:?} 内容 {:?} 不是合法读数: {e}",
            path.display(),
            s.trim()
        ))
    })
}

fn pwm_write(path: &Path, val: impl std::fmt::Display) -> Result<(), Rr3HalError> {
    std::fs::write(path, format!("{val}\n")).map_err(|e| {
        Rr3HalError::InvalidParam(format!(
            "pwm 写 {val} 到 {:?} 失败: {e}（权限不足或通道不在位）",
            path.display()
        ))
    })
}

/// linux sysfs PWM 舵机（[`crate::Rr3PwmServo`] 的 linux 后端）。
///
/// 选择函数（[`crate::backend::rr3_hal_servo`]）不触设备：构造只算路径，
/// 导出与全量配置延迟到首次 trait 调用（与 alsa 后端 open 延迟同形）。
pub struct Rr3LinuxPwmServo {
    root: PathBuf,
    chip: u32,
    channel: u32,
    /// None = 尚未 ensure；Some(we_exported) = 已就绪（we_exported 记通道是
    /// 本进程导出的还是先前已在位——Drop 只归还自己导出的）
    ensured: Mutex<Option<bool>>,
}

impl Rr3LinuxPwmServo {
    /// 指定根/芯片/通道构造（选择函数从生效物理绑定取 `pwm_chip`/`pwm_channel`，
    /// 批D④ fail-closed——芯片/通道没有代码缺省，描述符/env 都没给就在选择面
    /// 点名；测试用假 sysfs 树也走这里，不触真硬件）
    pub fn rr3_linux_pwm_servo_new(root: &Path, chip: u32, channel: u32) -> Self {
        Self {
            root: root.to_path_buf(),
            chip,
            channel,
            ensured: Mutex::new(None),
        }
    }

    fn chip_dir(&self) -> PathBuf {
        self.root.join(format!("pwmchip{}", self.chip))
    }

    fn pwm_dir(&self) -> PathBuf {
        self.chip_dir().join(format!("pwm{}", self.channel))
    }

    /// 首次使用：按需导出 + 落定全量配置（周期/中位脉宽/使能）。
    /// `ensured` 由调用方在持锁状态下传入——本函数只在 ensure 路径上跑一次。
    fn ensure(&self, ensured: &mut Option<bool>) -> Result<(), Rr3HalError> {
        if ensured.is_some() {
            return Ok(());
        }
        let dir = self.pwm_dir();
        let we_exported = !dir.exists();
        if we_exported {
            let export_path = self.chip_dir().join("export");
            std::fs::write(&export_path, format!("{}\n", self.channel)).map_err(|e| {
                Rr3HalError::InvalidParam(format!(
                    "pwm 导出通道 {} 写 {:?} 失败: {e}（芯片不在位——dtoverlay=pwm-2chan 未开，或权限不足）",
                    self.channel,
                    export_path.display()
                ))
            })?;
            let t0 = Instant::now();
            while !dir.exists() {
                if t0.elapsed() >= PWM_EXPORT_TIMEOUT {
                    return Err(Rr3HalError::InvalidParam(format!(
                        "pwm 导出后 {:?} 未在 {}ms 内出现（内核 sysfs 异常）",
                        dir.display(),
                        PWM_EXPORT_TIMEOUT.as_millis()
                    )));
                }
                std::thread::sleep(PWM_EXPORT_POLL);
            }
        }
        // 周期仅在变化时重写；重写前先归零占空比（旧 duty > 新 period 会被内核
        // EINVAL 拒）。读不到/读成垃圾按 0 处理 → 走全量写。
        if pwm_read_u64(&dir.join("period")).unwrap_or(0) != RR3_SERVO_PERIOD_NS as u64 {
            pwm_write(&dir.join("duty_cycle"), 0)?;
            pwm_write(&dir.join("period"), RR3_SERVO_PERIOD_NS)?;
        }
        pwm_write(&dir.join("duty_cycle"), servo_duty_ns(SERVO_NEUTRAL_DEG)?)?;
        pwm_write(&dir.join("enable"), 1)?;
        *ensured = Some(we_exported);
        Ok(())
    }
}

impl Drop for Rr3LinuxPwmServo {
    fn drop(&mut self) {
        let ensured = self
            .ensured
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let Some(we_exported) = ensured else {
            return; // 从未使用：选择函数构造后直接丢弃的实例不碰任何文件
        };
        // 停发脉冲（进程退出后舵机不被未知占空比维持）；错误全忽略——
        // Drop 里的写失败无从上报，真件档验收关注板上实况
        let _ = std::fs::write(self.pwm_dir().join("enable"), "0\n");
        if we_exported {
            let _ = std::fs::write(
                self.chip_dir().join("unexport"),
                format!("{}\n", self.channel),
            );
        }
    }
}

#[async_trait]
impl Rr3PwmServo for Rr3LinuxPwmServo {
    async fn set_angle(&self, deg: u16) -> Result<(), Rr3HalError> {
        let duty = servo_duty_ns(deg)?;
        let mut guard = self.ensured.lock().unwrap_or_else(|p| p.into_inner());
        self.ensure(&mut guard)?;
        pwm_write(&self.pwm_dir().join("duty_cycle"), duty)
    }

    async fn angle(&self) -> Result<u16, Rr3HalError> {
        let mut guard = self.ensured.lock().unwrap_or_else(|p| p.into_inner());
        self.ensure(&mut guard)?;
        let duty = pwm_read_u64(&self.pwm_dir().join("duty_cycle"))?;
        servo_angle_of_duty(duty)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// 假 sysfs 树：pwm 目录已在位（通道先前已导出的形状，ensure 不走 export 路）
    fn fake_chip_in_place(root: &Path, chip: u32, channel: u32) -> PathBuf {
        let chip_dir = root.join(format!("pwmchip{chip}"));
        let pwm_dir = chip_dir.join(format!("pwm{channel}"));
        std::fs::create_dir_all(&pwm_dir).unwrap();
        for f in ["export", "unexport"] {
            std::fs::write(chip_dir.join(f), "").unwrap();
        }
        for f in ["period", "duty_cycle", "enable"] {
            std::fs::write(pwm_dir.join(f), "").unwrap();
        }
        pwm_dir
    }

    fn temp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rr3-hal-pwm-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn read_u64(p: &Path) -> u64 {
        std::fs::read_to_string(p).unwrap().trim().parse().unwrap()
    }

    #[test]
    fn duty_maps_angle_edges_linearly() {
        assert_eq!(servo_duty_ns(0).unwrap(), RR3_SERVO_PULSE_MIN_NS);
        assert_eq!(servo_duty_ns(90).unwrap(), 1_500_000, "中位 90° = 1.5ms");
        assert_eq!(servo_duty_ns(180).unwrap(), RR3_SERVO_PULSE_MAX_NS);
        assert_eq!(servo_duty_ns(45).unwrap(), 1_000_000, "45° = 1.0ms");
        // 整数运算的截断方向：向下取整（脉宽略短，绝不越出 2.5ms 上界）
        assert_eq!(servo_duty_ns(1).unwrap(), 511_111);
        assert_eq!(servo_duty_ns(179).unwrap(), 2_488_888);
    }

    #[test]
    fn duty_rejects_out_of_domain_angle() {
        for bad in [181u16, 200, 360, u16::MAX] {
            let e = servo_duty_ns(bad).unwrap_err();
            assert!(
                matches!(e, Rr3HalError::InvalidParam(ref s) if s.contains("180")),
                "越域角 {bad} 要点名物理域: {e}"
            );
        }
    }

    /// 反演对映射全域往返恒等（整数式精确，无累积误差）
    #[test]
    fn angle_inverts_duty_exactly_across_domain() {
        for deg in 0..=RR3_SERVO_ANGLE_MAX_DEG {
            let duty = servo_duty_ns(deg).unwrap() as u64;
            assert_eq!(servo_angle_of_duty(duty).unwrap(), deg, "deg={deg}");
        }
    }

    #[test]
    fn angle_rejects_duty_out_of_pulse_band() {
        for bad in [0u64, 499_999, 2_500_001, RR3_SERVO_PERIOD_NS as u64] {
            let e = servo_angle_of_duty(bad).unwrap_err();
            assert!(
                matches!(e, Rr3HalError::InvalidParam(ref s) if s.contains("脉宽映射域")),
                "域外占空比 {bad} 要点名: {e}"
            );
        }
    }

    /// 通道已在位（不写 export）：ensure 落定全量配置 → 探活读回中位 90°，
    /// 与 sim 后端"初始角 90°"观测一致（R11 换装判据：编排零改动）。
    #[test]
    fn ensure_configures_and_probe_reads_neutral() {
        let root = temp_root("in-place");
        let pwm_dir = fake_chip_in_place(&root, 0, 0);
        let dev = Rr3LinuxPwmServo::rr3_linux_pwm_servo_new(&root, 0, 0);
        assert_eq!(
            crate::rr3_block_on(dev.angle()).unwrap(),
            90,
            "探活读回中位——sim/linux 两后端同观测"
        );
        assert_eq!(read_u64(&pwm_dir.join("period")), RR3_SERVO_PERIOD_NS as u64);
        assert_eq!(read_u64(&pwm_dir.join("duty_cycle")), 1_500_000);
        assert_eq!(read_u64(&pwm_dir.join("enable")), 1);
        assert_eq!(
            std::fs::read_to_string(root.join("pwmchip0/export")).unwrap(),
            "",
            "通道先前在位，不写 export"
        );
        drop(dev);
        assert_eq!(read_u64(&pwm_dir.join("enable")), 0, "Drop 停发脉冲");
        assert_eq!(
            std::fs::read_to_string(root.join("pwmchip0/unexport")).unwrap(),
            "",
            "不是自己导出的，不归还"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// set_angle 落占空比 + 反演回读一致；越域角回错且不改现状
    #[test]
    fn set_angle_writes_duty_and_reads_back() {
        let root = temp_root("set");
        let pwm_dir = fake_chip_in_place(&root, 0, 1);
        let dev = Rr3LinuxPwmServo::rr3_linux_pwm_servo_new(&root, 0, 1);
        crate::rr3_block_on(dev.set_angle(45)).unwrap();
        assert_eq!(read_u64(&pwm_dir.join("duty_cycle")), 1_000_000);
        assert_eq!(crate::rr3_block_on(dev.angle()).unwrap(), 45);
        assert_eq!(
            read_u64(&pwm_dir.join("period")),
            RR3_SERVO_PERIOD_NS as u64,
            "set_angle 不重写周期（ensure 已落定）"
        );
        let e = crate::rr3_block_on(dev.set_angle(181)).unwrap_err();
        assert!(matches!(e, Rr3HalError::InvalidParam(_)), "{e}");
        assert_eq!(
            read_u64(&pwm_dir.join("duty_cycle")),
            1_000_000,
            "回错不落写"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 通道未导出：ensure 写 export，等目录出现后配置；Drop 归还自己导出的通道。
    /// 假 sysfs 不会自动建目录，由看护线程在 export 被写入后补建（仿真内核行为）。
    #[test]
    fn ensure_exports_channel_and_drop_returns_it() {
        let root = temp_root("export");
        let chip_dir = root.join("pwmchip0");
        std::fs::create_dir_all(&chip_dir).unwrap();
        for f in ["export", "unexport"] {
            std::fs::write(chip_dir.join(f), "").unwrap();
        }
        let watch_dir = chip_dir.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..1000 {
                let exported =
                    std::fs::read_to_string(watch_dir.join("export")).unwrap_or_default();
                if exported.trim() == "0" {
                    let d = watch_dir.join("pwm0");
                    std::fs::create_dir_all(&d).unwrap();
                    for f in ["period", "duty_cycle", "enable"] {
                        std::fs::write(d.join(f), "").unwrap();
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            panic!("看护线程: export 未被写入");
        });
        let dev = Rr3LinuxPwmServo::rr3_linux_pwm_servo_new(&root, 0, 0);
        assert_eq!(crate::rr3_block_on(dev.angle()).unwrap(), 90);
        watcher.join().unwrap();
        assert_eq!(
            std::fs::read_to_string(chip_dir.join("export")).unwrap().trim(),
            "0"
        );
        drop(dev);
        assert_eq!(
            std::fs::read_to_string(chip_dir.join("unexport"))
                .unwrap()
                .trim(),
            "0",
            "自己导出的通道在 Drop 归还"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 未使用即丢弃（选择函数构造 → 探活前失败退出的路径）：不碰任何文件
    #[test]
    fn drop_without_use_touches_nothing() {
        let root = temp_root("unused");
        let pwm_dir = fake_chip_in_place(&root, 2, 0);
        let dev = Rr3LinuxPwmServo::rr3_linux_pwm_servo_new(&root, 2, 0);
        drop(dev);
        assert_eq!(read_u64(&pwm_dir.join("enable")), 0, "空文件解析为 0");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
