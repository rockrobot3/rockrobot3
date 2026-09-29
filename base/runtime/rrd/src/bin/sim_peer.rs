//! sim-peer —— 基座测试夹具二进制（spec §6 / R5）。
//!
//! 它是**第二个真进程**：走 rrd 的线上入口完成注册→心跳→发布→退场，用来证明协议层不是
//! "测试自己跟自己说话"。不进 `modules/`、不进 `contracts/topics.toml`。
//!
//! 为什么是 `rrd` 包里的一个 `[[bin]]` 而不是独立 workspace 成员：测试要用
//! `env!("CARGO_BIN_EXE_sim_peer")` 拿到它的绝对路径（manifest 的 entry 门禁要绝对路径，
//! 而构建产物路径随机器变），cargo 只为同一个包的 bin 设置那个变量。
//! 平台无关的部分放在 `rrd` 的 lib 里（`rrd::manifest`），bin 才能直接复用。

fn main() {
    #[cfg(unix)]
    if let Err(e) = peer::rr3_peer_main() {
        println!("sim-peer: 失败: {e}");
        std::process::exit(1);
    }
    #[cfg(not(unix))]
    {
        println!("sim-peer: 本平台不支持 UDS（需 Unix）");
        std::process::exit(2);
    }
}

/// 整个夹具只在 Unix 有意义（要连 UDS）。非 Unix 下这块不参与编译，
/// 所以 Windows 的 `cargo check --all-targets` 绿只证明 cfg 边界没破，不证明它能跑。
#[cfg(unix)]
mod peer {
    use std::env;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use rr_bus::RR3_TASK_RRD;
    use rr_ipc::{
        opcode, rr3_body_encode, rr3_body_split, rr3_frame_read, rr3_frame_write, Rr3Frame,
        Rr3Header, Rr3PayloadType,
    };
    use rrd::manifest::rr3_manifest_facts_from_text;
    use serde_json::{json, Value};

    /// 一次"挑自己的应答"最多扫多少帧。与集成测试夹具同口径，理由也相同：
    /// 应答由 conn 线程写、state 广播由这条连接的 writer 线程写，**整帧原子但相对次序不保证**，
    /// 所以只能按 id 挑、不能读第 N 帧。扫不到就是 rrd 真没发，不是预算不够。
    const REPLY_SCAN_BUDGET: usize = 64;

    /// 排空接收队列（`rr3_drain_incoming`）时单次读的最长等待。
    /// 它不是"过了多久"的测量，只用来回答"还有没有存货"：读到空即返回，所以这个数放大
    /// 只会让 abort 支多等一会儿，不影响任何判据（本轮用例都不测时长）。
    const DRAIN_READ_TIMEOUT: Duration = Duration::from_millis(150);

    /// `silent_then_beat` 的先静默时长：必须**显著大于**到期上界
    /// （`watchdog_ms 500 + tick 20` = 520ms）。取 700ms 留 180ms 余量，而真正让判据
    /// 不靠运气的是这条算式：本支在第 700/850/1000/1150/1300ms 各喂一次，
    /// **只有最后一次的结果进判据**（见 W4），到那一刻已静默 ≥780ms，
    /// watchdog 只要跑过任何一拍就必然已判死。
    const SILENT_BEFORE_REVIVE: Duration = Duration::from_millis(700);
    /// 复活尝试的喂狗间隔
    const REVIVE_BEAT_GAP_MS: u64 = 150;

    /// 身份只从 manifest 来；role 只选行为。见 support 的约定 1。
    pub fn rr3_peer_main() -> Result<(), String> {
        let bus = env::var("RR3_BUS_PATH").map_err(|_| "缺 RR3_BUS_PATH".to_string())?;
        let manifest_path = env::var("RR3_MANIFEST").map_err(|_| "缺 RR3_MANIFEST".to_string())?;
        let role = env::var("RR3_SIM_PEER_ROLE").unwrap_or_else(|_| "resident".to_string());

        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("读 {manifest_path} 失败: {e}"))?;
        let facts = rr3_manifest_facts_from_text(&text).map_err(|errs| errs.join("; "))?;

        let mut stream = UnixStream::connect(&bus).map_err(|e| format!("连 {bus} 失败: {e}"))?;
        // 有超时：卡住要成为"夹具退出并打一行原因"，而不是把整个 cargo test 挂住
        stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;

