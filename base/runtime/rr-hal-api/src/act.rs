//! 执行/传感契约：GPIO、舵机、显示、环境采样、数字输入与执行件动作面。

use crate::err::{Rr3ErrText, Rr3HalError};

/// 数字输出（继电器、LED）
pub trait Rr3GpioOut {
    fn set(&mut self, level: bool) -> Result<(), Rr3HalError>;
    fn level(&self) -> Result<bool, Rr3HalError>;
}

/// PWM 舵机（角度级指令，闭环在 rr3-act 反射环内）
pub trait Rr3PwmServo {
    fn set_angle(&mut self, deg: u16) -> Result<(), Rr3HalError>;
    fn angle(&self) -> Result<u16, Rr3HalError>;
}

/// 位置读数来源（批N3，全案 §1 坑 11：开环舵机的"位置"不是测量——任何把
/// `angle()` 回读当位置的说法都是该坑的重演，来源必须在接口上可区分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3PositionSource {
    /// 指令回显：无位置反馈的开环面，"位置"= 最后一次已受理的指令值（记账位，
    /// 非测量——断电/堵转后它与真实轴角可任意偏离，消费方不得当反馈用）
    Commanded,
    /// 闭环反馈（`act.axis.enc` 预留格位，路线 D 换闭环执行器才进场——预留
    /// 不 = 立格，全案 §6 路线表；本版无实现）
    Encoder,
}

/// 位置读数（批N3）：角度与来源同帧——来源缺席的裸角度就是坑 11 的形状
/// （读数可信度与档位同帧给出的 T-6 同一族：拿不到"敢不敢信"的角度不许冒充测量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3Position {
    /// 角度（度，本轴行程域内；越域值由后端拒收，不进读数）
    pub deg: u16,
    /// 读数来源（[`Rr3PositionSource`]——Commanded 是回显不是测量）
    pub source: Rr3PositionSource,
}

/// 执行件挡块事实（批N3，档案 `hard_stop` 键的契约投影，全案 §4.4/§6）。
/// `unknown` 不是"没有挡块"，是"还没标定过这个事实"——护栏判据按最保守一侧处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActHardStop {
    /// 挡块事实未标定：路线 B 禁用、路线 C 自动段禁用（档案注释原文）
    Unknown,
    /// 存在硬挡块（标定实测过）
    Yes,
    /// 无硬挡块（标定实测过——路线 B 无挡块可找，同样禁用）
    No,
}

impl Rr3ActHardStop {
    /// 挡块事实词表（档名与取值的单点真值）
    const NAMES: [(Rr3ActHardStop, &'static str); 3] = [
        (Rr3ActHardStop::Unknown, "unknown"),
        (Rr3ActHardStop::Yes, "yes"),
        (Rr3ActHardStop::No, "no"),
    ];

    /// 档名（档案取值串同源：`unknown` / `yes` / `no`）
    pub fn rr3_hard_stop_name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(h, _)| *h == self)
            .map_or("", |(_, w)| *w)
    }

    /// 从档案词解析（词表外回 `None`，装载侧点名红，不猜——与 name 同源成对，
    /// 词形改动只许落这一处）
    pub fn rr3_hard_stop_parse(s: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(_, w)| *w == s).map(|(h, _)| *h)
    }
}

/// 执行件动作形态（批N3，判据实体 [`rr3_motion_allowed`] 的判据对象）。
/// 四形态各有判据，不许在调用点手抄第二份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActMotion {
    /// 点动（带 deadline，§6：唯一永不拒的动作形态——未标定轴的合法动作面）
    Jog,
    /// 运行期自动动作（扫摆 / 表情联动 / goto 走位）——未标定一律拒（§6 硬约束）
    Auto,
    /// 标定路线 C 自动段（开环自恢复走向软挡块，D13 已裁默认路线）——
    /// `hard_stop = unknown` 拒（§4.4 档案注释原文）
    CalibOpenLoop,
    /// 标定路线 B（电流找挡块）——`hard_stop = unknown` 拒、`no` 拒（无挡块可找）；
    /// 逐域电流采样前提归采样侧判据（校验⑥/⑨），不进本判据
    CalibStallSearch,
}

