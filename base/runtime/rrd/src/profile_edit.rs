//! 装配单文本手术面（批N22 件一，敞口 15 写面半边的机制层；批N44 起自
//! `profile.rs` 拆出为独立模块——超长文件的"校验 / 手术 / 装载"三分第一刀）。
//!
//! 写口的落盘语义 = "下次 boot 生效"：本模块只做**文本手术**（保留注释与其余行
//! 原封，只动目标键行），手术后由调用方对手术产物跑装载链预核
//! （[`crate::profile::rr3_boot_inputs`]——与 boot 正门同一份裁决，写面不重复判
//! 业务合法性，判了就是第二份裁决点）。手术判据 fail-closed：目标键行缺失 /
//! 重复 / 目标域不唯一 / 缺域一律拒，不猜不插——键行缺失说明档已被改病
//! （装载期十二键全键必填保证在场），写入面把病灶誊进新值等于带病落盘。
//!
//! 对外的两个入口经 [`crate::profile`] 原路径转出（`rr3_key_line_apply` /
//! `rr3_domain_apply`）：调用方与文档锚点不变，机制归属独立。

/// 键行命中判据（单点）：行（去前导空白）以 `key` 开头且其后紧跟 `=`。
/// 词边界由"后随字符必须是 '='"保证——`severe_mv` 不会误命中 `mild_mv` 行。
fn rr3_key_line_is(line: &str, key: &str) -> bool {
    let t = line.trim_start();
    t.strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// 在行序列内把 `key` 的键行替换为 `key = new_raw`（机制核心，两手术面共用）：
/// 命中 0 行 = 拒（键缺失——装载期全键必填保证在场，缺失即档已病）；命中多行 = 拒
/// （TOML 重复键解析期就红，这里不替解析器兜底）。替换保留原行前导空白（域块/
/// 轴档键行今无缩进，保留是防御不是依赖）。
fn rr3_key_line_replace(lines: &mut [String], key: &str, new_raw: &str) -> Result<(), String> {
    let mut hits = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if rr3_key_line_is(l, key) {
            hits.push(i);
        }
    }
    match hits.len() {
        0 => Err(format!(
            "键行 {key} 缺失（装配期全键必填保证在场，缺失说明档已被改病），拒绝手术"
        )),
        1 => {
            let indent = lines[hits[0]][..lines[hits[0]].len() - lines[hits[0]].trim_start().len()]
                .to_string();
            lines[hits[0]] = format!("{indent}{key} = {new_raw}");
            Ok(())
        }
        n => Err(format!(
            "键行 {key} 出现 {n} 次（TOML 重复键是解析期红），拒绝手术"
        )),
    }
}

/// 平铺 TOML 键行手术（轴档案等无块头平表用，批N22 件一）：全文本内 `key` 键行
/// 唯一（轴档案十四键每键一行）时替换为 `key = new_raw`，注释与其余行原封。
pub fn rr3_key_line_apply(text: &str, key: &str, new_raw: &str) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    rr3_key_line_replace(&mut lines, key, new_raw).map_err(|e| format!("键行手术拒: {e}"))?;
    Ok(lines.join("\n") + if text.ends_with('\n') { "\n" } else { "" })
}

/// [[supplies.domain]] 域块键行手术（批N22 件一，写面机制）：在 `text` 中定位
/// `name = <domain>` 的唯一域块，把 `updates` 逐键行替换为 `key = new_raw`。
/// fail-closed 四拒：缺域（域名不在册——写面不做注册新域，新域是契约事件）、
/// 同名多义（域名唯一是装载期判据，多义 = 档已病）、键行缺失/重复（同上）。
/// 其余域块与其余行（含注释）原封。
pub fn rr3_domain_apply(
    text: &str,
    domain: &str,
    updates: &[(&str, &str)],
) -> Result<String, String> {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    // 块扫描（两段式）：先收块头行序号（`[` 起头行；[[supplies.domain]] 精确等值
    // 才算域块头——GOOD 锚形状，装载期验收过的档保证），再按下一块头定界块行范围。
    let heads: Vec<(usize, bool)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, raw)| {
            let t = raw.trim_start();
            if t == "[[supplies.domain]]" {
                Some((i, true))
            } else if t.starts_with('[') {
                Some((i, false))
            } else {
                None
            }
        })
        .collect();
    let mut matched: Vec<(usize, usize)> = Vec::new(); // 命中块行范围（头行+1 .. 下一块头）
    for (idx, &(head_i, is_domain)) in heads.iter().enumerate() {
        if !is_domain {
            continue;
        }
        let end = heads.get(idx + 1).map(|&(h, _)| h).unwrap_or(lines.len());
        let is_match = lines[head_i + 1..end].iter().any(|l| {
            rr3_key_line_is(l, "name")
                && l.split_once('=')
                    // 域名是引号串（GOOD 形状），允许行内注释——取首对引号内内容
                    .and_then(|(_, v)| v.trim().strip_prefix('"'))
                    .and_then(|s| s.split_once('"'))
                    .map(|(inner, _)| inner == domain)
                    .unwrap_or(false)
        });
        if is_match {
            matched.push((head_i + 1, end));
        }
    }
    match matched.len() {
        0 => Err(format!(
            "域 {domain:?} 不在 [[supplies.domain]] 在册（缺域拒——新域是契约事件，不经写口注册）"
        )),
        1 => {
            let (s, e) = matched[0];
            let mut out = lines;
            for (key, new_raw) in updates {
                // 替换限定在命中块行范围内——全文本替换依赖"域块键名不在他处出现"
                // 的隐式前提，不依赖（批N22 件一 self-review 修正）
                rr3_key_line_replace(&mut out[s..e], key, new_raw)
                    .map_err(|e| format!("域 {domain:?} 键行手术拒: {e}"))?;
            }
            Ok(out.join("\n") + if text.ends_with('\n') { "\n" } else { "" })
        }
        n => Err(format!(
            "域名 {domain:?} 命中 {n} 个 [[supplies.domain]] 块（装载期域名唯一判据破，档已病），拒绝手术"
        )),
    }
}
