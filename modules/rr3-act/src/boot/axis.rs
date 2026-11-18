//! 执行轴状态装载：注入文本解析、坏形折保守缺省与策略面回显。

/// rrd spawn 注入的执行轴状态（批N3 件三，supervisor 与 RR3_HAL_BINDINGS 同位注入）：
/// 装配链装载 `hardware/<形态>/axis/*.toml`、按 `RR3_HAL_PERIPH_AXIS`（缺省契约
/// 常量）消歧后，对 instance 相等的恰一份合法档案投影
/// `calibrated=<bool>,hard_stop=<档案词>,lo=<n>,hi=<n>,center=<n>`。未注入（无档案/
/// 坏件/副本/轴错配）与坏形文本一律折保守缺省——§6"未标定不得自动"是代码拒动，
/// 缺真相就按最窄面起跑（行程窄域除外：保守行程 = 器件全域，点动面必须活着）。
const RR3_ACT_AXIS_STATE_ENV: &str = "RR3_ACT_AXIS_STATE";

/// 执行轴状态（批N3 件三建，件五扩形）：拒动判据 [`rr_hal_api::rr3_motion_allowed`]
/// 的输入面＋**行程真值**（门 g 收敛：模块行程改读档案——config 键撤除，`lo/hi`
/// 两键就是 `Rr3ServoCfg` 行程的填值来源，`center` 是档案中位锚的随路真值）。五键
/// 都来自档案（件二装载器真值），模块侧零判定载荷——只解析不判域（行程窗口自洽
/// lo<center<hi 在 rrd 装载器已判，这里不设第二裁决点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rr3ActAxisState {
    pub(crate) hard_stop: rr_hal_api::Rr3ActHardStop,
    pub(crate) calibrated: bool,
    /// 行程下限（度）——档案 lo_deg
    pub(crate) lo_deg: u16,
    /// 行程上限（度）——档案 hi_deg
    pub(crate) hi_deg: u16,
    /// 中位锚（度）——档案 center_deg（当前仅随路可查：jog/走位锚随 Rr3Actuator
    /// 进场接线，先保真值不丢）
    pub(crate) center_deg: u16,
}

impl Rr3ActAxisState {
    /// 保守缺省：未标定 + 挡块未知 + **器件全域行程**——判据按 §6 硬约束把 Auto
    /// 全拒，执行面只剩点动；行程给器件全域（0..=180）而非窄域，窄域会杀死点动面
    /// （点动是未标定轴唯一合法动作面，行程窄域等于把"活着"也收走）。center 取
    /// 全域中点 90。
    pub(crate) fn rr3_conservative() -> Self {
        Self {
            hard_stop: rr_hal_api::Rr3ActHardStop::Unknown,
            calibrated: false,
            lo_deg: 0,
            hi_deg: 180,
            center_deg: 90,
        }
    }

    /// 注入文本解析：`calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>`，
    /// 键序无关、五键必在、重复键/坏布尔/词表外词/越 u16/未知键逐条点名。解析失败
    /// 由调用方折保守缺省并打点（不 exit——保守态是安全向，退出反而进重启循环）。
    pub(super) fn rr3_from_env(text: &str) -> Result<Self, String> {
        let mut raw = Rr3AxisRaw::default();
        Self::rr3_axis_pairs(text, &mut raw)?;
        Self::rr3_axis_finish(raw)
    }

    /// 键值对切分：形状错点名，逐键交 [`rr3_axis_kv`] 路由。
    fn rr3_axis_pairs(text: &str, raw: &mut Rr3AxisRaw) -> Result<(), String> {
        for kv in text.split(',') {
            let (k, v) = Self::rr3_axis_split(kv)?;
            rr3_axis_kv(raw, k, v)?;
        }
        Ok(())
    }

    /// 单个键值对切分（缺 `=` 点名形状）。
    fn rr3_axis_split(kv: &str) -> Result<(&str, &str), String> {
        kv.split_once('=').ok_or_else(|| {
            format!(
                "执行轴状态段 {kv:?} 缺 =（形状 calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>）"
            )
        })
    }

