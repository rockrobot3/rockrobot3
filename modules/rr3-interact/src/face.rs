//! 表情渲染：idle / blink / talk / think / happy（`rr3-interact/face/expression`）。
//!
//! v0.0.1 的"渲染"只是状态机本身（真屏随硬件里程碑），但状态转移是现在就要
//! 钉死的契约面：五态白名单、`talk` 由 TTS 播放期间自动挂起并恢复——这保证
//! "说话时表情在 talk"不依赖调用方自觉。

use serde_json::{json, Value};

pub const SURFACE: &[&str] = &["face: rpc rr3-interact/face/expression (request)"];

/// 本模块 exposes 的表情主题（module.toml [rpc].exposes 同源）
pub const RR3_FACE_TOPIC: &str = "rr3-interact/face/expression";

/// 合法表情白名单（contracts/topics.toml 的 note 同源）
pub const RR3_FACE_EXPRESSIONS: [&str; 5] = ["idle", "blink", "talk", "think", "happy"];

/// TTS 播放期间的临时表情（模块内自动切换，不接受点名）
pub const RR3_FACE_TALK: &str = "talk";

/// 最小表情机：持有当前表情；`talk` 是唯一的瞬态——播放开始挂起（记住被打断
/// 的表情），播放结束恢复。单线程主循环内使用，无并发面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3Face {
    current: &'static str,
    /// talk 挂起前的表情（end_talk 恢复的去处）
    suspended: Option<&'static str>,
}

impl Default for Rr3Face {
    fn default() -> Self {
        Self {
            current: "idle",
            suspended: None,
        }
    }
}

impl Rr3Face {
    /// 点名切换表情：名字必须在白名单内。返回 `(切换后, 切换前)`。
    /// talk 不可被点名（它是播放面的自动态），点名切换会打断挂起态——
    /// 显式指令优先于自动恢复，这是表情机的最小仲裁规则。
    pub fn rr3_face_set(&mut self, expr: &str) -> Result<(&'static str, &'static str), String> {
        let expr = RR3_FACE_EXPRESSIONS
            .iter()
            .find(|&&e| e == expr)
            .copied()
            .ok_or_else(|| {
                format!(
                    "{RR3_FACE_TOPIC} 不认表情 {expr:?}（允许 {expr_list}）",
                    expr_list = RR3_FACE_EXPRESSIONS.join("/")
                )
            })?;
        if expr == RR3_FACE_TALK {
            return Err(format!("{RR3_FACE_TALK} 由 TTS 播放自动驱动，不接受点名"));
        }
        let prev = std::mem::replace(&mut self.current, expr);
        self.suspended = None; // 显式指令打断 talk 挂起态
        Ok((expr, prev))
    }

    /// TTS 播放开始：切到 talk，返回被挂起的表情。已在 talk（嵌套播放）则幂等。
    pub fn rr3_face_begin_talk(&mut self) -> &'static str {
        if self.current == RR3_FACE_TALK {
            return self.suspended.unwrap_or("idle");
        }
        let prev = std::mem::replace(&mut self.current, RR3_FACE_TALK);
        self.suspended = Some(prev);
        prev
    }

    /// TTS 播放结束：恢复挂起前的表情。没有挂起态（如点名后直接调）则回 idle。
    pub fn rr3_face_end_talk(&mut self) -> &'static str {
        match self.suspended.take() {
            Some(prev) => {
                self.current = prev;
                prev
            }
            None => {
                self.current = "idle";
                "idle"
            }
        }
    }
}

/// 从 rr3.call 的 `params.args` 里取表情请求参数：`expression` 必填。
/// 合法性交给 [`Rr3Face::rr3_face_set`]（错误信息里带白名单，这里不做两遍）。
pub fn rr3_face_parse_args(args: &Value) -> Result<String, String> {
    args.get("expression")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{RR3_FACE_TOPIC} 缺 string 参数 expression"))
}

