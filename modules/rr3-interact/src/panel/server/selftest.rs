//! 自检预期表与三态词表（批N5 件五 F-31）：词表闭集，装配期查表。

/// 自检预期表值域闭集（批N5 件五 F-31）：固定腿期望三态词表——parse 面对账
/// 在册（词表外拒 boot），verdict 等值判定的合法判值集。
pub const RR3_SELFTEST_LEG_STATES: [&str; 3] = ["pass", "fail", "n-a"];

/// wiring 组预期词表（批N5 件五 F-31）：组内零 fail 即组过（n-a 不拖累不救场）。
/// 单值词表也是闭集——拼错的预期值（如 "zero_fail"）当场拒 boot，不容静默。
pub const RR3_SELFTEST_WIRING_EXPECTS: [&str; 1] = ["zero-fail"];

/// contracts 组预期词表（批N5 件五笔4，Z-4）：契约四册"齐全且 sha 对"的自检
/// 腿组，判据同质恒 zero-fail——词表保持闭集同款（拼错值当场拒 boot）。
pub const RR3_SELFTEST_CONTRACTS_EXPECTS: [&str; 1] = ["zero-fail"];

/// 自检预期表（批N5 件五 F-31）：固定腿 speaker/relay/face 逐名期望三态 +
/// wiring / contracts 组预期——verdict 的判据集，整体红绿是结果集对预期集
/// 等值的派生量。装配期自 config `[rr3-interact.selftest.expect]` 读出构造
/// （parse 已对账词表），字段恒持在册值（'static 来自词表 lookup）。
#[derive(Debug, Clone, Copy)]
pub struct Rr3SelftestExpect {
    pub speaker: &'static str,
    pub relay: &'static str,
    pub face: &'static str,
    /// wiring/* 组预期（`zero-fail` = 组内零 fail 即组过）
    pub wiring: &'static str,
    /// contracts/* 组预期（批N5 件五笔4，Z-4：契约四册齐全且 sha 对）
    pub contracts: &'static str,
}

impl Default for Rr3SelftestExpect {
    fn default() -> Self {
        // 缺省 = 固定腿全 pass + 两组零 fail——speaker 内置测试音（批N5
        // 件五笔2）落地后缺省判据即全绿；形态性缺腿仍可改表如实申报（F-31）
        Self {
            speaker: "pass",
            relay: "pass",
            face: "pass",
            wiring: "zero-fail",
            contracts: "zero-fail",
        }
    }
}

impl Rr3SelftestExpect {
    /// 固定腿三态 lookup（'static 来自词表）；词表外 None，调用方点名拒 boot
    pub fn rr3_state_lookup(s: &str) -> Option<&'static str> {
        RR3_SELFTEST_LEG_STATES.iter().find(|&&v| v == s).copied()
    }

    /// wiring 组预期 lookup（'static 来自词表）；词表外 None，调用方点名拒 boot
    pub fn rr3_wiring_lookup(s: &str) -> Option<&'static str> {
        RR3_SELFTEST_WIRING_EXPECTS
            .iter()
            .find(|&&v| v == s)
            .copied()
    }

    /// contracts 组预期 lookup（'static 来自词表）；词表外 None，调用方点名拒 boot
    pub fn rr3_contracts_lookup(s: &str) -> Option<&'static str> {
        RR3_SELFTEST_CONTRACTS_EXPECTS
            .iter()
            .find(|&&v| v == s)
            .copied()
    }
}
