//! rr3-pack —— 发行版快路 deb 打包器（0.0.2 T5 / ADR-0013 裁定 5）。
//!
//! 输入两份真值，装配各自薄装（保底轨 post-build.sh 读同一注入格式，两轨不互抄装配）：
//! - `--payload` 注入产物（两轨同源约定，与 buildroot post-build.sh 同一口径）：
//!   `rrd`、`rr3-ctl`；rust 模块 `rr3-modules/<name>/{bin,manifest.toml}`；
//!   python 模块 `rr3-modules/<name>/{pkg/,manifest.toml}`（`pkg/` = 包树，
//!   如 `rr3_brain/`；entry 装机产物是打包器生成的 shim，见下）；
//! - `--repo` 仓根：`profiles/distro.toml`、`contracts/{topics,slots}.toml`、
//!   `modules/<name>/config.toml`（config 是源真值，不是构建产物，不进注入格式）。
//!
//! staging 装配是纯 Rust（macOS 可跑可单测），deb 布局：
//! `/usr/bin/{rrd,rr3-ctl}` + `/usr/lib/rockrobot3/{profiles,contracts,modules}` +
//! `/usr/lib/systemd/system/rrd.service` + `DEBIAN/{control,postinst}`。
//!
//! **entry 装机改写**是本打包器唯一一处内容改写（modules/README.md："entry 在仓相对、
//! 装机绝对"）：manifest 只把 entry 的值换成装机绝对路径，其余字节原样。为什么必须改：
//! 线上注册门禁（rrd conn.rs）要求 manifest.entry 绝对路径——模块开机读的就是装机里
//! 这份 manifest 原文上报，相对值必被 `AIRY_EINVAL` 拒；装配单磁盘装载那侧绝对/相对
//! 皆可，改写成绝对两头都过。错行不改、少行不改：除 entry 引号内一个值外零差异。
//!
//! **python 模块的 entry 装机**（`lang = "python"`，批F1）：产物不是 ELF 而是 shim 脚本，
//! 且 shim 里的 `PYTHONPATH`/`RR3_BRAIN_CONFIG` 必须是**装机绝对路径**（只有打包器知道
//! `RR3_INSTALL_ROOT`），故 shim 由打包器生成而非注入（`bin` 那个位置对 python 模块换成
//! `pkg/` 包树）。shim 形状与模拟演练侧 `rr3_brain_shim_script` 同构（演练侧布局 = 仓内
//! 布局，config 回退自洽，不注 config env）：
//! `#!/bin/sh\nRR3_BRAIN_CONFIG=<装机 config> PYTHONPATH=<装机包树> exec python3 -m <包名>\n`
//! （包名 = 目录名连字符转下划线；config env 走 brain `rr3_config_load` 的文档化优先口，
//! python 模块 config 契约当前即 brain 契约）。
//! 运行期语言不分支（rrd 只看 entry 可执行），`lang` 只在本打包器与演练侧做产物路径推导。

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use rrd::manifest::rr3_manifest_facts_from_text;

/// deb 包名/版本/装机根。版本跟方案批次走（0.0.6），升级批次时改这一处。
const RR3_DEB_PACKAGE: &str = "rockrobot3";
const RR3_DEB_VERSION: &str = "0.0.6";
const RR3_INSTALL_ROOT: &str = "/usr/lib/rockrobot3";

/// systemd 单元实体在 base/os/distro/units/rrd.service，include_str! 编译期嵌入：
/// 文件缺席直接编译失败（比运行期读 --repo 再报错更早），打包器对 repo 布局的依赖
/// 只剩 --repo 下的 profiles/contracts/config.toml 三处数据文件。
const RR3_SERVICE_UNIT: &str = include_str!("../../../base/os/distro/units/rrd.service");

/// 打包配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3PackConfig {
    payload: PathBuf,
    repo: PathBuf,
    out: PathBuf,
    /// deb Architecture 字段。缺省 arm64（出厂目标 aarch64）；CI 容器冒烟按
    /// `dpkg --print-architecture` 传 amd64——架构跟着真产物走，不硬编码。
    arch: String,
    stage_only: bool,
}

fn usage() -> String {
    "用法: rr3-pack --payload <dir> --repo <dir> --out <file.deb> [--arch <arch>] [--stage-only]\n说明: --payload 是两轨同源的注入产物目录（rrd、rr3-ctl；rust 模块 rr3-modules/<name>/{bin,manifest.toml}，python 模块 rr3-modules/<name>/{pkg/,manifest.toml}）；--repo 是仓根（读 profiles/distro.toml、contracts/{topics,slots}.toml、modules/<name>/config.toml）。--arch 缺省 arm64（出厂目标），容器冒烟传本机 dpkg 架构。--stage-only 只落 staging 树不调 dpkg-deb（macOS 本机无 dpkg，装配单测/演练用）。".to_string()
}

