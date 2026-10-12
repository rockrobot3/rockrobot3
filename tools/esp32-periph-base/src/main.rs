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
//! - UART2 = GPIO17→TX / GPIO16←RX（115200，CP2102 指令面；批B 行协议临时面，
//!   批C 换能力帧——接线与波特率不变，为进城铺路）
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

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_hal::gpio::{AnyInputPin, AnyOutputPin};
use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::ledc::{config::TimerConfig, LedcDriver, LedcTimerDriver, Resolution};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::uart::{UartConfig, UartDriver};
use esp_idf_hal::units::Hertz;
use esp_idf_sys::{
    esp_get_free_heap_size, esp_get_minimum_free_heap_size, esp_random, esp_timer_get_time,
    link_patches, TickType_t,
};
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, Rr3ByteStream, Rr3Frame, Rr3FrameBuf, Rr3Header,
    Rr3PayloadType, Rr3TransportConn,
};
use serde_json::{json, Value};

/// I2C 事务超时（TickType_t；portMAX_DELAY 同义 u32::MAX——阻塞至传输完成）。
const I2C_TIMEOUT: TickType_t = u32::MAX;
/// I2C 写重试次数（**有限电源优化**：板载 5V 直供下舵机转动/保持的电流尖峰拉塌轨压、
/// 或总线噪声，会让 SSD1306 事务瞬态失败 ESP_FAIL/NACK——真板实录"角度行刷新失败:
/// I2C 命令写失败 ESP_FAIL"即此）。**每笔失败先走 [`i2c_bus_recover`] 总线自愈**
/// （见其注：丢拍会把从机锁进"等时钟"态，此后重发恒不复——这是真板 8 页胶囊眼
/// 恒败的真因），故 3 次已足（恢复+重发≈必过），无需长窗死磕）。
const I2C_RETRY: u32 = 3;
/// I2C 重试间隔（给轨压/总线一点恢复时间，勿紧逼重发再撞失败）。
const I2C_RETRY_DELAY_MS: u64 = 5;
/// I2C 总线频率（SSD1306 标准档）。**真板实锤：必须 100k，不能降到 50k**——50k 档
/// 下连 init 的清屏页写（33B 数据笔）都恒 `ESP_FAIL`（命令笔 ≤4B 恒过），比 100k 更
/// 差；老 I2C 驱动的低速档时序是反直觉的坏档。100k 是 SSD1306 标准档、真板实测
/// init/boot 文本恒过（唯有背靠背 8 页密集写会丢拍），靠 [`i2c_bus_recover`] 自愈兜底。
const I2C_BAUD_HZ: u32 = 100_000;
/// 分块间短延时：给模块电荷泵与总线电平留一拍（边际失败集中在背靠背事务）。
const I2C_CHUNK_GAP_MS: u64 = 1;
/// 单页整页重试上限：失败 → 总线恢复 → **从页首重写整页**（一笔数据丢拍后从机的
/// 列指针已错位，只重发该块会写歪；整页重发把指针拉回确定性起点）。外层还有
/// [`RtPeriph::capsules_degrading`] 的降亮重来，故本层不必死磕。
const I2C_PAGE_ATTEMPTS: u32 = 2;
/// OLED 亮度阶梯（**整板电压降级**，本轮真板实锤的主修复）：SSD1306 满亮（0xCF）下
/// 大面亮点会把模块升压电路电流顶高、抬高地弹/轨压噪声，反噬 I2C 通信——真板实录
/// "空白页 8 页恒过、boot 文本小面积恒过、8 页胶囊眼在**页 3（大面亮点开始上屏）**
/// 起恒 `ESP_FAIL`，且此后整屏恒败";降到 0x2F 后胶囊眼 8 页全过、微动作连跑零失败。
/// 初始取中档，I2C 反复失败即逐级下调保通信——"极限电源下宁暗不瞎"。
const OLED_CONTRAST_LEVELS: [u8; 4] = [0x4F, 0x3F, 0x2F, 0x1F];
/// SSD1306 I2C 从机地址（0.96" 128x64 模块缺省）。
const OLED_ADDR: u8 = 0x3C;
/// OLED 每行字符数：5px 字模 + 1px 间隔 = 6px/字符，128px/6 = 21.3 → 21 列。
const OLED_COLS: usize = 21;
/// OLED 文本行数：4 行 = 文本 3 行（页 0/2/4，`OLED` 命令按 21 列硬切）+ 固定
/// 角度行（页 6，P/T 随舵机命令刷新）——Mac 实时数据面（温度/内存/IP/时间）
/// 同屏三行布局，批B 增量批裁定（2→4 扩行，`OLED <text>` 形状不变向后兼容）。
const OLED_ROWS: usize = 4;
/// 指令行缓冲上限（超出整行丢弃回 ERR，fail-closed 不截半执行）。
const LINE_CAP: usize = 128;
/// 指令/心跳类线程的栈面（板上实测实锤 16KB 运行下限，哨兵同款，勿再调低）。
const THREAD_STACK: usize = 16384;
/// 轴间整定（**串行降级**：先 pan 到位 → 等这么久 → 再 tilt）。SG90 空载约
/// 0.1s/60°，本轴最大行程 40° 约 67ms，200ms 留裕量（含负载迟滞与死区）。
const AXIS_SETTLE_MS: u64 = 200;
/// 舵机缓速步进（**极限电源下的软启动**）：每次只挪 [`SERVO_SLEW_STEP_DEG`] 度、
/// 每步歇 [`SERVO_SLEW_STEP_MS`]——把"整段跳变"的启动/换向电流尖峰摊成涓流，
/// 避免板载 5V 直供被单次大电流拉塌（真板 BOD 复位实录）。2°/20ms = 100°/s，
/// pan 满行程 60° 约 600ms、tilt 20° 约 200ms，慢于 SG90 空载能力但最省轨压。
const SERVO_SLEW_STEP_DEG: u32 = 2;
const SERVO_SLEW_STEP_MS: u64 = 20;
/// SCAN 每轴每点驻留（判据② 逐点回读；串行后单点 = pan 段 + tilt 段各一次）。
const SCAN_POINT_MS: u64 = 600;
/// 运行面自主节拍（表情微动作 + 云台扫摆；无作业时空转一拍）。
const RUNTIME_TICK_MS: u64 = 10;

/// 帧面本板 task 位形（帧头 dst 门）：外设执行面占首模块池位形 2——与宿主
/// rr-hal::esp32_uart::RR3_PERIPH_TASK 同值（本 crate 不依赖 rr-core，此位形是
/// 两端帧头约定，宿主侧有同值锚定测试防静默分叉）。
const RR3_PERIPH_TASK: u64 = 2;
/// 帧头 magic "1ERA" 的 LE 首字节（0x31='1'）——双面分流窥视判据。行面首字节
/// 集全为字母（HELP/OLED/PAN/...），与 0x31 无交集，分流无歧义。
const RR3_MAGIC_FIRST: u8 = 0x31;
/// 舵机点名主题（在册 request L1；形状事实源
/// contracts/events/act-servo-command.schema.json，与 topics.toml 同串）。
const RR3_SERVO_TOPIC: &str = "rr3-act/servo/command";
/// 表情点名主题（在册 request L1；形状事实源
/// contracts/events/interact-face-expression.schema.json，与 topics.toml 同串）。
const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";
/// 表情白名单（rr3-interact 模块面同款 7 态；帧面执行面同步执行该裁定）。
const RR3_FACE_EXPRS: [&str; 7] = ["idle", "blink", "talk", "think", "happy", "listen", "turn"];

// ---------------------------------------------------------------------------
// 舵机换算（50Hz / 14bit：1 tick = 20ms / 16384 = 1.2207µs）
// ---------------------------------------------------------------------------

/// 0° 对应 duty ticks（0.5ms = 500µs / 1.2207µs ≈ 409.6 → 410）。
const DUTY_MIN: u32 = 410;
/// 180° 对应 duty ticks（2.5ms = 2048）。
const DUTY_MAX: u32 = 2048;

// ---------------------------------------------------------------------------
// 分轴行程（方案 C 裁定 2026-10-12；现场续批再裁：pan 由 ±20° 放宽到 ±30°）
// ——pan 相对中位 ±30°、tilt 相对中位 ±10°（tilt 由 ±5° 放宽，纳现场实测所需的
// 上抬 98°，行程 [80,100]）
// ---------------------------------------------------------------------------
//
// 真板实锤（批C v0.3.2 与现场排障）：云台机械行程远小于 SG90 全程 0–180°——
// 旧版 WANDER 全行程扫摆 / SCAN 五点（0/45/90/135/180）会把舵机推到机械死点
// 并**持续施力**，表现为"卡住不动 + 舵机发烫"（堵转电流 ~700mA/只持续流动），
// 叠加板载 USB 5V 直供时触发欠压复位与 USB 链路反复掉线。故本轴行程以中位为
// 基准收窄，越界**拒收点名**（fail-closed：静默钳到边界会让调用方误以为按给值
// 生效），WANDER/SCAN 的端点与判据点一律收进区间内。
// **pan ±30°（用户 2026-10-12 现场裁定，纳"左转 30°"所需）**：放宽后若机械到不了
// 边界会持续堵转——现场须目视，一旦卡住即断电（见 README 现场注记）。

/// 云台中位绝对角（两轴共用刻度中点；boot 置位与角度行基准）。
const AXIS_CENTER_DEG: u32 = 90;
/// pan（水平轴）相对中位的最大摆幅。
const PAN_LIMIT_DEG: u32 = 30;
/// tilt（俯仰轴）相对中位的最大摆幅。
const TILT_LIMIT_DEG: u32 = 10;
/// 开机默认姿态（用户现场裁定）：pan 左转 30°（中位 −30）、tilt 上抬 8°（中位 +8）。
const BOOT_PAN_DEG: u32 = AXIS_CENTER_DEG - 30;
const BOOT_TILT_DEG: u32 = AXIS_CENTER_DEG + 8;

/// 角度 → duty ticks（线性 0–180 ↦ 410–2048；入参已由调用方 clamp 0–180）。
fn deg_to_duty(deg: u32) -> u32 {
    DUTY_MIN + (deg * (DUTY_MAX - DUTY_MIN)) / 180
}

/// duty ticks → 角度（回读换算，与 deg_to_duty 互逆的整数近似，±1° 内）。
fn duty_to_deg(duty: u32) -> u32 {
    ((duty.saturating_sub(DUTY_MIN)) * 180 + (DUTY_MAX - DUTY_MIN) / 2) / (DUTY_MAX - DUTY_MIN)
}

// ---------------------------------------------------------------------------
// SSD1306 5x7 字模（ASCII 32–126，经典公共域 glcdfont 表；每字符 5 列 LSB 在左）
// ---------------------------------------------------------------------------

