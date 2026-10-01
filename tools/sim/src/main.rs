//! rr3-sim —— x86 全链路联调入口（F4 完整化）。
//!
//! 规划（architecture.md §3）：mock HAL + 假模块，让整条链路在无板设备上跑起来。
//! 本入口**脚本化**走一遍真实链路（全部真进程、真套接字、真契约）：
//!
//! 1. 起 rrd，`--manifest` 带 **rr3-sense**（boot spawn，resident 常驻喂狗）；
//! 2. 等 rr3-sense 到 `running`（轮询 `rr3-ctl status`，一次性 ctl 连接）；
//! 3. **外部**起第二个真模块 **rr3-interact**（sim_peer + `publish_once`），
//!    发布 `rr3-interact/asr/text` 后退场——事件真的流过一次总线；
//! 4. `rr3-ctl` 四命令巡检：status（断言 `denied_publish = 0`——发布被拒会在这里现形）、
//!    health、logs、ota（恒拒随 M6，预期非 0 退出）；
//! 5. 报告 + 收尾（`--keep` 保留现场）。
//!
//! sim 设备源（`devices.rs`，rr-hal 的 sim 后端消费方）保留：报告里带一段
//! 静音流的时长换算，证明"sim 与 linux 后端交出同一种流"这条不变量仍有落点。

#[cfg(not(unix))]
fn main() {
    eprintln!("rr3-sim: 全链路演练依赖 UDS，仅 Unix 可用");
    std::process::exit(2);
}

// 演练编排依赖 UDS 与真进程（Unix）；drill 是纯逻辑（参数/路径/解析），全平台参与编译，
// 它的单测因此 Windows 上也能跑到（④ 步的编译面与 ④ 之外的本地测试都受益）。
#[cfg(unix)]
mod brain;
#[cfg(unix)]
mod devices;
mod drill;

#[cfg(unix)]
mod run {
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use crate::drill::{
        rr3_default_bins, rr3_find_repo_root, rr3_manifest_text, rr3_parse_status,
        rr3_require_bins, rr3_section, rr3_wait_modules, RR3_SIM_B_TOPIC, RR3_SIM_MODULE_A,
        RR3_SIM_MODULE_B,
    };

    /// 巡检轮询的上界：冷启动（首次加载契约 + 起子进程）在慢机器上也要够用
    const RR3_READY_DEADLINE: Duration = Duration::from_secs(15);
    /// 模块健康参数：心跳 200ms / 看门狗 1000ms——比生产紧、比测试宽，演练看得见心跳数
    const RR3_SIM_HB_MS: u64 = 200;
    const RR3_SIM_WD_MS: u64 = 1000;

