// 扇出/索引/快照/健康/相位名与错误 Display：纯查询与转发面。
use super::*;

#[test]
fn fanout_shadow_counts_unsubscribed_without_changing_delivery() {
    let mut r = RrdRegistry::new(1000);
    let (txa, rxa) = chan();
    let (txb, rxb) = chan();
    r.rr3_reg_add(&facts_subs("rr3-sima", &["rr3-sense/audio/pcm"]), txa, 0)
        .unwrap();
    r.rr3_reg_add(&facts_subs("rr3-simb", &[]), txb, 0).unwrap();
    assert_eq!(
        r.rr3_reg_shadow("rr3-sense/audio/pcm", 0),
        1,
        "simb 未申报订阅，属本应过滤目标"
    );
    assert_eq!(
        r.rr3_reg_shadow("rr3-sense/audio/pcm", 0),
        1,
        "重复帧继续记账"
    );
    let c = r.rr3_reg_counters();
    assert_eq!(c.get("fanout_shadow_frames"), Some(&2));
    assert_eq!(c.get("fanout_shadow_conns"), Some(&2));
    assert_eq!(
        c.get("fanout_shadow/rr3-sense/audio/pcm"),
        Some(&2),
        "主题分账按连接数累加"
    );
    let frame = Rr3Frame {
        header: rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0),
        payload: vec![],
    };
    assert_eq!(r.rr3_reg_fanout(&frame, 0), 2, "投递行为不变：仍全量扇出");
    assert!(rxa.try_recv().is_ok());
    assert!(rxb.try_recv().is_ok());
}

/// 转发留痕闸：首次必记、同数沉默、连接数变化再记——音频流每帧一条会刷爆日志环。
#[test]
fn fanout_log_gate_only_on_conn_change() {
    let mut r = RrdRegistry::new(1000);
    assert!(r.rr3_reg_fan_changed("rr3-sense/audio/pcm", 4), "首次必记");
    assert!(!r.rr3_reg_fan_changed("rr3-sense/audio/pcm", 4), "同数沉默");
    assert!(
        r.rr3_reg_fan_changed("rr3-sense/audio/pcm", 3),
        "连接数变化必记"
    );
    assert!(
        !r.rr3_reg_fan_changed("rr3-sense/audio/pcm", 3),
        "回落同数沉默"
    );
    assert!(
        r.rr3_reg_fan_changed("rr3-act/servo/state", 2),
        "按 topic 分账"
    );
}

/// 路由表读取：exposes 匹配、多 callee 取 task 最小、failed 排除、未声明回 None
#[test]
fn callee_for_matches_exposes_and_prefers_lowest_task() {
    let mut r = RrdRegistry::new(1000);
    // simb 先注册（task 小），同样 exposes t/a；sima 晚 300ms 注册（判死时刻随之错开）
    let b = r
        .rr3_reg_add(&facts_exposes("rr3-simb", &["rr3-x/t/a"]), chan().0, 0)
        .unwrap();
    let a = r
        .rr3_reg_add(
            &facts_exposes("rr3-sima", &["rr3-x/t/a", "rr3-x/t/b"]),
            chan().0,
            300,
        )
        .unwrap();
    r.rr3_reg_add(&facts_exposes("rr3-silent", &[]), chan().0, 0)
        .unwrap();
    // 多个 callee：取 src_task 最小的 simb（注册序在前），不随名字序漂
    assert_eq!(
        r.rr3_reg_callee_for("rr3-x/t/a"),
        Some(("rr3-simb".into(), b))
    );
    assert_eq!(
        r.rr3_reg_callee_for("rr3-x/t/b"),
        Some(("rr3-sima".into(), a))
    );
    // 没人 exposes 的 topic：None
    assert_eq!(r.rr3_reg_callee_for("rr3-x/t/none"), None);
    // 判死后（failed + tx 摘除）不再是被调方：600ms 时 simb（watchdog 500、基准 0）已死，
    // sima（基准 300，300 < 500）还活着
    r.rr3_reg_expire(600);
    assert_eq!(
        r.rr3_reg_callee_for("rr3-x/t/a"),
        Some(("rr3-sima".into(), a))
    );
    r.rr3_reg_expire(1200);
    assert_eq!(
        r.rr3_reg_callee_for("rr3-x/t/b"),
        None,
        "failed 条目必须被路由排除"
    );
}

