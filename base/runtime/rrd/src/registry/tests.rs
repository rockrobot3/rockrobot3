// registry 的单元测试：状态机全转移、索引维护、扇出与降级路径。
// 测试是 registry 的后代模块，可触私有字段（与实现同口径）。
use super::*;
use crate::manifest::rr3_manifest_parse;
use std::sync::mpsc;

fn facts(name: &str, hb: u64, wd: u64, policy: &str) -> Rr3ManifestFacts {
    let text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = []

[resources]
restart_policy = "{policy}"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

fn chan() -> (SyncSender<Rr3Frame>, std::sync::mpsc::Receiver<Rr3Frame>) {
    mpsc::sync_channel(RR3_FAN_CAP)
}

/// 带 `[rpc].exposes` 的 facts（M3 路由用例的夹具；exposes 为空 = 无 [rpc] 段）
fn facts_exposes(name: &str, exposes: &[&str]) -> Rr3ManifestFacts {
    let rpc = if exposes.is_empty() {
        String::new()
    } else {
        let list = exposes
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[rpc]\nexposes = [{list}]\n\n")
    };
    let text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = []

{rpc}[resources]
restart_policy = "never"

[health]
heartbeat_ms = 100
watchdog_timeout_ms = 500
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

/// 带 `[events].subscribes` 的 facts（广播影子统计用例的夹具）
fn facts_subs(name: &str, subs: &[&str]) -> Rr3ManifestFacts {
    let list = subs
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = [{list}]

[resources]
restart_policy = "never"

[health]
heartbeat_ms = 100
watchdog_timeout_ms = 500
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

/// 影子统计：按 subscribes 只记账"本应过滤"的连接，不改变全量投递
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

/// 同名在册必须拒，且不留影子条目
#[test]
fn duplicate_name_is_rejected() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0)
        .unwrap();
    let e = r
        .rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0)
        .expect_err("同名在册必须拒");
    assert_eq!(
        e,
        Rr3RegisterError::Duplicate {
            name: "rr3-sima".into(),
            src_task: t
        }
    );
    assert_eq!(r.rr3_reg_snapshot().len(), 1);
    assert_eq!(
        e.to_string(),
        "模块 rr3-sima 已在册（task=2），拒绝重复注册"
    );
    // 拒重复不许改已在册条目的状态，也不许换掉它的 task 号
    assert_eq!(
        r.rr3_reg_snapshot()[0],
        ("rr3-sima".to_string(), t, Rr3Phase::Registered)
    );
    assert_eq!(r.rr3_reg_name_of(t).as_deref(), Some("rr3-sima"));
}

/// rr3-runtime 是 rrd 自己的号段，模块冒充即最危险的伪造面
#[test]
fn reserved_module_name_is_rejected() {
    let mut r = RrdRegistry::new(1000);
    assert_eq!(
        r.rr3_reg_add(&facts("rr3-runtime", 200, 1000, "always"), chan().0, 0),
        Err(Rr3RegisterError::ReservedName {
            name: "rr3-runtime".into()
        })
    );
    assert!(r.rr3_reg_snapshot().is_empty());
}

#[test]
fn capacity_limit_is_enforced() {
    let mut r = RrdRegistry::new(1000);
    for i in 0..RR3_REG_MAX_MODULES {
        r.rr3_reg_add(
            &facts(&format!("rr3-m{i}"), 200, 1000, "never"),
            chan().0,
            0,
        )
        .unwrap();
    }
    assert_eq!(
        r.rr3_reg_add(&facts("rr3-over", 200, 1000, "never"), chan().0, 0),
        Err(Rr3RegisterError::Capacity {
            limit: RR3_REG_MAX_MODULES
        })
    );
    // 超额拒绝不许留下半个条目，也不许消耗一个 task 号
    assert_eq!(r.rr3_reg_snapshot().len(), RR3_REG_MAX_MODULES);
    assert_eq!(
        r.rr3_reg_name_of(rr_bus::RR3_TASK_FIRST_MODULE + RR3_REG_MAX_MODULES as u64),
        None
    );
}

/// 容量门的那半句 `superseded.is_none() &&` **承重**（Task 7 评审遗留 ②）：表满但其中一个
/// 同名条目已是 Failed 时，它即将被整体替换、不占新槽位，这是合法的 Failed 重注册，
/// 绝不会被容量拒。Task 5 的 17 组变异实验里，删掉这半句后"表满仍可重注册 Failed"
/// 立刻变成 `Capacity` 拒而当时全绿 —— 因为没有任何用例把两件事同时凑齐（表满 + 同名 Failed）。
/// 这里就把那个组合钉住，并带上两个对照：新名字在满表上仍被拒（门没被整条删掉），
/// 未 Failed 的同名仍是 `Duplicate` 而不是容量错（门的序也没换）。
#[test]
fn full_table_still_allows_re_registering_a_failed_module() {
    let mut r = RrdRegistry::new(1000);
    for i in 0..RR3_REG_MAX_MODULES {
        r.rr3_reg_add(
            &facts(&format!("rr3-m{i}"), 200, 1000, "never"),
            chan().0,
            0,
        )
        .unwrap();
    }
    // 对照 2：满表上的**新**名字必须仍被容量拒
    assert_eq!(
        r.rr3_reg_add(&facts("rr3-over", 200, 1000, "never"), chan().0, 0),
        Err(Rr3RegisterError::Capacity {
            limit: RR3_REG_MAX_MODULES
        }),
        "对照：容量门本身不许被整条删掉"
    );
    // 对照 3：满表上的同名**非 Failed** 条目仍按 Duplicate 拒（三条门的序：在册先于容量）
    assert_eq!(
        r.rr3_reg_add(&facts("rr3-m0", 200, 1000, "never"), chan().0, 0),
        Err(Rr3RegisterError::Duplicate {
            name: "rr3-m0".into(),
            src_task: rr_bus::RR3_TASK_FIRST_MODULE
        }),
        "对照：同名在册该回 Duplicate，不是 Capacity"
    );

    let old = rr_bus::RR3_TASK_FIRST_MODULE;
    assert!(
        r.rr3_reg_death("rr3-m0", true, 10).rr3_plan().is_none(),
        "never 不给重启计划"
    );
    assert_eq!(
        r.rr3_reg_snapshot().len(),
        RR3_REG_MAX_MODULES,
        "failed 不出表，槽位数不变"
    );
    assert_eq!(
        r.rr3_reg_snapshot()[0].2,
        Rr3Phase::Failed,
        "用例前提：待替换条目确为 Failed"
    );

    // 本用例的主角：表满 + 同名 Failed → 必须放行换新 task，而不是回 Capacity
    let again = r
        .rr3_reg_add(&facts("rr3-m0", 200, 1000, "never"), chan().0, 20)
        .expect("表满时重注册 Failed 的同名条目必须放行（容量门的 superseded 半句）");
    assert_eq!(
        again,
        old + RR3_REG_MAX_MODULES as u64,
        "重注册拿下一个新号，旧号作废不回收"
    );
    assert_eq!(
        r.rr3_reg_snapshot().len(),
        RR3_REG_MAX_MODULES,
        "替换而非追加：槽位数不得增长"
    );
    assert_eq!(r.rr3_reg_name_of(old), None, "旧 task 号必须从映射里摘掉");
    assert_eq!(r.rr3_reg_name_of(again).as_deref(), Some("rr3-m0"));
    assert_eq!(
        r.rr3_reg_snapshot()
            .iter()
            .filter(|(n, _, _)| n == "rr3-m0")
            .count(),
        1,
        "同名条目不得同时存在两条"
    );
    assert!(r.rr3_reg_live("rr3-m0"), "重注册之后它又是一条活连接");
}

