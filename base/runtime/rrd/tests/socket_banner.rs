#![cfg(unix)]
//! 启动横幅回显的必须是**解析后的绝对套接字路径**（Task 3 评审遗留）。
//!
//! 为什么值得单独一条：`--socket rel.sock` 时 daemon 绑的是"当前目录 + rel.sock"，
//! 而横幅按 CLI 原词回显 `rel.sock`。运维照横幅去连，连的是自己 shell 的 CWD——
//! 横幅与 bind 各说一套，比报错更难查（rrd 明明活着）。
//! 这条用例把两者钉回同一个值。

mod support;

use std::path::{Path, PathBuf};
use support::*;

/// 横幅与监听行都必须回显解析后的绝对路径，且那个路径真的能连上并注册
#[test]
fn banner_and_listen_line_show_the_resolved_absolute_path() {
    let name = format!("rr3-m2-{}-banner.sock", std::process::id());
    let abs = PathBuf::from("/tmp").join(&name);
    // 子进程 CWD = /tmp，CLI 原词只给文件名：rr3_spawn 的就绪判据等的就是"监听 <绝对路径>"，
    // 横幅若按原词回显，这里 3s 超时并把实际读到的日志打出来。
    let mut rrd = RrdTest::rr3_spawn(&abs, Some(Path::new("/tmp")), Path::new(&name));

    let banner = rrd.rr3_wait_log("rrd 0.0.1");
    assert!(
        banner.contains(&format!("bus {},", abs.display())),
        "版本横幅的 bus 字段也必须是解析后的绝对路径，实际: {banner}"
    );

    // 只断言"字符串像绝对路径"不够：连得上、注册得了才证明横幅说的就是绑的那个地方
    let mut c = rrd.rr3_connect();
    rr3_send(&mut c, &rr3_register_frame(&rr3_manifest("rr3-sima", 100, 500, "always"), "rr3-sima"));
    assert_eq!(rr3_read_rpc(&mut c)["result"]["src_task"], 2, "按横幅里的路径连得上、注册得了");
}
