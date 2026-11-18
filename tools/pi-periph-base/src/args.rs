//! 轴名与角度参数解码。

use rr_periph_proto::ports::Rr3PwmChannel;
use serde_json::Value;

use crate::servo::Axis;

pub(crate) fn channel_of(axis: Axis) -> Rr3PwmChannel {
    match axis {
        Axis::Pan => Rr3PwmChannel::Pan,
        Axis::Tilt => Rr3PwmChannel::Tilt,
    }
}

pub(crate) fn parse_axis(v: Option<&Value>) -> Result<Axis, String> {
    match v {
        None | Some(Value::Null) => Ok(Axis::Pan),
        Some(v) => match v.as_str() {
            Some("pan") => Ok(Axis::Pan),
            Some("tilt") => Ok(Axis::Tilt),
            _ => Err(format!("axis 非法: {v}（pan|tilt）")),
        },
    }
}

pub(crate) fn parse_deg(v: Option<&str>) -> Result<u32, String> {
    v.ok_or_else(|| "缺角度参数".to_string())?
        .parse::<u32>()
        .map_err(|_| format!("角度非法: {v:?}"))
}
