//! WiFi 链路：`rr3-connect/net/link` 状态机与周期流。
//!
//! HAL 读数（`Rr3LinkState`）→ 状态机裁决（翻转必发 + 周期保底）→ 事件体上路。
//! 漫游/重连是**模块侧策略**，本轮不实装（roam_probe_enabled 只解析回显，与
//! rr3-power 的 cutoff 同款口径）；NetworkManager/D-Bus 属板上件（cfg 门骨架，
//! 0.0.2 方案 D7），sim 后端状态由环境注入定死——状态机的翻转分支只有纯函数
//! 单测能钉，e2e 只见首拍基线与周期流（e2e_power 的时序教训同款分工）。
//!
//! 云端凭据不落盘（conventions §3），HTTPS 会话参数随 M5 与 rr3-brain 对齐。

use rr_hal::Rr3LinkState;
use serde_json::{json, Value};

pub const SURFACE: &[&str] = &[
    "wifi: NetworkManager/D-Bus（板上件）-> rr3-connect/net/link (event)；sim 后端 env 注入",
    "wifi: 状态翻转必发 + 周期保底重发（流是摘要不是逐拍镜像）",
];

/// config `[rr3-connect.wifi]` 的形状（上报周期 + 漫游探针开关）
#[derive(Debug, Clone, Copy)]
pub struct Rr3LinkCfg {
    pub link_report_period_ms: u64,
    pub roam_probe_enabled: bool,
}

/// net/link 状态机：跟踪上一次的 up 值。首拍与**翻转**返回 true（立即发，
/// 不等周期）；同态返回 false（交给主循环的周期拍点保底）。状态只有这一位。
pub struct Rr3LinkEdge {
    last: Option<bool>,
}

impl Rr3LinkEdge {
    pub fn rr3_link_edge_new() -> Self {
        Self { last: None }
    }

    /// 每拍 HAL 读数喂一次 up 值。返回 true = 此刻应立即发布（首拍基线 / 翻转）。
    pub fn rr3_link_edge_cross(&mut self, up: bool) -> bool {
        match self.last {
            None => {
                self.last = Some(up);
                true // 首拍基线：观察者无论何时接入，下一条周期流必然等不来"没发过"
            }
            Some(prev) if prev != up => {
                self.last = Some(up);
                true // 翻转必发：在线/离线边界是这条流唯一的"新闻"
            }
            Some(_) => false,
        }
    }
}

/// net/link 事件体：seq 是本流自己的计数（envelope 惯例，从 1 起）。
/// ssid/rssi 断链时是 null——设备面（Rr3SimLink）已抹掉，这里如实透传。
pub fn rr3_link_body(seq: u64, st: &Rr3LinkState) -> Value {
    json!({
        "seq": seq,
        "up": st.up,
        "ssid": st.ssid,
        "rssi_dbm": st.rssi_dbm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(up: bool) -> Rr3LinkState {
        Rr3LinkState {
            up,
            ssid: up.then(|| "rock-sim".to_string()),
            rssi_dbm: up.then_some(-50),
        }
    }

    /// 状态机主语义：首拍基线必发、翻转必发、同态不发——"翻转必发 + 周期保底"
    /// 的"必发"一半钉在这里（sim 后端状态恒定，e2e 看不见翻转，只有单测能钉）。
    #[test]
    fn link_edge_fires_on_first_and_flip_only() {
        let mut e = Rr3LinkEdge::rr3_link_edge_new();
        assert!(e.rr3_link_edge_cross(true), "首拍基线必发");
        assert!(!e.rr3_link_edge_cross(true), "同态不发（周期拍点保底）");
        assert!(!e.rr3_link_edge_cross(true), "同态继续不发");
        assert!(e.rr3_link_edge_cross(false), "在线→离线翻转必发");
        assert!(!e.rr3_link_edge_cross(false), "翻转后同态不发");
        assert!(e.rr3_link_edge_cross(true), "离线→在线翻转必发");
    }

    #[test]
    fn link_body_carries_full_state_with_own_seq() {
        let v = rr3_link_body(3, &st(true));
        assert_eq!(v["seq"], 3);
        assert_eq!(v["up"], true);
        assert_eq!(v["ssid"], "rock-sim");
        assert_eq!(v["rssi_dbm"], -50);
        let v = rr3_link_body(4, &st(false));
        assert_eq!(v["up"], false);
        assert!(
            v["ssid"].is_null() && v["rssi_dbm"].is_null(),
            "断链时 ssid/rssi 必须是 null（设备面已抹掉）: {v}"
        );
    }
}
