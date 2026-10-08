//! `--profile` 装配演练（D9，0.0.2 方案）：装配单驱动 boot + 全模块真进程全链路巡检。
//!
//! 与既有两面演练（`--manifest` 手工清单）和 `--brain`（对话闭环）互补：本模式证
//! **广度**——仓内装配单（缺省 profiles/default.toml，16 格满配六模块）驱动 rrd
//! 装配链正门（`--profile` + `--contracts`，参数与 rrd.service 的 ExecStart 同形），
//! boot spawn 全部 face 模块（5 Rust + 1 Python brain），判据是「六模块全
//! running + `denied_publish = 0`」+ rr3-ctl 四命令巡检。
//!
//! 演练 modules-root 布局 = **装机布局**（`<root>/profiles/<机>.toml` 上两级是
//! modules-root、`modules/<name>/{module.toml,config.toml,<entry>}`）：
//! - manifest 用仓源原文、只改 entry 一个值（rr3-pack `rr3_manifest_install_rewrite`
//!   同语义同三道门）——装配对账的 provides 真值必须是仓源那一册，自造 manifest
//!   等于第二份真值；entry 绝对化是「entry 在仓相对、装机绝对」的第三面
//!   （装机 = 演练 modules-root，前两面：deb 打包、buildroot post-build）；
//! - config 同目录就位：Rust 模块用仓源 config（纯运行参数，sim 环境全部适用），
//!   brain 用 M4 先例的演练 config（mock 云 + 短判停，`rr3_brain_config_text`）——
//!   仓源 brain config 是真云端，演练不碰网络；
//! - python 模块的产物 = shim 脚本（`python3 -m rr3_brain`，M4 先例同形）。
//!
//! mock 注入走 rrd 进程 env（`RR3_BRAIN_CLOUD=mock` 由 brain 继承；其余模块无
//! 消费者）+ `RR3_HAL_BACKEND=sim`（模块 HAL 后端全落 sim）。本模块依赖 UDS 与
//! 真进程，仅 Unix 参与编译。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use rrd::manifest::rr3_manifest_facts_from_text;
use rrd::profile::rr3_profile_from_text;

use crate::brain::{
    make_executable, rr3_brain_config_text, rr3_brain_shim_script, wait_socket_exists,
    which_python3,
};
use crate::drill::{
    rr3_default_bins, rr3_find_repo_root, rr3_parse_status, rr3_require_bins_ext, rr3_section,
    rr3_wait_modules, Rr3SimArgs,
};

/// 装配演练就绪上界：冷启动 + 六进程（含 Python brain）注册也要够用
const RR3_ASSEMBLE_READY_DEADLINE: Duration = Duration::from_secs(60);

