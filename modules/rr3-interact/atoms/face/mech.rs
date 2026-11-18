//! face 原子件机制主体（五件套③，自原 src/face.rs 逐字迁入，逻辑零改动）。
//!
//! 每态配 8x8 点阵帧序列（blink/talk 双帧、其余单帧）；[`Rr3Face::rr3_face_tick`]
//! 一拍报当前帧并推进（到尾回卷），切换表情 / talk 挂起恢复都归零指针从首帧
//! 重播。最小表情机：持有当前表情；`talk` 是唯一的瞬态——播放开始挂起（记住
//! 被打断的表情），播放结束恢复。单线程主循环内使用，无并发面。
//!
//! 载荷组装（`rr3_face_result` 的 json!）与点名下钻（`rr3_face_parse_args`）
//! 已上移承载层（0.0.5 批I I-8，ADR-0019 修订 4）：件面只交状态机
//! （结构体 + 判定），组装归 `src/composite.rs`，线上字节逐字不变。

#[path = "mech/frames_core.rs"]
mod frames_core;
#[path = "mech/frames_emotion.rs"]
mod frames_emotion;
#[path = "mech/frames_glyph.rs"]
mod frames_glyph;
#[path = "mech/frames_table.rs"]
mod frames_table;
#[path = "mech/machine.rs"]
mod machine;

pub use frames_table::rr3_face_frames;
pub use machine::{Rr3Face, Rr3FaceSetError};

#[cfg(test)]
mod tests {
    use super::frames_core::{
        RR3_FRAMES_BLINK, RR3_FRAMES_HAPPY, RR3_FRAMES_IDLE, RR3_FRAMES_LISTEN, RR3_FRAMES_TALK,
        RR3_FRAMES_THINK, RR3_FRAMES_TURN,
    };
    use super::frames_glyph::RR3_FRAMES_CHAR_H;
    use super::frames_table::RR3_FACE_FRAME_TABLE;
    use super::*;
    use crate::face::RR3_FACE_EXPRESSIONS;

    /// 缺省形态的不可点名集（config 档同源：`[rr3-interact.face] non_pointable`）
    const NON_TALK: &[&str] = &["talk"];

    /// 白名单口径：七核心态在前（帧表下标锚点，契约字面量不动），char 单字符
    /// 档 36 个随后（charA..charZ + char0..char9），末尾 9 情绪/彩蛋态，全集 52。
    #[test]
    fn whitelist_covers_core_and_char_expressions() {
        assert_eq!(RR3_FACE_EXPRESSIONS.len(), 52);
        assert_eq!(
            &RR3_FACE_EXPRESSIONS[..7],
            ["idle", "blink", "talk", "think", "happy", "listen", "turn"]
        );
        for (ix, byte) in (b'A'..=b'Z').chain(b'0'..=b'9').enumerate() {
            let expected = format!("char{}", byte as char);
            assert_eq!(
                RR3_FACE_EXPRESSIONS[7 + ix],
                expected.as_str(),
                "白名单位次 7+{ix}"
            );
        }
        assert_eq!(
            &RR3_FACE_EXPRESSIONS[43..],
            [
                "sleepy",
                "surprised",
                "angry",
                "sad",
                "confused",
                "focus",
                "love",
                "dizzy",
                "scared"
            ]
        );
        // 初始态 idle：首次切换的"切换前"就是它
        assert_eq!(
            Rr3Face::default()
                .rr3_face_set("blink", NON_TALK)
                .unwrap()
                .1,
            "idle"
        );
    }

