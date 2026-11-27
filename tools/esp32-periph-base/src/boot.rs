// SPDX-License-Identifier: Apache-2.0
//! 板级启动装配：外设开箱（UART2/I2C0/ADC1/LEDC）→ 两平面线程 → 音频面 → 状态环。
//!
//! 本文件只做**装配序**：引脚真值仍以 `board/pins_*.rs` 对账锚为准，具名引脚
//! （`peripherals.pins.gpioNN`）在本文件的 cfg 分支里显式选择——esp-idf-hal 要求
//! 具体引脚类型，无法由常量构造。机制件（OLED/轨压/采集/播放）各自的头注是
//! 接线与判据真源，本文件不复制；OLED 装配细则住 `boot/oled.rs`，本文件只在
//! 开箱序里调用 `open_oled`。
//!
//! 序列是硬件契约（BOD/时序实证在案）：UART2 先于 I2C0（16/17 打死 I2C 的教训
//! 见 pins_classic 头注）→ OLED 先出表情 → 舵机装配即释放 → 运行面先起、控制面
//! 随后 → 采集中段 → 播放面最后。改序须带真板证据，不靠推断。

mod diag;
mod oled;
mod report;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_hal::gpio::{AnyInputPin, AnyOutputPin, InputPin, OutputPin};
#[cfg(feature = "esp32s3")]
use esp_idf_hal::gpio::{Gpio15, Gpio16, Gpio17, Gpio18, Gpio6, Gpio7};
#[cfg(feature = "esp32")]
use esp_idf_hal::gpio::{Gpio16, Gpio17, Gpio25, Gpio26, Gpio27, Gpio4};
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::i2s::{I2S0, I2S1};
use esp_idf_hal::ledc::{
    config::TimerConfig, LedcDriver, LedcTimerDriver, CHANNEL0, CHANNEL1, TIMER0,
};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::uart::{UartConfig, UartDriver, UART2};
use esp_idf_hal::units::Hertz;
use esp_idf_sys::{esp_get_free_heap_size, esp_get_minimum_free_heap_size, EspError};

use crate::board::{
    ADC_RAIL_DIVIDER_DEN, ADC_RAIL_DIVIDER_NUM, BOOT_PAN_DEG, BOOT_TILT_DEG, LEDC_FREQ_HZ,
    LEDC_RES_BITS, RAIL_GATE_ARMED, RR3_BIND_LEDC_PAN, RR3_BIND_LEDC_TILT, RR3_BIND_UART2_RX,
    RR3_BIND_UART2_TX, THREAD_STACK, UART2_BAUD_HZ, UART2_RX_RING, UART2_TX_RING,
};
use crate::commands::cmd_loop;
use crate::face::expr_eye_base;
use crate::mic;
use crate::periph::{CtlUart, RtPeriph};
#[cfg(feature = "tts")]
use crate::playout;
use crate::power::RailMonitor;
use crate::runtime::runtime_loop;
#[cfg(feature = "spk-tone")]
use crate::speaker;
use crate::state::{lock_bus, push_job, Bus, Job, PeriphState};

use oled::open_oled;
use report::{boot_banner, ram_point};

/// UART2 TX 驱动档拉满 CAP_3（判据② 音频上行病害的定案修复，2026-10-16）。根因
/// （真板双探针实锤）：默认档 CAP_2（≈20mA）推不动指令面长跳线 + FT232 接收链路
/// ——长零串（线上持续低电平）期间电平被耦合/漏电抬高，接收端把 0 读成 1（受害区＝
/// 帧头部长零串与 payload 头 212B）；CAP_3（≈40mA）下 27 完整帧位级全对。失败
/// 点名后继续跑，不阻断装配。
fn uart_tx_drive() {
    // SAFETY：只按编译期常量引脚号调用 IDF GPIO API，不涉及裸指针/所有权；
    // 失败由返回值判定，不改变任何 Rust 侧状态。
    let res = unsafe {
        esp_idf_sys::gpio_set_drive_capability(
            RR3_BIND_UART2_TX as esp_idf_sys::gpio_num_t,
            esp_idf_sys::gpio_drive_cap_t_GPIO_DRIVE_CAP_3,
        )
    };
    if res != 0 {
        println!(
            "[periph] UART2 TX(GPIO{RR3_BIND_UART2_TX}) 驱动档 CAP_3 设置失败 err={res}（继续跑，查 esp-idf-sys 对表）"
        );
    }
}