    pub fn rr3_sim_main(args: &[String]) -> i32 {
        let a = match crate::drill::rr3_sim_args_parse(args) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("rr3-sim: {e}");
                return 2;
            }
        };
        // --brain 走 M4 四真进程演练（ADR-0014 裁定 4），与两面演练互斥
        if a.brain {
            return match crate::brain::rr3_brain_drill(&a) {
                Ok(report) => {
                    println!("{report}");
                    0
                }
                Err(e) => {
                    eprintln!("rr3-sim: {e}");
                    1
                }
            };
        }
        match rr3_sim_drill(&a) {
            Ok(report) => {
                println!("{report}");
                0
            }
            Err(e) => {
                eprintln!("rr3-sim: {e}");
                1
            }
        }
    }

    fn rr3_sim_drill(a: &crate::drill::Rr3SimArgs) -> Result<String, String> {
        // ---- 0. 定位：仓根与三个二进制 ----
        let repo = match &a.repo {
            Some(p) => p.clone(),
            None => std::env::current_dir()
                .ok()
                .and_then(|cwd| rr3_find_repo_root(&cwd))
                .ok_or("找不到仓根（缺 contracts/topics.toml）：请在仓内运行，或 --repo <路径>")?,
        };
        let (d_rrd, d_peer, d_rrctl) = rr3_default_bins(&repo);
        let rrd = a.rrd.clone().unwrap_or(d_rrd);
        let peer = a.sim_peer.clone().unwrap_or(d_peer);
        let rrctl = a.rrctl.clone().unwrap_or(d_rrctl);
        rr3_require_bins(&rrd, &peer, &rrctl)?;

        // ---- 1. 夹具：套接字 + 两份 manifest（entry 用 sim_peer 的绝对路径） ----
        let workdir = match &a.socket {
            Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
            None => std::env::temp_dir().join(format!("rr3-sim-{}", std::process::id())),
        };
        std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
        let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
        let _ = std::fs::remove_file(&socket);
        let m_a = workdir.join(format!("{RR3_SIM_MODULE_A}.toml"));
        let m_b = workdir.join(format!("{RR3_SIM_MODULE_B}.toml"));
        std::fs::write(
            &m_a,
            rr3_manifest_text(
                RR3_SIM_MODULE_A,
                &peer,
                RR3_SIM_HB_MS,
                RR3_SIM_WD_MS,
                "always",
                &[],
            ),
        )
        .map_err(|e| format!("写 {m_a:?} 失败: {e}"))?;
        std::fs::write(
            &m_b,
            rr3_manifest_text(
                RR3_SIM_MODULE_B,
                &peer,
                RR3_SIM_HB_MS,
                RR3_SIM_WD_MS,
                "never",
                &[RR3_SIM_B_TOPIC],
            ),
        )
        .map_err(|e| format!("写 {m_b:?} 失败: {e}"))?;

        // ---- 2. 起 rrd：boot spawn 模块 A（resident） ----
        let log_path = workdir.join("rrd.log");
        let log =
            std::fs::File::create(&log_path).map_err(|e| format!("建 {log_path:?} 失败: {e}"))?;
        let mut rrd_child = Command::new(&rrd)
            .arg("--socket")
            .arg(&socket)
            .arg("--manifest")
            .arg(&m_a)
            .arg("--boot-role")
            .arg("resident")
            .args(["--pre-register-timeout-ms", "3000"])
            .stdout(Stdio::from(log))
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("起 rrd 失败: {e}"))?;
        println!(
            "rr3-sim: rrd 已起 pid={}，socket={}",
            rrd_child.id(),
            socket.display()
        );
        println!(
            "rr3-sim: boot manifest {}（resident，等 running）",
            m_a.display()
        );

        let mut report = String::new();
        let outcome = (|| -> Result<(), String> {
            // ---- 3. 轮询等 A 到 running（rr3-ctl 尚未就绪时连接失败按"还没好"重试） ----
            rr3_wait_modules(
                || rr3_ctl_rows(&rrctl, &socket),
                &[(RR3_SIM_MODULE_A, "running")],
                RR3_READY_DEADLINE,
            )
            .map_err(|e| rr3_with_log_tail(&e, &log_path))?;
            report.push_str(rr3_section("模块 A（boot spawn → running）已就绪").as_str());

            // ---- 4. 外部起模块 B（publish_once：发布一条真事件后退场） ----
            let st = Command::new(&peer)
                .env("RR3_BUS_PATH", &socket)
                .env("RR3_MANIFEST", &m_b)
                .env("RR3_SIM_PEER_ROLE", "publish_once")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .map_err(|e| format!("起 {RR3_SIM_MODULE_B} 失败: {e}"))?;
            if !st.success() {
                return Err(format!(
                    "{RR3_SIM_MODULE_B} 退出码 {st}（详见上方输出与 {log_path:?}）"
                ));
            }
            report.push_str(&format!(
                "{RR3_SIM_MODULE_B} 已发布 {RR3_SIM_B_TOPIC} 并退场（exit {st}）\n"
            ));

            // ---- 5. rr3-ctl 四命令巡检 ----
            report.push_str(rr3_section("rr3-ctl status 巡检").as_str());
            let status_out = rr3_ctl_out(&rrctl, &socket, "status")?;
            report.push_str(&status_out);
            let rows = rr3_ctl_rows(&rrctl, &socket);
            // denied_publish = 0 是"事件真被接受"的线上证据：发布被拒会在这里现形
            if !status_out.contains("denied_publish = 0") {
                return Err(format!(
                    "denied_publish ≠ 0：{RR3_SIM_B_TOPIC} 的发布被 rrd 拒了，见日志 {log_path:?}\n{status_out}"
                ));
            }
            if !rows
                .iter()
                .any(|(n, _, p)| n == RR3_SIM_MODULE_A && p == "running")
            {
                return Err(format!(
                    "{RR3_SIM_MODULE_A} 不在 running（巡检期间掉线？）：{rows:?}"
                ));
            }
            // B 退场后的终相**不作死断言**：sim_peer 的 publish_once 退场不排空接收队列，
            // 未读广播帧会让对端把 FIN 变 RST（sim_peer.rs 的 abort 支注释记了同一形状），
            // 终相随收尾路径在 unregistered/failed 两可—— rr3-sim 只断言"不在 running 了"。
            if let Some((n, _, p)) = rows.iter().find(|(n, _, _)| n == RR3_SIM_MODULE_B) {
                if p == "running" {
                    return Err(format!("{n} 退场后仍 running（EOF 没被记账？）：{rows:?}"));
                }
                report.push_str(&format!(
                    "注：{n} 终相 = {p}（退场未排空 → 终相两可，见 sim_peer.rs）\n"
                ));
            }

            report.push_str(rr3_section("rr3-ctl health").as_str());
            report.push_str(&rr3_ctl_out(&rrctl, &socket, "health")?);
            report.push_str(rr3_section("rr3-ctl logs（rrd 日志环尾部）").as_str());
            report.push_str(&rr3_ctl_out(&rrctl, &socket, "logs")?);
            report.push_str(rr3_section("rr3-ctl ota（预期恒拒，随 M6）").as_str());
            match rr3_ctl_maybe_fail(&rrctl, &socket, "ota") {
                Ok(out) => report.push_str(&format!("意外成功：\n{out}")),
                Err((code, out)) => report.push_str(&format!("按预期被拒（exit {code}）:\n{out}")),
            }

            // ---- 6. sim 设备源不变量（rr-hal 同一种流） ----
            let stream = crate::devices::rr3_sim_silence(1600);
            report.push_str(rr3_section("sim 设备源不变量").as_str());
            report.push_str(&format!(
                "backend=sim，1600 帧 @16kHz → {}ms（与 linux 后端同一种流）\n",
                stream.rr3_pcm_duration_ms()
            ));
            Ok(())
        })();

        let result = match outcome {
            Ok(()) => {
                report.push_str(rr3_section("演练结论").as_str());
                report.push_str("全链路 OK：rrd + boot spawn + 外部发布 + rr3-ctl 巡检\n");
                Ok(())
            }
            Err(e) => Err(e),
        };
        // ---- 收尾：杀 rrd；resident 模块喂狗失败自退；--keep 保留现场 ----
        if a.keep {
            println!(
                "rr3-sim: --keep 保留现场：socket={} log={}",
                socket.display(),
                log_path.display()
            );
            println!(
                "rr3-sim: 手动收尾：kill {}；rm -rf {workdir:?}",
                rrd_child.id()
            );
        } else {
            let _ = rrd_child.kill();
            let _ = rrd_child.wait();
        }
        result.map(|()| report)
    }

    /// 跑一次 rr3-ctl 子命令，stdout 原样带回（巡检报告的素材）
    fn rr3_ctl_out(rrctl: &Path, socket: &Path, cmd: &str) -> Result<String, String> {
        let out = Command::new(rrctl)
            .arg("--bus-path")
            .arg(socket)
            .arg(cmd)
            .output()
            .map_err(|e| format!("跑 rr3-ctl {cmd} 失败: {e}"))?;
        let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            s.push_str(&String::from_utf8_lossy(&out.stderr));
            return Err(format!(
                "rr3-ctl {cmd} 意外失败（exit {}）:\n{s}",
                out.status
            ));
        }
        if !s.ends_with('\n') {
            s.push('\n');
        }
        Ok(s)
    }

    /// 预期会失败的那一类（ota）：把 (退出码, 合并输出) 带回来由调用方裁定
    fn rr3_ctl_maybe_fail(rrctl: &Path, socket: &Path, cmd: &str) -> Result<String, (i32, String)> {
        let out = Command::new(rrctl)
            .arg("--bus-path")
            .arg(socket)
            .arg(cmd)
            .output()
            .map_err(|e| (1, format!("跑 rr3-ctl {cmd} 失败: {e}")))?;
        let code = out.status.code().unwrap_or(1);
        let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
        s.push_str(&String::from_utf8_lossy(&out.stderr));
        if out.status.success() {
            Ok(s)
        } else {
            Err((code, s))
        }
    }

    /// 轮询用的"一次快照"：连接失败（rrd 还没起）当空表处理，由 deadline 收口
    fn rr3_ctl_rows(rrctl: &Path, socket: &Path) -> Vec<(String, u64, String)> {
        Command::new(rrctl)
            .arg("--bus-path")
            .arg(socket)
            .arg("status")
            .output()
            .map(|o| rr3_parse_status(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }

    /// 失败时带上 rrd.log 尾部（排查起点，不让人再跑一遍去找日志）
    fn rr3_with_log_tail(msg: &str, log_path: &Path) -> String {
        let tail = std::fs::read_to_string(log_path)
            .map(|s| {
                let lines: Vec<&str> = s.lines().collect();
                let start = lines.len().saturating_sub(15);
                lines[start..].join("\n")
            })
            .unwrap_or_else(|e| format!("（读日志失败: {e}）"));
        format!("{msg}\nrrd.log 尾部（{}）:\n{tail}", log_path.display())
    }
}

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run::rr3_sim_main(&args));
}

