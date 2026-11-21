//! `tail` 子命令：订阅一个主题，把 Event 以 JSONL 逐条写 stdout（计划册 §15
//! 第 2 项；DoA 标定与现场排障共用）。
//!
//! 与其余八个子命令不同，tail **必须注册**：rrd 只向在册连接广播 Event
//! （`rrd/src/conn/publish.rs` 的 `rr3_reg_fanout`），一次性控制连接收不到任何
//! 业务帧。因此本面直接用模块侧标准客户端 `Rr3BusClient`——连接 / 注册 / 心跳 /
//! 收帧全走同一机制单点，本文件只负责参数、过滤与打印。
//!
//! 身份固定为 `rr3-tail`（`manifest.name` 规则 `^rr3-[a-z]+$` 不含数字，同机
//! 同一时刻只允许一个 tail；重复注册由 rrd 回 Busy 并原样点名）。订阅面声明
//! `events.subscribes = [topic]`，topic 必须已在 `contracts/topics.toml` 登记——
//! 拼错在注册门被拒，不会静默地永远看不到事件。
//!
//! 输出形状（stdout，一行一条）：
//! `{"ts_ms":<转发时刻>,"src_task":<发布者>,"body":{...}}`。body 是线上原样
//! 事件体（含 `topic`），`jq '.body.doa.deg'` 即取 DoA 标定读数。诊断与退出
//! 摘要走 stderr，stdout 保持可管道。
//!
//! 退出码：收满 `--count` 为 0；超时 / 断链 / 注册失败为 1（main 统一渲染）。

#[cfg(unix)]
pub use imp::rr3_tail_run;

#[cfg(not(unix))]
pub fn rr3_tail_run(_bus: &str, _rest: &[String]) -> Result<String, String> {
    Err("tail 依赖 UDS 总线，仅在 Unix 可用".to_string())
}

#[cfg(unix)]
mod imp {
    //! Unix 实现：参数 → 注册 → 过滤收帧 → JSONL。

    use std::time::{Duration, Instant};

    use rr_bus_api::Rr3BusClient;
    use rr_ipc::{Rr3Frame, Rr3PayloadType};
    use serde_json::{json, Value};

    /// 观察者在册名（规则 `^rr3-[a-z]+$`：不能带数字，故同机同一时刻只允许一个
    /// tail；第二个实例由 rrd 回 Busy）。
    const RR3_TAIL_NAME: &str = "rr3-tail";

    /// 缺省收满条数与缺省超时（两界都可被 `--count` / `--secs` 覆盖）。
    const RR3_TAIL_COUNT: usize = 20;
    const RR3_TAIL_SECS: u64 = 30;

    /// 收帧切片：定期回循环顶检查总超时与连接死活，不把等待压在一个长
    /// `recv_timeout` 里（rrd 崩溃时最多 200ms 就能报出来）。
    const RR3_TAIL_SLICE: Duration = Duration::from_millis(200);

    /// 主题长度上界（契约主题远短于此，只为拦截明显异常的输入）。
    const RR3_TAIL_TOPIC_MAX: usize = 128;

    /// 一次 tail 的解析结果。
    struct Rr3TailOpts {
        topic: String,
        count: usize,
        timeout: Duration,
    }

    pub fn rr3_tail_run(bus: &str, rest: &[String]) -> Result<String, String> {
        let opts = rr3_tail_opts(rest)?;
        let client = Rr3BusClient::rr3_connect_at(bus, &rr3_tail_manifest(&opts.topic))
            .map_err(|e| format!("tail 注册失败: {e}"))?;
        eprintln!(
            "tail: {} task={} 订阅 {}（{}/{}s，ctrl-c 退出）",
            client.rr3_name(),
            client.rr3_task(),
            opts.topic,
            opts.count,
            opts.timeout.as_secs()
        );
        rr3_tail_collect(&client, &opts)?;
        Ok(String::new())
    }

    /// 余参解析：`<topic> [--count N] [--secs S]`（开关次序任意）。
    fn rr3_tail_opts(rest: &[String]) -> Result<Rr3TailOpts, String> {
        let mut topic = None;
        let mut count = RR3_TAIL_COUNT;
        let mut secs = RR3_TAIL_SECS;
        let mut i = 0;
        while i < rest.len() {
            i += rr3_tail_arg(rest, i, &mut topic, &mut count, &mut secs)?;
        }
        let topic =
            topic.ok_or_else(|| "缺少主题（形如 rr3-sense/audio/vad_start）".to_string())?;
        rr3_tail_check(&topic, count, secs)?;
        Ok(Rr3TailOpts {
            topic,
            count,
            timeout: Duration::from_secs(secs),
        })
    }

