//! 装配侧策略量写口（批N22 件一，敞口 15 写面半边；判据源三处：conventions §6.1
//! 三写口点名、ADR-0021 裁定 6"拒动类阈值写必须带二次确认与留痕"、B-15 ③"安全写
//! 必须携带证据引用"）。
//!
//! 职责边界（机制与策略分离）：**机制**全在别处单点——文本手术在
//! [`crate::profile::rr3_domain_apply`] / [`crate::profile::rr3_key_line_apply`]，
//! 原子落盘在 `rr_ipc::persist::rr3_atomic_store`（re-export 单点
//! [`crate::counters::rr3_atomic_store`]），装载判据在 [`crate::profile::
//! rr3_boot_inputs`]（与 boot 正门同一份裁决，写面不重复判业务合法性），
//! 证据四道核在 `rr_ipc::evidence`（唯一实现，§6.1 纪律），轴档全判据＋落盘在
//! [`crate::axis_write::rr3_axis_write_run`]（§7"落盘只许有一份实现"，第二条写
//! 路径即红——本模块是它的第二个接线位，不是第二份实现）。**本模块只持写编排
//! 策略**：谁能写（confirm 门＋key 白名单）、凭什么写（证据引用规则）、写到哪
//! （路径自真值集记账推导，零新增全局）、怎么生效（落盘 → 装载链预核 → 原子写回
//! → 契约记账重记；**运行态不动，下次 rrd boot 生效**）。
//!
//! 三写口与证据引用规则（conventions §6.1"哪些写必须带引用"归各写面）：
//! - `armed`：置 true 必携证据引用且**联动入档**（装载期⑥-e 成对纪律——armed=true
//!   必携形制合法引用、armed=false 显式写 `"none"` 不携过期引用，写面直接落出
//!   成对形状）；置 false 不要求引用，档内 evidence 联动写 `"none"`。
//! - `severe_mv` / `current_ceil_ma`：必携证据引用，**仅留痕不入档**（档内
//!   evidence 键语义 = 武装证据，单主键；写动作的证据住 rrd 日志审计留痕）。
//! - 三个键统一 confirm 门与留痕：ADR-0021 裁定 6 点名 severe_mv/armed 必须
//!   二次确认；current_ceil_ma 同为拒动类阈值（写大 = 烧轴风险），同一端点的
//!   三键差别化确认会让端点语义劈叉，取保守侧统一（fail-closed 方向不违判据源）。
//!
//! 生效语义与记账跟随：落盘后运行态（装配快照）维持 boot 时旧值——`effective:
//! "boot"` 应答如实申报；契约记账快照在写口成功后重记（写口的 tmp 产物过装载链
//! 全量预核 = 新验收事实，记账跟住最新验收事实——否则写口自造一笔常驻"sha 漂"
//! 自检红，把真漂移淹没，见 contracts.rs Mutex 化注记）。
//!
//! 快照未在册（`--manifest` 调试路径）如实回不可得：无记账即无路径依据，写口
//! 拒（与 wiring/contracts 读口"如实报不可得"同一条诚实纪律）。

use std::path::{Path, PathBuf};

use rr_ipc::Rr3StatusCode;
use serde_json::{json, Value};

use crate::contracts::Rr3TruthSetEntry;

mod axis;
mod params;
mod paths;
mod plan;
#[cfg(test)]
mod tests;

pub use axis::*;
pub use params::*;
pub use paths::*;
pub use plan::*;

/// conn 臂入口：执行 + 拒绝留痕（ADR-0021 裁定 6"二次确认与留痕"的拒半边——
/// 成功路径在各自 `rr3_write_*` 里留痕，被拒的安全写在审计面也必须可查）。
/// 拒因文案不含任何密钥值（写口参数只有域/轴名、键名、数字与证据引用）。
#[cfg(unix)]
pub fn rr3_domain_write_run(params: &Value) -> Result<Value, (Rr3StatusCode, String)> {
    let out = rr3_write_body(params);
    if let Err((code, msg)) = &out {
        crate::rr3_log(&format!("domain 写口拒（{code:?}）: {msg}"));
    }
    out
}

/// 写口执行薄壳：params 拆包 → 快照路径推导 → 执行。快照未在册（--manifest
/// 调试路径）如实回不可得——无记账即无路径依据，写口不猜路径。
#[cfg(unix)]
fn rr3_write_body(params: &Value) -> Result<Value, (Rr3StatusCode, String)> {
    let parsed = rr3_write_parse(params)?;
    let mut p = parsed.clone();
    p.confirm = Rr3DomainWriteParams::confirm_from(params);
    let Some(snap) = crate::contracts::rr3_contracts_snap() else {
        return Err((
            Rr3StatusCode::Permission,
            "契约记账未在册（--manifest 调试路径无装配单），写口不可得——请以 --profile 启动"
                .to_string(),
        ));
    };
    let paths = rr3_write_paths(&snap.truth_set).map_err(|e| (Rr3StatusCode::Invalid, e))?;
    rr3_write_execute(&paths, &p)
}