fn rr3_need(args: &[String], i: usize, flag: &str) -> Result<String, String> {
    match args.get(i + 1) {
        Some(v) if !v.is_empty() => Ok(v.clone()),
        _ => Err(format!("{flag} 缺参数\n{}", usage())),
    }
}

/// 解析参数。必给项缺席 / 未知选项一律报错带用法，不静默回落（fail-closed）。
pub fn rr3_pack_parse(args: &[String]) -> Result<Rr3PackConfig, String> {
    let mut cfg = Rr3PackConfig {
        payload: PathBuf::new(),
        repo: PathBuf::new(),
        out: PathBuf::new(),
        arch: "arm64".to_string(),
        stage_only: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--payload" => {
                cfg.payload = PathBuf::from(rr3_need(args, i, "--payload")?);
                i += 2;
            }
            "--repo" => {
                cfg.repo = PathBuf::from(rr3_need(args, i, "--repo")?);
                i += 2;
            }
            "--out" => {
                cfg.out = PathBuf::from(rr3_need(args, i, "--out")?);
                i += 2;
            }
            "--arch" => {
                cfg.arch = rr3_need(args, i, "--arch")?;
                i += 2;
            }
            "--stage-only" => {
                cfg.stage_only = true;
                i += 1;
            }
            other => return Err(format!("未知选项 {other}\n{}", usage())),
        }
    }
    if cfg.payload.as_os_str().is_empty()
        || cfg.repo.as_os_str().is_empty()
        || cfg.out.as_os_str().is_empty()
    {
        return Err(format!("--payload/--repo/--out 三者必给\n{}", usage()));
    }
    Ok(cfg)
}

/// entry 可装机性：必须相对、且每段都是普通名（拒绝对路径与 `..`/`.` 段——
/// staging 按字面 join，含逃逸段的 entry 会把二进制写进安装根之外）。
/// 在仓六份 manifest 全是生成器落盘的裸模块名，这条门禁只为把"装机约定"从
/// 口头变成打包期硬门：绝对/逃逸 entry 一律点名拒绝，不猜测意图。
fn rr3_entry_is_installable(entry: &str, module: &str) -> Result<(), String> {
    let p = Path::new(entry);
    if p.is_absolute() {
        return Err(format!(
            "模块 {module} 的 entry 是绝对路径 {entry}（在仓约定 entry 相对，装机绝对由打包器写入——见 modules/README.md）"
        ));
    }
    for c in p.components() {
        if !matches!(c, Component::Normal(_)) {
            return Err(format!(
                "模块 {module} 的 entry {entry} 含 . 或 .. 段（装机路径逃逸，打包拒绝）"
            ));
        }
    }
    Ok(())
}

/// python 模块的 entry 装机产物：shim 脚本（与模拟演练侧 `rr3_brain_shim_script` 同构，
/// 演练侧布局 = 仓内布局故不注 config——装机布局多一层 `pkg/`，config 位只有打包器知道）。
/// `pkg_dir` 是**装机绝对**包树目录（其下含 `<py_module>/` 包），
/// 包名 = 目录名连字符转下划线（`rr3-brain` → `rr3_brain`）；`config_path` 是装机绝对
/// config 位，经 `RR3_BRAIN_CONFIG` 注入（brain rr3_config_load 的文档化优先口，
/// 0.0.6 批F1：python 模块 config 契约当前即 brain 契约）。
fn rr3_python_shim_text(pkg_dir: &Path, py_module: &str, config_path: &Path) -> String {
    format!(
        "#!/bin/sh\nRR3_BRAIN_CONFIG={} PYTHONPATH={} exec python3 -m {py_module}\n",
        config_path.display(),
        pkg_dir.display()
    )
}

/// 递归拷贝目录树（python pkg 落位用）。只处理普通文件与目录；符号链接/特殊文件
/// 一律点名拒绝——注入产物是仓内包的纯副本，出现链接是装配契约事故，不静默跟随。
fn rr3_copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("建目录 {} 失败: {e}", dst.display()))?;
    let entries = fs::read_dir(src).map_err(|e| format!("读目录 {} 失败: {e}", src.display()))?;
    for e in entries {
        let e = e.map_err(|err| format!("读目录 {} 失败: {err}", src.display()))?;
        let from = e.path();
        let to = dst.join(e.file_name());
        let ft = e
            .file_type()
            .map_err(|err| format!("取 {} 类型失败: {err}", from.display()))?;
        if ft.is_dir() {
            rr3_copy_dir(&from, &to)?;
        } else if ft.is_file() {
            fs::copy(&from, &to).map_err(|err| format!("复制 {} 失败: {err}", from.display()))?;
        } else {
            return Err(format!(
                "python pkg 树含非普通文件（{}）——注入产物须是仓内包目录的纯副本，打包拒绝",
                from.display()
            ));
        }
    }
    Ok(())
}

