//! 表情/点阵渲染臂与 OLED 文本重绘。

use rr_periph_proto::ports::Rr3PwmChannel;
use serde_json::{json, Value};

use crate::app::Board;
use crate::face::expr_eye_base;
use crate::topics_gen::RR3_FACE_EXPRS;

impl Board {
    pub(crate) fn handle_face(&mut self, args: &Value) -> Result<Value, String> {
        if let Some(v) = args.get("expression") {
            self.expr_apply(v)
        } else if let Some(v) = args.get("frame") {
            self.frame_apply(v)
        } else {
            Err("缺 expression/frame 之一".into())
        }
    }

    /// 表情臂：白名单门 → 渲染 → 提交状态（先渲染后提交：渲染失败不污染视图）。
    fn expr_apply(&mut self, v: &Value) -> Result<Value, String> {
        let expr = v
            .as_str()
            .ok_or_else(|| format!("expression 非法: {v}（须字符串）"))?;
        if !RR3_FACE_EXPRS.contains(&expr) {
            return Err(format!("expression 不在白名单: \"{expr}\""));
        }
        let base = expr_eye_base(expr);
        self.oled
            .capsules(base)
            .map_err(|e| format!("OLED 表情渲染失败: {e}"))?;
        let previous = std::mem::replace(&mut self.state.prev_expr, expr.to_string());
        self.state.base = base;
        self.state.expr_mode = true;
        self.state.frame_mode = false;
        Ok(json!({ "expression": expr, "previous": previous }))
    }

    /// 点阵臂：8 字节帧门 → 上屏 → 提交状态（同上，上屏成功才切模式）。
    fn frame_apply(&mut self, v: &Value) -> Result<Value, String> {
        let arr = v
            .as_array()
            .ok_or_else(|| format!("frame 非法: {v}（须 8 字节数组）"))?;
        if arr.len() != 8 {
            return Err(format!("frame 长度非法: {}（须 8）", arr.len()));
        }
        let mut f = [0u8; 8];
        for (i, item) in arr.iter().enumerate() {
            let b = item
                .as_u64()
                .ok_or_else(|| format!("frame[{i}] 非法: {item}"))?;
            if b > 255 {
                return Err(format!("frame[{i}] 超 u8: {b}"));
            }
            f[i] = b as u8;
        }
        self.oled
            .frame(&f)
            .map_err(|e| format!("OLED 点阵渲染失败: {e}"))?;
        self.state.frame_mode = true;
        self.state.expr_mode = false;
        Ok(json!({ "applied": true }))
    }

    pub(crate) fn render_expression(&mut self, expr: &str) -> Result<(), String> {
        let base = expr_eye_base(expr);
        // 先渲染后提交状态（同 handle_face：效应确认才记账）
        self.oled
            .capsules(base)
            .map_err(|e| format!("OLED 表情渲染失败: {e}"))?;
        self.state.base = base;
        self.state.expr_mode = true;
        self.state.frame_mode = false;
        self.state.prev_expr = expr.to_string();
        Ok(())
    }

    /// 文本模式重绘（帧/表情模式占满 8 页时跳过，不凿位图）。
    pub(crate) fn redraw_text(&mut self) {
        if self.state.frame_mode || self.state.expr_mode {
            return;
        }
        let text = self.state.last_text.clone();
        let pan = self.state.servo.deg(Rr3PwmChannel::Pan);
        let tilt = self.state.servo.deg(Rr3PwmChannel::Tilt);
        if let Err(e) = self.oled.show_text(&text, pan, tilt) {
            eprintln!("[periph] OLED 文本渲染失败: {e}");
        }
    }

    /// FACE 表情臂：白名单内才进渲染。
    pub(crate) fn face_expr(&mut self, expr: Option<&str>) -> Result<String, String> {
        match expr {
            Some(expr) if RR3_FACE_EXPRS.contains(&expr) => self
                .render_expression(expr)
                .map(|()| format!("OK face={expr}")),
            Some(expr) => Err(format!("FACE 表情不在白名单: \"{expr}\"")),
            None => Err("FACE 缺表情参数".into()),
        }
    }
}
