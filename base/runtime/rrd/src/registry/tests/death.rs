// SPDX-License-Identifier: Apache-2.0
// 死亡/收尸/崩溃预算：与到期互斥收费、重注册后的预算延续。
use super::*;

/// fanout：跳过发起者；对端已 drop 只是少计，不 panic
#[test]
fn fanout_skips_peer() {
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
    assert_eq!(r.rr3_reg_fanout(frame.clone(), ta), 1, "只发给非发起者");
    assert!(rxa.try_recv().is_err());
    assert!(rxb.try_recv().is_ok());
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), rr_bus::RR3_BROADCAST_DST_TASK),
        2,
        "0 不是任何模块的 task，两人都该收到"
    );
    assert!(rxa.try_recv().is_ok(), "上一行里发起者也不能被排除");
    drop(rxb);
    assert_eq!(r.rr3_reg_fanout(frame.clone(), ta), 0);
}

/// 有界队列满侧（0.0.8b 审计 B-5 初版；本轮审计修正）：容量用满即**丢当前帧、
/// 保留连接**——返回 0、计数 +1；队列腾空后下一帧自动续投。旧行为置 tx=None
/// 会让模块在心跳仍通的情况下永久失聪（没有重连信号），播放期积压即可触发。
#[test]
fn fanout_but_consumer() {
    let mut r = RrdRegistry::new(1000);
    let (tx, rx) = mpsc::sync_channel::<Rr3FanFrame>(1);
    let t = r
        .rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), tx, 0)
        .unwrap();
    let frame = Rr3Frame {
        header: rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0),
        payload: vec![],
    };
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), t),
        0,
        "发起者自己永远被排除"
    );
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), rr_bus::RR3_BROADCAST_DST_TASK),
        1,
        "第一帧入队"
    );
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), rr_bus::RR3_BROADCAST_DST_TASK),
        0,
        "队列满 → 丢当前帧（不阻塞、不摘连接）"
    );
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), rr_bus::RR3_BROADCAST_DST_TASK),
        0,
        "仍满继续丢，但连接保留"
    );
    assert_eq!(
        r.rr3_reg_fan_dropped(),
        2,
        "积压丢帧总数必须可读（ctl.status 的 fan_dropped 列）"
    );
    assert_eq!(
        r.rr3_reg_health(0)[0].fan_dropped,
        2,
        "health 行必须点名慢消费者（不能只有一个全局总数）"
    );
    // 锁内只收集、放锁后落盘（续轮十七）：首次丢帧点名一行，取走即空
    let logs = r.rr3_logs_take();
    assert_eq!(logs.len(), 1, "每 64 帧点名一次：两次丢帧只应收集一行");
    assert!(
        logs[0].contains("广播积压丢帧 rr3-sima"),
        "待打印行必须点名慢消费者: {logs:?}"
    );
    assert!(r.rr3_logs_take().is_empty(), "取走即清空，不得重复打印");
    assert!(rx.try_recv().is_ok(), "已入队的帧不丢");
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), rr_bus::RR3_BROADCAST_DST_TASK),
        1,
        "队列腾空后自动续投（连接未被永久断投）"
    );
}

/// 到期后连接句柄被丢掉：之后 fanout 不再往死连接塞
#[test]
fn expired_fanout() {
    let mut r = RrdRegistry::new(1000);
    let (txa, rxa) = chan();
    let (txb, _rxb) = chan();
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0)
        .unwrap();
    let b = r
        .rr3_reg_add(&facts("rr3-simb", 100, 300, "always"), txb, 0)
        .unwrap();
    r.rr3_reg_expire(400);
    assert_eq!(r.rr3_reg_fanout(frame_to(b), 0), 1, "只剩 sima 活着");
    assert!(rxa.try_recv().is_ok());
}

