// 降级与恢复：能力提供者全灭/回归、依赖饱和、事实回读。
use super::*;

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
