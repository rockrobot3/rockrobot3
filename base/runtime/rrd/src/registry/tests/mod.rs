// registry 的单元测试：状态机全转移、索引维护、扇出与降级路径。
// 测试是 registry 的后代模块，可触私有字段（与实现同口径）。
use super::*;
use crate::manifest::rr3_manifest_parse;
use std::sync::mpsc;

fn facts(name: &str, hb: u64, wd: u64, policy: &str) -> Rr3ManifestFacts {
    let text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = []

[resources]
restart_policy = "{policy}"

[health]
heartbeat_ms = {hb}
watchdog_timeout_ms = {wd}
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

fn chan() -> (SyncSender<Rr3Frame>, std::sync::mpsc::Receiver<Rr3Frame>) {
    mpsc::sync_channel(RR3_FAN_CAP)
}

/// 带 `[rpc].exposes` 的 facts（M3 路由用例的夹具；exposes 为空 = 无 [rpc] 段）
fn facts_exposes(name: &str, exposes: &[&str]) -> Rr3ManifestFacts {
    let rpc = if exposes.is_empty() {
        String::new()
    } else {
        let list = exposes
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[rpc]\nexposes = [{list}]\n\n")
    };
    let text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = []

{rpc}[resources]
restart_policy = "never"

[health]
heartbeat_ms = 100
watchdog_timeout_ms = 500
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

/// 带 `[events].subscribes` 的 facts（广播影子统计用例的夹具）
fn facts_subs(name: &str, subs: &[&str]) -> Rr3ManifestFacts {
    let list = subs
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!(
        r#"[manifest]
schema_version = 1
name = "{name}"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "/usr/bin/{name}"

[events]
publishes = []
subscribes = [{list}]

[resources]
restart_policy = "never"

[health]
heartbeat_ms = 100
watchdog_timeout_ms = 500
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

/// 影子统计：按 subscribes 只记账"本应过滤"的连接，不改变全量投递

mod fanout;
mod register;
mod beat;
mod publish;
mod restart;
mod death;
mod degrade;
