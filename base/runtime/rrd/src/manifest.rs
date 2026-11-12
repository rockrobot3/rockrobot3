//! 从 manifest 文本里取 rrd 运行期要用的"事实"（health / policy / entry / priority /
//! depends / provides）。
//!
//! 字段合法性由 `rr-registry::rr3_manifest_chk` 负责（Task 2）。
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
    /// `[manifest].lang`：rust | python。运行期不分支语言（spawn 只看 entry 可执行），
    /// 进 facts 是因为演练/打包侧的产物路径推导要它（python 模块产物 = shim 脚本），
    /// 解析真值保持一册——不逼调用方自己再开一份 TOML 读取。
    pub lang: String,
    pub start_priority: i64,
    pub depends_required: Vec<String>,
    /// `[provides].capabilities`：本模块向 boot 全集声明提供的能力（ADR-0009 裁定 1）。
    /// 可选段，缺省空 vec——不声明就是"不提供跨模块能力"。boot 仲裁用它建边、
    /// 运行期降级传播用它定"谁死了谁的下游降级"。
    pub provides: Vec<String>,
    /// `[rpc].exposes`：本模块愿意被 `rr3.call` 点名的主题（M3 路由的路由表来源）。
    /// 可选段，缺省空 vec——不声明就是"不接模块间请求"，与 publishes 的归属检查互不相干。
    pub rpc_exposes: Vec<String>,
    /// `[events].subscribes`：本模块消费的广播主题（契约面）。运行期只做
    /// **影子统计**——广播仍全量扇出，按本字段模拟"本应过滤"的目标数；
    /// 切换为过滤投递前不改行为（ADR 评审后另行落地）。
    pub subscribes: Vec<String>,
}

fn lookup<'a>(root: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// 模块名形状（唯一真值：boot 装载与线上注册共用本解析器）。名字是 per-unit
/// 日志路径分量（supervisor 落 `<socket>.log/<name>.log`）与计数键前缀，
/// 带路径分隔符即越出日志目录的根因——只收 ASCII 字母/数字/-/_/.。
fn rr3_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// 可选字符串数组段（rpc.exposes / depends.required / events.subscribes /
/// provides.capabilities 共用同一形状纪律）：缺省空；给了必须是非空字符串
/// 数组，类型错/空项逐条点名——空表本身是合法语义（不接请求/不消费），
/// 类型错被静默吞掉会让调用方拿到假诊断（fail-closed）。
fn rr3_opt_str_list(
    root: &Value,
    path: &str,
    type_label: &str,
    item_label: &str,
    errs: &mut Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    let Some(v) = lookup(root, path) else {
        return out;
    };
    let Some(items) = v.as_array() else {
        errs.push(format!("{type_label} 应为字符串数组"));
        return out;
    };
    for item in items {
        rr3_str_item(item, item_label, &mut out, errs);
    }
    out
}

/// 单项：非空串入表，否则逐条点名。
fn rr3_str_item(item: &Value, item_label: &str, out: &mut Vec<String>, errs: &mut Vec<String>) {
    match item.as_str() {
        Some(s) if !s.is_empty() => out.push(s.to_string()),
        _ => errs.push(format!("{item_label} 的每一项都必须是非空字符串")),
    }
}

fn req_str(root: &Value, path: &str, errs: &mut Vec<String>) -> String {
    let Some(v) = rr3_req_present(root, path, errs) else {
        return String::new();
    };
    match v.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        Some(_) => {
            errs.push(format!("{path} 不得为空字符串"));
            String::new()
        }
        None => {
            errs.push(format!("{path} 应为字符串"));
            String::new()
        }
    }
}

fn req_int(root: &Value, path: &str, errs: &mut Vec<String>) -> u64 {
    let Some(v) = rr3_req_present(root, path, errs) else {
        return 0;
    };
    match v.as_integer() {
        Some(n) if n > 0 => n as u64,
        Some(n) => {
            errs.push(format!("{path} 应为正整数，实际 {n}"));
            0
        }
        None => {
            errs.push(format!("{path} 应为整数"));
            0
        }
    }
}

