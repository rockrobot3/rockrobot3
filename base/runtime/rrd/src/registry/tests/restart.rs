// 重启记账：重启策略、退避表、计数前缀合并与键上限。
use super::*;

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
