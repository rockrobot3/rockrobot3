//! wifi 原子件机制主体（五件套③，自原 src/wifi.rs 逐字迁入，逻辑零改动）。
//!
//! WiFi 链路：`rr3-connect/net/link` 状态机与周期流 + 配网面三 RPC（0.0.4 批D）。
//!
//! HAL 读数（`Rr3LinkState`）→ 状态机裁决（翻转必发 + 周期保底）→ 事件体上路。
//! 漫游/重连是**模块侧策略**，本轮不实装（0.0.5 批I N-6：无读者的
//! roam_probe_enabled 键已删除，漫游实装时再随工单进册）；NetworkManager/D-Bus
//! 属板上件（cfg 门骨架，0.0.2 方案 D7），sim 后端状态由环境注入定死——状态机
//! 的翻转分支只有纯函数单测能钉，e2e 只见首拍基线与周期流（e2e_power 的时序
//! 教训同款分工）。
//!
//! 配网面（0.0.4 方案 D4/Q3）：AP 配网状态机是**模块内策略**，不进 HAL——sim
//! 是"设备"不是"策略"，HAL 只如实报数（up/ssid/rssi），凭据态归
//! [`Rr3Provision`]，net/link 体以 `mode` 字段随流发布（契约只增不改，0.0.4
//! 批D append）。connect/forget 只改状态机与内存凭据：凭据不落盘（conventions
//! §3），真凭据持久化（wpa_supplicant/NVS）板上后置——重启即忘，如实声明。
//! scan 回 sim 模拟列表（`backend:"sim"` 诚实标注），板上换 NetworkManager。
//!
//! 载荷组装与参数下钻面（0.0.5 批I N-7，ADR-0019 修订 4）：`rr3_link_body`、
//! 三枚 `*_result` 与三枚 `*_parse_args` 的 json!/Value 下钻上移承载层
//! （src/composite.rs），件面 serde_json 清零——本件只交纯结构体与判定。

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

// ---------- 配网面（0.0.4 批D）：AP 配网状态机 ----------

/// AP 配网状态机（模块内策略，HAL 不知情）：内存凭据有 → `station`（客户端
/// 模式），没有 → `ap`（配网热点在开）。connect 存凭据、forget 清凭据，
/// 就这两步转移——"AP 态可模拟进出"由纯函数单测钉死（方案 D4 判据）。
/// 真凭据持久化（wpa_supplicant/NVS）板上后置，重启即忘，不 claim。
pub struct Rr3Provision {
    credentials: Option<(String, String)>,
}

impl Rr3Provision {
    pub fn rr3_provision_new() -> Self {
        Self { credentials: None }
    }

    /// 当前配网模式：`"ap"` | `"station"`（net/link 的 mode 字段词汇，只有两值）。
    pub fn rr3_provision_mode(&self) -> &'static str {
        match self.credentials {
            None => "ap",
            Some(_) => "station",
        }
    }

    /// 存凭据并转入 station（参数合法性由承载层 `rr3_wifi_connect_parse_args`
    /// 裁决（N-7 上移），状态机只管转移）。
    pub fn rr3_provision_connect(&mut self, ssid: String, pass: String) {
        self.credentials = Some((ssid, pass));
    }

    /// 清凭据回 ap——legacy `/api/factory_reset` 的 WiFi 语义承接（≠ M6 整机
    /// 恢复出厂，partition-layout 两语义不合并）。
    pub fn rr3_provision_forget(&mut self) {
        self.credentials = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 配网状态机进出（方案 D4 判据"AP 态可模拟进出"钉在纯函数单测）：
    /// 出厂 ap → connect 转 station → forget 回 ap，凭据只存内存。
    #[test]
    fn provision_roundtrips_ap_station() {
        let mut p = Rr3Provision::rr3_provision_new();
        assert_eq!(p.rr3_provision_mode(), "ap", "出厂无凭据 = 配网热点在开");
        p.rr3_provision_connect("home".into(), "password8".into());
        assert_eq!(
            p.rr3_provision_mode(),
            "station",
            "connect 存凭据即转 station"
        );
        p.rr3_provision_forget();
        assert_eq!(p.rr3_provision_mode(), "ap", "forget 清凭据即回 ap");
    }
}
