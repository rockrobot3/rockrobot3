// 注册面：重名/保留名/容量/看门狗断言/panic 留条目。
use super::*;

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
