//! control 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;
use alloc::vec::Vec;

/// 限速门：首拍必放；窗内连发被限；恰好到窗放行；窗从上次放行起算。
/// now 全部合成注入（原 rr3-act servo 用例语义随迁，时钟改 ms）。
#[test]
fn rate_gate_admits_first_blocks_within_window_reopens_after() {
    let mut gate = Rr3RateGate::rr3_gate_new(50);
    assert!(gate.rr3_gate_allow(1_000), "首拍必放");
    assert!(!gate.rr3_gate_allow(1_010));
    assert!(!gate.rr3_gate_allow(1_049));
    assert!(gate.rr3_gate_allow(1_050), "恰好到窗放行");
    assert!(!gate.rr3_gate_allow(1_060));
    assert!(gate.rr3_gate_allow(1_100), "窗从上次放行起算");
}

/// 低电边沿主语义：越阈第一拍恰报一次，段内后续拍不再报
/// （原 rr3-power battery 用例语义随迁）。
#[test]
fn low_edge_fires_once_per_episode() {
    let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
    assert!(!e.rr3_low_edge_cross(3900), "高电平不触发");
    assert!(e.rr3_low_edge_cross(3300), "恰好等于阈值算越阈（≤）");
    assert!(!e.rr3_low_edge_cross(3250), "段内第二拍不再报");
    assert!(!e.rr3_low_edge_cross(3200), "段内继续跌也不再报");
}

/// 严格大于才重新武装；武装本身不是事件（原 rr3-power 用例语义随迁）
#[test]
fn low_edge_rearms_only_above_threshold() {
    let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
    assert!(e.rr3_low_edge_cross(3290));
    assert!(!e.rr3_low_edge_cross(3300), "等于阈值不恢复");
    assert!(
        !e.rr3_low_edge_cross(3301),
        "严格大于才重新武装——武装本身不触发，否则发出高电压的'低电'事件"
    );
    assert!(!e.rr3_low_edge_cross(3301), "武装后高电平拍照旧不触发");
    assert!(e.rr3_low_edge_cross(3200), "重新武装已生效：再次越阈再报");
    assert!(!e.rr3_low_edge_cross(3200));
}

/// 恢复沿主语义（批N5 件二笔3）：低电段内不报、恰等阈值不报（仍在段内）、
/// 严格大于报恰一次（恢复首拍）、段内重报不存在——与 cross 的低电首拍对称：
/// 一次低电段 = 一个 battery/low + 一个 battery/ok。
#[test]
fn recover_fires_once_per_episode_strictly_above_threshold() {
    let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
    assert!(
        !e.rr3_low_edge_recover(3900),
        "武装态（未低电）没有恢复沿可言"
    );
    assert!(e.rr3_low_edge_cross(3290), "越阈进低电段");
    assert!(!e.rr3_low_edge_recover(3290), "段内不恢复");
    assert!(!e.rr3_low_edge_recover(3300), "恰等阈值仍是低电段内");
    assert!(e.rr3_low_edge_recover(3301), "严格大于阈值：恢复首拍");
    assert!(!e.rr3_low_edge_recover(3301), "恢复后不再报（armed 已回）");
    assert!(!e.rr3_low_edge_is_low(), "恢复即出段");
}

/// 喂入次序契约（钉 doc 那句"先 recover 后 cross"）：同拍先 cross 会把恢复沿
/// 吞掉（cross 的再武装分支置回 armed，recover 见 !armed 不成立而回 false）——
/// 先 recover 后 cross 才能既报出 battery/ok 又不破坏 cross 的判定。
#[test]
fn recover_before_cross_is_the_only_order_that_keeps_the_edge() {
    let thr = 3300;
    // 正确次序：先 recover（报沿）再 cross（armed 已回、无事发生）
    let mut good = Rr3LowEdge::rr3_low_edge_new(thr);
    assert!(good.rr3_low_edge_cross(3200));
    assert!(good.rr3_low_edge_recover(3400), "先 recover：恢复沿报出");
    assert!(!good.rr3_low_edge_cross(3400), "同拍 cross 不再有动作");
    // 错误次序（反例钉契约）：先 cross 吞沿——这正是调用方必须守的次序
    let mut bad = Rr3LowEdge::rr3_low_edge_new(thr);
    assert!(bad.rr3_low_edge_cross(3200));
    assert!(
        !bad.rr3_low_edge_cross(3400),
        "cross 静默再武装（不是事件）"
    );
    assert!(
        !bad.rr3_low_edge_recover(3400),
        "后 recover 已无沿可报——battery/ok 被吞，这就是次序契约存在的理由"
    );
}

