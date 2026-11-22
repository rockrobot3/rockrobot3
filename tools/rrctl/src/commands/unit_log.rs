//! `logs --unit`：直读 per-unit 日志文件尾部（不走总线）。
//!
//! 与 rrd 的 `<socket>.log/<unit>.log` 派生同源（`rr3_unit_log_dir` 同一规则），
//! 补上"`logs` 只读 rrd 内存环、模块自身日志在另一个面"的入口割裂。
//!
//! 拆分动因：`commands/mod.rs` 已被 ⑯ 行数门（≥500 硬线）逼近，本面独立成文件；
//! 参数/名字门/尾部截取三层各自可测。

/// `logs --unit <name> [--lines N]`：读日志尾部。模块名 fail-closed 只认
/// `rr3-<小写字母>`（路径穿越防御）；行数缺省 50、上限 2000——append 面可能
/// 很大，逐行环形截取，绝不全量读进内存再切。
pub fn rr3_unit_log_tail(bus: &str, args: &[String]) -> Result<String, String> {
    let (unit, lines) = rr3_unit_args(args)?;
    let path = std::path::Path::new(&format!("{bus}.log")).join(format!("{unit}.log"));
    rr3_tail_file(&path, lines)
}

/// 参数：`--unit <name>` 必给、`--lines N` 可选（缺省 50，收敛到 1..=2000）。
fn rr3_unit_args(args: &[String]) -> Result<(String, usize), String> {
    let mut unit = None;
    let mut lines = 50usize;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--unit" => unit = it.next().cloned(),
            "--lines" => {
                lines = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--lines 需正整数")?;
            }
            other => {
                return Err(format!(
                    "未知参数 {other}（logs --unit <name> [--lines N]）"
                ))
            }
        }
    }
    let unit = unit.ok_or("logs --unit <name>：缺模块名")?;
    if !rr3_unit_name_ok(&unit) {
        return Err(format!("模块名非法 {unit}（只认 rr3-<小写字母>）"));
    }
    Ok((unit, lines.clamp(1, 2000)))
}

/// 模块名规则（与 rrd 注册门 `^rr3-[a-z]+$` 同源）：拼路径前 fail-closed。
fn rr3_unit_name_ok(name: &str) -> bool {
    name.strip_prefix("rr3-")
        .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_lowercase()))
}

/// 读文件尾部 N 行（环形缓冲；打不开点名"模块未起过/实例路径不对"）。
fn rr3_tail_file(path: &std::path::Path, lines: usize) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).map_err(|e| {
        format!(
            "读 {} 失败: {e}（模块未起过 / --bus-path 指向的实例不对）",
            path.display()
        )
    })?;
    let mut ring: std::collections::VecDeque<String> =
        std::collections::VecDeque::with_capacity(lines);
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
        if ring.len() == lines {
            ring.pop_front();
        }
        ring.push_back(line);
    }
    if ring.is_empty() {
        return Ok(format!("（{} 无内容）", path.display()));
    }
    Ok(ring.into_iter().collect::<Vec<_>>().join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_name_gate_is_fail_closed() {
        assert!(rr3_unit_name_ok("rr3-sense"));
        assert!(rr3_unit_name_ok("rr3-brain"));
        for bad in [
            "",
            "rr3-",
            "rr3-Sense",
            "rr3-a1",
            "../etc/passwd",
            "rr3-sense/x",
            "rrd",
        ] {
            assert!(!rr3_unit_name_ok(bad), "{bad} 应被拒");
        }
    }

    #[test]
    fn unit_args_reject_bad_shapes() {
        assert!(rr3_unit_args(&[]).is_err(), "缺 --unit 应拒");
        let bad = vec!["--unit".to_string(), "../x".to_string()];
        assert!(rr3_unit_args(&bad).is_err(), "非法名应拒");
        let ok = vec![
            "--unit".to_string(),
            "rr3-sense".to_string(),
            "--lines".to_string(),
            "3".to_string(),
        ];
        assert_eq!(rr3_unit_args(&ok).unwrap(), ("rr3-sense".to_string(), 3));
        let big = vec![
            "--unit".to_string(),
            "rr3-sense".to_string(),
            "--lines".to_string(),
            "99999".to_string(),
        ];
        assert_eq!(rr3_unit_args(&big).unwrap().1, 2000, "行数应收敛到上限");
    }

    #[test]
    fn unit_tail_reads_last_lines_only() {
        let dir = std::env::temp_dir().join(format!("rr3-ctl-unitlog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rr3-sense.log");
        std::fs::write(&path, "a\nb\nc\nd\ne\n").unwrap();
        assert_eq!(rr3_tail_file(&path, 2).unwrap(), "d\ne");
        assert_eq!(rr3_tail_file(&path, 50).unwrap(), "a\nb\nc\nd\ne");
        assert!(rr3_tail_file(&dir.join("rr3-absent.log"), 5).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
