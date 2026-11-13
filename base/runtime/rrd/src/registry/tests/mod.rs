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

mod beat;
mod death;
mod degrade;
/// 影子统计：按 subscribes 只记账"本应过滤"的连接，不改变全量投递

/// 小测试辅助：主题集
fn topic_set(entries: &[(&str, &str)]) -> Rr3TopicSet {
    use std::collections::BTreeSet;
    let mut owners: BTreeSet<&str> = BTreeSet::new();
    for (_, o) in entries {
        owners.insert(o);
    }
    let mut text = String::from("schema_version = 1\n");
    for o in &owners {
        text.push_str(&format!("\n[[owner]]\nname = \"{o}\"\n"));
    }
    for (t, o) in entries {
        text.push_str(&format!(
            "\n[[topic]]\nname = \"{t}\"\nkind = \"event\"\nowner = \"{o}\"\n"
        ));
    }
    let set = rr_registry::rr3_topic_parse(&text);
    assert!(
        set.rr3_topic_check().is_empty(),
        "测试主题集自身必须合法: {:?}",
        set.rr3_topic_check()
    );
    set
}

fn frame_to(dst: u64) -> Rr3Frame {
    let mut h = rr_ipc::Rr3Header::rr3_hdr_broadcast(rr_ipc::opcode::SEND, 0);
    h.dst_task = dst;
    Rr3Frame {
        header: h,
        payload: vec![],
    }
}

// ---------- 降级传播（ADR-0009 裁定 3/4）----------

/// 带 [depends].required 的 facts（wd 取 60s：判死只该发生在被点名的提供者身上）
fn facts_with_depends(name: &str, deps: &[&str]) -> Rr3ManifestFacts {
    let list = deps
        .iter()
        .map(|d| format!("\"{d}\""))
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
subscribes = []

[depends]
required = [{list}]

[resources]
restart_policy = "always"

[health]
heartbeat_ms = 100
watchdog_timeout_ms = 60000
"#
    );
    rr3_manifest_parse(&text).expect("测试 manifest 必须可读")
}

fn providers_of(pairs: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
    let mut m: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (cap, names) in pairs {
        m.entry(cap.to_string())
            .or_default()
            .extend(names.iter().map(|n| n.to_string()));
    }
    m
}

mod fanout;
mod publish;
mod register;
mod restart;