    /// 兜底表情注入（批N4 件五 I-17/M-3）：`rr3_face_default` 注入后，
    /// 各回落点（end_talk 无挂起、begin_talk 幂等分支、切换前值）取注入值
    /// 而非硬编码 idle——机制零策略载荷的 mech 侧钉子。
    #[test]
    fn default_expression_injection_drives_fallbacks() {
        let mut f = Rr3Face::rr3_face_default("happy");
        // 初态即兜底表情：首次切换的"切换前"就是注入值
        assert_eq!(f.rr3_face_set("blink", NON_TALK).unwrap().1, "happy");
        // end_talk 无挂起：回落注入值（连续调用同样回落，不折 idle）
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        // begin_talk 幂等分支：已在 talk 且无挂起时返回注入值
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_begin_talk(), "happy");
    }

    /// char 单字符档：帧表映射对得上 + 点名成功 + 单帧恒报 0（charH 横杠行钉死，
    /// 字模转换走偏在此最先响）
    #[test]
    fn char_expression_is_pointable_single_frame() {
        assert_eq!(rr3_face_frames("charH"), RR3_FRAMES_CHAR_H.as_slice());
        assert_eq!(
            RR3_FRAMES_CHAR_H[0][3], 0b0111_1100,
            "H 横杠行（列 bit6..bit2）"
        );
        let mut f = Rr3Face::default();
        assert_eq!(
            f.rr3_face_set("charH", NON_TALK).unwrap(),
            ("charH", "idle")
        );
        for _ in 0..3 {
            assert_eq!(f.rr3_face_tick(), ("charH", 0), "单帧态恒报帧 0");
        }
        assert_eq!(rr3_face_frames("char9").len(), 1, "数字档同构单帧");
    }

    #[test]
    fn new_states_are_point_namable() {
        let mut f = Rr3Face::default();
        assert_eq!(
            f.rr3_face_set("listen", NON_TALK).unwrap(),
            ("listen", "idle")
        );
        assert_eq!(
            f.rr3_face_set("turn", NON_TALK).unwrap(),
            ("turn", "listen")
        );
    }

    #[test]
    fn single_frame_state_ticks_in_place() {
        let mut f = Rr3Face::default();
        for _ in 0..3 {
            assert_eq!(f.rr3_face_tick(), ("idle", 0), "单帧态恒报帧 0");
        }
    }

    #[test]
    fn multi_frame_state_cycles() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink", NON_TALK).unwrap();
        assert_eq!(f.rr3_face_tick(), ("blink", 0));
        assert_eq!(f.rr3_face_tick(), ("blink", 1));
        assert_eq!(f.rr3_face_tick(), ("blink", 0), "到尾回卷");
        assert_eq!(f.rr3_face_tick(), ("blink", 1));
    }

    #[test]
    fn switch_resets_frame_pointer() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink", NON_TALK).unwrap();
        f.rr3_face_tick(); // 指针已推到 1
        f.rr3_face_set("think", NON_TALK).unwrap();
        assert_eq!(f.rr3_face_tick(), ("think", 0), "切换后从首帧重播");
    }

    #[test]
    fn talk_window_ticks_talk_frames() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("happy", NON_TALK).unwrap();
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_tick(), ("talk", 0));
        assert_eq!(f.rr3_face_tick(), ("talk", 1), "挂起期间走 talk 帧表");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(f.rr3_face_tick(), ("happy", 0), "恢复后指针归零");
    }

    #[test]
    fn frame_table_shape_is_sound() {
        assert_eq!(RR3_FACE_EXPRESSIONS.len(), RR3_FACE_FRAME_TABLE.len());
        // 双帧态帧间互异（有动画可言）；全表序列两两互异（状态→帧映射无歧义）
        assert_ne!(RR3_FRAMES_BLINK[0], RR3_FRAMES_BLINK[1]);
        assert_ne!(RR3_FRAMES_TALK[0], RR3_FRAMES_TALK[1]);
        for i in 0..RR3_FACE_FRAME_TABLE.len() {
            assert!(!rr3_face_frames(RR3_FACE_EXPRESSIONS[i]).is_empty());
            for j in 0..i {
                assert_ne!(
                    RR3_FACE_FRAME_TABLE[i], RR3_FACE_FRAME_TABLE[j],
                    "表情 {} 与 {} 的帧序列必须互异",
                    RR3_FACE_EXPRESSIONS[i], RR3_FACE_EXPRESSIONS[j]
                );
            }
        }
        // 白名单名逐个查得回对应表项；未知名兜底回 idle 帧
        assert_eq!(rr3_face_frames("idle"), RR3_FRAMES_IDLE.as_slice());
        assert_eq!(rr3_face_frames("blink"), RR3_FRAMES_BLINK.as_slice());
        assert_eq!(rr3_face_frames("talk"), RR3_FRAMES_TALK.as_slice());
        assert_eq!(rr3_face_frames("think"), RR3_FRAMES_THINK.as_slice());
        assert_eq!(rr3_face_frames("happy"), RR3_FRAMES_HAPPY.as_slice());
        assert_eq!(rr3_face_frames("listen"), RR3_FRAMES_LISTEN.as_slice());
        assert_eq!(rr3_face_frames("turn"), RR3_FRAMES_TURN.as_slice());
        assert_eq!(rr3_face_frames("nope"), rr3_face_frames("idle"));
    }

    #[test]
    fn set_accepts_whitelist_and_reports_transition() {
        let mut f = Rr3Face::default();
        assert_eq!(
            f.rr3_face_set("happy", NON_TALK).unwrap(),
            ("happy", "idle")
        );
        assert_eq!(
            f.rr3_face_set("think", NON_TALK).unwrap(),
            ("think", "happy")
        );
    }

    #[test]
    fn set_rejects_unknown_and_point_named_talk() {
        let mut f = Rr3Face::default();
        assert!(f.rr3_face_set("wink", NON_TALK).is_err());
        assert!(f.rr3_face_set("", NON_TALK).is_err());
        assert!(
            f.rr3_face_set("talk", NON_TALK).is_err(),
            "talk 是播放面的自动态"
        );
        // 失败切换不动状态：下次成功切换的"切换前"仍是 idle
        assert_eq!(
            f.rr3_face_set("think", NON_TALK).unwrap(),
            ("think", "idle")
        );
    }

    /// I-11：排除规则从件内 if 迁出为注入集——改数据即改行为，件内无 `match` 分支
    #[test]
    fn injected_non_pointable_narrows_surface() {
        // 空集：本形态不排除任何表情点名，连 talk 都可点名
        let mut open = Rr3Face::default();
        assert_eq!(open.rr3_face_set("talk", &[]).unwrap(), ("talk", "idle"));
        // 换一个排除子集（点名规则随数据走）：此刻排除 blink、放开 talk
        let mut swap = Rr3Face::default();
        assert_eq!(
            swap.rr3_face_set("talk", &["blink"]).unwrap(),
            ("talk", "idle")
        );
        assert!(
            swap.rr3_face_set("blink", &["blink"]).is_err(),
            "blink 进了排除集"
        );
        // 非在册名与排除集无关：白名单是契约面，任何注入集都拒
        assert!(swap.rr3_face_set("wink", &[]).is_err());
    }

    /// 两拒因分变体（批N24 件四）：不认 → UnknownExpression 携原始输入、
    /// 排除集命中 → NonPointable 携在册名——枚举映射是编排层渲染的依据。
    #[test]
    fn set_rejects_split_into_two_variants() {
        let mut f = Rr3Face::default();
        assert_eq!(
            f.rr3_face_set("wink", NON_TALK).unwrap_err(),
            Rr3FaceSetError::UnknownExpression("wink"),
            "不认的表情携原始输入"
        );
        assert_eq!(
            f.rr3_face_set("talk", NON_TALK).unwrap_err(),
            Rr3FaceSetError::NonPointable("talk"),
            "排除集命中携在册名（'static 白名单值）"
        );
    }

    /// I-11：拒因文案点名"本形态不可点名集"（空集渲染 `(空)`，见 face.rs 用例）
    #[test]
    fn non_pointable_reject_names_the_set() {
        let mut f = Rr3Face::default();
        assert_eq!(
            f.rr3_face_set("talk", NON_TALK).unwrap_err(),
            Rr3FaceSetError::NonPointable("talk"),
            "talk 是播放面的自动态（拒因携在册名，集渲染在编排层）"
        );
    }

    #[test]
    fn talk_suspends_and_restores_previous() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("think", NON_TALK).unwrap();
        assert_eq!(f.rr3_face_begin_talk(), "think", "挂起前的表情");
        assert_eq!(
            f.rr3_face_set("blink", NON_TALK).unwrap(),
            ("blink", "talk"),
            "talk 期间 current 是 talk"
        );
        // talk 期间被点名打断了挂起态，end_talk 只能回 idle——见 explicit_set 测试；
        // 这里另起一台机器验证正常恢复路径
        let mut g = Rr3Face::default();
        g.rr3_face_set("think", NON_TALK).unwrap();
        g.rr3_face_begin_talk();
        assert_eq!(g.rr3_face_end_talk(), "think", "播放结束恢复");
        assert_eq!(
            g.rr3_face_set("blink", NON_TALK).unwrap(),
            ("blink", "think")
        );
    }

    #[test]
    fn talk_without_suspend_restores_idle_and_is_idempotent() {
        let mut f = Rr3Face::default();
        // 无挂起态的 end_talk 回 idle
        assert_eq!(f.rr3_face_end_talk(), "idle");
        // 嵌套 begin_talk 幂等：不覆盖 suspended
        f.rr3_face_set("happy", NON_TALK).unwrap();
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_begin_talk(), "happy", "嵌套挂起不丢原表情");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(
            f.rr3_face_set("think", NON_TALK).unwrap(),
            ("think", "happy"),
            "恢复到位"
        );
    }

    #[test]
    fn explicit_set_interrupts_talk_suspend() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink", NON_TALK).unwrap();
        f.rr3_face_begin_talk();
        // 播放途中被点名 happy：挂起态作废，end_talk 不再回 blink
        let (cur, prev) = f.rr3_face_set("happy", NON_TALK).unwrap();
        assert_eq!((cur, prev), ("happy", "talk"));
        assert_eq!(f.rr3_face_end_talk(), "idle", "挂起态已被显式指令打断");
        assert_eq!(
            f.rr3_face_set("blink", NON_TALK).unwrap(),
            ("blink", "idle")
        );
    }

    // ---------- 两腿对拍（architecture §4.0.3 判据②，0.0.5 批K H1） ----------
    // 腿一·确定性：§8 设计行"set → begin_talk → set(listen) → end_talk"权威序
    // 序列喂两台新表情机，(调用结果, 相位, 帧指针轨迹) 逐拍相等。腿二·正确性：
    // 期望值引自裁定语义（interact 册 §2 权威序：挂起期被显式点名则挂起作废、
    // end_talk 回默认态、连续 begin 幂等）+ §5 载荷腿（应答体两字段，
    // additionalProperties=false 口径——形状断言在承载层 args_and_result_shapes，
    // 此处钉机制侧的转移与帧轨迹）。

    /// 腿一：权威序序列两跑轨迹相等
    #[test]
    fn parity_leg1_face_authority_order_train_is_deterministic() {
        fn run() -> Vec<(&'static str, &'static str, usize)> {
            let mut f = Rr3Face::default();
            let mut out = Vec::new();
            let (cur, prev) = f.rr3_face_set("happy", NON_TALK).unwrap();
            out.push((cur, prev, f.rr3_face_tick().1));
            out.push((f.rr3_face_begin_talk(), "", f.rr3_face_tick().1));
            let (cur, prev) = f.rr3_face_set("listen", NON_TALK).unwrap();
            out.push((cur, prev, f.rr3_face_tick().1));
            let back = f.rr3_face_end_talk();
            out.push((back, "", f.rr3_face_tick().1));
            out
        }
        assert_eq!(run(), run(), "同输入两跑逐拍相等（腿一·确定性）");
    }

    /// 腿二：权威序裁定逐点断言——显式点名打断挂起、end_talk 回 idle、
    /// 嵌套 begin 幂等、未 begin 就 end 回 idle（§8 边界集）
    #[test]
    fn parity_leg2_face_matches_authority_order() {
        // 挂起期被显式点名 → 挂起作废 → end_talk 回默认态 idle
        let mut f = Rr3Face::default();
        f.rr3_face_set("happy", NON_TALK).unwrap();
        f.rr3_face_begin_talk();
        f.rr3_face_set("listen", NON_TALK).unwrap();
        assert_eq!(
            f.rr3_face_end_talk(),
            "idle",
            "裁定：挂起被点名打断后回默认态"
        );
        // 连续两次 begin（幂等）：不覆盖挂起态
        let mut g = Rr3Face::default();
        g.rr3_face_set("think", NON_TALK).unwrap();
        assert_eq!(g.rr3_face_begin_talk(), "think");
        assert_eq!(g.rr3_face_begin_talk(), "think", "嵌套 begin 幂等");
        assert_eq!(g.rr3_face_end_talk(), "think", "挂起态未被嵌套覆盖");
        // 未 begin 就 end：回 idle 且无 panic（§8 边界）
        let mut h = Rr3Face::default();
        assert_eq!(h.rr3_face_end_talk(), "idle", "未 begin 就 end 回 idle");
        // 双帧态回绕 + 换态归零（§8 帧推进行）
        let mut k = Rr3Face::default();
        k.rr3_face_set("blink", NON_TALK).unwrap();
        assert_eq!(k.rr3_face_tick().1, 0);
        assert_eq!(k.rr3_face_tick().1, 1);
        assert_eq!(k.rr3_face_tick().1, 0, "双帧态到尾回卷");
        k.rr3_face_set("turn", NON_TALK).unwrap();
        assert_eq!(k.rr3_face_tick().1, 0, "换态归零");
    }
}
