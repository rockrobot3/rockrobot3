//! `--profile` 装配演练（D9，0.0.2 方案）：装配单驱动 boot + 全模块真进程全链路巡检。
//!
//! 与既有两面演练（`--manifest` 手工清单）和 `--brain`（对话闭环）互补：本模式证
//! **广度**——仓内装配单（缺省 profiles/default.toml，16 格满配六模块）驱动 rrd
//! 装配链正门（`--profile` + `--contracts`，参数与 rrd.service 的 ExecStart 同形），
//! boot spawn 全部 face 模块（六 Rust 模块，批N32 件四起 brain 也是 Rust），判据是
//! 「六模块全 running + `denied_publish = 0`」+ rr3-ctl 四命令巡检。
//!
//! 演练 modules-root 布局 = **装机布局**（`<root>/profiles/<机>.toml` 上两级是
//! modules-root、`modules/<name>/{module.toml,config.toml,<entry>}`）：
//! - manifest 用仓源原文、只改 entry 一个值（rr3-pack `rr3_manifest_rewrite`
//!   同语义同三道门）——装配对账的 provides 真值必须是仓源那一册，自造 manifest
//!   等于第二份真值；entry 绝对化是「entry 在仓相对、装机绝对」的第三面
//!   （装机 = 演练 modules-root，前两面：deb 打包、buildroot post-build）；
//! - config 同目录就位：全部模块用仓源 config（纯运行参数）。brain 的云端由
//!   rrd 进程 env `RR3_BRAIN_CLOUD=mock` 压成 mock 档（确定性应答不发网络，
//!   仓源真云 base_url 不触达）。
//!
//! mock 注入走 rrd 进程 env（`RR3_BRAIN_CLOUD=mock` 由 brain 继承；其余模块无
//! 消费者）+ `RR3_HAL_BACKEND=sim`。HAL 面的 sim 落点有二：沙箱装配单副本经
//! [`rr3_asm_sim_text`] 变换（hal_backend → sim、可选真后端专面摘除）——
//! P0-3 起 supervisor 按装配单声明的 `RR3_HAL_BACKEND` 注入子进程、装配单是
//! 部署真值，rrd env 的 sim 只护 rrd 自己不护模块；rrd env 再兜一层
//! `--manifest` 调试路径。本模块依赖 UDS 与真进程，仅 Unix 参与编译。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use rrd::manifest::rr3_manifest_parse;
use rrd::profile::rr3_profile_parse;

use crate::brain::wait_socket_exists;
use crate::drill::{
    rr3_bin_dir, rr3_default_bins, rr3_parse_status, rr3_require_bins_ext, rr3_section,
    rr3_wait_modules, Rr3SimArgs,
};

/// 装配演练就绪上界：冷启动 + 六进程注册也要够用
const RR3_ASSEMBLE_READY_DEADLINE: Duration = Duration::from_secs(60);

/// 本次运行的唯一戳（纳秒）：缺省工作目录据此与上一轮隔离。
fn rr3_sim_stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 仓源 manifest 的 entry 装机改写（rr3-pack `rr3_manifest_rewrite` 同语义同
/// 三道门，只改 entry 引号内的值，其余字节原样）：
/// 门 0 装机值可装机（绝对路径 + 无 `.` / `..` 段；POSIX 语法判定——
/// `Path::is_absolute` 在 Windows host 对 `/x` 返回 false，语义随 host 漂移会让同一
/// 份 manifest 两侧判定不同，registry 的 `rr3_manifest_abs` 同款理由）；
/// 门 1 entry 行恰一条；门 2 旧值与解析事实交叉核验（防夹具读错文件）；
/// 门 3 改写后重解析复核。
pub fn rr3_entry_rewrite(
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
    let re = rr3_manifest_parse(&out)
        .map_err(|errs| format!("模块 {module} 改写后不可解析: {errs:?}"))?;
    if re.entry != target {
        return Err(format!(
            "模块 {module} 改写复核不一致（得 {:?}，want {target:?}）",
            re.entry
        ));
    }
    Ok(out)
}

/// 装配布局的一条产物：模块名、entry 绝对路径
pub type Rr3AssembleEntry = (String, PathBuf);

