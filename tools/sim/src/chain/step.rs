//! 单个演练步执行：起进程、总线观察者取证、日志留痕与状态巡检。

use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rr_bus::RR3_TASK_RRD;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType,
};
use serde_json::{json, Value};

use crate::brain::{rr3_ctl_denied_zero, rr3_spawn_logged, wait_log_contains};
use crate::drill::{rr3_section, RR3_SIM_MODULE_B, RR3_SIM_MODULE_BRAIN, RR3_SIM_PANEL_ADDR};

use super::{
    Rr3ChainCtx, Rr3ChainFix, RR3_ACT_TOPIC_SERVO_STATE, RR3_CHAIN_ASR_TEXT, RR3_CHAIN_DOA_DEG,
    RR3_CHAIN_OBS, RR3_CHAIN_READY_DEADLINE, RR3_SENSE_TOPIC_VAD, RR3_SIM_MODULE_ACT,
};

/// 观察者取证袋：判向档（deg/bin）与转向落角。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Rr3ChainObs {
    pub doa_deg: Option<i64>,
    pub doa_bin: Option<String>,
    pub servo_angle: Option<i64>,
}

impl Rr3ChainObs {
    /// 两路取证齐了（收集循环的停机条件）
    pub fn rr3_obs_ready(&self) -> bool {
        self.doa_deg.is_some() && self.servo_angle.is_some()
    }
}

/// 事件体喂进取证袋（纯函数，tests 直测）：vad_start 携 doa 取判向档
/// （offset 不带 doa，批F2 负载纪律，喂了也取不到），servo/state 取落角。
pub fn rr3_obs_feed(obs: &mut Rr3ChainObs, body: &Value) {
    match body.get("topic").and_then(Value::as_str) {
        Some(RR3_SENSE_TOPIC_VAD) => rr3_obs_doa(obs, body),
        Some(RR3_ACT_TOPIC_SERVO_STATE) => {
            obs.servo_angle = body.get("angle_deg").and_then(Value::as_i64);
        }
        _ => {}
    }
}

/// vad_start 的 doa 档入袋（offset 不带 doa，喂了也取不到）。
fn rr3_obs_doa(obs: &mut Rr3ChainObs, body: &Value) {
    if let Some(d) = body.get("doa") {
        obs.doa_deg = d.get("deg").and_then(Value::as_i64);
        obs.doa_bin = d.get("bin").and_then(Value::as_str).map(str::to_string);
    }
}

/// 裸 UDS 观察者：rr3.register 最小身份（与 e2e support 的 rr3_connect_observer
/// 同一机制）。应答 id=1 的 error 面原样带回（注册被拒要当场可辨，不等取证超时）。
fn rr3_obs_register(stream: &mut UnixStream, manifest_text: &str) -> Result<(), String> {
    // 读超时先行：注册应答不来时按 deadline 收口，不无限阻塞演练
    rr3_obs_arm(stream, Duration::from_secs(2))?;
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "rr3.register",
        "params": {"name": RR3_CHAIN_OBS, "manifest": manifest_text}
    });
    rr3_frame_write(stream, &rr3_obs_frame(Rr3PayloadType::Request, &body))
        .map_err(|e| format!("写注册帧失败: {e:?}"))?;
    rr3_obs_wait(stream)
}

/// 等注册应答：2s 一拍读，10s deadline 统一收口。
fn rr3_obs_wait(stream: &mut UnixStream) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() <= deadline {
        if rr3_obs_one(stream)? {
            return Ok(());
        }
    }
    Err("注册应答超时（10s）：rrd 没应答 rr3.register".to_string())
}

/// 一拍：读到注册成功应答回 `true`；读超时/不入账回 `false` 继续等。
fn rr3_obs_one(stream: &mut UnixStream) -> Result<bool, String> {
    match rr3_frame_read(&mut *stream) {
        Ok(frame) => rr3_obs_reply(&frame.payload).map(|hit| hit.is_some()),
        // 读超时（2s 一拍）：到 deadline 统一收口
        Err(_) => Ok(false),
    }
}

