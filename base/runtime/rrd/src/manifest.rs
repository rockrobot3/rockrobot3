//! 从 manifest 文本里取 rrd 运行期要用的"事实"（health / policy / entry / priority / depends）。
//!
//! 字段合法性由 `rr-registry::rr3_registry_validate_manifest_text` 负责（Task 2）。
//! 本模块**不校验规则**，只做"取不到就点名"的读取，供状态机与 spawn 使用。

use toml::Value;

/// rrd 运行期需要的 manifest 事实
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3ManifestFacts {
    pub name: String,
    pub heartbeat_ms: u64,
    pub watchdog_ms: u64,
    pub restart_policy: String,
    pub entry: String,
    pub start_priority: i64,
    pub depends_required: Vec<String>,
}

fn lookup<'a>(root: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

fn req_str(root: &Value, path: &str, errs: &mut Vec<String>) -> String {
    match lookup(root, path) {
        None => {
            errs.push(format!("缺少 {path}"));
            String::new()
        }
        Some(v) => match v.as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            Some(_) => {
                errs.push(format!("{path} 不得为空字符串"));
                String::new()
            }
            None => {
                errs.push(format!("{path} 应为字符串"));
                String::new()
            }
        },
    }
}

fn req_int(root: &Value, path: &str, errs: &mut Vec<String>) -> u64 {
    match lookup(root, path) {
        None => {
            errs.push(format!("缺少 {path}"));
            0
        }
        Some(v) => match v.as_integer() {
            Some(n) if n > 0 => n as u64,
            Some(n) => {
                errs.push(format!("{path} 应为正整数，实际 {n}"));
                0
            }
            None => {
                errs.push(format!("{path} 应为整数"));
                0
            }
        },
    }
}

/// 读取运行期事实；取不到的字段逐条点名（全部收集完才返回，便于运维一次改对）
pub fn rr3_manifest_facts_from_text(text: &str) -> Result<Rr3ManifestFacts, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("manifest TOML 解析失败: {e}")]),
    };
    let mut errs = Vec::new();
    let name = req_str(&root, "manifest.name", &mut errs);
    let entry = req_str(&root, "manifest.entry", &mut errs);
    let restart_policy = req_str(&root, "resources.restart_policy", &mut errs);
    let heartbeat_ms = req_int(&root, "health.heartbeat_ms", &mut errs);
    let watchdog_ms = req_int(&root, "health.watchdog_timeout_ms", &mut errs);
    // 只在两个数都取到时才比对：否则会把"缺字段"伪装成"跨字段矛盾"
    if heartbeat_ms > 0 && watchdog_ms > 0 && heartbeat_ms >= watchdog_ms {
        errs.push(format!(
            "health.heartbeat_ms({heartbeat_ms}) 必须小于 watchdog_timeout_ms({watchdog_ms})"
        ));
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Rr3ManifestFacts {
        name,
        heartbeat_ms,
        watchdog_ms,
        restart_policy,
        entry,
        start_priority: lookup(&root, "resources.start_priority").and_then(Value::as_integer).unwrap_or(100),
        depends_required: lookup(&root, "depends.required")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
    })
}