/// 演练夹具：把仓源真值装进 workdir 的装机布局（modules-root），返回
/// (夹具装配单路径, 模块产物清单)。
/// 装配单经 [`rr3_asm_sim_text`] sim 变换后落位（仓源原文零改）；接线档
/// 同批原文拷贝零改（`hardware/<形态>/wiring.toml`，校验⑦ 要求装机档必在，缺件
/// fail-fast）；manifest 仅 entry 绝对化；config 同目录就位。
pub fn rr3_assemble_layout(
    repo: &Path,
    workdir: &Path,
    profile_src: &Path,
) -> Result<(PathBuf, Vec<Rr3AssembleEntry>), String> {
    let profile_text = std::fs::read_to_string(profile_src)
        .map_err(|e| format!("读不到装配单 {}: {e}", profile_src.display()))?;
    let sim_text = rr3_asm_sim_text(&profile_text)?;
    let profile = rr3_profile_parse(&sim_text).map_err(|errs| {
        format!(
            "装配单 {} sim 变换后不可解析: {errs:?}",
            profile_src.display()
        )
    })?;
    let profile_dst = rr3_layout_profile(workdir, profile_src, &sim_text)?;
    let stem = rr3_layout_stem(profile_src)?;
    rr3_layout_wiring(repo, workdir, stem)?;

    let mut entries = Vec::with_capacity(profile.enabled_slots.len());
    for module in profile.enabled_slots.keys() {
        entries.push(rr3_layout_module(repo, workdir, module)?);
    }
    Ok((profile_dst, entries))
}

/// 装配单键名摘取（行首 `key =` 形；非键行 None）。
fn rr3_profile_key_of(line: &str) -> Option<&str> {
    let t = line.trim();
    let eq = t.find('=')?;
    let k = t[..eq].trim();
    let ok = !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    ok.then_some(k)
}

/// sim 演练装配单变换：仓源装配单是部署真值（P0-3 起 `[runtime].hal_backend`
/// 必填、真件档声明 linux/esp32 等真后端），而演练环境（开发机/CI runner）无
/// 硬件面可兑现——沙箱副本改写为 sim 面：`hal_backend` → `"sim"`，可选真后端
/// 专面覆盖（`audio_backend`/`servo_backend`/`display_backend`）整行摘除。
/// 与 [`rr3_asm_up_rrd`] 的 rrd env、`--chain` 的子进程注入同一语义；机制与
/// 策略分离：仓源装配单零改，变换只落 `profiles/<名>.toml` 沙箱副本。改写后
/// 由调用方重解析复核（hal_backend 必填缺行即红，其余字段保真——与 manifest
/// entry 绝对化同一条"改写后复核"纪律）。
fn rr3_asm_sim_text(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + 16);
    let mut in_runtime = false;
    let mut saw_hal = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_runtime = t.trim() == "[runtime]";
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_runtime {
            match rr3_profile_key_of(line) {
                Some("hal_backend") => {
                    saw_hal = true;
                    out.push_str("hal_backend = \"sim\"\n");
                    continue;
                }
                Some("audio_backend" | "servo_backend" | "display_backend") => continue,
                _ => {}
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !saw_hal {
        return Err("装配单 [runtime] 缺 hal_backend（P0-3 必填）——sim 变换无落点".into());
    }
    Ok(out)
}

/// 装配单 sim 变换落位到演练 modules-root（`profiles/<名>.toml`，文件名保仓源）。
fn rr3_layout_profile(workdir: &Path, profile_src: &Path, text: &str) -> Result<PathBuf, String> {
    let profiles_dir = workdir.join("profiles");
    std::fs::create_dir_all(&profiles_dir).map_err(|e| format!("建 {profiles_dir:?} 失败: {e}"))?;
    let profile_name = profile_src
        .file_name()
        .ok_or_else(|| format!("装配单路径 {} 无文件名", profile_src.display()))?;
    let profile_dst = profiles_dir.join(profile_name);
    std::fs::write(&profile_dst, text).map_err(|e| format!("写 {profile_dst:?} 失败: {e}"))?;
    Ok(profile_dst)
}

/// 装配单文件名主干（接线档目录名，`hardware/<stem>/wiring.toml`）。
fn rr3_layout_stem(profile_src: &Path) -> Result<&str, String> {
    profile_src
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| format!("装配单路径 {} 无文件名", profile_src.display()))
}

