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
    rr3_str_take(v, path, errs)
}

/// 必填字符串值型：空串与类型错两类点名（均回空串）。
fn rr3_str_take(v: &Value, path: &str, errs: &mut Vec<String>) -> String {
    let Some(s) = v.as_str() else {
        errs.push(format!("{path} 应为字符串"));
        return String::new();
    };
    if s.is_empty() {
        errs.push(format!("{path} 不得为空字符串"));
        return String::new();
    }
    s.to_string()
}

fn req_int(root: &Value, path: &str, errs: &mut Vec<String>) -> u64 {
    let Some(v) = rr3_req_present(root, path, errs) else {
        return 0;
    };
    rr3_int_take(v, path, errs)
}

/// 必填整数值型：非正与类型错两类点名（均回 0）。
fn rr3_int_take(v: &Value, path: &str, errs: &mut Vec<String>) -> u64 {
    let Some(n) = v.as_integer() else {
        errs.push(format!("{path} 应为整数"));
        return 0;
    };
    if n <= 0 {
        errs.push(format!("{path} 应为正整数，实际 {n}"));
        return 0;
    }
    n as u64
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
    let out = rr3_entry_swap(text, (idx, a, b), installed_entry, facts_entry, module)?;
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

/// 只改目标行引号内的值：其余字节原样；旧值对账失败即拒。
fn rr3_entry_swap(
    text: &str,
    span: (usize, usize, usize),
    installed_entry: &str,
    facts_entry: &str,
    module: &str,
) -> Result<String, String> {
    let (idx, a, b) = span;
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
    Ok(out)
}

#[cfg(test)]
mod tests;