impl Rr3ActMotion {
    /// 动作形态词表（词名与取值的单点真值）
    const NAMES: [(Rr3ActMotion, &'static str); 4] = [
        (Rr3ActMotion::Jog, "jog"),
        (Rr3ActMotion::Auto, "auto"),
        (Rr3ActMotion::CalibOpenLoop, "calib-openloop"),
        (Rr3ActMotion::CalibStallSearch, "calib-stall-search"),
    ];

    /// 词名（命令面取值串同源：`jog` / `auto` / `calib-openloop` / `calib-stall-search`）
    pub fn rr3_act_motion_name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(m, _)| *m == self)
            .map_or("", |(_, w)| *w)
    }

    /// 从词解析（词表外回 `None`，调用侧点名红，不猜——与 name 同源成对，
    /// 词形改动只许落这一处）
    pub fn rr3_act_motion_parse(s: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(_, w)| *w == s).map(|(m, _)| *m)
    }
}

/// 执行件动作合法性判据实体（批N3，全案 §6 硬约束——"未标定不得自动"与
/// "hard_stop=unknown 护栏"都是**代码拒动**，非文档建议）。
///
/// 判据两条写完：
/// 1. `calibrated = false` ⇒ [`Rr3ActMotion::Auto`] 一律拒（`rr3_goto` 只收点动）；
/// 2. `hard_stop = unknown` ⇒ [`Rr3ActMotion::CalibOpenLoop`] /
///    [`Rr3ActMotion::CalibStallSearch`] 拒；`hard_stop = no` ⇒
///    [`Rr3ActMotion::CalibStallSearch`] 拒（无硬挡块可找）。
///
/// [`Rr3ActMotion::Jog`] 永远放行。
///
/// 单真值点：产出侧（rr3-act 组合件执行面）与判据用例共用本函数，不许各自
/// 手抄 `if` 链。拒收回 [`Rr3HalError::InvalidParam`]（文案带动作形态与判据名，
/// 与板侧 `axis_check` 越域回错同形——越界显式回错不静默 clamp）。
/// 动作 → 判据表项（形态, 判据）：判据只看硬挡块与标定态，返回拒因或放行。
type Rr3MotionRule = (
    Rr3ActMotion,
    fn(Rr3ActHardStop, bool) -> Result<(), Rr3HalError>,
);

pub fn rr3_motion_allowed(
    hard_stop: Rr3ActHardStop,
    calibrated: bool,
    motion: Rr3ActMotion,
) -> Result<(), Rr3HalError> {
    let rules: [Rr3MotionRule; 4] = [
        (Rr3ActMotion::Jog, |_, _| Ok(())),
        (Rr3ActMotion::Auto, |_, cal| rr3_motion_auto(cal)),
        (Rr3ActMotion::CalibOpenLoop, |hs, _| rr3_motion_calib(hs)),
        (Rr3ActMotion::CalibStallSearch, |hs, _| rr3_motion_stall(hs)),
    ];
    rules
        .iter()
        .find(|(m, _)| *m == motion)
        .map_or(Ok(()), |(_, f)| f(hard_stop, calibrated))
}

/// 拒收回错（文案带动作形态与判据名，与板侧 `axis_check` 越域回错同形）。
fn rr3_motion_refuse(why: &str) -> Result<(), Rr3HalError> {
    Err(Rr3HalError::InvalidParam(Rr3ErrText::rr3_err_text_new(why)))
}

/// 自动动作：未标定一律拒。
fn rr3_motion_auto(calibrated: bool) -> Result<(), Rr3HalError> {
    if calibrated {
        return Ok(());
    }
    rr3_motion_refuse("未标定轴不得自动（只许点动，§6 硬约束）")
}