/// M2 只接受绝对路径 entry（manifest 从 wire 来，没有"所在目录"可依据）。
///
/// 按 **POSIX 语法**判定，刻意不用 `std::path::Path::is_absolute`：entry 永远是目标机
/// （Linux）上的路径，而 host 侧 Windows 的 `Path::is_absolute("/usr/bin/sim_peer")`
/// 实测返回 `false`（Windows 要求带盘符才算绝对，见 task-4 报告 L1 证据）。语义随 host
/// 漂移会让同一份 manifest 在两侧得到不同判定，注册门禁就不可复现了。
pub fn rr3_manifest_entry_is_absolute(entry: &str) -> bool {
    entry.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"[manifest]
schema_version = 1
name = "rr3-sima"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/sim_peer"

[events]
publishes = ["rr3-sima/ping/state"]
subscribes = []

[resources]
restart_policy = "always"
start_priority = 40

[health]
heartbeat_ms = 200
watchdog_timeout_ms = 1000
"#;

    /// 7 个事实全部取到
    #[test]
    fn facts_are_extracted() {
        let f = rr3_manifest_facts_from_text(GOOD).expect("应能读取");
        assert_eq!((f.name.as_str(), f.heartbeat_ms, f.watchdog_ms, f.restart_policy.as_str()), ("rr3-sima", 200, 1000, "always"));
        assert_eq!((f.entry.as_str(), f.start_priority), ("/usr/bin/sim_peer", 40));
        assert!(f.depends_required.is_empty());
    }

    /// 缺字段逐条点名且继续检查其余字段（不是遇一个就返回）
    #[test]
    fn missing_fields_are_all_reported_with_paths() {
        let text = GOOD.replace("entry = \"/usr/bin/sim_peer\"\n", "").replace("restart_policy = \"always\"\n", "");
        let errs = rr3_manifest_facts_from_text(&text).expect_err("缺字段必须报错");
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("缺少 manifest.entry")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("缺少 resources.restart_policy")), "{errs:?}");
    }

    /// 类型错与空串都是错误
    #[test]
    fn wrong_types_and_empty_values_are_errors() {
        let t = GOOD.replace("heartbeat_ms = 200", "heartbeat_ms = \"200\"");
        let errs = rr3_manifest_facts_from_text(&t).expect_err("类型错必须报错");
        assert_eq!(errs, vec!["health.heartbeat_ms 应为整数"], "{errs:?}");

        assert!(rr3_manifest_facts_from_text(&GOOD.replace("restart_policy = \"always\"", "restart_policy = \"\""))
            .expect_err("空 policy 必须报错")
            .iter()
            .any(|e| e.contains("manifest") || e.contains("resources.restart_policy 不得为空")));

        assert!(rr3_manifest_facts_from_text(&GOOD.replace("watchdog_timeout_ms = 1000", "watchdog_timeout_ms = 0"))
            .expect_err("0 毫秒不可排看门狗")
            .iter()
            .any(|e| e.contains("health.watchdog_timeout_ms 应为正整数")));
    }

    /// 坏 TOML 只报一条解析错，不继续瞎猜
    #[test]
    fn unparsable_toml_is_single_error() {
        let errs = rr3_manifest_facts_from_text("[manifest\nname = ").expect_err("必须报错");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].starts_with("manifest TOML 解析失败"), "{errs:?}");
    }

    /// rrd 侧的 heartbeat>=watchdog 拦截（与 rr-registry 同规则，纵深防御）
    #[test]
    fn heartbeat_must_stay_below_watchdog_here_too() {
        let errs = rr3_manifest_facts_from_text(&GOOD.replace("heartbeat_ms = 200", "heartbeat_ms = 1000"))
            .expect_err("相等也不行");
        assert_eq!(
            errs,
            vec!["health.heartbeat_ms(1000) 必须小于 watchdog_timeout_ms(1000)"],
            "文案要带两个具体数字，运维才知道改哪个"
        );
    }

    /// depends.required 取出来，供 Task 9 判 base 能力是否就位；start_priority 缺省 100
    #[test]
    fn depends_and_default_priority() {
        let text = GOOD.replace("[resources]", "[depends]\nrequired = [\"bus.publish\", \"bus.rpc\"]\n\n[resources]");
        let f = rr3_manifest_facts_from_text(&text).expect("应能读取");
        assert_eq!(f.depends_required, vec!["bus.publish".to_string(), "bus.rpc".to_string()]);

        let no_prio = GOOD.replace("start_priority = 40\n", "");
        assert_eq!(rr3_manifest_facts_from_text(&no_prio).unwrap().start_priority, 100);
    }

    /// entry 绝对性判定（M2 只接受绝对路径，见本计划"与 spec 的偏离 3"）。
    /// 反斜杠开头在 POSIX 里是合法文件名而非绝对路径，必须判 false——判定不随 host 漂移。
    #[test]
    fn entry_absoluteness() {
        assert!(rr3_manifest_entry_is_absolute("/usr/bin/sim_peer"));
        for rel in ["sim_peer", "./sim_peer", "../x/sim_peer", "", "\\usr\\bin\\sim_peer"] {
            assert!(!rr3_manifest_entry_is_absolute(rel), "{rel} 不是绝对路径");
        }
    }
}
