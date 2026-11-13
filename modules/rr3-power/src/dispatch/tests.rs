//! dispatch 的单测（自同名单测模块原样抽出，判据不变）。

use super::*;

fn st(mv: u16, percent: u8) -> Rr3BatteryState {
    Rr3BatteryState {
        voltage_mv: mv,
        percent,
        charging: false,
    }
}

#[test]
fn state_body_carries_full_summary_with_own_seq() {
    let v = rr3_battery_body(7, st(3700, 50));
    assert_eq!(v["seq"], 7);
    assert_eq!(v["voltage_mv"], 3700);
    assert_eq!(v["percent"], 50);
    assert_eq!(v["charging"], false);
}

/// battery/low 体与 schema 逐字段对齐：三必填，且**没有**多余字段
/// （additionalProperties=false，多带 charging 会直接违反契约）。
#[test]
fn low_body_matches_schema_exactly() {
    let v = rr3_battery_low_body(1, st(3250, 5));
    assert_eq!(v["seq"], 1);
    assert_eq!(v["percent"], 5);
    assert_eq!(v["voltage_mv"], 3250);
    assert_eq!(
        v.as_object().unwrap().len(),
        3,
        "schema additionalProperties=false，字段超集即违约: {v}"
    );
}

/// battery/ok 体与 schema 逐字段对齐（批N5 件二笔3）：三必填，且**没有**多余
/// 字段——与 low 同形状（additionalProperties=false）。
#[test]
fn ok_body_matches_schema_exactly() {
    let v = rr3_battery_ok_body(2, st(3310, 8));
    assert_eq!(v["seq"], 2);
    assert_eq!(v["percent"], 8);
    assert_eq!(v["voltage_mv"], 3310);
    assert_eq!(
        v.as_object().unwrap().len(),
        3,
        "schema additionalProperties=false，字段超集即违约: {v}"
    );
}

#[test]
fn charger_body_carries_own_seq_and_charging() {
    let v = rr3_charger_body(3, true);
    assert_eq!(v["seq"], 3);
    assert_eq!(v["charging"], true);
    assert_eq!(v.as_object().unwrap().len(), 2, "两字段，不加戏: {v}");
}

// ---------- 两腿对拍用例（0.0.5 批K H1，architecture §4.0.3 判据②） ----------
// battery/charger 的载荷 body fn 随 0.0.8 批1 拆分落 dispatch.rs（原 composite.rs；
// 对拍计数门扫描面随批N26 件二扩四路），用例随载荷落本文件。

#[test]
fn parity_leg1_battery_output_train_is_deterministic() {
    // 腿一·确定性：同一电压序列喂两跑，(state 体, low 触发) 输出轨迹逐拍相等
    fn run() -> Vec<(Value, bool)> {
        let mut edge = Rr3LowEdge::rr3_low_edge_new(3300);
        [3700u16, 3310, 3300, 3290, 3295, 3400, 3299]
            .iter()
            .map(|&mv| {
                let st = Rr3BatteryState {
                    voltage_mv: mv,
                    percent: 50,
                    charging: false,
                };
                let low = edge.rr3_low_edge_cross(mv);
                (rr3_battery_body(1, st), low)
            })
            .collect()
    }
    assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
}

#[test]
fn parity_leg2_battery_matches_ruling_semantics() {
    // 腿二·正确性：低电边沿裁定语义独立重写（真值 = rr-core Rr3LowEdge）
    let mut e = Rr3LowEdge::rr3_low_edge_new(3300);
    assert!(!e.rr3_low_edge_cross(3301), "严格大于阈值：不触发");
    assert!(e.rr3_low_edge_cross(3300), "等号算低：v == threshold 触发");
    assert!(!e.rr3_low_edge_cross(3290), "段内不重发（武装已解除）");
    assert!(!e.rr3_low_edge_cross(3300), "段内等号也不重发");
    assert!(!e.rr3_low_edge_cross(3310), "回充不返回 true（只是再武装）");
    assert!(e.rr3_low_edge_cross(3299), "再武装后再次越阈才再发");
    // state 体四字段、low 体三必填无加戏（schema additionalProperties=false）
    let v = rr3_battery_body(9, st(3700, 50));
    assert_eq!(v["seq"], 9);
    assert_eq!(v["voltage_mv"], 3700);
    assert_eq!(v["percent"], 50);
    assert_eq!(v["charging"], false);
    assert_eq!(v.as_object().unwrap().len(), 4, "state 四字段: {v}");
    let low = rr3_battery_low_body(1, st(3300, 10));
    assert_eq!(low["percent"], 10);
    assert_eq!(low["voltage_mv"], 3300);
    assert_eq!(low.as_object().unwrap().len(), 3, "low 三必填无加戏: {low}");
}