/// 路线 C 自动段：hard_stop=unknown 拒。
fn rr3_motion_calib(hard_stop: Rr3ActHardStop) -> Result<(), Rr3HalError> {
    if hard_stop == Rr3ActHardStop::Unknown {
        return rr3_motion_refuse("hard_stop=unknown：路线 C 自动段禁用（§6 护栏）");
    }
    Ok(())
}

/// 路线 B（电流找挡块）：hard_stop≠yes 拒（unknown 未标定挡块事实 / no 无挡块可找）。
fn rr3_motion_stall(hard_stop: Rr3ActHardStop) -> Result<(), Rr3HalError> {
    if hard_stop != Rr3ActHardStop::Yes {
        return rr3_motion_refuse(
            "hard_stop≠yes：路线 B 禁用（unknown 未标定挡块事实 / no 无挡块可找）",
        );
    }
    Ok(())
}

/// 执行件动作面（批N3，全案 §9 N3——"关节成为对象"的契约承载）。
///
/// 实现归属：rr3-act 组合件在 `dyn Rr3PwmServo` 句柄之上包行程 / 标定 / 拒动
/// 语义（与批N2 件五 RailMonitor 归属搬家同构——机制住组合件，契约住本层）。
/// 拒动判据不进签名：调用方先经 [`rr3_motion_allowed`] 判，再调本面——
/// 契约层只供给动作形态与判据实体，不持策略（[`crate::Rr3Rail`] 同线）。
///
/// 四方法在立 trait 时一次定形（§18 F-32：不许有一个未定义就进文档的方法名）：
/// - [`Rr3Actuator::rr3_jog`]：点动一步（带 deadline），未标定轴唯一合法动作；
/// - [`Rr3Actuator::rr3_goto`]：自动走位（判据拒绝时本方法不受理）；
/// - [`Rr3Actuator::rr3_position`]：位置读数，角度与来源同帧（[`Rr3Position`]）；
/// - [`Rr3Actuator::rr3_hold`]：激磁保持语义（`false` = 释放激磁，`failsafe=release`
///   既有裁定，§6 复位语义③）。
pub trait Rr3Actuator {
    /// 点动一步到 `deg`（超 `deadline_ms` 回错——受控堵转护栏的时间维，§6 路线 B）
    fn rr3_jog(&mut self, deg: u16, deadline_ms: u32) -> Result<(), Rr3HalError>;
    /// 自动走位到 `deg`（判据 [`rr3_motion_allowed`] 拒收的动作形态不受理）
    fn rr3_goto(&mut self, deg: u16, deadline_ms: u32) -> Result<(), Rr3HalError>;
    /// 位置读数（角度 + 来源同帧；无反馈面回 `Commanded` 回显，不许冒充测量）
    fn rr3_position(&self) -> Result<Rr3Position, Rr3HalError>;
    /// 激磁保持（`true` = 保持激磁；`false` = 释放——欠压/复位后让轴自由，
    /// 避免复位循环里反复堵转，§6 复位语义③）
    fn rr3_hold(&mut self, hold: bool) -> Result<(), Rr3HalError>;
}

/// 8x8 点阵单帧（R11③ 定案档）：行主序 8 字节，每字节 bit7 为该行最左点
/// （MAX7219 行寄存器直排）。类型在契约层定义是刻意的：它是 [`Rr3Display`]
/// trait 签名的事实源，模块面（rr3-interact 表情机）只引用不复制定义。
pub type Rr3FaceFrame = [u8; 8];

