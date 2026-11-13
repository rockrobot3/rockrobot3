// 死亡/收尸/崩溃预算：与到期互斥收费、重注册后的预算延续。
use super::*;

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
