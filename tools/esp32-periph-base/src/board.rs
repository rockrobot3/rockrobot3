use rr_ipc::Rr3BoardBindings;

use esp_idf_hal::adc::{attenuation, Resolution as AdcResolution};
use esp_idf_hal::ledc::Resolution;
use esp_idf_sys::TickType_t;

// 行程生成物中位锚（boot 姿态策略的基准）——生成物自 servo 机制件引入
// （批N23 件一收编：机制面收拢 atoms/servo/mech.rs，策略→机制单向依赖）。
use crate::servo::{RR3_AXIS_PAN, RR3_AXIS_TILT};

/// I2C 事务超时（TickType_t；portMAX_DELAY 同义 u32::MAX——阻塞至传输完成）。
pub(crate) const I2C_TIMEOUT: TickType_t = u32::MAX;
/// I2C 写重试次数（**有限电源优化**：板载 5V 直供下舵机转动/保持的电流尖峰拉塌轨压、
/// 或总线噪声，会让 SSD1306 事务瞬态失败 ESP_FAIL/NACK——真板实录"角度行刷新失败:
/// I2C 命令写失败 ESP_FAIL"即此）。**每笔失败先走 [`i2c_bus_recover`] 总线自愈**
/// （见其注：丢拍会把从机锁进"等时钟"态，此后重发恒不复——这是真板 8 页胶囊眼
/// 恒败的真因），故 3 次已足（恢复+重发≈必过），无需长窗死磕）。
pub(crate) const I2C_RETRY: u32 = 3;
/// I2C 重试间隔（给轨压/总线一点恢复时间，勿紧逼重发再撞失败）。
pub(crate) const I2C_RETRY_DELAY_MS: u64 = 5;
/// I2C 总线频率（SSD1306 标准档）。**真板实锤：必须 100k，不能降到 50k**——50k 档
/// 下连 init 的清屏页写（33B 数据笔）都恒 `ESP_FAIL`（命令笔 ≤4B 恒过），比 100k 更
/// 差；老 I2C 驱动的低速档时序是反直觉的坏档。100k 是 SSD1306 标准档、真板实测
/// init/boot 文本恒过（唯有背靠背 8 页密集写会丢拍），靠 [`i2c_bus_recover`] 自愈兜底。
pub(crate) const I2C_BAUD_HZ: u32 = 100_000;
/// 分块间短延时：给模块电荷泵与总线电平留一拍（边际失败集中在背靠背事务）。
pub(crate) const I2C_CHUNK_GAP_MS: u64 = 1;
/// 单页整页重试上限：失败 → 总线恢复 → **从页首重写整页**（一笔数据丢拍后从机的
/// 列指针已错位，只重发该块会写歪；整页重发把指针拉回确定性起点）。外层还有
/// [`RtPeriph::capsules_degrading`] 的降亮重来，故本层不必死磕。
pub(crate) const I2C_PAGE_ATTEMPTS: u32 = 2;
/// OLED 亮度阶梯（**整板电压降级**，本轮真板实锤的主修复）：SSD1306 满亮（0xCF）下
/// 大面亮点会把模块升压电路电流顶高、抬高地弹/轨压噪声，反噬 I2C 通信——真板实录
/// "空白页 8 页恒过、boot 文本小面积恒过、8 页胶囊眼在**页 3（大面亮点开始上屏）**
/// 起恒 `ESP_FAIL`，且此后整屏恒败";降到 0x2F 后胶囊眼 8 页全过、微动作连跑零失败。
/// 初始取中档，I2C 反复失败即逐级下调保通信——"极限电源下宁暗不瞎"。
pub(crate) const OLED_CONTRAST_LEVELS: [u8; 4] = [0x4F, 0x3F, 0x2F, 0x1F];
/// 单次渲染的**恢复环轮数上限**（机制护栏；策略量集中在本文件）。本值须
/// `>= OLED_CONTRAST_LEVELS.len()`：前若干轮逐级降亮，落到亮度地板那轮触发
/// **一次**模块重初始化 + 一次再试即收手（见 [`RtPeriph::render_with_recovery`]）。
/// 真板实锤（2026-10-13）：大面亮点帧丢拍会把 SSD1306 latch 成"连 2 字节命令都恒
/// NACK"，降亮与总线自愈都救不回；但旧版**隐式计数**会在一拍内触发 2 次重初始化、
/// 30s 内 56 次重初始化风暴——既救不回 latch 又白烧 I2C 与轨压。故显式收敛为
/// "每轮至多一次"。**注**：本恢复环只治"拍丢/latch"这一子症状，不治根因
/// （大面亮点帧拉塌轨压 → ESP32 BOD 复位），根因归"电源/负载均衡"机制。
pub(crate) const OLED_RENDER_ATTEMPTS: u32 = 4;
/// SSD1306 I2C 从机地址（0.96" 128x64 模块缺省）。
pub(crate) const OLED_ADDR: u8 = 0x3C;
/// OLED 每行字符数：5px 字模 + 1px 间隔 = 6px/字符，128px/6 = 21.3 → 21 列。
pub(crate) const OLED_COLS: usize = 21;
/// OLED 文本行数：4 行 = 文本 3 行（页 0/2/4，`OLED` 命令按 21 列硬切）+ 固定
/// 角度行（页 6，P/T 随舵机命令刷新）——Mac 实时数据面（温度/内存/IP/时间）
/// 同屏三行布局，批B 增量批裁定（2→4 扩行，`OLED <text>` 形状不变向后兼容）。
pub(crate) const OLED_ROWS: usize = 4;
/// 指令行缓冲上限（超出整行丢弃回 ERR，fail-closed 不截半执行）。
pub(crate) const LINE_CAP: usize = 128;
/// 指令/心跳类线程的栈面（板上实测实锤 16KB 运行下限，哨兵同款，勿再调低）。
pub(crate) const THREAD_STACK: usize = 16384;
/// 轴间整定（**串行降级**：先 pan 到位 → 等这么久 → 再 tilt）。SG90 空载约
/// 0.1s/60°，本轴最大行程 40° 约 67ms，200ms 留裕量（含负载迟滞与死区）。
pub(crate) const AXIS_SETTLE_MS: u64 = 200;
/// 舵机缓速步进（**极限电源下的软启动**）：每次只挪 [`SERVO_SLEW_STEP_DEG`] 度、
/// 每步歇 [`SERVO_SLEW_STEP_MS`]——把"整段跳变"的启动/换向电流尖峰摊成涓流，
/// 避免板载 5V 直供被单次大电流拉塌（真板 BOD 复位实录）。2°/20ms = 100°/s，
/// pan 满行程 60° 约 600ms、tilt 20° 约 200ms，慢于 SG90 空载能力但最省轨压。
pub(crate) const SERVO_SLEW_STEP_DEG: u32 = 2;
pub(crate) const SERVO_SLEW_STEP_MS: u64 = 20;
/// SCAN 每轴每点驻留（判据② 逐点回读；串行后单点 = pan 段 + tilt 段各一次）。
pub(crate) const SCAN_POINT_MS: u64 = 600;
/// 运行面自主节拍（表情微动作 + 云台扫摆；无作业时空转一拍）。
pub(crate) const RUNTIME_TICK_MS: u64 = 10;