/// 注册期不许把 `watchdog_ms = 0` 收进表里：那样 `now - last_beat >= 0` 恒成立，
/// 首轮到期扫描就把全表秒判死。`rr3_manifest_parse` 已经把非正整数拒在
/// 读取期，但 `Rr3ManifestFacts` 字段全 `pub`，调用方可以直接构造绕过它——所以入库
/// 那一刻还得钉一次。`cargo test` 跑的是 debug 构建（`debug_assertions` 开），
/// 故这里用 catch_unwind 断言它当场被拦；release 侧的下界不在本表也不在 CLI（没有 watchdog
/// 这个旋钮），由 `manifest.rs` 的 `req_int` 在文本读取期拦住（Task 4 已落，debug/release 都拦）。
#[cfg(debug_assertions)]
#[test]
fn zero_watchdog_is_caught_at_register_time() {
    let mut r = RrdRegistry::new(1000);
    let mut f = facts("rr3-sima", 100, 500, "always");
    f.watchdog_ms = 0; // 绕过 manifest.rs 校验的唯一途径：pub 字段直接改
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        r.rr3_reg_add(&f, chan().0, 0)
    }));
    assert!(
        caught.is_err(),
        "watchdog_ms = 0 必须在入库那一刻被拦下，不能留给到期扫描去判死"
    );
}

/// 三条拒绝门必须先于 watchdog 断言：`rr3_reg_add` 的文档序是"保留名 → 同名在册 →
/// 超额 → 分配 task"，而断言原先坐在函数第一行，debug 构建里一个带着坏 watchdog 的
/// 重复注册会 panic 而不是如实返回 Duplicate——调用方拿不到拒绝理由，也就回不出
/// 正确错误码（Task 6 的 `rr3_rpc_reg_status` 正是拿这个 Err 去映射，它的调用方
/// `conn.rs` 已随 Task 7 落地）。
/// 三条门各给一个"坏 watchdog + 该拒"的输入，缺一条就抓不到。
#[cfg(debug_assertions)]
#[test]
fn register_gates_run_before_the_watchdog_assertion() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();

    let mut reserved = facts("rr3-runtime", 100, 500, "always");
    reserved.watchdog_ms = 0;
    assert_eq!(
        r.rr3_reg_add(&reserved, chan().0, 0),
        Err(Rr3RegisterError::ReservedName {
            name: "rr3-runtime".into()
        }),
        "保留名必须先拒，不能让 watchdog 断言把拒绝变成 panic"
    );
    let mut dup = facts("rr3-sima", 100, 500, "always");
    dup.watchdog_ms = 0;
    assert_eq!(
        r.rr3_reg_add(&dup, chan().0, 0),
        Err(Rr3RegisterError::Duplicate {
            name: "rr3-sima".into(),
            src_task: t
        }),
        "同名在册必须先拒"
    );
    // 循环补 15 条，加上已有的 rr3-sima 正好占满 16 槽；下面的 over 是第 17 次尝试，必须被容量门拒
    for i in 0..RR3_REG_MAX_MODULES - 1 {
        r.rr3_reg_add(&facts(&format!("rr3-m{i}"), 100, 500, "never"), chan().0, 0)
            .unwrap();
    }
    let mut over = facts("rr3-over", 100, 500, "never");
    over.watchdog_ms = 0;
    assert_eq!(
        r.rr3_reg_add(&over, chan().0, 0),
        Err(Rr3RegisterError::Capacity {
            limit: RR3_REG_MAX_MODULES
        }),
        "超额必须先拒"
    );
    assert_eq!(
        r.rr3_reg_snapshot().len(),
        RR3_REG_MAX_MODULES,
        "三次拒绝都不许留下影子条目"
    );
}

/// watchdog 断言移位后，被 catch_unwind 捕获的 debug panic 不许留下"已改过的表"：
/// Failed 同身份重注册路径上，旧条目的摘除原先发生在断言**之前**，panic 一旦被外层
/// 吞掉，Failed 条目与它的 task→name 映射就双双蒸发（发布归属、fanout、
/// `rr3_reg_name_of` 全部受影响）。现在摘除延后到门+断言全过之后，panic 时表必须原样。
/// 仅 debug 构建（`debug_assert` 在 release 被编译掉，这条窗口在 release 不存在）。
#[cfg(debug_assertions)]
#[test]
fn caught_panic_leaves_the_failed_entry_registered() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "never"), chan().0, 0)
        .unwrap();
    assert!(
        r.rr3_reg_death("rr3-sima", true, 600).rr3_plan().is_none(),
        "never 不给重启计划"
    );
    assert_eq!(
        r.rr3_reg_snapshot(),
        vec![("rr3-sima".into(), t, Rr3Phase::Failed)]
    );
    let mut again = facts("rr3-sima", 100, 500, "never");
    again.watchdog_ms = 0; // 绕过 facts 读取入口，触发入库断言
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        r.rr3_reg_add(&again, chan().0, 700)
    }));
    assert!(caught.is_err(), "watchdog_ms = 0 必须仍被断言拦下");
    assert_eq!(
        r.rr3_reg_snapshot(),
        vec![("rr3-sima".into(), t, Rr3Phase::Failed)],
        "被捕获的 panic 不得改动在册表：Failed 条目与 task 号必须原样留在表里"
    );
    assert_eq!(
        r.rr3_reg_name_of(t).as_deref(),
        Some("rr3-sima"),
        "task→name 映射同样不许被摘掉"
    );
}

/// 注册即 registered；收到心跳才 running；未在册 task 的心跳被拒
#[test]
fn heartbeat_promotes_registered_to_running() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 200, 1000, "always"), chan().0, 0)
        .unwrap();
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered);
    assert_eq!(r.rr3_reg_beat(t, 100), Rr3Beat::Promoted);
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Running);
    assert_eq!(
        r.rr3_reg_beat(99, 200),
        Rr3Beat::Rejected,
        "未在册 task 的心跳必须拒"
    );
}

/// 到期完全由显式毫秒驱动：窗口内不判死，超窗口判死转 failed，此后喂狗无效
#[test]
fn expire_uses_explicit_monotonic_millis() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    assert_eq!(r.rr3_reg_beat(t, 400), Rr3Beat::Promoted);
    assert!(
        r.rr3_reg_expire(499).is_empty(),
        "距上次心跳 499-400=99ms < 500ms 不得判死"
    );
    assert_eq!(
        r.rr3_reg_expire(901),
        vec![Rr3Expiry {
            name: "rr3-sima".into(),
            src_task: t,
            reason: "heartbeat_timeout",
            elapsed_ms: 501,
            watchdog_ms: 500,
            restart: Some(Rr3RestartPlan {
                name: "rr3-sima".into(),
                attempt: 1,
                delay_ms: 1000,
                entry: "/usr/bin/rr3-sima".into(),
            }),
        }]
    );
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    assert_eq!(
        r.rr3_reg_beat(t, 950),
        Rr3Beat::Rejected,
        "已 failed 的模块不得靠补心跳复活"
    );
}

/// 回报里的两个数必须就是判定本身：`elapsed_ms == now - last_beat`、
/// `watchdog_ms == 该模块自己的超时值`。线程侧的日志逐字引用它们，
/// 日志说"501 ≥ 500"而实际按别的数判死，就是线上唯一可读的判死证据在撒谎。
#[test]
fn expiry_report_carries_the_numbers_that_decided_it() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    assert_eq!(
        r.rr3_reg_beat(t, 400),
        Rr3Beat::Promoted,
        "夹具心跳必须先被接受，否则 elapsed 无从谈起"
    );
    let e = &r.rr3_reg_expire(1_001)[0];
    assert_eq!(
        e.elapsed_ms, 601,
        "elapsed 必须是 now-last_beat，不是 now-注册时刻"
    );
    assert_eq!(
        e.watchdog_ms, 500,
        "必须回显该模块自己的 watchdog_ms，不是全局常量"
    );
}

/// registered（一颗心跳都没拿到）同样受 watchdog 约束
#[test]
fn registered_without_any_beat_still_expires() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 300, "always"), chan().0, 0)
        .unwrap();
    assert!(r.rr3_reg_expire(299).is_empty());
    assert_eq!(r.rr3_reg_expire(300)[0].src_task, t, "从注册时刻起算");
}

