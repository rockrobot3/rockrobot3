//! 子命令表。
//!
//! v0.0.1 每个命令只回显"它将问谁、问什么"，把输出形状先定下来；真正的帧往返随
//! M2（rrd 的 UDS 上线）。查询走 `dst_task = 0` 广播，与 rr3-runtime 的
//! 心跳/状态广播同一形状（ADR-0006）。

use rr_bus::RR3_BROADCAST_DST_TASK;

pub const COMMANDS: [&str; 4] = ["status", "health", "logs", "ota"];

pub fn run(cmd: &str, bus: &str) -> Result<String, String> {
    match cmd {
        "status" => Ok(format!(
            "status: 向 {bus} 广播查询在册模块（dst_task={RR3_BROADCAST_DST_TASK}），实现随 M2"
        )),
        "health" => Ok(format!(
            "health: 订阅 {bus} 的 rr3-runtime/system/heartbeat 并汇总各模块 phase，实现随 M2"
        )),
        "logs" => Ok(format!("logs: 经 {bus} 向 rrd 取模块日志尾部，实现随 M2")),
        "ota" => Ok(
            "ota: 触发模块级 staging 更新（rr-ota 状态机 + 断电安全判定），实现随 M6".to_string(),
        ),
        other => Err(format!(
            "未知命令 {other}，可用：{}",
            COMMANDS.join(" | ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_command_is_dispatchable() {
        for cmd in COMMANDS {
            assert!(run(cmd, "/tmp/t.sock").is_ok(), "{cmd} 在 COMMANDS 里却没有分支");
        }
    }

    #[test]
    fn unknown_command_names_the_alternatives() {
        let err = run("reboot", "/tmp/t.sock").unwrap_err();
        assert!(err.contains("status") && err.contains("ota"), "错误文案没给出可用命令: {err}");
    }
}