/// manifest 装机改写：把 entry 引号内的值换成装机绝对路径，**其余字节零差异**
/// （缩进、注释、段序原样保留）。行扫描 + 引号内拼接，不整文件重序列化——
/// 重序列化会重排/丢注释，违反"只动一个值"的承诺。
/// 门禁三道：entry 行恰一条（0 或多条 = 歧义契约事故）；旧行值与 facts.entry
/// 交叉核验（防扫到别的表的同名键）；改写完重新解析 facts 复核新值已生效。
fn rr3_manifest_install_rewrite(
    text: &str,
    installed_entry: &str,
    facts_entry: &str,
    module: &str,
) -> Result<String, String> {
    let mut hits: Vec<(usize, Range)> = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let t = line.trim_start();
        let is_entry_line =
            t.starts_with("entry") && t["entry".len()..].trim_start().starts_with('=');
        if !is_entry_line {
            continue;
        }
        let (Some(a), Some(b)) = (line.find('"'), line.rfind('"')) else {
            return Err(format!(
                "模块 {module} 的 entry 行不是引号包裹的字符串（schema 违约）: {line}"
            ));
        };
        if b <= a {
            return Err(format!(
                "模块 {module} 的 entry 行引号内无值（schema 违约）: {line}"
            ));
        }
        hits.push((idx, Range { a, b }));
    }
    let (idx, rng) = match hits.as_slice() {
        [(idx, rng)] => (*idx, *rng),
        [] => {
            return Err(format!(
                "模块 {module} 的 manifest 无 entry 行（schema 违约，打包拒绝）"
            ))
        }
        n => {
            return Err(format!(
                "模块 {module} 的 manifest 有 {} 行 entry（歧义，打包拒绝）",
                n.len()
            ))
        }
    };
    if &text.lines().nth(idx).unwrap()[rng.a + 1..rng.b] != facts_entry {
        return Err(format!(
            "模块 {module} 的 entry 行值与 [manifest].entry 解析值不一致（疑似同名键，打包拒绝）"
        ));
    }
    let mut out_lines: Vec<String> = Vec::with_capacity(text.lines().count());
    for (i, line) in text.lines().enumerate() {
        if i == idx {
            // 引号内拼接：引号外（缩进/key/行尾注释）原样
            let head = &line[..rng.a + 1];
            let tail = &line[rng.b..];
            out_lines.push(format!("{head}{installed_entry}{tail}"));
        } else {
            out_lines.push(line.to_string());
        }
    }
    let out = if text.ends_with('\n') {
        format!("{}\n", out_lines.join("\n"))
    } else {
        out_lines.join("\n")
    };
    // 复核：改写产物必须仍能按 facts 解析且 entry 已是新值（改错了当场炸，不带病出包）
    let reparsed = rr3_manifest_facts_from_text(&out).map_err(|errs| {
        format!(
            "模块 {module} 的 manifest 改写后解析失败: {}",
            errs.join("; ")
        )
    })?;
    if reparsed.entry != installed_entry {
        return Err(format!(
            "模块 {module} 的 manifest 改写后 entry 仍为 {}（期望 {installed_entry}，改写逻辑事故）",
            reparsed.entry
        ));
    }
    Ok(out)
}

/// 引号内值在行内的字节区间 [a+1, b)
#[derive(Debug, Clone, Copy)]
struct Range {
    a: usize,
    b: usize,
}