/// 接线档同批装机（原文零改）：装载期校验⑦要求 hardware/<形态>/wiring.toml
/// 必在，仓源缺件在这里红，不让演练跑一半才红。
fn rr3_layout_wiring(repo: &Path, workdir: &Path, stem: &str) -> Result<(), String> {
    let wiring_src = repo.join("hardware").join(stem).join("wiring.toml");
    let wiring_dst_dir = workdir.join("hardware").join(stem);
    std::fs::create_dir_all(&wiring_dst_dir)
        .map_err(|e| format!("建 {wiring_dst_dir:?} 失败: {e}"))?;
    std::fs::copy(&wiring_src, wiring_dst_dir.join("wiring.toml"))
        .map(|_| ())
        .map_err(|e| format!("仓源接线档 {wiring_src:?} 拷贝失败: {e}（校验⑦要求装机档必在）"))
}

/// 一个模块的五件套装机（manifest entry 绝对化 + config + atoms 申报面）。
/// 拆成"manifest 面 / 旁件面"两件（CC 攻坚）：前者是身份改写的主件，后者是
/// 开机即读的旁件（缺件都必红），两段判据互不依赖。
fn rr3_layout_module(
    repo: &Path,
    workdir: &Path,
    module: &str,
) -> Result<Rr3AssembleEntry, String> {
    let src_dir = repo.join("modules").join(module);
    let dst_dir = workdir.join("modules").join(module);
    std::fs::create_dir_all(&dst_dir).map_err(|e| format!("建 {dst_dir:?} 失败: {e}"))?;
    let entry = rr3_layout_manifest(repo, &src_dir, &dst_dir, module)?;
    rr3_layout_sidecars(&src_dir, &dst_dir)?;
    Ok((module.to_string(), entry))
}

/// manifest 面：读仓源 manifest → 解析 → 入口绝对化改写（三道门）→ 落位，
/// 回写入产物的绝对入口。
fn rr3_layout_manifest(
    repo: &Path,
    src_dir: &Path,
    dst_dir: &Path,
    module: &str,
) -> Result<PathBuf, String> {
    let src_manifest = src_dir.join("module.toml");
    let text = std::fs::read_to_string(&src_manifest).map_err(|e| {
        format!(
            "读不到仓源 manifest {}: {e}（装配演练要求 face 模块五件齐全）",
            src_manifest.display()
        )
    })?;
    let facts = rr3_manifest_parse(&text)
        .map_err(|errs| format!("仓源 manifest {src_manifest:?} 解析失败: {errs:?}"))?;
    // 产物 = 与 rr3-sim 同 target-dir 的模块产物（批N32 件四起六模块全
    // Rust——brain 翻面后 shim 产物路径退役；lang 合法性由 rrd manifest
    // 解析保证）。target-dir 已由仓根配置移出源码区，定位走单点。
    let entry_abs = rr3_bin_dir(repo).join(&facts.entry);
    let installed = rr3_entry_rewrite(&text, &entry_abs, &facts.entry, module)?;
    let dst_manifest = dst_dir.join("module.toml");
    std::fs::write(&dst_manifest, installed)
        .map_err(|e| format!("写 {dst_manifest:?} 失败: {e}"))?;
    Ok(entry_abs)
}

/// 旁件面：config 同目录就位（模块开机即读，缺件必重启环）+ atoms/ 申报面
/// 整树同批装机（校验⑨ 从 `modules/<m>/atoms/*/assemble.toml` 读 demands，
/// 漏拷会让 rrd 判"件未申报 demands 而角色 kind=power"红、拒绝 bind——演练
/// 布局与装机布局在此处必须逐件等价）。
fn rr3_layout_sidecars(src_dir: &Path, dst_dir: &Path) -> Result<(), String> {
    let src_config = src_dir.join("config.toml");
    std::fs::copy(&src_config, dst_dir.join("config.toml")).map_err(|e| {
        format!("仓源 config {src_config:?} 拷贝失败: {e}——模块开机即读 config，缺件必重启环")
    })?;
    let atoms_src = src_dir.join("atoms");
    if atoms_src.is_dir() {
        rr3_copy_tree(&atoms_src, &dst_dir.join("atoms"))?;
    }
    Ok(())
}