/// 必填项取在场值：缺项当场点名并返回 `None`。
fn rr3_req_present<'a>(root: &'a Value, path: &str, errs: &mut Vec<String>) -> Option<&'a Value> {
    let v = lookup(root, path);
    if v.is_none() {
        errs.push(format!("缺少 {path}"));
    }
    v
}

/// 读取运行期事实；取不到的字段逐条点名（全部收集完才返回，便于运维一次改对）
pub fn rr3_manifest_parse(text: &str) -> Result<Rr3ManifestFacts, Vec<String>> {
    let root: Value = match toml::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("manifest TOML 解析失败: {e}")]),
    };
    let mut errs = Vec::new();
    let name = req_str(&root, "manifest.name", &mut errs);
    rr3_name_shape(&name, &mut errs);
    let entry = req_str(&root, "manifest.entry", &mut errs);
    let lang = req_str(&root, "manifest.lang", &mut errs);
    let restart_policy = req_str(&root, "resources.restart_policy", &mut errs);
    let heartbeat_ms = req_int(&root, "health.heartbeat_ms", &mut errs);
    let watchdog_ms = req_int(&root, "health.watchdog_timeout_ms", &mut errs);
    rr3_hb_order(heartbeat_ms, watchdog_ms, &mut errs);
    let (rpc_exposes, depends_required, subscribes, provides) =
        rr3_manifest_lists(&root, &mut errs);
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Rr3ManifestFacts {
        name,
        heartbeat_ms,
        watchdog_ms,
        restart_policy,
        entry,
        lang,
        start_priority: lookup(&root, "resources.start_priority")
            .and_then(Value::as_integer)
            .unwrap_or(100),
        depends_required,
        provides,
        rpc_exposes,
        subscribes,
    })
}

/// 四个可选字符串数组段（[rpc].exposes / [depends].required / [events].subscribes /
/// [provides].capabilities）：给了就必须是**非空字符串数组**——类型错要报错，
/// 不能静默当空表（空表 = "不接请求"，类型错被吞掉会让调用方拿到"无人在册"的假诊断）。
fn rr3_manifest_lists(
    root: &Value,
    errs: &mut Vec<String>,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let rpc_exposes = rr3_opt_str_list(root, "rpc.exposes", "rpc.exposes", "[rpc].exposes", errs);
    let depends_required = rr3_opt_str_list(
        root,
        "depends.required",
        "depends.required",
        "depends.required",
        errs,
    );
    let subscribes = rr3_opt_str_list(
        root,
        "events.subscribes",
        "events.subscribes",
        "[events].subscribes",
        errs,
    );
    let provides = rr3_opt_str_list(
        root,
        "provides.capabilities",
        "provides.capabilities",
        "provides.capabilities",
        errs,
    );
    (rpc_exposes, depends_required, subscribes, provides)
}

/// 模块名形状点名（空名留给"缺少 manifest.name"单点报，不重复）。
fn rr3_name_shape(name: &str, errs: &mut Vec<String>) {
    if !name.is_empty() && !rr3_name_ok(name) {
        errs.push(format!(
            "manifest.name {name:?} 形状非法（只允许 ASCII 字母/数字/-/_/.，不得为 . 或 ..，长度 1..=64）"
        ));
    }
}

/// 心跳/看门狗次序点名：只在两个数都取到时才比对，否则会把"缺字段"伪装成"跨字段矛盾"。
fn rr3_hb_order(heartbeat_ms: u64, watchdog_ms: u64, errs: &mut Vec<String>) {
    if heartbeat_ms > 0 && watchdog_ms > 0 && heartbeat_ms >= watchdog_ms {
        errs.push(format!(
            "health.heartbeat_ms({heartbeat_ms}) 必须小于 watchdog_timeout_ms({watchdog_ms})"
        ));
    }
}