/// 列出注入产物里的模块名（升序——staging 落位与出错点名都要确定性，read_dir 序不可靠）。
fn rr3_module_names(payload: &Path) -> Result<Vec<String>, String> {
    let dir = payload.join("rr3-modules");
    let mut names = Vec::new();
    let entries =
        fs::read_dir(&dir).map_err(|e| format!("读注入目录 {} 失败: {e}", dir.display()))?;
    for e in entries {
        let e = e.map_err(|err| format!("读注入目录 {} 失败: {err}", dir.display()))?;
        if e.path().is_dir() {
            names.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    if names.is_empty() {
        return Err(format!(
            "注入目录 {} 无模块（约定 rr3-modules/<name>/{{bin,manifest.toml}}，至少一个模块）",
            dir.display()
        ));
    }
    Ok(names)
}

/// staging 装配（纯 Rust）：把两份真值落成 deb 布局树，返回 staging 目录路径。
/// 路径 = `--out` 值加 `.stage` 后缀（与产物相邻，清掉即重来，不污染别处）。
pub fn rr3_stage(cfg: &Rr3PackConfig) -> Result<PathBuf, String> {
    // ---- 注入产物完整性（缺席逐件点名，不静默跳过——装机半成品比不出包更危险）----
    for bin in ["rrd", "rr3-ctl"] {
        let p = cfg.payload.join(bin);
        if !p.is_file() {
            return Err(format!(
                "注入产物 {bin} 缺席: {}（约定 --payload 下有 rrd、rr3-ctl、rr3-modules/<name>/{{bin,manifest.toml}}）",
                p.display()
            ));
        }
    }
    let names = rr3_module_names(&cfg.payload)?;
    // ---- 仓内数据真值（缺席点名）----
    let profile_src = cfg.repo.join("profiles").join("distro.toml");
    let topics_src = cfg.repo.join("contracts").join("topics.toml");
    let slots_src = cfg.repo.join("contracts").join("slots.toml");
    for p in [&profile_src, &topics_src, &slots_src] {
        if !p.is_file() {
            return Err(format!(
                "仓内文件缺席: {}（--repo 须指向仓根：profiles/distro.toml 与 contracts/{{topics,slots}}.toml 是装机正门的真值）",
                p.display()
            ));
        }
    }
    // ---- 落 staging ----
    let stage = cfg.out.with_file_name(format!(
        "{}.stage",
        cfg.out
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("out")
    ));
    let root = stage.join("usr/lib/rockrobot3");
    for d in [
        stage.join("DEBIAN"),
        stage.join("usr/bin"),
        stage.join("usr/lib/systemd/system"),
        root.join("profiles"),
        root.join("contracts"),
    ] {
        fs::create_dir_all(&d).map_err(|e| format!("建目录 {} 失败: {e}", d.display()))?;
    }
    fs::copy(cfg.payload.join("rrd"), stage.join("usr/bin/rrd"))
        .map_err(|e| format!("复制 rrd 失败: {e}"))?;
    fs::copy(cfg.payload.join("rr3-ctl"), stage.join("usr/bin/rr3-ctl"))
        .map_err(|e| format!("复制 rr3-ctl 失败: {e}"))?;
    fs::write(stage.join("DEBIAN/control"), rr3_control_text(&cfg.arch))
        .map_err(|e| format!("写 control 失败: {e}"))?;
    let postinst = stage.join("DEBIAN/postinst");
    fs::write(&postinst, rr3_postinst_text()).map_err(|e| format!("写 postinst 失败: {e}"))?;
    rr3_make_executable(&postinst);
    fs::write(
        stage.join("usr/lib/systemd/system/rrd.service"),
        RR3_SERVICE_UNIT,
    )
    .map_err(|e| format!("写 rrd.service 失败: {e}"))?;
    fs::copy(&profile_src, root.join("profiles/distro.toml"))
        .map_err(|e| format!("复制 distro.toml 失败: {e}"))?;
    fs::copy(&topics_src, root.join("contracts/topics.toml"))
        .map_err(|e| format!("复制 topics.toml 失败: {e}"))?;
    fs::copy(&slots_src, root.join("contracts/slots.toml"))
        .map_err(|e| format!("复制 slots.toml 失败: {e}"))?;
    // ---- 逐模块：manifest（entry 改写）+ 产物（rust bin / python pkg+shim）+ config（仓内源真值）----
    for name in &names {
        let mod_src = cfg.payload.join("rr3-modules").join(name);
        let manifest_src = mod_src.join("manifest.toml");
        if !manifest_src.is_file() {
            return Err(format!(
                "模块 {name} 注入产物不完整: {} 缺席（约定 rr3-modules/<name>/manifest.toml）",
                manifest_src.display()
            ));
        }
        let text = fs::read_to_string(&manifest_src)
            .map_err(|e| format!("读 {} 失败: {e}", manifest_src.display()))?;
        let facts = rr3_manifest_facts_from_text(&text).map_err(|errs| {
            format!(
                "模块 {name} 的 manifest 解析失败: {}（注入产物须是仓内 module.toml 的副本）",
                errs.join("; ")
            )
        })?;
        if &facts.name != name {
            return Err(format!(
                "模块目录名 {name} 与 manifest.name {} 不一致（注入产物自相矛盾，打包拒绝）",
                facts.name
            ));
        }
        rr3_entry_is_installable(&facts.entry, name)?;
        let installed_entry = format!("{RR3_INSTALL_ROOT}/modules/{name}/{}", facts.entry);
        let staged = root.join("modules").join(name);
        fs::create_dir_all(&staged)
            .map_err(|e| format!("建目录 {} 失败: {e}", staged.display()))?;
        fs::write(
            staged.join("module.toml"),
            rr3_manifest_install_rewrite(&text, &installed_entry, &facts.entry, name)?,
        )
        .map_err(|e| format!("写 {}/module.toml 失败: {e}", staged.display()))?;
        let entry_path = staged.join(&facts.entry);
        if let Some(parent) = entry_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("建目录 {} 失败: {e}", parent.display()))?;
        }
        // 产物按 lang 分型（批F1）：rust = 注入 bin 按 entry 落位；python = pkg 树落位 +
        // 打包器生成 shim（PYTHONPATH 须装机绝对，只有打包器知道 RR3_INSTALL_ROOT）。
        if facts.lang == "python" {
            let pkg_src = mod_src.join("pkg");
            if !pkg_src.is_dir() {
                return Err(format!(
                    "模块 {name}（lang=python）注入产物不完整: {} 缺席（python 模块约定 rr3-modules/<name>/{{pkg/,manifest.toml}}）",
                    pkg_src.display()
                ));
            }
            let pkg_dst = staged.join("pkg");
            rr3_copy_dir(&pkg_src, &pkg_dst).map_err(|e| format!("复制 {name} pkg 树失败: {e}"))?;
            let py_module = name.replace('-', "_");
            // shim 里的 PYTHONPATH 必须是**装机绝对**（不是 staging 临时路径——staging 在
            // 宿主出、板上 dpkg -i 才落 /usr/lib/rockrobot3，写 staging 路径装完即指向虚处）
            let pkg_installed = format!("{RR3_INSTALL_ROOT}/modules/{name}/pkg");
            let config_installed = format!("{RR3_INSTALL_ROOT}/modules/{name}/config.toml");
            fs::write(
                &entry_path,
                rr3_python_shim_text(
                    Path::new(&pkg_installed),
                    &py_module,
                    Path::new(&config_installed),
                ),
            )
            .map_err(|e| format!("写 {name} shim 失败: {e}"))?;
            rr3_make_executable(&entry_path);
        } else {
            let bin_src = mod_src.join("bin");
            if !bin_src.is_file() {
                return Err(format!(
                    "模块 {name} 注入产物不完整: {} 缺席（rust 模块约定 rr3-modules/<name>/{{bin,manifest.toml}}）",
                    bin_src.display()
                ));
            }
            fs::copy(&bin_src, &entry_path).map_err(|e| format!("复制 {name} bin 失败: {e}"))?;
        }
        let config_src = cfg.repo.join("modules").join(name).join("config.toml");
        if !config_src.is_file() {
            return Err(format!(
                "仓内文件缺席: {}（模块开机即读 manifest 同目录 config.toml，缺件必重启环——随包必带）",
                config_src.display()
            ));
        }
        fs::copy(&config_src, staged.join("config.toml"))
            .map_err(|e| format!("复制 {name} config.toml 失败: {e}"))?;
    }
    Ok(stage)
}