/// UART2 指令面装配：TX 环 = UART2_TX_RING、RX 环 = UART2_RX_RING（策略面）。
/// TX 环缺省 256B 下整帧写 11.4KB 同步等线速排空 (~59ms@1.5M)，mic 组帧线程被
/// 写调用饿死 → I2S DMA 环溢出丢数（真板实录 [mic] 采样率崩塌）；RX 环缺省
/// 256B 在宿主突发多命令时静默丢字节（2026-10-30 实测 60 条连发丢 15 条）。
fn open_uart2<P, Q>(
    uart2: impl Peripheral<P = UART2> + 'static,
    tx: impl Peripheral<P = P> + 'static,
    rx: impl Peripheral<P = Q> + 'static,
) -> Result<UartDriver<'static>, String>
where
    P: OutputPin + 'static,
    Q: InputPin + 'static,
{
    let config = UartConfig::new()
        .baudrate(Hertz(UART2_BAUD_HZ))
        .tx_fifo_size(UART2_TX_RING)
        .rx_fifo_size(UART2_RX_RING);
    let uart = UartDriver::new(
        uart2,
        tx,
        rx,
        None::<AnyInputPin>,
        None::<AnyOutputPin>,
        &config,
    )
    .map_err(diag::uart2_fail)?;
    println!("[periph] UART2 ✓ 指令面 {UART2_BAUD_HZ} @ GPIO{RR3_BIND_UART2_TX}(TX)/GPIO{RR3_BIND_UART2_RX}(RX)（对账锚，board.rs）");
    uart_tx_drive();
    Ok(uart)
}

/// 5V 轨监测装配（失败不 panic，回 `None`——采样路径据此回 Err，不冒领读数）。
fn rail_open(rail: Result<RailMonitor, EspError>) -> Option<RailMonitor> {
    match rail {
        Ok(r) => {
            // 测点引脚号不上打印（批N4 件七(c)）：真值以 rr3.set_bindings 注入为准。
            // 分压比与接闸态从 board 常量取（2026-11-27 批120 修打印口径漂移：
            // 常量已是 2:1，旧文案写死 1:1——假读数比缺读数更坏）。
            println!(
                "[periph] 5V 轨监测 ✓ ADC1 @ 测点引脚经 rr3.set_bindings 下发（分压 {}:{}，接闸 armed={}；RAIL 命令采一次）",
                ADC_RAIL_DIVIDER_NUM, ADC_RAIL_DIVIDER_DEN, RAIL_GATE_ARMED
            );
            Some(r)
        }
        Err(e) => {
            println!("[periph] 5V 轨监测装配失败（不 panic 继续跑指令面）: {e:?}");
            None
        }
    }
}

/// 云台两轴装配：timer 与两通道同源（50Hz/14bit 见 board），返回 timer 供调用方
/// 持有——`LedcTimerDriver` 的 Drop 会复位硬件，timer 必须活到进程终局。
fn open_servos<P, Q>(
    timer0: impl Peripheral<P = TIMER0> + 'static,
    ch0: impl Peripheral<P = CHANNEL0> + 'static,
    ch1: impl Peripheral<P = CHANNEL1> + 'static,
    pan: impl Peripheral<P = P> + 'static,
    tilt: impl Peripheral<P = Q> + 'static,
) -> Result<
    (
        LedcTimerDriver<'static, TIMER0>,
        LedcDriver<'static>,
        LedcDriver<'static>,
    ),
    String,