/// M2 只接受绝对路径 entry（manifest 从 wire 来，没有"所在目录"可依据）。
///
/// 按 **POSIX 语法**判定，刻意不用 `std::path::Path::is_absolute`：entry 永远是目标机
/// （Linux）上的路径，而 host 侧 Windows 的 `Path::is_absolute("/usr/bin/sim_peer")`
/// 实测返回 `false`（Windows 要求带盘符才算绝对，见 task-4 报告 L1 证据）。语义随 host
/// 漂移会让同一份 manifest 在两侧得到不同判定，注册门禁就不可复现了。
pub fn rr3_manifest_abs(entry: &str) -> bool {
    entry.starts_with('/')
}

/// entry 行定位（单点）：全文本内恰一条 `entry = "…"`，返回（行号, 值区间）。
/// 引号内无值、多条、零条都是契约事故，逐类点名拒绝。
fn rr3_entry_span(text: &str, module: &str) -> Result<(usize, usize, usize), String> {
    let hits = rr3_entry_hits(text, module)?;
    rr3_entry_single(module, &hits)
}

/// 全文扫 entry 行：逐行收集（行号, 值区间）。
fn rr3_entry_hits(text: &str, module: &str) -> Result<Vec<(usize, usize, usize)>, String> {
    let mut hits: Vec<(usize, usize, usize)> = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if let Some((a, b)) = rr3_entry_row(line, module)? {
            hits.push((idx, a, b));
        }
    }
    Ok(hits)
}

/// entry 行数裁定：恰一条返回位置，零条 / 多条逐类点名。
fn rr3_entry_single(
    module: &str,
    hits: &[(usize, usize, usize)],
) -> Result<(usize, usize, usize), String> {
    if let [one] = hits {
        return Ok(*one);
    }
    if hits.is_empty() {
        return Err(format!(
            "模块 {module} 的 manifest 无 entry 行（schema 违约，打包/装机拒绝）"
        ));
    }
    Err(format!(
        "模块 {module} 的 manifest 有 {} 行 entry（须恰一条；歧义，打包/装机拒绝）",
        hits.len()
    ))
}

/// 单行 entry 定位：非 entry 行回 None；entry 行回引号内的值区间（行内偏移）。
fn rr3_entry_row(line: &str, module: &str) -> Result<Option<(usize, usize)>, String> {
    if !rr3_entry_head(line) {
        return Ok(None);
    }
    let (a, b) = rr3_entry_bounds(line, module)?;
    Ok(Some((a + 1, b)))
}

fn rr3_entry_head(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("entry")
        && t["entry".len()..].starts_with(|c: char| c.is_whitespace() || c == '=')
}

fn rr3_entry_bounds(line: &str, module: &str) -> Result<(usize, usize), String> {
    let (Some(a), Some(b)) = (line.find('"'), line.rfind('"')) else {
        return Err(format!(
            "模块 {module} 的 entry 行不是双引号字符串（schema 违约）: {line}"
        ));
    };
    if b <= a + 1 {
        return Err(format!(
            "模块 {module} 的 entry 行引号内无值（schema 违约）: {line}"
        ));
    }
    Ok((a, b))
}

/// 装机的 entry 值形制门（单点）：POSIX 绝对路径且不含 `.`/`..` 段——装机布局的
/// 入口直接进"是否可装机"判据，相对路径会让起进程方由 CWD 决定起哪个二进制。
fn rr3_entry_target(installed: &str, module: &str) -> Result<(), String> {
    if !rr3_manifest_abs(installed) || rr3_entry_dotty(installed) {
        return Err(format!(
            "模块 {module} 的装机 entry 不可装机（{installed}：须 POSIX 绝对路径且无 . / .. 段）"
        ));
    }
    Ok(())
}

/// 路径含 `.` / `..` 段（装机布局禁止）。
fn rr3_entry_dotty(installed: &str) -> bool {
    installed.split('/').any(|s| s == "." || s == "..")
}

