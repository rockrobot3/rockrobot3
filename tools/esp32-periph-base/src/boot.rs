//! 板级启动装配：外设开箱（UART2/I2C0/ADC1/LEDC）→ 两平面线程 → 音频面 → 状态环。
//!
//! 本文件只做**装配序**：引脚真值仍以 `board/pins_*.rs` 对账锚为准，具名引脚
//! （`peripherals.pins.gpioNN`）在本文件的 cfg 分支里显式选择——esp-idf-hal 要求
//! 具体引脚类型，无法由常量构造。机制件（OLED/轨压/采集/播放）各自的头注是
//! 接线与判据真源，本文件不复制。
//!
//! 序列是硬件契约（BOD/时序实证在案）：UART2 先于 I2C0（16/17 打死 I2C 的教训
//! 见 pins_classic 头注）→ OLED 先出表情 → 舵机装配即释放 → 运行面先起、控制面
//! 随后 → 采集中段 → 播放面最后。改序须带真板证据，不靠推断。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_hal::gpio::{AnyInputPin, AnyOutputPin, InputPin, OutputPin};
#[cfg(feature = "esp32s3")]
use esp_idf_hal::gpio::{Gpio15, Gpio16, Gpio17, Gpio18, Gpio6, Gpio7};
#[cfg(feature = "esp32")]
use esp_idf_hal::gpio::{Gpio16, Gpio17, Gpio25, Gpio26, Gpio27, Gpio4};
use esp_idf_hal::i2c::{I2cConfig, I2cDriver, I2C0};
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
    BOOT_PAN_DEG, BOOT_TILT_DEG, I2C_BAUD_HZ, LEDC_FREQ_HZ, LEDC_RES_BITS, OLED_ADDR,
    RR3_BIND_LEDC_PAN, RR3_BIND_LEDC_TILT, RR3_BIND_UART2_RX, RR3_BIND_UART2_TX, THREAD_STACK,
    UART2_BAUD_HZ, UART2_RX_RING, UART2_TX_RING,
};
use crate::commands::cmd_loop;
use crate::face::expr_eye_base;
use crate::mic;
use crate::oled::{i2c_bus_unstick, i2c_scan, oled_init, oled_text};
use crate::periph::{CtlUart, RtPeriph};
#[cfg(feature = "tts")]
use crate::playout;
use crate::power::RailMonitor;
use crate::runtime::runtime_loop;
#[cfg(feature = "spk-tone")]
use crate::speaker;
use crate::state::{lock_bus, push_job, Bus, Job, PeriphState};

/// 三点堆采样观测行（批D 三点 RAM 复测沿用本法，批B 先落观测面）。
fn ram_point(point: &str) {
    // SAFETY：四个堆查询都是无参只读的 IDF 系统调用，不触碰 Rust 内存状态。
    let free = unsafe { esp_get_free_heap_size() };
    let min_free = unsafe { esp_get_minimum_free_heap_size() };
    // PSRAM 进 malloc 后 esp_get_free_heap_size 含外部堆；内存门看**内部 SRAM**
    // 单列（计划册 §3.2：内部只留栈与热路径），避免"PSRAM 掩盖 SRAM 见底"。
    let int_free =
        unsafe { esp_idf_sys::heap_caps_get_free_size(esp_idf_sys::MALLOC_CAP_INTERNAL) };
    let int_min =
        unsafe { esp_idf_sys::heap_caps_get_minimum_free_size(esp_idf_sys::MALLOC_CAP_INTERNAL) };
    println!(
        "[periph] RAM point={point} free={free}B int_free={int_free}B int_min={int_min}B min_free={min_free}B"
    );
}

/// 启动横幅（引脚号不上横幅：I2C/轨测点真值经 rr3.set_bindings 注入；芯片档自证行
/// 让证据记录直接可辨 classic / S3，引脚真值以 board/pins_*.rs 对账锚为准）。
/// n20-live 标记：重烧后此行在册＝新固件上板实证；同屏「凭据源=nvs」＝NVS 凭据跨
/// 重烧保留实证（升级即重烧语义）。
fn boot_banner() {
    println!(
        "[periph] surface=esp-rs-std crate=esp32-periph-base oled=i2c0 servo=ledc rail=adc1 cmd=uart2@{UART2_BAUD_HZ} line+frame planes=ctrl+runtime serial-servo n20-live"
    );
    #[cfg(feature = "esp32")]
    println!("[periph] chip=esp32 pins=classic");
    #[cfg(feature = "esp32s3")]
    {
        psram_point();
        println!("[periph] chip=esp32s3 pins=s3 flash=16MB psram=8MB-octal");
    }
    ram_point("1-boot");
}