/// 仓源 manifest 的 entry 装机改写（rr3-pack `rr3_manifest_install_rewrite` 同语义同
/// 三道门，只改 entry 引号内的值，其余字节原样）：
/// 门 0 装机值可装机（绝对路径 + 无 `.` / `..` 段；POSIX 语法判定——
/// `Path::is_absolute` 在 Windows host 对 `/x` 返回 false，语义随 host 漂移会让同一
/// 份 manifest 两侧判定不同，registry 的 `rr3_manifest_entry_is_absolute` 同款理由）；
/// 门 1 entry 行恰一条；门 2 旧值与解析事实交叉核验（防夹具读错文件）；
/// 门 3 改写后重解析复核。
pub fn rr3_assemble_entry_rewrite(
    text: &str,
    installed: &Path,
    facts_entry: &str,
    module: &str,
) -> Result<String, String> {
    let target = installed.to_string_lossy().into_owned();
    if !target.starts_with('/') || target.split('/').any(|s| s == "." || s == "..") {
        return Err(format!(
            "模块 {module} 的装机 entry 不可装机（{target}：须 POSIX 绝对路径且无 . / .. 段）"
        ));
    }
    let mut n = 0;
    let mut out = String::with_capacity(text.len() + target.len());
    // split_inclusive 保住原文件的行尾形状（含"无尾换行"），除 entry 外逐字节零差异
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let is_entry = trimmed.starts_with("entry")
            && trimmed["entry".len()..].starts_with(|c: char| c.is_whitespace() || c == '=');
        if !is_entry {
            out.push_str(line);
            continue;
        }
        n += 1;
        if n > 1 {
            return Err(format!(
                "模块 {module} 的 manifest 有多条 entry 行（须恰一条）"
            ));
        }
        let (before, rest) = line
            .split_once('"')
            .ok_or_else(|| format!("模块 {module} 的 entry 行无引号值"))?;
        let (old, after) = rest
            .split_once('"')
            .ok_or_else(|| format!("模块 {module} 的 entry 行引号不闭合"))?;
        if old != facts_entry {
            return Err(format!(
                "模块 {module} 的 entry 旧值 {old:?} 与解析事实 {facts_entry:?} 不一致（夹具读错文件？）"
            ));
        }
        out.push_str(before);
        out.push('"');
        out.push_str(&target);
        out.push('"');
        out.push_str(after);
    }
    if n == 0 {
        return Err(format!("模块 {module} 的 manifest 无 entry 行"));
    }
    // 门 3：改写后重解析复核（entry 恰为新值，顺带保住"其余字段仍可解析"）
    let re = rr3_manifest_facts_from_text(&out)
        .map_err(|errs| format!("模块 {module} 改写后不可解析: {errs:?}"))?;
    if re.entry != target {
        return Err(format!(
            "模块 {module} 改写复核不一致（得 {:?}，want {target:?}）",
            re.entry
        ));
    }
    Ok(out)
}

/// 装配布局的一条产物：模块名、entry 绝对路径、是否 Rust 产物（需在盘）
pub type Rr3AssembleEntry = (String, PathBuf, bool);

/// 演练夹具：把仓源真值装进 workdir 的装机布局（modules-root），返回
/// (夹具装配单路径, 模块产物清单)。
/// 装配单原文拷贝零改；manifest 仅 entry 绝对化；config 同目录就位。
pub fn rr3_assemble_layout(
    repo: &Path,
    workdir: &Path,
    profile_src: &Path,
) -> Result<(PathBuf, Vec<Rr3AssembleEntry>), String> {
    let profile_text = std::fs::read_to_string(profile_src)
        .map_err(|e| format!("读不到装配单 {}: {e}", profile_src.display()))?;
    let profile = rr3_profile_from_text(&profile_text)
        .map_err(|errs| format!("装配单 {} 形状不合: {errs:?}", profile_src.display()))?;

    let profiles_dir = workdir.join("profiles");
    std::fs::create_dir_all(&profiles_dir).map_err(|e| format!("建 {profiles_dir:?} 失败: {e}"))?;
    let profile_name = profile_src
        .file_name()
        .ok_or_else(|| format!("装配单路径 {} 无文件名", profile_src.display()))?;
    let profile_dst = profiles_dir.join(profile_name);
    std::fs::write(&profile_dst, &profile_text)
        .map_err(|e| format!("写 {profile_dst:?} 失败: {e}"))?;

    let mut entries = Vec::new();
    for module in profile.enabled_slots.keys() {
        let src_dir = repo.join("modules").join(module);
        let src_manifest = src_dir.join("module.toml");
        let text = std::fs::read_to_string(&src_manifest).map_err(|e| {
            format!(
                "读不到仓源 manifest {}: {e}（装配演练要求 face 模块五件齐全）",
                src_manifest.display()
            )
        })?;
        let facts = rr3_manifest_facts_from_text(&text)
            .map_err(|errs| format!("仓源 manifest {src_manifest:?} 解析失败: {errs:?}"))?;
        // 产物推导：python = shim 脚本（M4 先例同形，包名 = 目录名连字符转下划线），
        // rust = 独立工程 target/debug 产物
        let (entry_abs, needs_bin) = if facts.lang == "python" {
            let pkg_dir = workdir.join("modules").join(module);
            std::fs::create_dir_all(&pkg_dir).map_err(|e| format!("建 {pkg_dir:?} 失败: {e}"))?;
            let shim = pkg_dir.join(&facts.entry);
            let py_module = module.replace('-', "_");
            std::fs::write(&shim, rr3_brain_shim_script(&src_dir, &py_module))
                .map_err(|e| format!("写 shim {shim:?} 失败: {e}"))?;
            make_executable(&shim)?;
            (shim, false)
        } else {
            (
                repo.join("modules")
                    .join(module)
                    .join("target/debug")
                    .join(&facts.entry),
                true,
            )
        };
        // manifest 装机改写（三道门）后落位
        let installed = rr3_assemble_entry_rewrite(&text, &entry_abs, &facts.entry, module)?;
        let dst_dir = workdir.join("modules").join(module);
        std::fs::create_dir_all(&dst_dir).map_err(|e| format!("建 {dst_dir:?} 失败: {e}"))?;
        let dst_manifest = dst_dir.join("module.toml");
        std::fs::write(&dst_manifest, installed)
            .map_err(|e| format!("写 {dst_manifest:?} 失败: {e}"))?;
        // config 同目录就位（模块 rr3_config_load 读 manifest 同目录 config.toml，
        // 缺件必重启环——演练 fail-fast 不装必死模块）：brain 用演练 mock config，
        // 其余用仓源 config（纯运行参数，sim 环境全部适用）
        let dst_config = dst_dir.join("config.toml");
        if facts.lang == "python" {
            std::fs::write(&dst_config, rr3_brain_config_text())
                .map_err(|e| format!("写 {dst_config:?} 失败: {e}"))?;
        } else {
            let src_config = src_dir.join("config.toml");
            std::fs::copy(&src_config, &dst_config).map_err(|e| {
                format!(
                    "仓源 config {src_config:?} 拷贝失败: {e}——模块开机即读 config，缺件必重启环"
                )
            })?;
        }
        entries.push((module.clone(), entry_abs, needs_bin));
    }
    Ok((profile_dst, entries))
}

