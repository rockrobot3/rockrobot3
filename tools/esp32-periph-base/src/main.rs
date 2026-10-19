//! esp32-periph-base —— 0.0.5 增补轮五 批B 外设底座档 + 批C 帧执行面（espidf std，不依赖桥）。
//!
//! 职责（全案 §4 批B + 批C）：**SSD1306 I2C 驱动出字 + LEDC PWM 云台两轴逐度转向 +
//! UART2 双面指令接入**。批B 是独立固件档（行协议面）；批C 在本 crate 上加**帧
//! 执行面**（rr-ipc 能力帧，不进城——路线二裁定：板不注册总线身份，宿主 HAL 桥
//! 后端 rr-hal::esp32_uart 直连发 `rr3.call` 帧，本固件只是被点名的执行面）；
//! 批D 加 I2S——同一固件 crate 分批合入（全案 §5 板上固件形态）。
//!
//! 双面分流（批C）：指令循环逐字节收，首字节 = `0x31`（帧头 magic "1ERA" 的 LE
//! 首字节）走帧面（`rr3_framebuf_read` 整帧收 → rr3.call 分发 → Response 应答）；
//! 其余走批B 行面（调试与舵机五点 SCAN 判据沿用）。行面首字节集全为字母，
//! 分流无歧义。
//!
//! 引脚预算（全案 §5 约束面，与 0.0.7 实测接线零冲突）：
//! - UART2 = GPIO18→TX / GPIO19←RX（921600，CP2102 指令面；批B 行协议临时面，
//!   批C 换能力帧——接线不变；批D② 升 921600 音频带宽档，Q1=(a) 带宽账裁定）——**订正**：原记 17→TX/16←RX
//!   已过时，GPIO16/17 一装 UART2 驱动即把 I2C0（21/22）打死，故弃用，真板钉 18/19。
//! - I2C0  = GPIO21 SDA / GPIO22 SCL（100kHz，SSD1306 0x3C；400k 在跳线面时序边际，
//!   真板实录页写 ESP_FAIL，降 100k 稳过——见 init 段注）
//! - LEDC  = GPIO32 水平轴（pan）/ GPIO33 俯仰轴（tilt）——避开 strapping 面
//!   （0/2/5/12/15）、UART2（16/17）、I2C（21/22）、I2S 候选（25/26/27，批D）、
//!   VSPI 缺省组（18/19/23，点阵预留不占用不使能）
//! - UART0 = 控制台（monitor 观测面，板载 USB cu.usbserial-1410）
//!
//! 供电铁律（全案 §5）：舵机由 UBEC 独立 5V 轨供电（堵转 ~700mA/只，禁接板载
//! 5V），UBEC/板/舵机单点共地；信号线 3.3V 逻辑直驱 SG90 级兼容。
//!
//! esp-idf-hal 0.44.1 API 对表实录（vendored 源码核对，批B）：
//! - `I2cDriver::new(i2c, sda, scl, &I2cConfig::new().baudrate(Hertz))`（i2c.rs
//!   L207；`I2cConfig = config::Config` alias L66）；事务面 `write(addr, bytes,
//!   timeout: TickType_t)` 每调一次 START-addr-bytes-STOP（SSD1306 每命令/每
//!   数据块一传输）；探测用单字节 0x00 写（控制字节后 STOP，芯片无害）。
//! - `LedcTimerDriver::new(timer, &TimerConfig::new().frequency(Hertz(50))
//!   .resolution(Resolution::Bits14))`（ledc.rs L108）；`LedcDriver::new(channel,
//!   &timer, pin)`（L201，B: Borrow<LedcTimerDriver>——同一 timer 借用两次驱动
//!   两通道，timer 未 move）；`set_duty(u32)`（L289）。50Hz×2^14=819200Hz，
//!   LEDC 源 80MHz 分频 97.66 → 实际 ≈50.0Hz（±0.3%），SG90 容差内。
//!   peripherals.ledc 的 timer0–3/channel0–7 为低速组（L625），Bits14 上限
//!   同样适用。
//! - I2cDriver/LedcDriver/UartDriver 上游 0.44.1 **均已标 `Send`**（vendored 实证：
//!   i2c.rs L393 / ledc.rs L334 / uart.rs L653），故两平面 wrapper（[`CtlUart`]/
//!   [`RtPeriph`]）自动 Send、**无需手补 unsafe impl**（旧单 wrapper 的手补系误判，
//!   同批删除并订正）。
//! - UART2 整装 `UartDriver::new(uart, tx, rx, cts, rts, config)`（cts/rts 传
//!   None；同口二次 install 必败的教训，哨兵 API 对表实录在案，照抄不赘）。
//!   不 split：控制面单线程读写同驱动，owned move 即可。
//! - xtensa LX6 无 64 位原子；状态面（[`PeriphState`]）以 `Arc<Mutex<..>>` 护，
//!   角度用 u32 直传，无需原子。
//!
//! 线程模型（0.0.5 现场处置续批 2026-10-12：**控制面/运行面分离**）：main 只做初始化
//! （默认栈 3584B，println 少量，哨兵同款安全），随后起**两个** 16KB 线程（8KB→16KB
//! 运行下限钉死的教训在案，勿再调低）：
//! - **控制面** [`cmd_loop`]（独占 UART2）：逐字节收、双面分流、解析、校验、**改状态**、
//!   回包——**不碰 I2C/LEDC**，慢外设一律不阻塞命令吞吐（消"卡顿"）；
//! - **运行面** [`runtime_loop`]（独占 I2C0 + LEDC）：取控制面推送的硬件作业 [`Job`]
//!   执行（OLED 出字 / 舵机移动 / 五点扫描），并跑自主节拍（表情微动作 / 云台扫摆）。
//! 两面经 `Arc<Mutex<Bus>>` 通信（哨兵 Arc<Mutex> 纪律同款，xtensa 面已被其背书）：
//! `Bus { jobs, outbox, st }`——控制面推作业 + 改 `st`，运行面取作业执行 + 回推 outbox
//! 行。**锁只护状态读写，绝不跨 I2C 持有**（运行面"决策段持锁纯算，渲染段脱锁慢跑"）。
//!
//! 舵机**串行降级**（同批，用户明文裁定）：板载 5V 直供、无 UBEC 时两轴并行驱动的峰值
//! 电流叠加会拉塌轨压（真板 BOD / 舵机发烫实录），故复合动作一律**先 pan 到位 → 整定
//! [`AXIS_SETTLE_MS`] → 再 tilt**（任一时刻只驱动一轴，削峰值）；单轴命令直动。
//! SCAN 同守此律（每点 pan/tilt 两段），WANDER 扫摆**同拍只动一轴**（pan/tilt 错拍）。