// 帧面协议真值（ADR-0024）：task 位形/magic 单点在 rr-periph-proto（与宿主
// rr-hal 双向复用），本固件只 re-export 保既有 board::* 路径。
pub(crate) use rr_periph_proto::consts::{
    RR3_MAGIC_FIRST, RR3_PERIPH_TASK, RR3_TASK_HOST as RR3_HOST_TASK,
};

// 板侧常量表生成物（主题串三枚 + 板侧表情白名单可渲染子集）——真值点（contracts/
// topics.toml + contracts/face-expressions.toml）与生成器（tools/rr3-codegen）
// 见 topics_gen.rs 头注。本文件不再手写副本：全案 0.0.5 增补轮六 §4.5 裁定
// "真值点 → 生成副本"，重跑生成器 diff 为空 = 门 f 拟 ⑨-d（check.sh 接线）。
//（include! 前的 doc 注释挂不到宏产物上，用普通注释。）
include!("topics_gen.rs");
// 行程生成物（RR3_AXIS_* + rr3_axis_travel）已随批N23 件一收编迁入 servo 机制件
// （atoms/servo/mech.rs include!）——行程是舵机机制真值，中位锚经上方 use 回流
// 供 boot 姿态策略引用。
/// 生成物 ⇄ 协议 crate 同值断言（编译期）：契约改主题串而未同步 rr-periph-proto
/// 常量即编译红——两处副本不许静默分叉（⑨-d 只管生成物⇄契约，本断言补另一条边）。
const fn rr3_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}
const _: () = assert!(rr3_str_eq(
    RR3_SERVO_TOPIC,
    rr_periph_proto::consts::RR3_SERVO_TOPIC
));
const _: () = assert!(rr3_str_eq(
    RR3_FACE_TOPIC,
    rr_periph_proto::consts::RR3_FACE_TOPIC
));
/// 音频上行分片（每通道帧数）：2048 帧 = 128ms @16kHz = 8192B PCM/分片——b64+
/// JSON+帧头开销摊薄后单分片上行 ≈11.1KB，7.8 分片/s ≈ 87KB/s，落 921600 有效
/// 92.16KB/s 内（Q1=(a) 带宽账）。分片过短开销占比暴涨、过长背压延迟变大。
pub(crate) const AUDIO_CHUNK_FRAMES: usize = 2048;
/// 板侧 audio_q 深度（有界背压）：mic 线产满即丢**最旧**分片并计
/// `audio_dropped`（判据② 背压观测前哨，与 recorder 端 seq 断档互为对账）。
/// 4 分片 ≈ 45KB 堆 + 单分片传输 ~120ms，吸收宿主 ~0.5s 级收流抖动。
pub(crate) const AUDIO_Q_CAP: usize = 4;
/// 语音档采样率（判据① wav 口径；全案批D §4）——采集档位策略量（I2S 配置与
/// 上行 JSON `rate`/`ms` 字段同源；换采集档只动本文件，变换机制在 capture
/// 机制件按参吃值）。
pub(crate) const MIC_SR_HZ: u32 = 16000;
/// 板麦数字增益 ×8（+18dB，真板冒烟(一) 2026-10-18 标定）：半米说话 RMS 实测
/// ~163 / 环境底噪 ~73，信噪余量 1.36 倍太薄（宿主 VAD rms_start 千位级口径
/// 复用不了）——×8 后 ~1300/584，余量拉到舒适区，同时服务 DoA 判向质量
/// （B 铺垫；DoA 相关计算走 24bit 原始域，线性增益不影响相关峰位置，仅作用
/// 本上行面）。近讲大音量饱和钳位（不翻卷——翻卷产生假静音假爆音，比削顶伤）。
/// **策略量**：标定值随板麦个体/装配再标定，钳位机制在 capture 机制件按参吃值。
pub(crate) const MIC_GAIN: i64 = 8;
/// UART2 发送环形缓冲字节数（tx_fifo_size，策略量集中本文件）。**真板实锤
/// （2026-10-13 D③ 验收）**：缺省 TX 环仅 256B，整帧写 11.4KB 只能同步等线速
/// 排空（921600 下 ~120ms/帧），mic 组帧线程被写调用拖住 → I2S DMA 环溢出丢数
/// （[mic] 采样率 16.5kHz → 4.5kHz 崩塌实录）。16KB ≥ 单分片整帧 ≈11.4KB：
/// write 降为 memcpy 入环、ISR/驱动任务异步排水，组帧线程不被线速拖住。
pub(crate) const UART2_TX_RING: usize = 16384;