const FONT5X7: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x00, 0x00, 0x5F, 0x00, 0x00], // !
    [0x00, 0x07, 0x00, 0x07, 0x00], // "
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // #
    [0x24, 0x2A, 0x7F, 0x2A, 0x12], // $
    [0x23, 0x13, 0x08, 0x64, 0x62], // %
    [0x36, 0x49, 0x55, 0x22, 0x50], // &
    [0x00, 0x05, 0x03, 0x00, 0x00], // '
    [0x00, 0x1C, 0x22, 0x41, 0x00], // (
    [0x00, 0x41, 0x22, 0x1C, 0x00], // )
    [0x14, 0x08, 0x3E, 0x08, 0x14], // *
    [0x08, 0x08, 0x3E, 0x08, 0x08], // +
    [0x00, 0x50, 0x30, 0x00, 0x00], // ,
    [0x08, 0x08, 0x08, 0x08, 0x08], // -
    [0x00, 0x60, 0x60, 0x00, 0x00], // .
    [0x20, 0x10, 0x08, 0x04, 0x02], // /
    [0x3E, 0x51, 0x49, 0x45, 0x3E], // 0
    [0x00, 0x42, 0x7F, 0x40, 0x00], // 1
    [0x42, 0x61, 0x51, 0x49, 0x46], // 2
    [0x21, 0x41, 0x45, 0x4B, 0x31], // 3
    [0x18, 0x14, 0x12, 0x7F, 0x10], // 4
    [0x27, 0x45, 0x45, 0x45, 0x39], // 5
    [0x3C, 0x4A, 0x49, 0x49, 0x30], // 6
    [0x01, 0x71, 0x09, 0x05, 0x03], // 7
    [0x36, 0x49, 0x49, 0x49, 0x36], // 8
    [0x06, 0x49, 0x49, 0x29, 0x1E], // 9
    [0x00, 0x36, 0x36, 0x00, 0x00], // :
    [0x00, 0x56, 0x36, 0x00, 0x00], // ;
    [0x08, 0x14, 0x22, 0x41, 0x00], // <
    [0x14, 0x14, 0x14, 0x14, 0x14], // =
    [0x00, 0x41, 0x22, 0x14, 0x08], // >
    [0x02, 0x01, 0x51, 0x09, 0x06], // ?
    [0x32, 0x49, 0x79, 0x41, 0x3E], // @
    [0x7E, 0x11, 0x11, 0x11, 0x7E], // A
    [0x7F, 0x49, 0x49, 0x49, 0x36], // B
    [0x3E, 0x41, 0x41, 0x41, 0x22], // C
    [0x7F, 0x41, 0x41, 0x22, 0x1C], // D
    [0x7F, 0x49, 0x49, 0x49, 0x41], // E
    [0x7F, 0x09, 0x09, 0x09, 0x01], // F
    [0x3E, 0x41, 0x49, 0x49, 0x7A], // G
    [0x7F, 0x08, 0x08, 0x08, 0x7F], // H
    [0x00, 0x41, 0x7F, 0x41, 0x00], // I
    [0x20, 0x40, 0x41, 0x3F, 0x01], // J
    [0x7F, 0x08, 0x14, 0x22, 0x41], // K
    [0x7F, 0x40, 0x40, 0x40, 0x40], // L
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], // M
    [0x7F, 0x04, 0x08, 0x10, 0x7F], // N
    [0x3E, 0x41, 0x41, 0x41, 0x3E], // O
    [0x7F, 0x09, 0x09, 0x09, 0x06], // P
    [0x3E, 0x41, 0x51, 0x21, 0x5E], // Q
    [0x7F, 0x09, 0x19, 0x29, 0x46], // R
    [0x46, 0x49, 0x49, 0x49, 0x31], // S
    [0x01, 0x01, 0x7F, 0x01, 0x01], // T
    [0x3F, 0x40, 0x40, 0x40, 0x3F], // U
    [0x1F, 0x20, 0x40, 0x20, 0x1F], // V
    [0x3F, 0x40, 0x38, 0x40, 0x3F], // W
    [0x63, 0x14, 0x08, 0x14, 0x63], // X
    [0x07, 0x08, 0x70, 0x08, 0x07], // Y
    [0x61, 0x51, 0x49, 0x45, 0x43], // Z
    [0x00, 0x7F, 0x41, 0x41, 0x00], // [
    [0x02, 0x04, 0x08, 0x10, 0x20], // '\'
    [0x00, 0x41, 0x41, 0x7F, 0x00], // ]
    [0x04, 0x02, 0x01, 0x02, 0x04], // ^
    [0x40, 0x40, 0x40, 0x40, 0x40], // _
    [0x00, 0x01, 0x02, 0x04, 0x00], // `
    [0x20, 0x54, 0x54, 0x54, 0x78], // a
    [0x7F, 0x48, 0x44, 0x44, 0x38], // b
    [0x38, 0x44, 0x44, 0x44, 0x20], // c
    [0x38, 0x44, 0x44, 0x48, 0x7F], // d
    [0x38, 0x54, 0x54, 0x54, 0x18], // e
    [0x08, 0x7E, 0x09, 0x01, 0x02], // f
    [0x0C, 0x52, 0x52, 0x52, 0x3E], // g
    [0x7F, 0x08, 0x04, 0x04, 0x78], // h
    [0x00, 0x44, 0x7D, 0x40, 0x00], // i
    [0x20, 0x40, 0x44, 0x3D, 0x00], // j
    [0x7F, 0x10, 0x28, 0x44, 0x00], // k
    [0x00, 0x41, 0x7F, 0x40, 0x00], // l
    [0x7C, 0x04, 0x18, 0x04, 0x78], // m
    [0x7C, 0x08, 0x04, 0x04, 0x78], // n
    [0x38, 0x44, 0x44, 0x44, 0x38], // o
    [0x7C, 0x14, 0x14, 0x14, 0x08], // p
    [0x08, 0x14, 0x14, 0x18, 0x7C], // q
    [0x7C, 0x08, 0x04, 0x04, 0x08], // r
    [0x48, 0x54, 0x54, 0x54, 0x20], // s
    [0x04, 0x3F, 0x44, 0x40, 0x20], // t
    [0x3C, 0x40, 0x40, 0x20, 0x7C], // u
    [0x1C, 0x20, 0x40, 0x20, 0x1C], // v
    [0x3C, 0x40, 0x30, 0x40, 0x3C], // w
    [0x44, 0x28, 0x10, 0x28, 0x44], // x
    [0x0C, 0x50, 0x50, 0x50, 0x3C], // y
    [0x44, 0x64, 0x54, 0x4C, 0x44], // z
    [0x00, 0x08, 0x36, 0x41, 0x00], // {
    [0x00, 0x00, 0x7F, 0x00, 0x00], // |
    [0x00, 0x41, 0x36, 0x08, 0x00], // }
    [0x08, 0x08, 0x2A, 0x1C, 0x08], // ~
];

/// 可见 ASCII 取字模（其余一律空格字形——fail-closed，不越表下标）。
fn glyph(c: u8) -> [u8; 5] {
    let idx = match c {
        0x20..=0x7E => (c - 0x20) as usize,
        _ => 0,
    };
    FONT5X7[idx]
}

// ---------------------------------------------------------------------------
// SSD1306 原语驱动（命令流 0x00 控制字节 / 数据流 0x40 控制字节；页寻址模式）
// ---------------------------------------------------------------------------

/// 带重试的 I2C 写（**总线自愈 + 瞬态兜底**）：失败先 [`i2c_bus_recover`] 解卡再重发，
/// 至多 [`I2C_RETRY`] 次、每次隔 [`I2C_RETRY_DELAY_MS`]。SSD1306 每笔
/// START-addr-bytes-STOP 独立，重发安全。成功即返回；全败才回 Err（文案带重试次数
/// 与末次错因，便于 monitor 判因）。
fn i2c_write_retry(i2c: &mut I2cDriver, addr: u8, bytes: &[u8]) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..=I2C_RETRY {
        match i2c.write(addr, bytes, I2C_TIMEOUT) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = format!("{e:?}");
                if attempt < I2C_RETRY {
                    // 失败即先自愈总线再退避重发（丢拍锁死是本工程主故障模式）
                    i2c_bus_recover(false);
                    thread::sleep(Duration::from_millis(I2C_RETRY_DELAY_MS));
                }
            }
        }
    }
    Err(format!("（重试 {I2C_RETRY} 次后）{last}"))
}