/// exposes 索引随条目离表同步（批D2）：unregister 摘掉候选后路由即空；同身份
/// 重注册换新 task，旧 task 的索引项不残留（索引不随失败历史增长）。
#[test]
fn callee_index_follows_unregister_and_reregistration() {
    let mut r = RrdRegistry::new(1000);
    let t0 = r
        .rr3_reg_add(&facts_exposes("rr3-idx", &["rr3-x/t/a"]), chan().0, 0)
        .unwrap();
    assert_eq!(
        r.rr3_reg_callee_for("rr3-x/t/a"),
        Some(("rr3-idx".into(), t0))
    );
    assert!(r.rr3_reg_remove("rr3-idx"));
    assert_eq!(r.rr3_reg_callee_for("rr3-x/t/a"), None, "摘除后索引同步");
    assert!(r.exposes.is_empty(), "摘空的 topic 键必须整键删掉");

    let t1 = r
        .rr3_reg_add(&facts_exposes("rr3-idx", &["rr3-x/t/a"]), chan().0, 0)
        .unwrap();
    r.rr3_reg_death("rr3-idx", false, 0);
    assert_eq!(r.rr3_reg_callee_for("rr3-x/t/a"), None, "failed 不进路由");
    let t2 = r
        .rr3_reg_add(&facts_exposes("rr3-idx", &["rr3-x/t/a"]), chan().0, 700)
        .unwrap();
    assert_eq!(
        r.rr3_reg_callee_for("rr3-x/t/a"),
        Some(("rr3-idx".into(), t2))
    );
    assert!(
        !r.exposes["rr3-x/t/a"].contains(&t1),
        "被替换条目的旧 task 不得留在索引里"
    );
}

/// 点名投递：在册活连接收得到，未知 task / 已死连接回 false
#[test]
fn send_to_delivers_to_named_connection_only() {
    use rr_ipc::{opcode, rr3_body_encode, Rr3Header, Rr3PayloadType};
    let mut r = RrdRegistry::new(1000);
    let (tx, rx) = chan();
    let t = r
        .rr3_reg_add(&facts_exposes("rr3-sima", &["rr3-x/t/a"]), tx, 0)
        .unwrap();
    let payload = rr3_body_encode(Rr3PayloadType::Notification, b"hello");
    let frame = Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: 0,
            src_task: rr_bus::RR3_TASK_RRD,
            dst_task: t,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    };
    assert!(r.rr3_reg_send_to(t, &frame));
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .payload,
        frame.payload
    );
    // 未知 task 与非在册号段都不投
    assert!(!r.rr3_reg_send_to(999, &frame));
    assert!(!r.rr3_reg_send_to(rr_bus::RR3_TASK_RRD, &frame));
    // 判死后 tx 被摘：不投
    r.rr3_reg_expire(600);
    assert!(!r.rr3_reg_send_to(t, &frame));
}

/// 分配从 2 起单调递增，0/1 永不出现；快照按 task 升序
#[test]
fn task_ids_start_at_two_and_increase() {
    let mut r = RrdRegistry::new(1000);
    let a = r
        .rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0)
        .unwrap();
    let b = r
        .rr3_reg_add(&facts("rr3-simb", 200, 1000, "always"), chan().0, 0)
        .unwrap();
    assert_eq!(a, rr_bus::RR3_TASK_FIRST_MODULE);
    assert_eq!(b, a + 1);
    assert_eq!(
        r.rr3_reg_snapshot(),
        vec![
            ("rr3-sima".into(), 2, Rr3Phase::Registered),
            ("rr3-simb".into(), 3, Rr3Phase::Registered)
        ]
    );
}

/// 快照的序是 task 升序而非名字升序：先注册 z 再注册 a，顺序必须跟着 task 走
/// （少了 sort_by_key 这一行，`BTreeMap<String,_>` 的迭代序会把名字序冒充成 task 序）
#[test]
fn snapshot_orders_by_task_not_by_name() {
    let mut r = RrdRegistry::new(1000);
    let z = r
        .rr3_reg_add(&facts("rr3-zeta", 200, 1000, "always"), chan().0, 0)
        .unwrap();
    let a = r
        .rr3_reg_add(&facts("rr3-alpha", 200, 1000, "always"), chan().0, 0)
        .unwrap();
    assert!(a > z, "后注册的号必须更大");
    assert_eq!(
        r.rr3_reg_snapshot(),
        vec![
            ("rr3-zeta".into(), z, Rr3Phase::Registered),
            ("rr3-alpha".into(), a, Rr3Phase::Registered)
        ],
        "名字序（alpha, zeta）与 task 序（zeta, alpha）相反，才能抓出没排序的实现"
    );
}