/// 收件端消失（writer 线程写超时收摊 → 通道 drop）→ 该连接**当场摘出投递面**：
/// 投递数不再计入、`tx` 置空（`rr3_reg_live` 转假），活连接不受牵连。
/// 这是"慢消费者不拖累全局"的最后一段闭环——写侧只负责让 writer 退出，
/// 回收由本分支完成；缺了它，扇出会一直往一条没人读的通道里塞帧并记 `fan_dropped`。
#[test]
fn fanout_drops_dead_writer() {
    let mut r = RrdRegistry::new(1000);
    let (txa, _rxa) = chan();
    let (txb, rxb) = chan();
    r.rr3_reg_add(&facts("rr3-sima", 100, 500, "always"), txa, 0)
        .unwrap();
    r.rr3_reg_add(&facts("rr3-simb", 100, 500, "always"), txb, 0)
        .unwrap();
    drop(rxb);
    let frame = frame_to(rr_bus::RR3_BROADCAST_DST_TASK);
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), rr_bus::RR3_BROADCAST_DST_TASK),
        1,
        "死连接不得计入投递数"
    );
    assert!(
        !r.rr3_reg_live("rr3-simb"),
        "Disconnected 必须把 tx 置空（此后不再投递）"
    );
    assert!(r.rr3_reg_live("rr3-sima"), "活连接不受牵连");
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), rr_bus::RR3_BROADCAST_DST_TASK),
        1,
        "置空后第二帧仍只投活连接"
    );
    assert_eq!(
        r.rr3_reg_fan_dropped(),
        0,
        "死连接的帧不算积压丢帧（两者是不同故障）"
    );
}

/// 移除后既不在快照里也不再是 fanout 目标，task 号也不回收
#[test]
fn removed_entirely() {
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
    assert_eq!(
        r.rr3_reg_fanout(frame.clone(), 0),
        0,
        "已移除的连接不得再被投递"
    );
    assert!(rxa.try_recv().is_err(), "死条目还留着句柄就会被塞消息");
}

/// 真实崩溃循环：每次死亡之间隔着一次重注册（新 task）。今天必然红——
/// `rr3_reg_add` 把 `restarts` 归零，第 6 次仍给得出计划，5 次上限永远撞不到。
#[test]
fn crash_caps_five_inc() {
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

/// 达上限不是"只打一行日志"：health 行必须把崩溃连击与终态一起回显，
/// 外部（ctl/面板/运维脚本）才能把"永久降级"与"正在重启"区分开。
#[test]
fn health_restart_cap() {
    let mut r = RrdRegistry::new(1000);
    let f = || facts("rr3-sima", 100, 500, "always");
    for i in 0..RR3_RESTART_FAILURE_CAP {
        let _t = r.rr3_reg_add(&f(), chan().0, i as u64 * 10).unwrap();
        let _ = r.rr3_reg_death("rr3-sima", false, i as u64 * 10 + 5);
    }
    let rows = r.rr3_reg_health(100);
    let row = rows.iter().find(|x| x.name == "rr3-sima").expect("在册行");
    assert_eq!(row.crash_streak, RR3_RESTART_FAILURE_CAP);
    assert!(row.restart_exhausted, "达上限必须可读为终态");
}

/// 到期那一路也记账，且**只记一次**：判死后 conn 再收尾不得双计（ledger R-A4 的收口）
#[test]
fn expiry_conn_silent() {
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
fn expiry_no_restart() {
    let mut r = RrdRegistry::new(1000);
    let t = r
        .rr3_reg_add(&facts("rr3-simb", 100, 500, "never"), chan().0, 0)
        .unwrap();
    let _ = r.rr3_reg_beat(t, 100);
    assert_eq!(r.rr3_reg_expire(700).remove(0).restart, None);
}

fn charge_first_crash(r: &mut RrdRegistry) {
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
}

fn survive_second_life(r: &mut RrdRegistry, t2: u64) {
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
}

/// 活过一整条窗口 = 上一世的账到此为止（spec"仍在 watchdog 超时内死亡"的正向写法）。
/// 两颗基准不许混：归零看 **registered_at_ms**（这一世活了多久），判死看 **last_beat_ms**
/// （多久没喂狗）。所以本条一直喂狗（永不判死），只在"注册后第 521ms"那一刻等归零。
#[test]
fn surviving_crash_budg() {
    let mut r = RrdRegistry::new(1000);
    let f = || facts("rr3-sima", 100, 500, "always");
    let t = r.rr3_reg_add(&f(), chan().0, 0).unwrap();
    charge_first_crash(&mut r);

    let t2 = r.rr3_reg_add(&f(), chan().0, 100).unwrap();
    assert_ne!(t2, t, "重注册必须换新 task，否则本条测的是同一世");
    survive_second_life(&mut r, t2);
    // 简报冲突（见实施报告）：第 2 世全程喂狗、健康存活，`rr3_reg_add` 对**在册非 Failed**
    // 的同名条目按 Duplicate 拒（这是既有不变量，`duplicate_name_rej` 钉着，不许为
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
fn owned_exit_status() {
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
fn death_order_once() {
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
fn unregister_charge() {
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
fn child_reg_name_cap() {
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
fn phase_exit_two_words() {
    assert_eq!(rr3_phase_for_exit(true), "unregistered");
    assert_eq!(rr3_phase_for_exit(false), "failed");
}