#[test]
fn parity_leg1_charger_output_train_is_deterministic() {
    // 腿一·确定性：同一 charging 派生序列两跑，(seq, charging) 流逐拍相等
    fn run() -> Vec<(u64, bool)> {
        let mut seq = 0;
        [true, true, false, false, true]
            .iter()
            .map(|&charging| {
                seq += 1;
                (
                    seq,
                    rr3_charger_body(seq, charging)["charging"]
                        .as_bool()
                        .unwrap(),
                )
            })
            .collect()
    }
    assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
}

#[test]
fn parity_leg2_charger_matches_ruling_semantics() {
    // 腿二·正确性：seq 本流自计数（与 battery/state 的 seq 独立）+ 两字段体
    // + 派生语义（charging 位来自最近一次 battery 读数，body 原样透传）
    let v = rr3_charger_body(3, true);
    assert_eq!(v["seq"], 3);
    assert_eq!(v["charging"], true, "charging 位是 battery 读数的派生透传");
    assert_eq!(v.as_object().unwrap().len(), 2, "两字段不加戏: {v}");
    assert_ne!(
        rr3_charger_body(1, false)["charging"],
        rr3_charger_body(2, true)["charging"],
        "派生值随读数变，不受 seq 影响"
    );
}

// ---------- 轨压档位对拍（批N2 件四，⑥-c2 真值面） ----------
// rr_hal_api 的 rr3_rail_tier_fold 是契约层纯函数（无原子件 mech.rs，⑥-c2
// 判据单真值点），对拍随其运行期唯一消费模块 rr3-power 落本文件。power 不是
// atom 名（本模块原子 = battery/charger），⑧-a 计数门不数这两条（不占原子门
// 分母，命名前缀照模块名走）。

#[test]
fn parity_leg1_power_rail_tier_effective_train_is_deterministic() {
    // 腿一·确定性：同一 (档位, 龄期, 上界) 合成序列喂两跑，输出档位序列逐拍相等
    use rr_hal_api::{rr3_rail_tier_fold, Rr3RailTier};
    fn run() -> Vec<Rr3RailTier> {
        let max = 250u32;
        [
            (Rr3RailTier::Ok, Some(0u32)),
            (Rr3RailTier::Ok, Some(max)),     // 边界：不折
            (Rr3RailTier::Ok, Some(max + 1)), // 越上界：折
            (Rr3RailTier::Mild, Some(max)),
            (Rr3RailTier::Mild, Some(max + 1)),
            (Rr3RailTier::Severe, Some(9)),
            (Rr3RailTier::Severe, None),     // 取不到基准：折
            (Rr3RailTier::Unknown, Some(0)), // 已是 Unknown：原样
            (Rr3RailTier::Unknown, None),
        ]
        .iter()
        .map(|&(tier, age)| rr3_rail_tier_fold(tier, age, max))
        .collect()
    }
    assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
}

