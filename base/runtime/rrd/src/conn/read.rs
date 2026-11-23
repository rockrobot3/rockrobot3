// SPDX-License-Identifier: Apache-2.0
//! 读侧循环与静默上界（ADR-0025 裁定 1/4）：注册后的逐帧读取、读错分流与
//! 空闲回收。父模块 `conn.rs` 只留连接主流程与收尾，本文件不碰协议路由
//! （`rr3_serve_frame` 仍在父模块，是唯一入口）。
//!
//! 上界语义：静默上界 = 该模块自己申报的 `watchdog_ms × 2`。2 倍是归属裁定
//! ——保证看门狗先到期，连接收尾走 `Silent`，判死/广播/重启预算只归看门狗
//! 一拍。健康模块按 heartbeat_ms 喂狗（< watchdog_ms），正常业务永远触不到
//! 这条界。

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rr_ipc::{rr3_frame_read_cap, Rr3FrameError, RR3_FRAMING_PLEN_CAP_DEFAULT};

use super::{rr3_serve_frame, Rr3ConnCtx, Rr3Flow, Rr3Peer};
use crate::registry::RrdRegistry;
use crate::{rr3_lock, rr3_log};

/// 注册后读侧静默上界就位（ADR-0025 裁定 1/4），返回上界毫秒；0 = 不设上界。
///
/// 在册事实缺失或看门狗窗口为 0 时不设上界并留痕——宁可退化为无上界，也不拿
/// 错的上界误杀健康连接。
pub(super) fn rr3_conn_idle_set(
    stream: &UnixStream,
    shared: &Arc<Mutex<RrdRegistry>>,
    name: &str,
) -> Result<u64, String> {
    let idle_ms = rr3_lock(shared)
        .rr3_reg_facts(name)
        .map(|f| f.watchdog_ms.saturating_mul(2))
        .unwrap_or(0);
    rr3_idle_arm(stream, name, idle_ms)?;
    Ok(idle_ms)
}

/// 按上界落位：有窗口即设读超时，无窗口走无上界分支。
fn rr3_idle_arm(stream: &UnixStream, name: &str, idle_ms: u64) -> Result<(), String> {
    if idle_ms > 0 {
        stream
            .set_read_timeout(Some(Duration::from_millis(idle_ms)))
            .map_err(|e| format!("set_read_timeout 失败: {e}"))?;
        Ok(())
    } else {
        rr3_idle_unbounded(stream, name)
    }
}

/// 无上界分支：留痕并清掉读超时（退化依赖对端断开）。
fn rr3_idle_unbounded(stream: &UnixStream, name: &str) -> Result<(), String> {
    rr3_log(&format!(
        "{name} 无可用看门狗窗口（在册事实缺失或为 0），读侧不设静默上界——\
         连接回收退化为依赖对端断开"
    ));
    stream
        .set_read_timeout(None)
        .map_err(|e| format!("清读超时失败: {e}"))
}

/// 注册后的读循环：逐帧读、逐帧处理，直到干净收尾或带原因断开。
/// EOF（对端关连接）与 `rr3.unregister` 都是 `QuitClean`；其余读错/处理错带原因返回。
///
/// `idle_ms` 是注册后静默上界（ADR-0025：2 × 该模块看门狗窗口，0 = 不设上界）。
/// 触发时文案点名空闲时长与归属——判死、广播、重启预算仍归看门狗那一拍，本循环
/// 只回收连接本身。
pub(super) fn rr3_read_loop(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    reader: &mut UnixStream,
    idle_ms: u64,
) -> Result<Rr3Flow, String> {
    let mut last_frame = Instant::now();
    loop {
        // 逐帧读：`Some` = 该收尾了（干净或带原因），`None` = 继续下一帧。
        if let Some(done) = rr3_read_step(out, ctx, me, reader, &mut last_frame, idle_ms) {
            break done;
        }
    }
}

/// 读一帧并处理；`Some(结论)` = 收尾，`None` = 本帧已消化、继续读。
fn rr3_read_step(
    out: &Arc<Mutex<UnixStream>>,
    ctx: &Rr3ConnCtx,
    me: &Rr3Peer,
    reader: &mut UnixStream,
    last_frame: &mut Instant,
    idle_ms: u64,
) -> Option<Result<Rr3Flow, String>> {
    match rr3_frame_read_cap(&mut *reader, RR3_FRAMING_PLEN_CAP_DEFAULT) {
        Ok(frame) => {
            *last_frame = Instant::now();
            match rr3_serve_frame(out, ctx, me, &frame) {
                Ok(Rr3Flow::Keep) => None,
                Ok(Rr3Flow::QuitClean) => Some(Ok(Rr3Flow::QuitClean)),
                Err(reason) => Some(Err(reason)),
            }
        }
        Err(Rr3FrameError::Closed) => Some(Ok(Rr3Flow::QuitClean)),
        Err(e) => Some(Err(rr3_read_err(&e, *last_frame, idle_ms))),
    }
}

/// 读错文案：静默超时（上界 = 2 × 看门狗）与普通读错分流。
fn rr3_read_err(e: &Rr3FrameError, last_frame: Instant, idle_ms: u64) -> String {
    let idle = last_frame.elapsed().as_millis() as u64;
    if idle_ms > 0 && idle >= idle_ms {
        format!(
            "静默超时：{idle}ms 无完整帧（上界 = 2 × 看门狗 {idle_ms}ms），\
             本连接回收；判死与重启归看门狗一拍"
        )
    } else {
        format!("帧读取失败: {e:?}")
    }
}