#[cfg(test)]
mod tests {
    use super::drill::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn args_parse_defaults_and_overrides() {
        let a = rr3_sim_args_parse(&v(&[])).unwrap();
        assert!(!a.keep);
        assert!(a.socket.is_none() && a.repo.is_none() && a.rrd.is_none());
        let a =
            rr3_sim_args_parse(&v(&["--keep", "--socket", "/tmp/s.sock", "--repo", "/r"])).unwrap();
        assert!(a.keep);
        assert_eq!(a.socket.unwrap(), std::path::PathBuf::from("/tmp/s.sock"));
        assert_eq!(a.repo.unwrap(), std::path::PathBuf::from("/r"));
        // --brain 开关
        let a = rr3_sim_args_parse(&v(&["--brain"])).unwrap();
        assert!(a.brain);
        // 开关在值的位置收到另一个开关要拒，不许静默吞掉
        let e = rr3_sim_args_parse(&v(&["--socket", "--keep"])).unwrap_err();
        assert!(e.contains("疑似开关"), "{e}");
        assert!(rr3_sim_args_parse(&v(&["--nonsense"])).is_err());
    }

    #[test]
    fn repo_root_found_upward_by_contract_marker() {
        let base = std::env::temp_dir().join(format!("rr3-sim-root-{}", std::process::id()));
        let deep = base.join("a/b/c");
        std::fs::create_dir_all(deep.join("contracts")).unwrap();
        std::fs::write(deep.join("contracts/topics.toml"), "schema_version = 1\n").unwrap();
        std::fs::create_dir_all(deep.join("a/b/c")).unwrap();
        let found = rr3_find_repo_root(&deep).expect("向上找仓根");
        assert_eq!(found, deep);
        // 找不到返回 None，不 panic
        let none = std::env::temp_dir().join(format!("rr3-sim-root-absent-{}", std::process::id()));
        assert_eq!(rr3_find_repo_root(&none), None);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn default_bins_follow_repo_layout() {
        let (rrd, peer, rrctl) = rr3_default_bins(std::path::Path::new("/repo"));
        assert_eq!(
            rrd,
            std::path::PathBuf::from("/repo/base/runtime/target/debug/rrd")
        );
        assert_eq!(
            peer,
            std::path::PathBuf::from("/repo/base/runtime/target/debug/sim_peer")
        );
        assert_eq!(
            rrctl,
            std::path::PathBuf::from("/repo/tools/rrctl/target/debug/rr3-ctl")
        );
        // 缺失检查点名 cargo build 命令
        let e = rr3_require_bins(
            std::path::Path::new("/nope/rrd"),
            std::path::Path::new("/nope/sim_peer"),
            std::path::Path::new("/nope/rr3-ctl"),
        )
        .unwrap_err();
        assert!(e.contains("cargo build"), "报错要给构建指引: {e}");
        // 扩展版同一真值：自定义 (路径, 指引) 对照查
        let e = rr3_require_bins_ext(&[
            (std::path::Path::new("/nope/x"), "cargo build -p x"),
            (std::path::Path::new("/nope/y"), "cargo build -p y"),
        ])
        .unwrap_err();
        assert!(
            e.contains("/nope/x") && e.contains("cargo build -p x"),
            "{e}"
        );
        assert!(rr3_require_bins_ext(&[]).is_ok());
    }

    #[test]
    fn manifest_text_has_absolute_entry_and_real_topic() {
        let t = rr3_manifest_text(
            "rr3-interact",
            std::path::Path::new("/abs/target/debug/sim_peer"),
            200,
            1000,
            "never",
            &[RR3_SIM_B_TOPIC],
        );
        assert!(t.contains("entry = \"/abs/target/debug/sim_peer\""), "{t}");
        assert!(
            t.contains(&format!("publishes = [\"{RR3_SIM_B_TOPIC}\"]")),
            "{t}"
        );
        assert!(t.contains("name = \"rr3-interact\""));
        assert!(t.contains("watchdog_timeout_ms = 1000"));
        // publishes 为空时是空数组而不是残缺语法
        let t = rr3_manifest_text(
            "rr3-sense",
            std::path::Path::new("/abs/x"),
            1,
            2,
            "always",
            &[],
        );
        assert!(t.contains("publishes = []"), "{t}");
    }

    #[test]
    fn status_table_parses_and_skips_non_rows() {
        let out = "NAME          TASK  PHASE\n\
                   rr3-sense     2     running\n\
                   denied_publish = 0\n";
        let rows = rr3_parse_status(out);
        assert_eq!(rows, vec![("rr3-sense".into(), 2, "running".into())]);
        // 空在册表：只有表头 + 计数行
        assert!(rr3_parse_status("NAME  TASK  PHASE\ndenied_publish = 0\n").is_empty());
        assert!(rr3_parse_status("").is_empty());
    }

    #[test]
    fn wait_modules_polls_until_all_wanted_phases_appear() {
        // 前两轮缺 A，第三轮齐了
        let mut calls = 0;
        rr3_wait_modules(
            || {
                calls += 1;
                if calls < 3 {
                    vec![]
                } else {
                    vec![("rr3-sense".to_string(), 2, "running".to_string())]
                }
            },
            &[("rr3-sense", "running")],
            std::time::Duration::from_secs(5),
        )
        .expect("第三轮应达成");
        assert_eq!(calls, 3);
        // 超时路径：点名缺谁并带上最后一次快照
        let e = rr3_wait_modules(
            || vec![("rr3-sense".to_string(), 2, "registered".to_string())],
            &[("rr3-sense", "running")],
            std::time::Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(
            e.contains("rr3-sense/running") && e.contains("registered"),
            "{e}"
        );
    }

    #[test]
    fn sim_module_identities_exist_in_the_real_contract() {
        // 钉住"演练用的名字/主题必须真实在契约里"——契约改名时这里红，不是演练跑到一半红。
        // 路径从 CARGO_MANIFEST_DIR 上去（tools/sim → 仓根）
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let contract = std::path::Path::new(manifest_dir).join("../../contracts/topics.toml");
        let text = std::fs::read_to_string(&contract)
            .unwrap_or_else(|e| panic!("读 {}: {e}", contract.display()));
        for needle in [RR3_SIM_MODULE_A, RR3_SIM_MODULE_B, RR3_SIM_B_TOPIC] {
            assert!(text.contains(needle), "契约缺 {needle}");
        }
    }
}
