// 发布归属：越权前缀、运行期冒充、保留命名空间、拒发布计数。
use super::*;

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