/// 读出面不改判定（MC-5）：`is_low` = `armed` 反相，与 `cross` 同拍一致；
/// 严格大于阈值才解禁（回阈），恰等阈值仍在低电段内。
#[test]
fn low_edge_is_low_readout_matches_cross() {
    let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
    assert!(!e.rr3_low_edge_is_low(), "初态未低电");
    assert!(e.rr3_low_edge_cross(3300), "恰等阈值：越阈");
    assert!(e.rr3_low_edge_is_low(), "越阈后读出低电");
    assert!(!e.rr3_low_edge_cross(3300));
    assert!(e.rr3_low_edge_is_low(), "段内仍低");
    assert!(!e.rr3_low_edge_cross(3301), "严格大于才再武装");
    assert!(!e.rr3_low_edge_is_low(), "回阈后解禁读出");
}

/// 低电禁动闸门（MC-5 + 批N3 复位语义④）：复位即闭锁未武装——未武装解禁
/// 一律拒；武装后闭锁/解禁各恰一次翻转，重复同态不再翻转（去抖）。
#[test]
fn low_gate_trips_and_releases_with_single_flip() {
    let mut g = Rr3LowGate::rr3_low_gate_new();
    assert!(g.rr3_low_gate_blocked(), "复位即闭锁（§6 复位语义④）");
    assert!(!g.rr3_low_gate_release(), "未武装解禁被拒，保持闭锁");
    assert!(g.rr3_low_gate_blocked());
    g.rr3_low_gate_arm();
    assert!(g.rr3_low_gate_release(), "武装后解禁是真边沿");
    assert!(!g.rr3_low_gate_blocked());
    assert!(!g.rr3_low_gate_release(), "重复解禁不再翻转");
}

/// trip 撤销武装（复位语义④的核心）：让旗升起的轨压事实已过时，闭锁后
/// 不取新样本（re-arm）就解禁，闸门必须纹丝不动。
#[test]
fn low_gate_trip_disarms_until_fresh_sample() {
    let mut g = Rr3LowGate::rr3_low_gate_new();
    g.rr3_low_gate_arm();
    assert!(g.rr3_low_gate_release(), "先武装解禁（起手健康开局）");
    assert!(g.rr3_low_gate_trip(), "低电闭锁是真边沿");
    assert!(!g.rr3_low_gate_release(), "旧武装已随 trip 撤销：解禁被拒");
    assert!(g.rr3_low_gate_blocked(), "trip 撤了武装：闭锁保持");
    g.rr3_low_gate_arm();
    assert!(g.rr3_low_gate_release(), "闭锁后的新样本（re-arm）才解禁");
    assert!(!g.rr3_low_gate_blocked());
}

/// bare 本地兜底合成（MC-5 ④）：安全核段本地采样 → `Rr3LowEdge` 回阈 + 迟滞
/// 判定 → 闸门闭锁/解禁。判定纯函数、无时钟、无总线（裁定 5），
/// 与 host 档"消费广播置位"共用同一个 `Rr3LowGate`（一位状态，一个真值点）。
/// 每拍采样即样本（复位语义④的武装由喂入侧表达）：高电压拍 = arm + release。
#[test]
fn low_gate_composes_with_edge_for_bare_local_fallback() {
    let mut edge = Rr3LowEdge::rr3_low_edge_new(3300);
    let mut gate = Rr3LowGate::rr3_low_gate_new();
    // 安全核每一拍：feed 电压 → 把边沿状态镜像进闸门（低电闭锁；健康样本
    // 武装并解禁——未武装的 release 被 §6 复位语义④拒，故 arm 必须先行）
    let mut feed = |mv: u16| {
        edge.rr3_low_edge_cross(mv);
        if edge.rr3_low_edge_is_low() {
            gate.rr3_low_gate_trip();
        } else {
            gate.rr3_low_gate_arm();
            gate.rr3_low_gate_release();
        }
        gate.rr3_low_gate_blocked()
    };
    assert!(!feed(3900), "高电压：开（首拍样本即武装）");
    assert!(feed(3300), "越阈：闭锁（拒动）");
    assert!(feed(3250), "段内继续跌落：仍闭锁");
    assert!(!feed(3301), "回阈（严格大于）：解禁");
}

/// 节拍方案三例（原 rr3-act reflex 用例语义随迁）
#[test]
fn window_plan_derives_window_need_and_expect() {
    let cfg = Rr3ReflexCfg {
        loop_hz: 1000,
        saturate_notice_ms: 500,
    };
    assert_eq!(rr3_reflex_plan(&cfg), (250, 2, 250));
    let cfg = Rr3ReflexCfg {
        loop_hz: 1000,
        saturate_notice_ms: 100,
    };
    assert_eq!(rr3_reflex_plan(&cfg), (100, 1, 100));
    let cfg = Rr3ReflexCfg {
        loop_hz: 100,
        saturate_notice_ms: 500,
    };
    assert_eq!(rr3_reflex_plan(&cfg), (250, 2, 25));
}