/// 显示面（8x8 点阵帧渲染，R11③——单 trait 帧面）。
///
/// "状态→帧"映射与表情白名单是模块面策略（同 led 灯效白名单先例，不进契约层）：
/// 表情机出帧、显示件收帧，HAL 只搬运已定的位图。`draw` 语义是整帧替换
/// （MAX7219 是 8 个行寄存器的一次全刷），没有局部改写。
pub trait Rr3Display {
    /// 渲染一帧（整帧替换，阻塞至行寄存器落定——事件级慢速面，无帧队列）
    fn draw(&mut self, frame: &Rr3FaceFrame) -> Result<(), Rr3HalError>;
    /// 读回当前帧（开机探活与对账：读回 == 上次 draw 的帧）
    fn frame(&self) -> Result<Rr3FaceFrame, Rr3HalError>;
}

/// 采样形状类别（增补轮六 §3.5 家族 E：一张格多器件档案，不逐器件扩格——
/// 温湿度这类单值量与姿态这类三元量共用一张格，形状在样本上可区分而非格上分家）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3SampleKind {
    /// 标量量（温度/湿度/光照/VOC 这类单值；取 [`Rr3Sample::value`][0]）
    Scalar,
    /// 三元量（加速度/角速度这类姿态分量；value 全长有效，融合归 act 反射环——
    /// sensor.imu 格 note 明写，契约面只搬运不定融合语义）
    Vec3,
}

/// 样本可信度（T-6 同族："读数可信度"必须与读数同帧给出——拿不到可信度的
/// 数值不许冒充测量。`Stale` 是后端缓存的陈旧快照，消费方按最保守一侧处置）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3SampleQuality {
    /// 本次轮询实测
    Fresh,
    /// 陈旧快照（器件无响应等——数值仍给出但已不可信，消费方不得当新鲜数据用）
    Stale,
}

/// 一次环境量采样（定长形状，`no_std` 可达——[全案 §3.5](../../docs/process/dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)：
/// `Rr3Sample{kind,value,unit,quality}`。`value` 定长 `[f32; 3]`：`Scalar` 只用
/// [0]，`Vec3` 全长有效——不引入 Vec，与 [`Rr3FaceFrame`] 同一"契约层不拥有堆"线）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rr3Sample {
    /// 形状类别（[`Rr3SampleKind`]——value 的有效长度由此裁）
    pub kind: Rr3SampleKind,
    /// 采样值（Scalar 用 [0]；Vec3 全长；单位见 unit）
    pub value: [f32; 3],
    /// 计量单位（器件档案同源："°C"/"%RH"/"lux" 这类 SI 词；&'static str——
    /// 单位词是器件事实不是运行期拼串）
    pub unit: &'static str,
    /// 可信度位（[`Rr3SampleQuality`]——与读数同帧，T-6）
    pub quality: Rr3SampleQuality,
}

/// 环境量采样面（批N3 契约面先行，无实现件——件一 `Rr3Actuator` 先例 c5220f5；
/// 真后端随真板批，`sensor.env`/`sensor.imu` 格升 normal 的时机 = 本 trait 落 +
/// 一个真后端到，D9 一并裁）。探测面先行不依赖它：器件级在位探测走
/// devices.toml `id_reg`（L-14），句柄面与此独立。
pub trait Rr3Sensor {
    /// 读一次样本（[`Rr3Sample`] 四元同帧：形状/数值/单位/可信度）
    fn read(&self) -> Result<Rr3Sample, Rr3HalError>;
}

/// 数字输入面（批N3 契约面先行；与 [`Rr3GpioOut`] 对称——能力面今天只有输出
/// 没有输入是半边残缺，全案 §3.5 家族 E）。触摸/限位/碰壁检测这类部件的
/// 极性（active_low）、消抖（debounce_ms）、内部上拉（internal_pull）走
/// [pins] 绑定参数（roles.toml requires），不在签名里——签名只有"读"一个动词。
pub trait Rr3DigitalIn {
    /// 读逻辑有效态：active_low 绑定时物理低 = true；消抖由后端按绑定参数执行
    /// （未消抖的毛刺不许透传——碰壁检测误触发 = 停机误动作）。
    fn level(&self) -> Result<bool, Rr3HalError>;
}
