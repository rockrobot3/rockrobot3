//! 证据引用校验（brain B-15，批N4 件一）：真板对拍/实测证据的唯一形制真值。
//!
//! D11 的 `armed=true` 界面门（"必须引用一条真板对拍证据的编号"）与
//! `severe_mv`/`current_ceil_ma` 两个安全写同规则——它们要可执法，仓内必须先有
//! "一条证据长什么样、住哪儿、怎么被引用"的机制事实（全案 §18 F-24：没有容器，
//! 门只能靠人肉判）。本模块就是那个容器侧的**机制单点**：
//!
//! - 命名空间与编号规则的**策略原文**住 conventions §6.1（唯一登记处，本模块不复制）；
//! - 本模块只持"一条引用怎么算合法"的可执行判据：形制 → 存在 → frontmatter
//!   六键齐 → `form` 与形态目录名一致，四道全过才算一条可引用的证据；
//! - "哪些写必须带引用"是策略，归各写面（面板写面随批N4 件九端点挂接，装载期
//!   随落盘数据的 evidence 引用字段核验）——机制与策略分离。
//!
//! 归宿说明：面板写面（rr3-interact）与装载期（rrd）两个消费面经依赖面纪律
//! （模块互不依赖、rr-hal-api 为 no_std 不载文件面）只剩本 crate 共同可达，
//! 故校验器唯一实现住这里——跨文件同形函数须指向共享件（基-30 判据）。
//!
//! 本模块不产出、不代答任何板上结论：它只规定结论的**容器形制**，证据本身
//! 随 0.0.6/0.0.7 验证窗产出。

use std::format;
use std::fs;
use std::path::Path;
use std::string::{String, ToString};
use std::vec::Vec;

/// frontmatter 六必填键（语义映射：形态/格或角色/观测方法/结论/日期/取证人，
/// 规则原文住 conventions §6.1）。解析面只认这六键：未知键红、缺一红。
const RR3_EVIDENCE_KEYS: [&str; 6] = [
    "form",
    "slot_or_role",
    "method",
    "conclusion",
    "date",
    "recorder",
];

/// 证据引用形制判定：`<形态>/<编号>`——形态段非空且不含空白；编号段恰好
/// 四位十进制数字（0001 起，四位零填充）。形制不合的引用在任何消费面直接拒，
/// 不到"查文件"那一步。
pub fn rr3_evidence_ref_ok(reference: &str) -> bool {
    let Some((form, num)) = reference.split_once('/') else {
        return false;
    };
    // 形态段：非空、无空白（斜杠不可能出现在本段——split_once 只切第一个 '/'）
    if !rr3_ref_form_ok(form) {
        return false;
    }
    // 编号段：恰好四位数字（同时天然排除第二个 '/' 与字母）
    num.len() == 4 && num.bytes().all(|b| b.is_ascii_digit())
}

/// 形态段合法：非空且不含空白。
fn rr3_ref_form_ok(form: &str) -> bool {
    !form.is_empty() && !form.chars().any(char::is_whitespace)
}

/// 证据引用完整校验：`root`＝证据容器根（仓内上下文为 `docs/evidence`），
/// `reference`＝`<形态>/<编号>`。四道判据全过才返回 `Ok(())`：
///
/// 1. 形制（[`rr3_evidence_ref_ok`]）；
/// 2. 文件存在且可读（`<root>/<形态>/<编号>.md`）；
/// 3. frontmatter 合法（`+++` 定界、六键齐、无未知键、值为一行双引号文本）；
/// 4. `form` 值与引用中的形态目录名一致（文件搬家不改 form = 立刻红）。
///
/// fail-closed：任一道不过即点名拒绝，无豁免通道——与"缺键即红"同列语义
/// （B-15 ②）。
pub fn rr3_evidence_verify(root: &Path, reference: &str) -> Result<(), String> {
    let (form, path) = rr3_ev_ref(root, reference)?;
    rr3_ev_check(&path, &form)
}

/// 形制门 + 落盘路径（形制不合在查文件之前就拒）。
fn rr3_ev_ref(root: &Path, reference: &str) -> Result<(String, std::path::PathBuf), String> {
    if !rr3_evidence_ref_ok(reference) {
        return Err(format!(
            "证据引用 {reference:?} 形制不合法（应为 <形态>/<四位编号>，形态段不含空白）"
        ));
    }
    let (form, _) = reference.split_once('/').expect("形制已过，'/' 必在");
    Ok((form.to_string(), root.join(format!("{reference}.md"))))
}