/// control 文本（字段序按 dpkg 惯例）。Maintainer 与 Git 提交身份同源。
fn rr3_control_text(arch: &str) -> String {
    format!(
        "Package: {RR3_DEB_PACKAGE}\nVersion: {RR3_DEB_VERSION}\nSection: net\nPriority: optional\nArchitecture: {arch}\nMaintainer: lei_huang <lei_huang71@spharx.cn>\nDescription: RockRobot3 运行时 payload（rrd + 模块 + systemd 单元，发行版快路轨）\n"
    )
}

/// postinst：systemd 在位才 enable——容器冒烟里 dpkg -i 时没有 systemd，不得因此失败。
/// enable 不带 --now：启动时机归 systemd 策略，装机脚本不擅自拉起守护。
fn rr3_postinst_text() -> &'static str {
    "#!/bin/sh\n# RockRobot3 deb 后置脚本（rr3-pack 生成）\n\
     # 运维计数持久化目录（RR3_DATA_DIR，0.0.6 批E' R3：unit 注入同名环境变量，\n\
     # 缺目录时 rrd 回写建 tmp 失败、boots 每次开机重置）\n\
     mkdir -p /var/lib/rockrobot3\nchmod 755 /var/lib/rockrobot3\nchown root:root /var/lib/rockrobot3\n\
     # ctl socket 属主组（0.0.6 批F5b R12，ADR-0010 权限模型增补）：rrd bind 后\n\
     # chmod 0o660 + chgrp 到本组，运维用户装机现场入组（deb 不动用户）\n\
     addgroup --system rockrobot3 2>/dev/null || true\n\
     if [ -d /run/systemd/system ]; then\n    systemctl enable rrd.service\nfi\n"
}

/// postinst 必须可执行否则 dpkg 不跑它。staging 在 Unix 上落，权限语义也只对 Unix 有意义。
#[cfg(unix)]
fn rr3_make_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
}
#[cfg(not(unix))]
fn rr3_make_executable(_p: &Path) {}