/// 到期扫描不得牵连健康模块（唯一监视线程可安全每 tick 全表扫）
#[test]
fn expire_only_reports_stale_modules() {
    let mut r = RrdRegistry::new(1000);
    let a = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    r.rr3_reg_add(&facts("rr3-simb", 100, 5000, "always"), chan().0, 0)
        .unwrap();
    let _ = r.rr3_reg_beat(a, 1000);
    let e = r.rr3_reg_expire(1400);
    assert_eq!(e.len(), 0, "simb 窗口 5000ms 未到、sima 刚喂过");
    // sima 继续喂狗（它自己的窗口是 500ms，不喂它就该和 simb 一起被报）
    let _ = r.rr3_reg_beat(a, 5900);
    let e2 = r.rr3_reg_expire(6000);
    assert_eq!(
        e2.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
        vec!["rr3-simb"]
    );
    // 只有 simb 被判死：sima 仍活着且句柄未被牵连
    assert_eq!(
        r.rr3_reg_snapshot()[0].2,
        Rr3Phase::Running,
        "健康模块不得被邻居超时改掉状态"
    );
    assert_eq!(
        r.rr3_reg_expire(6200).len(),
        0,
        "已 failed 的条目不得重复回报"
    );
}

/// 越权发布：前缀不等于注册名即拒，文案带 owner/sender
#[test]
fn publish_rejects_foreign_prefix() {
    let set = topic_set(&[
        ("rr3-sima/ping/state", "rr3-sima"),
        ("rr3-simb/pong/state", "rr3-simb"),
    ]);
    assert!(RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-sima/ping/state", &set).is_ok());
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-simb/pong/state", &set)
            .expect_err("越权必须拒"),
        Rr3PublishDeny::ForeignPrefix {
            topic: "rr3-simb/pong/state".into(),
            owner: "rr3-simb".into(),
            sender: "rr3-sima".into(),
        }
    );
}

/// 归属判定的**最后一道**看的是主题集里登记的 owner，而不是主题串的首段：
/// 首段相等只可能来自"表没被 `rr3_topic_check` 复验过"（check 强制首段 == owner，
/// `rr-registry/src/topics.rs` 的 `rr3_topic_check`），而那条前置只在
/// `rr3_topics_load` 的装载路径上成立。`rr3_topic_parse` 是 public，
/// 任何绕开 check 造出来的表都可能带着"首段是 rr3-sima、owner 登记的是 rr3-simb"的条目。
/// 故本用例**刻意不用** `topic_set()`（它 assert 过 check，结构上造不出这张表）。
#[test]
fn publish_check_uses_the_registered_owner_not_just_the_first_segment() {
    let set = rr_registry::rr3_topic_parse(
        r#"schema_version = 1
[[owner]]
name = "rr3-simb"
[[topic]]
name = "rr3-sima/ping/state"
kind = "event"
owner = "rr3-simb"
"#,
    );
    // 夹具前提三条：首段就是发送方自己、主题确实在册、owner 登记的却是别人。
    assert_eq!(
        set.rr3_topic_owner("rr3-sima/ping/state").as_deref(),
        Some("rr3-simb")
    );
    assert!(set.rr3_topic_contains("rr3-sima/ping/state"));
    assert!(
        !set.rr3_topic_check().is_empty(),
        "前提：这张表本来就形状不合法（首段 ≠ owner），装载路径永不产出它，只用于验第三道防线"
    );
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-sima/ping/state", &set),
        Err(Rr3PublishDeny::ForeignPrefix {
            topic: "rr3-sima/ping/state".into(),
            owner: "rr3-simb".into(),
            sender: "rr3-sima".into(),
        }),
        "前缀对得上但在册 owner 是别人：必须按越权拒，而不是因为首段相符就放行"
    );
}

/// 在册表里 owner 缺失（`[[topic]]` 没写 owner 键，解析成空串）时，越权文案的 owner
/// 必须回显 `?`：与 `Rr3PublishDeny::ForeignPrefix` 的文档、也与"主题根本不在册"那条
/// 分支（`owner: ...unwrap_or_else(|| "?")`）一致——同一种"不知道 owner 是谁"不许一会儿
/// 空串一会儿 `?`，否则日志里两种写法要人猜是不是两回事。
/// 夹具要点：主题**在册**且首段与发送方相符，所以前两道分支都不会命中，只有第三道
/// （登记 owner 与发送方比对）能拒——本用例钉的就是它的回显值。
#[test]
fn publish_foreign_prefix_echoes_question_mark_when_owner_is_missing() {
    let set = rr_registry::rr3_topic_parse(
            "\n[[owner]]\nname = \"rr3-sima\"\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"event\"\n",
        );
    assert!(
        set.rr3_topic_contains("rr3-sima/ping/state"),
        "用例前提：主题在册"
    );
    assert_eq!(
        set.rr3_topic_owner("rr3-sima/ping/state").as_deref(),
        Some("")
    );
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-sima/ping/state", &set),
        Err(Rr3PublishDeny::ForeignPrefix {
            topic: "rr3-sima/ping/state".into(),
            owner: "?".into(),
            sender: "rr3-sima".into(),
        }),
        "登记 owner 缺失必须回显 ?，不得回显空串"
    );
}

/// `owner=?` 回显的一致性：`ForeignPrefix` 有**两处**构造点（前缀不符 / owner 比对不符），
/// 两处遇到"在册但登记文本没写 owner 键（解析成空串）"都必须回显 `?`——同一种"不知道
/// owner 是谁"不许一处空串一处 `?`。本用例把 `Rr3PublishDeny::ForeignPrefix` 文档里那句
/// "两条分支同一写法"逐分支钉住：前一段打第一处构造点（head ≠ sender），
/// 后一段打第三道分支（head == sender、在册、owner 却取不到）。
#[test]
fn foreign_prefix_owner_echo_is_question_mark_on_both_construction_sites() {
    // 第一处构造点：主题 rr3-sima/x/y 在册且 owner 键缺失，发送方 rr3-simb → 前缀不符分支
    let set = rr_registry::rr3_topic_parse(
        "\n[[owner]]\nname = \"rr3-sima\"\n[[topic]]\nname = \"rr3-sima/x/y\"\nkind = \"event\"\n",
    );
    assert_eq!(set.rr3_topic_owner("rr3-sima/x/y").as_deref(), Some(""));
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-simb", "rr3-sima/x/y", &set),
        Err(Rr3PublishDeny::ForeignPrefix {
            topic: "rr3-sima/x/y".into(),
            owner: "?".into(),
            sender: "rr3-simb".into(),
        }),
        "前缀不符分支在 owner 键缺失时必须回显 ?，不得回显登记的空串"
    );
    // 第三道分支：首段与发送方相符、主题在册、owner 键缺失 → 只有 owner 比对能拒
    let set2 = rr_registry::rr3_topic_parse(
            "\n[[owner]]\nname = \"rr3-sima\"\n[[topic]]\nname = \"rr3-sima/ping/state\"\nkind = \"event\"\n",
        );
    assert_eq!(
            RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-sima/ping/state", &set2),
            Err(Rr3PublishDeny::ForeignPrefix {
                topic: "rr3-sima/ping/state".into(),
                owner: "?".into(),
                sender: "rr3-sima".into(),
            }),
            "owner 比对分支在 owner 键缺失时必须回显 ?（与 publish_foreign_prefix_echoes_question_mark_when_owner_is_missing 同路径，这里双分支同测）"
        );
}

/// 主题名少于两段（`rr3-sima` 独占一段）时前缀仍须等于注册名，越权面不因段数缩小
#[test]
fn publish_rejects_foreign_prefix_with_single_segment_topic() {
    let set = topic_set(&[
        ("rr3-sima/ping/state", "rr3-sima"),
        ("rr3-simb/pong/state", "rr3-simb"),
    ]);
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-simb", &set),
        Err(Rr3PublishDeny::ForeignPrefix {
            topic: "rr3-simb".into(),
            owner: "?".into(),
            sender: "rr3-sima".into(),
        })
    );
}

/// 冒充内建命名空间优先于一切判定（哪怕该主题真在主题集里）
#[test]
fn publish_rejects_runtime_impersonation_first() {
    let set = topic_set(&[("rr3-runtime/module/state", "rr3-runtime")]);
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-runtime/module/state", &set)
            .expect_err("下行广播冒充必须拒"),
        Rr3PublishDeny::ReservedNamespace {
            topic: "rr3-runtime/module/state".into()
        }
    );
    // 该主题既在册、owner 又是 rr3-runtime：若顺序颠倒会先撞上"在册"或"前缀"两层，
    // 得到 Ok / ForeignPrefix 而不是"冒充"这条定性
    assert!(set.rr3_topic_contains("rr3-runtime/module/state"));
}

