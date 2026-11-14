//! 执行件档案契约：十四键词表、档案值面、轴 env 映射与载体窄缝。

use crate::act::Rr3ActHardStop;
use crate::err::Rr3HalError;

/// 执行件档案字段的读者三态（ADR-0022 修订 1 工单 F-25，门 ⑨-m 的**标注面**——
/// "标注与实际读者集合不一致即红"的唯一数据源）。三态含义：
/// [`Rr3ActFieldReader::Adjudicate`] = 参与裁决（判据输入，拒动 / 对账一侧）、
/// [`Rr3ActFieldReader::Act`] = 有动作侧读者（驱动执行）、
/// [`Rr3ActFieldReader::Echo`] = 仅回显（写了保护语义却没有动作侧——如实标注，
/// "登记为只回显不丢人，写了保护语义却没有动作侧才丢人"，§4.4）。
///
/// 已知"只回显"如实登记（§4.4 点名 `settle_ms`（批N3 前无串行读者）与
/// `warmup_ms`（激磁预热无动作侧）两处候选；`source` 标定来源是追溯回显面，
/// 同属回显——标注与实际读者集合一致，门 ⑨-m 建成后按本表对账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActFieldReader {
    Adjudicate,
    Act,
    Echo,
}

/// 执行件档案字段词表（批N3，全案 §4.4 档案形状的**契约面**：键名 / 值类型 /
/// 读者三态一张表）。装载侧（rrd 档案装载器）的白名单、类型派发与门 ⑨-m 的
/// 标注对账都只认本表——键名第二份真值点不存在。
///
/// `schema` 与 `pin` 两键是装载域键（版本对账 / 外键对账），读者同属裁决侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ActProfileField {
    pub key: &'static str,
    /// 值类型（int = 整数域，取值域是键自己的事实、由装载侧键级判据管——行程键
    /// 非负、`dir_sign` ∈ {±1}、ms 键上限 u32；text = 在册枚举词；bool = 显式布尔）
    pub kind: Rr3ActFieldKind,
    pub reader: Rr3ActFieldReader,
}

/// 档案字段值类型（装载派发用，与 [`Rr3ActProfileField`] 同表）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActFieldKind {
    Int,
    Text,
    Bool,
}

/// 档案十四键（§4.4 示例档逐键对齐；加键 = 本表 append + 装载侧同批，契约级评审）
pub const RR3_ACT_PROFILE_FIELDS: &[Rr3ActProfileField] = &[
    Rr3ActProfileField {
        key: "schema",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "pin",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "center_deg",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "lo_deg",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "hi_deg",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "dir_sign",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "slew_deg_s",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "settle_ms",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Echo,
    },
    Rr3ActProfileField {
        key: "hold",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Act,
    },
    Rr3ActProfileField {
        key: "current_ceil_ma",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "warmup_ms",
        kind: Rr3ActFieldKind::Int,
        reader: Rr3ActFieldReader::Echo,
    },
    Rr3ActProfileField {
        key: "hard_stop",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "calibrated",
        kind: Rr3ActFieldKind::Bool,
        reader: Rr3ActFieldReader::Adjudicate,
    },
    Rr3ActProfileField {
        key: "source",
        kind: Rr3ActFieldKind::Text,
        reader: Rr3ActFieldReader::Echo,
    },
];

/// 激磁保持策略（批N3，档案 `hold` 键的契约投影，全案 §4.4：`hold | release`
/// 对应"不保持激磁"的既有裁定）。词表两态显式给词，不给布尔缺省——缺省与
/// 漏填不可区分（T-5 同一族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActHold {
    /// 保持激磁
    Hold,
    /// 释放激磁（§4.4 示例档取值——欠压/复位让轴自由，§6 复位语义③）
    Release,
}

impl Rr3ActHold {
    /// 保持策略词表（档名与取值的单点真值）
    const NAMES: [(Rr3ActHold, &'static str); 2] =
        [(Rr3ActHold::Hold, "hold"), (Rr3ActHold::Release, "release")];

    /// 档名（档案取值串同源：`hold` / `release`）
    pub fn rr3_act_hold_name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(h, _)| *h == self)
            .map_or("", |(_, w)| *w)
    }

    /// 从档案词解析（词表外回 `None`，装载侧点名红，不猜——与 name 同源成对）
    pub fn rr3_act_hold_parse(s: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(_, w)| *w == s).map(|(h, _)| *h)
    }
}

/// 标定来源（批N3，档案 `source` 键的契约投影，全案 §4.4：标定动作的来源，
/// 可追溯）。词表四态与 §6 标定路线表同源——`openloop-recover` = 路线 C、
/// `stall-search` = 路线 B；`manual` / `fixture` 是人工两侧（手持 / 夹具）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3ActSource {
    /// 手工标定（手持限位/目测，出厂或现场）
    Manual,
    /// 夹具标定（定位工装，产线口径）
    Fixture,
    /// 开环自恢复（路线 C 自动段）
    OpenLoopRecover,
    /// 电流找挡块（路线 B）
    StallSearch,
}