#[test]
fn parity_leg2_power_rail_tier_effective_matches_ruling_semantics() {
    // 腿二·正确性：裁定语义逐条点名（真值 = rr_hal_api rr3_rail_tier_fold，
    // 全案 §6 T-6 运行期读数可信度 + Y-4 如实申报）
    use rr_hal_api::{rr3_rail_tier_fold, Rr3RailTier};
    let max = 250u32;
    // 取不到基准（None）⇒ Unknown，与档位无关——新鲜 Ok 也折不出"可信"
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Ok, None, max),
        Rr3RailTier::Unknown,
        "时基取不到基准 ⇒ Unknown（Y-4：None 是如实申报，不许编龄期冒充新鲜）"
    );
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Severe, None, max),
        Rr3RailTier::Unknown,
        "None 折档与输入档位无关"
    );
    // 越上界 ⇒ Unknown：陈旧的 Severe 与新鲜的 Severe 必须可区分（T-6）
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Severe, Some(max + 1), max),
        Rr3RailTier::Unknown,
        "age > max ⇒ Unknown（陈旧 Severe ≠ 新鲜 Severe，T-6）"
    );
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Ok, Some(max + 1), max),
        Rr3RailTier::Unknown
    );
    // 边界 age == max 不折（新鲜度上界是闭区间内）；龄期内原样透传
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Severe, Some(max), max),
        Rr3RailTier::Severe,
        "age == max 边界不折（闭区间内）"
    );
    assert_eq!(
        rr3_rail_tier_fold(Rr3RailTier::Ok, Some(0), max),
        Rr3RailTier::Ok,
        "新鲜读数原样透传"
    );
    // 档名串同源：第四态 "unknown" 不冒充 "severe"（守护输出行 INFO rail_tier=
    // 与板侧 RailTier::name 同一词表——展示面原样的数据基础）
    assert_eq!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "unknown");
    assert_ne!(Rr3RailTier::Unknown.rr3_rail_tier_name(), "severe");
    // 消费纪律的类型基础（本函数只折档不判处置，执法归⑥-c2 消费侧）：Unknown
    // 是独立第四态（执行面 match 得到独立臂按 Severe 那一侧处置），且折档结果
    // 不许再折成 Ok（展示面原样）
    assert_ne!(
        rr3_rail_tier_fold(Rr3RailTier::Severe, None, max),
        Rr3RailTier::Severe,
        "折出的 Unknown 与 Severe 可区分（执行面独立处置的判据基础）"
    );
    assert_ne!(
        rr3_rail_tier_fold(Rr3RailTier::Severe, None, max),
        Rr3RailTier::Ok,
        "Unknown 不许折算成 Ok（展示面原样不折）"
    );
}

// ---------- 轨压监测归属搬家用例（批N2 件五，P-9：机制不动、归属搬家） ----------
// 判定核切片（rr3_rail_tier_of / Rr3RailJudge）与 ⑥-c2 消费执法点（rr3_rail_beat）
// 的用例落本文件；板侧 tools/esp32-periph-base 的 tier_of/RailJudge 不改，语义逐字对照。

#[test]
fn rail_state_body_carries_four_keys() {
    let v = rr3_rail_state_body(5, 4500, 120, "mild");
    assert_eq!(v["seq"], 5);
    assert_eq!(v["mv"], 4500);
    assert_eq!(v["age_ms"], 120);
    assert_eq!(v["tier"], "mild");
    assert_eq!(v.as_object().unwrap().len(), 4, "四键不加戏: {v}");
}

#[test]
fn rail_judge_feed_matches_board_semantics() {
    // 板侧 RailJudge::feed 逐字对照（mild=4600/severe=4400）：档位 = tier_of 纯
    // mV 分类，event = 两线任一低电段首拍。4500 对 severe 线是"严格回阈 ⇒ 再
    // 武装"（Rr3LowEdge 语义），故 4300 是新段首发再报——与"向下档变 ⇔ 边沿
    // 事件"一致，不是同段重发。
    let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
    let seq = [
        (4700u16, Rr3RailTier::Ok, false),
        (4600, Rr3RailTier::Mild, true),   // 轻度线首发（Ok→Mild）
        (4500, Rr3RailTier::Mild, false),  // 段内不重发
        (4400, Rr3RailTier::Severe, true), // 重度线首发（Mild→Severe）
        (4500, Rr3RailTier::Mild, false),  // 回阈不报（severe 线再武装）
        (4300, Rr3RailTier::Severe, true), // severe 新段首发（4500 已严格回阈）
        (4700, Rr3RailTier::Ok, false),    // 回 Ok（mild 线再武装）
        (4600, Rr3RailTier::Mild, true),   // 再武装后越阈再发
    ];
    for (mv, want_tier, want_event) in seq {
        let (tier, event) = j.rr3_rail_feed(mv);
        assert_eq!(tier, want_tier, "mv={mv} 档位");
        assert_eq!(event, want_event, "mv={mv} 事件");
    }
}

