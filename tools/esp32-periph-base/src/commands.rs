use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use esp_idf_sys::{esp_get_free_heap_size, esp_get_minimum_free_heap_size};

use crate::board::{OLED_COLS, OLED_ROWS, RR3_FACE_EXPRS, RR3_MAGIC_FIRST};
use crate::face::expr_eye_base;
use crate::frame::{frame_dispatch, frame_recv, frame_send_response};
use crate::link::{read_line, reply, UartIo};
use crate::oled::glyph;
use crate::periph::CtlUart;
use crate::ports::Rr3BoardClock;
use crate::servo::{axis_check, Axis};
use crate::state::{
    drain_audio, drain_outbox, esp_random_u32, lock_bus, push_job, Bus, CtlState, Job,
};

// ---------------------------------------------------------------------------
// 指令循环（批B 行面 + 批C 帧面双面并存：行面保留调试与舵机五点 SCAN 判据）
// ---------------------------------------------------------------------------

/// 帧面一轮（控制面双面分流的 0x31 分支）：收帧 + 协议门（失败丢弃不回）→ 分发
/// （校验/改状态/推作业）→ Response 回帧。UartIo 只在收/发两段活着；执行段无硬件
/// 借用（作业已推给运行面），控制面保持轻。
fn frame_round(io: &mut CtlUart, bus: &Arc<Mutex<Bus>>, ctl: &mut CtlState, first: u8) {
    let req = {
        let mut uio = UartIo {
            uart: &mut io.uart,
            pre: Some(first),
        };
        match frame_recv(&mut uio) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[periph] 帧面丢弃: {e}");
                return;
            }
        }
    };
    println!("[periph] 帧面 id={} topic=\"{}\"", req.id, req.topic);
    let result = frame_dispatch(bus, ctl, &req);
    if let Err(e) = &result {
        eprintln!("[periph] 帧面业务失败 id={}: {e}", req.id);
    }
    let mut uio = UartIo {
        uart: &mut io.uart,
        pre: None,
    };
    frame_send_response(&mut uio, &req, result, &Rr3BoardClock);
}