    /// 五键齐备检查 → 投影（缺键逐条点名）。
    fn rr3_axis_finish(raw: Rr3AxisRaw) -> Result<Self, String> {
        let (calibrated, hard_stop) = rr3_axis_flags(raw.calibrated, raw.hard_stop)?;
        let (lo_deg, hi_deg, center_deg) = rr3_axis_pose_of(raw.lo, raw.hi, raw.center)?;
        Ok(Self {
            calibrated,
            hard_stop,
            lo_deg,
            hi_deg,
            center_deg,
        })
    }
}

/// 布尔/词表两键（缺席逐条点名）。
fn rr3_axis_flags(
    calibrated: Option<bool>,
    hard_stop: Option<rr_hal_api::Rr3ActHardStop>,
) -> Result<(bool, rr_hal_api::Rr3ActHardStop), String> {
    Ok((
        calibrated.ok_or("执行轴状态缺 calibrated")?,
        hard_stop.ok_or("执行轴状态缺 hard_stop")?,
    ))
}

/// 三数值键（缺席逐条点名）。
fn rr3_axis_pose_of(
    lo: Option<u16>,
    hi: Option<u16>,
    center: Option<u16>,
) -> Result<(u16, u16, u16), String> {
    Ok((
        lo.ok_or("执行轴状态缺 lo（档案行程下限）")?,
        hi.ok_or("执行轴状态缺 hi（档案行程上限）")?,
        center.ok_or("执行轴状态缺 center（档案中位锚）")?,
    ))
}

/// 注入文本的原始槽位（五键各一，重复键由 [`rr3_axis_seen`] 拒）。
#[derive(Default)]
struct Rr3AxisRaw {
    calibrated: Option<bool>,
    hard_stop: Option<rr_hal_api::Rr3ActHardStop>,
    lo: Option<u16>,
    hi: Option<u16>,
    center: Option<u16>,
}

/// 单键解析（**只做路由**；值与词表/域校验各自成体，文案逐字保留）。
/// B9：重复键判定前移一次（[`rr3_axis_seen`]），三数值键归一臂
/// [`rr3_axis_pose`]——路由本身不再每臂各判一次。
fn rr3_axis_kv(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    if rr3_axis_seen(raw, k)? {
        return Err(format!("执行轴状态 {k} 重复"));
    }
    rr3_axis_slot(raw, k, v)
}

/// 单键落位分派（重复键已由调用方判过；未知键点名）。
fn rr3_axis_slot(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    match k {
        "calibrated" | "hard_stop" => rr3_axis_flag(raw, k, v),
        "lo" | "hi" | "center" => rr3_axis_pose(raw, k, v),
        other => Err(format!("执行轴状态未知键 {other:?}")),
    }
}

/// calibrated/hard_stop 两键（显式布尔 / 档案词表）。
fn rr3_axis_flag(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    if k == "calibrated" {
        raw.calibrated = Some(rr3_axis_bool(v)?);
    } else if k == "hard_stop" {
        raw.hard_stop = Some(rr3_axis_stop(v)?);
    } else {
        return Err(format!("执行轴状态未知键 {k:?}"));
    }
    Ok(())
}

/// 键是否已出现（重复键判据的单点；未知键在此点名，与旧 [`rr3_axis_once`]
/// 同一文案域）。
fn rr3_axis_seen(raw: &Rr3AxisRaw, k: &str) -> Result<bool, String> {
    type Rr3AxisProbe = fn(&Rr3AxisRaw) -> bool;
    let probes: [(&str, Rr3AxisProbe); 5] = [
        ("calibrated", |r| r.calibrated.is_some()),
        ("hard_stop", |r| r.hard_stop.is_some()),
        ("lo", |r| r.lo.is_some()),
        ("hi", |r| r.hi.is_some()),
        ("center", |r| r.center.is_some()),
    ];
    let Some((_, probe)) = probes.iter().find(|(key, _)| *key == k) else {
        return Err(format!("执行轴状态未知键 {k:?}"));
    };
    Ok(probe(raw))
}