/// `rr3-runtime/*` 对任何模块都是禁区，与它在主题集里有没有条目无关
#[test]
fn publish_rejects_reserved_namespace_whatever_the_sender_claims() {
    let set = topic_set(&[("rr3-sima/ping/state", "rr3-sima")]);
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-runtime/system/heartbeat", &set),
        Err(Rr3PublishDeny::ReservedNamespace {
            topic: "rr3-runtime/system/heartbeat".into()
        })
    );
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-runtime", &set),
        Err(Rr3PublishDeny::ReservedNamespace {
            topic: "rr3-runtime".into()
        })
    );
}

/// 前缀对但没在册（拼错域名）也必须拒
#[test]
fn publish_rejects_unregistered_topic() {
    let set = topic_set(&[("rr3-sima/ping/state", "rr3-sima")]);
    assert_eq!(
        RrdRegistry::rr3_reg_pub_check("rr3-sima", "rr3-sima/ping/others", &set),
        Err(Rr3PublishDeny::NotRegistered {
            topic: "rr3-sima/ping/others".into()
        })
    );
}

/// 越权只计数，不影响在册状态与后续合法发布
#[test]
fn denied_publish_is_counted_not_fatal() {
    let mut r = RrdRegistry::new(1000);
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    r.rr3_reg_note_denied();
    r.rr3_reg_note_denied();
    assert_eq!(r.rr3_reg_denied(), 2);
    assert_eq!(
        r.rr3_reg_snapshot()[0].2,
        Rr3Phase::Registered,
        "越权不改生命周期状态"
    );
    assert!(r.rr3_reg_live("rr3-sima"), "越权不得掐掉自己的连接");
    assert_eq!(r.rr3_reg_expire(100).len(), 0, "计数不改动到期基准");
}

/// never：死亡即 failed，不给重启计划
#[test]
fn never_policy_never_restarts() {
    let mut r = RrdRegistry::new(1000);
    r.rr3_reg_add(&facts("rr3-simb", 100, 500, "never"), chan().0, 0)
        .unwrap();
    assert_eq!(r.rr3_reg_death("rr3-simb", false, 600).rr3_plan(), None);
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
}

/// 运维计数聚合（0.0.4 批B）：模块前缀归并、同键累加、空拍不算变化
#[test]
fn counters_merge_prefixes_and_accumulates() {
    use std::collections::BTreeMap;
    let mut r = RrdRegistry::new(1000);
    let mut beat = BTreeMap::new();
    beat.insert("asr_ok".to_string(), 2u64);
    beat.insert("asr_fail".to_string(), 1u64);
    assert!(r.rr3_reg_ctr_merge("rr3-brain", &beat), "首拍即变化");
    // 同模块同键累加；另一模块同名键不互踩
    let mut beat2 = BTreeMap::new();
    beat2.insert("asr_ok".to_string(), 3u64);
    assert!(r.rr3_reg_ctr_merge("rr3-brain", &beat2));
    assert!(r.rr3_reg_ctr_merge("rr3-brain", &beat2));
    let mut beat3 = BTreeMap::new();
    beat3.insert("relay_on".to_string(), 1u64);
    assert!(r.rr3_reg_ctr_merge("rr3-act", &beat3));
    let t = r.rr3_reg_counters();
    assert_eq!(t["rr3-brain/asr_ok"], 8, "2+3+3 同键累加");
    assert_eq!(t["rr3-brain/asr_fail"], 1);
    assert_eq!(t["rr3-act/relay_on"], 1, "模块前缀隔离");
    // 空拍不算变化（调用方不落盘）
    assert!(!r.rr3_reg_ctr_merge("rr3-brain", &BTreeMap::new()));
    // rrd 自有键（boots）不走前缀
    r.rr3_reg_ctr_bump("boots", 1);
    r.rr3_reg_ctr_replace(BTreeMap::new());
    assert!(
        r.rr3_reg_counters().is_empty(),
        "replace 是启动装载入口，整体替换"
    );
}

/// 计数键数上界（外部输入防护）：满帽后拒新键、既有键照常累加、只点名一次。
#[test]
fn counters_key_cap_rejects_new_keys_keeps_old() {
    use std::collections::BTreeMap;
    let mut r = RrdRegistry::new(1000);
    let mut beat = BTreeMap::new();
    for i in 0..RR3_CTR_KEY_CAP {
        beat.insert(format!("k{i}"), 1u64);
    }
    assert!(r.rr3_reg_ctr_merge("m", &beat));
    assert_eq!(r.rr3_reg_counters().len(), RR3_CTR_KEY_CAP);
    let mut extra = BTreeMap::new();
    extra.insert("overflow".to_string(), 1u64);
    assert!(!r.rr3_reg_ctr_merge("m", &extra), "越界新键不产生变化");
    assert!(!r.rr3_reg_counters().contains_key("m/overflow"));
    let mut old = BTreeMap::new();
    old.insert("k0".to_string(), 5u64);
    assert!(r.rr3_reg_ctr_merge("m", &old), "既有键仍可累加");
    assert_eq!(r.rr3_reg_counters()["m/k0"], 6);
}

/// `--restart-base-ms 0` 的**现值**是 `delay_ms = 0`（第 1 档 1×0、第 2 档 2×0 都是 0，
/// 即"立即重启"）。本条只把**本表算术**的现值钉死，不裁下限：下界已由 Task 7 的 CLI 门禁
/// 拦住（`cli.rs::parse_restart_base` 拒 0，提交 f69eda8），所以配置面根本进不来 0。
/// 它挡的是有人"顺手"在延迟算术里给 base 补一个 `.max(1)` 之类的下界——
/// 那会让这里从 0 变成 1，而配置面看起来什么都没改。
#[test]
fn zero_restart_base_ms_currently_yields_zero_delay() {
    let mut r = RrdRegistry::new(0);
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    let p = r
        .rr3_reg_death("rr3-sima", false, 600)
        .rr3_plan()
        .expect("base=0 不改变是否给计划");
    assert_eq!((p.attempt, p.delay_ms), (1, 0), "现值：第 1 档 1×0 = 0");
    // 真实崩溃循环里两次死亡之间必隔一次重注册（换 task），这里照此补：
    // 新语义下对同一条 failed 条目连调 death 会被判 Silent，取不到第 2 档。
    r.rr3_reg_remove("rr3-sima");
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 650)
        .unwrap();
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 700)
            .rr3_plan()
            .expect("第 2 次仍有计划")
            .delay_ms,
        0,
        "现值：第 2 档 2×0 仍是 0（不是被补成 1ms）"
    );
}

/// 退避乘法是纯算术：base 大到会溢出时封顶，绝不 panic（debug 的 `*` 会 panic）
/// 也绝不回绕（release 的 `*` 会 wrap 成一个小延迟＝"疯狂重启"，正是 spec §5.3 要避免的）
#[test]
fn backoff_multiply_saturates_instead_of_wrapping() {
    let base = u64::MAX / 2;
    let mut r = RrdRegistry::new(base);
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 600)
            .rr3_plan()
            .expect("第 1 次有计划")
            .delay_ms,
        base
    );
    // 三次取档之间各补一次 remove + add（真实循环里每次死亡隔着一次重注册）：
    // 新语义对同一条 failed 条目连调 death 会判 Silent，跨重注册才是本表算术的真实形状。
    r.rr3_reg_remove("rr3-sima");
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 610)
        .unwrap();
    // 2×base 仍可表示，必须给出精确值（封顶实现若写成"一律 u64::MAX"在这里就红）
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 620)
            .rr3_plan()
            .expect("第 2 次有计划")
            .delay_ms,
        base.checked_mul(2).expect("前提：2×base 不溢出")
    );
    r.rr3_reg_remove("rr3-sima");
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 630)
        .unwrap();
    // 第 3 次是 4×base，溢出 → 封顶（回绕的话会拿到一个很小的数）
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 640)
            .rr3_plan()
            .expect("第 3 次有计划")
            .delay_ms,
        u64::MAX
    );
}