// ---------------------------------------------------------------------------
// 舵机换算（50Hz / 14bit：1 tick = 20ms / 16384 = 1.2207µs）——**策略量**：
// SG90 脉宽锚 × LEDC 档位换算的换算锚，换舵机/换 LEDC 档只动本文件；换算
// 机制（线性映射与回读近似）在 servo 机制件（atoms/servo/mech.rs，按参吃值）。
// ---------------------------------------------------------------------------

/// 0° 对应 duty ticks（0.5ms = 500µs / 1.2207µs ≈ 409.6 → 410）。
pub(crate) const DUTY_MIN: u32 = 410;
/// 180° 对应 duty ticks（2.5ms = 2048）。
pub(crate) const DUTY_MAX: u32 = 2048;

// ---------------------------------------------------------------------------
// 分轴行程（批N3 件五门 g 收敛：真值 = 轴档案；批N23 件一收编：机制面迁
// atoms/servo/mech.rs，本文件只留 boot 姿态策略）
// ---------------------------------------------------------------------------
//
// 行程三元组（lo, hi, center）与其消费（Axis::range/center_deg、axis_check、
// axis_scan_points、wander_step）已迁 servo 机制件（行程唯一真值点 =
// hardware/esp32-mini/axis/*.toml，生成物 axis_gen.rs 随迁）。
//
// 真板实锤（批C v0.3.2 与现场排障）：云台机械行程远小于 SG90 全程 0–180°——
// 旧版 WANDER 全行程扫摆 / SCAN 五点（0/45/90/135/180）会把舵机推到机械死点
// 并**持续施力**，表现为"卡住不动 + 舵机发烫"（堵转电流 ~700mA/只持续流动），
// 叠加板载 USB 5V 直供时触发欠压复位与 USB 链路反复掉线。故本轴行程以中位为
// 基准收窄（pan [60,120] / tilt [80,100]，2026-10-12 现场裁定），越界**拒收点名**
// （fail-closed：静默钳到边界会让调用方误以为按给值生效），WANDER/SCAN 的端点
// 与判据点一律收进区间内。**pan ±30°（用户 2026-10-12 现场裁定，纳"左转 30°"
// 所需）**：放宽后若机械到不了边界会持续堵转——现场须目视，一旦卡住即断电
// （见 README 现场注记）。改行程 = 改档案 + 重跑生成器（门 f 拟 ⑨-d 兜底）。