/// 单帧归类：`Ok(Some(()))` = 注册成功，`Ok(None)` = 不入账继续等；
/// `Err` = JSON 非形或注册被拒（原样带回原因）。
fn rr3_obs_reply(payload: &[u8]) -> Result<Option<()>, String> {
    let Some(v) = rr3_obs_reply_body(payload)? else {
        return Ok(None);
    };
    if v.get("error").is_some_and(Value::is_object) {
        return Err(format!("观察者注册被拒: {}", v["error"]));
    }
    Ok(Some(()))
}

/// 注册应答体：非 Response、id≠1、非 JSON 都不入账（`Ok(None)`）；非 JSON 是错。
fn rr3_obs_reply_body(payload: &[u8]) -> Result<Option<Value>, String> {
    let Ok((ty, raw)) = rr3_body_split(payload) else {
        return Ok(None);
    };
    rr3_obs_response(ty, &raw)
}

/// Response 帧体：非 Response/id≠1 不入账，非 JSON 是错。
fn rr3_obs_response(ty: Rr3PayloadType, raw: &[u8]) -> Result<Option<Value>, String> {
    if ty != Rr3PayloadType::Response {
        return Ok(None); // 注册期间到达的广播/通知不入账
    }
    let v: Value = serde_json::from_slice(raw).map_err(|e| format!("注册应答非 JSON: {e}"))?;
    Ok(rr3_obs_id_ok(&v).then_some(v))
}

/// 注册应答的 id 门（id≠1 不入账）。
fn rr3_obs_id_ok(v: &Value) -> bool {
    v.get("id").and_then(Value::as_u64) == Some(1)
}

/// 取证收集：读到两路证据齐或超时。读超时/暂无帧按"这拍没帧"继续，
/// 到 deadline 收口——缺什么在错误里点名什么。
pub(crate) fn rr3_obs_collect(
    stream: &mut UnixStream,
    deadline: Duration,
) -> Result<Rr3ChainObs, String> {
    rr3_obs_arm(stream, Duration::from_millis(250))?;
    rr3_obs_gather(stream, deadline)
}

/// 读超时设置（取证 250ms 一拍；注册应答 2s）。
fn rr3_obs_arm(stream: &mut UnixStream, timeout: Duration) -> Result<(), String> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("设读超时失败: {e}"))
}

/// 收集循环：两路证据齐或到 deadline（缺什么在错误里点名什么）。
fn rr3_obs_gather(stream: &mut UnixStream, deadline: Duration) -> Result<Rr3ChainObs, String> {
    let mut obs = Rr3ChainObs::default();
    let t0 = Instant::now();
    while !obs.rr3_obs_ready() {
        rr3_obs_deadline(t0, deadline, &obs)?;
        rr3_obs_step(stream, &mut obs);
    }
    Ok(obs)
}

/// 取证超时门（缺什么在错误里点名什么）。
fn rr3_obs_deadline(t0: Instant, deadline: Duration, obs: &Rr3ChainObs) -> Result<(), String> {
    if t0.elapsed() >= deadline {
        return Err(format!(
            "超时（{deadline:?}）：全链取证未齐（doa={:?} servo={:?}）——判向注入或意图-转向链没走通",
            obs.doa_deg, obs.servo_angle
        ));
    }
    Ok(())
}

/// 一拍取证：读超时/非事件/非 JSON 一律按"这拍没帧"跳过。
fn rr3_obs_step(stream: &mut UnixStream, obs: &mut Rr3ChainObs) {
    // 读侧 Err（读超时 250ms 一拍）不进分支：到 deadline 统一收口
    let Ok(frame) = rr3_frame_read(&mut *stream) else {
        return;
    };
    let Ok((ty, raw)) = rr3_body_split(&frame.payload) else {
        return;
    };
    if ty != Rr3PayloadType::Event {
        return;
    }
    if let Ok(v) = serde_json::from_slice::<Value>(&raw) {
        rr3_obs_feed(obs, &v);
    }
}