mod board;
mod commands;
mod frame;
mod link;
mod mic;
mod oled;
mod periph;
mod power;
mod runtime;
mod state;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_hal::gpio::{AnyInputPin, AnyOutputPin};
use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::ledc::{config::TimerConfig, LedcDriver, LedcTimerDriver};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::uart::{UartConfig, UartDriver};
use esp_idf_hal::units::Hertz;
use esp_idf_sys::{esp_get_free_heap_size, esp_get_minimum_free_heap_size, link_patches};

use crate::board::{
    Axis, BOOT_PAN_DEG, BOOT_TILT_DEG, I2C_BAUD_HZ, LEDC_FREQ_HZ, LEDC_RES_BITS, OLED_ADDR,
    THREAD_STACK, UART2_BAUD_HZ, UART2_TX_RING,
};
use crate::commands::cmd_loop;
use crate::oled::{expr_eye_base, i2c_bus_unstick, i2c_scan, oled_init, oled_text};
use crate::periph::{CtlUart, RtPeriph};
use crate::power::{RailJudge, RailMonitor, RailTier};
use crate::runtime::runtime_loop;
use crate::state::{lock_bus, push_job, Bus, Job, PeriphState};

// ---------------------------------------------------------------------------

/// 三点堆采样观测行（批D 三点 RAM 复测沿用本法，批B 先落观测面）。
fn ram_point(point: &str) {
    let free = unsafe { esp_get_free_heap_size() };
    let min_free = unsafe { esp_get_minimum_free_heap_size() };
    println!("[periph] RAM point={point} free={free}B min_free={min_free}B");
}