/// 文件存在 + frontmatter 合法 + form 与形态目录一致（三道路径判据）。
fn rr3_ev_check(path: &Path, form: &str) -> Result<(), String> {
    let text = rr3_ev_read(path)?;
    let fields = rr3_evidence_head(&text)
        .map_err(|e| format!("证据文件 {} frontmatter 不合法: {e}", path.display()))?;
    let declared = fields
        .iter()
        .find(|(k, _)| *k == "form")
        .map(|(_, v)| *v)
        .expect("frontmatter 已验六键齐，form 必在");
    rr3_ev_declared_chk(path, declared, form)
}

/// 读证据文件全文；读不到即点名路径（文案与旧内联 map_err 逐字同）。
fn rr3_ev_read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("证据文件读不到（{}）: {e}", path.display()))
}

/// 声明值与引用形态目录一致（文件搬家不改 form = 立刻红）。
fn rr3_ev_declared_chk(path: &Path, declared: &str, form: &str) -> Result<(), String> {
    if declared != form {
        return Err(format!(
            "证据文件 {} 的 form = {declared:?} 与引用形态目录 {form:?} 不一致",
            path.display()
        ));
    }
    Ok(())
}

/// frontmatter 解析（TOML 子集，手切零依赖——本 crate 主依赖保持为零）：
/// 首行 `+++` 开、下一行 `+++` 收，之间每行 `key = "value"`；key 须在册、
/// value 为一行双引号文本（内部不许出现双引号）；六键齐、缺一红、未知键红。
fn rr3_evidence_head(text: &str) -> Result<Vec<(&str, &str)>, String> {
    let mut lines = text.lines();
    rr3_ev_open(lines.next())?;
    let fields = rr3_ev_fields(lines)?;
    rr3_ev_required(&fields)?;
    Ok(fields)
}

/// 首行定界：必须是 `+++`。
fn rr3_ev_open(first: Option<&str>) -> Result<(), String> {
    match first {
        Some(l) if l.trim() == "+++" => Ok(()),
        _ => Err("首行必须是 +++（frontmatter 定界）".to_string()),
    }
}

/// 收界前逐行取字段（空行跳过，收界 `+++` 即返回）；缺收界即红。
fn rr3_ev_fields<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<Vec<(&'a str, &'a str)>, String> {
    let mut fields: Vec<(&str, &str)> = Vec::new();
    for raw in lines {
        let line = raw.trim();
        if rr3_ev_close(line) {
            return Ok(fields);
        }
        rr3_ev_line(line, &mut fields)?;
    }
    Err("frontmatter 缺收界 +++".to_string())
}

/// 收界行：`+++` 即 frontmatter 结束。
fn rr3_ev_close(line: &str) -> bool {
    line == "+++"
}

/// 非收界行：空行跳过，其余取一条字段（键值形状交给 `rr3_ev_field`）。
fn rr3_ev_line<'a>(line: &'a str, fields: &mut Vec<(&'a str, &'a str)>) -> Result<(), String> {
    if line.is_empty() {
        return Ok(());
    }
    fields.push(rr3_ev_field(line)?);
    Ok(())
}

/// 单字段行：`key = "一行文本"`——键在册、值双引号包一行、值内不许再出现双引号
fn rr3_ev_field(line: &str) -> Result<(&str, &str), String> {
    let Some((key, value)) = line.split_once('=') else {
        return Err(format!("字段行缺少 '=': {line:?}"));
    };
    let key = key.trim();
    let value = value.trim();
    if !RR3_EVIDENCE_KEYS.contains(&key) {
        return Err(format!(
            "未知键 {key:?}（只认 {}）",
            RR3_EVIDENCE_KEYS.join("/")
        ));
    }
    let inner = rr3_ev_quoted(key, value)?;
    Ok((key, inner))
}

/// 双引号值：剥离首尾引号，值内不许再出现双引号（一行文本的形制）。
fn rr3_ev_quoted<'a>(key: &str, value: &'a str) -> Result<&'a str, String> {
    let inner = rr3_quoted_inner(key, value)?;
    if inner.contains('"') {
        return Err(format!("{key} 的值内不许出现双引号: {value:?}"));
    }
    Ok(inner)
}

/// 剥离首尾双引号；不成对即点名。
fn rr3_quoted_inner<'a>(key: &str, value: &'a str) -> Result<&'a str, String> {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .ok_or_else(|| format!("{key} 的值必须是一行双引号文本: {value:?}"))
}