/// PSRAM 点名行（W6 内存门证据）：sdkconfig.defaults 未生效时这里为 0——把
/// "配置没吃到"从隐性变成 boot 第一屏可见。
#[cfg(feature = "esp32s3")]
fn psram_point() {
    // SAFETY：两个 heap_caps 查询都是无参只读的 IDF 堆账读取。
    let total = unsafe { esp_idf_sys::heap_caps_get_total_size(esp_idf_sys::MALLOC_CAP_SPIRAM) };
    let free = unsafe { esp_idf_sys::heap_caps_get_free_size(esp_idf_sys::MALLOC_CAP_SPIRAM) };
    println!("[periph] PSRAM total={total}B free={free}B");
}

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
) -> UartDriver<'static>
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
    .expect("UART2 驱动安装失败");
    println!("[periph] UART2 ✓ 指令面 {UART2_BAUD_HZ} @ GPIO{RR3_BIND_UART2_TX}(TX)/GPIO{RR3_BIND_UART2_RX}(RX)（对账锚，board.rs）");
    uart_tx_drive();
    uart
}

/// I2C 扫描结果点名（OLED 在列 ✓ / 不在列 ✗）。
fn i2c_scan_report(found: &[u8]) {
    if found.contains(&OLED_ADDR) {
        println!("[periph] I2C 扫描应答: {found:02X?} —— OLED 0x{OLED_ADDR:02X} 在列 ✓");
    } else {
        println!(
            "[periph] I2C 扫描应答: {found:02X?} —— 0x{OLED_ADDR:02X} 不在列 ✗（查接线/上拉）"
        );
    }
}