/// on-failure：干净退出不重启，非零/crash 才重启
#[test]
fn on_failure_only_restarts_unclean_exit() {
    let mut r = RrdRegistry::new(1000);
    r.rr3_reg_add(&facts("rr3-sense", 100, 500, "on-failure"), chan().0, 0)
        .unwrap();
    assert!(
        r.rr3_reg_death("rr3-sense", true, 600).rr3_plan().is_none(),
        "干净退出不得重启"
    );
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    assert!(r.rr3_reg_remove("rr3-sense"));
    r.rr3_reg_add(&facts("rr3-sense", 100, 500, "on-failure"), chan().0, 0)
        .unwrap();
    assert_eq!(
        r.rr3_reg_death("rr3-sense", false, 600)
            .rr3_plan()
            .expect("非零退出必须给计划")
            .delay_ms,
        1000
    );
}

/// 未知策略值不得 panic、也不得擅自重启（校验在 Task 2 已挡，运行期只保证不猜）
#[test]
fn unexpected_policy_string_is_treated_as_never() {
    let mut r = RrdRegistry::new(1000);
    let mut f = facts("rr3-sima", 100, 500, "always");
    f.restart_policy = "ALWAYS".into(); // 大小写错 = 非预期值
    r.rr3_reg_add(&f, chan().0, 0).unwrap();
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 600).rr3_plan(),
        None,
        "非预期值必须按 never 处理"
    );
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
}

/// 不在册的名字问死亡：None 且不得凭空造条目
#[test]
fn death_of_unknown_name_is_none() {
    let mut r = RrdRegistry::new(1000);
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    assert_eq!(r.rr3_reg_death("rr3-nobody", false, 600).rr3_plan(), None);
    assert_eq!(r.rr3_reg_snapshot().len(), 1);
    assert_eq!(
        r.rr3_reg_snapshot()[0].2,
        Rr3Phase::Registered,
        "问错名字不得改掉别人的状态"
    );
}

/// 退避表自证：7 档、末档 60、单调不减、无 0
#[test]
fn backoff_table_is_monotonic_with_cap() {
    assert_eq!(RR3_RESTART_BACKOFF.len(), 7);
    assert_eq!(*RR3_RESTART_BACKOFF.last().unwrap(), 60);
    assert!(RR3_RESTART_BACKOFF.windows(2).all(|w| w[0] <= w[1]));
    assert!(RR3_RESTART_BACKOFF.iter().all(|v| *v > 0));
}

/// failed 后同身份可重注册，但必须换新 task（旧号不复用）
#[test]
fn failed_entry_can_be_re_registered_with_new_task() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    let _ = r.rr3_reg_death("rr3-sima", false, 600);
    assert!(!r.rr3_reg_live("rr3-sima"), "failed 即无活连接");
    let t2 = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 700)
        .expect("failed 后必须可重注册");
    assert_eq!(t2, t + 1, "重注册必须换新 task 号");
    assert_eq!(r.rr3_reg_snapshot().len(), 1);
    // 旧号作废：迟到的旧心跳既不能喂到新条目身上，也不能改名存实亡的旧条目
    assert_eq!(r.rr3_reg_name_of(t), None, "旧 task 号必须从映射里摘掉");
    assert_eq!(
        r.rr3_reg_beat(t, 800),
        Rr3Beat::Rejected,
        "旧 task 号不得再被认成在册"
    );
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Registered);
    assert!(r.rr3_reg_live("rr3-sima"));
}

/// `rr3_reg_facts` 的现值钉住（Task 11 的 spawn 序靠它取 start_priority / entry）：
/// 在册名字必须原样回**入库时那份 facts**（逐字段相等，不是只回个名字），不在册的名字
/// 回 None——既不能 panic，也不能回一条半数据（空的 start_priority 会让 spawn 序排错位）。
#[test]
fn facts_roundtrip_for_known_and_unknown_names() {
    let mut r = RrdRegistry::new(1000);
    let f = facts("rr3-sima", 100, 500, "on-failure");
    r.rr3_reg_add(&f, chan().0, 0).unwrap();
    assert_eq!(
        r.rr3_reg_facts("rr3-sima").as_ref(),
        Some(&f),
        "取回的 facts 必须与入库的那份逐字段相同"
    );
    assert_eq!(r.rr3_reg_facts("rr3-nobody"), None, "不在册的名字必须 None");
}

/// fanout：跳过发起者；对端已 drop 只是少计，不 panic
#[test]
fn fanout_skips_sender_and_survives_dropped_peer() {
    let mut r = RrdRegistry::new(1000);
    let (txa, rxa) = chan();
    let (txb, rxb) = chan();
    let ta = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0)
        .unwrap();
    r.rr3_reg_add(&facts("rr3-simb", 100, 500, "always"), txb, 0)
        .unwrap();
    let frame = Rr3Frame {
        header: rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0),
        payload: vec![],
    };
    assert_eq!(r.rr3_reg_fanout(&frame, ta), 1, "只发给非发起者");
    assert!(rxa.try_recv().is_err());
    assert!(rxb.try_recv().is_ok());
    assert_eq!(
        r.rr3_reg_fanout(&frame, rr_bus::RR3_BROADCAST_DST_TASK),
        2,
        "0 不是任何模块的 task，两人都该收到"
    );
    assert!(rxa.try_recv().is_ok(), "上一行里发起者也不能被排除");
    drop(rxb);
    assert_eq!(r.rr3_reg_fanout(&frame, ta), 0);
}

/// 有界队列满侧（0.0.8b 审计 B-5 初版；本轮审计修正）：容量用满即**丢当前帧、
/// 保留连接**——返回 0、计数 +1；队列腾空后下一帧自动续投。旧行为置 tx=None
/// 会让模块在心跳仍通的情况下永久失聪（没有重连信号），播放期积压即可触发。
#[test]
fn fanout_drops_frames_but_keeps_slow_consumer() {
    let mut r = RrdRegistry::new(1000);
    let (tx, rx) = mpsc::sync_channel::<Rr3Frame>(1);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), tx, 0)
        .unwrap();
    let frame = Rr3Frame {
        header: rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0),
        payload: vec![],
    };
    assert_eq!(r.rr3_reg_fanout(&frame, t), 0, "发起者自己永远被排除");
    assert_eq!(
        r.rr3_reg_fanout(&frame, rr_bus::RR3_BROADCAST_DST_TASK),
        1,
        "第一帧入队"
    );
    assert_eq!(
        r.rr3_reg_fanout(&frame, rr_bus::RR3_BROADCAST_DST_TASK),
        0,
        "队列满 → 丢当前帧（不阻塞、不摘连接）"
    );
    assert_eq!(
        r.rr3_reg_fanout(&frame, rr_bus::RR3_BROADCAST_DST_TASK),
        0,
        "仍满继续丢，但连接保留"
    );
    assert!(rx.try_recv().is_ok(), "已入队的帧不丢");
    assert_eq!(
        r.rr3_reg_fanout(&frame, rr_bus::RR3_BROADCAST_DST_TASK),
        1,
        "队列腾空后自动续投（连接未被永久断投）"
    );
}

/// 到期后连接句柄被丢掉：之后 fanout 不再往死连接塞
#[test]
fn expired_module_is_excluded_from_fanout() {
    let mut r = RrdRegistry::new(1000);
    let (txa, rxa) = chan();
    let (txb, _rxb) = chan();
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0)
        .unwrap();
    let b = r
        .rr3_reg_add(&facts("rr3-simb", 100, 300, "always"), txb, 0)
        .unwrap();
    r.rr3_reg_expire(400);
    assert_eq!(r.rr3_reg_fanout(&frame_to(b), 0), 1, "只剩 sima 活着");
    assert!(rxa.try_recv().is_ok());
}

/// 移除后既不在快照里也不再是 fanout 目标，task 号也不回收
#[test]
fn removed_module_leaves_the_table_entirely() {
    let mut r = RrdRegistry::new(1000);
    let (txa, rxa) = chan();
    let a = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0)
        .unwrap();
    assert!(r.rr3_reg_remove("rr3-sima"));
    assert!(!r.rr3_reg_remove("rr3-sima"), "重复移除必须返回 false");
    assert!(!r.rr3_reg_live("rr3-sima"));
    assert_eq!(r.rr3_reg_name_of(a), None);
    assert!(r.rr3_reg_snapshot().is_empty());
    let frame = frame_to(rr_bus::RR3_BROADCAST_DST_TASK);
    assert_eq!(r.rr3_reg_fanout(&frame, 0), 0, "已移除的连接不得再被投递");
    assert!(rxa.try_recv().is_err(), "死条目还留着句柄就会被塞消息");
}

