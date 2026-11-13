// 心跳与到期：提升、显式单调毫秒、到期回报内容。
use super::*;

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