/// OLED init 重试：100k 档上电电荷泵未稳的瞬态失败，每次隔 1s、最多 5 次；
/// 真板诊断面纪律——失败打印 Err 明细不 panic（panic_abort 下 expect 无消息）。
fn oled_init_retry(i2c: &mut I2cDriver) -> bool {
    for attempt in 1..=5u32 {
        match oled_init(i2c) {
            Ok(()) => {
                println!("[periph] OLED init ✓（100k 档第 {attempt} 次过）");
                return true;
            }
            Err(e) => {
                println!("[periph] OLED 100k init 第 {attempt}/5 次失败: {e}");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
    false
}

/// boot 四行（4 行扩行能力的直证；行2/行3 从 board 真值拼出，防字面量漂移）。
fn oled_boot_rows(i2c: &mut I2cDriver) {
    let boot_rows: [(String, &str); 4] = [
        ("ROCK3 PERIPH BASE".into(), "boot 行0写失败"),
        ("4-ROW OLED READY".into(), "boot 行1写失败"),
        (format!("UART2 CMD @{UART2_BAUD_HZ}"), "boot 行2写失败"),
        (
            format!("P:{BOOT_PAN_DEG:03} T:{BOOT_TILT_DEG:03}"),
            "boot 角度行写失败",
        ),
    ];
    let mut boot_ok = true;
    for (row, (text, msg)) in boot_rows.iter().enumerate() {
        if let Err(e) = oled_text(i2c, row, text.as_bytes()) {
            println!("[periph] {msg}: {e}");
            boot_ok = false;
        }
    }
    if boot_ok {
        println!("[periph] OLED ✓ boot 画面已出（4 行）");
    }
}

/// OLED 装配面：扫描点名 → init 重试 → boot 画面；全程不 panic（终败继续跑指令面）。
fn oled_boot(i2c: &mut I2cDriver) {
    let found = i2c_scan(&mut crate::ports_impl::rr3_esp_i2c(&mut *i2c));
    i2c_scan_report(&found);
    if oled_init_retry(i2c) {
        oled_boot_rows(i2c);
    } else {
        println!(
            "[periph] OLED init 终败（400k/100k 双档）——不 panic 继续跑指令面（查上拉/接线/供电）"
        );
    }
}

/// I2C0 + OLED 装配：装驱动前先解卡（欠压复位后 SSD1306 常抱死 SDA→总线全黑）。
/// 批N41：注入优先、缺席回编译期装配锚（独立部署下 boot 期桥必未连，不能因此
/// 永久拒解）；false 只代表 9 时钟后 SDA 仍低（模块未上电/硬短/线松），不阻断 boot。
/// 频率档见 board：50k 不是更稳档（真板实锤，见 [`I2C_BAUD_HZ`] 注）。
fn open_oled<S, C>(
    i2c0: impl Peripheral<P = I2C0> + 'static,
    sda: impl Peripheral<P = S> + 'static,
    scl: impl Peripheral<P = C> + 'static,
) -> I2cDriver<'static>
where
    S: InputPin + OutputPin + 'static,
    C: InputPin + OutputPin + 'static,
{
    // SAFETY：启动期在 I2C 驱动安装之前、单线程上下文调用；函数只按注入/装配锚
    // 的引脚号操作 GPIO，不同时存在 I2C 事务（见 `i2c_bus_unstick` 的契约）。
    if unsafe { i2c_bus_unstick() } {
        println!("[periph] I2C boot 解卡 ✓（SDA 本已高或已释放）");
    } else {
        eprintln!("[periph] I2C boot 解卡未释（9 时钟后 SDA 仍低：模块未上电/硬短/线松）——继续 boot，运行面自愈将退避重试");
    }
    let config = I2cConfig::new()
        .baudrate(Hertz(I2C_BAUD_HZ))
        .sda_enable_pullup(true)
        .scl_enable_pullup(true);
    let mut i2c = I2cDriver::new(i2c0, sda, scl, &config).expect("I2C0 驱动安装失败");
    // 扫描点名 → init 重试 → boot 画面（真板诊断面纪律：失败打印 Err 明细不 panic）
    oled_boot(&mut i2c);
    i2c
}

/// 5V 轨监测装配（失败不 panic，回 `None`——采样路径据此回 Err，不冒领读数）。
fn rail_open(rail: Result<RailMonitor, EspError>) -> Option<RailMonitor> {
    match rail {
        Ok(r) => {
            // 测点引脚号不上打印（批N4 件七(c)）：真值以 rr3.set_bindings 注入为准。
            println!(
                "[periph] 5V 轨监测 ✓ ADC1 @ 测点引脚经 rr3.set_bindings 下发（占位分压 1:1 待标定；RAIL 命令采一次）"
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
) -> (
    LedcTimerDriver<'static, TIMER0>,
    LedcDriver<'static>,
    LedcDriver<'static>,
)
where
    P: OutputPin + 'static,
    Q: OutputPin + 'static,
{
    let config = TimerConfig::new()
        .frequency(Hertz(LEDC_FREQ_HZ))
        .resolution(LEDC_RES_BITS);
    let timer = LedcTimerDriver::new(timer0, &config).expect("LEDC timer0 配置失败");
    let pan = LedcDriver::new(ch0, &timer, pan).expect("LEDC channel0(PAN) 安装失败");
    let tilt = LedcDriver::new(ch1, &timer, tilt).expect("LEDC channel1(TILT) 安装失败");
    (timer, pan, tilt)
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
fn spawn_planes(ctl: CtlUart, rt: RtPeriph, bus: Arc<Mutex<Bus>>) {
    {
        let bus = bus.clone();
        thread::Builder::new()
            .name("periph-runtime".into())
            .stack_size(THREAD_STACK)
            .spawn(move || runtime_loop(rt, bus))
            .expect("起运行面线程失败");
    }
    thread::Builder::new()
        .name("periph-ctl".into())
        .stack_size(THREAD_STACK)
        .spawn(move || cmd_loop(ctl, bus))
        .expect("起控制面线程失败");
}

/// 板侧服务面起序（W3-2b/W4-2b）：自治监督 → 云 TTS worker → httpd → SNTP。
/// TTS worker 先于 httpd 起（`/say` 不撞"未装配"窗）；SNTP 在 httpd 之后（对时
/// 失败不拖慢面板）；dialog worker 由调用方在音频面之前起（W6）；全部按 feature
/// 档条件编译，默认固件零行为变化。
fn start_services(bus: &Arc<Mutex<Bus>>) {
    #[cfg(feature = "autonomy")]
    crate::autonomy::rr3_start();
    #[cfg(feature = "tts")]
    crate::cloud::spawn_tts(bus.clone(), crate::dialog::rr3_turn);
    crate::httpd::rr3_httpd_spawn(bus);
    #[cfg(feature = "autonomy")]
    crate::sntp::spawn_sntp();
}

/// 主环：周期状态行（观测面）。断开复位由各线程负责，这里只打印。
fn status_loop() {
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
) -> (
    LedcTimerDriver<'static, TIMER0>,
    CtlUart,
    RtPeriph,
    AudioFace,
) {
    // UART2 指令面引脚：classic 18/19；S3 N16R8 41/42（真板订正，见 pins_s3.rs 头注）。
    #[cfg(feature = "esp32")]
    let uart = open_uart2(p.uart2, p.pins.gpio18, p.pins.gpio19);
    #[cfg(feature = "esp32s3")]
    let uart = open_uart2(p.uart2, p.pins.gpio41, p.pins.gpio42);
    // I2C0 引脚：classic 21/22，S3 8/9（判据①：扫描 0x3C 应答）。
    #[cfg(feature = "esp32")]
    let i2c = open_oled(p.i2c0, p.pins.gpio21, p.pins.gpio22);
    #[cfg(feature = "esp32s3")]
    let i2c = open_oled(p.i2c0, p.pins.gpio8, p.pins.gpio9);
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
    );
    #[cfg(feature = "esp32s3")]
    let (timer, pan, tilt) = open_servos(
        p.ledc.timer0,
        p.ledc.channel0,
        p.ledc.channel1,
        p.pins.gpio4,
        p.pins.gpio5,
    );
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
    (timer, CtlUart { uart }, rt, audio)
}

/// 板级启动总序：外设开箱 → 两平面 → 音频面 → 状态环（永不返回，复位靠 BOD/复位键）。
pub(crate) fn rr3_boot() {
    boot_banner();
    let p = Peripherals::take()
        .expect("Peripherals::take 失败：外设已被取走（启动期不应发生，查是否有二次取）");
    let (_timer, ctl, rt, audio) = open_parts(p);
    let bus = bus_seed();
    ram_point("2-periph-ready");
    // W7-b1a 存储承载：只读记录头点名 spool 分区状态，不写不擦（启动路径零磨损）。
    #[cfg(feature = "tts")]
    crate::spool::rr3_spool_probe();
    start_services(&bus);
    spawn_planes(ctl, rt, bus.clone());
    spawn_audio(audio, &bus);
    status_loop();
}