/// 开机默认姿态（用户现场裁定）：pan 左转 30°（中位 −30）、tilt 上抬 3°（中位 +3）。
/// 中位锚引用生成表 center（RR3_AXIS_*.2）——boot 姿态是"相对本轴中位"的偏移
/// 语义（复位语义②），不属行程字面量；偏移量 −30/+3 保留字面（现场姿态裁定，
/// 非行程）。改档案中位时本两值随生成表联动。
pub(crate) const BOOT_PAN_DEG: u32 = RR3_AXIS_PAN.2 - 30;
pub(crate) const BOOT_TILT_DEG: u32 = RR3_AXIS_TILT.2 + 3;

// ---------------------------------------------------------------------------
// 引脚真值（**注入面 + 装配对账锚**，批N4 件七(c) / 批N9 件五；批N35 件八按
// target 分档）——接线总表已拆到 `board/pins_classic.rs` 与 `board/pins_s3.rs`：
//   classic：UART2 18/19、I2C0 21/22、LEDC 32/33、I2S0 4/27/26、ADC 34；
//   S3 N16R8：UART2 17/18、I2C0 8/9、LEDC 4/5、I2S0 6/15/7、ADC 1。
// **两套线束不可混用**：S3 的 GPIO26–37 被 16MB Flash / 8MB Octal PSRAM 占用。
// **消费面数字引脚号不落常量**（批N4 件七(c) 定案维持）：I2C 自愈/轨测点的真值由
// 宿主经 rr3.set_bindings 下发（esp32*-mini [hardware] → rrd RR3_HAL_BINDINGS →
// supervisor env → 桥上 method），板侧落 [`crate::state`] 单点，未注入即拒不动作
// （裸寄存器自愈路径 [`crate::oled::i2c_bus_recover`] / [`crate::oled::i2c_bus_unstick`]；
// 观测面 = 绑定注入打印 + 运行面 `selfheal_skipped` 计数）。
// **装配对账锚常量**（批N9 件五，[`RR3_BIND_*`] 十枚）：esp-idf-hal 需**具名引脚
// 类型**（`peripherals.pins.gpioNN`，无法由 i32 构造）——主驱动（UART2/LEDC/I2S/
// I2C/ADC）仍具名装配，注入值作**对账真值**逐键判等本段常量（不齐 ERROR 点名＋
// 计数，不拒 boot 不打死指令面——批N4 件七(c) 消费面纪律同款）。本段常量只作
// 对账与装配打印自证引用，**不进任何消费路径**；与具名装配的一致性由装配打印
// 引用同源（`peripherals.pins.gpioNN` ↔ 常量字面），三方（装配单 [hardware] /
// 板侧常量 / 具名装配）漂移即对账实录红。双真值点（[`RR3_BIND_LEDC_PAN`] 等
// 与 [[pin]] physical）由对账面抓；rtos 档 [pins] 派生落位后（0.0.7 批G/D1）
// ledc 两键退役、具名引脚真值化改造收官。
// ---------------------------------------------------------------------------

