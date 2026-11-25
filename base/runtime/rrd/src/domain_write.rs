// SPDX-License-Identifier: Apache-2.0
//! 装配侧策略量写口：`armed` / `severe_mv` / `current_ceil_ma` 三个拒动类阈值的写编排。
//!
//! 职责边界：**机制全在别处单点**——文本手术在 [`crate::profile::rr3_domain_apply`]
//! 与 [`crate::profile::rr3_key_line_apply`]，原子落盘在
//! `rr_ipc::persist::rr3_atomic_store`，装载判据在 [`crate::profile::rr3_boot_inputs`]
//! （与 boot 正门同一份裁决，写面不重复判业务合法性），证据四道核在 `rr_ipc::evidence`，
//! 轴档落盘在 [`crate::axis_write::rr3_axis_write_run`]（本模块是第二接线位，不是第二
//! 份实现）。本模块只持**写编排策略**：谁能写（confirm 门 + key 白名单）、凭什么写
//! （证据引用规则）、写到哪（路径自真值集记账推导）、怎么生效（落盘 → 装载链预核 →
//! 原子写回 → 契约记账重记；**运行态不动，下次 boot 生效**）。
//!
//! 三写口与证据规则：
//! - `armed`：置 true 必携证据引用且**联动入档**（装载期⑥-e 成对纪律）；置 false
//!   不要求引用，档内 evidence 写 `"none"`。
//! - `severe_mv` / `current_ceil_ma`：必携证据引用，**仅留痕不入档**（档内 evidence
//!   键语义 = 武装证据，单主键；写动作的证据住 rrd 日志审计）。
//! - 三键统一 confirm 门：ADR-0021 裁定 6 点名 severe_mv/armed 须二次确认；
//!   current_ceil_ma 同为拒动类（写大 = 烧轴风险），差别化确认会让端点语义劈叉，
//!   取保守侧统一（fail-closed 方向不违判据源）。
//!
//! 生效与记账：落盘后装配快照维持 boot 时旧值，应答如实 `effective: "boot"`；写口的
//! tmp 产物过装载链全量预核后，契约记账快照重记（跟住最新验收事实——否则自造常驻
//! "sha 漂"自检红，把真漂移淹没）。
//!
//! 快照未在册（`--manifest` 调试路径）如实回不可得：无记账即无路径依据，写口拒
//! （与 wiring/contracts 读口同一条诚实纪律）。

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

use axis::*;
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