pub fn rr3_assemble_drill(a: &Rr3SimArgs) -> Result<String, String> {
    let profile_src = a
        .profile
        .clone()
        .ok_or("内部错误：装配演练缺 --profile（main 路由保证）")?;

    // ---- 0. 定位：仓根与二进制 ----
    let repo = match &a.repo {
        Some(p) => p.clone(),
        None => std::env::current_dir()
            .ok()
            .and_then(|cwd| rr3_find_repo_root(&cwd))
            .ok_or("找不到仓根（缺 contracts/topics.toml）：请在仓内运行，或 --repo <路径>")?,
    };
    let (d_rrd, _peer, d_rrctl) = rr3_default_bins(&repo);
    let rrd = a.rrd.clone().unwrap_or(d_rrd);
    let rrctl = a.rrctl.clone().unwrap_or(d_rrctl);

    // ---- 1. 夹具：演练 modules-root（装机布局）----
    let workdir = match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!("rr3-sim-assemble-{}", std::process::id())),
    };
    std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
    let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
    let _ = std::fs::remove_file(&socket);
    let (profile_dst, entries) = rr3_assemble_layout(&repo, &workdir, &profile_src)?;

    // 产物存在性 fail-fast（点名 cargo build 指引，先例句式）
    let mut bins: Vec<(&Path, String)> = vec![
        (&rrd, "cargo build -p rrd（在 base/runtime/ 下）".into()),
        (
            &rrctl,
            "cargo build --manifest-path tools/rrctl/Cargo.toml".into(),
        ),
    ];
    for (module, entry_abs, needs_bin) in &entries {
        if *needs_bin {
            bins.push((
                entry_abs,
                format!("cargo build -p {module}（在 modules/{module}/ 下）"),
            ));
        }
    }
    let bin_refs: Vec<(&Path, &str)> = bins.iter().map(|(p, h)| (*p, h.as_str())).collect();
    rr3_require_bins_ext(&bin_refs)?;
    // python face 在册才需要 python3（shim 的 exec 依赖它）
    if entries.iter().any(|(_, _, needs_bin)| !*needs_bin) {
        which_python3()?;
    }

    let mut report = String::new();
    let outcome = (|| -> Result<(), String> {
        // ---- 2. 起 rrd：装配单驱动 boot（参数与 rrd.service 的 ExecStart 同形）----
        let log_path = workdir.join("rrd.log");
        let mut rrd_child = Command::new(&rrd)
            .arg("--socket")
            .arg(&socket)
            .arg("--contracts")
            .arg(repo.join("contracts"))
            .arg("--profile")
            .arg(&profile_dst)
            // mock 注入走 rrd env，boot spawn 的子模块全部继承：
            // HAL 后端全落 sim；burst 波形让 VAD 周期发边沿；brain 云端走 mock
            .env("RR3_HAL_BACKEND", "sim")
            .env("RR3_HAL_SIM_WAVE", "burst")
            .env("RR3_BRAIN_CLOUD", "mock")
            .stdout(Stdio::from(
                std::fs::File::create(&log_path)
                    .map_err(|e| format!("建 {log_path:?} 失败: {e}"))?,
            ))
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("起 rrd 失败: {e}"))?;
        let rrd_pid = rrd_child.id();
        println!(
            "rr3-sim --profile: rrd 已起 pid={rrd_pid}，socket={}，装配单={}",
            socket.display(),
            profile_dst.display()
        );

        let mut kill_rrd = true;
        let outcome = (|| -> Result<(), String> {
            // ---- 3. 等 rrd bind 套接字，再轮询全部 face 模块到 running ----
            wait_socket_exists(&socket, RR3_ASSEMBLE_READY_DEADLINE)?;
            let wanted: Vec<(&str, &str)> = entries
                .iter()
                .map(|(m, _, _)| (m.as_str(), "running"))
                .collect();
            rr3_wait_modules(
                || rr3_ctl_status_rows(&rrctl, &socket),
                &wanted,
                RR3_ASSEMBLE_READY_DEADLINE,
            )
            .map_err(|e| rr3_with_log_tail(&e, &log_path))?;
            report.push_str(rr3_section("装配单驱动 boot：全部 face 模块 running").as_str());
            for (m, entry_abs, _) in &entries {
                report.push_str(&format!("{m}  entry={}\n", entry_abs.display()));
            }

            // ---- 4. rr3-ctl 四命令巡检 ----
            report.push_str(rr3_section("rr3-ctl status 巡检").as_str());
            let status_out = rr3_ctl_out(&rrctl, &socket, "status")?;
            report.push_str(&status_out);
            // denied_publish = 0 是"事件真被接受"的线上证据（D9 验收判据）
            if !status_out.contains("denied_publish = 0") {
                return Err(format!("denied_publish ≠ 0：发布被 rrd 拒了\n{status_out}"));
            }
            let rows = rr3_ctl_status_rows(&rrctl, &socket);
            for (m, _, _) in &entries {
                match rows.iter().find(|(n, _, p)| n == m && p == "running") {
                    Some(_) => {}
                    None => return Err(format!("巡检快照里 {m} 不在 running（掉线？）：{rows:?}")),
                }
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
            Ok(())
        })();

        // ---- 收尾：杀 rrd（子模块喂狗失败自退）；--keep 保留现场 ----
        if a.keep {
            kill_rrd = false;
            println!(
                "rr3-sim --profile: --keep 保留现场：socket={} workdir={}",
                socket.display(),
                workdir.display()
            );
            println!("rr3-sim --profile: 手动收尾：kill {rrd_pid}；rm -rf {workdir:?}");
        }
        if kill_rrd {
            let _ = rrd_child.kill();
            let _ = rrd_child.wait();
            let _ = std::fs::remove_file(&socket);
        }
        outcome
    })();

    match outcome {
        Ok(()) => {
            report.push_str(rr3_section("演练结论").as_str());
            report.push_str(&format!(
                "装配演练 OK：{} 个 face 模块全 running + denied_publish = 0（装配单 {}）\n",
                entries.len(),
                profile_dst.display()
            ));
            Ok(report)
        }
        Err(e) => Err(format!("{e}\n（现场日志在 {workdir:?}）")),
    }
}

