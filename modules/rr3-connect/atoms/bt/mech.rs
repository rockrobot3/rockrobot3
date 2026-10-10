//! bt 原子件机制主体（五件套③，自原 src/bt.rs 逐字迁入，逻辑零改动）。
//!
//! 蓝牙（BlueZ）：配网备选通道的链路状态机骨架（0.0.4 方案 D5/Q5）。
//!
//! legacy btctl 的日志回放与远程命令语义已被 rrd 日志环 + `rr3.ctl.logs` 与
//! rr3-ctl 总线运维面承接（legacy-digest btctl 行在案），真正剩下的只有
//! "蓝牙作为一条链路"。本文件立**链路状态机骨架**：转移本体与后端无关
//! （纯函数，单测钉死），开机初值由 ops/描述符注入（N-1：env 读出件，
//! 当前经 config `[rr3-connect.bt]` up 数据化注入，批J G4 落 ops 槽时
//! 迁移；缺省关，出厂不自动开蓝牙）；真后端属板上件（cfg 门，0.0.4 方案
//! §1.3——实装依赖硬件采购，后置板上批次），本骨架不 claim 真射频链路。

/// 蓝牙链路相位（三态）：关机 / 可发现（配网备选通道在开）/ 已连（对端在链）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rr3BtPhase {
    Off,
    Discoverable,
    /// 骨架期（0.0.4 批E）：事件面（主题）未登记，主循环不构造此相位，
    /// 转移矩阵单测钉死其可达性；真后端（BlueZ 板上批次）接线后移除豁免。
    #[allow(dead_code)]
    Connected,
}

impl Rr3BtPhase {
    /// 相位的小写词汇（开机自述与日志用；不进事件体——主题未登记）。
    pub fn rr3_bt_phase_name(self) -> &'static str {
        match self {
            Rr3BtPhase::Off => "off",
            Rr3BtPhase::Discoverable => "discoverable",
            Rr3BtPhase::Connected => "connected",
        }
    }
}

/// 蓝牙链路状态机（模块内策略，HAL 不知情——与 wifi 原子的配网状态机
/// `Rr3Provision` 同层，跨原子零类型依赖只过 serde_json::Value）。
/// 合法转移矩阵：
/// Off --power_on--> Discoverable --link_up--> Connected，
/// Connected --link_down--> Discoverable --power_off--> Off。
/// 非法转移返回 `false` 且状态不动。
///
/// 骨架期在飞（0.0.4 批E，conventions §2）：事件面未登记 → 主循环暂无转移
/// 消费点（开机自述只走 new/phase/power_on/phase_name），转移函数本体由单测
/// 钉死；豁免随真后端（BlueZ 板上批次）接线消费即移除。
#[allow(dead_code)]
pub struct Rr3BtState {
    phase: Rr3BtPhase,
}

#[allow(dead_code)]
impl Rr3BtState {
    /// 开机初值注入（N-1）：`up=true` 构造即上电进 Discoverable，false = Off
    /// （出厂不自动开蓝牙）。初值是装配期注入不走转移矩阵——件内不再有
    /// env 读（全仓 13 件里唯一非 test 面的 env 读，批I 拔除）。
    pub fn rr3_bt_state_new(up: bool) -> Self {
        Self {
            phase: if up {
                Rr3BtPhase::Discoverable
            } else {
                Rr3BtPhase::Off
            },
        }
    }

    pub fn rr3_bt_phase(&self) -> Rr3BtPhase {
        self.phase
    }

    /// 上电：Off → Discoverable（可发现态，配网备选通道在开）。
    pub fn rr3_bt_power_on(&mut self) -> bool {
        if self.phase != Rr3BtPhase::Off {
            return false;
        }
        self.phase = Rr3BtPhase::Discoverable;
        true
    }

    /// 对端入链：Discoverable → Connected。
    pub fn rr3_bt_link_up(&mut self) -> bool {
        if self.phase != Rr3BtPhase::Discoverable {
            return false;
        }
        self.phase = Rr3BtPhase::Connected;
        true
    }

    /// 对端离链：Connected → Discoverable（回可发现态，不断电）。
    pub fn rr3_bt_link_down(&mut self) -> bool {
        if self.phase != Rr3BtPhase::Connected {
            return false;
        }
        self.phase = Rr3BtPhase::Discoverable;
        true
    }

    /// 断电：Discoverable → Off。Connected 不能直接断电（须先 link_down）——
    /// 状态机不 fake"连着还能静默关"，与 connect result 诚实原则同款。
    pub fn rr3_bt_power_off(&mut self) -> bool {
        if self.phase != Rr3BtPhase::Discoverable {
            return false;
        }
        self.phase = Rr3BtPhase::Off;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 开机初值注入（N-1）：up=true 构造即 Discoverable，false = Off
    /// （出厂不自动开蓝牙）——注入语义钉在纯函数单测。
    #[test]
    fn bt_state_new_takes_up_injection() {
        let s = Rr3BtState::rr3_bt_state_new(true);
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Discoverable, "注入 up 即上电");
        let s = Rr3BtState::rr3_bt_state_new(false);
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Off, "缺省关=出厂不自动开");
    }

    /// 转移矩阵主链（方案 D5 判据"骨架单测绿"钉在这里）：出厂 Off →
    /// power_on → Discoverable → link_up → Connected → link_down →
    /// Discoverable → power_off → Off，全程合法转移必成。
    #[test]
    fn bt_phase_roundtrips_full_chain() {
        let mut s = Rr3BtState::rr3_bt_state_new(false);
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Off, "出厂关机");
        assert!(s.rr3_bt_power_on());
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Discoverable, "上电即可发现");
        assert!(s.rr3_bt_link_up());
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Connected);
        assert!(s.rr3_bt_link_down());
        assert_eq!(
            s.rr3_bt_phase(),
            Rr3BtPhase::Discoverable,
            "离链回可发现态不断电"
        );
        assert!(s.rr3_bt_power_off());
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Off);
    }

    /// 非法转移全部拒绝且状态不动：Off 不能直接入链/离链/再断电；
    /// Discoverable 不能重复上电、不能离链；Connected 不能重复入链、
    /// 不能直接断电（须先 link_down——不 fake"连着还能静默关"）。
    #[test]
    fn bt_illegal_transitions_are_rejected_without_moving() {
        let mut s = Rr3BtState::rr3_bt_state_new(false);
        assert!(!s.rr3_bt_link_up(), "Off 不能直接入链");
        assert!(!s.rr3_bt_link_down(), "Off 不能离链");
        assert!(!s.rr3_bt_power_off(), "Off 不能再断电");
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Off, "三次非法转移状态不动");

        assert!(s.rr3_bt_power_on());
        assert!(!s.rr3_bt_power_on(), "Discoverable 不能重复上电");
        assert!(!s.rr3_bt_link_down(), "Discoverable 不能离链");
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Discoverable);

        assert!(s.rr3_bt_link_up());
        assert!(!s.rr3_bt_link_up(), "Connected 不能重复入链");
        assert!(
            !s.rr3_bt_power_off(),
            "Connected 不能直接断电（先 link_down）"
        );
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Connected);

        assert!(s.rr3_bt_link_down());
        assert!(s.rr3_bt_power_off());
        assert_eq!(s.rr3_bt_phase(), Rr3BtPhase::Off, "非法拒绝后合法链路仍通");
    }
}