/// 真实崩溃循环：每次死亡之间隔着一次重注册（新 task）。今天必然红——
/// `rr3_reg_add` 把 `restarts` 归零，第 6 次仍给得出计划，5 次上限永远撞不到。
#[test]
fn crash_budget_survives_reregistration_and_caps_after_five_incarnations() {
    let mut r = RrdRegistry::new(1000);
    let f = || facts("rr3-sima", 100, 500, "always");
    let mut delays = Vec::new();
    for i in 0..RR3_RESTART_FAILURE_CAP {
        let t = r.rr3_reg_add(&f(), chan().0, i as u64 * 10).unwrap();
        let p = r
            .rr3_reg_death("rr3-sima", false, i as u64 * 10 + 5)
            .rr3_plan()
            .unwrap_or_else(|| panic!("第 {} 次崩溃应有计划", i + 1));
        assert_eq!(p.attempt, i + 1, "attempt 必须跨重注册连续增长");
        assert_eq!(p.name, "rr3-sima");
        assert_eq!(p.entry, "/usr/bin/rr3-sima");
        delays.push(p.delay_ms);
        assert!(!r.rr3_reg_live("rr3-sima"), "判死后不得算活");
        let _ = t;
    }
    assert_eq!(
        delays,
        vec![1000, 2000, 4000, 8000, 16000],
        "退避必须按表 × base，且序不因重注册被打乱"
    );
    let t = r.rr3_reg_add(&f(), chan().0, 99);
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 100).rr3_plan(),
        None,
        "达上限必须停止重启"
    );
    assert!(r.rr3_reg_hit_cap("rr3-sima"));
    assert!(
        !r.rr3_reg_hit_cap("rr3-nobody"),
        "没配过的名字不得误报达上限"
    );
    let _ = t;
}

/// 到期那一路也记账，且**只记一次**：判死后 conn 再收尾不得双计（ledger R-A4 的收口）
#[test]
fn expiry_charges_once_and_the_later_conn_death_is_silent() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), chan().0, 0)
        .unwrap();
    let _ = r.rr3_reg_beat(t, 100);
    let e = r.rr3_reg_expire(700).remove(0);
    assert_eq!((e.elapsed_ms, e.watchdog_ms), (600, 500));
    assert_eq!(
        e.restart.clone().map(|p| p.attempt),
        Some(1),
        "到期必须给出第 1 次重启计划"
    );
    assert_eq!(
        r.rr3_reg_death("rr3-sima", true, 800),
        Rr3Death::Silent,
        "已判死的条目不得再广播一次终态"
    );
    assert_eq!(e.restart.map(|p| p.delay_ms), Some(1000));
    assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 1, "双路只记一次账");
}

/// `never` 策略下到期不给计划——`Rr3Expiry.restart` 不是装饰
#[test]
fn expiry_of_a_never_module_reports_no_restart() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-simb", 100, 500, "never"), chan().0, 0)
        .unwrap();
    let _ = r.rr3_reg_beat(t, 100);
    assert_eq!(r.rr3_reg_expire(700).remove(0).restart, None);
}

/// 活过一整条窗口 = 上一世的账到此为止（spec"仍在 watchdog 超时内死亡"的正向写法）。
/// 两颗基准不许混：归零看 **registered_at_ms**（这一世活了多久），判死看 **last_beat_ms**
/// （多久没喂狗）。所以本条一直喂狗（永不判死），只在"注册后第 521ms"那一刻等归零。
#[test]
fn surviving_a_watchdog_window_clears_the_crash_budget() {
    let mut r = RrdRegistry::new(1000);
    let f = || facts("rr3-sima", 100, 500, "always");
    let t = r.rr3_reg_add(&f(), chan().0, 0).unwrap();
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 10)
            .rr3_plan()
            .map(|p| p.attempt),
        Some(1)
    );
    assert_eq!(
        r.rr3_reg_crash_streak("rr3-sima"),
        1,
        "第 1 世崩过，账上记着 1"
    );

    let t2 = r.rr3_reg_add(&f(), chan().0, 100).unwrap();
    assert_ne!(t2, t, "重注册必须换新 task，否则本条测的是同一世");
    for at in [120u64, 220, 320, 420, 520] {
        // 只有第一颗是升级；后面四颗必须是"只续命"（D-6 的另一半判据）
        let want = if at == 120 {
            Rr3Beat::Promoted
        } else {
            Rr3Beat::Kept
        };
        assert_eq!(r.rr3_reg_beat(t2, at), want, "第 {at}ms 的心跳相位回报不对");
        assert!(r.rr3_reg_expire(at + 1).is_empty(), "喂狗后 1ms 绝不许判死");
        if at < 520 {
            assert_eq!(
                r.rr3_reg_crash_streak("rr3-sima"),
                1,
                "没活满窗口之前不许归零"
            );
        }
    }
    // 注册于 100 + watchdog 500 → 归零时刻是 now_ms >= 600；620 那颗心跳之后第一次扫描到点
    assert!(r.rr3_reg_expire(621).is_empty(), "健康模块不该被扫死");
    assert_eq!(
        r.rr3_reg_crash_streak("rr3-sima"),
        0,
        "第 2 世已活满 521ms ≥ 500ms 窗口"
    );
    // 简报冲突（见实施报告）：第 2 世全程喂狗、健康存活，`rr3_reg_add` 对**在册非 Failed**
    // 的同名条目按 Duplicate 拒（这是既有不变量，`duplicate_name_is_rejected` 钉着，不许为
    // 本条让步）。故 t3 那句"第 3 世"必须先让第 2 世退场——最小忠实实现：insert 一行 remove
    // 把条目摘掉，再重注册。判据的语义（归零后第 3 世从 0 重计）不变。
    let _ = r.rr3_reg_remove("rr3-sima");
    let t3 = r.rr3_reg_add(&f(), chan().0, 700).unwrap();
    assert_eq!(
        r.rr3_reg_death("rr3-sima", false, 701)
            .rr3_plan()
            .map(|p| p.attempt),
        Some(1),
        "归零之后第 3 世从零重计（不是从 2 起步 = 4 秒退避）"
    );
    let _ = (t, t2, t3);
}

/// 所有权：owned 的连接断线时 conn 一路什么都不做（等退出码）
#[test]
fn owned_conn_death_defers_terminal_state_to_the_exit_status() {
    let mut r = RrdRegistry::new(1000);
    let f = facts("rr3-sima", 100, 500, "always");
    let t = r.rr3_reg_add(&f, chan().0, 0).unwrap();
    let _ = r.rr3_reg_beat(t, 10);
    r.rr3_reg_own("rr3-sima");
    assert_eq!(
        r.rr3_reg_death("rr3-sima", true, 20),
        Rr3Death::DeferredToExitStatus
    );
    assert_eq!(
        r.rr3_reg_snapshot()[0].2,
        Rr3Phase::Running,
        "deferred 不得改相位"
    );
    assert_eq!(r.rr3_reg_crash_streak("rr3-sima"), 0, "deferred 不得记账");
    // 收尸路接手：相位、句柄、账都在这一次到位
    match r.rr3_reg_child_exit(&f, false, 25) {
        Rr3ChildExit::Crashed {
            src_task,
            clean_exit,
            plan,
        } => {
            assert_eq!(src_task, t);
            assert!(!clean_exit);
            assert_eq!(plan.map(|p| p.attempt), Some(1));
        }
        other => panic!("退出码那一路必须给出 Crashed，实际 {other:?}"),
    }
    assert_eq!(r.rr3_reg_snapshot()[0].2, Rr3Phase::Failed);
    assert!(!r.rr3_reg_is_owned("rr3-sima"), "收过尸必须撤下所有权标记");
    // 撤标之后，同名新实例的 EOF 回到 conn 那一路（否则第二次崩溃会永远等一个不再来的退出码）
    let _t2 = r.rr3_reg_add(&f, chan().0, 30).unwrap();
    assert!(matches!(
        r.rr3_reg_death("rr3-sima", false, 40),
        Rr3Death::Broadcast { .. }
    ));
}