/// `rr3-interact/face/expression` 的应答体：切换后的表情与切换前的表情。
pub fn rr3_face_result(current: &str, previous: &str) -> Value {
    json!({ "expression": current, "previous": previous })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whitelist_covers_five_expressions() {
        assert_eq!(
            RR3_FACE_EXPRESSIONS,
            ["idle", "blink", "talk", "think", "happy"]
        );
        // 初始态 idle：首次切换的"切换前"就是它
        assert_eq!(Rr3Face::default().rr3_face_set("blink").unwrap().1, "idle");
    }

    #[test]
    fn set_accepts_whitelist_and_reports_transition() {
        let mut f = Rr3Face::default();
        assert_eq!(f.rr3_face_set("happy").unwrap(), ("happy", "idle"));
        assert_eq!(f.rr3_face_set("think").unwrap(), ("think", "happy"));
    }

    #[test]
    fn set_rejects_unknown_and_point_named_talk() {
        let mut f = Rr3Face::default();
        assert!(f.rr3_face_set("wink").is_err());
        assert!(f.rr3_face_set("").is_err());
        assert!(f.rr3_face_set("talk").is_err(), "talk 是播放面的自动态");
        // 失败切换不动状态：下次成功切换的"切换前"仍是 idle
        assert_eq!(f.rr3_face_set("think").unwrap(), ("think", "idle"));
    }

    #[test]
    fn talk_suspends_and_restores_previous() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("think").unwrap();
        assert_eq!(f.rr3_face_begin_talk(), "think", "挂起前的表情");
        assert_eq!(
            f.rr3_face_set("blink").unwrap(),
            ("blink", "talk"),
            "talk 期间 current 是 talk"
        );
        // talk 期间被点名打断了挂起态，end_talk 只能回 idle——见 explicit_set 测试；
        // 这里另起一台机器验证正常恢复路径
        let mut g = Rr3Face::default();
        g.rr3_face_set("think").unwrap();
        g.rr3_face_begin_talk();
        assert_eq!(g.rr3_face_end_talk(), "think", "播放结束恢复");
        assert_eq!(g.rr3_face_set("blink").unwrap(), ("blink", "think"));
    }

    #[test]
    fn talk_without_suspend_restores_idle_and_is_idempotent() {
        let mut f = Rr3Face::default();
        // 无挂起态的 end_talk 回 idle
        assert_eq!(f.rr3_face_end_talk(), "idle");
        // 嵌套 begin_talk 幂等：不覆盖 suspended
        f.rr3_face_set("happy").unwrap();
        f.rr3_face_begin_talk();
        assert_eq!(f.rr3_face_begin_talk(), "happy", "嵌套挂起不丢原表情");
        assert_eq!(f.rr3_face_end_talk(), "happy");
        assert_eq!(
            f.rr3_face_set("think").unwrap(),
            ("think", "happy"),
            "恢复到位"
        );
    }

    #[test]
    fn explicit_set_interrupts_talk_suspend() {
        let mut f = Rr3Face::default();
        f.rr3_face_set("blink").unwrap();
        f.rr3_face_begin_talk();
        // 播放途中被点名 happy：挂起态作废，end_talk 不再回 blink
        let (cur, prev) = f.rr3_face_set("happy").unwrap();
        assert_eq!((cur, prev), ("happy", "talk"));
        assert_eq!(f.rr3_face_end_talk(), "idle", "挂起态已被显式指令打断");
        assert_eq!(f.rr3_face_set("blink").unwrap(), ("blink", "idle"));
    }

    #[test]
    fn args_and_result_shapes() {
        assert_eq!(
            rr3_face_parse_args(&json!({ "expression": "blink" })).unwrap(),
            "blink"
        );
        assert!(rr3_face_parse_args(&json!({})).is_err());
        assert!(rr3_face_parse_args(&json!({ "expression": 1 })).is_err());
        let r = rr3_face_result("talk", "idle");
        assert_eq!(r["expression"], "talk");
        assert_eq!(r["previous"], "idle");
        assert_eq!(r.get("topic"), None, "应答体不带 topic");
    }
}