/// 观察者上行帧（src=0 注册前约定，dst=rrd；与 sim_peer 的本地 rr3_send 同形）
fn rr3_obs_frame(ty: Rr3PayloadType, body: &Value) -> Rr3Frame {
    let payload = rr3_body_encode(ty, body.to_string().as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task: 0,
            dst_task: RR3_TASK_RRD,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 起 rrd：boot spawn rr3-sense（双麦 burst 源 + 判向注入随 rrd 环境继承）。
pub(crate) fn rr3_chain_up_rrd(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
) -> Result<(), String> {
    let fix = ctx.fix;
    let rrd_log = fix.workdir.join("rrd.log");
    let log = Stdio::from(
        std::fs::File::create(&rrd_log).map_err(|e| format!("建 {rrd_log:?} 失败: {e}"))?,
    );
    let rrd_child = rr3_chain_rrd_cmd(ctx, log)
        .spawn()
        .map_err(|e| format!("起 rrd 失败: {e}"))?;
    children.push(("rrd".into(), rrd_child));
    println!(
        "rr3-sim --chain: rrd 已起，socket={}（sense 双麦 burst + DOA 注入 {RR3_CHAIN_DOA_DEG}°）",
        fix.socket.display()
    );
    Ok(())
}

/// rrd 命令面（boot spawn rr3-sense；双麦 burst 与判向注入随 rrd 环境继承）。
fn rr3_chain_rrd_cmd(ctx: &Rr3ChainCtx, log: Stdio) -> Command {
    let fix = ctx.fix;
    let mut cmd = Command::new(&ctx.bins.rrd);
    cmd.arg("--socket")
        .arg(&fix.socket)
        .arg("--manifest")
        .arg(&fix.m_sense)
        .arg("--boot-role")
        .arg("resident")
        .args(["--pre-register-timeout-ms", "3000"])
        .env("RR3_HAL_BACKEND", "sim")
        .env("RR3_HAL_SIM_WAVE", "burst")
        .env("RR3_HAL_SIM_DOA_DEG", RR3_CHAIN_DOA_DEG.to_string())
        .env("RR3_SENSE_CONFIG", &fix.sense_cfg)
        .stdout(log)
        .stderr(Stdio::inherit());
    cmd
}

/// 观察者连总线并注册（最小身份，专收两个取证主题）。
pub(crate) fn rr3_chain_obs_up(
    fix: &Rr3ChainFix,
    report: &mut String,
) -> Result<UnixStream, String> {
    let mut obs_stream = UnixStream::connect(&fix.socket)
        .map_err(|e| format!("观察者连 {:?} 失败: {e}", fix.socket))?;
    let obs_manifest_text = std::fs::read_to_string(&fix.obs_manifest)
        .map_err(|e| format!("读 {:?} 失败: {e}", fix.obs_manifest))?;
    rr3_obs_register(&mut obs_stream, &obs_manifest_text)?;
    report.push_str(&format!(
        "总线观察者已注册（{RR3_CHAIN_OBS}，订阅 {RR3_SENSE_TOPIC_VAD} / {RR3_ACT_TOPIC_SERVO_STATE}）\n"
    ));
    Ok(obs_stream)
}

/// 外部起 interact / brain / act（三真模块，各自日志进 workdir，复用共用起法）。
pub(crate) fn rr3_chain_up_ext(
    ctx: &Rr3ChainCtx,
    children: &mut Vec<(String, Child)>,
) -> Result<(), String> {
    let fix = ctx.fix;
    for (name, mut cmd) in rr3_chain_ext_cmds(ctx) {
        let log = fix.workdir.join(format!("{name}.log"));
        cmd.env("RR3_BUS_PATH", &fix.socket)
            .env("RR3_MANIFEST", fix.workdir.join(format!("{name}.toml")));
        let child = rr3_spawn_logged(cmd, &log, name)?;
        children.push((name.to_string(), child));
    }
    Ok(())
}

/// 三外部真模块的命令面（interact / brain / act，各自的环境接线）。
fn rr3_chain_ext_cmds(ctx: &Rr3ChainCtx) -> [(&'static str, Command); 3] {
    let fix = ctx.fix;
    [
        {
            let mut c = Command::new(&ctx.bins.interact);
            c.env("RR3_HAL_BACKEND", "sim")
                .env("RR3_PANEL_ADDR", RR3_SIM_PANEL_ADDR)
                .env("RR3_INTERACT_CONFIG", &fix.interact_cfg);
            (RR3_SIM_MODULE_B, c)
        },
        {
            let mut c = Command::new(&ctx.bins.brain);
            c.env("RR3_BRAIN_CLOUD", "mock")
                .env("RR3_BRAIN_MOCK_ASR_TEXT", RR3_CHAIN_ASR_TEXT)
                .env("RR3_BRAIN_CONFIG", &fix.brain_cfg);
            (RR3_SIM_MODULE_BRAIN, c)
        },
        {
            let mut c = Command::new(&ctx.bins.act);
            c.env("RR3_HAL_BACKEND", "sim")
                .env("RR3_ACT_CONFIG", &fix.act_cfg)
                // 执行轴状态（批N3 件三建、件五五键）：chain 直起路径与
                // rrd --manifest 并存，模块 env 由 chain 直设——sim 档已
                // 标定 + 挡块在册，行程/中位同档案样本值，Auto 放行
                .env(
                    "RR3_ACT_AXIS_STATE",
                    "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
                );
            (RR3_SIM_MODULE_ACT, c)
        },
    ]
}

/// act 落角行（执行动作真落 servo/state 的留痕）。
pub(crate) fn rr3_chain_act(ctx: &Rr3ChainCtx) -> Result<String, String> {
    let act_log = ctx.fix.workdir.join(format!("{RR3_SIM_MODULE_ACT}.log"));
    wait_log_contains(&act_log, "servo angle=", RR3_CHAIN_READY_DEADLINE)
}

/// brain 轮次行：动作数必须是 1（词表左档应产一条 servo 动作）。
pub(crate) fn rr3_chain_turn(ctx: &Rr3ChainCtx) -> Result<String, String> {
    let brain_log = ctx.fix.workdir.join(format!("{RR3_SIM_MODULE_BRAIN}.log"));
    let turn = wait_log_contains(
        &brain_log,
        crate::brain::RR3_BRAIN_TURN_MARK,
        RR3_CHAIN_READY_DEADLINE,
    )?;
    rr3_chain_acts(&turn)?;
    Ok(turn)
}

/// 轮次行动作数门：词表左档应产一条 servo 动作。
fn rr3_chain_acts(turn: &str) -> Result<(), String> {
    if !turn.contains("actions=1") {
        return Err(format!(
            "brain 轮次行的动作数不是 1（词表左档应产一条 servo 动作）: {turn}"
        ));
    }
    Ok(())
}

/// interact 播放行：必须携「talk 挂起」（表情机没接管即红）。
pub(crate) fn rr3_chain_tts(ctx: &Rr3ChainCtx) -> Result<String, String> {
    let interact_log = ctx.fix.workdir.join(format!("{RR3_SIM_MODULE_B}.log"));
    let tts = wait_log_contains(&interact_log, "tts:", RR3_CHAIN_READY_DEADLINE)?;
    if !tts.contains("talk 挂起") {
        return Err(format!(
            "interact 播放行没携 talk 挂起（表情机没接管）: {tts}"
        ));
    }
    Ok(tts)
}

/// `rr3-ctl status` 巡检：denied_publish = 0 是"事件真被接受"的线上证据。
pub(crate) fn rr3_chain_status(ctx: &Rr3ChainCtx, report: &mut String) -> Result<(), String> {
    let status_out = rr3_ctl_denied_zero(
        &ctx.bins.rrctl,
        &ctx.fix.socket,
        "denied_publish ≠ 0：链上发布被 rrd 拒了",
    )?;
    report.push_str(rr3_section("rr3-ctl status 巡检").as_str());
    report.push_str(&status_out);
    Ok(())
}