>
where
    P: OutputPin + 'static,
    Q: OutputPin + 'static,
{
    let config = TimerConfig::new()
        .frequency(Hertz(LEDC_FREQ_HZ))
        .resolution(LEDC_RES_BITS);
    let timer = LedcTimerDriver::new(timer0, &config).map_err(diag::ledc_timer_fail)?;
    let pan = LedcDriver::new(ch0, &timer, pan).map_err(diag::ledc_pan_fail)?;
    let tilt = LedcDriver::new(ch1, &timer, tilt).map_err(diag::ledc_tilt_fail)?;
    Ok((timer, pan, tilt))
}

/// 运行面装配：初始账角 = 各轴中位锚、判定核两线阈值取策略面（`RtPeriph::new` 单点）。
/// **舵机暂不激磁**：装配即经端口释放两轴 duty=0（无脉冲=舵机松脱），待运行面把
/// 开机表情画完再走 `Move` 激磁——板上 5V 直供下舵机一动即拉塌轨压触发 BOD 复位
/// （真板实录：boot 画面出后 ~0.1s 即 Brownout），"先表情上屏再动舵机"是可见性优先。
fn init_rt(
    i2c: I2cDriver<'static>,
    pan: LedcDriver<'static>,
    tilt: LedcDriver<'static>,
    rail: Option<RailMonitor>,
) -> RtPeriph {
    let max_duty = pan.get_max_duty();
    let mut rt = RtPeriph::new(i2c, pan, tilt, rail);
    rt.servo_release();
    println!(
        "[periph] LEDC ✓ pan=GPIO{RR3_BIND_LEDC_PAN} tilt=GPIO{RR3_BIND_LEDC_TILT} duty=0（待表情上屏后激磁）max={max_duty}"
    );
    rt
}

/// 共享总线 + 开机表情/姿态种子：表情**先上屏**（可见性优先：板载 5V 直供下舵机
/// 启动是全板最大电流事件，万一触发 BOD 也要保住已出的表情），随后由运行面走姿态
/// Move——串行降级（先 pan 后 tilt）+ 缓速软启动把两轴电流尖峰摊开摊薄。
fn bus_seed() -> Arc<Mutex<Bus>> {
    let bus = Arc::new(Mutex::new(Bus {
        jobs: VecDeque::new(),
        outbox: VecDeque::new(),
        audio_q: VecDeque::new(),
        st: PeriphState::boot(),
    }));
    {
        let base = expr_eye_base("idle");
        let mut b = lock_bus(&bus);
        b.st.expr_mode = true;
        b.st.frame_mode = false;
        b.st.base = base;
        b.st.last_text = String::from("EXPR idle");
        b.st.anim_reset();
        b.st.drawn = base;
        b.st.pan_deg = BOOT_PAN_DEG;
        b.st.tilt_deg = BOOT_TILT_DEG;
    }
    push_job(&bus, Job::RenderCapsules);
    push_job(
        &bus,
        Job::Move {
            pan: Some(BOOT_PAN_DEG),
            tilt: Some(BOOT_TILT_DEG),
        },
    );
    bus
}

/// 起两平面线程：运行面先起（boot 后即可接作业 + 跑自主节拍），控制面随后。
fn spawn_planes(ctl: CtlUart, rt: RtPeriph, bus: Arc<Mutex<Bus>>) -> Result<(), String> {
    {
        let bus = bus.clone();
        thread::Builder::new()
            .name("periph-runtime".into())
            .stack_size(THREAD_STACK)
            .spawn(move || runtime_loop(rt, bus))
            .map_err(diag::rt_thread_fail)?;
    }
    thread::Builder::new()
        .name("periph-ctl".into())
        .stack_size(THREAD_STACK)
        .spawn(move || cmd_loop(ctl, bus))
        .map_err(diag::ctl_thread_fail)?;
    Ok(())
}

/// 板侧服务面起序（W3-2b/W4-2b）：自治监督 → 云 TTS worker → httpd → SNTP。
/// TTS worker 先于 httpd 起（`/say` 不撞"未装配"窗）；SNTP 在 httpd 之后（对时
/// 失败不拖慢面板）；dialog worker 由调用方在音频面之前起（W6）；全部按 feature
/// 档条件编译，默认固件零行为变化。
fn start_services(bus: &Arc<Mutex<Bus>>) {
    #[cfg(feature = "autonomy")]
    crate::autonomy::rr3_start();
    #[cfg(feature = "tts")]
    crate::cloud::spawn_tts(
        bus.clone(),
        crate::dialog::rr3_turn,
        crate::dialog::rr3_turn_spool,
    );
    crate::httpd::rr3_httpd_spawn(bus);
    #[cfg(feature = "autonomy")]
    crate::sntp::spawn_sntp();
}

