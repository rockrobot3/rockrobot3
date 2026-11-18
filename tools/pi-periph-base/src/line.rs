//! 行面命令解析与视图应答。

use std::str::SplitWhitespace;

use crate::app::Board;
use crate::args::parse_deg;
use crate::board::{OLED_COLS, OLED_ROWS};
use crate::servo::Axis;

impl Board {
    pub(crate) fn line_round(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let mut parts = line.split_whitespace();
        let head = parts.next().unwrap_or("");
        let result = match head {
            "PAN" | "TILT" | "SERVO" | "WANDER" | "SCAN" | "AUDIO" => {
                self.cmd_motion(head, &mut parts)
            }
            _ => self.cmd_view(line, head, &mut parts),
        };
        match result {
            Ok(s) => self.reply(&s),
            Err(e) => self.reply(&format!("ERR {e}")),
        }
    }

    /// 运动类命令（定位/云台/明确不支持项）：pan/tilt 共用一条轴臂。
    fn cmd_motion(
        &mut self,
        head: &str,
        parts: &mut SplitWhitespace<'_>,
    ) -> Result<String, String> {
        match head {
            "PAN" => self.axis_deg(Axis::Pan, "pan", parts.next()),
            "TILT" => self.axis_deg(Axis::Tilt, "tilt", parts.next()),
            "SERVO" => self.servo_pair(parse_deg(parts.next()), parse_deg(parts.next())),
            "WANDER" => Err("WANDER 不支持（T3 档无自主扫摆；INFO 恒报 wander=off）".into()),
            "SCAN" => Err("SCAN 不支持（用 PAN/TILT/SERVO 逐点）".into()),
            _ => Err("AUDIO 不支持（T3 档明确不做语音/DoA）".into()),
        }
    }

    /// 展示/查询类命令（帮助/信息/轨压/文本/表情）。
    fn cmd_view(
        &mut self,
        line: &str,
        head: &str,
        parts: &mut SplitWhitespace<'_>,
    ) -> Result<String, String> {
        match head {
            "HELP" => Ok("CMDS: OLED <text> | FACE <expr> | PAN <deg> | TILT <deg> | SERVO <pan> <tilt> | RAIL | INFO | HELP".into()),
            "INFO" => Ok(self.info_line()),
            "RAIL" => self.sample_rail().map(|(mv, raw)| {
                format!("OK rail={mv}mV raw={raw} tier={}", self.state.rail_tier.name())
            }),
            "OLED" => self.oled_text(line[head.len()..].trim_start()),
            "FACE" => self.face_expr(parts.next()),
            other => Err(format!("unknown cmd \"{other}\"")),
        }
    }

    /// OLED 文本臂：非空 + 长度上限（行数×列数），设置后重绘。
    fn oled_text(&mut self, rest: &str) -> Result<String, String> {
        if rest.is_empty() {
            Err("OLED 缺文本参数".into())
        } else if rest.len() > OLED_COLS * (OLED_ROWS - 1) {
            Err(format!(
                "OLED 文本超长（≤{} 字符 = {} 行 × {} 列）",
                OLED_COLS * (OLED_ROWS - 1),
                OLED_ROWS - 1,
                OLED_COLS
            ))
        } else {
            self.state.last_text = rest.to_string();
            self.state.frame_mode = false;
            self.state.expr_mode = false;
            self.redraw_text();
            Ok(format!("OK oled \"{rest}\""))
        }
    }
}
