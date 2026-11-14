//! sim 执行/读数面模拟件：servo、gpio、display、电池、链路与扫描列表。

use std::sync::Mutex;

use crate::{Rr3HalError, Rr3SsidText};

/// sim 舵机（虚拟 PWM 舵机）：角度记录在内存，读回即最后一次 set 的值。
///
/// 分工口径与音频 sim 同一条：sim 是"设备"不是"策略"——行程越界裁决属模块侧
/// （angle_limit 来自模块 config），HAL 只如实记录。初始角 90°（中位）。
pub struct Rr3SimServo {
    angle: Mutex<u16>,
}

impl Rr3SimServo {
    pub fn rr3_sim_servo_new(initial_deg: u16) -> Self {
        Self {
            angle: Mutex::new(initial_deg),
        }
    }

    /// 同步读角（测试与诊断用；trait 的 `angle` 是 async 面）
    pub fn rr3_sim_servo_angle(&self) -> u16 {
        *self.angle.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl crate::Rr3PwmServo for Rr3SimServo {
    fn set_angle(&mut self, deg: u16) -> Result<(), Rr3HalError> {
        *self.angle.lock().unwrap_or_else(|p| p.into_inner()) = deg;
        Ok(())
    }

    fn angle(&self) -> Result<u16, Rr3HalError> {
        Ok(self.rr3_sim_servo_angle())
    }
}

/// sim 数字输出（虚拟 GPIO 通道）：电平记录在内存。LED 与继电器各持一路。
/// 初始电平由构造方给定（执行件一律从断电态起步）。
pub struct Rr3SimGpioOut {
    level: std::sync::atomic::AtomicBool,
}

impl Rr3SimGpioOut {
    pub fn rr3_sim_gpio_new(initial: bool) -> Self {
        Self {
            level: std::sync::atomic::AtomicBool::new(initial),
        }
    }

    /// 同步读电平（测试与诊断用；trait 的 `level` 是 async 面）
    pub fn rr3_sim_gpio_level(&self) -> bool {
        self.level.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl crate::Rr3GpioOut for Rr3SimGpioOut {
    fn set(&mut self, level: bool) -> Result<(), Rr3HalError> {
        self.level
            .store(level, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn level(&self) -> Result<bool, Rr3HalError> {
        Ok(self.rr3_sim_gpio_level())
    }
}

/// sim 显示件（虚拟 8x8 点阵）：当前帧记录在内存，`draw` 即替换、`frame` 读回。
/// 与 sim gpio 同口径：位图内存记录，不做任何"渲染"（真渲染属真件后端）；
/// 初始帧由构造给定（模块面探活前的黑屏用全 0）。线程面用 `Mutex` 不用原子：
/// 帧是 8 字节组合量，拆原子要字节序换算，可读性不值。
pub struct Rr3SimDisplay {
    frame: Mutex<crate::Rr3FaceFrame>,
}

impl Rr3SimDisplay {
    /// 指定初始帧构造（模块面尚未出帧的黑屏态传全 0）
    pub fn rr3_sim_display_new(initial: crate::Rr3FaceFrame) -> Self {
        Self {
            frame: Mutex::new(initial),
        }
    }

    /// 同步读当前帧（测试与诊断用；trait 的 `frame` 是 async 面）
    pub fn rr3_sim_frame(&self) -> crate::Rr3FaceFrame {
        *self.frame.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl crate::Rr3Display for Rr3SimDisplay {
    fn draw(&mut self, frame: &crate::Rr3FaceFrame) -> Result<(), Rr3HalError> {
        *self.frame.lock().unwrap_or_else(|p| p.into_inner()) = *frame;
        Ok(())
    }

    fn frame(&self) -> Result<crate::Rr3FaceFrame, Rr3HalError> {
        Ok(self.rr3_sim_frame())
    }
}

/// sim 电池（虚拟电量计）：电量与充电态记录在内存，电压按电量**确定性线性派生**。
///
/// 分工口径与舵机/数字输出同一条：sim 是"设备"不是"策略"——低电阈值裁决属模块侧
/// （low_voltage_mv 来自 rr3-power config），低压关断属 rr3-power 本地决策与 M6，
/// HAL 只如实报数。电压映射钉死为 `3200mV + 10mV/1%`（0% → 3200、100% → 4200），
/// 与 golden 向量同一条确定性纪律：同一电量任何机器读出同一电压。
pub struct Rr3SimBattery {
    percent: std::sync::atomic::AtomicU8,
    charging: std::sync::atomic::AtomicBool,
}

impl Rr3SimBattery {
    pub fn rr3_sim_battery_new(percent: u8, charging: bool) -> Self {
        Self {
            percent: std::sync::atomic::AtomicU8::new(percent),
            charging: std::sync::atomic::AtomicBool::new(charging),
        }
    }

    /// 直接写设备状态（测试与 e2e 驱动用；trait 面只读，驱动必须走构造方持有的引用）
    pub fn rr3_sim_battery_set(&self, percent: u8, charging: bool) {
        self.percent
            .store(percent, std::sync::atomic::Ordering::Relaxed);
        self.charging
            .store(charging, std::sync::atomic::Ordering::Relaxed);
    }

    /// 同步读电量（测试与诊断用；trait 的 `battery` 是 async 面）
    pub fn rr3_sim_batt_pct(&self) -> u8 {
        self.percent.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn rr3_sim_batt_chg(&self) -> bool {
        self.charging.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 电量 → 电压（毫伏）的唯一映射。放关联函数而不是方法：它是"这颗虚拟电芯的
    /// 特性曲线"，不随实例状态变——e2e 断言与实现引用同一处，改曲线这里当场红。
    pub fn rr3_sim_batt_mv(percent: u8) -> u16 {
        3200 + percent as u16 * 10
    }
}

impl crate::Rr3Power for Rr3SimBattery {
    fn battery(&self) -> Result<crate::Rr3BatteryState, Rr3HalError> {
        let percent = self.rr3_sim_batt_pct();
        Ok(crate::Rr3BatteryState {
            voltage_mv: Self::rr3_sim_batt_mv(percent),
            percent,
            charging: self.rr3_sim_batt_chg(),
        })
    }
}

/// sim 网络链路（虚拟 WiFi 状态源）：up / ssid / rssi 记录在内存。
///
/// 分工口径与舵机/电池同一条：sim 是"设备"不是"策略"——漫游/重连决策属模块侧
/// （rr3-connect），NetworkManager/D-Bus 属板上件（cfg 门骨架，0.0.2 方案 D7），
/// HAL 只如实报数。断链时 ssid/rssi 强制 `None`：真设备断链后 NM 侧拿不到这些
/// 读数，设备面如实表现，模块侧不许从"up=false 还带着 ssid"里发明语义。
pub struct Rr3SimLink {
    up: std::sync::atomic::AtomicBool,
    ssid: Mutex<Option<Rr3SsidText>>,
    rssi_dbm: std::sync::atomic::AtomicI8,
}

impl Rr3SimLink {
    pub fn rr3_sim_link_new(up: bool, ssid: &str, rssi_dbm: i8) -> Self {
        Self {
            up: std::sync::atomic::AtomicBool::new(up),
            ssid: Mutex::new(Some(Rr3SsidText::rr3_ssid_text_new(ssid))),
            rssi_dbm: std::sync::atomic::AtomicI8::new(rssi_dbm),
        }
    }

    /// 直接写设备状态（测试与 e2e 驱动用；trait 面只读，驱动必须走构造方持有的引用）
    pub fn rr3_sim_link_set(&self, up: bool, ssid: &str, rssi_dbm: i8) {
        self.up.store(up, std::sync::atomic::Ordering::Relaxed);
        *self.ssid.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(Rr3SsidText::rr3_ssid_text_new(ssid));
        self.rssi_dbm
            .store(rssi_dbm, std::sync::atomic::Ordering::Relaxed);
    }
}

impl crate::Rr3Link for Rr3SimLink {
    fn state(&self) -> Result<crate::Rr3LinkState, Rr3HalError> {
        let up = self.up.load(std::sync::atomic::Ordering::Relaxed);
        let mut ssid = *self.ssid.lock().unwrap_or_else(|p| p.into_inner());
        let mut rssi = Some(self.rssi_dbm.load(std::sync::atomic::Ordering::Relaxed));
        if !up {
            ssid = None;
            rssi = None;
        }
        Ok(crate::Rr3LinkState {
            up,
            ssid,
            rssi_dbm: rssi,
        })
    }
}

/// sim 扫描列表（0.0.5 批I N-9 自 rr3-connect wifi 件迁入）：确定性三行假 AP，
/// 元素 = `(ssid, rssi_dbm, auth)`。首行 SSID 即 sim 链路缺省 SSID
/// （`crate::backend::RR3_HAL_SIM_LINK_SSID_DEFAULT`，合流一处真值）。
/// 只保证形状与确定性，不保证"周围真有这些网"——诚实标注由承载层的
/// `backend:"sim"` 承担，板上换 NetworkManager 真扫描。
pub fn rr3_sim_scan_list() -> [(&'static str, i8, bool); 3] {
    [
        (crate::backend::RR3_HAL_SIM_LINK_SSID_DEFAULT, -42, true),
        ("rock-sim-guest", -58, true),
        ("neighbour-2g", -71, false),
    ]
}