/// I2C 总线自愈（**运行期恢复，与启动期 [`i2c_bus_unstick`] 同机理**）：SSD1306 在
/// 某笔事务中途丢拍（总线噪声/轨压瞬降）时会把自己锁在"等后续时钟"状态——此后主机
/// 每次 START 它一律不应答（真板实录：8 页胶囊眼密集页写中一笔数据段失败后，其后
/// **所有**事务恒 `ESP_FAIL`，重试 6 次 × 5ms 不复、锁死到复位；而短命令笔与 boot
/// 4 页因未撞上丢拍恒过）。恢复动作三步：
/// 1) 把 SDA/SCL 抢回 GPIO（IOMUX 由 I2C 切到 GPIO 功能）；
/// 2) 若 SDA 被从机拉住 → 手动打 9 个 SCL 时钟走完其位计数器，再补 STOP 释放；
/// 3) `i2c_set_pin` 把两脚**交还 I2C 外设**（否则位置留在 GPIO 功能，后续事务全废）。
/// 与启动期版本唯一差别就是第 3 步（启动期驱动尚未装，不需要交还）。
///
/// `force` 参数：**无条件**打 9 时钟 + STOP。真板实锤（2026-10-12 第二批）：舵机电机
/// 换向噪声会在**空闲期**（两笔事务之间）把 SSD1306 的 I2C 状态机顶进"半途态"——此后
/// 恒 NACK，而**此刻 SDA 是高电平**（不是被拉低的"锁死"），故条件版（只在 SDA 低时打
/// 时钟）根本不触发、救不回来。故在每次动作收尾（尤其舵机动作后）**强制**重同步一次。
fn i2c_bus_recover(force: bool) {
    use esp_idf_sys as sys;
    const SDA: i32 = 21;
    const SCL: i32 = 22;
    unsafe {
        sys::gpio_set_direction(SDA, sys::gpio_mode_t_GPIO_MODE_INPUT);
        sys::gpio_set_pull_mode(SDA, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
        sys::gpio_set_direction(SCL, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
        sys::gpio_set_pull_mode(SCL, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
        sys::gpio_set_level(SCL, 1);
        sys::esp_rom_delay_us(5);
        let stuck = sys::gpio_get_level(SDA) == 0;
        if force || stuck {
            // SDA 被从机拉低 = 丢拍锁死：9 个时钟走完从机位计数器 + STOP 释放。
            // force 但 SDA 高 = 半途态：同样打 9 时钟 + STOP 把从机推回 idle。
            if stuck {
                println!("[periph] I2C 自愈：SDA 被从机拉低（丢拍锁死），打 9 时钟 + STOP");
            }
            for _ in 0..9 {
                sys::gpio_set_level(SCL, 0);
                sys::esp_rom_delay_us(5);
                sys::gpio_set_level(SCL, 1);
                sys::esp_rom_delay_us(5);
            }
            sys::gpio_set_direction(SDA, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
            sys::gpio_set_level(SDA, 0);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_level(SCL, 1);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_level(SDA, 1);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_direction(SDA, sys::gpio_mode_t_GPIO_MODE_INPUT);
        }
        // 交还 I2C 外设（IOMUX 复位为 I2C 功能 + 内部上拉），驱动内部状态不受影响
        sys::i2c_set_pin(
            sys::i2c_port_t_I2C_NUM_0,
            SDA,
            SCL,
            true,
            true,
            sys::i2c_mode_t_I2C_MODE_MASTER,
        );
    }
}

/// 发命令（控制字节 0x00 + 命令字节序列）。SSD1306 控制字节 Co=0/D/C#=0 表示
/// 其后全部为命令——流式命令面单笔 I2C 写合法（数据手册 §8.1.5.1；init 全序列
/// 25B 一笔发，b7 真板实锤：8B 缓冲护栏误拒 init 致"命令序列超长"假故障）。
fn oled_cmd(i2c: &mut I2cDriver, cmds: &[u8]) -> Result<(), String> {
    let mut buf = [0u8; 32];
    buf[0] = 0x00;
    if cmds.len() > 31 {
        return Err("命令序列超长".into());
    }
    buf[1..=cmds.len()].copy_from_slice(cmds);
    i2c_write_retry(i2c, OLED_ADDR, &buf[..=cmds.len()])
        .map_err(|e| format!("I2C 命令写失败: {e}"))
}

/// 写一页 GDDRAM（128 列）。**分块写**：每笔 ≤33 字节（1 控制字节 + 32 数据），
/// 真板实录——129B 单笔在跳线/弱上拉面连重试 3 次仍 ESP_FAIL，而短命令笔（≤26B）
/// 恒过：ESP32 老 I2C 驱动的硬件 FIFO 即 32B，长事务是其边际路径。SSD1306 页寻址
/// 下列指针跨事务自动递增（数据手册 §8.1.5.2 连续数据模式），分块续写等价于单笔。
///
/// **整页重试**（外层）：任一块失败 → [`i2c_write_retry`] 内部已解卡，但丢拍后从机
/// 列指针已错位，故本层**把整页（页地址 + 128B）从确定性起点重写**，至多
/// [`I2C_PAGE_ATTEMPTS`] 次；块间留 [`I2C_CHUNK_GAP_MS`]。
fn oled_page(i2c: &mut I2cDriver, page: u8, data: &[u8; 128]) -> Result<(), String> {
    /// 单笔数据字节上限（= 硬件 FIFO 32B，留控制字节外的整块）。
    const CHUNK: usize = 32;
    let mut last = String::from("未知");
    for _ in 0..I2C_PAGE_ATTEMPTS {
        match oled_page_once(i2c, page, data, CHUNK) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(format!("（整页重写 {I2C_PAGE_ATTEMPTS} 次后）{last}"))
}

/// 单页单趟写（页地址命令 + 分块数据）。失败即返回，由 [`oled_page`] 决定整页重写。
fn oled_page_once(
    i2c: &mut I2cDriver,
    page: u8,
    data: &[u8; 128],
    chunk: usize,
) -> Result<(), String> {
    oled_cmd(i2c, &[0xB0 | page, 0x00, 0x10])?;
    let mut buf = [0u8; 33];
    buf[0] = 0x40;
    let mut off = 0usize;
    while off < data.len() {
        let n = chunk.min(data.len() - off);
        buf[1..1 + n].copy_from_slice(&data[off..off + n]);
        i2c_write_retry(i2c, OLED_ADDR, &buf[..1 + n])
            .map_err(|e| format!("I2C 页数据写失败 page={page} off={off} n={n}: {e}"))?;
        off += n;
        if off < data.len() {
            thread::sleep(Duration::from_millis(I2C_CHUNK_GAP_MS));
        }
    }
    Ok(())
}

/// 初始化（页寻址模式 + 电荷泵开 + 全清屏；序列为 SSD1306 数据手册标准序）。
fn oled_init(i2c: &mut I2cDriver) -> Result<(), String> {
    oled_cmd(
        i2c,
        &[
            0xAE, // display off
            0xD5, 0x80, // clock div
            0xA8, 0x3F, // multiplex 64
            0xD3, 0x00, // display offset
            0x40, // start line 0
            0x8D, 0x14, // charge pump on
            0x20, 0x02, // page addressing mode
            0xA1, // segment remap
            0xC8, // COM scan dec
            0xDA, 0x12, // COM pins
            0x81, OLED_CONTRAST_LEVELS[0], // contrast（**初始中档**；见 OLED_CONTRAST_LEVELS
                         // ——满亮 0xCF 会在胶囊眼大面亮点上屏时反噬 I2C，故起点即已调低，
                         // 失败再逐级往下走）
            0xD9, 0xF1, // precharge
            0xDB, 0x40, // VCOM detect
            0xA4, // resume from RAM
            0xA6, // normal display
            0xAF, // display on
        ],
    )?;
    oled_clear(i2c)
}

/// 全清屏（8 页 × 128B = 1KB 全零）。
fn oled_clear(i2c: &mut I2cDriver) -> Result<(), String> {
    for page in 0u8..8 {
        oled_page(i2c, page, &[0u8; 128])?;
    }
    Ok(())
}

/// 画一行文本（row 0/1 ↦ 页 0/2；≤21 可见字符，超出截断——行协议侧同样约束）。
fn oled_text(i2c: &mut I2cDriver, row: usize, text: &[u8]) -> Result<(), String> {
    if row >= OLED_ROWS {
        return Err(format!("文本行号越界: {row}"));
    }
    let page = (row * 2) as u8;
    let mut buf = [0u8; 128];
    let mut col = 0usize;
    for &c in text.iter().take(OLED_COLS) {
        if col + 6 > 128 {
            break;
        }
        buf[col..col + 5].copy_from_slice(&glyph(c));
        col += 6; // 5px 字模 + 1px 空列
    }
    oled_page(i2c, page, &buf)
}

/// I2C 总线扫描（0x03–0x77 逐地址单字节 0x00 探测，返回应答地址表）。
/// 判据①：0x3C 必在表内（全案 §4 批B ① 预期输出）。
fn i2c_scan(i2c: &mut I2cDriver) -> Vec<u8> {
    let mut found = Vec::new();
    for addr in 0x03u8..=0x77 {
        if i2c.write(addr, &[0x00], I2C_TIMEOUT).is_ok() {
            found.push(addr);
        }
    }
    found
}

// ---------------------------------------------------------------------------
// 外设面（控制面/运行面分离）：控制面独占 UART2，运行面独占 I2C0 + LEDC
// ---------------------------------------------------------------------------

/// 控制面独占的 UART2（指令面 io：行面直用固有方法，帧面经 [`UartIo`] 窄缝）。
/// 驱动 'static（外设 token 即 'static 无借用绑定；move 进线程后 esp_restart
/// 前永不 drop）。UartDriver 上游已标 `Send`，本结构自动 Send。
struct CtlUart {
    uart: UartDriver<'static>,
}

/// 运行面独占的外设面：I2C（OLED）+ 两轴 LEDC 舵机。**全部硬件慢动作在此面执行**
/// ——慢 I2C 与舵机整定都不阻塞控制面的命令吞吐。驱动 'static 同上；
/// I2cDriver/LedcDriver 上游已标 `Send`，本结构自动 Send（无需 unsafe impl）。
struct RtPeriph {
    i2c: I2cDriver<'static>,
    pan: LedcDriver<'static>,
    tilt: LedcDriver<'static>,
    /// 各轴**记账角**（缓速步进的起点：只有本面驱动舵机，故本面记账即真值；duty=0
    /// 未激磁期以中位起算，避免首帧从 0° 起跳的假行程）。
    pan_deg: u32,
    tilt_deg: u32,
    /// 当前 OLED 亮度档下标（整板电压降级用；见 [`OLED_CONTRAST_LEVELS`]）。
    contrast_idx: usize,
}

/// 轴行程校验（**控制面纯函数**：只算不碰硬件，故控制面能在下发作业前即时拒）。
/// 与运行面 [`RtPeriph::servo_set`] 共用同一判据、同一文案（"一处真值、两个检查点
/// 复用同一判定"——两面都调本函数，杜绝控制/运行口径漂移）。
fn axis_check(axis: Axis, deg: u32) -> Result<u32, String> {
    if deg > 180 {
        return Err(format!("角度超范围 0-180: {deg}"));
    }
    let (lo, hi) = axis.range();
    if !(lo..=hi).contains(&deg) {
        return Err(format!(
            "{} 轴超出行程 [{lo}, {hi}]（中位 {AXIS_CENTER_DEG}° ±{}°）: {deg}",
            axis.name(),
            axis.limit_deg()
        ));
    }
    Ok(deg)
}

impl RtPeriph {
    /// 舵机转角（先过本轴行程裁决 → **缓速步进**逼近 → 返回实际设定角）。
    /// 协议级 0–180 之外的角先被拒；轴行程（中位 ± 摆幅）之外**点名拒收**，
    /// 不静默钳位——见文件头"分轴行程"段。缓速步进见 [`SERVO_SLEW_STEP_DEG`]
    /// （极限电源下的软启动：把整段跳变的电流尖峰摊成涓流）。
    fn servo_set(&mut self, axis: Axis, deg: u32) -> Result<u32, String> {
        let deg = axis_check(axis, deg)?;
        let cur = match axis {
            Axis::Pan => self.pan_deg,
            Axis::Tilt => self.tilt_deg,
        };
        let mut step = cur;
        while step != deg {
            step = if deg > step {
                (step + SERVO_SLEW_STEP_DEG).min(deg)
            } else {
                step.saturating_sub(SERVO_SLEW_STEP_DEG).max(deg)
            };
            let driver = match axis {
                Axis::Pan => &mut self.pan,
                Axis::Tilt => &mut self.tilt,
            };
            driver
                .set_duty(deg_to_duty(step))
                .map_err(|e| format!("LEDC set_duty 失败: {e:?}"))?;
            if step != deg {
                thread::sleep(Duration::from_millis(SERVO_SLEW_STEP_MS));
            }
        }
        match axis {
            Axis::Pan => self.pan_deg = deg,
            Axis::Tilt => self.tilt_deg = deg,
        }
        Ok(deg)
    }

    /// 下调一档 OLED 亮度（**整板电压降级**：见 [`OLED_CONTRAST_LEVELS`]）。已到地板
    /// 返回 false（调用方据此收手）；未到则写新档并返回 true。
    fn degrade_contrast(&mut self) -> bool {
        if self.contrast_idx + 1 >= OLED_CONTRAST_LEVELS.len() {
            return false;
        }
        self.contrast_idx += 1;
        let lv = OLED_CONTRAST_LEVELS[self.contrast_idx];
        match oled_cmd(&mut self.i2c, &[0x81, lv]) {
            Ok(()) => println!("[periph] OLED 降亮 → 0x{lv:02X}（轨压降级第 {} 档）", self.contrast_idx),
            Err(e) => eprintln!("[periph] 降亮写失败: {e}"),
        }
        true
    }

    /// 渲染胶囊眼（**带整板电压降级**）：整屏渲染失败即降一档亮度重来，到地板仍败
    /// 才回 Err。真板主故障（大面亮点 → 轨压/地弹 → I2C 丢拍）由此闭环自愈。
    fn capsules_degrading(&mut self, open: u32, dx: i16, dy: i16) -> Result<(), String> {
        loop {
            match oled_capsules(&mut self.i2c, open, dx, dy) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if !self.degrade_contrast() {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// 舵机让位（**供电分时仲裁**）：板载 5V 轨上 SSD1306 电荷泵与舵机保持电流
    /// **不能共存**——真板实锤（2026-10-12）：两轴激磁时 OLED 页写/命令笔恒
    /// `ESP_FAIL`（地址单笔因弱上拉假 ACK 仍"通过"，具欺骗性），而**掉 duty 后同一笔
    /// 立刻 Ok**；且激磁越久越顽固（一度到掉 duty 也不复、需重走 init）。
    /// 故除"正在移动"外一律释放两轴：把 5V 轨整段让给 OLED。运行面单线程、渲染与
    /// 移动在时间上本就串行，故本函数在**每次动作收尾**调用即可保证两者零重叠。
    fn servo_release(&mut self) {
        let _ = self.pan.set_duty(0);
        let _ = self.tilt.set_duty(0);
    }

    /// OLED 文本 3 行（21 列硬切）+ 角度行（row 3）刷新。
    fn oled_show(&mut self, text: &str, pan_deg: u32, tilt_deg: u32) -> Result<(), String> {
        // 先过滤成可见 ASCII（其余转空格，与 glyph 行为一致）再按 21 列硬切
        // 3 行——字节切片不劈多字节字符（char boundary 安全），行对齐不漂移。
        let ascii: String = text
            .chars()
            .map(|c| {
                if (0x20..=0x7E).contains(&(c as u32)) {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        let bytes = ascii.as_bytes();
        for row in 0..(OLED_ROWS - 1) {
            let start = row * OLED_COLS;
            let chunk = if start < bytes.len() {
                let end = (start + OLED_COLS).min(bytes.len());
                &bytes[start..end]
            } else {
                b"" as &[u8]
            };
            oled_text(&mut self.i2c, row, chunk)?;
        }
        let angle_line = format!("P:{pan_deg:03} T:{tilt_deg:03}");
        oled_text(&mut self.i2c, OLED_ROWS - 1, angle_line.as_bytes())
    }
}

#[derive(Clone, Copy)]
enum Axis {
    Pan,
    Tilt,
}

impl Axis {
    /// 轴名（点名文案用，与帧面 `axis` 键取值同串）。
    fn name(self) -> &'static str {
        match self {
            Axis::Pan => "pan",
            Axis::Tilt => "tilt",
        }
    }

    /// 本轴相对中位的最大摆幅。
    fn limit_deg(self) -> u32 {
        match self {
            Axis::Pan => PAN_LIMIT_DEG,
            Axis::Tilt => TILT_LIMIT_DEG,
        }
    }

    /// 本轴绝对行程区间 `[lo, hi]` = 中位 ± 摆幅（WANDER 端点与 SCAN 判据点
    /// 都从这里取，机制不另写字面量）。
    fn range(self) -> (u32, u32) {
        (
            AXIS_CENTER_DEG - self.limit_deg(),
            AXIS_CENTER_DEG + self.limit_deg(),
        )
    }
}

/// 轴行程五等分判据点（SCAN 判据：端点 + 三分点，全部落在本轴行程内——
/// 旧版写死的 0/45/135/180 越出机械行程，正是堵转来源）。
fn axis_scan_points(axis: Axis) -> [u32; 5] {
    let (lo, hi) = axis.range();
    let span = hi - lo;
    [lo, lo + span / 4, lo + span / 2, lo + span * 3 / 4, hi]
}

/// 应答行回写（UART2；\r\n 结尾，终端与 pyserial 同读友好）。
fn reply(uart: &mut UartDriver, line: &str) {
    let _ = uart.write(format!("{line}\r\n").as_bytes());
    // 整装 UartDriver 未实现 std::io::Write（vendored 面只有 embedded_io::Write），
    // 用固有 wait_tx_done(portMAX_DELAY) 等移位完成——语义即"应答发完再返回"。
    let _ = uart.wait_tx_done(u32::MAX);
}

/// 收一行（阻塞逐字节读直到 '\n'；'\r' 剥离；超 [LINE_CAP] 丢弃整行回 Err）。
/// `first` 是 cmd_loop 双面分流时已消耗的首字节（非 0x31 的行面回流字节）。
fn read_line(uart: &mut UartDriver, first: u8) -> Result<String, &'static str> {
    let mut buf = Vec::new();
    if first == b'\n' {
        // 空行保持批B 行为：静默空串（cmd_loop trim 后跳过）
        return Ok(String::new());
    }
    buf.push(first);
    let mut byte = [0u8; 1];
    loop {
        let n = uart
            .read(&mut byte, u32::MAX)
            .map_err(|_| "uart read 失败")?;
        if n == 0 {
            continue;
        }
        match byte[0] {
            b'\n' => {
                while matches!(buf.last(), Some(b'\r')) {
                    buf.pop();
                }
                return Ok(String::from_utf8_lossy(&buf).into_owned());
            }
            b => {
                buf.push(b);
                if buf.len() > LINE_CAP {
                    return Err("line too long");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 帧面（批C）：rr3.call 能力帧执行——宿主 rr-hal::esp32_uart 桥后端直连点名
// ---------------------------------------------------------------------------

/// UART2 的帧面 io 窄缝：驱动可变借用 + cmd_loop 已窥视消耗的首字节。实现
/// rr-ipc [`Rr3ByteStream`] 两 `usize` 方法（板侧接帧语义的全部——transport
/// mod.rs 形状用例为证），经 blanket impl 免费获得 rr3_send_buf/rr3_recv_buf
/// ——与宿主/桥/哨兵同源帧语义，零第二份帧层。行面继续直用 UartDriver 固有
/// 方法（逐字节语义），两面互不侵扰。
struct UartIo<'a> {
    uart: &'a mut UartDriver<'static>,
    /// 窥视消耗的帧 magic 首字节（0x31）：Read 首次先吐它，128B 头的 magic
    /// 窗口才完整；发 Response 时此位恒 None（无窥视）。
    pre: Option<u8>,
}

impl Rr3ByteStream for UartIo<'_> {
    fn rr3_read_into(&mut self, buf: &mut [u8]) -> usize {
        if let Some(b) = self.pre.take() {
            if !buf.is_empty() {
                buf[0] = b;
                return 1;
            }
            return 0;
        }
        // 固有 read（阻塞，delay = portMAX_DELAY）；错误折叠为 0（缝上无错误
        // 通道，Closed 形状）——细节 eprintln 当场可见（批J G5 口径）。
        match self.uart.read(buf, u32::MAX) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[periph] UART 读失败: {e:?}");
                0
            }
        }
    }

    fn rr3_write_all_bytes(&mut self, buf: &[u8]) -> usize {
        let mut done = 0usize;
        while done < buf.len() {
            match self.uart.write(&buf[done..]) {
                Ok(0) => break, // 无进展（WriteZero 形状）
                Ok(n) => done += n,
                Err(e) => {
                    eprintln!("[periph] UART 写失败: {e:?}");
                    break;
                }
            }
        }
        // 等移位完成再返回（reply 同款"应答发完再返回"语义）
        let _ = self.uart.wait_tx_done(u32::MAX);
        done
    }
}

/// 帧头 timestamp（esp_timer 单调时基 µs→ns；epoch 无关——时间戳只作诊断参考
/// 不参与校验，板侧取不到 epoch 墙钟，与宿主 rr3_now_ns 各自独立合法）。
fn rr3_frame_now_ns() -> u64 {
    (unsafe { esp_timer_get_time() }).unsigned_abs() * 1000
}

/// 控制面/运行面共享状态（[`Bus`] 内，`Arc<Mutex<..>>` 护）：行面/帧面的文本与
/// 角度记账 + 自主行为（表情/扫摆）时钟。**不含任何硬件句柄**（故自动 Send）——
/// 控制面改策略字段，运行面读之执行 + 回写自主推进的角度/拍号。帧面 expression
/// 应答的 previous 字段不在本结构（命令由控制面串行化，记账在 [`CtlState`]）。
struct PeriphState {
    /// 最近一次 OLED 文本（行面 OLED 命令 / 帧面 expression 出字）
    last_text: String,
    pan_deg: u32,
    tilt_deg: u32,
    /// 显示模式：true=帧面 64x64 点阵位图占全屏（舵机角度行不刷，不凿位图）；
    /// 行面 OLED 命令 / 帧面 expression 复位 false。
    frame_mode: bool,
    /// 表情模式：true=胶囊眼 + 生命感微动作（帧面 expression 专有）；位图占屏
    /// 与行面文本模式复位 false（动画只在此模式跑）。
    expr_mode: bool,
    /// 表情基态（expr_eye_base 预设）：眼开合% + 水平/垂直扫视偏移
    base_open: u32,
    base_dx: i16,
    base_dy: i16,
    /// 微动作时钟（esp_timer ms）：眨眼/扫视的下一触发点与进行中截止点 +
    /// 上次渲染合成参数（变化才重绘，省 I2C）。
    next_blink_ms: u64,
    blink_until_ms: u64,
    next_glance_ms: u64,
    glance_until_ms: u64,
    glance_dx: i16,
    glance_dy: i16,
    drawn: (u32, i16, i16),
    /// 云台自主扫摆（WANDER 行面开关）：pan/tilt 缓速连续摆动不停顿。外部
    /// PAN/TILT/SERVO/SCAN 或帧面 servo 写到达即关（外部点名优先）。
    wander_on: bool,
    wander_pan_dir: i8,
    wander_tilt_dir: i8,
    /// 扫摆节拍：60ms 一拍的拍号（u8 回绕无妨，只用模数）+ 下拍时间。
    wander_phase: u8,
    next_wander_ms: u64,
}

impl PeriphState {
    /// 开机初值（中位 90 / 文本 "(boot)" / 无表情无扫摆）。
    fn boot() -> Self {
        Self {
            last_text: String::from("(boot)"),
            pan_deg: AXIS_CENTER_DEG,
            tilt_deg: AXIS_CENTER_DEG,
            frame_mode: false,
            expr_mode: false,
            base_open: 100,
            base_dx: 0,
            base_dy: 0,
            next_blink_ms: 0,
            blink_until_ms: 0,
            next_glance_ms: 0,
            glance_until_ms: 0,
            glance_dx: 0,
            glance_dy: 0,
            drawn: (100, 0, 0),
            wander_on: false,
            wander_pan_dir: 0,
            wander_tilt_dir: 0,
            wander_phase: 0,
            next_wander_ms: 0,
        }
    }

    /// 角度行是否该刷：帧模式/表情模式占满 8 页，刷角度行会凿穿画面（跳过）。
    fn angle_row_visible(&self) -> bool {
        !(self.frame_mode || self.expr_mode)
    }

    /// 表情切换时重置微动作时钟：立即显基态，眨眼/扫视各按随机间隔推迟。
    fn anim_reset(&mut self) {
        let now = now_ms();
        self.blink_until_ms = 0;
        self.glance_until_ms = 0;
        self.next_blink_ms = now + 2800 + (esp_random_u32() % 2400) as u64;
        self.next_glance_ms = now + 4000 + (esp_random_u32() % 4000) as u64;
    }

    /// 表情微动作决策（**纯计算，不碰硬件**——运行面决策段持锁调用）：自动眨眼
    /// （2.8–5.2s 随机间隔，闭 130ms）+ 随机微扫视（4–8s 间隔，水平 ±6px、垂直
    /// ±2 行，500–900ms 回位）。返回需要重绘的合成参数（None = 与上次一致，
    /// 省 I2C 不重绘）；非表情模式恒 None。渲染由运行面脱锁执行。
    fn anim_decide(&mut self) -> Option<(u32, i16, i16)> {
        if !self.expr_mode {
            return None;
        }
        let now = now_ms();
        if self.blink_until_ms == 0 {
            if now >= self.next_blink_ms {
                self.blink_until_ms = now + 130;
                self.next_blink_ms = now + 2800 + (esp_random_u32() % 2400) as u64;
            }
        } else if now >= self.blink_until_ms {
            self.blink_until_ms = 0;
        }
        let open = if self.blink_until_ms != 0 {
            4
        } else {
            self.base_open
        };
        let mut dx = self.base_dx;
        let mut dy = self.base_dy;
        if self.glance_until_ms == 0 {
            if now >= self.next_glance_ms {
                self.glance_dx = (esp_random_u32() % 13) as i16 - 6;
                self.glance_dy = match esp_random_u32() % 3 {
                    0 => -2,
                    1 => 2,
                    _ => 0,
                };
                self.glance_until_ms = now + 500 + (esp_random_u32() % 400) as u64;
                self.next_glance_ms = now + 4000 + (esp_random_u32() % 4000) as u64;
            }
        } else if now >= self.glance_until_ms {
            self.glance_until_ms = 0;
        } else {
            dx += self.glance_dx;
            dy += self.glance_dy;
        }
        let key = (open, dx, dy);
        if key != self.drawn {
            self.drawn = key;
            Some(key)
        } else {
            None
        }
    }

    /// 云台自主扫摆决策（**纯计算**，60ms 一拍）：pan 每 2 拍走 2°（≈16.7°/s，
    /// 行程 40° 单程约 2.4s）、tilt 每 4 拍走 3°（≈12.5°/s，行程 10° 单程约 1s），
    /// 端点折返持续不停。**端点 = 本轴行程边界**（[`Axis::range`]，方案 C 分轴限幅：
    /// pan 中位±30° / tilt 中位±10°）——旧版折返点在 0/180，会把舵机推过机械死点
    /// 并持续施力（真板表现为卡死 + 发烫）。步幅下限实测定标：tilt 托整个云台上部
    /// （重载），2°/300ms 部分落死区里视觉近乎不动——tilt 用 3°（≈27 ticks）可见。
    /// **串行降级：同一拍只驱动一轴**（tilt 拍占位时该拍跳过 pan）——两轴错拍，
    /// 削并行峰值电流（适配板载 5V 直供）。返回本拍要点名的单轴动作（None=不动）
    /// + 是否刷角度行（每 20 拍 = 1.2s 一次，省 I2C；模式守卫在刷新处再判）。
    fn wander_decide(&mut self) -> (Option<(Axis, u32)>, bool) {
        if !self.wander_on || now_ms() < self.next_wander_ms {
            return (None, false);
        }
        self.next_wander_ms = now_ms() + 60;
        self.wander_phase = self.wander_phase.wrapping_add(1);
        let mv = if self.wander_phase % 4 == 0 {
            let next = wander_step(
                self.tilt_deg,
                &mut self.wander_tilt_dir,
                3,
                Axis::Tilt.range(),
            );
            self.tilt_deg = next;
            Some((Axis::Tilt, next))
        } else if self.wander_phase % 2 == 0 {
            let next = wander_step(
                self.pan_deg,
                &mut self.wander_pan_dir,
                2,
                Axis::Pan.range(),
            );
            self.pan_deg = next;
            Some((Axis::Pan, next))
        } else {
            None
        };
        (mv, self.wander_phase % 20 == 0)
    }
}

/// 扫摆步进：deg ± step，轴行程端点折返后返回新角度。dir 应为 ±1
/// （WANDER 开关随机起步；0 防御上不会出现——0 时原地不动）。
/// `bounds` 由 [`Axis::range`] 给出，端点即机械安全边界。
fn wander_step(deg: u32, dir: &mut i8, step: i32, bounds: (u32, u32)) -> u32 {
    let (lo, hi) = bounds;
    let mut next = deg as i32 + *dir as i32 * step;
    if next >= hi as i32 {
        next = hi as i32;
        *dir = -1;
    } else if next <= lo as i32 {
        next = lo as i32;
        *dir = 1;
    }
    next as u32
}

/// 帧面请求（协议门全过后的视图）：id/method/params 摘出，帧头位形随行携带
/// （应答回带 trace_id / dst=src_task 用）。
struct FrameReq {
    id: u64,
    method: String,
    topic: String,
    args: Value,
    src_task: u64,
    trace_id: u64,
}

/// 收帧 + 协议门（全过才返回 Ok）：`rr3_framebuf_read` 整帧收 → 帧还原（magic/
/// 长度校验在 rr-ipc）→ dst 门（非本板位形丢弃）→ payload 前缀门（非 Request
/// 丢弃）→ JSON 门 → id 门。任一门失败回 Err = **丢弃不回帧**（fail-closed：
/// 帧级不可信的请求不作确定性应答；宿主侧重开连接即恢复）。半帧失败后流上可能
/// 残留尾巴字节——下一轮 magic 窥视大概率落行面 ERR，自愈路径在案。
fn frame_recv(io: &mut UartIo) -> Result<FrameReq, String> {
    let buf = io.rr3_recv_buf().map_err(|e| format!("收帧失败: {e:?}"))?;
    let frame = Rr3Frame::try_from(buf).map_err(|e| format!("帧还原失败: {e:?}"))?;
    if frame.header.dst_task != RR3_PERIPH_TASK {
        return Err(format!(
            "dst_task={} 非本板位形 {RR3_PERIPH_TASK}",
            frame.header.dst_task
        ));
    }
    let (ty, body) =
        rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
    if ty != Rr3PayloadType::Request {
        return Err(format!("payload 类型非 Request: {ty:?}"));
    }
    let v: Value = serde_json::from_slice(&body).map_err(|e| format!("请求体非 JSON: {e}"))?;
    let id = v.get("id").and_then(Value::as_u64).ok_or("请求缺 id")?;
    let method = v
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let (topic, args) = match v.get("params") {
        Some(p) => (
            p.get("topic")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            p.get("args").cloned().unwrap_or(Value::Null),
        ),
        None => (String::new(), Value::Null),
    };
    Ok(FrameReq {
        id,
        method,
        topic,
        args,
        src_task: frame.header.src_task,
        trace_id: frame.header.trace_id,
    })
}

/// 主题分发（**控制面**：只解析/校验/改状态/推作业，不碰硬件）：method 恒
/// rr3.call（rrd 转发形状，宿主桥同款）；主题只认两条在册能力面。method/主题/
/// args 层面的失败是**业务性失败**——折 result.error 点名回帧（模块面同款形状，
/// 宿主 rr3_bridge_parse_response 的解析契约）。
fn frame_dispatch(
    bus: &Arc<Mutex<Bus>>,
    ctl: &mut CtlState,
    req: &FrameReq,
) -> Result<Value, String> {
    if req.method != "rr3.call" {
        return Err(format!("method 非法: \"{}\"（恒 rr3.call）", req.method));
    }
    match req.topic.as_str() {
        RR3_SERVO_TOPIC => frame_handle_servo(bus, &req.args),
        RR3_FACE_TOPIC => frame_handle_face(bus, ctl, &req.args),
        other => Err(format!("未知主题: \"{other}\"")),
    }
}

/// 舵机执行（rr3-act/servo/command，**控制面**）：args 带 angle_deg = 写（校验 +
/// 状态记账 + 推 `Job::Move` 作业给运行面执行 + 生效角回显），不带 = 读（回状态
/// 记账角——LEDC 只由运行面本面作业改动，状态即真值、±10ms 内一致）。axis 可选
/// 缺省 pan（宿主单轴 trait ↔ 板上双轴的映射面，RR3_HAL_PERIPH_AXIS env + 本参数
/// 两级）。写路径推 `Job::RedrawOled` 令角度行随动（运行面按模式守卫决定刷否）。
fn frame_handle_servo(bus: &Arc<Mutex<Bus>>, args: &Value) -> Result<Value, String> {
    let axis = match args.get("axis") {
        None | Some(Value::Null) => Axis::Pan,
        Some(v) => match v.as_str() {
            Some("pan") => Axis::Pan,
            Some("tilt") => Axis::Tilt,
            _ => return Err(format!("axis 非法: {v}（pan|tilt）")),
        },
    };
    match args.get("angle_deg") {
        None | Some(Value::Null) => {
            let deg = {
                let b = lock_bus(bus);
                match axis {
                    Axis::Pan => b.st.pan_deg,
                    Axis::Tilt => b.st.tilt_deg,
                }
            };
            Ok(json!({ "angle_deg": deg, "move_ms": 0 }))
        }
        Some(v) => {
            let deg = v.as_u64().ok_or_else(|| format!("angle_deg 非法: {v}"))? as u32;
            let d = axis_check(axis, deg)?;
            {
                let mut b = lock_bus(bus);
                b.st.wander_on = false; // 外部点名优先，自主扫摆让位
                match axis {
                    Axis::Pan => b.st.pan_deg = d,
                    Axis::Tilt => b.st.tilt_deg = d,
                }
            }
            push_job(
                bus,
                match axis {
                    Axis::Pan => Job::Move {
                        pan: Some(d),
                        tilt: None,
                    },
                    Axis::Tilt => Job::Move {
                        pan: None,
                        tilt: Some(d),
                    },
                },
            );
            push_job(bus, Job::RedrawOled);
            Ok(json!({ "angle_deg": d, "move_ms": 0 }))
        }
    }
}

/// 表情执行（rr3-interact/face/expression，**控制面**）：expression 变体出胶囊眼
/// （基态预设 + 微动作动画，rr-hal-api Rr3FaceFrame 契约之外的表情名面）——控制面
/// 改状态（模式/基态/时钟）+ 推 `Job::RenderCapsules`，previous 记账在 [`CtlState`]；
/// frame 变体 8x8 点阵 → OLED 中央 64x64 放大渲染（HAL 渲染变体：行主序 8 字节、
/// 每字节 bit7 为该行最左点）——推 `Job::RenderFrame`。
fn frame_handle_face(
    bus: &Arc<Mutex<Bus>>,
    ctl: &mut CtlState,
    args: &Value,
) -> Result<Value, String> {
    if let Some(v) = args.get("expression") {
        let expr = v
            .as_str()
            .ok_or_else(|| format!("expression 非法: {v}（须字符串）"))?;
        if !RR3_FACE_EXPRS.contains(&expr) {
            return Err(format!("expression 不在白名单: \"{expr}\""));
        }
        let (base_open, base_dx, base_dy) = expr_eye_base(expr);
        let previous = std::mem::replace(&mut ctl.prev_expr, expr.to_string());
        {
            let mut b = lock_bus(bus);
            b.st.expr_mode = true;
            b.st.frame_mode = false;
            b.st.base_open = base_open;
            b.st.base_dx = base_dx;
            b.st.base_dy = base_dy;
            b.st.last_text = format!("EXPR {expr}");
            b.st.anim_reset();
            b.st.drawn = (base_open, base_dx, base_dy);
        }
        push_job(bus, Job::RenderCapsules);
        Ok(json!({ "expression": expr, "previous": previous }))
    } else if let Some(v) = args.get("frame") {
        let arr = v
            .as_array()
            .ok_or_else(|| format!("frame 非法: {v}（须 8 字节数组）"))?;
        if arr.len() != 8 {
            return Err(format!("frame 长度非法: {}（须 8）", arr.len()));
        }
        let mut f = [0u8; 8];
        for (i, item) in arr.iter().enumerate() {
            let b = item
                .as_u64()
                .ok_or_else(|| format!("frame[{i}] 非法: {item}"))?;
            if b > 255 {
                return Err(format!("frame[{i}] 超 u8: {b}"));
            }
            f[i] = b as u8;
        }
        {
            let mut b = lock_bus(bus);
            b.st.frame_mode = true;
            b.st.expr_mode = false;
        }
        push_job(bus, Job::RenderFrame(f));
        Ok(json!({ "applied": true }))
    } else {
        Err("缺 expression/frame 之一".into())
    }
}

/// 8x8 点阵帧 → OLED 中央 64x64 放大渲染：源帧每像素横向铺 8 列、纵向铺 8 行，
/// 落在列 32..96 × 全 8 页；页 p 全部 8 个位行同显源行 p（sy = (p*8+by)/8 = p）。
/// 位序映射：MAX7219 字节 bit7=行最左 ↔ 目标列 x=32 处（c=0 取 bit7）——方向
/// 不镜像；SSD1306 页内列字节 bit0=页顶。两侧留白清零（居中不残影）。
fn oled_frame(i2c: &mut I2cDriver, frame: &[u8; 8]) -> Result<(), String> {
    for page in 0u8..8 {
        let src = frame[page as usize];
        let mut buf = [0u8; 128];
        for (x, cell) in buf.iter_mut().enumerate().skip(32).take(64) {
            let c = (x - 32) / 8;
            *cell = if (src >> (7 - c)) & 1 == 1 { 0xFF } else { 0 };
        }
        oled_page(i2c, page, &buf)?;
    }
    Ok(())
}

/// esp_timer 毫秒（单调时基，微动作时钟源；u64 毫秒 58 万年不回绕）。
fn now_ms() -> u64 {
    (unsafe { esp_timer_get_time() }).unsigned_abs() / 1000
}

/// 硬件 RNG（WIFI/蓝牙未启也能用——ESP32 硬件熵源独立于无线栈）。
fn esp_random_u32() -> u32 {
    unsafe { esp_random() }
}

/// 胶囊眼渲染（表情模式画面）：两只竖直胶囊眼，开合 open_pct% 与扫视偏移
/// (dx,dy) 由表情基态 + 微动作动画合成。形状=圆角竖矩形（圆角半径取
/// min(半宽, 半高)：满高 26px 时即纯胶囊，眯缝低压扁成 18px 宽横缝不退化成
/// 圆盘）；行内实心段解析求宽（直段区全宽、圆角区 sqrt 余量），逐页写
/// GDDRAM。扫视出界自然裁剪（0..127 列钳制）不回卷。
fn oled_capsules(i2c: &mut I2cDriver, open_pct: u32, dx: i16, dy: i16) -> Result<(), String> {
    const W2: i32 = 9; // 眼半宽（眼宽 18px）
    const H_FULL: i32 = 26; // 满开眼高
    const CY: i32 = 32; // 眼中心行（64 行画布居中）
    const EYES: [i32; 2] = [42, 86]; // 眼中心列（128 列画布左右分置）
    let h = ((H_FULL * open_pct.clamp(4, 100) as i32) / 100).max(4);
    let half_h = h / 2;
    let rc = W2.min(half_h); // 圆角半径
    let straight = half_h - rc; // 垂直直段半长
    for page in 0u8..8 {
        let mut buf = [0u8; 128];
        let y0 = page as i32 * 8;
        for y in y0..y0 + 8 {
            let py = (y - (CY + dy as i32)).abs();
            if py > half_h {
                continue;
            }
            for &ecx in &EYES {
                let cx = ecx + dx as i32;
                let hw = if py <= straight {
                    W2
                } else {
                    let rem = rc * rc - (py - straight) * (py - straight);
                    if rem <= 0 {
                        continue;
                    }
                    (W2 - rc + (rem as f32).sqrt() as i32).min(W2)
                };
                for x in (cx - hw).max(0)..=(cx + hw).min(127) {
                    buf[x as usize] |= 1 << (y % 8);
                }
            }
        }
        oled_page(i2c, page, &buf)?;
    }
    Ok(())
}

/// 表情→胶囊眼基态预设（开合% / 水平扫视 / 垂直扫视）：七态白名单各有面目，
/// 微动作动画在此基态上叠加（眨眼/扫视见 [`PeriphState::anim_decide`]）。
fn expr_eye_base(expr: &str) -> (u32, i16, i16) {
    match expr {
        "blink" => (4, 0, 0),     // 眯缝
        "happy" => (55, 0, 2),    // 眯眼微垂
        "talk" => (70, 0, 0),     // 说话态半开
        "think" => (100, 6, -3),  // 抬眼右上
        "listen" => (100, -6, 0), // 侧耳向左
        "turn" => (100, 8, 0),    // 回望右方
        _ => (100, 0, 0),         // idle 与兜底：正目全开
    }
}

/// Response 回帧：result 在位（业务失败折 {"error": 文案}——宿主解析契约），
/// trace_id 原样回带、src=本板位形 2、dst=调用方、timestamp 单调时基。发送失败
/// eprintln 点名不重试（宿主 io 失败自会重开连接重发）。
fn frame_send_response(io: &mut UartIo, req: &FrameReq, result: Result<Value, String>) {
    let result = match result {
        Ok(v) => v,
        Err(e) => json!({ "error": e }),
    };
    let body = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
    let payload = rr3_body_encode(Rr3PayloadType::Response, body.to_string().as_bytes());
    let mut hdr = Rr3Header::rr3_hdr_broadcast(opcode::SEND, payload.len() as u32);
    hdr.trace_id = req.trace_id;
    hdr.timestamp_ns = rr3_frame_now_ns();
    hdr.src_task = RR3_PERIPH_TASK;
    hdr.dst_task = req.src_task;
    let resp = Rr3Frame {
        header: hdr,
        payload,
    };
    if let Err(e) = io.rr3_send_buf(Rr3FrameBuf::from(&resp)) {
        eprintln!("[periph] Response 发送失败: {e:?}");
    }
}

// ---------------------------------------------------------------------------
// 控制面 ↔ 运行面共享总线（Arc<Mutex>；作业/回包队列 + 共享状态）
// ---------------------------------------------------------------------------

/// 控制面 ↔ 运行面共享总线（哨兵 `Arc<Mutex>` 纪律同款）：`jobs` = 控制→运行的
/// 硬件作业队列，`outbox` = 运行→控制的异步回包行队列（SCAN 逐点），`st` = 双面
/// 共享状态。**锁只护读写，绝不跨 I2C 持有**（运行面"决策段持锁纯算，渲染段脱锁"）。
struct Bus {
    jobs: VecDeque<Job>,
    outbox: VecDeque<String>,
    st: PeriphState,
}

/// 控制面 → 运行面的硬件作业（**控制面只推作业，不碰硬件**；运行面取之执行）。
enum Job {
    /// 依 `st` 重绘文本 3 行 + 角度行（OLED 命令 / 舵机命令后角度行随动；模式守卫）。
    RedrawOled,
    /// 舵机移动（**串行降级**：双轴 = 先 pan 到位 → 整定 → 再 tilt；单轴直动）。
    Move { pan: Option<u32>, tilt: Option<u32> },
    /// 五点串行扫描（判据②；逐点异步回包走 outbox）。
    Scan,
    /// 依 `st` 基态渲染胶囊眼（表情切换/初态；后续微动作走自主节拍）。
    RenderCapsules,
    /// 渲染 8x8 点阵全屏放大。
    RenderFrame([u8; 8]),
}

/// 控制面私有状态（不跨线程）：帧面 expression 应答的 previous 字段——命令由控制面
/// 串行化，故 previous 记账在控制面即真值（无需随 `st` 跨线程）。
struct CtlState {
    prev_expr: String,
}

/// 取总线锁（中毒不失效：std 惰性 pthread 面，哨兵同款 `into_inner` 纪律）。
fn lock_bus(bus: &Arc<Mutex<Bus>>) -> std::sync::MutexGuard<'_, Bus> {
    bus.lock().unwrap_or_else(|p| p.into_inner())
}

/// 控制面推一件作业（唤醒运行面；运行面每轮取队列头执行）。
fn push_job(bus: &Arc<Mutex<Bus>>, job: Job) {
    lock_bus(bus).jobs.push_back(job);
}

/// 运行面 → 控制面的异步回包行（SCAN 逐点等；控制面每轮取空）。
fn outbox_push(bus: &Arc<Mutex<Bus>>, line: String) {
    lock_bus(bus).outbox.push_back(line);
}

/// 取空 outbox 并回写 UART2（控制面每轮首步；异步回包不阻塞命令吞吐）。
fn drain_outbox(uart: &mut UartDriver<'static>, bus: &Arc<Mutex<Bus>>) {
    loop {
        let line = lock_bus(bus).outbox.pop_front();
        match line {
            Some(l) => reply(uart, &l),
            None => break,
        }
    }
}

// ---------------------------------------------------------------------------
// 运行面（独占 I2C0 + LEDC）：执行硬件作业 + 自主节拍（表情微动作 / 云台扫摆）
// ---------------------------------------------------------------------------

/// I2C 总线解卡（**欠压复位后的自愈**）：ESP32 在 SSD1306 事务中途被 BOD 复位时，
/// 从机常保持 SDA 低电平（半个字节未收完），此后主机每次 START 都撞"总线忙"→
/// 全部事务 NACK/超时（真板实录：复位后 boot 扫描恒 `[]`、init 5 连败）。IC 上电
/// 前先用 GPIO 手动发 9 个 SCL 时钟把从机的位计数器走完、再补一个 STOP，SDA 即
/// 释放。裸寄存器操作（不进 HAL 驱动状态机），随后 I2cDriver::new 会重配这两脚。
unsafe fn i2c_bus_unstick() {
    use esp_idf_sys as sys;
    let sda = 21i32;
    let scl = 22i32;
    // SDA 输入上拉（读电平）、SCL 开漏输出上拉
    sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT);
    sys::gpio_set_pull_mode(sda, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
    sys::gpio_set_direction(scl, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
    sys::gpio_set_pull_mode(scl, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
    sys::gpio_set_level(scl, 1);
    sys::esp_rom_delay_us(5);
    if sys::gpio_get_level(sda) == 0 {
        // SDA 被从机拉低 = 卡死：走 9 个时钟
        for _ in 0..9 {
            sys::gpio_set_level(scl, 0);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_level(scl, 1);
            sys::esp_rom_delay_us(5);
        }
        // 补一个 STOP：SCL 高时 SDA 由低→高
        sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
        sys::gpio_set_level(sda, 0);
        sys::esp_rom_delay_us(5);
        sys::gpio_set_level(scl, 1);
        sys::esp_rom_delay_us(5);
        sys::gpio_set_level(sda, 1);
        sys::esp_rom_delay_us(5);
        sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT);
        println!("[periph] I2C 总线解卡：SDA 曾被拉低，已发 9 时钟 + STOP");
    }
}

/// 运行面主循环：有作业即取之执行（硬件慢动作全在此），无作业走自主节拍（10ms
/// 一拍）。**慢 I2C / 舵机整定都停在本线程**——控制面命令吞吐不受其阻塞（这是
/// "控制面/运行面分离"的落点，消"卡顿"）。
fn runtime_loop(mut ph: RtPeriph, bus: Arc<Mutex<Bus>>) {
    println!("[periph] 运行面线程起（独占 I2C0 + LEDC；表情/舵机/扫描全在此面）");
    loop {
        let job = lock_bus(&bus).jobs.pop_front();
        match job {
            Some(job) => exec_job(&mut ph, &bus, job),
            None => {
                autonomous_tick(&mut ph, &bus);
                thread::sleep(Duration::from_millis(RUNTIME_TICK_MS));
            }
        }
        // 供电分时仲裁收尾：每次动作（作业/自主节拍）结束即释放舵机，把 5V 轨整段
        // 让给 OLED——见 [`RtPeriph::servo_release`]。这一步是"OLED 表情稳定"的根因
        // 修复点：舵机只在"正在移动/扫描"的窗口里激磁，其余时间不占轨。
        ph.servo_release();
        // 紧接着**强制重同步** I2C 从机：舵机电机换向噪声会在空闲期把 SSD1306 的
        // I2C 状态机顶进半途态（此后恒 NACK 且 SDA 仍高，条件版自愈不触发）——
        // 见 [`i2c_bus_recover`] 的 `force` 说明。每拍一次 9 时钟 + STOP，代价 ~100µs。
        i2c_bus_recover(true);
    }
}

/// 执行一件硬件作业（脱锁慢跑阶段——只在需要读状态时短暂持锁取快照）。
fn exec_job(ph: &mut RtPeriph, bus: &Arc<Mutex<Bus>>, job: Job) {
    match job {
        Job::RedrawOled => {
            let (text, pan, tilt, visible) = {
                let b = lock_bus(bus);
                (
                    b.st.last_text.clone(),
                    b.st.pan_deg,
                    b.st.tilt_deg,
                    b.st.angle_row_visible(),
                )
            };
            if visible {
                if let Err(e) = ph.oled_show(&text, pan, tilt) {
                    eprintln!("[periph] 角度行刷新失败: {e}");
                }
            }
        }
        Job::Move { pan, tilt } => {
            if rt_move(ph, pan, tilt) {
                // 姿态到位回读（**硬件侧证据**：直接读 LEDC 寄存器换算，不听记账角；
                // 只回读本次请求的轴——未动的轴 duty 可能为 0，换算无意义）。
                let rd = |axis: Axis| duty_to_deg(match axis {
                    Axis::Pan => ph.pan.get_duty(),
                    Axis::Tilt => ph.tilt.get_duty(),
                });
                let want = |v: Option<u32>| v.map_or_else(|| "-".into(), |d| d.to_string());
                println!(
                    "[periph] 姿态到位 pan={} tilt={}（LEDC 回读 pan={}° tilt={}°，记账 {}°/{}°）",
                    want(pan),
                    want(tilt),
                    rd(Axis::Pan),
                    rd(Axis::Tilt),
                    ph.pan_deg,
                    ph.tilt_deg
                );
            }
        }
        Job::Scan => rt_scan(ph, bus),
        Job::RenderCapsules => {
            let (open, dx, dy) = {
                let b = lock_bus(bus);
                (b.st.base_open, b.st.base_dx, b.st.base_dy)
            };
            if let Err(e) = ph.capsules_degrading(open, dx, dy) {
                eprintln!("[periph] 胶囊眼渲染失败: {e}");
            }
        }
        Job::RenderFrame(frame) => {
            if let Err(e) = oled_frame(&mut ph.i2c, &frame) {
                eprintln!("[periph] 点阵渲染失败: {e}");
            }
        }
    }
}

/// 舵机移动（**串行降级核心**）：单轴直动；双轴**先 pan 到位 → 整定
/// [`AXIS_SETTLE_MS`] → 再 tilt**——任一时刻只驱动一轴，把两轴并行的峰值电流摊开
/// （适配板载 5V 直供、无 UBEC 场景）。pan 失败则不动 tilt（fail-closed，绝不留
/// 两轴同时拉电）。
fn rt_move(ph: &mut RtPeriph, pan: Option<u32>, tilt: Option<u32>) -> bool {
    if let Some(p) = pan {
        if let Err(e) = ph.servo_set(Axis::Pan, p) {
            eprintln!("[periph] pan 移动失败: {e}");
            return false;
        }
    }
    if let Some(t) = tilt {
        if pan.is_some() {
            thread::sleep(Duration::from_millis(AXIS_SETTLE_MS)); // 先左右到位再俯仰
        }
        if let Err(e) = ph.servo_set(Axis::Tilt, t) {
            eprintln!("[periph] tilt 移动失败: {e}");
            return false;
        }
    }
    true
}

/// SCAN 判据②（运行面执行）：两轴各按本轴行程五等分点**串行**扫描——每点先 pan
/// 到位、驻留，再 tilt 到位、驻留（任一时刻只驱动一轴），逐点 LEDC 寄存器回读
/// 换算并 outbox 回包。旧版两轴并行 + 写死 0/45/135/180 越程，正是堵转来源。
fn rt_scan(ph: &mut RtPeriph, bus: &Arc<Mutex<Bus>>) {
    let pan_pts = axis_scan_points(Axis::Pan);
    let tilt_pts = axis_scan_points(Axis::Tilt);
    let mut ok = true;
    for (i, (dp, dt)) in pan_pts.iter().zip(tilt_pts.iter()).enumerate() {
        let p = match ph.servo_set(Axis::Pan, *dp) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[periph] SCAN pan 失败: {e}");
                ok = false;
                break;
            }
        };
        thread::sleep(Duration::from_millis(SCAN_POINT_MS));
        let t = match ph.servo_set(Axis::Tilt, *dt) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[periph] SCAN tilt 失败: {e}");
                ok = false;
                break;
            }
        };
        thread::sleep(Duration::from_millis(SCAN_POINT_MS));
        // 回读闭环：LEDC 寄存器实际 duty 换算回角度（duty_to_deg），与设定角并打
        // ——判据②"目视 + 串口回读一致"的硬件侧证据
        let (pr, tr) = (duty_to_deg(ph.pan.get_duty()), duty_to_deg(ph.tilt.get_duty()));
        {
            let mut b = lock_bus(bus);
            b.st.wander_on = false;
            b.st.pan_deg = p;
            b.st.tilt_deg = t;
        }
        println!(
            "[periph] SCAN point={} pan={}(read {pr}) tilt={}(read {tr})",
            i + 1,
            p,
            t
        );
        outbox_push(
            bus,
            format!("SCAN point={} pan={p} read={pr} tilt={t} read={tr}", i + 1),
        );
    }
    outbox_push(
        bus,
        if ok {
            "OK scan done".into()
        } else {
            "ERR SCAN 中途失败（见 monitor 日志）".into()
        },
    );
}

/// 运行面自主节拍（无作业时 10ms 一拍）：**决策段持锁纯算，渲染/驱动段脱锁慢跑**
/// ——慢 I2C 不阻塞控制面命令吞吐。
fn autonomous_tick(ph: &mut RtPeriph, bus: &Arc<Mutex<Bus>>) {
    let (render, mv, refresh) = {
        let mut b = lock_bus(bus);
        let st = &mut b.st;
        let render = st.anim_decide();
        let (mv, refresh) = st.wander_decide();
        (render, mv, refresh)
    };
    if let Some((open, dx, dy)) = render {
        if let Err(e) = ph.capsules_degrading(open, dx, dy) {
            eprintln!("[periph] 表情微动作渲染失败: {e}");
        }
    }
    if let Some((axis, deg)) = mv {
        if let Err(e) = ph.servo_set(axis, deg) {
            eprintln!("[periph] 扫摆驱动失败: {e}");
        }
    }
    if refresh {
        // 刷前重读（含本拍刚走的角度）；帧/表情模式由 angle_row_visible 守卫跳过
        let (text, pan, tilt, visible) = {
            let b = lock_bus(bus);
            (
                b.st.last_text.clone(),
                b.st.pan_deg,
                b.st.tilt_deg,
                b.st.angle_row_visible(),
            )
        };
        if visible {
            if let Err(e) = ph.oled_show(&text, pan, tilt) {
                eprintln!("[periph] 角度行刷新失败: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 指令循环（批B 行面 + 批C 帧面双面并存：行面保留调试与舵机五点 SCAN 判据）
// ---------------------------------------------------------------------------

/// 帧面一轮（控制面双面分流的 0x31 分支）：收帧 + 协议门（失败丢弃不回）→ 分发
/// （校验/改状态/推作业）→ Response 回帧。UartIo 只在收/发两段活着；执行段无硬件
/// 借用（作业已推给运行面），控制面保持轻。
fn frame_round(io: &mut CtlUart, bus: &Arc<Mutex<Bus>>, ctl: &mut CtlState, first: u8) {
    let req = {
        let mut uio = UartIo {
            uart: &mut io.uart,
            pre: Some(first),
        };
        match frame_recv(&mut uio) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[periph] 帧面丢弃: {e}");
                return;
            }
        }
    };
    println!("[periph] 帧面 id={} topic=\"{}\"", req.id, req.topic);
    let result = frame_dispatch(bus, ctl, &req);
    if let Err(e) = &result {
        eprintln!("[periph] 帧面业务失败 id={}: {e}", req.id);
    }
    let mut uio = UartIo {
        uart: &mut io.uart,
        pre: None,
    };
    frame_send_response(&mut uio, &req, result);
}

/// 控制面主循环（**独占 UART2，不碰 I2C/LEDC**）：逐字节窥视首字节——0x31（帧
/// magic LE 首字节）走帧面 round，其余作行首字节继续收行走批B 行面。行面的硬件
/// 动作一律折成作业推给运行面；本循环只做解析/校验/改状态/回包，故慢外设不阻塞
/// 命令吞吐。未知指令 ERR（fail-closed，不猜不拖）。
fn cmd_loop(mut io: CtlUart, bus: Arc<Mutex<Bus>>) {
    let mut ctl = CtlState {
        prev_expr: String::from("none"),
    };
    reply(
        &mut io.uart,
        "READY periph-base v0.3.3 ctrl/runtime split + serial-servo (type HELP for commands)",
    );
    loop {
        // 先清 outbox（运行面异步回包：SCAN 逐点等），再窥视下一字节
        drain_outbox(&mut io.uart, &bus);
        let mut first = [0u8; 1];
        // 窥视带 1-tick（10ms）超时：空口返回 Ok(0) 即回到轮首（转手清 outbox +
        // 让出 CPU，自主行为在运行面跑，不再占用控制面）；有字节（Ok(n≥1)）照旧
        // 双面分流（帧/行路径的后续读仍 portMAX_DELAY 阻塞）。Err 才是真读失败。
        match io.uart.read(&mut first, 1) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(_) => {
                eprintln!("[periph] UART 读失败，退避后重试");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
        if first[0] == RR3_MAGIC_FIRST {
            frame_round(&mut io, &bus, &mut ctl, first[0]);
            continue;
        }
        let line = match read_line(&mut io.uart, first[0]) {
            Ok(l) => l,
            Err(why) => {
                eprintln!("[periph] 收行失败: {why}");
                continue;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let head = parts.next().unwrap_or("");
        let result: Result<String, String> = match head {
            "HELP" => Ok(
                "CMDS: OLED <text> | FACE <idle|blink|talk|think|happy|listen|turn> | PAN <deg> | TILT <deg> | SERVO <pan> <tilt> | SCAN | WANDER [STOP] | INFO | HELP  (行程 pan 60-120 / tilt 80-100，中位 90；SERVO/SCAN 串行先左右后俯仰；开机默认表情 idle + 姿态 P060 T098)"
                    .into(),
            ),
            "OLED" => {
                // 参数取原始行去指令头，保留内嵌空格（≤63 字符 = 3 行 × 21 列，row3 角度行）
                let rest = line[head.len()..].trim_start();
                if rest.is_empty() {
                    Err("OLED 缺文本参数".into())
                } else if rest.len() > OLED_COLS * (OLED_ROWS - 1) {
                    Err(format!(
                        "OLED 文本超长（≤{} 字符 = 3 行 × 21 列，第 4 行为角度行）",
                        OLED_COLS * (OLED_ROWS - 1)
                    ))
                } else {
                    // 控制面记账 + 排队，运行面出字（慢 I2C 不在控制面）
                    {
                        let mut b = lock_bus(&bus);
                        b.st.last_text = rest.to_string();
                        b.st.frame_mode = false;
                        b.st.expr_mode = false;
                    }
                    push_job(&bus, Job::RedrawOled);
                    Ok(format!("OK oled \"{rest}\""))
                }
            }
            "PAN" => parse_deg(parts.next()).and_then(|deg| {
                let d = axis_check(Axis::Pan, deg)?;
                {
                    let mut b = lock_bus(&bus);
                    b.st.wander_on = false; // 外部点名优先，自主扫摆让位
                    b.st.pan_deg = d;
                }
                push_job(
                    &bus,
                    Job::Move {
                        pan: Some(d),
                        tilt: None,
                    },
                );
                push_job(&bus, Job::RedrawOled);
                Ok(format!("OK pan={d}"))
            }),
            "TILT" => parse_deg(parts.next()).and_then(|deg| {
                let d = axis_check(Axis::Tilt, deg)?;
                {
                    let mut b = lock_bus(&bus);
                    b.st.wander_on = false;
                    b.st.tilt_deg = d;
                }
                push_job(
                    &bus,
                    Job::Move {
                        pan: None,
                        tilt: Some(d),
                    },
                );
                push_job(&bus, Job::RedrawOled);
                Ok(format!("OK tilt={d}"))
            }),
            "SERVO" => match (parse_deg(parts.next()), parse_deg(parts.next())) {
                (Ok(p), Ok(t)) => match (axis_check(Axis::Pan, p), axis_check(Axis::Tilt, t)) {
                    (Ok(p), Ok(t)) => {
                        {
                            let mut b = lock_bus(&bus);
                            b.st.wander_on = false;
                            b.st.pan_deg = p;
                            b.st.tilt_deg = t;
                        }
                        // 串行降级：运行面先 pan 到位 → 整定 → 再 tilt（削峰值电流）
                        push_job(
                            &bus,
                            Job::Move {
                                pan: Some(p),
                                tilt: Some(t),
                            },
                        );
                        push_job(&bus, Job::RedrawOled);
                        Ok(format!("OK pan={p} tilt={t}"))
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
                _ => Err("SERVO 需两个 0-180 角度参数".into()),
            },
            // 表情（行面）：控制面改状态 + 推 Job::RenderCapsules（慢 I2C 在运行面），
            // 与帧面 expression 同一套基态预设/微动作（expr_eye_base + anim_reset）。
            // 补此命令的原因：表情原只走帧面（需 UART2 帧链路），现场该链路劣化时
            // 表情无路可触发——行面直命令是可达性兜底。
            "FACE" => match parts.next() {
                Some(expr) if RR3_FACE_EXPRS.contains(&expr) => {
                    let (open, dx, dy) = expr_eye_base(expr);
                    {
                        let mut b = lock_bus(&bus);
                        b.st.expr_mode = true;
                        b.st.frame_mode = false;
                        b.st.base_open = open;
                        b.st.base_dx = dx;
                        b.st.base_dy = dy;
                        b.st.last_text = format!("EXPR {expr}");
                        b.st.anim_reset();
                        b.st.drawn = (open, dx, dy);
                    }
                    push_job(&bus, Job::RenderCapsules);
                    Ok(format!("OK face={expr}"))
                }
                Some(other) => Err(format!(
                    "expression 不在白名单: \"{other}\"（可选: {}）",
                    RR3_FACE_EXPRS.join("/")
                )),
                None => Err(format!(
                    "FACE 需表情参数（可选: {}）",
                    RR3_FACE_EXPRS.join("/")
                )),
            },
            "WANDER" => match parts.next() {
                Some("STOP") => {
                    lock_bus(&bus).st.wander_on = false;
                    let (p, t) = {
                        let b = lock_bus(&bus);
                        (b.st.pan_deg, b.st.tilt_deg)
                    };
                    Ok(format!("OK wander off pan={p} tilt={t}"))
                }
                None => {
                    {
                        let mut b = lock_bus(&bus);
                        b.st.wander_on = true;
                        // 两轴方向独立随机起步，立即起摆
                        b.st.wander_pan_dir = if esp_random_u32() % 2 == 0 { 1 } else { -1 };
                        b.st.wander_tilt_dir = if esp_random_u32() % 2 == 0 { 1 } else { -1 };
                        b.st.next_wander_ms = 0;
                    }
                    let (p, t) = {
                        let b = lock_bus(&bus);
                        (b.st.pan_deg, b.st.tilt_deg)
                    };
                    Ok(format!("OK wander on pan={p} tilt={t}"))
                }
                Some(other) => Err(format!("WANDER 参数非法: {other}（无参=开，STOP=停）")),
            },
            "SCAN" => {
                // 判据②（**串行**）：运行面按本轴行程五等分点逐点扫描（pan
                // 70/80/90/100/110、tilt 80/85/90/95/100），每点先 pan 到位再 tilt
                // 到位，逐点 outbox 回包。控制面只置位 + 排队并即刻 ack——不被 ~6s
                // 的扫描阻塞（旧版在命令线程里跑，是"卡顿"来源之一）。
                lock_bus(&bus).st.wander_on = false; // 判据测试不被自主扫摆扰动
                push_job(&bus, Job::Scan);
                Ok("OK scan queued (串行 pan→tilt，逐点回包)".into())
            }
            "INFO" => {
                let (plo, phi) = Axis::Pan.range();
                let (tlo, thi) = Axis::Tilt.range();
                let (pan, tilt, wander, phase, last_text) = {
                    let b = lock_bus(&bus);
                    (
                        b.st.pan_deg,
                        b.st.tilt_deg,
                        b.st.wander_on,
                        b.st.wander_phase,
                        b.st.last_text.clone(),
                    )
                };
                Ok(format!(
                    "INFO pan={pan} tilt={tilt} lim=pan[{plo},{phi}]/tilt[{tlo},{thi}] wander={} phase={phase} oled_last=\"{last_text}\" free={}B min_free={}B",
                    if wander { "on" } else { "off" },
                    unsafe { esp_get_free_heap_size() },
                    unsafe { esp_get_minimum_free_heap_size() }
                ))
            }
            "FONT" => {
                // 字模自检护栏：打印 "ABC123" 的 5x7 字模 hex，供人工比对
                let hex: Vec<String> = "ABC123"
                    .bytes()
                    .map(|c| {
                        glyph(c)
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<String>()
                    })
                    .collect();
                Ok(format!("FONT ABC123 = {}", hex.join(",")))
            }
            other => Err(format!("unknown cmd \"{other}\"（HELP 看指令表）")),
        };
        match result {
            Ok(line) => reply(&mut io.uart, &line),
            Err(e) => reply(&mut io.uart, &format!("ERR {e}")),
        }
    }
}

/// 角度参数解析（0–180 之外一律 Err——调用方不再 clamp，fail-closed）。
fn parse_deg(s: Option<&str>) -> Result<u32, String> {
    let Some(s) = s else {
        return Err("缺角度参数".into());
    };
    let deg: u32 = s.parse().map_err(|_| format!("角度参数非法: \"{s}\""))?;
    if deg > 180 {
        return Err(format!("角度超范围 0-180: {deg}"));
    }
    Ok(deg)
}

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
        "[periph] surface=esp-rs-std crate=esp32-periph-base oled=i2c0@21/22 servo=ledc@32/33 cmd=uart2@115200 line+frame planes=ctrl+runtime serial-servo"
    );
    ram_point("1-boot");

    let peripherals = Peripherals::take().unwrap();

    // ---- UART2 指令面 @ GPIO18(TX)/GPIO19(RX) 115200 ----
    // **引脚定案（真板实锤 2026-10-12）**：GPIO16/17（UART2 的 IOMUX 缺省组）在本板
    // 上**装驱动即把 I2C0（21/22）打死**——先装/后装、换序均复现全黑；换到 18/19 后
    // I2C 全绿（扫描 0x3C / init / boot 文本 / 页写全过）。故本板弃用 16/17（疑与
    // 底板走线桥接或模块 PSRAM 脚冲突），钉 18/19。接线表见 README；19 与点阵 VSPI
    // 预留同脚，点阵立项时需再迁（已在案）。
    let uart_config = UartConfig::new().baudrate(Hertz(115_200));
    let uart = UartDriver::new(
        peripherals.uart2,
        peripherals.pins.gpio18,
        peripherals.pins.gpio19,
        None::<AnyInputPin>,
        None::<AnyOutputPin>,
        &uart_config,
    )
    .expect("UART2 驱动安装失败");
    println!("[periph] UART2 ✓ 指令面 115200 @ GPIO18(TX)/GPIO19(RX)");

    // ---- I2C0 @ GPIO21/22（判据①：扫描 0x3C 应答）----
    // 频率档 400k → 100k → **50k**：真板实证 400k/100k 在长跳线 + 模块弱上拉下**时序
    // 边际**——背靠背密集页写（8 页胶囊眼）在数据段丢拍，且丢拍一次即锁死总线
    // （机理见 [`i2c_bus_recover`]）。50k 上升裕量翻倍 + 失败自愈 + 整页重写三者叠加。
    let i2c_config = I2cConfig::new()
        .baudrate(Hertz(I2C_BAUD_HZ))
        .sda_enable_pullup(true)
        .scl_enable_pullup(true);
    // 欠压复位后 SSD1306 常抱死 SDA→总线全黑；装驱动前先解卡（见 i2c_bus_unstick）。
    unsafe { i2c_bus_unstick() };
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
        let boot_rows: [(&[u8], &str); 4] = [
            (b"ROCK3 PERIPH BASE", "boot 行0写失败"),
            (b"4-ROW OLED READY", "boot 行1写失败"),
            (b"UART2 CMD @115200", "boot 行2写失败"),
            (b"P:060 T:098", "boot 角度行写失败"),
        ];
        let mut boot_ok = true;
        for (row, (text, msg)) in boot_rows.iter().enumerate() {
            if let Err(e) = oled_text(&mut i2c, row, text) {
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

    // ---- LEDC timer0 50Hz 14bit：channel0→GPIO32 pan / channel1→GPIO33 tilt ----
    let timer_config = TimerConfig::new()
        .frequency(Hertz(50))
        .resolution(Resolution::Bits14);
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
        pan_deg: AXIS_CENTER_DEG,
        tilt_deg: AXIS_CENTER_DEG,
        contrast_idx: 0,
    };
    let ctl = CtlUart { uart };
    let bus = Arc::new(Mutex::new(Bus {
        jobs: VecDeque::new(),
        outbox: VecDeque::new(),
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