impl Rr3ActSource {
    /// 标定来源词表（档名与取值的单点真值）
    const NAMES: [(Rr3ActSource, &'static str); 4] = [
        (Rr3ActSource::Manual, "manual"),
        (Rr3ActSource::Fixture, "fixture"),
        (Rr3ActSource::OpenLoopRecover, "openloop-recover"),
        (Rr3ActSource::StallSearch, "stall-search"),
    ];

    /// 档名（档案取值串同源：`manual` / `fixture` / `openloop-recover` / `stall-search`）
    pub fn rr3_act_source_name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(s, _)| *s == self)
            .map_or("", |(_, w)| *w)
    }

    /// 从档案词解析（词表外回 `None`，装载侧点名红，不猜——与 name 同源成对）
    pub fn rr3_act_source_parse(s: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(_, w)| *w == s).map(|(s, _)| *s)
    }
}

/// ESP32 档舵机轴 env 键（批N3 件五上移单点）：宿主单轴 trait ↔ 板上双轴云台的
/// 映射面——rr-hal 桥后端按它选轴（缺省 [`RR3_PERIPH_AXIS_DEFAULT`]，板侧帧面
/// `axis` 参数可逐调用覆盖），rrd 装配链按同一 env、同一缺省为注入面选轴档案
/// （执行轴状态只投影被驱动那根轴的行程真值）。键名/缺省值两份副本不存在。
pub const RR3_PERIPH_AXIS_ENV: &str = "RR3_HAL_PERIPH_AXIS";
/// 轴 env 缺省值（与板侧帧面 `axis` 键缺席 = pan 同构；档案 pin instance 词形
/// 与本值同域——pin 真值在装配单 [[pin]]，此处只是映射缺省，不是词表）。
pub const RR3_PERIPH_AXIS_DEFAULT: &str = "pan";

/// 执行件档案值面（批N3，全案 §4.4 十四键的**值投影**）。装载域两键不进本面：
/// `schema` 是版本对账数（装载侧只认 1，升版走契约级评审）、`pin` 是外键
/// （F-4，由装载产物 `Rr3AxisFile` 持有）——十二键值域逐键注记给到。
/// no_std 值类型：无 String / 无 Vec，与契约面 F1 同线。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rr3ActProfile {
    /// 中位角（度）——点动与走位的角域锚（自洽判据 lo < center < hi）
    pub center_deg: u16,
    /// 行程下限（度）——**行程真值收敛点**（§1 坑 8：档案之外的行程读者数 = 0）
    pub lo_deg: u16,
    /// 行程上限（度）——同上
    pub hi_deg: u16,
    /// 方向符号（±1；0 = 物理矛盾，装载红）
    pub dir_sign: i8,
    /// 速度上限（度/秒，>0）
    pub slew_deg_s: u16,
    /// 到位稳定窗口 ms（当前仅回显——F-25 如实标注，串行读者进场同批改标）
    pub settle_ms: u32,
    /// 激磁保持策略（[`Rr3ActHold`]）
    pub hold: Rr3ActHold,
    /// 电流上限 mA（>0，校验⑨ 预算一侧的执行件事实）
    pub current_ceil_ma: u16,
    /// 激磁预热窗口 ms（当前仅回显——F-25 如实标注，激磁预热无动作侧）
    pub warmup_ms: u32,
    /// 挡块事实（[`Rr3ActHardStop`]——unknown 按最保守一侧处置，判据
    /// [`crate::act::rr3_motion_allowed`]）
    pub hard_stop: Rr3ActHardStop,
    /// 标定完成位（false ⇒ Auto 拒，§6 硬约束——rtos 档只许运行期内存态，
    /// §4.4 载体语义②/S-14）
    pub calibrated: bool,
    /// 标定来源（[`Rr3ActSource`]，当前仅回显——追溯面）
    pub source: Rr3ActSource,
}

/// 载体窄缝（批N3，全案 §4.4 Y-2/Y-7 + 工单 sense S-14）：执行件档案的掉电载体
/// 抽象。host 档档案是文件（rrd 装载链直读直写，复用 counters 原子序，不经本缝）；
/// rtos 档的 NVS 实现走本缝。**S-14 三项载体性质实测前（`set_blob` 掉电原子性 /
/// 写放大上限 / 单键容量上限），实现侧 fail-closed 回 `Unsupported`**——
/// `calibrated` 只许运行期内存态（§6 复位语义④），rtos 可达终点表明写
/// "标定状态重启即失"（§17），不许拿一个没测过掉电原子性的写路径冒充持久化。
pub trait Rr3Store {
    /// 读一个键到调用方缓冲，返回有效字节数（缓冲不足回 `InvalidParam`——
    /// 容量语义显式，不静默截断：档案字节被劈半比失败更糟）
    fn rr3_store_read(&self, key: &str, buf: &mut [u8]) -> Result<usize, Rr3HalError>;
    /// 写一个键（掉电原子性未实测前实现侧必须回 `Unsupported`，见类型注记）
    fn rr3_store_write(&mut self, key: &str, data: &[u8]) -> Result<(), Rr3HalError>;
}