        // 注册前还没有 task，按契约填 0（"用户态不得自设"的另一种写法，与夹具同形）
        rr3_send(
            &mut stream,
            Rr3PayloadType::Request,
            0,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "rr3.register",
                    "params": {"name": facts.name, "manifest": text}}),
        )?;
        let reply = rr3_wait_reply(&mut stream, 1)?;
        if reply["error"].is_object() {
            return Err(format!("注册被拒: {}", &reply["error"]));
        }
        let task = reply["result"]["src_task"].as_u64().ok_or("注册应答里没有 result.src_task")?;
        println!("sim-peer: 注册成功 name={} task={task}", facts.name);

        match role.as_str() {
            "resident" => rr3_resident(&mut stream, task, facts.heartbeat_ms),
            "publish_once" => {
                // 主题名从自己的 manifest 取（约定 2），不硬编码
                let topic = rr3_first_publish(&text).ok_or("publish_once 需要 publishes 非空")?;
                rr3_send(&mut stream, Rr3PayloadType::Event, task, &json!({"topic": topic, "seq": 1}))?;
                println!("sim-peer: 已发布 {topic}");
                rr3_beat(&mut stream, task, 10)
            }
            "exit_clean" => {
                rr3_send(
                    &mut stream,
                    Rr3PayloadType::Request,
                    task,
                    &json!({"jsonrpc": "2.0", "id": 2, "method": "rr3.unregister", "params": {}}),
                )?;
                let r = rr3_wait_reply(&mut stream, 2)?;
                if r["error"].is_object() {
                    return Err(format!("退场被拒: {}", &r["error"]));
                }
                println!("sim-peer: 已主动退场");
                Ok(()) // 返回即 drop 掉 stream
            }
            "abort" => {
                rr3_beat(&mut stream, task, 10)?;
                // 先排空接收队列再打死。为什么这属于夹具而不是被测行为：队列里留着未读的
                // 广播帧时，内核在进程死亡那一刻把关闭改成 RST，rrd 读到的就是 ECONNRESET
                // 而不是 EOF → 收尾走 `Err` 分支（phase `failed`）；队列空了才是 FIN → EOF →
                // `Rr3FrameError::Closed`（phase `unregistered`）。实测未排空时本用例 10 次红 1 次。
                // 这一维量的是"进程有没有把 socket 读干净"，不是"有没有体面退场"，
                // 所以本任务把它当混淆项排掉；FIN/RST 双路（连同退出码）归 Task 11 的崩溃检测。
                rr3_drain_incoming(&mut stream)?;
                println!("sim-peer: 即将 abort");
                // 不 close、不 unregister：让 rrd 只看到一次 EOF。`abort` 本身就跳过析构，
                // 这里显式 forget 是为了让"socket 有没有被优雅关闭"不靠编译器心情。
                std::mem::forget(stream);
                std::process::abort();
            }
            // 注册 + 一颗心跳后**彻底静默**（不再喂狗，也不再读帧）。
            // 这正是 watchdog 要抓的形状：连接还在、fd 没关、只是不喂狗了。
            // 里程碑只打一行（约定 3：只打里程碑，不打循环）。
            "go_silent" => {
                rr3_beat(&mut stream, task, 10)?;
                println!("sim-peer: 静默中（已喂 1 次）");
                loop {
                    std::thread::sleep(Duration::from_secs(3600));
                }
            }
            // 静默到必然已过期，再回头连续喂 5 次，并把每一次的接受/被拒打出来。
            // 与 `rr3_beat` 的区别：那个把被拒当致命错误直接退出（继续喂一条已不在册的
            // 连接没有观察价值）；本支要观察的恰是"被拒之后是什么样"，所以必须用
            // `rr3_beat_verdict`。
            "silent_then_beat" => {
                rr3_beat(&mut stream, task, 10)?;
                println!("sim-peer: 静默中（已喂 1 次）");
                std::thread::sleep(SILENT_BEFORE_REVIVE);
                for n in 1..=5u64 {
                    let accepted = rr3_beat_verdict(&mut stream, task, 20 + n)?;
                    println!(
                        "sim-peer: 喂狗 #{n} {}",
                        if accepted { "接受" } else { "被拒" }
                    );
                    std::thread::sleep(Duration::from_millis(REVIVE_BEAT_GAP_MS));
                }
                println!("sim-peer: 已试喂 5 次");
                loop {
                    std::thread::sleep(Duration::from_secs(3600));
                }
            }
            other => Err(format!("未知 RR3_SIM_PEER_ROLE: {other}")),
        }
    }

    /// 常驻：按 manifest 的 heartbeat_ms 一直喂狗，直到被杀。
    /// 里程碑只打一行（第 3 次），见约定 3。
    fn rr3_resident(stream: &mut UnixStream, task: u64, heartbeat_ms: u64) -> Result<(), String> {
        let mut id = 10u64;
        for beat in 1..=3u32 {
            rr3_beat(stream, task, id)?;
            if beat == 3 {
                println!("sim-peer: 连喂 {beat} 次均被接受");
            }
            id += 1;
            std::thread::sleep(Duration::from_millis(heartbeat_ms));
        }
        loop {
            rr3_beat(stream, task, id)?;
            id += 1;
            std::thread::sleep(Duration::from_millis(heartbeat_ms));
        }
    }

    /// 把接收队列读到"读不出完整一帧"为止（超时 / EOF / 坏帧都算排完）。
    /// 只在 abort 支用，理由见那里。
    ///
    /// 不设帧数上限：上限是一条会随任务推进失效的假设——本轮这条连接上最多攒 4 帧，
    /// 但 Task 10 的 watchdog 广播与 Task 11 的重启扰动都会把它推高，排不完就留未读、
    /// 未读就把 FIN 变成 RST，用例于是从"判据"退化成"运气"。终止性由 `DRAIN_READ_TIMEOUT`
    /// 保证：读到空即返回，最多比存货多等一次超时。
    fn rr3_drain_incoming(stream: &mut UnixStream) -> Result<(), String> {
        stream
            .set_read_timeout(Some(DRAIN_READ_TIMEOUT))
            .map_err(|e| format!("设排空读超时失败: {e}"))?;
        loop {
            if rr3_frame_read(&mut *stream).is_err() {
                return Ok(());
            }
        }
    }

    fn rr3_beat(stream: &mut UnixStream, task: u64, id: u64) -> Result<(), String> {
        rr3_send(
            stream,
            Rr3PayloadType::Request,
            task,
            &json!({"jsonrpc": "2.0", "id": id, "method": "rr3.heartbeat", "params": {}}),
        )?;
        let reply = rr3_wait_reply(stream, id)?;
        if reply["error"].is_object() {
            // 被拒即致命：继续喂一条已经不在册的连接没有任何观察价值
            return Err(format!("喂狗被拒: {}", &reply["error"]));
        }
        Ok(())
    }

    /// 喂一次狗，返回"被接受了吗"——**不**把被拒当致命错误。
    /// 与 `rr3_beat` 的分工是刻意的：那条的"被拒即退出"服务于"常驻模块不该继续用一条
    /// 已不在册的连接"，这条服务于"要看被拒之后的形状"。合成一条会让两边都失去自己的语义。
    fn rr3_beat_verdict(stream: &mut UnixStream, task: u64, id: u64) -> Result<bool, String> {
        rr3_send(
            stream,
            Rr3PayloadType::Request,
            task,
            &json!({"jsonrpc": "2.0", "id": id, "method": "rr3.heartbeat", "params": {}}),
        )?;
        let reply = rr3_wait_reply(stream, id)?;
        Ok(!reply["error"].is_object())
    }

    /// 写一帧：payload 先编（2 字节类型前缀），`payload_len` 取**编出来之后**的字节数
    fn rr3_send(stream: &mut UnixStream, ty: Rr3PayloadType, src: u64, body: &Value) -> Result<(), String> {
        let payload = rr3_body_encode(ty, body.to_string().as_bytes());
        let frame = Rr3Frame {
            header: Rr3Header {
                opcode: opcode::SEND,
                flags: 0,
                trace_id: 0,
                timestamp_ns: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0),
                src_task: src,
                dst_task: RR3_TASK_RRD,
                payload_len: payload.len() as u32,
            },
            payload,
        };
        rr3_frame_write(stream, &frame).map_err(|e| format!("写帧失败: {e:?}"))
    }

    /// 按 id 挑自己的应答，跳过途中遇到的广播帧（见 `REPLY_SCAN_BUDGET` 的注释）
    fn rr3_wait_reply(stream: &mut UnixStream, id: u64) -> Result<Value, String> {
        for _ in 0..REPLY_SCAN_BUDGET {
            // `rr3_frame_read` 收 `impl Read`（按值），所以这里必须逐次 reborrow，
            // 否则第一帧就把 `stream` 移走、循环第二轮编译不过。
            let frame = rr3_frame_read(&mut *stream).map_err(|e| format!("读帧失败: {e:?}"))?;
            let (ty, body) =
                rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
            if ty != Rr3PayloadType::Response {
                continue; // 下行广播（Notification）不是我要的
            }
            let v: Value = serde_json::from_slice(&body).map_err(|e| format!("应答体不是 JSON: {e}"))?;
            if v["id"].as_u64() == Some(id) {
                return Ok(v);
            }
        }
        Err(format!("{REPLY_SCAN_BUDGET} 帧内没等到 id={id} 的应答"))
    }

    /// `events.publishes[0]`（约定 2）。取不到返回 None，由调用方决定是不是错。
    fn rr3_first_publish(text: &str) -> Option<String> {
        let root: toml::Value = text.parse().ok()?;
        root.get("events")?
            .get("publishes")?
            .as_array()?
            .first()?
            .as_str()
            .map(str::to_string)
    }
}