/// 健康快照（F3 / ADR-0010 裁定 4）：task 升序、回显判定用的三个数——
/// `since_beat_ms` 必须是 `now - last_beat`，配置回显条目自己的 heartbeat/watchdog
#[test]
fn health_snapshot_echoes_the_judgement_inputs_in_task_order() {
    let mut r = RrdRegistry::new(1000);
    let z = r
        .rr3_reg_add(&facts("rr3-zeta", 200, 1000, "always"), chan().0, 0)
        .unwrap();
    let a = r
        .rr3_reg_add(&facts("rr3-alpha", 300, 2000, "always"), chan().0, 0)
        .unwrap();
    let rows = r.rr3_reg_health(1500);
    assert_eq!(
        rows.iter()
            .map(|x| (x.name.as_str(), x.src_task))
            .collect::<Vec<_>>(),
        vec![("rr3-zeta", z), ("rr3-alpha", a)],
        "与 rr3_reg_snapshot 同一排序口径（task 升序）"
    );
    let alpha = rows.iter().find(|x| x.name == "rr3-alpha").unwrap();
    assert_eq!(alpha.heartbeat_ms, 300);
    assert_eq!(alpha.watchdog_ms, 2000);
    assert_eq!(
        alpha.since_beat_ms, 1500,
        "注册时刻 0，now=1500 → 距上次心跳 1500"
    );
    assert_eq!(alpha.phase, Rr3Phase::Registered);
}

/// 健康快照的 `since_beat_ms` 对"now 早于 last_beat"不得回绕：saturating_sub 兜底
#[test]
fn health_snapshot_since_beat_never_wraps() {
    let mut r = RrdRegistry::new(1000);
    r.rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 1000)
        .unwrap();
    let rows = r.rr3_reg_health(500);
    assert_eq!(
        rows[0].since_beat_ms, 0,
        "now < last_beat 时夹 0，不是 u64 回绕的大数"
    );
}

/// 四个阶段名与 `as_str()` 一对一：下行状态广播与集成测试按这些字面量比对
#[test]
fn phase_names_are_the_wire_vocabulary() {
    assert_eq!(Rr3Phase::Registered.as_str(), "registered");
    assert_eq!(Rr3Phase::Running.as_str(), "running");
    assert_eq!(Rr3Phase::Degraded.as_str(), "degraded");
    assert_eq!(Rr3Phase::Failed.as_str(), "failed");
    // 四条必须是互不相同的字符串，否则状态广播会把两种状态发成同一个词
    let names = [
        Rr3Phase::Registered.as_str(),
        Rr3Phase::Running.as_str(),
        Rr3Phase::Degraded.as_str(),
        Rr3Phase::Failed.as_str(),
    ];
    let mut sorted: Vec<&str> = names.into_iter().collect();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "阶段名互不相同: {names:?}");
}

/// 注册拒绝的三条文案逐字钉住（运维日志与 Task 10 的 wire 用例按字面比对）
#[test]
fn register_error_display_is_verbatim() {
    assert_eq!(
        Rr3RegisterError::Duplicate {
            name: "rr3-sima".into(),
            src_task: 7
        }
        .to_string(),
        "模块 rr3-sima 已在册（task=7），拒绝重复注册"
    );
    assert_eq!(
        Rr3RegisterError::ReservedName {
            name: "rr3-runtime".into()
        }
        .to_string(),
        "模块名 rr3-runtime 属内建命名空间保留号，拒绝注册"
    );
    assert_eq!(
        Rr3RegisterError::Capacity {
            limit: RR3_REG_MAX_MODULES
        }
        .to_string(),
        "在册模块已达上限 16，拒绝注册"
    );
}

/// 三条拒绝文案各自点名自己的关键信息：冒充点名性质、越权点名 owner+sender、
/// 未点名主题点名主题（Task 10 靠这些子串区分被哪一层拒的）
#[test]
fn publish_deny_display_distinguishes_the_three_layers() {
    let d = Rr3PublishDeny::ReservedNamespace {
        topic: "rr3-runtime/module/state".into(),
    };
    assert_eq!(
        d.to_string(),
        "拒绝 rr3-runtime/module/state：rr3-runtime/* 是守护进程的下行广播，模块不得冒充"
    );
    let f = Rr3PublishDeny::ForeignPrefix {
        topic: "rr3-simb/pong/state".into(),
        owner: "rr3-simb".into(),
        sender: "rr3-sima".into(),
    };
    assert_eq!(
        f.to_string(),
        "越权发布 rr3-simb/pong/state（owner=rr3-simb，发送方=rr3-sima）"
    );
    assert_eq!(
        Rr3PublishDeny::NotRegistered {
            topic: "rr3-sima/x/state".into()
        }
        .to_string(),
        "未注册主题 rr3-sima/x/state，拒绝发布"
    );
}