#[cfg(feature = "esp32")]
mod pins_classic;
#[cfg(feature = "esp32s3")]
mod pins_s3;
#[cfg(feature = "esp32")]
pub(crate) use pins_classic::*;
#[cfg(feature = "esp32s3")]
pub(crate) use pins_s3::*;

/// 装配对账（批N9 件五，**纯函数**：只判等打印，不碰硬件不拒收）：注入快照逐键
/// 判等具名装配真值（[`RR3_BIND_*`] 十枚）。键缺席（未注入）与值失配都点名——
/// 部分注入合法（缺键消费点各自判齐），但对账如实记"不齐"，非零即装配单/下发链
/// 漂移（fail-visible）。返回不齐键数（0 = 十键全齐）。
pub(crate) fn rr3_bindings_reconcile(b: &Rr3BoardBindings) -> usize {
    let table: [(&str, Option<u8>, u8); 10] = [
        ("i2c_sda", b.i2c_sda, RR3_BIND_I2C_SDA),
        ("i2c_scl", b.i2c_scl, RR3_BIND_I2C_SCL),
        ("adc_rail_pin", b.adc_rail_pin, RR3_BIND_ADC_RAIL),
        ("uart2_tx", b.uart2_tx, RR3_BIND_UART2_TX),
        ("uart2_rx", b.uart2_rx, RR3_BIND_UART2_RX),
        ("ledc_pan", b.ledc_pan, RR3_BIND_LEDC_PAN),
        ("ledc_tilt", b.ledc_tilt, RR3_BIND_LEDC_TILT),
        ("i2s_bclk", b.i2s_bclk, RR3_BIND_I2S_BCLK),
        ("i2s_sd", b.i2s_sd, RR3_BIND_I2S_SD),
        ("i2s_ws", b.i2s_ws, RR3_BIND_I2S_WS),
    ];
    let mut bad = 0usize;
    for (k, got, want) in table {
        if got != Some(want) {
            eprintln!(
                "[periph] 绑定对账 ✗ {k}: 注入 {got:?} ≠ 装配 GPIO{want}（查 profiles/esp32-mini.toml [hardware] 与下发链）"
            );
            bad += 1;
        }
    }
    if bad == 0 {
        println!("[periph] 绑定对账 ✓ 十键全齐（注入 = 具名装配）");
    }
    bad
}

/// UART2 指令面波特率（批D② 落板 921600——Q1=(a) 带宽账裁定：音频上行双通道
/// 16kHz×16bit ≈ 512kbps，115200 载不动；烧录即 deliberate。宿主侧缺省仍
/// 115200，会话须以 RR3_HAL_PERIPH_BAUD=921600 或 `_with_baud` 构造显式同档）。
/// **板固件小批 2026-10-20 实测否决提波特率，回守 921600**：b64 组帧需求
/// ~90KB/s 已吃到 921600 上限的 ~97%（余量 ~7%），真板冒烟(一) resync 风暴
/// 实证余量见底——提档尝试 2M 与 1.5M **均被实板否决**（2M：字节滑移稳定乱码
/// "ERR unknown cmd"；1.5M：首包干净续包间歇乱码，双发方向同现）——真凶不是
/// 分频精度（FT232R 1.5M=n=2 精确档）而是**杜邦线信号完整性**：921600 是当前
/// 杜邦接线的物理可靠上限。提波特率**回推**：换短屏蔽线/PCB 走线后另批重评
/// （1.5M 仍为首选：双侧精确分频）。增益收益（×8 见 mic.rs）不受此影响。
/// 宿主同档 `RR3_HAL_PERIPH_BAUD=921600`。
pub(crate) const UART2_BAUD_HZ: u32 = 921_600;
/// LEDC 定时器频率（SG90 50Hz）。
pub(crate) const LEDC_FREQ_HZ: u32 = 50;
/// LEDC 分辨率（50Hz×2^14=819200Hz，源 80MHz 分频 97.66 → 实际 ≈50.0Hz）。
pub(crate) const LEDC_RES_BITS: Resolution = Resolution::Bits14;

