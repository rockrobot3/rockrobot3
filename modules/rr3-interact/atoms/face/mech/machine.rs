//! 最小表情机：当前表情、talk 挂起恢复与帧指针推进。

use super::super::{rr3_face_expr_lookup, RR3_FACE_TALK};
use super::frames_table::rr3_face_frames;

/// 最小表情机：持有当前表情；`talk` 是唯一的瞬态——播放开始挂起（记住被打断
/// 的表情），播放结束恢复。单线程主循环内使用，无并发面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3Face {
    current: &'static str,
    /// talk 挂起前的表情（end_talk 恢复的去处）
    suspended: Option<&'static str>,
    /// 当前表情帧序列内的指针（rr3_face_tick 推进；入态归零从首帧重播）
    frame_ix: usize,
    /// 兜底表情（批N4 件五 I-17/M-3 数据化）：各回落点（end_talk 无挂起、
    /// begin_talk 幂等分支）的去处——装配期自 config `default_expression`
    /// 注入，构造器 [`Rr3Face::rr3_face_default`]；Default 是 legacy 回落
    /// （S-5 口径），不再是唯一真值点
    default_expr: &'static str,
}

impl Rr3Face {
    /// 带兜底表情的构造器（批N4 件五 I-17/M-3）：`default_expr` 须为在册值
    /// （装配期 parse 已对账白名单；此处机制面不再复检，值域由调用方保证）。
    /// 初态即兜底表情——新机器当前态就是人格取向本身。
    pub fn rr3_face_default(default_expr: &'static str) -> Self {
        Self {
            current: default_expr,
            suspended: None,
            frame_ix: 0,
            default_expr,
        }
    }
}

impl Default for Rr3Face {
    fn default() -> Self {
        // legacy 兜底 idle（一个事实一个真值点：legacy 值只在此处与 config.toml
        // 注释对表）
        Self::rr3_face_default("idle")
    }
}

/// [`Rr3Face::rr3_face_set`] 的零堆错误（批N24 件四）：两拒因各占一变体，
/// 动态数据（原始输入 / 在册名）随枚举走，机制层不落 `String` 堆——人读
/// 文案由编排层渲染（`src/rpc.rs` 的 `rr3_face_err_text`，与
/// 改前逐字一致；白名单连接串用 [`super::RR3_FACE_EXPRESSIONS_JOINED`] 常量
/// 投影，渲染不再运行时 join）。
#[derive(Debug, PartialEq, Eq)]
pub enum Rr3FaceSetError<'a> {
    /// 不认的表情：携带原始输入（渲染带白名单连接串点名允许集）
    UnknownExpression(&'a str),
    /// 不可点名：携带查得后的在册名（'static，白名单值），渲染带本形态
    /// 不可点名集（集是 config 注入的运行时数据，渲染面组装）
    NonPointable(&'static str),
}

impl Rr3Face {
    /// 点名切换表情：名字必须在白名单内。返回 `(切换后, 切换前)`。
    /// `non_pointable` = 本形态不可点名集（0.0.5 批M MC-10 I-11：config 注入，
    /// 默认含 `talk`——它是播放面的自动态）。命中排除集则拒（状态不动）；点名
    /// 切换会打断挂起态——显式指令优先于自动恢复，这是表情机的最小仲裁规则。
    pub fn rr3_face_set<'a>(
        &mut self,
        expr: &'a str,
        non_pointable: &[&str],
    ) -> Result<(&'static str, &'static str), Rr3FaceSetError<'a>> {
        let expr = rr3_face_expr_lookup(expr).ok_or(Rr3FaceSetError::UnknownExpression(expr))?;
        if non_pointable.contains(&expr) {
            return Err(Rr3FaceSetError::NonPointable(expr));
        }
        let prev = core::mem::replace(&mut self.current, expr);
        self.suspended = None; // 显式指令打断 talk 挂起态
        self.frame_ix = 0; // 切换即从首帧重播
        Ok((expr, prev))
    }

    /// TTS 播放开始：切到 talk，返回被挂起的表情。已在 talk（嵌套播放）则幂等。
    pub fn rr3_face_begin_talk(&mut self) -> &'static str {
        if self.current == RR3_FACE_TALK {
            return self.suspended.unwrap_or(self.default_expr);
        }
        let prev = core::mem::replace(&mut self.current, RR3_FACE_TALK);
        self.suspended = Some(prev);
        self.frame_ix = 0; // 进 talk 从首帧（张口帧）播起
        prev
    }

    /// TTS 播放结束：恢复挂起前的表情。没有挂起态（如点名后直接调）则回兜底
    /// 表情（批N4 件五 I-17/M-3：default_expr 数据化，legacy 回落 idle）。
    pub fn rr3_face_end_talk(&mut self) -> &'static str {
        match self.suspended.take() {
            Some(prev) => {
                self.current = prev;
                self.frame_ix = 0;
                prev
            }
            None => {
                self.current = self.default_expr;
                self.frame_ix = 0;
                self.default_expr
            }
        }
    }

    /// 帧动画一拍：报 `(当前表情, 本拍帧序号)` 并把指针推向下一帧（到尾回卷）。
    /// 先报后推——入态后的首拍报帧 0，调用方按返回值取帧渲染（真件随批F5a
    /// 显示 trait 接线；sim 档主循环按 ~250ms 节拍空推）。单帧态恒报 0。
    pub fn rr3_face_tick(&mut self) -> (&'static str, usize) {
        let ix = self.frame_ix;
        self.frame_ix = (self.frame_ix + 1) % rr3_face_frames(self.current).len();
        (self.current, ix)
    }
}