/// 轮询用的 status 快照（与 main.rs run / brain.rs 的同名助手同一真值）：
/// 连接失败（rrd 尚未就绪）当空表处理，由 deadline 收口
fn rr3_ctl_status_rows(rrctl: &Path, socket: &Path) -> Vec<(String, u64, String)> {
    Command::new(rrctl)
        .arg("--bus-path")
        .arg(socket)
        .arg("status")
        .output()
        .map(|o| rr3_parse_status(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// 跑一次 rr3-ctl 子命令，stdout 原样带回（巡检报告素材）；意外失败即 Err
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

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "[manifest]\nschema_version = 1\nname = \"rr3-x\"\nlang = \"rust\"\n\
entry = \"rr3-x\" # 行尾注释随原值保留\n\n[provides]\ncapabilities = [\"a.b\"]\n\n\
[resources]\nrestart_policy = \"always\"\n\n[health]\nheartbeat_ms = 1000\n\
watchdog_timeout_ms = 5000\n";

    #[test]
    fn rewrite_changes_only_the_entry_value() {
        let out = rr3_assemble_entry_rewrite(
            GOOD,
            Path::new("/usr/lib/rr3/modules/rr3-x/rr3-x"),
            "rr3-x",
            "rr3-x",
        )
        .unwrap();
        assert!(
            out.contains("entry = \"/usr/lib/rr3/modules/rr3-x/rr3-x\" # 行尾注释随原值保留\n"),
            "{out}"
        );
        // 除 entry 引号内的值外逐字节一致
        let strip = |s: &str| {
            s.lines()
                .map(|l| {
                    if l.trim_start().starts_with("entry") {
                        "<entry>"
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(strip(GOOD), strip(&out));
        // 尾换行形状原样保留
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn rewrite_gates_reject_the_bad_shapes() {
        // 多条 entry 行
        let t = format!("{GOOD}entry = \"other\"\n");
        assert!(
            rr3_assemble_entry_rewrite(&t, Path::new("/x"), "rr3-x", "rr3-x")
                .unwrap_err()
                .contains("恰一条"),
            "多 entry 行须拒"
        );
        // 无 entry 行
        let t = GOOD.replace("entry = \"rr3-x\" # 行尾注释随原值保留\n", "");
        assert!(
            rr3_assemble_entry_rewrite(&t, Path::new("/x"), "rr3-x", "rr3-x")
                .unwrap_err()
                .contains("无 entry 行"),
            "无 entry 行须拒"
        );
        // 旧值与 facts 交叉核验
        assert!(
            rr3_assemble_entry_rewrite(GOOD, Path::new("/x"), "wrong", "rr3-x")
                .unwrap_err()
                .contains("不一致")
        );
        // 不可装机：相对 / .. 段
        for bad in ["rel/entry", "/abs/../esc"] {
            assert!(
                rr3_assemble_entry_rewrite(GOOD, Path::new(bad), "rr3-x", "rr3-x")
                    .unwrap_err()
                    .contains("不可装机"),
                "{bad} 须拒"
            );
        }
    }

    #[test]
    fn layout_stages_install_shape_from_a_fixture_repo() {
        // 临时"仓"：profiles/default.toml + modules/rr3-pa（rust）/rr3-pb（python）
        let repo = std::env::temp_dir().join(format!("rr3-assemble-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        for (m, lang, entry) in [("rr3-pa", "rust", "rr3-pa"), ("rr3-pb", "python", "rr3-pb")] {
            let dir = repo.join("modules").join(m);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("module.toml"),
                format!(
                    "[manifest]\nschema_version = 1\nname = \"{m}\"\nlang = \"{lang}\"\n\
                     entry = \"{entry}\"\n\n[provides]\ncapabilities = []\n\n\
                     [resources]\nrestart_policy = \"always\"\n\n[health]\n\
                     heartbeat_ms = 1000\nwatchdog_timeout_ms = 5000\n"
                ),
            )
            .unwrap();
            std::fs::write(dir.join("config.toml"), "[x]\nv = 1\n").unwrap();
        }
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::write(
            repo.join("profiles/default.toml"),
            "schema_version = 1\n\n[bindings]\n\"a.b\" = \"rr3-pa\"\n\n\
             [enabled_slots]\nrr3-pa = [\"a.b\"]\nrr3-pb = []\n",
        )
        .unwrap();

        let work = std::env::temp_dir().join(format!("rr3-assemble-w-{}", std::process::id()));
        let (profile_dst, entries) =
            rr3_assemble_layout(&repo, &work, &repo.join("profiles/default.toml"))
                .expect("布局装配");
        // 装配单原文零改
        assert_eq!(
            std::fs::read_to_string(&profile_dst).unwrap(),
            std::fs::read_to_string(repo.join("profiles/default.toml")).unwrap()
        );
        // 两模块各就位：rust entry 指 target/debug 产物、python 指 workdir 内 shim
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            (
                "rr3-pa".into(),
                repo.join("modules/rr3-pa/target/debug/rr3-pa"),
                true
            )
        );
        let shim = work.join("modules/rr3-pb/rr3-pb");
        assert_eq!(entries[1], ("rr3-pb".into(), shim.clone(), false));
        // manifest 已绝对化且原文仅 entry 一处改写
        let staged = std::fs::read_to_string(work.join("modules/rr3-pa/module.toml")).unwrap();
        assert!(
            staged.contains(&format!(
                "entry = \"{}\"",
                repo.join("modules/rr3-pa/target/debug/rr3-pa").display()
            )),
            "{staged}"
        );
        assert!(staged.contains("name = \"rr3-pa\""));
        // shim 已写即存在（make_executable 已断言）；config 同目录就位
        let shim_text = std::fs::read_to_string(&shim).unwrap();
        assert!(shim_text.contains("python3 -m rr3_pb"), "{shim_text}");
        for m in ["rr3-pa", "rr3-pb"] {
            assert!(work.join("modules").join(m).join("config.toml").is_file());
        }
        std::fs::remove_dir_all(&repo).ok();
        std::fs::remove_dir_all(&work).ok();
    }

    /// 装配演练的仓内前提：default 机型 face 的每个模块，仓源 manifest 可解析、
    /// entry 可装机（仓相对）、config 在——契约/目录漂移让单测红，不是演练跑一半红
    #[test]
    fn repo_default_enabled_slots_are_assembly_ready() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let profile_text = std::fs::read_to_string(repo.join("profiles/default.toml")).unwrap();
        let profile = rr3_profile_from_text(&profile_text).expect("default 装配单可解析");
        assert_eq!(profile.enabled_slots.len(), 6, "default 机六模块满配");
        for module in profile.enabled_slots.keys() {
            let dir = repo.join("modules").join(module);
            let text = std::fs::read_to_string(dir.join("module.toml"))
                .unwrap_or_else(|e| panic!("读 {module} manifest: {e}"));
            let facts = rr3_manifest_facts_from_text(&text)
                .unwrap_or_else(|e| panic!("{module} manifest 解析: {e:?}"));
            assert_eq!(
                facts.name, *module,
                "manifest.name == 目录名（naming.md §3）"
            );
            assert!(
                !facts.entry.starts_with('/') && facts.entry.split('/').all(|s| s != ".."),
                "{module} entry 须仓相对且无 .. 段"
            );
            assert!(
                dir.join("config.toml").is_file(),
                "{module} 缺仓源 config.toml"
            );
        }
    }
}