    /// 消费一个参数并返回占用槽数（带值开关占 2 槽）。
    fn rr3_tail_arg(
        rest: &[String],
        i: usize,
        topic: &mut Option<String>,
        count: &mut usize,
        secs: &mut u64,
    ) -> Result<usize, String> {
        match rest[i].as_str() {
            "--count" => {
                *count = rr3_tail_num(rest.get(i + 1), "--count")?;
                Ok(2)
            }
            "--secs" => {
                *secs = rr3_tail_num(rest.get(i + 1), "--secs")?;
                Ok(2)
            }
            arg => rr3_tail_pos(arg, topic),
        }
    }

    /// 位置参数：首个即 topic；未知选项与多余参数分别点名（文案逐字保留）。
    fn rr3_tail_pos(arg: &str, topic: &mut Option<String>) -> Result<usize, String> {
        if arg.starts_with('-') {
            return Err(format!("未知选项 {arg}（tail 支持 --count N / --secs S）"));
        }
        if topic.is_none() {
            *topic = Some(arg.to_string());
            return Ok(1);
        }
        Err(format!("多余的参数 {arg}"))
    }

    /// 开关值：缺值 / 非整数都点名（不在下游拿 0 冒充）。
    fn rr3_tail_num<T: std::str::FromStr>(v: Option<&String>, flag: &str) -> Result<T, String> {
        let raw = v.ok_or_else(|| format!("{flag} 缺值"))?;
        raw.parse()
            .map_err(|_| format!("{flag} 的值 {raw:?} 不是非负整数"))
    }

    /// 输入门：主题字符集、条数与秒数的下界。
    fn rr3_tail_check(topic: &str, count: usize, secs: u64) -> Result<(), String> {
        if !rr3_tail_topic_ok(topic) {
            return Err(format!(
                "主题 {topic:?} 非法：只允许 [A-Za-z0-9/_-]，长度 1..={RR3_TAIL_TOPIC_MAX}"
            ));
        }
        if count == 0 {
            return Err("--count 必须 ≥ 1".into());
        }
        if secs == 0 {
            return Err("--secs 必须 ≥ 1".into());
        }
        Ok(())
    }

    /// 主题字符集门：manifest 是拼文本下发的，topic 必须先挡住引号/换行等注入面。
    fn rr3_tail_topic_ok(topic: &str) -> bool {
        !topic.is_empty()
            && topic.len() <= RR3_TAIL_TOPIC_MAX
            && topic.bytes().all(rr3_tail_byte_ok)
    }

    /// 主题允许的字节（契约词形 `rr3-<owner>/<域>/<事件>`）。
    fn rr3_tail_byte_ok(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_')
    }

    /// 观察者 manifest：订阅面只声明目标主题，entry 由客户端按当前可执行文件
    /// 绝对化；`restart_policy = "never"`（tail 是一次性进程，不受 supervisor 管）。
    fn rr3_tail_manifest(topic: &str) -> String {
        format!(
            concat!(
                "[manifest]\n",
                "schema_version = 1\n",
                "name = \"{name}\"\n",
                "version = \"0.0.1\"\n",
                "type = \"module\"\n",
                "lang = \"rust\"\n",
                "entry = \"/dev/null\"\n\n",
                "[events]\n",
                "publishes = []\n",
                "subscribes = [\"{topic}\"]\n\n",
                "[resources]\n",
                "restart_policy = \"never\"\n\n",
                "[health]\n",
                "heartbeat_ms = 1000\n",
                "watchdog_timeout_ms = 3000\n",
            ),
            name = RR3_TAIL_NAME,
            topic = topic
        )
    }

    /// 收帧主循环：总超时 + 目标条数双界；每片回顶检查连接死活。
    fn rr3_tail_collect(client: &Rr3BusClient, opts: &Rr3TailOpts) -> Result<(), String> {
        let deadline = Instant::now() + opts.timeout;
        let mut seen = 0usize;
        while seen < opts.count {
            let left = rr3_tail_left(deadline, seen, opts.count)?;
            for frame in client.rr3_poll_wait(left.min(RR3_TAIL_SLICE)) {
                if let Some(line) = rr3_tail_line(&frame, &opts.topic) {
                    println!("{line}");
                    seen += 1;
                    if seen == opts.count {
                        break;
                    }
                }
            }
            if seen < opts.count && !client.rr3_connected() {
                return Err(format!("总线已断开（已收 {seen}/{} 条）", opts.count));
            }
        }
        eprintln!("tail: 收到 {seen} 条，正常退出");
        Ok(())
    }