/// 主环：周期状态行（观测面）。断开复位由各线程负责，这里只打印。
fn status_loop() -> ! {
    loop {
        thread::sleep(Duration::from_secs(30));
        // SAFETY：两个堆查询都是无参只读的 IDF 系统调用，不触碰 Rust 内存状态。
        let (free, min_free) =
            unsafe { (esp_get_free_heap_size(), esp_get_minimum_free_heap_size()) };
        println!("[periph] status free={free}B min_free={min_free}B");
    }
}

/// 音频面外部件（开箱一并取出）：`Peripherals` 字段部分移动后不能再整体传递，
/// 而音频面按硬件契约最后起——脚位在开箱时定型，线程由 [`spawn_audio`] 末段拉起。
#[cfg(feature = "esp32")]
struct AudioFace {
    i2s0: I2S0,
    mic: (Gpio4, Gpio27, Gpio26),
    i2s1: I2S1,
    spk: (Gpio25, Gpio17, Gpio16),
}

#[cfg(feature = "esp32s3")]
struct AudioFace {
    i2s0: I2S0,
    mic: (Gpio6, Gpio15, Gpio7),
    i2s1: I2S1,
    spk: (Gpio16, Gpio18, Gpio17),
}

/// 音频面起序（硬件契约末段）：先采集，后提示音/常驻播放；I2S1 两档互斥。
/// 元组序 = (bclk, din/dout, ws)，与各 spawn 签名同序。
fn spawn_audio(face: AudioFace, bus: &Arc<Mutex<Bus>>) {
    let AudioFace {
        i2s0,
        mic,
        i2s1,
        spk,
    } = face;
    // 采集冒烟：接法与 BCLK 迁移史见 mic.rs 头注（2026-10-20 迁 GPIO4）。
    mic::spawn_mic_smoke(bus.clone(), i2s0, mic.0, mic.1, mic.2);
    ram_point("3-mic-smoke");
    // W4-1 提示音验收档（spk-tone）。
    #[cfg(feature = "spk-tone")]
    {
        speaker::spawn_tone(i2s1, spk.0, spk.1, spk.2);
        ram_point("4-spk-tone");
    }
    // W4-2 常驻播放面（tts）：播放线程独占 I2S1 TX。
    #[cfg(feature = "tts")]
    {
        playout::spawn_player(i2s1, spk.0, spk.1, spk.2, bus.clone());
        ram_point("4-spk-play");
    }
    #[cfg(not(any(feature = "spk-tone", feature = "tts")))]
    let _ = (i2s1, spk); // 默认固件不占 I2S1：外部件就地释放
}

/// 板级外设开箱（自 rr3_boot 抽出）：按芯片档选脚后逐件装配，返回 LEDC timer、
/// 指令面、运行面与音频面外部件。timer 由调用方持有到进程终局（Drop 会复位硬件）。
fn open_parts(
    p: Peripherals,
) -> Result<
    (
        LedcTimerDriver<'static, TIMER0>,
        CtlUart,
        RtPeriph,
        AudioFace,
    ),
    String,