/// lo/hi/center 三数值键（同形 u16 域，文案按键取域说明）
fn rr3_axis_pose(raw: &mut Rr3AxisRaw, k: &str, v: &str) -> Result<(), String> {
    let n = rr3_axis_num(v, k, rr3_axis_what(k))?;
    if k == "lo" {
        raw.lo = Some(n);
    } else if k == "hi" {
        raw.hi = Some(n);
    } else {
        raw.center = Some(n);
    }
    Ok(())
}

/// lo/hi/center 三数值键的文案域说明。
fn rr3_axis_what(k: &str) -> &'static str {
    match k {
        "lo" => "行程下限",
        "hi" => "行程上限",
        _ => "中位锚",
    }
}

/// 显式布尔（只认 `true`/`false`：注入链错值必须点名，不猜同义写法）。
fn rr3_axis_bool(v: &str) -> Result<bool, String> {
    if v == "true" {
        Ok(true)
    } else if v == "false" {
        Ok(false)
    } else {
        Err(format!("执行轴状态 calibrated={v:?} 非显式布尔"))
    }
}

/// 硬限位词表（档案三值 `unknown/yes/no`；词表外点名回错）。
fn rr3_axis_stop(v: &str) -> Result<rr_hal_api::Rr3ActHardStop, String> {
    rr_hal_api::Rr3ActHardStop::rr3_hard_stop_parse(v)
        .ok_or_else(|| format!("执行轴状态 hard_stop={v:?} 不在档案词表（unknown/yes/no）"))
}

/// u16 域整数键（lo/hi/center 三键同形；`what` 是文案里的域说明）。
fn rr3_axis_num(v: &str, k: &str, what: &str) -> Result<u16, String> {
    v.parse::<u16>()
        .map_err(|_| format!("执行轴状态 {k}={v:?} 非{what}整数（u16 域）"))
}

/// 执行轴状态装载：env 读 + 坏形/缺席折保守缺省并点名（安全向，不退），回显注入
/// 的策略面（A-9：改数据即改行为，日志是第一证据）。自原 composite.rs 逐字迁入
/// （缺席/坏形/回显三文案逐字）。
pub(crate) fn rr3_axis_state_load() -> Rr3ActAxisState {
    let axis = rr3_axis_env_axis();
    rr3_axis_echo(&axis);
    axis
}

/// env 读取 + 坏形/缺席折保守缺省并点名（安全向，不退）。
fn rr3_axis_env_axis() -> Rr3ActAxisState {
    match std::env::var(RR3_ACT_AXIS_STATE_ENV) {
        Ok(text) => Rr3ActAxisState::rr3_from_env(&text).unwrap_or_else(|e| rr3_axis_bad(&e)),
        Err(_) => {
            eprintln!("执行轴状态: 未注入（无档案/坏件/副本/轴错配），按保守缺省起跑");
            Rr3ActAxisState::rr3_conservative()
        }
    }
}

/// 注入文本坏形：点名后折保守缺省（安全向，不退）。
fn rr3_axis_bad(e: &str) -> Rr3ActAxisState {
    eprintln!("rr3-act: {e}，按保守缺省起跑（未标定，只受点动）");
    Rr3ActAxisState::rr3_conservative()
}

/// 注入策略面回显（A-9：改数据即改行为，日志是第一证据）。
fn rr3_axis_echo(axis: &Rr3ActAxisState) {
    eprintln!(
        "执行轴状态: calibrated={} hard_stop={} 行程=[{}, {}] 中位={}（注入策略面在启动自述可见，A-9）",
        axis.calibrated,
        axis.hard_stop.rr3_hard_stop_name(),
        axis.lo_deg,
        axis.hi_deg,
        axis.center_deg
    );
}