/// 目录递归拷贝（std 无内置；只服务演练夹具，符号链接按文件内容拷贝）。
fn rr3_copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("建 {dst:?} 失败: {e}"))?;
    let rd = std::fs::read_dir(src).map_err(|e| format!("读目录 {src:?} 失败: {e}"))?;
    for ent in rd {
        let ent = ent.map_err(|e| format!("遍历 {src:?} 失败: {e}"))?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            rr3_copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("拷贝 {from:?} 失败: {e}"))?;
        }
    }
    Ok(())
}

pub fn rr3_assemble_drill(a: &Rr3SimArgs) -> Result<String, String> {
    let profile_src = a
        .profile
        .clone()
        .ok_or("内部错误：装配演练缺 --profile（main 路由保证）")?;
    // ---- 0. 定位：仓根与二进制 ----
    let repo = crate::drill::rr3_repo_locate(a)?;
    let (d_rrd, _peer, d_rrctl) = rr3_default_bins(&repo);
    let rrd = a.rrd.clone().unwrap_or(d_rrd);
    let rrctl = a.rrctl.clone().unwrap_or(d_rrctl);

    // ---- 1. 夹具：演练 modules-root（装机布局）----
    // 缺省工作目录每次运行唯一（pid 会被容器快速复用，单 pid 命名会让上一轮
    // 遗留模块混进本轮装配）；显式 --socket 时目录归调用方，不删其父目录。
    let owned_dir = a.socket.is_none();
    let workdir = match &a.socket {
        Some(s) => s.parent().unwrap_or(Path::new(".")).to_path_buf(),
        None => std::env::temp_dir().join(format!(
            "rr3-sim-assemble-{}-{}",
            std::process::id(),
            rr3_sim_stamp()
        )),
    };
    if owned_dir {
        let _ = std::fs::remove_dir_all(&workdir);
    }
    std::fs::create_dir_all(&workdir).map_err(|e| format!("建 {workdir:?} 失败: {e}"))?;
    let socket = a.socket.clone().unwrap_or_else(|| workdir.join("bus.sock"));
    let _ = std::fs::remove_file(&socket);
    let (profile_dst, entries) = rr3_assemble_layout(&repo, &workdir, &profile_src)?;
    rr3_asm_bins(&rrd, &rrctl, &entries)?;

    let log_path = workdir.join("rrd.log");
    let ctx = Rr3AsmCtx {
        rrd: &rrd,
        rrctl: &rrctl,
        socket: &socket,
        profile_dst: &profile_dst,
        log_path: &log_path,
        entries: &entries,
        repo: &repo,
    };
    let mut report = String::new();
    let (mut rrd_child, rrd_pid) = rr3_asm_up_rrd(&ctx)?;
    let outcome = rr3_asm_wait(&ctx, &mut report);

    // ---- 收尾：杀 rrd（子模块喂狗失败自退）；--keep 保留现场 ----
    if a.keep {
        println!(
            "rr3-sim --profile: --keep 保留现场：socket={} workdir={}",
            socket.display(),
            workdir.display()
        );
        println!("rr3-sim --profile: 手动收尾：kill {rrd_pid}；rm -rf {workdir:?}");
    } else {
        let _ = rrd_child.kill();
        let _ = rrd_child.wait();
        let _ = std::fs::remove_file(&socket);
    }

    match outcome {
        Ok(()) => {
            // 成功即收现场（失败保留供排障，错误信息已点名 workdir）。
            if owned_dir {
                let _ = std::fs::remove_dir_all(&workdir);
            }
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

/// 装配演练上下文（一次演练内的不变量；只读借用，便于按阶段拆函数）。
struct Rr3AsmCtx<'a> {
    rrd: &'a Path,
    rrctl: &'a Path,
    socket: &'a Path,
    profile_dst: &'a Path,
    log_path: &'a Path,
    entries: &'a [Rr3AssembleEntry],
    repo: &'a Path,
}

/// 产物存在性 fail-fast（点名 cargo build 指引，先例句式）。
fn rr3_asm_bins(rrd: &Path, rrctl: &Path, entries: &[Rr3AssembleEntry]) -> Result<(), String> {
    let mut bins: Vec<(&Path, String)> = vec![
        (rrd, "cargo build -p rrd（在 base/runtime/ 下）".into()),
        (
            rrctl,
            "cargo build --manifest-path tools/rrctl/Cargo.toml".into(),
        ),
    ];
    for (module, entry_abs) in entries {
        bins.push((
            entry_abs.as_path(),
            format!("cargo build -p {module}（在 modules/{module}/ 下）"),
        ));
    }
    let bin_refs: Vec<(&Path, &str)> = bins.iter().map(|(p, h)| (*p, h.as_str())).collect();
    rr3_require_bins_ext(&bin_refs)
}

/// 起 rrd：装配单驱动 boot（参数与 rrd.service 的 ExecStart 同形）。
fn rr3_asm_up_rrd(ctx: &Rr3AsmCtx) -> Result<(std::process::Child, u32), String> {
    let child = Command::new(ctx.rrd)
        .arg("--socket")
        .arg(ctx.socket)
        .arg("--contracts")
        .arg(ctx.repo.join("contracts"))
        .arg("--profile")
        .arg(ctx.profile_dst)
        // mock 注入走 rrd env，boot spawn 的子模块全部继承：
        // HAL 后端全落 sim；sim 探针（MC-4：非 test 面不再默认 sim——开发机/CI
        // 无 linux sysfs 硬件面，不显式选 sim 会被 fail-closed 判缺格、模块拒绝起）；
        // burst 波形让 VAD 周期发边沿；brain 云端走 mock
        .env("RR3_HAL_BACKEND", "sim")
        .env("RR3_PROBE", "sim")
        .env("RR3_HAL_SIM_WAVE", "burst")
        .env("RR3_BRAIN_CLOUD", "mock")
        .stdout(Stdio::from(
            std::fs::File::create(ctx.log_path)
                .map_err(|e| format!("建 {:?} 失败: {e}", ctx.log_path))?,
        ))
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("起 rrd 失败: {e}"))?;
    let pid = child.id();
    println!(
        "rr3-sim --profile: rrd 已起 pid={pid}，socket={}，装配单={}",
        ctx.socket.display(),
        ctx.profile_dst.display()
    );
    Ok((child, pid))
}

/// 等全模块 running → rr3-ctl 四命令巡检（status/health/logs/ota）。
fn rr3_asm_wait(ctx: &Rr3AsmCtx, report: &mut String) -> Result<(), String> {
    wait_socket_exists(ctx.socket, RR3_ASSEMBLE_READY_DEADLINE)?;
    let wanted: Vec<(&str, &str)> = ctx
        .entries
        .iter()
        .map(|(m, _)| (m.as_str(), "running"))
        .collect();
    rr3_wait_modules(
        || rr3_ctl_status_rows(ctx.rrctl, ctx.socket),
        &wanted,
        RR3_ASSEMBLE_READY_DEADLINE,
    )
    .map_err(|e| rr3_with_log_tail(&e, ctx.log_path))?;
    report.push_str(rr3_section("装配单驱动 boot：全部 face 模块 running").as_str());
    for (m, entry_abs) in ctx.entries {
        report.push_str(&format!("{m}  entry={}\n", entry_abs.display()));
    }

    // ---- 4. rr3-ctl 四命令巡检 ----
    report.push_str(rr3_section("rr3-ctl status 巡检").as_str());
    let status_out = rr3_ctl_out(ctx.rrctl, ctx.socket, "status")?;
    report.push_str(&status_out);
    // denied_publish = 0 是"事件真被接受"的线上证据（D9 验收判据）
    if !status_out.contains("denied_publish = 0") {
        return Err(format!("denied_publish ≠ 0：发布被 rrd 拒了\n{status_out}"));
    }
    let rows = rr3_ctl_status_rows(ctx.rrctl, ctx.socket);
    for (m, _) in ctx.entries {
        if !rows.iter().any(|(n, _, p)| n == m && p == "running") {
            return Err(format!("巡检快照里 {m} 不在 running（掉线？）：{rows:?}"));
        }
    }
    report.push_str(rr3_section("rr3-ctl health").as_str());
    report.push_str(&rr3_ctl_out(ctx.rrctl, ctx.socket, "health")?);
    report.push_str(rr3_section("rr3-ctl logs（rrd 日志环尾部）").as_str());
    report.push_str(&rr3_ctl_out(ctx.rrctl, ctx.socket, "logs")?);
    report.push_str(rr3_section("rr3-ctl ota（预期恒拒，随 M6）").as_str());
    match rr3_ctl_maybe_fail(ctx.rrctl, ctx.socket, "ota") {
        Ok(out) => report.push_str(&format!("意外成功：\n{out}")),
        Err((code, out)) => report.push_str(&format!("按预期被拒（exit {code}）:\n{out}")),
    }
    Ok(())
}

/// 轮询用的 status 快照（main.rs run / chain / brain 各演练共用同一真值）：
/// 连接失败（rrd 尚未就绪）当空表处理，由 deadline 收口
pub(crate) fn rr3_ctl_status_rows(rrctl: &Path, socket: &Path) -> Vec<(String, u64, String)> {
    Command::new(rrctl)
        .arg("--bus-path")
        .arg(socket)
        .arg("status")
        .output()
        .map(|o| rr3_parse_status(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// 跑一次 rr3-ctl 子命令，stdout 原样带回（巡检报告素材）；意外失败即 Err
pub(crate) fn rr3_ctl_out(rrctl: &Path, socket: &Path, cmd: &str) -> Result<String, String> {
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
pub(crate) fn rr3_ctl_maybe_fail(
    rrctl: &Path,
    socket: &Path,
    cmd: &str,
) -> Result<String, (i32, String)> {
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
pub(crate) fn rr3_with_log_tail(msg: &str, log_path: &Path) -> String {
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
        let out = rr3_entry_rewrite(
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
            rr3_entry_rewrite(&t, Path::new("/x"), "rr3-x", "rr3-x")
                .unwrap_err()
                .contains("恰一条"),
            "多 entry 行须拒"
        );
        // 无 entry 行
        let t = GOOD.replace("entry = \"rr3-x\" # 行尾注释随原值保留\n", "");
        assert!(
            rr3_entry_rewrite(&t, Path::new("/x"), "rr3-x", "rr3-x")
                .unwrap_err()
                .contains("无 entry 行"),
            "无 entry 行须拒"
        );
        // 旧值与 facts 交叉核验
        assert!(rr3_entry_rewrite(GOOD, Path::new("/x"), "wrong", "rr3-x")
            .unwrap_err()
            .contains("不一致"));
        // 不可装机：相对 / .. 段
        for bad in ["rel/entry", "/abs/../esc"] {
            assert!(
                rr3_entry_rewrite(GOOD, Path::new(bad), "rr3-x", "rr3-x")
                    .unwrap_err()
                    .contains("不可装机"),
                "{bad} 须拒"
            );
        }
    }

    #[test]
    fn sim_profile_transform_rewrites_hal_and_drops_face_overrides() {
        let t = "[runtime]\nlog_ring_cap = 512\nhal_backend = \"linux\"\n\
                 audio_backend = \"esp32\"\nservo_backend = \"esp32\"\n\n\
                 [platform]\ntier = \"host\"\nnote = \"hal_backend 字样在段外不受影响\"\n";
        let out = rr3_asm_sim_text(t).unwrap();
        assert!(out.contains("hal_backend = \"sim\"\n"), "{out}");
        assert!(!out.contains("audio_backend"), "{out}");
        assert!(!out.contains("servo_backend"), "{out}");
        assert!(out.contains("段外不受影响"), "{out}");
        // 缺 hal_backend 即红（P0-3 必填无落点）
        let no_hal = "[runtime]\nlog_ring_cap = 512\n\n[platform]\ntier = \"host\"\n";
        assert!(rr3_asm_sim_text(no_hal)
            .unwrap_err()
            .contains("缺 hal_backend"));
    }

    #[test]
    fn layout_stages_install_shape_from_a_fixture_repo() {
        // 临时"仓"：profiles/default.toml + modules/rr3-pa（rust）
        let repo = std::env::temp_dir().join(format!("rr3-assemble-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        let (m, lang, entry) = ("rr3-pa", "rust", "rr3-pa");
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
        // atoms/ 申报面：校验⑨ 从装机树读 demands，夹具必须与装机布局同形
        std::fs::create_dir_all(dir.join("atoms/servo")).unwrap();
        std::fs::write(dir.join("atoms/servo/assemble.toml"), "[assemble]\n").unwrap();
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::write(
            repo.join("profiles/default.toml"),
            // schema_version=5 形状（批N5 [runtime] 进场，F-30 环容量落数据）：host 档
            // 合法不适用显式写；本夹具不声明 [pins]，"a.b" 是夹具自造格位，本函数
            // 只走形状解析（rr3_profile_parse），不查册。
            "schema_version = 5\n\n[runtime]\nlog_ring_cap = 512\nhal_backend = \"sim\"\n\n[platform]\ntier = \"host\"\nchip = \"rk3566\"\n\n\
             [supplies]\nwatchdog_present = false\nwatchdog_timeout_ms = 15000\n\
             watchdog_feeder = \"none\"\ntick_period_ms = 200\ntick_jitter_ms = 100\n\
             safety_core_interval_ms = 1000\n\n\
             [[supplies.domain]]\nname = \"rail5v\"\nnominal_mv = 5000\nmild_mv = 4600\n\
             severe_mv = 4400\ncurrent_cap_ma = 2000\nsense = \"none\"\n\
             current_sense = \"none\"\nswitch = \"none\"\nswitchable = false\n\
             armed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n\
             [supplies.budget]\nnot_applicable = true\n\n\
             [bindings]\n\"a.b\" = \"rr3-pa\"\n\n\
             [enabled_slots]\nrr3-pa = [\"a.b\"]\n\n\
             [slots]\npresent = [\"a.b\"]\n",
        )
        .unwrap();
        // 接线档仓源（校验⑦ 装机档必在）：vcc/gnd-only 单线组，对夹具不声明
        // [pins] 免疫
        std::fs::create_dir_all(repo.join("hardware/default")).unwrap();
        std::fs::write(
            repo.join("hardware/default/wiring.toml"),
            "[[wire]]\nconnector = \"J-T\"\nhdr_pin = [1, 2]\nsignal = [\"vcc\", \"gnd\"]\n\
             net = \"rail5v\"\nfeeds = []\nmate = \"none\"\nkeyed = false\nrated_a = 1.0\n\
             gnd_common = \"star-1\"\n",
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
        // 接线档同批原文零改
        assert_eq!(
            std::fs::read_to_string(work.join("hardware/default/wiring.toml")).unwrap(),
            std::fs::read_to_string(repo.join("hardware/default/wiring.toml")).unwrap()
        );
        // 模块就位：entry 指与 rr3-sim 同 target-dir 的产物
        assert_eq!(entries.len(), 1);
        let want_entry = rr3_bin_dir(&repo).join("rr3-pa");
        assert_eq!(entries[0], ("rr3-pa".into(), want_entry.clone()));
        // manifest 已绝对化且原文仅 entry 一处改写
        let staged = std::fs::read_to_string(work.join("modules/rr3-pa/module.toml")).unwrap();
        assert!(
            staged.contains(&format!("entry = \"{}\"", want_entry.display())),
            "{staged}"
        );
        assert!(staged.contains("name = \"rr3-pa\""));
        // config 同目录就位
        assert!(work.join("modules/rr3-pa/config.toml").is_file());
        // atoms/ 树同批装机（漏拷会让 rrd 校验⑨ 误判"件未申报 demands"）
        assert_eq!(
            std::fs::read_to_string(work.join("modules/rr3-pa/atoms/servo/assemble.toml")).unwrap(),
            "[assemble]\n"
        );
        std::fs::remove_dir_all(&repo).ok();
        std::fs::remove_dir_all(&work).ok();
    }

    /// 装配演练的仓内前提：default 机型 face 的每个模块，仓源 manifest 可解析、
    /// entry 可装机（仓相对）、config 在；接线档在（校验⑦ 装机档必在）——
    /// 契约/目录漂移让单测红，不是演练跑一半红
    #[test]
    fn repo_default_enabled_slots_are_assembly_ready() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let profile_text = std::fs::read_to_string(repo.join("profiles/default.toml")).unwrap();
        let profile = rr3_profile_parse(&profile_text).expect("default 装配单可解析");
        assert_eq!(profile.enabled_slots.len(), 6, "default 机六模块满配");
        assert!(
            repo.join("hardware/default/wiring.toml").is_file(),
            "default 机缺仓源接线档（校验⑦ 要求 hardware/<形态>/wiring.toml 必在）"
        );
        for module in profile.enabled_slots.keys() {
            let dir = repo.join("modules").join(module);
            let text = std::fs::read_to_string(dir.join("module.toml"))
                .unwrap_or_else(|e| panic!("读 {module} manifest: {e}"));
            let facts = rr3_manifest_parse(&text)
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