/// 必填键核对：字段集必须覆盖 [`RR3_EVIDENCE_KEYS`] 全集
fn rr3_ev_required(fields: &[(&str, &str)]) -> Result<(), String> {
    for key in RR3_EVIDENCE_KEYS {
        if !fields.iter().any(|(k, _)| *k == key) {
            return Err(format!("缺必填键 {key:?}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 测试容器根：temp_dir 下按进程 id + 用例名隔离，尾夹具手清（与 uds.rs
    /// 测试同范式，本 crate dev-dependencies 保持为零）。
    fn ut_root(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rr3_evidence_ut_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("对拍")).expect("造形态目录");
        dir
    }

    fn ut_write(root: &Path, reference: &str, body: &str) {
        let path = root.join(format!("{reference}.md"));
        fs::write(path, body).expect("写证据文件");
    }

    const GOOD_FM: &str = "+++\nform = \"对拍\"\nslot_or_role = \"power.rail\"\nmethod = \"万用表实测与面板读数对拍\"\nconclusion = \"severe_mv=3300 与实测一致\"\ndate = \"2026-10-19\"\nrecorder = \"lei_huang\"\n+++\n\n正文。\n";

    #[test]
    fn ref_wellformed_accepts_form_plus_four_digits() {
        assert!(rr3_evidence_ref_ok("对拍/0001"));
        assert!(rr3_evidence_ref_ok("实测/9999"));
        assert!(!rr3_evidence_ref_ok("对拍/0001/x"), "编号段含斜杠红");
        assert!(!rr3_evidence_ref_ok("对拍/001"), "三位编号红");
        assert!(!rr3_evidence_ref_ok("对拍/00001"), "五位编号红");
        assert!(!rr3_evidence_ref_ok("对拍/00a1"), "编号含字母红");
        assert!(!rr3_evidence_ref_ok("对拍0001"), "缺斜杠红");
        assert!(!rr3_evidence_ref_ok("/0001"), "形态段空红");
        assert!(!rr3_evidence_ref_ok("对 拍/0001"), "形态段含空白红");
        assert!(!rr3_evidence_ref_ok("对拍/"), "编号段空红");
        assert!(!rr3_evidence_ref_ok("对拍"), "整体缺编号红");
    }

    #[test]
    fn verify_passes_a_wellformed_referenced_file() {
        let root = ut_root("pass");
        ut_write(&root, "对拍/0001", GOOD_FM);
        assert_eq!(rr3_evidence_verify(&root, "对拍/0001"), Ok(()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_rejects_missing_file_by_name() {
        let root = ut_root("missing");
        let e = rr3_evidence_verify(&root, "对拍/0001").expect_err("文件不存在必须红");
        assert!(e.contains("读不到"), "文案点名读不到: {e}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_rejects_form_mismatch() {
        let root = ut_root("form_mismatch");
        // 文件在 实测/ 下但 form 写"对拍"——搬家不改 form 即红
        fs::create_dir_all(root.join("实测")).expect("造第二形态目录");
        ut_write(&root, "实测/0001", GOOD_FM);
        let e = rr3_evidence_verify(&root, "实测/0001").expect_err("form 与目录不一致必须红");
        assert!(e.contains("不一致"), "文案点名不一致: {e}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_rejects_bad_frontmatter_variants() {
        let root = ut_root("bad_fm");
        // 缺必填键
        ut_write(
            &root,
            "对拍/0001",
            &GOOD_FM.replace("recorder = \"lei_huang\"\n", ""),
        );
        assert!(rr3_evidence_verify(&root, "对拍/0001")
            .expect_err("缺键必须红")
            .contains("缺必填键"),);
        // 未知键
        ut_write(
            &root,
            "对拍/0002",
            &GOOD_FM.replace("+++\n", "+++\nextra = \"x\"\n"),
        );
        assert!(rr3_evidence_verify(&root, "对拍/0002")
            .expect_err("未知键必须红")
            .contains("未知键"),);
        // 无 frontmatter 定界
        ut_write(&root, "对拍/0003", "纯正文，没有 frontmatter。\n");
        assert!(rr3_evidence_verify(&root, "对拍/0003")
            .expect_err("缺定界必须红")
            .contains("+++"),);
        // 缺收界
        ut_write(&root, "对拍/0004", &GOOD_FM.replace("+++\n\n正文。\n", ""));
        assert!(rr3_evidence_verify(&root, "对拍/0004")
            .expect_err("缺收界必须红")
            .contains("收界"),);
        // 值不含引号
        ut_write(
            &root,
            "对拍/0005",
            &GOOD_FM.replace("form = \"对拍\"", "form = 对拍"),
        );
        assert!(rr3_evidence_verify(&root, "对拍/0005")
            .expect_err("值缺引号必须红")
            .contains("双引号"),);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_rejects_malformed_reference_before_touching_fs() {
        let root = ut_root("bad_ref");
        ut_write(&root, "对拍/0001", GOOD_FM);
        // 形制不合的引用直接拒，文案不提文件——形制这道门在查文件之前
        let e = rr3_evidence_verify(&root, "对拍/1").expect_err("三位编号必须红");
        assert!(e.contains("形制"), "文案点名形制: {e}");
        let _ = fs::remove_dir_all(&root);
    }
}