#[test]
fn parity_leg1_power_rail_judge_train_is_deterministic() {
    // 腿一·确定性：同一 mV 序列喂两跑，(档位, 事件) 轨迹逐拍相等
    fn run() -> Vec<(Rr3RailTier, bool)> {
        let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
        [4700u16, 4600, 4500, 4400, 4500, 4300, 4700, 4600]
            .iter()
            .map(|&mv| j.rr3_rail_feed(mv))
            .collect()
    }
    assert_eq!(run(), run(), "同输入序列两跑逐拍相等（腿一·确定性）");
}

#[test]
fn parity_leg2_power_rail_judge_edge_equivalent_to_downward_transition() {
    // 腿二·正确性：边沿事件 ⇔ 向下档变（板侧 RailJudge 类型注的逐拍等价性）。
    // 档序 Ok=0/Mild=1/Severe=2，prev 初值 Ok：event ⇔ rank 增；回阈（rank 降）
    // 不报。与 rail_judge_feed_matches_board_semantics 同一序列、判据独立重写。
    fn rank(t: Rr3RailTier) -> u8 {
        match t {
            Rr3RailTier::Ok => 0,
            Rr3RailTier::Mild => 1,
            Rr3RailTier::Severe => 2,
            Rr3RailTier::Unknown => 3,
        }
    }
    let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
    let mut prev = rank(Rr3RailTier::Ok);
    for mv in [4700u16, 4600, 4500, 4400, 4500, 4300, 4700, 4600] {
        let (tier, event) = j.rr3_rail_feed(mv);
        assert_eq!(event, rank(tier) > prev, "mv={mv}: 事件 ⇔ 向下档变");
        prev = rank(tier);
    }
}

#[test]
fn rail_beat_unknown_not_refed_and_passthrough() {
    // ⑥-c2 消费执法两条（P-15 末列"执行面按保守侧处置、展示面原样显示"的落点）：
    // ① 产出侧已折的 Unknown 原样透传——mv 再好也不许从 mv 重分类出 Ok（不喂核，
    //    执行面保持置位）；
    let mut j = Rr3RailJudge::rr3_rail_judge_new(4600, 4400);
    let st = Rr3RailState {
        mv: 4700,
        tier: Rr3RailTier::Unknown,
        age_ms: 0,
    };
    let (tier, event) = rr3_rail_beat(&mut j, st, 250);
    assert_eq!(tier, Rr3RailTier::Unknown, "Unknown 原样透传，不折 Ok");
    assert_eq!(event, None, "Unknown 不喂判定核");
    // ② 陈旧读数（age > max）折 Unknown——同样不喂核（T-6：陈旧 Ok ≠ 新鲜 Ok）
    let st = Rr3RailState {
        mv: 4700,
        tier: Rr3RailTier::Ok,
        age_ms: 251,
    };
    let (tier, event) = rr3_rail_beat(&mut j, st, 250);
    assert_eq!(tier, Rr3RailTier::Unknown, "陈旧 Ok 折 Unknown（T-6）");
    assert_eq!(event, None);
    // 新鲜读数（age == max 边界不折）正常喂核：档位 = 判定核分类值（机制产出）
    let st = Rr3RailState {
        mv: 4300,
        tier: Rr3RailTier::Severe,
        age_ms: 250,
    };
    let (tier, event) = rr3_rail_beat(&mut j, st, 250);
    assert_eq!(tier, Rr3RailTier::Severe);
    assert_eq!(event, Some(true), "fresh 拍喂核，severe 线首发");
}
