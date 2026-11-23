// SPDX-License-Identifier: Apache-2.0
//! `logs --unit`：直读 per-unit 日志文件尾部（不走总线）。
//!
//! 与 rrd 的 `<socket>.log/<unit>.log` 派生同源（`rr3_unit_log_dir` 同一规则），
//! 补上"`logs` 只读 rrd 内存环、模块自身日志在另一个面"的入口割裂。
//!
//! 拆分动因：`commands/mod.rs` 已被 ⑯ 行数门（≥500 硬线）逼近，本面独立成文件；
//! 参数/名字门/尾部截取三层各自可测。

/// `logs --unit <name>`：读日志尾部（固定最近 50 行）。模块名 fail-closed 只认
/// `rr3-<小写字母>`（路径穿越防御）。
pub fn rr3_unit_log_tail(bus: &str, args: &[String]) -> Result<String, String> {
    let unit = rr3_arg(args, "--unit").ok_or("logs --unit <name>：缺模块名")?;
    if !rr3_unit_name_ok(unit) {
        return Err(format!("模块名非法 {unit}（只认 rr3-<小写字母>）"));
    }
    let path = std::path::Path::new(&format!("{bus}.log")).join(format!("{unit}.log"));
    rr3_tail_file(&path)
}

/// `--flag` 后面的值（缺 flag 或缺值 = None）。
fn rr3_arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

/// 模块名规则（与 rrd 注册门 `^rr3-[a-z]+$` 同源）：拼路径前 fail-closed。
fn rr3_unit_name_ok(name: &str) -> bool {
    name.strip_prefix("rr3-")
        .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_lowercase()))
}

/// 读文件尾部 50 行：一次读入后按行切片（per-unit 文件有轮转上界，量级 MB）。
fn rr3_tail_file(path: &std::path::Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "读 {} 失败: {e}（模块未起过 / --bus-path 指向的实例不对）",
            path.display()
        )
    })?;
    let all: Vec<&str> = text.lines().collect();
    let tail = all[all.len().saturating_sub(50)..].join("\n");
    if tail.is_empty() {
        return Ok(format!("（{} 无内容）", path.display()));
    }
    Ok(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_fail_closed() {
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
    fn unit_arg_miss_rej() {
        assert!(
            rr3_unit_log_tail("/tmp/x.sock", &[]).is_err(),
            "缺 --unit 应拒"
        );
        let bad = vec!["--unit".to_string(), "../x".to_string()];
        assert!(
            rr3_unit_log_tail("/tmp/x.sock", &bad).is_err(),
            "非法名应拒"
        );
    }

    #[test]
    fn unit_tail_last_lines() {
        let dir = std::env::temp_dir().join(format!("rr3-ctl-unitlog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rr3-sense.log");
        std::fs::write(&path, "a\nb\nc\nd\ne\n").unwrap();
        assert_eq!(rr3_tail_file(&path).unwrap(), "a\nb\nc\nd\ne");
        assert!(rr3_tail_file(&dir.join("rr3-absent.log")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