// ---------------------------------------------------------------------------
// 5V 轨电压监测（方案A Q1/Q2）——**策略量集中处**（机制在 [`crate::power`]）
// ---------------------------------------------------------------------------
//
// 根因链（真板实锤）：OLED 大面亮点帧 + 舵机保持电流抢同一条 5V 轨 → 轨压跌落
// → ESP32 BOD 复位。此前这条轨**无任何观测手段**——欠压既看不见、也响应不了。
// 本组常量是板侧唯一策略面（引脚/分压比/采样节拍/ADC 档位/欠压阈值/接闸）；机制
// （读 ADC 一次 + 按分压比换算回轨压 mV + 双阈值迟滞判定核）在 [`crate::power`]。
// **机制与策略分离**：换板只动本文件，power.rs 零板名。分压测点接线后与万用表
// 对拍（≤2%）再回填标定——**无真板证据前不 claim**。

/// 5V 轨采样节拍（运行面自主节拍周期采样；占位值 **待真板标定**）。
pub(crate) const RAIL_SAMPLE_PERIOD_MS: u64 = 500;
/// 分压比分子：轨压 mV = 引脚 mV × NUM / DEN。**占位 1:1 待标定**——真板装上
/// 分压网络（如 5V→≤3.3V 的 2:1 分压）后按实测比值回填。
pub(crate) const ADC_RAIL_DIVIDER_NUM: u32 = 1;
/// 分压比分母（见 [`ADC_RAIL_DIVIDER_NUM`]）。
pub(crate) const ADC_RAIL_DIVIDER_DEN: u32 = 1;
/// ADC 衰减档（11dB：量程约 0–3.1V，配分压后覆盖 5V 轨）。
pub(crate) const ADC_ATTEN: attenuation::adc_atten_t = attenuation::DB_11;
/// ADC 分辨率（12bit；ESP32 支持档）。
pub(crate) const ADC_RESOLUTION: AdcResolution = AdcResolution::Resolution12Bit;
/// ADC 标定开关（true：`read()` 直接回 mV，免手算 raw→mV 曲线）。
pub(crate) const ADC_CALIBRATION: bool = true;

// ---- 双阈值迟滞 + 接闸（方案A Q2 步3）-----------------------------------------
// 档位判定是**双阈值电平分类**（[`crate::power::tier_of`]），越阈事件是**迟滞边沿**
// （[`rr_mech::control::Rr3LowEdge`] 两实例，一核两条线）。重度 < 轻度。
// **占位值随票 §8-Q5 明写"待标定"**——真板接分压测点后按实测轨压对拍回填。

/// 5V 轨**轻度**欠压阈值（mV）：跌破 → 执行面维持**串行降级**（pan→整定→tilt，
/// 削峰值电流）。**占位值待标定**（BOD 关心带 ~4.3–5.2V）。
pub(crate) const RAIL_MILD_MV: u16 = 4_600;
/// 5V 轨**重度**欠压阈值（mV）：跌破 → 执行面**拒动**（MC-5 语义：执行面拒动、
/// 妆面不禁、回阈解禁不自动重放）。须 < [`RAIL_MILD_MV`]。**占位值待标定**。
pub(crate) const RAIL_SEVERE_MV: u16 = 4_400;
/// **接闸**（标定闸）：`false` = 分压比/阈值仍是占位、**未真板标定**——档位照算照看
/// （`INFO rail_tier`）、越阈照打站记数，但**不驱动执行面拒动**。理由：GPIO34 测点
/// **非必接线**，浮空/未标定下 ADC 可能读到近 0 的**伪欠压**，若直接接闸会把正常运动
/// 误判打死（把一台能跑的板子变砖的回归 > 收益）。真板接分压测点、与万用表对拍
/// （≤2%）回填分压比与阈值后置 `true` 接闸。**无真板证据前不接闸、不 claim**。
pub(crate) const RAIL_GATE_ARMED: bool = false;