/// 两种到达次序都不许双计：到期先判死 vs conn 先 deferred
#[test]
fn death_and_expiry_in_either_order_charge_exactly_once() {
    let f = || facts("rr3-sima", 100, 500, "always");
    // (a) watchdog 先判死并记账，收尸后到 → 收尸必须是 Silent（R-A4 的封印就在这半条）
    let mut a = RrdRegistry::new(1000);
    let ta = a.rr3_reg_add(&f(), chan().0, 0).unwrap();
    let _ = a.rr3_reg_beat(ta, 10);
    a.rr3_reg_own("rr3-sima");
    assert_eq!(
        a.rr3_reg_expire(600).remove(0).restart.map(|p| p.attempt),
        Some(1)
    );
    assert!(
        matches!(a.rr3_reg_child_exit(&f(), false, 610), Rr3ChildExit::Silent),
        "已判死的条目被收尸不得再记一次、也不得再广播一次"
    );
    assert_eq!(a.rr3_reg_crash_streak("rr3-sima"), 1);
    // (b) conn 先 deferred（什么都不做），收尸后到 → 只有收尸记一次
    let mut b = RrdRegistry::new(1000);
    let tb = b.rr3_reg_add(&f(), chan().0, 0).unwrap();
    let _ = b.rr3_reg_beat(tb, 10);
    b.rr3_reg_own("rr3-sima");
    assert_eq!(
        b.rr3_reg_death("rr3-sima", true, 20),
        Rr3Death::DeferredToExitStatus
    );
    assert_eq!(
        b.rr3_reg_crash_streak("rr3-sima"),
        0,
        "deferred 一路不许先记账"
    );
    assert!(matches!(
        b.rr3_reg_child_exit(&f(), false, 25),
        Rr3ChildExit::Crashed { .. }
    ));
    assert_eq!(b.rr3_reg_crash_streak("rr3-sima"), 1);
}

/// 显式 unregister 之后，子进程退出码不得变成一次崩溃
#[test]
fn unregister_then_child_exit_does_not_charge() {
    let mut r = RrdRegistry::new(1000);
    let f = facts("rr3-sima", 100, 500, "always");
    let t = r.rr3_reg_add(&f, chan().0, 0).unwrap();
    let _ = r.rr3_reg_beat(t, 10);
    r.rr3_reg_own("rr3-sima");
    assert!(r.rr3_reg_remove("rr3-sima"), "unregister 必须真的摘掉条目");
    assert!(matches!(
        r.rr3_reg_child_exit(&f, true, 20),
        Rr3ChildExit::Silent
    ));
    assert_eq!(
        r.rr3_reg_crash_streak("rr3-sima"),
        0,
        "我让它停的，不是它崩的"
    );
    // 新实例重新登记 → stopped 让位（否则一个名字被 stop 过一次就永远起不来了）
    let t2 = r.rr3_reg_add(&f, chan().0, 30).unwrap();
    let _ = r.rr3_reg_beat(t2, 40);
    assert!(matches!(
        r.rr3_reg_death("rr3-sima", false, 50),
        Rr3Death::Broadcast { plan: Some(_) }
    ));
}

/// 从未注册的子进程：没有条目可广播，但账必须记（否则坏 entry 能把机器转着圈地重起）
#[test]
fn child_that_never_registered_is_charged_by_name_until_the_cap() {
    let mut r = RrdRegistry::new(1000);
    let f = facts("rr3-sima", 100, 500, "always");
    for i in 0..RR3_RESTART_FAILURE_CAP {
        match r.rr3_reg_child_exit(&f, false, i as u64 * 10) {
            Rr3ChildExit::EarlyExit { plan } => {
                assert_eq!(plan.map(|p| p.attempt), Some(i + 1))
            }
            other => panic!("第 {} 次应为 EarlyExit，实际 {other:?}", i + 1),
        }
        assert_eq!(r.rr3_reg_snapshot().len(), 0, "没注册过就不许凭空造条目");
    }
    assert!(matches!(
        r.rr3_reg_child_exit(&f, false, 999),
        Rr3ChildExit::EarlyExit { plan: None }
    ));
    assert!(r.rr3_reg_hit_cap("rr3-sima"));
}

/// 死因→phase 的映射只有这一处（conn 与 supervisor 共用）
#[test]
fn phase_for_exit_is_the_single_source_of_the_two_words() {
    assert_eq!(rr3_phase_for_exit(true), "unregistered");
    assert_eq!(rr3_phase_for_exit(false), "failed");
}

// ---------- 降级传播（ADR-0009 裁定 3/4）----------