/// manifest 的 entry 装机改写（**单点**：打包与演练布局共用同一实现，防两侧
/// 判据漂移）：形制门 → 恰一条 → 旧值对账解析事实 → 引号外零差异改写 →
/// 重解析复核新值已生效。不整文件重序列化（那会重排/丢注释，违反"只动一个值"）。
pub fn rr3_manifest_rewrite(
    text: &str,
    installed_entry: &str,
    facts_entry: &str,
    module: &str,
) -> Result<String, String> {
    rr3_entry_target(installed_entry, module)?;
    let (idx, a, b) = rr3_entry_span(text, module)?;
    let mut out = String::with_capacity(text.len() + installed_entry.len());
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i != idx {
            out.push_str(line);
            continue;
        }
        let old = &line[a..b];
        if old != facts_entry {
            return Err(format!(
                "模块 {module} 的 entry 旧值 {old:?} 与解析事实 {facts_entry:?} 不一致（疑似同名键，拒绝改写）"
            ));
        }
        out.push_str(&line[..a]);
        out.push_str(installed_entry);
        out.push_str(&line[b..]);
    }
    let re = rr3_manifest_parse(&out)
        .map_err(|errs| format!("模块 {module} 改写后不可解析: {errs:?}"))?;
    if re.entry != installed_entry {
        return Err(format!(
            "模块 {module} 改写复核不一致（得 {:?}，want {installed_entry:?}）",
            re.entry
        ));
    }
    Ok(out)
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
        let f = rr3_manifest_parse(GOOD).expect("应能读取");
        assert_eq!(
            (
                f.name.as_str(),
                f.heartbeat_ms,
                f.watchdog_ms,
                f.restart_policy.as_str()
            ),
            ("rr3-sima", 200, 1000, "always")
        );
        assert_eq!(
            (f.entry.as_str(), f.start_priority),
            ("/usr/bin/sim_peer", 40)
        );
        assert!(f.depends_required.is_empty());
        assert!(f.provides.is_empty());
    }

    /// 装机改写（B45 起打包与演练布局共用本实现）：只动 entry 引号内的值，其余
    /// 字节（行尾注释、段序、尾换行）原样；四道门各自的坏形拒且点名。
    #[test]
    fn rewrite_touches_only_the_entry_value() {
        let text = GOOD.replace(
            "entry = \"/usr/bin/sim_peer\"",
            "entry = \"/usr/bin/sim_peer\" # 行尾注释随原值保留",
        );
        let out = rr3_manifest_rewrite(
            &text,
            "/opt/rr3/modules/rr3-sima/rr3-sima",
            "/usr/bin/sim_peer",
            "rr3-sima",
        )
        .expect("合法改写应过");
        assert!(
            out.contains("entry = \"/opt/rr3/modules/rr3-sima/rr3-sima\" # 行尾注释随原值保留\n"),
            "{out}"
        );
        let strip = |s: &str| {
            s.lines()
                .filter(|l| !l.trim_start().starts_with("entry"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(strip(&text), strip(&out), "除 entry 外须逐字节零差异");
        assert!(
            rr3_manifest_rewrite(&text, "rel/bin", "/usr/bin/sim_peer", "m")
                .unwrap_err()
                .contains("不可装机")
        );
        assert!(
            rr3_manifest_rewrite(&text, "/abs/../esc", "/usr/bin/sim_peer", "m")
                .unwrap_err()
                .contains("不可装机")
        );
        assert!(rr3_manifest_rewrite(&text, "/x", "other", "m")
            .unwrap_err()
            .contains("不一致"));
        let dup = format!("{text}entry = \"/usr/bin/other\"\n");
        assert!(rr3_manifest_rewrite(&dup, "/x", "/usr/bin/sim_peer", "m")
            .unwrap_err()
            .contains("恰一条"));
    }

    /// 缺字段逐条点名且继续检查其余字段（不是遇一个就返回）
    #[test]
    fn missing_fields_are_all_reported_with_paths() {
        let text = GOOD
            .replace("entry = \"/usr/bin/sim_peer\"\n", "")
            .replace("restart_policy = \"always\"\n", "");
        let errs = rr3_manifest_parse(&text).expect_err("缺字段必须报错");
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("缺少 manifest.entry")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("缺少 resources.restart_policy")),
            "{errs:?}"
        );
    }

    /// 类型错与空串都是错误
    #[test]
    fn wrong_types_and_empty_values_are_errors() {
        let t = GOOD.replace("heartbeat_ms = 200", "heartbeat_ms = \"200\"");
        let errs = rr3_manifest_parse(&t).expect_err("类型错必须报错");
        assert_eq!(errs, vec!["health.heartbeat_ms 应为整数"], "{errs:?}");

        assert!(rr3_manifest_parse(
            &GOOD.replace("restart_policy = \"always\"", "restart_policy = \"\"")
        )
        .expect_err("空 policy 必须报错")
        .iter()
        .any(|e| e.contains("manifest") || e.contains("resources.restart_policy 不得为空")));

        assert!(rr3_manifest_parse(
            &GOOD.replace("watchdog_timeout_ms = 1000", "watchdog_timeout_ms = 0")
        )
        .expect_err("0 毫秒不可排看门狗")
        .iter()
        .any(|e| e.contains("health.watchdog_timeout_ms 应为正整数")));
    }

    /// 坏 TOML 只报一条解析错，不继续瞎猜
    #[test]
    fn unparsable_toml_is_single_error() {
        let errs = rr3_manifest_parse("[manifest\nname = ").expect_err("必须报错");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].starts_with("manifest TOML 解析失败"), "{errs:?}");
    }

    /// rrd 侧的 heartbeat>=watchdog 拦截（与 rr-registry 同规则，纵深防御）
    #[test]
    fn heartbeat_must_stay_below_watchdog_here_too() {
        let errs = rr3_manifest_parse(&GOOD.replace("heartbeat_ms = 200", "heartbeat_ms = 1000"))
            .expect_err("相等也不行");
        assert_eq!(
            errs,
            vec!["health.heartbeat_ms(1000) 必须小于 watchdog_timeout_ms(1000)"],
            "文案要带两个具体数字，运维才知道改哪个"
        );
    }

    /// depends.required 取出来，供 boot 仲裁判能力是否就位；start_priority 缺省 100
    #[test]
    fn depends_and_default_priority() {
        let text = GOOD.replace(
            "[resources]",
            "[depends]\nrequired = [\"bus.publish\", \"bus.rpc\"]\n\n[resources]",
        );
        let f = rr3_manifest_parse(&text).expect("应能读取");
        assert_eq!(
            f.depends_required,
            vec!["bus.publish".to_string(), "bus.rpc".to_string()]
        );

        let no_prio = GOOD.replace("start_priority = 40\n", "");
        assert_eq!(rr3_manifest_parse(&no_prio).unwrap().start_priority, 100);
    }

    /// [provides].capabilities：可选段（缺省空 vec），给了就逐项进 facts；
    /// 类型错必须报错（与 depends.required 同一条纪律——旧代码对非字符串项
    /// filter_map 静默吞掉是 fail-open，随 ADR-0009 落地改判错误）
    #[test]
    fn provides_is_optional_and_typed() {
        let with = GOOD.replace(
            "[health]",
            "[provides]\ncapabilities = [\"audio.capture\"]\n\n[health]",
        );
        let f = rr3_manifest_parse(&with).expect("带 provides 的 manifest 应能读取");
        assert_eq!(f.provides, vec!["audio.capture".to_string()]);

        let bad = GOOD.replace("[health]", "[provides]\ncapabilities = 3\n\n[health]");
        let errs = rr3_manifest_parse(&bad).expect_err("非数组必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("provides.capabilities 应为字符串数组")),
            "{errs:?}"
        );

        // 非字符串项逐条点名，不再被静默吞掉（fail-closed）
        let item = GOOD.replace(
            "[health]",
            "[provides]\ncapabilities = [\"ok\", 1]\n\n[health]",
        );
        let errs = rr3_manifest_parse(&item).expect_err("非字符串项必须报错");
        assert!(
            errs.iter().any(|e| e.contains("每一项都必须是非空字符串")),
            "{errs:?}"
        );
    }

    /// [rpc].exposes：可选段（缺省空 vec = 不接 rr3.call），给了就逐项进 facts
    #[test]
    fn rpc_exposes_is_optional_and_typed() {
        // 缺省：GOOD 没有 [rpc] 段
        assert!(rr3_manifest_parse(GOOD).unwrap().rpc_exposes.is_empty());
        // 合法声明
        let with = GOOD.replace("[health]", "[rpc]\nexposes = [\"rr3-interact/tts/text\", \"rr3-interact/face/expression\"]\n\n[health]");
        let f = rr3_manifest_parse(&with).expect("带 exposes 的 manifest 应能读取");
        assert_eq!(
            f.rpc_exposes,
            vec![
                "rr3-interact/tts/text".to_string(),
                "rr3-interact/face/expression".to_string()
            ]
        );
        // 类型错必须报错，不许静默当空表
        let bad = GOOD.replace(
            "[health]",
            "[rpc]\nexposes = \"rr3-interact/tts/text\"\n\n[health]",
        );
        let errs = rr3_manifest_parse(&bad).expect_err("字符串不是数组必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("rpc.exposes 应为字符串数组")),
            "{errs:?}"
        );
        let empty_item = GOOD.replace("[health]", "[rpc]\nexposes = [\"\"]\n\n[health]");
        let errs = rr3_manifest_parse(&empty_item).expect_err("空串项必须报错");
        assert!(
            errs.iter().any(|e| e.contains("每一项都必须是非空字符串")),
            "{errs:?}"
        );
    }

    /// [events].subscribes：可选段（缺省空 vec），类型错/空项 fail-closed；
    /// 运行期消费面 = 广播影子统计（不改投递行为）
    #[test]
    fn subscribes_is_optional_and_typed() {
        assert!(rr3_manifest_parse(GOOD).unwrap().subscribes.is_empty());
        let with = GOOD.replace("subscribes = []", "subscribes = [\"rr3-sense/audio/pcm\"]");
        let f = rr3_manifest_parse(&with).expect("带 subscribes 的 manifest 应能读取");
        assert_eq!(f.subscribes, vec!["rr3-sense/audio/pcm".to_string()]);
        let bad = GOOD.replace("subscribes = []", "subscribes = \"rr3-sense/audio/pcm\"");
        let errs = rr3_manifest_parse(&bad).expect_err("字符串不是数组必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("events.subscribes 应为字符串数组")),
            "{errs:?}"
        );
        let empty_item = GOOD.replace("subscribes = []", "subscribes = [\"\"]");
        let errs = rr3_manifest_parse(&empty_item).expect_err("空串项必须报错");
        assert!(
            errs.iter().any(|e| e.contains("每一项都必须是非空字符串")),
            "{errs:?}"
        );
    }

    /// entry 绝对性判定（M2 只接受绝对路径，见本计划"与 spec 的偏离 3"）。
    /// 反斜杠开头在 POSIX 里是合法文件名而非绝对路径，必须判 false——判定不随 host 漂移。
    #[test]
    fn entry_absoluteness() {
        assert!(rr3_manifest_abs("/usr/bin/sim_peer"));
        for rel in [
            "sim_peer",
            "./sim_peer",
            "../x/sim_peer",
            "",
            "\\usr\\bin\\sim_peer",
        ] {
            assert!(!rr3_manifest_abs(rel), "{rel} 不是绝对路径");
        }
    }

    /// 模块名形状：路径分量（per-unit 日志落盘）不得带分隔符或 `..`；
    /// 非 ASCII/空白/超长同拒——名字从线上注册与 boot 两路进同一解析器。
    #[test]
    fn module_name_shape_rejects_path_components() {
        let long = "x".repeat(65);
        for bad in ["../x", "a/b", ".", "..", "名字", "a b", long.as_str()] {
            let text = GOOD.replace("name = \"rr3-sima\"", &format!("name = \"{bad}\""));
            assert!(rr3_manifest_parse(&text).is_err(), "非法名应拒: {bad:?}");
        }
        for ok in ["rr3-a", "a_b.c", "x"] {
            let text = GOOD.replace("name = \"rr3-sima\"", &format!("name = \"{ok}\""));
            assert!(rr3_manifest_parse(&text).is_ok(), "合法名应过: {ok:?}");
        }
    }
}