    /// 剩余时长；耗尽即超时报错（带已收条数，现场一眼看出卡在哪）。
    fn rr3_tail_left(deadline: Instant, seen: usize, want: usize) -> Result<Duration, String> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!("超时：已收 {seen}/{want} 条"));
        }
        Ok(left)
    }

    /// 帧 → 一行 JSONL：只要 Event，且 `body.topic` 与目标主题逐字相等。
    fn rr3_tail_line(frame: &Rr3Frame, topic: &str) -> Option<String> {
        let (ty, body) = Rr3BusClient::rr3_frame_body(frame).ok()?;
        if ty != Rr3PayloadType::Event {
            return None;
        }
        if body.get("topic").and_then(Value::as_str) != Some(topic) {
            return None;
        }
        Some(rr3_tail_json(frame, body).to_string())
    }

    /// 行体：转发时刻（rrd 的 CLOCK_REALTIME 毫秒）+ 发布者 task + 原样事件体。
    fn rr3_tail_json(frame: &Rr3Frame, body: Value) -> Value {
        json!({
            "ts_ms": frame.header.timestamp_ns / 1_000_000,
            "src_task": frame.header.src_task,
            "body": body,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use rr_ipc::{opcode, rr3_body_encode, Rr3Header};

        fn v(items: &[&str]) -> Vec<String> {
            items.iter().map(|s| s.to_string()).collect()
        }

        fn event_frame(topic: &str) -> Rr3Frame {
            let body = json!({"topic": topic, "doa": {"deg": 12.5, "bin": "left"}});
            let payload = rr3_body_encode(Rr3PayloadType::Event, body.to_string().as_bytes());
            Rr3Frame {
                header: Rr3Header {
                    opcode: opcode::SEND,
                    flags: 0,
                    trace_id: 0,
                    timestamp_ns: 1_700_000_000_123_000_000,
                    src_task: 7,
                    dst_task: rr_bus::RR3_BROADCAST_DST_TASK,
                    payload_len: payload.len() as u32,
                    payload_crc32: rr_ipc::rr3_crc32(&payload),
                },
                payload,
            }
        }

        #[test]
        fn opts_defaults_and_flags_any_order() {
            let a = rr3_tail_opts(&v(&["rr3-sense/audio/vad_start"])).unwrap();
            assert_eq!(a.topic, "rr3-sense/audio/vad_start");
            assert_eq!(a.count, RR3_TAIL_COUNT);
            assert_eq!(a.timeout, Duration::from_secs(RR3_TAIL_SECS));

            let b = rr3_tail_opts(&v(&[
                "--secs",
                "5",
                "rr3-sense/audio/vad_start",
                "--count",
                "3",
            ]))
            .unwrap();
            assert_eq!((b.count, b.timeout.as_secs()), (3, 5));
        }

        #[test]
        fn opts_rejects_bad_input() {
            assert!(rr3_tail_opts(&v(&[])).is_err(), "缺主题");
            assert!(rr3_tail_opts(&v(&["a", "b"])).is_err(), "多余的参数");
            assert!(rr3_tail_opts(&v(&["--nope"])).is_err(), "未知选项");
            assert!(rr3_tail_opts(&v(&["t", "--count"])).is_err(), "开关缺值");
            assert!(
                rr3_tail_opts(&v(&["t", "--count", "x"])).is_err(),
                "开关值非整数"
            );
            assert!(rr3_tail_opts(&v(&["t", "--count", "0"])).is_err(), "零条");
            assert!(rr3_tail_opts(&v(&["t", "--secs", "0"])).is_err(), "零秒");
            assert!(
                rr3_tail_opts(&v(&["rr3-sense/audio/vad_start\"\nx"])).is_err(),
                "引号/换行不得进 manifest"
            );
        }

        #[test]
        fn manifest_declares_subscription_only() {
            let m = rr3_tail_manifest("rr3-sense/audio/vad_start");
            assert!(m.contains("name = \"rr3-tail\""));
            assert!(m.contains("publishes = []"));
            assert!(m.contains("subscribes = [\"rr3-sense/audio/vad_start\"]"));
            assert!(m.contains("entry = \"/dev/null\""));
            assert!(!m.contains("rpc"), "观察者不暴露 RPC 面");
        }

        #[test]
        fn line_filters_topic_and_wraps_body() {
            let f = event_frame("rr3-sense/audio/vad_start");
            let line = rr3_tail_line(&f, "rr3-sense/audio/vad_start").expect("应命中");
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["src_task"], 7);
            assert_eq!(v["ts_ms"], 1_700_000_000_123u64);
            assert_eq!(v["body"]["doa"]["deg"], 12.5);
            assert!(
                rr3_tail_line(&f, "rr3-sense/audio/pcm").is_none(),
                "主题不同不得输出"
            );
        }

        #[test]
        fn line_ignores_non_event_payload() {
            let mut f = event_frame("rr3-sense/audio/vad_start");
            let body = json!({"topic": "rr3-sense/audio/vad_start"}).to_string();
            f.payload = rr3_body_encode(Rr3PayloadType::Response, body.as_bytes());
            assert!(rr3_tail_line(&f, "rr3-sense/audio/vad_start").is_none());
        }
    }
}