/// episode 语义：连续 need 窗饱和才翻 true 且只翻一次；非饱和窗复位
/// （原 rr3-act reflex 用例语义随迁）
#[test]
fn detector_fires_once_per_saturation_episode() {
    let mut det = Rr3SaturationDetector::rr3_detector_new(2);
    assert!(!det.rr3_detector_feed(true), "第 1 个饱和窗：还在积累");
    assert!(
        det.rr3_detector_feed(true),
        "第 2 个连续饱和窗：episode 首报"
    );
    assert!(!det.rr3_detector_feed(true), "episode 内不重复");
    assert!(!det.rr3_detector_feed(true), "episode 内不重复（继续）");
    assert!(!det.rr3_detector_feed(false), "恢复窗复位且不发");
    assert!(!det.rr3_detector_feed(true), "新 episode 重新积累");
    assert!(det.rr3_detector_feed(true), "新 episode 再次首报");
}

/// 孤立饱和窗夹恢复窗永不触发——抖动不是饱和
#[test]
fn isolated_saturated_window_never_fires() {
    let mut det = Rr3SaturationDetector::rr3_detector_new(2);
    for _ in 0..10 {
        assert!(!det.rr3_detector_feed(true));
        assert!(!det.rr3_detector_feed(false));
    }
}

/// 窗结算判定与速率折算的边界（半拍线 / 零除防御）
#[test]
fn window_saturated_and_rate_hz_boundaries() {
    assert!(!rr3_reflex_saturated(125, 250), "恰半拍不算饱和");
    assert!(rr3_reflex_saturated(124, 250), "低于半拍即饱和");
    assert!(rr3_reflex_saturated(0, 250));
    assert_eq!(rr3_reflex_rate_hz(250, 250), 1000);
    assert_eq!(rr3_reflex_rate_hz(0, 250), 0);
    assert_eq!(rr3_reflex_rate_hz(10, 0), 0, "window_ms=0 不炸除零");
}

/// 限位钳位：界内恒等 / 钳两端 / 端点颠倒收敛且饱和到最保守端
#[test]
fn travel_clamps_into_range_and_survives_inverted_range() {
    assert_eq!(rr3_travel_clamp(90, 0, 180), 90, "界内恒等");
    assert_eq!(rr3_travel_clamp(0, 0, 180), 0, "下界恒等");
    assert_eq!(rr3_travel_clamp(180, 0, 180), 180, "上界恒等");
    assert_eq!(rr3_travel_clamp(5, 45, 135), 45, "钳到下界");
    assert_eq!(rr3_travel_clamp(200, 45, 135), 135, "钳到上界");
    assert_eq!(rr3_travel_clamp(10, 135, 45), 45, "颠倒行程收敛不 panic");
    assert_eq!(
        rr3_travel_clamp(200, 135, 45),
        135,
        "颠倒行程仍收敛到有效区间 [45,135]：200 钳到最近端点"
    );
}

/// 逐拍一致性（批D₃ 判据：同一控制图注入时钟逐拍一致）——同一张图
/// （限速门 + 限位钳位 + 低电边沿）喂两遍同一合成时钟序列，输出序列
/// 逐拍相等：机制核纯函数无隐藏状态、无隐藏时钟，Linux 环与裸机定时器
/// 跑同一图必然同拍。
#[test]
fn same_control_graph_beat_for_beat_identical() {
    // 控制图：第 k 拍时刻 now=1000+10k，角度在 [0,90,180,270] 轮转，
    // 电压在 [3400, 3290, 3310, 3200] 轮转
    let run = || {
        let mut gate = Rr3RateGate::rr3_gate_new(50);
        let mut edge = Rr3LowEdge::rr3_low_edge_new(3300);
        let mut out = Vec::new();
        for k in 0..24u64 {
            let now = 1_000 + 10 * k;
            let angle = rr3_travel_clamp([0u16, 90, 180, 270][k as usize % 4], 0, 180);
            let published = gate.rr3_gate_allow(now);
            let low = edge.rr3_low_edge_cross([3400u16, 3290, 3310, 3200][k as usize % 4]);
            out.push((now, angle, published, low));
        }
        out
    };
    let a = run();
    let b = run();
    assert_eq!(a, b, "同一图同一输入必须逐拍一致");
    // 图本身的行为抽查：限速 50ms/拍 10ms → 每 5 拍放 1 次；低电 3290 首报、
    // 3310（>3300）重新武装、3200 再报
    assert!(a[0].2);
    assert!(!a[1].2);
    assert!(a[5].2, "1_050 距 1_000 恰 50ms，放行");
    assert!(a[1].3, "3290 越阈首报");
    assert!(!a[2].3, "3310 重新武装但不是事件");
    assert!(a[3].3, "3200 再次越阈再报");
}