/// dpkg-deb 出包。缺席给可操作的指引（macOS 本机没有 dpkg 是常态，不是故障）。
fn rr3_build_deb(stage: &Path, out: &Path) -> Result<(), String> {
    if Command::new("dpkg-deb").arg("--version").output().is_err() {
        return Err(
            "dpkg-deb 不可用（本机多半没装 dpkg）：用 --stage-only 只出 staging 树，真装交给 CI distro-smoke 容器"
                .to_string(),
        );
    }
    let outp = Command::new("dpkg-deb")
        .args(["--root-owner-group", "--build"])
        .arg(stage)
        .arg(out)
        .output()
        .map_err(|e| format!("调 dpkg-deb 失败: {e}"))?;
    if !outp.status.success() {
        return Err(format!(
            "dpkg-deb 失败: {}",
            String::from_utf8_lossy(&outp.stderr)
        ));
    }
    Ok(())
}

fn rr3_pack(cfg: &Rr3PackConfig) -> Result<String, String> {
    let stage = rr3_stage(cfg)?;
    if cfg.stage_only {
        return Ok(format!(
            "staging 树就绪（--stage-only，未调 dpkg-deb）: {}",
            stage.display()
        ));
    }
    rr3_build_deb(&stage, &cfg.out)?;
    Ok(format!(
        "deb 就绪: {}（staging 树: {}）",
        cfg.out.display(),
        stage.display()
    ))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match rr3_pack_parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rr3-pack: {e}");
            std::process::exit(2);
        }
    };
    match rr3_pack(&cfg) {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("rr3-pack: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 夹具根唯一序号：并行测试各占一个目录（ZST 指针恒同、pid 同批相同，都当不得唯一键）
    fn fixture_seq() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        SEQ.fetch_add(1, Ordering::Relaxed)
    }

    /// 夹具：临时 payload + repo。
    fn fixture() -> (PathBuf, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("rr3-pack-{}-{}", std::process::id(), fixture_seq()));
        let _ = fs::remove_dir_all(&base);
        let payload = base.join("payload");
        let repo = base.join("repo");
        fs::create_dir_all(payload.join("rr3-modules/rr3-act")).unwrap();
        fs::create_dir_all(payload.join("rr3-modules/rr3-connect")).unwrap();
        fs::create_dir_all(repo.join("profiles")).unwrap();
        fs::create_dir_all(repo.join("contracts")).unwrap();
        fs::create_dir_all(repo.join("modules/rr3-act")).unwrap();
        fs::create_dir_all(repo.join("modules/rr3-connect")).unwrap();
        fs::write(payload.join("rrd"), "rrd-bin-bytes").unwrap();
        fs::write(payload.join("rr3-ctl"), "ctl-bin-bytes").unwrap();
        let manifest = "[manifest]\nschema_version = 1\nname = \"rr3-act\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"rr3-act\" # 在仓相对\n\n[provides]\ncapabilities = [\"act.servo\"]\n\n[depends]\nrequired = [\"bus.publish\"]\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 1000\nwatchdog_timeout_ms = 5000\n\n[resources]\nrestart_policy = \"always\"\nmem_limit_mb = 16\nstart_priority = 40\n";
        for m in ["rr3-act", "rr3-connect"] {
            fs::write(
                payload.join(format!("rr3-modules/{m}/manifest.toml")),
                manifest.replace("rr3-act", m),
            )
            .unwrap();
            fs::write(
                payload.join(format!("rr3-modules/{m}/bin")),
                format!("{m}-bin"),
            )
            .unwrap();
            fs::write(repo.join(format!("modules/{m}/config.toml")), "[x]\n").unwrap();
        }
        // python 模块（批F1）：pkg 树 + manifest（lang=python），无 bin——
        // entry 装机产物是打包器生成的 shim（PYTHONPATH 须装机绝对）
        fs::create_dir_all(payload.join("rr3-modules/rr3-brain/pkg/rr3_brain")).unwrap();
        fs::create_dir_all(repo.join("modules/rr3-brain")).unwrap();
        fs::write(
            payload.join("rr3-modules/rr3-brain/manifest.toml"),
            manifest
                .replace("rr3-act", "rr3-brain")
                .replace("lang = \"rust\"", "lang = \"python\""),
        )
        .unwrap();
        fs::write(
            payload.join("rr3-modules/rr3-brain/pkg/rr3_brain/__init__.py"),
            "__version__ = \"0.0.1\"\n",
        )
        .unwrap();
        fs::write(repo.join("modules/rr3-brain/config.toml"), "[rr3-brain]\n").unwrap();
        fs::write(repo.join("profiles/distro.toml"), "# distro 装配单\n").unwrap();
        fs::write(repo.join("contracts/topics.toml"), "# topics\n").unwrap();
        fs::write(repo.join("contracts/slots.toml"), "# slots\n").unwrap();
        (payload, repo)
    }

    fn cfg_of(payload: &Path, repo: &Path) -> (Rr3PackConfig, PathBuf) {
        let out = payload.parent().unwrap().join("rockrobot3.deb");
        (
            Rr3PackConfig {
                payload: payload.to_path_buf(),
                repo: repo.to_path_buf(),
                out: out.clone(),
                arch: "arm64".to_string(),
                stage_only: true,
            },
            out,
        )
    }

    #[test]
    fn stage_tree_lays_out_deb_layout_and_rewrites_entry_only() {
        let (payload, repo) = fixture();
        let (cfg, out) = cfg_of(&payload, &repo);
        let stage = rr3_stage(&cfg).unwrap();
        assert_eq!(stage, out.with_file_name("rockrobot3.deb.stage"));
        let root = stage.join("usr/lib/rockrobot3");
        // 二进制注入位
        assert_eq!(
            fs::read_to_string(stage.join("usr/bin/rrd")).unwrap(),
            "rrd-bin-bytes"
        );
        assert_eq!(
            fs::read_to_string(stage.join("usr/bin/rr3-ctl")).unwrap(),
            "ctl-bin-bytes"
        );
        // 数据真值位：零改写
        assert_eq!(
            fs::read_to_string(root.join("profiles/distro.toml")).unwrap(),
            "# distro 装配单\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("contracts/slots.toml")).unwrap(),
            "# slots\n"
        );
        assert_eq!(
            fs::read_to_string(stage.join("usr/lib/systemd/system/rrd.service")).unwrap(),
            RR3_SERVICE_UNIT
        );
        // control / postinst
        let control = fs::read_to_string(stage.join("DEBIAN/control")).unwrap();
        assert!(control.contains("Package: rockrobot3"));
        assert!(control.contains("Version: 0.0.6"));
        assert!(control.contains("Architecture: arm64"));
        let postinst = fs::read_to_string(stage.join("DEBIAN/postinst")).unwrap();
        assert!(postinst.starts_with("#!/bin/sh"));
        assert!(postinst.contains("systemctl enable rrd.service"));
        // 计数持久化目录（0.0.6 批E' R3）：postinst 建目录，与 unit 的 RR3_DATA_DIR 同路径
        assert!(postinst.contains("mkdir -p /var/lib/rockrobot3"));
        assert!(postinst.contains("chown root:root /var/lib/rockrobot3"));
        // ctl socket 属主组（0.0.6 批F5b R12）：幂等建组（组在位不报错），与
        // rrd daemon 的 chgrp 组名同字面
        assert!(postinst.contains("addgroup --system rockrobot3 2>/dev/null || true"));
        // 模块位：entry 改写为装机绝对路径，其余字节零差异；bin 按 entry 落位；config 随包
        let staged_manifest = fs::read_to_string(root.join("modules/rr3-act/module.toml")).unwrap();
        let facts = rr3_manifest_facts_from_text(&staged_manifest).unwrap();
        assert_eq!(facts.entry, "/usr/lib/rockrobot3/modules/rr3-act/rr3-act");
        let src = fs::read_to_string(payload.join("rr3-modules/rr3-act/manifest.toml")).unwrap();
        let expect = src.replace(
            "entry = \"rr3-act\"",
            "entry = \"/usr/lib/rockrobot3/modules/rr3-act/rr3-act\"",
        );
        assert_eq!(staged_manifest, expect, "除 entry 值外必须字节一致");
        assert_eq!(
            fs::read_to_string(root.join("modules/rr3-act/rr3-act")).unwrap(),
            "rr3-act-bin"
        );
        assert!(root.join("modules/rr3-act/config.toml").is_file());
        assert!(root.join("modules/rr3-connect/rr3-connect").is_file());
    }

    #[test]
    fn missing_payload_pieces_are_named() {
        let (payload, repo) = fixture();
        fs::remove_file(payload.join("rrd")).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("rrd 缺席"), "{e}");
        // 模块缺 bin
        let (payload, repo) = fixture();
        fs::remove_file(payload.join("rr3-modules/rr3-act/bin")).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("rr3-act") && e.contains("bin"), "{e}");
        // 无模块（三个注入目录全清，含 python 的 rr3-brain）
        let (payload, repo) = fixture();
        for m in ["rr3-act", "rr3-connect", "rr3-brain"] {
            fs::remove_dir_all(payload.join(format!("rr3-modules/{m}"))).unwrap();
        }
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("无模块"), "{e}");
        // repo 缺装配单
        let (payload, repo) = fixture();
        fs::remove_file(repo.join("profiles/distro.toml")).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("profiles/distro.toml"), "{e}");
        // 缺 config（模块开机即读，缺件必重启环）
        let (payload, repo) = fixture();
        fs::remove_file(repo.join("modules/rr3-act/config.toml")).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("config.toml"), "{e}");
    }

    #[test]
    fn uninstallable_entries_are_rejected_by_name() {
        let (payload, repo) = fixture();
        let manifest = payload.join("rr3-modules/rr3-act/manifest.toml");
        fs::write(
            &manifest,
            fs::read_to_string(&manifest)
                .unwrap()
                .replace("entry = \"rr3-act\"", "entry = \"/usr/bin/rr3-act\""),
        )
        .unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("绝对路径"), "{e}");
        // `..` 逃逸段
        let (payload, repo) = fixture();
        let manifest = payload.join("rr3-modules/rr3-act/manifest.toml");
        fs::write(
            &manifest,
            fs::read_to_string(&manifest)
                .unwrap()
                .replace("entry = \"rr3-act\"", "entry = \"../rr3-act\""),
        )
        .unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains(".."), "{e}");
    }

    #[test]
    fn module_dir_and_manifest_name_must_agree() {
        let (payload, repo) = fixture();
        fs::rename(
            payload.join("rr3-modules/rr3-act"),
            payload.join("rr3-modules/rr3-actx"),
        )
        .unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("rr3-actx") && e.contains("不一致"), "{e}");
    }

    /// python 模块（批F1）：entry 装机产物 = 打包器生成的 shim（PYTHONPATH 装机绝对），
    /// pkg 树落位、entry 改写为装机绝对路径、config 随包、shim 可执行。
    #[test]
    fn python_module_stages_generated_shim_and_pkg_tree() {
        let (payload, repo) = fixture();
        let (cfg, _) = cfg_of(&payload, &repo);
        let stage = rr3_stage(&cfg).unwrap();
        let dir = stage.join("usr/lib/rockrobot3/modules/rr3-brain");
        // pkg 树落位（逐文件，非空目录即过）
        assert_eq!(
            fs::read_to_string(dir.join("pkg/rr3_brain/__init__.py")).unwrap(),
            "__version__ = \"0.0.1\"\n"
        );
        // entry 改写为装机绝对路径，lang 事实保留
        let facts =
            rr3_manifest_facts_from_text(&fs::read_to_string(dir.join("module.toml")).unwrap())
                .unwrap();
        assert_eq!(facts.lang, "python");
        assert_eq!(
            facts.entry,
            "/usr/lib/rockrobot3/modules/rr3-brain/rr3-brain"
        );
        // shim 形状：RR3_BRAIN_CONFIG + PYTHONPATH 装机绝对 + python3 -m rr3_brain
        // （目录名连字符转下划线；config 注入是装机布局修正——brain 回退按"包上一级"
        // 寻址，pkg/ 嵌套下会指到 pkg/config.toml 虚位，见批F1 真板验证）
        assert_eq!(
            fs::read_to_string(dir.join("rr3-brain")).unwrap(),
            "#!/bin/sh\nRR3_BRAIN_CONFIG=/usr/lib/rockrobot3/modules/rr3-brain/config.toml \
             PYTHONPATH=/usr/lib/rockrobot3/modules/rr3-brain/pkg exec python3 -m rr3_brain\n"
        );
        assert!(dir.join("config.toml").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("rr3-brain"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "shim 必须可执行（rrd 直接 exec entry）");
        }
    }

    /// python 模块缺 pkg 树 → 点名拒绝（不是含糊的 "bin 缺席"）
    #[test]
    fn python_module_without_pkg_is_rejected_by_name() {
        let (payload, repo) = fixture();
        fs::remove_dir_all(payload.join("rr3-modules/rr3-brain/pkg")).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("rr3-brain") && e.contains("pkg"), "{e}");
    }

    #[test]
    fn parse_requires_flags_and_knows_stage_only() {
        assert!(rr3_pack_parse(&[]).is_err());
        let v = |s: &[&str]| -> Vec<String> { s.iter().map(|x| x.to_string()).collect() };
        let cfg = rr3_pack_parse(&v(&[
            "--payload",
            "p",
            "--repo",
            "r",
            "--out",
            "o.deb",
            "--stage-only",
        ]))
        .unwrap();
        assert!(cfg.stage_only);
        assert_eq!(cfg.arch, "arm64");
        assert!(rr3_pack_parse(&v(&["--payload", "p"])).is_err());
        assert!(rr3_pack_parse(&v(&["--payload", "p", "--repo", "r"])).is_err());
        assert!(rr3_pack_parse(&v(&["--bogus"])).is_err());
    }
}
