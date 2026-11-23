// SPDX-License-Identifier: Apache-2.0
//! 控制面作业队列：有界 + 折叠（`RedrawOled` 幂等、同形 `Move` 就地改写、
//! 满队列丢最旧 `Move` 给新目标让位）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use super::{lock_bus, Bus, Job};

/// 控制面推一件作业（唤醒运行面；运行面每轮取队列头执行）。
///
/// **有界 + 折叠**（稳健性修复）：宿主以快于舵机消化速率（2°/20ms，60° 需 600ms，
/// ≈1.7 指令/s 以上）连发时，旧版无界 `VecDeque` 会一路涨到堆耗尽（alloc 失败 =
/// panic/复位），且陈旧目标按序执行（宿主以为的当前位置几秒后才到）。规则：
/// 1. `RedrawOled` 幂等（依 `st` 重绘）——队列里已有一件即折叠掉本件；
/// 2. 同形 `Move`（被动的轴集合相同）就地改写目标角——后到者为准，不追加；
/// 3. 仍到 `JOB_CAP`：丢最旧的 `Move` 给新目标让位；非 Move 则拒收本件；
///    两者都计 `job_dropped`（`INFO` 观测）。
pub(crate) fn push_job(bus: &Arc<Mutex<Bus>>, job: Job) {
    let mut b = lock_bus(bus);
    if rr3_job_fold(&mut b.jobs, &job) {
        return;
    }
    if !rr3_job_room(&mut b, &job) {
        return;
    }
    b.jobs.push_back(job);
}

/// 折叠判定（规则 1/2）：`true` = 本件已并入既有作业，不再入队。
fn rr3_job_fold(jobs: &mut VecDeque<Job>, job: &Job) -> bool {
    if rr3_job_redraw_dup(jobs, job) {
        return true;
    }
    rr3_job_move_merge(jobs, job)
}

/// 规则 1：队列里已有一件 `RedrawOled` 即折叠本件。
fn rr3_job_redraw_dup(jobs: &VecDeque<Job>, job: &Job) -> bool {
    matches!(job, Job::RedrawOled) && jobs.iter().any(|j| matches!(j, Job::RedrawOled))
}

/// 规则 2：同形 `Move`（被动的轴集合相同）就地改写目标角。
fn rr3_job_move_merge(jobs: &mut VecDeque<Job>, job: &Job) -> bool {
    let Job::Move { pan, tilt } = job else {
        return false;
    };
    let Some(queued) = jobs.iter_mut().find(|j| rr3_job_same_shape(j, pan, tilt)) else {
        return false;
    };
    rr3_job_move_take(queued, *pan, *tilt);
    true
}

/// 同形判定：被动的轴集合（`is_some` 形状）相同。
fn rr3_job_same_shape(j: &Job, pan: &Option<u32>, tilt: &Option<u32>) -> bool {
    match j {
        Job::Move { pan: qp, tilt: qt } => {
            qp.is_some() == pan.is_some() && qt.is_some() == tilt.is_some()
        }
        _ => false,
    }
}

/// 目标角改写：新目标为 `Some` 的轴才覆盖（与旧实现同义）。
fn rr3_job_move_take(queued: &mut Job, pan: Option<u32>, tilt: Option<u32>) {
    if let Job::Move { pan: qp, tilt: qt } = queued {
        *qp = pan.or(*qp);
        *qt = tilt.or(*qt);
    }
}

/// 容量判定（规则 3）：`true` = 有位置可入队；`false` = 本件拒收（已计 job_dropped）。
fn rr3_job_room(b: &mut Bus, job: &Job) -> bool {
    if b.jobs.len() < crate::board::JOB_CAP {
        return true;
    }
    let oldest_move = b.jobs.iter().position(|j| matches!(j, Job::Move { .. }));
    if !matches!(job, Job::Move { .. }) || oldest_move.is_none() {
        b.st.job_dropped = b.st.job_dropped.saturating_add(1);
        return false;
    }
    if let Some(pos) = oldest_move {
        b.jobs.remove(pos);
    }
    b.st.job_dropped = b.st.job_dropped.saturating_add(1);
    true
}
