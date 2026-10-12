use esp_idf_hal::ledc::Resolution;
use esp_idf_sys::TickType_t;

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

/// 帧面本板 task 位形（帧头 dst 门）：外设执行面占首模块池位形 2——与宿主
/// rr-hal::esp32_uart::RR3_PERIPH_TASK 同值（本 crate 不依赖 rr-core，此位形是
/// 两端帧头约定，宿主侧有同值锚定测试防静默分叉）。
pub(crate) const RR3_PERIPH_TASK: u64 = 2;
/// 帧头 magic "1ERA" 的 LE 首字节（0x31='1'）——双面分流窥视判据。行面首字节
/// 集全为字母（HELP/OLED/PAN/...），与 0x31 无交集，分流无歧义。
pub(crate) const RR3_MAGIC_FIRST: u8 = 0x31;
/// 舵机点名主题（在册 request L1；形状事实源
/// contracts/events/act-servo-command.schema.json，与 topics.toml 同串）。
pub(crate) const RR3_SERVO_TOPIC: &str = "rr3-act/servo/command";
/// 表情点名主题（在册 request L1；形状事实源
/// contracts/events/interact-face-expression.schema.json，与 topics.toml 同串）。
pub(crate) const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";
/// 表情白名单（rr3-interact 模块面同款 7 态；帧面执行面同步执行该裁定）。
pub(crate) const RR3_FACE_EXPRS: [&str; 7] = ["idle", "blink", "talk", "think", "happy", "listen", "turn"];

// ---------------------------------------------------------------------------
// 舵机换算（50Hz / 14bit：1 tick = 20ms / 16384 = 1.2207µs）
// ---------------------------------------------------------------------------

/// 0° 对应 duty ticks（0.5ms = 500µs / 1.2207µs ≈ 409.6 → 410）。
pub(crate) const DUTY_MIN: u32 = 410;
/// 180° 对应 duty ticks（2.5ms = 2048）。
pub(crate) const DUTY_MAX: u32 = 2048;

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
pub(crate) const AXIS_CENTER_DEG: u32 = 90;
/// pan（水平轴）相对中位的最大摆幅。
pub(crate) const PAN_LIMIT_DEG: u32 = 30;
/// tilt（俯仰轴）相对中位的最大摆幅。
pub(crate) const TILT_LIMIT_DEG: u32 = 10;
/// 开机默认姿态（用户现场裁定）：pan 左转 30°（中位 −30）、tilt 上抬 8°（中位 +8）。
pub(crate) const BOOT_PAN_DEG: u32 = AXIS_CENTER_DEG - 30;
pub(crate) const BOOT_TILT_DEG: u32 = AXIS_CENTER_DEG + 8;

// ---------------------------------------------------------------------------
// 引脚真值（board 层单一真值点）——接线总表：
//   UART2 = GPIO18→TX / GPIO19←RX（115200；GPIO16/17 一装 UART2 驱动即把 I2C0
//           打死，弃用；19 与点阵 VSPI 预留同脚，点阵立项时需再迁，已在案）
//   I2C0  = GPIO21 SDA / GPIO22 SCL（SSD1306 0x3C）
//   LEDC  = GPIO32 pan / GPIO33 tilt（50Hz 14bit）
//   UART0 = 控制台（板载 USB）
// 说明：esp-idf-hal 的 `LedcDriver::new`/`UartDriver::new` 需**具名引脚类型**
// （`peripherals.pins.gpioNN`），无法由 i32 常量构造——故这两类 new 仍按上表用
// `peripherals.pins.gpioNN` 传参（其名自证引脚号）；只有**裸寄存器自愈路径**
// （[`crate::oled::i2c_bus_recover`] / [`crate::oled::i2c_bus_unstick`]）需要
// 数字引脚，故单独落常量。不另立一份"驱动 new 用"的重复常量真值（防两处漂移）。
// ---------------------------------------------------------------------------

/// I2C0（SSD1306）SDA（裸寄存器自愈路径用）。
pub(crate) const PIN_I2C_SDA: i32 = 21;
/// I2C0（SSD1306）SCL（裸寄存器自愈路径用）。
pub(crate) const PIN_I2C_SCL: i32 = 22;
/// UART2 指令面波特率。
pub(crate) const UART2_BAUD_HZ: u32 = 115_200;
/// LEDC 定时器频率（SG90 50Hz）。
pub(crate) const LEDC_FREQ_HZ: u32 = 50;
/// LEDC 分辨率（50Hz×2^14=819200Hz，源 80MHz 分频 97.66 → 实际 ≈50.0Hz）。
pub(crate) const LEDC_RES_BITS: Resolution = Resolution::Bits14;

/// 角度 → duty ticks（线性 0–180 ↦ 410–2048；入参已由调用方 clamp 0–180）。
pub(crate) fn deg_to_duty(deg: u32) -> u32 {
    DUTY_MIN + (deg * (DUTY_MAX - DUTY_MIN)) / 180
}

/// duty ticks → 角度（回读换算，与 deg_to_duty 互逆的整数近似，±1° 内）。
pub(crate) fn duty_to_deg(duty: u32) -> u32 {
    ((duty.saturating_sub(DUTY_MIN)) * 180 + (DUTY_MAX - DUTY_MIN) / 2) / (DUTY_MAX - DUTY_MIN)
}

#[derive(Clone, Copy)]
pub(crate) enum Axis {
    Pan,
    Tilt,
}

impl Axis {
    /// 轴名（点名文案用，与帧面 `axis` 键取值同串）。
    pub(crate) fn name(self) -> &'static str {
        match self {
            Axis::Pan => "pan",
            Axis::Tilt => "tilt",
        }
    }

    /// 本轴相对中位的最大摆幅。
    pub(crate) fn limit_deg(self) -> u32 {
        match self {
            Axis::Pan => PAN_LIMIT_DEG,
            Axis::Tilt => TILT_LIMIT_DEG,
        }
    }

    /// 本轴绝对行程区间 `[lo, hi]` = 中位 ± 摆幅（WANDER 端点与 SCAN 判据点
    /// 都从这里取，机制不另写字面量）。
    pub(crate) fn range(self) -> (u32, u32) {
        (
            AXIS_CENTER_DEG - self.limit_deg(),
            AXIS_CENTER_DEG + self.limit_deg(),
        )
    }
}

/// 轴行程校验（**控制面纯函数**：只算不碰硬件，故控制面能在下发作业前即时拒）。
/// 与运行面 [`RtPeriph::servo_set`] 共用同一判据、同一文案（"一处真值、两个检查点
/// 复用同一判定"——两面都调本函数，杜绝控制/运行口径漂移）。
pub(crate) fn axis_check(axis: Axis, deg: u32) -> Result<u32, String> {
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

/// 轴行程五等分判据点（SCAN 判据：端点 + 三分点，全部落在本轴行程内——
/// 旧版写死的 0/45/135/180 越出机械行程，正是堵转来源）。
pub(crate) fn axis_scan_points(axis: Axis) -> [u32; 5] {
    let (lo, hi) = axis.range();
    let span = hi - lo;
    [lo, lo + span / 4, lo + span / 2, lo + span * 3 / 4, hi]
}

/// 扫摆步进：deg ± step，轴行程端点折返后返回新角度。dir 应为 ±1
/// （WANDER 开关随机起步；0 防御上不会出现——0 时原地不动）。
/// `bounds` 由 [`Axis::range`] 给出，端点即机械安全边界。
pub(crate) fn wander_step(deg: u32, dir: &mut i8, step: i32, bounds: (u32, u32)) -> u32 {
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