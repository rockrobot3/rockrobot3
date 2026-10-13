use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::board::{
    axis_scan_points, duty_to_deg, Axis, AXIS_SETTLE_MS, RAIL_SAMPLE_PERIOD_MS, RUNTIME_TICK_MS,
    SCAN_POINT_MS,
};
use crate::oled::i2c_bus_recover;
use crate::periph::RtPeriph;
use crate::state::{lock_bus, now_ms, outbox_push, Bus, Job};

// ---------------------------------------------------------------------------
// 运行面（独占 I2C0 + LEDC）：执行硬件作业 + 自主节拍（表情微动作 / 云台扫摆）
// ---------------------------------------------------------------------------

/// 运行面主循环：有作业即取之执行（硬件慢动作全在此），无作业走自主节拍（10ms
/// 一拍）。**慢 I2C / 舵机整定都停在本线程**——控制面命令吞吐不受其阻塞（这是
/// "控制面/运行面分离"的落点，消"卡顿"）。
pub(crate) fn runtime_loop(mut ph: RtPeriph, bus: Arc<Mutex<Bus>>) {
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
            if let Err(e) = ph.render_frame(&frame) {
                eprintln!("[periph] 点阵渲染失败: {e}");
            }
        }
        Job::SampleRail => match sample_rail(ph, bus) {
            Ok((mv, raw)) => {
                println!("[periph] RAIL 采样 rail={mv}mV pin_raw={raw}");
                outbox_push(bus, format!("RAIL rail={mv}mV raw={raw}"));
            }
            Err(e) => {
                eprintln!("[periph] RAIL 采样失败: {e}");
                outbox_push(bus, format!("ERR RAIL 采样失败: {e}"));
            }
        },
    }
}

/// 采样一次 5V 轨 → 判欠压档 → 写共享状态（**机制面**，方案A Q1/Q2）：成功即更新
/// `st.rail_mv/rail_raw/rail_tier`（越阈事件另计 `st.rail_low_events` 并打站）；失败只
/// 回 Err（**不改状态**——stale 旧值不冒充新鲜读数）。判定核 [`crate::power::RailJudge`]
/// 在此喂一拍：边沿产**一次性越阈事件**（打站/计数），电平产**档位**（驱动面拒动/降级）。
fn sample_rail(ph: &mut RtPeriph, bus: &Arc<Mutex<Bus>>) -> Result<(u16, u16), String> {
    let (mv, raw) = ph.rail_sample()?;
    let (tier, event) = ph.rail_judge.feed(mv);
    ph.rail_tier = tier;
    {
        let mut b = lock_bus(bus);
        b.st.rail_mv = mv;
        b.st.rail_raw = raw;
        b.st.rail_tier = tier;
        if event {
            b.st.rail_low_events = b.st.rail_low_events.saturating_add(1);
        }
    }
    if event {
        println!(
            "[periph] 5V 轨欠压越阈 tier={} rail={mv}mV raw={raw}",
            tier.name()
        );
    }
    Ok((mv, raw))
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
    // 5V 轨周期采样 + 欠压判定（**机制面**，方案A Q1/Q2）：到点采一次、喂判定核，写
    // 档位/计数入共享状态（`INFO` 观测面 + `servo_set` 拒动判据）。失败只打站不 panic。
    if now_ms() >= ph.next_rail_ms {
        ph.next_rail_ms = now_ms() + RAIL_SAMPLE_PERIOD_MS;
        if let Err(e) = sample_rail(ph, bus) {
            eprintln!("[periph] 轨压周期采样失败: {e}");
        }
    }
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