fn main() {
    // IDF 绑定面补丁（esp-idf-hal 工程惯例起手，哨兵同款）
    link_patches();

    println!(
        // 引脚号不上横幅（批N4 件七(c)）：I2C/轨测点真值经 rr3.set_bindings 注入
        // （成功打印见 frame_handle_set_bindings），装配面引脚号在各装配打印自证。
        "[periph] surface=esp-rs-std crate=esp32-periph-base oled=i2c0 servo=ledc rail=adc1 cmd=uart2@{UART2_BAUD_HZ} line+frame planes=ctrl+runtime serial-servo"
    );
    ram_point("1-boot");

    let peripherals = Peripherals::take().unwrap();

    // ---- UART2 指令面 @ GPIO18(TX)/GPIO19(RX) 921600（批D②）----
    // **引脚定案（真板实锤 2026-10-12）**：GPIO16/17（UART2 的 IOMUX 缺省组）在本板
    // 上**装驱动即把 I2C0（21/22）打死**——先装/后装、换序均复现全黑；换到 18/19 后
    // I2C 全绿（扫描 0x3C / init / boot 文本 / 页写全过）。故本板弃用 16/17（疑与
    // 底板走线桥接或模块 PSRAM 脚冲突），钉 18/19。接线表见 README；19 与点阵 VSPI
    // 预留同脚，点阵立项时需再迁（已在案）。
    // TX 环 = UART2_TX_RING（board 策略面）：缺省 256B 下整帧写 11.4KB 同步等线速
    // 排空 ~120ms（921600），mic 组帧线程被写调用饿死 → I2S DMA 环溢出丢数（真板
    // 实录 [mic] 采样率崩塌）。TX 环 ≥ 单帧后 write 降为 memcpy 入环、异步排水。
    let uart_config = UartConfig::new()
        .baudrate(Hertz(UART2_BAUD_HZ))
        .tx_fifo_size(UART2_TX_RING);
    let uart = UartDriver::new(
        peripherals.uart2,
        peripherals.pins.gpio18,
        peripherals.pins.gpio19,
        None::<AnyInputPin>,
        None::<AnyOutputPin>,
        &uart_config,
    )
    .expect("UART2 驱动安装失败");
    println!("[periph] UART2 ✓ 指令面 {UART2_BAUD_HZ} @ GPIO18(TX)/GPIO19(RX)");

    // ---- GPIO18 驱动档拉满 CAP_3（判据② 音频上行病害的定案修复，2026-10-16）----
    // 根因（真板双探针实锤）：默认档 CAP_2（≈20mA）推不动 18/19 的长跳线 + FT232
    // 接收链路——**长零串（线上持续低电平）期间电平被耦合/漏电抬高**，接收端把 0
    // 读成 1。受害区 = 帧头部长零串（trace_id/reserved 全零收成 0xff 族）+ payload
    // 头 212B；'A' 等边沿密集内容完好。三重证据定案：① 片内 TX→RX 环回 2KB 四段
    // 图案（长零串/'A'/递增/0xFF）逐字节全对上——芯片内排除；② CAP_3（≈40mA）
    // 拉满后 FT232 抓流 27 完整帧**位级全对**（payload 整体 + PCM 图案 L=seq/R=递增
    // 核验过）；③ 损坏按字节数定界不按时间（921600/460800 同窗口）= 内容依赖。与
    // I2C 被迫 50k 同族（长跳线电气边际），但本例驱动档即可治。临时排障探针（环回
    // 函数 + 460800 对照）定案已撤。
    let drive_res = unsafe {
        esp_idf_sys::gpio_set_drive_capability(
            18 as esp_idf_sys::gpio_num_t,
            esp_idf_sys::gpio_drive_cap_t_GPIO_DRIVE_CAP_3,
        )
    };
    if drive_res != 0 {
        println!(
            "[periph] GPIO18 驱动档 CAP_3 设置失败 err={drive_res}（继续跑，查 esp-idf-sys 对表）"
        );
    }

    // ---- I2C0 @ GPIO21/22（判据①：扫描 0x3C 应答）----
    // 频率档 400k → 100k → **50k**：真板实证 400k/100k 在长跳线 + 模块弱上拉下**时序
    // 边际**——背靠背密集页写（8 页胶囊眼）在数据段丢拍，且丢拍一次即锁死总线
    // （机理见 [`i2c_bus_recover`]）。50k 上升裕量翻倍 + 失败自愈 + 整页重写三者叠加。
    let i2c_config = I2cConfig::new()
        .baudrate(Hertz(I2C_BAUD_HZ))
        .sda_enable_pullup(true)
        .scl_enable_pullup(true);
    // 欠压复位后 SSD1306 常抱死 SDA→总线全黑；装驱动前先解卡（见 i2c_bus_unstick）。
    // fail-closed（批N4 件七(c)）：引脚真值经 rr3.set_bindings 注入，boot 期桥未连
    // 必缺席 → 拒解卡并点名（v3 拍板：消费面级 fail-closed，不做板级等宿主的启动
    // 时序重构；启动期解卡能力随 0.0.7 批G/D1 主驱动具名引脚改造回归）。
    if unsafe { i2c_bus_unstick() } {
        println!("[periph] I2C boot 解卡 ✓（绑定已注入；SDA 高则本无需）");
    } else {
        eprintln!("[periph] I2C boot 解卡拒（绑定未注入）——欠压复位致总线抱死的自愈待 rr3.set_bindings 后由运行面接管");
    }
    let mut i2c = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
        &i2c_config,
    )
    .expect("I2C0 驱动安装失败");
    let found = i2c_scan(&mut i2c);
    if found.contains(&OLED_ADDR) {
        println!(
            "[periph] I2C 扫描应答: {:02X?} —— OLED 0x{OLED_ADDR:02X} 在列 ✓",
            found
        );
    } else {
        println!(
            "[periph] I2C 扫描应答: {found:02X?} —— 0x{OLED_ADDR:02X} 不在列 ✗（查接线/上拉）"
        );
    }
    // 真板诊断面（b7 实录）：init 失败打印 Err 明细不 panic——panic_abort 下
    // expect 无消息直接 Guru 复位循环，排障拿不到证据；终败继续跑指令面。
    // 100k 档 init 偶发瞬态失败（上电电荷泵未稳）——重试 5 次（每次 1s）兜底。
    let mut oled_ok = false;
    for attempt in 1..=5u32 {
        match oled_init(&mut i2c) {
            Ok(()) => {
                oled_ok = true;
                println!("[periph] OLED init ✓（100k 档第 {attempt} 次过）");
                break;
            }
            Err(e) => {
                println!("[periph] OLED 100k init 第 {attempt}/5 次失败: {e}");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
    if oled_ok {
        // boot 画面 4 行全出（4 行扩行能力的 boot 直证；row3 由 oled_show 常刷，
        // boot 期先摆初始角度占位）
        // 行2 波特率从 board 真值拼出（批D② 起 921600，防字面量漂移）
        let boot_rows: [(String, &str); 4] = [
            ("ROCK3 PERIPH BASE".into(), "boot 行0写失败"),
            ("4-ROW OLED READY".into(), "boot 行1写失败"),
            (format!("UART2 CMD @{UART2_BAUD_HZ}"), "boot 行2写失败"),
            ("P:060 T:098".into(), "boot 角度行写失败"),
        ];
        let mut boot_ok = true;
        for (row, (text, msg)) in boot_rows.iter().enumerate() {
            if let Err(e) = oled_text(&mut i2c, row, text.as_bytes()) {
                println!("[periph] {msg}: {e}");
                boot_ok = false;
            }
        }
        if boot_ok {
            println!("[periph] OLED ✓ boot 画面已出（4 行）");
        }
    } else {
        println!(
            "[periph] OLED init 终败（400k/100k 双档）——不 panic 继续跑指令面（查上拉/接线/供电）"
        );
    }

    // ---- ADC1_CH6 @ GPIO34：5V 轨电压监测（方案A Q1）----
    // 根因链（真板实锤）：OLED 大面亮点帧 + 舵机保持电流抢同一条 5V 轨 → 轨压跌落
    // → ESP32 BOD 复位。这条轨此前**无任何观测手段**；本装 ADC1 CH6 周期采样轨压。
    // 失败不 panic（照 OLED init 纪律）：缺测点/ADC 组件未启用时其余外设面照跑，采样
    // 路径回 Err（**不冒领读数**）。占位分压 1:1 **待真板标定**（接线后与万用表对拍）。
    let rail = match RailMonitor::new(peripherals.adc1, peripherals.pins.gpio34) {
        Ok(r) => {
            println!(
                // 测点引脚号不上打印（批N4 件七(c)）：具名装配自证 gpio34，数字真值
                // 以 rr3.set_bindings 注入打印为准（观测面唯一真值点）。
                "[periph] 5V 轨监测 ✓ ADC1 @ 测点引脚经 rr3.set_bindings 下发（占位分压 1:1 待标定；RAIL 命令采一次）"
            );
            Some(r)
        }
        Err(e) => {
            println!("[periph] 5V 轨监测装配失败（不 panic 继续跑指令面）: {e:?}");
            None
        }
    };

    // ---- LEDC timer0 50Hz 14bit：channel0→GPIO32 pan / channel1→GPIO33 tilt ----
    let timer_config = TimerConfig::new()
        .frequency(Hertz(LEDC_FREQ_HZ))
        .resolution(LEDC_RES_BITS);
    let timer =
        LedcTimerDriver::new(peripherals.ledc.timer0, &timer_config).expect("LEDC timer0 配置失败");
    let mut pan = LedcDriver::new(peripherals.ledc.channel0, &timer, peripherals.pins.gpio32)
        .expect("LEDC channel0(PAN, GPIO32) 安装失败");
    let mut tilt = LedcDriver::new(peripherals.ledc.channel1, &timer, peripherals.pins.gpio33)
        .expect("LEDC channel1(TILT, GPIO33) 安装失败");
    // **舵机暂不激磁**：LEDC 装好后 duty=0（无脉冲=舵机松脱），待运行面把开机表情
    // 画完再走 `Move` 激磁——板上 5V 直供下舵机一动即拉塌轨压触发 BOD 复位（真板
    // 实录：boot 画面出后 ~0.1s 即 `Brownout detector was triggered`），"先表情上屏
    // 再动舵机"是可见性优先的排序。
    pan.set_duty(0).expect("pan 归零失败");
    tilt.set_duty(0).expect("tilt 归零失败");
    println!(
        "[periph] LEDC ✓ pan=GPIO32 tilt=GPIO33 duty=0（待表情上屏后激磁）max={}",
        pan.get_max_duty()
    );
    // 外设 'static 化 + 分发两平面（16KB 运行下限，哨兵纪律）：UART2 → 控制面，
    // I2C/LEDC → 运行面；共享 Bus（Arc<Mutex>）连通两面。
    let rt = RtPeriph {
        i2c,
        pan,
        tilt,
        // 初始姿态 = 各轴中位锚（生成表 center，批N3 件五：行程真值收敛档案）
        pan_deg: Axis::Pan.center_deg(),
        tilt_deg: Axis::Tilt.center_deg(),
        contrast_idx: 0,
        rail,
        next_rail_ms: 0,
        rail_judge: RailJudge::new(),
        rail_tier: RailTier::Ok,
    };
    let ctl = CtlUart { uart };
    let bus = Arc::new(Mutex::new(Bus {
        jobs: VecDeque::new(),
        outbox: VecDeque::new(),
        audio_q: VecDeque::new(),
        st: PeriphState::boot(),
    }));
    // 开机默认进入表情（idle 胶囊眼）+ 走一遍指定姿态（pan 左转 30°、tilt 上抬 8°，
    // 串行降级）——UART2 指令链路现场劣化时的"可见性"兜底（用户现场裁定）。排队
    // RenderCapsules + Move，由运行面线程接管；姿态经 axis_check 已在本轴行程内。
    {
        let (open, dx, dy) = expr_eye_base("idle");
        let mut b = lock_bus(&bus);
        b.st.expr_mode = true;
        b.st.frame_mode = false;
        b.st.base_open = open;
        b.st.base_dx = dx;
        b.st.base_dy = dy;
        b.st.last_text = String::from("EXPR idle");
        b.st.anim_reset();
        b.st.drawn = (open, dx, dy);
        b.st.pan_deg = BOOT_PAN_DEG;
        b.st.tilt_deg = BOOT_TILT_DEG;
    }
    // 表情**先上屏**（可见性优先：板载 5V 直供下舵机启动是全板最大电流事件，万一
    // 触发 BOD 也要保住已出的表情），随后由运行面走姿态 Move——串行降级（先 pan 后
    // tilt）+ 缓速软启动（[`SERVO_SLEW_STEP_DEG`]）把两轴电流尖峰摊开摊薄。
    push_job(&bus, Job::RenderCapsules);
    push_job(
        &bus,
        Job::Move {
            pan: Some(BOOT_PAN_DEG),
            tilt: Some(BOOT_TILT_DEG),
        },
    );
    ram_point("2-periph-ready");
    // mic 线与控制面共享 bus（audio_q 产者/消费者；clone 须在 cmd_loop move 前）
    let mic_bus = bus.clone();
    // 运行面先起（boot 后即可接作业 + 跑自主节拍），再起控制面
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

    // ---- 批D 采集冒烟 + 音频上行：INMP441 双麦 @ I2S0（GPIO25 BCLK / GPIO27 SD / GPIO26 WS）----
    mic::spawn_mic_smoke(
        mic_bus,
        peripherals.i2s0,
        peripherals.pins.gpio25,
        peripherals.pins.gpio27,
        peripherals.pins.gpio26,
    );
    ram_point("3-mic-smoke");

    // main 循环：周期状态行（观测面）
    loop {
        thread::sleep(Duration::from_secs(30));
        println!(
            "[periph] status free={}B min_free={}B",
            unsafe { esp_get_free_heap_size() },
            unsafe { esp_get_minimum_free_heap_size() }
        );
    }
}