/// 控制面主循环（**独占 UART2，不碰 I2C/LEDC**）：逐字节窥视首字节——0x31（帧
/// magic LE 首字节）走帧面 round，其余作行首字节继续收行走批B 行面。行面的硬件
/// 动作一律折成作业推给运行面；本循环只做解析/校验/改状态/回包，故慢外设不阻塞
/// 命令吞吐。未知指令 ERR（fail-closed，不猜不拖）。
pub(crate) fn cmd_loop(mut io: CtlUart, bus: Arc<Mutex<Bus>>) {
    let mut ctl = CtlState {
        prev_expr: String::from("none"),
    };
    reply(
        &mut io.uart,
        "READY periph-base v0.4.0 ctrl/runtime split + serial-servo + baud921600 (type HELP for commands)",
    );
    loop {
        // 先清 outbox（运行面异步回包：SCAN 逐点等）+ audio_q（批D③ 音频分片整帧
        // 上行，同线程串行写与行面字节互不交错），再窥视下一字节
        drain_outbox(&mut io.uart, &bus);
        drain_audio(&mut io.uart, &bus);
        let mut first = [0u8; 1];
        // 窥视带 1-tick（10ms）超时：空口返回 Ok(0) 即回到轮首（转手清 outbox +
        // 让出 CPU，自主行为在运行面跑，不再占用控制面）；有字节（Ok(n≥1)）照旧
        // 双面分流（帧/行路径的后续读仍 portMAX_DELAY 阻塞）。Err 才是真读失败。
        match io.uart.read(&mut first, 1) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(_) => {
                eprintln!("[periph] UART 读失败，退避后重试");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        }
        if first[0] == RR3_MAGIC_FIRST {
            frame_round(&mut io, &bus, &mut ctl, first[0]);
            continue;
        }
        let line = match read_line(&mut io.uart, first[0]) {
            Ok(l) => l,
            Err(why) => {
                eprintln!("[periph] 收行失败: {why}");
                continue;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let head = parts.next().unwrap_or("");
        let result: Result<String, String> = match head {
            "HELP" => Ok(
                "CMDS: OLED <text> | FACE <idle|blink|talk|think|happy|listen|turn> | PAN <deg> | TILT <deg> | SERVO <pan> <tilt> | SCAN | WANDER [STOP] | RAIL | AUDIO ON|OFF | INFO | HELP  (行程 pan 60-120 / tilt 80-100，中位 90；SERVO/SCAN 串行先左右后俯仰；RAIL 采一次 5V 轨压；AUDIO ON 起双麦 16k 双声道分片上行@921600，宿主 rr3-audio-rec 收流落 wav；开机默认表情 idle + 姿态 P060 T098)"
                    .into(),
            ),
            "OLED" => {
                // 参数取原始行去指令头，保留内嵌空格（≤63 字符 = 3 行 × 21 列，row3 角度行）
                let rest = line[head.len()..].trim_start();
                if rest.is_empty() {
                    Err("OLED 缺文本参数".into())
                } else if rest.len() > OLED_COLS * (OLED_ROWS - 1) {
                    Err(format!(
                        "OLED 文本超长（≤{} 字符 = 3 行 × 21 列，第 4 行为角度行）",
                        OLED_COLS * (OLED_ROWS - 1)
                    ))
                } else {
                    // 控制面记账 + 排队，运行面出字（慢 I2C 不在控制面）
                    {
                        let mut b = lock_bus(&bus);
                        b.st.last_text = rest.to_string();
                        b.st.frame_mode = false;
                        b.st.expr_mode = false;
                    }
                    push_job(&bus, Job::RedrawOled);
                    Ok(format!("OK oled \"{rest}\""))
                }
            }
            "PAN" => parse_deg(parts.next()).and_then(|deg| {
                let d = axis_check(Axis::Pan, deg)?;
                {
                    let mut b = lock_bus(&bus);
                    b.st.wander_on = false; // 外部点名优先，自主扫摆让位
                    b.st.pan_deg = d;
                }
                push_job(
                    &bus,
                    Job::Move {
                        pan: Some(d),
                        tilt: None,
                    },
                );
                push_job(&bus, Job::RedrawOled);
                Ok(format!("OK pan={d}"))
            }),
            "TILT" => parse_deg(parts.next()).and_then(|deg| {
                let d = axis_check(Axis::Tilt, deg)?;
                {
                    let mut b = lock_bus(&bus);
                    b.st.wander_on = false;
                    b.st.tilt_deg = d;
                }
                push_job(
                    &bus,
                    Job::Move {
                        pan: None,
                        tilt: Some(d),
                    },
                );
                push_job(&bus, Job::RedrawOled);
                Ok(format!("OK tilt={d}"))
            }),
            "SERVO" => match (parse_deg(parts.next()), parse_deg(parts.next())) {
                (Ok(p), Ok(t)) => match (axis_check(Axis::Pan, p), axis_check(Axis::Tilt, t)) {
                    (Ok(p), Ok(t)) => {
                        {
                            let mut b = lock_bus(&bus);
                            b.st.wander_on = false;
                            b.st.pan_deg = p;
                            b.st.tilt_deg = t;
                        }
                        // 串行降级：运行面先 pan 到位 → 整定 → 再 tilt（削峰值电流）
                        push_job(
                            &bus,
                            Job::Move {
                                pan: Some(p),
                                tilt: Some(t),
                            },
                        );
                        push_job(&bus, Job::RedrawOled);
                        Ok(format!("OK pan={p} tilt={t}"))
                    }
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
                _ => Err("SERVO 需两个 0-180 角度参数".into()),
            },
            // 表情（行面）：控制面改状态 + 推 Job::RenderCapsules（慢 I2C 在运行面），
            // 与帧面 expression 同一套基态预设/微动作（expr_eye_base + anim_reset）。
            // 补此命令的原因：表情原只走帧面（需 UART2 帧链路），现场该链路劣化时
            // 表情无路可触发——行面直命令是可达性兜底。
            "FACE" => match parts.next() {
                Some(expr) if RR3_FACE_EXPRS.contains(&expr) => {
                    let (open, dx, dy) = expr_eye_base(expr);
                    {
                        let mut b = lock_bus(&bus);
                        b.st.expr_mode = true;
                        b.st.frame_mode = false;
                        b.st.base_open = open;
                        b.st.base_dx = dx;
                        b.st.base_dy = dy;
                        b.st.last_text = format!("EXPR {expr}");
                        b.st.anim_reset();
                        b.st.drawn = (open, dx, dy);
                    }
                    push_job(&bus, Job::RenderCapsules);
                    Ok(format!("OK face={expr}"))
                }
                Some(other) => Err(format!(
                    "expression 不在白名单: \"{other}\"（可选: {}）",
                    RR3_FACE_EXPRS.join("/")
                )),
                None => Err(format!(
                    "FACE 需表情参数（可选: {}）",
                    RR3_FACE_EXPRS.join("/")
                )),
            },
            "WANDER" => match parts.next() {
                Some("STOP") => {
                    lock_bus(&bus).st.wander_on = false;
                    let (p, t) = {
                        let b = lock_bus(&bus);
                        (b.st.pan_deg, b.st.tilt_deg)
                    };
                    Ok(format!("OK wander off pan={p} tilt={t}"))
                }
                None => {
                    {
                        let mut b = lock_bus(&bus);
                        b.st.wander_on = true;
                        // 两轴方向独立随机起步，立即起摆
                        b.st.wander_pan_dir = if esp_random_u32() % 2 == 0 { 1 } else { -1 };
                        b.st.wander_tilt_dir = if esp_random_u32() % 2 == 0 { 1 } else { -1 };
                        b.st.next_wander_ms = 0;
                    }
                    let (p, t) = {
                        let b = lock_bus(&bus);
                        (b.st.pan_deg, b.st.tilt_deg)
                    };
                    Ok(format!("OK wander on pan={p} tilt={t}"))
                }
                Some(other) => Err(format!("WANDER 参数非法: {other}（无参=开，STOP=停）")),
            },
            "SCAN" => {
                // 判据②（**串行**）：运行面按本轴行程五等分点逐点扫描（pan
                // 70/80/90/100/110、tilt 80/85/90/95/100），每点先 pan 到位再 tilt
                // 到位，逐点 outbox 回包。控制面只置位 + 排队并即刻 ack——不被 ~6s
                // 的扫描阻塞（旧版在命令线程里跑，是"卡顿"来源之一）。
                lock_bus(&bus).st.wander_on = false; // 判据测试不被自主扫摆扰动
                push_job(&bus, Job::Scan);
                Ok("OK scan queued (串行 pan→tilt，逐点回包)".into())
            }
            // 5V 轨单次采样（方案A Q1 观测面）：控制面不碰 ADC（ADC 在运行面），推作业
            // 由运行面采一次、经 outbox 异步回包（读一次打轨压 mV 与引脚 raw——真板
            // 分压测点接线后与万用表对拍标定用）。
            "RAIL" => {
                push_job(&bus, Job::SampleRail);
                Ok("OK rail sample queued".into())
            }
            // 音频上行开关（批D③）：ON 后 mic 线攒满分片（2048 帧 = 128ms）即组
            // Event 帧推 audio_q，控制面每轮 drain_audio 整帧写 UART2。OFF 后队列
            // 余片仍照常发完（drain_audio 不看开关），mic 线不再攒新片。
            "AUDIO" => match parts.next() {
                Some("ON") => {
                    lock_bus(&bus).st.audio_on = true;
                    Ok("OK audio on".into())
                }
                Some("OFF") => {
                    lock_bus(&bus).st.audio_on = false;
                    Ok("OK audio off".into())
                }
                _ => Err("AUDIO 需 ON|OFF 参数".into()),
            },
            "INFO" => {
                let (plo, phi) = Axis::Pan.range();
                let (tlo, thi) = Axis::Tilt.range();
                let (
                    pan,
                    tilt,
                    wander,
                    phase,
                    last_text,
                    rail_mv,
                    rail_raw,
                    rail_tier,
                    rail_low,
                    selfheal_skip,
                    audio_on,
                    audio_seq,
                    audio_sent,
                    audio_dropped,
                    audio_q,
                ) = {
                    let b = lock_bus(&bus);
                    (
                        b.st.pan_deg,
                        b.st.tilt_deg,
                        b.st.wander_on,
                        b.st.wander_phase,
                        b.st.last_text.clone(),
                        b.st.rail_mv,
                        b.st.rail_raw,
                        b.st.rail_tier,
                        b.st.rail_low_events,
                        b.st.selfheal_skipped,
                        b.st.audio_on,
                        b.st.audio_seq,
                        b.st.audio_sent,
                        b.st.audio_dropped,
                        b.audio_q.len(),
                    )
                };
                Ok(format!(
                    "INFO pan={pan} tilt={tilt} lim=pan[{plo},{phi}]/tilt[{tlo},{thi}] wander={} phase={phase} rail={rail_mv}mV raw={rail_raw} rail_tier={} low_events={rail_low} selfheal_skip={selfheal_skip} audio={} seq={audio_seq} sent={audio_sent} drop={audio_dropped} q={audio_q} oled_last=\"{last_text}\" free={}B min_free={}B",
                    if wander { "on" } else { "off" },
                    rail_tier.name(),
                    if audio_on { "on" } else { "off" },
                    unsafe { esp_get_free_heap_size() },
                    unsafe { esp_get_minimum_free_heap_size() }
                ))
            }
            "FONT" => {
                // 字模自检护栏：打印 "ABC123" 的 5x7 字模 hex，供人工比对
                let hex: Vec<String> = "ABC123"
                    .bytes()
                    .map(|c| {
                        glyph(c)
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<String>()
                    })
                    .collect();
                Ok(format!("FONT ABC123 = {}", hex.join(",")))
            }
            other => Err(format!("unknown cmd \"{other}\"（HELP 看指令表）")),
        };
        match result {
            Ok(line) => reply(&mut io.uart, &line),
            Err(e) => reply(&mut io.uart, &format!("ERR {e}")),
        }
    }
}

/// 角度参数解析（0–180 之外一律 Err——调用方不再 clamp，fail-closed）。
fn parse_deg(s: Option<&str>) -> Result<u32, String> {
    let Some(s) = s else {
        return Err("缺角度参数".into());
    };
    let deg: u32 = s.parse().map_err(|_| format!("角度参数非法: \"{s}\""))?;
    if deg > 180 {
        return Err(format!("角度超范围 0-180: {deg}"));
    }
    Ok(deg)
}