> {
    // UART2 指令面引脚：classic 18/19；S3 N16R8 41/42（真板订正，见 pins_s3.rs 头注）。
    #[cfg(feature = "esp32")]
    let uart = open_uart2(p.uart2, p.pins.gpio18, p.pins.gpio19)?;
    #[cfg(feature = "esp32s3")]
    let uart = open_uart2(p.uart2, p.pins.gpio41, p.pins.gpio42)?;
    // I2C0 引脚：classic 21/22，S3 8/9（判据①：扫描 0x3C 应答）。
    #[cfg(feature = "esp32")]
    let i2c = open_oled(p.i2c0, p.pins.gpio21, p.pins.gpio22)?;
    #[cfg(feature = "esp32s3")]
    let i2c = open_oled(p.i2c0, p.pins.gpio8, p.pins.gpio9)?;
    // ADC1 5V 轨测点：classic GPIO34=CH6，S3 GPIO1=CH0（分压比/阈值见 board，待标定）。
    #[cfg(feature = "esp32")]
    let rail = rail_open(RailMonitor::new(p.adc1, p.pins.gpio34));
    #[cfg(feature = "esp32s3")]
    let rail = rail_open(RailMonitor::new(p.adc1, p.pins.gpio1));
    // 云台两轴：classic 32/33，S3 4/5。
    #[cfg(feature = "esp32")]
    let (timer, pan, tilt) = open_servos(
        p.ledc.timer0,
        p.ledc.channel0,
        p.ledc.channel1,
        p.pins.gpio32,
        p.pins.gpio33,
    )?;
    #[cfg(feature = "esp32s3")]
    let (timer, pan, tilt) = open_servos(
        p.ledc.timer0,
        p.ledc.channel0,
        p.ledc.channel1,
        p.pins.gpio4,
        p.pins.gpio5,
    )?;
    #[cfg(feature = "esp32")]
    let audio = AudioFace {
        i2s0: p.i2s0,
        mic: (p.pins.gpio4, p.pins.gpio27, p.pins.gpio26),
        i2s1: p.i2s1,
        spk: (p.pins.gpio25, p.pins.gpio17, p.pins.gpio16),
    };
    #[cfg(feature = "esp32s3")]
    let audio = AudioFace {
        i2s0: p.i2s0,
        mic: (p.pins.gpio6, p.pins.gpio15, p.pins.gpio7),
        i2s1: p.i2s1,
        spk: (p.pins.gpio16, p.pins.gpio18, p.pins.gpio17),
    };
    let rt = init_rt(i2c, pan, tilt, rail);
    Ok((timer, CtlUart { uart }, rt, audio))
}

/// 板级启动总序：外设开箱 → 两平面 → 音频面 → 状态环（永不返回，复位靠 BOD/复位键）。
///
/// 失败收口：任一步返回 Err 即打印完整诊断后显式停机（[`halt`]）——panic 档是
/// `immediate-abort`，`expect` 文案不落地；先打印再停，现场才有可查的一条。
pub(crate) fn rr3_boot() {
    if let Err(e) = rr3_boot_try() {
        halt(e);
    }
}

fn rr3_boot_try() -> Result<(), String> {
    boot_banner();
    // esp-idf-hal 的 take 返回 Result<Peripherals, Peripherals>（Err 是已取走的单例）：
    // 只取"是否拿到"，不读 Err 内容。
    let p = Peripherals::take().map_err(|_| {
        "Peripherals::take 失败：外设已被取走（启动期不应发生，查是否有二次取）".to_string()
    })?;
    let (_timer, ctl, rt, audio) = open_parts(p)?;
    let bus = bus_seed();
    ram_point("2-periph-ready");
    // W7-b3b 存储承载：仅无 PSRAM 档起 spool 写线程（启动清扫残留段；擦写读
    // 全在写线程）。S3 有 PSRAM 走内存路径，不占 6 KB 栈与命令队列。
    #[cfg(feature = "tts")]
    if crate::psram::rr3_psram_total() == 0 {
        crate::spool::rr3_start_spool(crate::cloud::rr3_turn_spool);
    }
    start_services(&bus);
    spawn_planes(ctl, rt, bus.clone())?;
    spawn_audio(audio, &bus);
    status_loop()
}

/// 启动失败的统一停机：先把诊断打完整，再 abort——不进入半装配态。
/// 不用 panic/expect：`immediate-abort` 档下 panic 文案不打印。
fn halt(e: String) -> ! {
    eprintln!("[periph] 启动失败：{e}");
    eprintln!("[periph] 显式停机：不进入半装配态（查上一条诊断；复位键/BOD 重启）");
    std::process::abort()
}