/// 带 [depends].required 的 facts（wd 取 60s：判死只该发生在被点名的提供者身上）
fn facts_with_depends(name: &str, deps: &[&str]) -> Rr3ManifestFacts {
    let list = deps
        .iter()
        .map(|d| format!("\"{d}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = []

[depends]
required = [{list}]

[resources]
restart_policy = "always"

[health]
heartbeat_ms = 100
watchdog_timeout_ms = 60000
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

fn providers_of(pairs: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
    let mut m: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (cap, names) in pairs {
        m.entry(cap.to_string())
            .or_default()
            .extend(names.iter().map(|n| n.to_string()));
    }
    m
}

/// 唯一提供者死了（conn EOF 路）→ 依赖它的 Running 模块置 Degraded；
/// 心跳只续命不升级（Kept），降级不被一颗心跳抹平
#[test]
fn provider_death_degrades_running_dependent() {
    let mut r = RrdRegistry::new(20);
    r.rr3_reg_providers(providers_of(&[("cap.x", &["rr3-prov"])]));
    let p = facts("rr3-prov", 100, 5000, "always");
    let d = facts_with_depends("rr3-dep", &["cap.x"]);
    let (ptx, _) = chan();
    let (dtx, _) = chan();
    let pt = r.rr3_reg_add(&p, ptx, 0).expect("provider 入册");
    let dt = r.rr3_reg_add(&d, dtx, 0).expect("dependent 入册");
    assert_eq!(r.rr3_reg_beat(pt, 10), Rr3Beat::Promoted);
    assert_eq!(r.rr3_reg_beat(dt, 20), Rr3Beat::Promoted);

    // 提供者判死前传播必须空转（前置：死亡先落账）
    assert!(
        r.rr3_reg_degrade("rr3-prov").is_empty(),
        "死亡未落账时不得误判"
    );

    assert!(matches!(
        r.rr3_reg_death("rr3-prov", false, 30),
        Rr3Death::Broadcast { .. }
    ));
    assert_eq!(
        r.rr3_reg_degrade("rr3-prov"),
        vec![("rr3-dep".to_string(), dt)],
        "唯一提供者全灭 → 下游 Degraded"
    );
    let snap = r.rr3_reg_snapshot();
    assert!(
        snap.iter()
            .any(|(n, t, ph)| n == "rr3-dep" && *t == dt && *ph == Rr3Phase::Degraded),
        "{snap:?}"
    );
    // 降级后心跳：Kept（续命不升级），相位不被抹平
    assert_eq!(r.rr3_reg_beat(dt, 40), Rr3Beat::Kept);
    assert_eq!(
        r.rr3_reg_snapshot()
            .iter()
            .find(|(n, _, _)| n == "rr3-dep")
            .map(|x| x.2),
        Some(Rr3Phase::Degraded)
    );
}

/// G4：装配缺项的预降级——注册入表即 Degraded、心跳只续命（Kept）、
/// 能力传播解除不捞回（修数据 + 重启 rrd 是唯一解除路）
#[test]
fn predegraded_module_registers_degraded_and_is_not_rescued() {
    let mut r = RrdRegistry::new(20);
    r.rr3_reg_predegrade(["rr3-dep".to_string()]);
    r.rr3_reg_providers(providers_of(&[("cap.x", &["rr3-prov"])]));
    let (dtx, _) = chan();
    let dt = r
        .rr3_reg_add(&facts_with_depends("rr3-dep", &["cap.x"]), dtx, 0)
        .expect("入册");
    let (ptx, _) = chan();
    let pt = r
        .rr3_reg_add(&facts("rr3-prov", 100, 5000, "always"), ptx, 0)
        .expect("入册");
    // 注册即 Degraded；心跳 Kept 不升级
    assert_eq!(
        r.rr3_reg_snapshot()
            .iter()
            .find(|(n, _, _)| n == "rr3-dep")
            .map(|x| x.2),
        Some(Rr3Phase::Degraded),
        "装配缺项模块在册即 Degraded"
    );
    assert_eq!(r.rr3_reg_beat(dt, 10), Rr3Beat::Kept);
    // 提供者回 Running 触发解除传播：预降级不捞回
    assert_eq!(r.rr3_reg_beat(pt, 20), Rr3Beat::Promoted);
    assert!(
        r.rr3_reg_undegrade("rr3-prov").is_empty(),
        "能力全就位也不得把装配缺项的预降级捞回 Running"
    );
    assert_eq!(
        r.rr3_reg_snapshot()
            .iter()
            .find(|(n, _, _)| n == "rr3-dep")
            .map(|x| x.2),
        Some(Rr3Phase::Degraded)
    );
}

/// 多提供者"任一存活即满足"：只死一个不降级；两个全灭才降级
#[test]
fn any_surviving_provider_keeps_dependent_running() {
    let mut r = RrdRegistry::new(20);
    r.rr3_reg_providers(providers_of(&[("cap.x", &["rr3-p1", "rr3-p2"])]));
    let d = facts_with_depends("rr3-dep", &["cap.x"]);
    let (t1x, _) = chan();
    let (t2x, _) = chan();
    let (dtx, _) = chan();
    let t1 = r
        .rr3_reg_add(&facts("rr3-p1", 100, 5000, "always"), t1x, 0)
        .expect("p1 入册");
    let t2 = r
        .rr3_reg_add(&facts("rr3-p2", 100, 5000, "always"), t2x, 0)
        .expect("p2 入册");
    let dt = r.rr3_reg_add(&d, dtx, 0).expect("dep 入册");
    for t in [t1, t2, dt] {
        assert_eq!(r.rr3_reg_beat(t, 10), Rr3Beat::Promoted);
    }
    assert!(matches!(
        r.rr3_reg_death("rr3-p1", false, 20),
        Rr3Death::Broadcast { .. }
    ));
    assert!(r.rr3_reg_degrade("rr3-p1").is_empty(), "p2 还活着，不降级");
    assert!(matches!(
        r.rr3_reg_death("rr3-p2", false, 30),
        Rr3Death::Broadcast { .. }
    ));
    assert_eq!(
        r.rr3_reg_degrade("rr3-p2"),
        vec![("rr3-dep".to_string(), dt)]
    );
}

/// 解除的判据是"全部 required 就位"：只回来一个提供者不许解除
#[test]
fn provider_recovery_undegrades_only_when_all_deps_satisfied() {
    let mut r = RrdRegistry::new(20);
    r.rr3_reg_providers(providers_of(&[
        ("cap.x", &["rr3-px"]),
        ("cap.y", &["rr3-py"]),
    ]));
    let d = facts_with_depends("rr3-dep", &["cap.x", "cap.y"]);
    let (dtx, _) = chan();
    let dt = r.rr3_reg_add(&d, dtx, 0).expect("dep 入册");
    let promote = |r: &mut RrdRegistry, name: &str| -> u64 {
        let (tx, _) = chan();
        let t = r
            .rr3_reg_add(&facts(name, 100, 5000, "always"), tx, 0)
            .expect("入册");
        assert_eq!(r.rr3_reg_beat(t, 10), Rr3Beat::Promoted);
        t
    };
    let px = promote(&mut r, "rr3-px");
    promote(&mut r, "rr3-py");
    assert_eq!(r.rr3_reg_beat(dt, 20), Rr3Beat::Promoted);

    assert!(matches!(
        r.rr3_reg_death("rr3-px", false, 30),
        Rr3Death::Broadcast { .. }
    ));
    // 断供即降级（非对称设计的前半）：cap.x 已无任何在位提供者，dep 缺它就跑不了，
    // 不等 cap.y 也断——置位要快，解除才从严
    assert_eq!(
        r.rr3_reg_degrade("rr3-px"),
        vec![("rr3-dep".to_string(), dt)]
    );
    assert!(matches!(
        r.rr3_reg_death("rr3-py", false, 40),
        Rr3Death::Broadcast { .. }
    ));
    // 已经 Degraded 的不重复传播（Running 过滤器天然挡住）
    assert!(r.rr3_reg_degrade("rr3-py").is_empty(), "重复置位不是传播");

    // px 回来了（新 task 重注册 + Promoted），但 cap.y 还没好 → 不解除
    let px2 = promote(&mut r, "rr3-px");
    assert_ne!(px, px2, "重注册必须换新 task");
    assert!(
        r.rr3_reg_undegrade("rr3-px").is_empty(),
        "cap.y 仍无提供者在位"
    );
    assert_eq!(
        r.rr3_reg_snapshot()
            .iter()
            .find(|(n, _, _)| n == "rr3-dep")
            .map(|x| x.2),
        Some(Rr3Phase::Degraded)
    );

    // py 也回来了 → 全部就位 → 解除回 Running
    promote(&mut r, "rr3-py");
    assert_eq!(
        r.rr3_reg_undegrade("rr3-py"),
        vec![("rr3-dep".to_string(), dt)]
    );
    assert_eq!(
        r.rr3_reg_snapshot()
            .iter()
            .find(|(n, _, _)| n == "rr3-dep")
            .map(|x| x.2),
        Some(Rr3Phase::Running)
    );
}

// ---------- 全局低电旗（0.0.2 方案 Q4 裁定；解除路径批N5 件二笔3）----------

/// 沿语义：raise 首次返回 true（上升沿，conn 据此广播恰好一次 system/power），
/// 此后重复 raise 只回 false——同一次低电段不重复点名；lower 在旗已置位时返回
/// true（下降沿，conn 据此广播恰一次 battery-ok 解除点名），旗未置位时返回
/// false；解除后可再次升起（低电是可循环的 episodes，不是一次性熔断）。
#[test]
fn low_power_flag_rises_once_lowers_and_rises_again() {
    let mut r = RrdRegistry::new(1000);
    assert!(!r.rr3_reg_lp_is_set(), "初始未置位");
    assert!(r.rr3_reg_lp_raise(), "首次置位是上升沿");
    assert!(r.rr3_reg_lp_is_set());
    assert!(!r.rr3_reg_lp_raise(), "重复置位不再是上升沿");
    assert!(r.rr3_reg_lp_lower(), "旗已置位：lower 是真下降沿");
    assert!(!r.rr3_reg_lp_is_set(), "解除即出段");
    assert!(!r.rr3_reg_lp_lower(), "重复解除不再是下降沿");
    assert!(r.rr3_reg_lp_raise(), "解除后可再升起（episodes 语义）");
    assert!(r.rr3_reg_lp_is_set());
}

/// 小测试辅助：主题集
fn topic_set(entries: &[(&str, &str)]) -> Rr3TopicSet {
    use std::collections::BTreeSet;
    let mut owners: BTreeSet<&str> = BTreeSet::new();
    for (_, o) in entries {
        owners.insert(o);
    }
    let mut text = String::from("schema_version = 1\n");
    for o in &owners {
        text.push_str(&format!("\n[[owner]]\nname = \"{o}\"\n"));
    }
    for (t, o) in entries {
        text.push_str(&format!(
            "\n[[topic]]\nname = \"{t}\"\nkind = \"event\"\nowner = \"{o}\"\n"
        ));
    }
    let set = rr_registry::rr3_topic_parse(&text);
    assert!(
        set.rr3_topic_check().is_empty(),
        "测试主题集自身必须合法: {:?}",
        set.rr3_topic_check()
    );
    set
}

fn frame_to(dst: u64) -> Rr3Frame {
    let mut h = rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0);
    h.dst_task = dst;
    Rr3Frame {
        header: h,
        payload: vec![],
    }
}
