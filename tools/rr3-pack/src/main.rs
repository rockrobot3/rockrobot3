//! rr3-pack —— 发行版快路 deb 打包器（0.0.2 T5 / ADR-0013 裁定 5）。
//!
//! 输入两份真值，装配各自薄装（保底轨 post-build.sh 读同一注入格式，两轨不互抄装配）：
//! - `--payload` 注入产物（两轨同源约定，与 buildroot post-build.sh 同一口径）：
//!   `rrd`、`rr3-ctl`；rust 模块 `rr3-modules/<name>/{bin,manifest.toml}`；
//!   python 模块 `rr3-modules/<name>/{pkg/,manifest.toml}`（`pkg/` = 包树，
//!   如 `rr3_brain/`；entry 装机产物是打包器生成的 shim，见下）；
//! - `--repo` 仓根：`profiles/<name>.toml`（`--profile` 选机型，缺省 distro——H4/C2 批K
//!   参数化，出包链不再只认一份装配单）、`contracts/{topics,slots,roles,devices}.toml`、
//!   `contracts/platform/<chip>.toml`（按装配单 `[platform].chip` 选册，批N1 基-22）、
//!   `hardware/<name>/wiring.toml`、`hardware/<name>/axis/`（轴档案目录，在则整目录
//!   随装）、`modules/<name>/config.toml`（config 是源真值，不是构建产物，不进注入
//!   格式）。前六册 + 轴档案是 rrd 装载门的 boot 真值集，成员清单的真值只有 rrd
//!   `rr3_runtime_truth_set` 一份（增补轮六 基-25／实-12：装载面定义清单，打包面照单
//!   收口、不自带第二份——deb 缺任一必需册 rrd 即拒 boot，打包面缺任一册拒出包；
//!   打包先按装载同判据预验装配单，坏单不得出包）。
//!
//! staging 装配是纯 Rust（macOS 可跑可单测），deb 布局：
//! `/usr/bin/{rrd,rr3-ctl}` + `/usr/lib/rockrobot3/{profiles,contracts,modules,hardware}` +
//! `/usr/lib/systemd/system/rrd.service` + `DEBIAN/{control,postinst}`。
//!
//! **内容改写只有两处**（除此外字节零差异）：①entry 装机改写（modules/README.md：
//! "entry 在仓相对、装机绝对"）：manifest 只把 entry 的值换成装机绝对路径，其余字节
//! 原样。为什么必须改：线上注册门禁（rrd conn.rs）要求 manifest.entry 绝对路径——模块
//! 开机读的就是装机里这份 manifest 原文上报，相对值必被 `AIRY_EINVAL` 拒；装配单磁盘
//! 装载那侧绝对/相对皆可，改写成绝对两头都过。错行不改、少行不改：除 entry 引号内
//! 一个值外零差异。
//!
//! ②rrd.service 的 --profile 装机位改写（0.0.5 批K 深挖收口）：单元模板 ExecStart
//! 锚定缺省机型 distro.toml 的装机位，而非 distro 机型出包若照抄，systemd 拉起的 rrd
//! 找不到装配单必败——装机位 profiles/<name>.toml 只装 --profile 指的那一份，故
//! ExecStart 的 profile 路径随 --profile 同步改写。缺省 distro 恒等改写字节零差异；
//! 模板锚点缺席即报错（fail-closed），不静默照抄。
//!
//! **python 模块的 entry 装机**（`lang = "python"`，批F1）：产物不是 ELF 而是 shim 脚本，
//! 且 shim 里的 `PYTHONPATH`/`RR3_BRAIN_CONFIG` 必须是**装机绝对路径**（只有打包器知道
//! `RR3_INSTALL_ROOT`），故 shim 由打包器生成而非注入（`bin` 那个位置对 python 模块换成
//! `pkg/` 包树）。机制按 manifest.lang 数据位分派（批N32 件四起仓内六模块全 Rust，
//! python 路径是机制保留位，仓内暂无消费模块）：
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
/// 只剩 --repo 下的 profiles/contracts/hardware 三个数据目录与各模块 config.toml。
/// 路径以 CARGO_MANIFEST_DIR 构建期注入（基-32：不手写跨仓层数）。
const RR3_SERVICE_UNIT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../base/os/distro/units/rrd.service"
));

/// 打包配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3PackConfig {
    payload: PathBuf,
    repo: PathBuf,
    out: PathBuf,
    /// deb Architecture 字段。缺省 arm64（出厂目标 aarch64）；CI 容器冒烟按
    /// `dpkg --print-architecture` 传 amd64——架构跟着真产物走，不硬编码。
    arch: String,
    /// 装机装配单名（不带 .toml）。缺省 distro 不破坏既有调用方（CI distro-smoke、
    /// pack-arm64.sh）；换机型 = `--profile <name>`，staging 装机位 profiles/<name>.toml
    /// 零改写落位——出包链不再只认 distro 一份（H4/C2，0.0.5 批K）。
    /// rrd.service ExecStart 的装机位随名同步改写（缺省 distro 恒等，见模块头注②）。
    profile: String,
    stage_only: bool,
}

fn usage() -> String {
    "用法: rr3-pack --payload <dir> --repo <dir> --out <file.deb> [--profile <name>] [--arch <arch>] [--stage-only]\n说明: --payload 是两轨同源的注入产物目录（rrd、rr3-ctl；rust 模块 rr3-modules/<name>/{bin,manifest.toml}，python 模块 rr3-modules/<name>/{pkg/,manifest.toml}）；--repo 是仓根（读 profiles/<name>.toml、contracts/{topics,slots,roles,devices}.toml、hardware/<name>/wiring.toml、modules/<name>/config.toml）。--profile 选装机装配单（缺省 distro 不破坏既有调用方；H4/C2 批K：出包链不再只认一份装配单，换板只改装配单）。--arch 缺省 arm64（出厂目标），容器冒烟传本机 dpkg 架构。--stage-only 只落 staging 树不调 dpkg-deb（macOS 本机无 dpkg，装配单测/演练用）。".to_string()
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
        profile: "distro".to_string(),
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
            "--profile" => {
                cfg.profile = rr3_need(args, i, "--profile")?;
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

/// python 模块的 entry 装机产物：shim 脚本（机制保留位——批N32 件四起仓内六模块
/// 全 Rust，暂无消费模块；装机布局多一层 `pkg/`，config 位只有打包器知道）。
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
    // ---- 装载消费真值集（基-25／实-12：清单真值只有 rrd rr3_runtime_truth_set 一份，
    // 打包面照单收口、不自带第二份文件清单）----
    let profile_name = format!("{}.toml", cfg.profile);
    let profile_src = cfg.repo.join("profiles").join(&profile_name);
    // 打包先按装载同判据预验装配单（rr3_profile_from_text，与 boot 链同一函数）：坏单
    // 拒出包——出包时红比装机 boot 红早一个环节（块-15：装载必需件缺席而装机验证全绿
    // 才是最贵的洞）；预验同时给出 [platform].chip，平台事实表按它选册。
    let profile_text = fs::read_to_string(&profile_src)
        .map_err(|e| format!("读不到装配单 {}: {e}", profile_src.display()))?;
    let profile = rrd::profile::rr3_profile_from_text(&profile_text).map_err(|errs| {
        format!(
            "装配单 {} 预验失败（打包与装载同一判据，坏单不得出包）: {}",
            profile_src.display(),
            errs.join("; ")
        )
    })?;
    let truth = rrd::profile::rr3_runtime_truth_set(&profile, &cfg.profile);
    let missing: Vec<String> = truth
        .files
        .iter()
        .filter(|rel| !cfg.repo.join(rel).is_file())
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "装载消费真值集缺册: {}（--repo 须指向仓根 {}；清单真值 = rrd rr3_runtime_truth_set——缺任一必需册 rrd 装载即拒 boot，打包拒出包，基-25/实-12）",
            missing.join(", "),
            cfg.repo.display()
        ));
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
    // ExecStart 的 --profile 装机位随 --profile 改写（内容改写②，见模块头注）：模板
    // 锚定缺省机型 distro.toml，非 distro 机型照抄则 systemd 拉起的 rrd 找不到装配单。
    // 锚点缺席先报错（对表 base/os/distro/units/rrd.service），不静默照抄。
    const SERVICE_PROFILE_ANCHOR: &str = "/usr/lib/rockrobot3/profiles/distro.toml";
    if !RR3_SERVICE_UNIT.contains(SERVICE_PROFILE_ANCHOR) {
        return Err(format!(
            "rrd.service 模板缺 --profile 装机位锚点 {SERVICE_PROFILE_ANCHOR}——对表 base/os/distro/units/rrd.service 后再出包"
        ));
    }
    fs::write(
        stage.join("usr/lib/systemd/system/rrd.service"),
        RR3_SERVICE_UNIT.replace(
            SERVICE_PROFILE_ANCHOR,
            &format!("{RR3_INSTALL_ROOT}/profiles/{profile_name}"),
        ),
    )
    .map_err(|e| format!("写 rrd.service 失败: {e}"))?;
    // ---- 数据真值随包：按清单逐条落位（files 直落、dirs 在则整目录随装）----
    // 装机布局与仓根同名同布（清单相对路径两侧通用）；轴档案目录缺席合法跳过——
    // 与装载链 rr3_axis_files_load 的"无 axis 目录 = 无档案"同语义（基-25 清单 dirs 成员）。
    for rel in &truth.files {
        let dst = root.join(rel);
        let parent = dst
            .parent()
            .ok_or_else(|| format!("清单相对路径异常（无父目录）: {rel}"))?;
        fs::create_dir_all(parent).map_err(|e| format!("建目录 {} 失败: {e}", parent.display()))?;
        fs::copy(cfg.repo.join(rel), &dst).map_err(|e| format!("复制 {rel} 失败: {e}"))?;
    }
    for rel in &truth.dirs {
        let src = cfg.repo.join(rel);
        if src.is_dir() {
            rr3_copy_dir(&src, &root.join(rel))?;
        }
    }
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
        // G4/C1（ADR-0019 修订 1 ③ + 批J C1 裁定）：staging 扩整树——组合件编排声明
        // composite.toml 与原子件声明树 atoms/（五件套含 assemble.toml）随包进 deb，
        // 装机后板上 rrd 装配段（--profile 正门）才能扫到件级注入声明。两者可选
        // （无原子件/无编排的模块形态允许缺省），在位即纯副本（pkg 同款纪律）。
        let repo_mod = cfg.repo.join("modules").join(name);
        // 形态档配置（可选，2026-10-31）：`config-esp32.toml` 是板麦双通道档
        // （channels=2 + 反射环开启），与默认 config.toml 同源随包——部署侧用
        // RR3_SENSE_CONFIG 指向装机路径显式装载；默认档不读它，不存在即不随装
        // （与 composite/atoms 同一条"在位即纯副本"可选件纪律）。
        let config_esp32_src = repo_mod.join("config-esp32.toml");
        if config_esp32_src.is_file() {
            fs::copy(&config_esp32_src, staged.join("config-esp32.toml"))
                .map_err(|e| format!("复制 {name} config-esp32.toml 失败: {e}"))?;
        }
        let composite_src = repo_mod.join("composite.toml");
        if composite_src.is_file() {
            fs::copy(&composite_src, staged.join("composite.toml"))
                .map_err(|e| format!("复制 {name} composite.toml 失败: {e}"))?;
        }
        let atoms_src = repo_mod.join("atoms");
        if atoms_src.is_dir() {
            rr3_copy_dir(&atoms_src, &staged.join("atoms"))
                .map_err(|e| format!("复制 {name} atoms 树失败: {e}"))?;
        }
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

    /// 真仓装配单（基-25：打包预验走 rrd 解析器，夹具必须给合法装配单——空文本过不了
    /// 解析器。include_str! 相对源文件 src/，到仓根四级）。distro chip=rk3566、
    /// esp32-mini chip=esp32，平台事实表册位随之。
    const DISTRO_PROFILE: &str = include_str!("../../../profiles/distro.toml");
    const ESP32_MINI_PROFILE: &str = include_str!("../../../profiles/esp32-mini.toml");

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
        fs::write(repo.join("profiles/distro.toml"), DISTRO_PROFILE).unwrap();
        fs::write(repo.join("contracts/topics.toml"), "# topics\n").unwrap();
        fs::write(repo.join("contracts/slots.toml"), "# slots\n").unwrap();
        // boot 真值集（基-25 清单 files 成员）：roles/devices 契约 + 平台事实表
        //（按装配单 [platform].chip 选册）+ 机型接线档
        fs::write(repo.join("contracts/roles.toml"), "# roles\n").unwrap();
        fs::write(repo.join("contracts/devices.toml"), "# devices\n").unwrap();
        fs::create_dir_all(repo.join("contracts/platform")).unwrap();
        fs::write(
            repo.join("contracts/platform/rk3566.toml"),
            "# platform rk3566\n",
        )
        .unwrap();
        fs::create_dir_all(repo.join("hardware/distro")).unwrap();
        fs::write(repo.join("hardware/distro/wiring.toml"), "# wiring\n").unwrap();
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
                profile: "distro".to_string(),
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
            DISTRO_PROFILE
        );
        assert_eq!(
            fs::read_to_string(root.join("contracts/slots.toml")).unwrap(),
            "# slots\n"
        );
        // boot 真值集随包（基-25 清单 files 成员）：roles/devices 契约 + chip 选册的
        // 平台事实表 + 机型接线档；轴档案目录缺席（distro 无 axis）不造假数据
        assert_eq!(
            fs::read_to_string(root.join("contracts/roles.toml")).unwrap(),
            "# roles\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("contracts/devices.toml")).unwrap(),
            "# devices\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("contracts/platform/rk3566.toml")).unwrap(),
            "# platform rk3566\n",
            "平台事实表按装配单 [platform].chip 选册随包（基-25）"
        );
        assert_eq!(
            fs::read_to_string(root.join("hardware/distro/wiring.toml")).unwrap(),
            "# wiring\n",
            "接线档落装机位 hardware/<机型>/wiring.toml（装载推导同布局）"
        );
        assert!(!root.join("hardware/distro/axis").exists());
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
    fn stage_copies_whole_declaration_tree_for_g4() {
        let (payload, repo) = fixture();
        // rr3-act 给齐声明树：组合件编排 + 两原子件（含子文件，证递归纯副本）
        let repo_act = repo.join("modules/rr3-act");
        fs::write(
            repo_act.join("composite.toml"),
            "[assemble]\natoms = [\"led\", \"servo\"]\n",
        )
        .unwrap();
        fs::create_dir_all(repo_act.join("atoms/servo")).unwrap();
        fs::create_dir_all(repo_act.join("atoms/led")).unwrap();
        fs::write(
            repo_act.join("atoms/servo/assemble.toml"),
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n",
        )
        .unwrap();
        fs::write(repo_act.join("atoms/servo/servo.rs"), "// mech\n").unwrap();
        fs::write(
            repo_act.join("atoms/led/assemble.toml"),
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n",
        )
        .unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let stage = rr3_stage(&cfg).unwrap();
        let root = stage.join("usr/lib/rockrobot3");
        assert_eq!(
            fs::read_to_string(root.join("modules/rr3-act/composite.toml")).unwrap(),
            "[assemble]\natoms = [\"led\", \"servo\"]\n",
            "组合件编排声明随包"
        );
        assert_eq!(
            fs::read_to_string(root.join("modules/rr3-act/atoms/servo/assemble.toml")).unwrap(),
            "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("modules/rr3-act/atoms/servo/servo.rs")).unwrap(),
            "// mech\n",
            "atoms 子树递归纯副本"
        );
        assert!(root
            .join("modules/rr3-act/atoms/led/assemble.toml")
            .is_file());
        // 未给声明树的模块不造假数据
        assert!(!root.join("modules/rr3-connect/atoms").exists());
        assert!(!root.join("modules/rr3-connect/composite.toml").exists());
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
        // repo 缺机型接线档（boot 真值集第五册，Z-4：deb 缺它 rrd 装载即拒 boot）
        let (payload, repo) = fixture();
        fs::remove_file(repo.join("hardware/distro/wiring.toml")).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("hardware/distro/wiring.toml"), "{e}");
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

    /// H4/C2（0.0.5 批K）：--profile 参数化——出包链不再只认 distro 一份装配单，
    /// "换板只改装配单"在装机路径真走通（staging 按 --profile 名零改写落位；
    /// 缺席点名到具体机型文件，不静默回落 distro）。
    #[test]
    fn profile_flag_stages_named_profile_and_names_it_when_absent() {
        // 正例：--profile esp32-mini → 装机位 profiles/esp32-mini.toml，字节零改写
        //（真仓文本：rtos 档装配单过打包预验 = 过装载同判据，chip=esp32）
        let (payload, repo) = fixture();
        fs::write(repo.join("profiles/esp32-mini.toml"), ESP32_MINI_PROFILE).unwrap();
        // chip 选册的平台事实表随 --profile 机型（基-25 清单成员）
        fs::write(repo.join("contracts/platform/esp32.toml"), "# platform esp32\n").unwrap();
        // 机型接线档随 --profile 同名落位（基-25 清单成员）
        fs::create_dir_all(repo.join("hardware/esp32-mini")).unwrap();
        fs::write(
            repo.join("hardware/esp32-mini/wiring.toml"),
            "# esp32-mini 接线档\n",
        )
        .unwrap();
        // 轴档案目录在则整目录随装（基-25 清单 dirs 成员，rtos 机型）
        fs::create_dir_all(repo.join("hardware/esp32-mini/axis")).unwrap();
        fs::write(repo.join("hardware/esp32-mini/axis/pan.toml"), "# pan\n").unwrap();
        fs::write(repo.join("hardware/esp32-mini/axis/tilt.toml"), "# tilt\n").unwrap();
        let (mut cfg, _) = cfg_of(&payload, &repo);
        cfg.profile = "esp32-mini".to_string();
        let stage = rr3_stage(&cfg).unwrap();
        assert_eq!(
            fs::read_to_string(stage.join("usr/lib/rockrobot3/profiles/esp32-mini.toml")).unwrap(),
            ESP32_MINI_PROFILE,
            "指定机型装配单零改写落位"
        );
        assert_eq!(
            fs::read_to_string(
                stage.join("usr/lib/rockrobot3/contracts/platform/esp32.toml")
            )
            .unwrap(),
            "# platform esp32\n",
            "平台事实表按机型 chip 选册随包（基-25）"
        );
        assert_eq!(
            fs::read_to_string(stage.join("usr/lib/rockrobot3/hardware/esp32-mini/wiring.toml"))
                .unwrap(),
            "# esp32-mini 接线档\n",
            "指定机型接线档落同名装机位（装载推导同布局）"
        );
        assert_eq!(
            fs::read_to_string(stage.join("usr/lib/rockrobot3/hardware/esp32-mini/axis/pan.toml"))
                .unwrap(),
            "# pan\n",
            "轴档案目录在则整目录随装（基-25 清单 dirs 成员）"
        );
        assert!(stage
            .join("usr/lib/rockrobot3/hardware/esp32-mini/axis/tilt.toml")
            .is_file());
        // deb 只装 --profile 指的那一份装配单/接线档，不顺手带上 distro
        assert!(!stage
            .join("usr/lib/rockrobot3/profiles/distro.toml")
            .exists());
        assert!(!stage.join("usr/lib/rockrobot3/hardware/distro").exists());
        // rrd.service ExecStart 的装机位随 --profile 同步改写（深挖收口 P1）：
        // 非 distro 机型出包，单元不得再指向包里不存在的 distro.toml
        let staged_unit =
            fs::read_to_string(stage.join("usr/lib/systemd/system/rrd.service")).unwrap();
        assert!(
            staged_unit.contains("--profile /usr/lib/rockrobot3/profiles/esp32-mini.toml"),
            "ExecStart 装机位须随 --profile 改写"
        );
        assert!(
            !staged_unit.contains("profiles/distro.toml"),
            "非 distro 机型的单元不得残留 distro 装机位"
        );
        // 反例：指定机型缺席 → 点名到具体文件
        let (payload, repo) = fixture();
        let (mut cfg, _) = cfg_of(&payload, &repo);
        cfg.profile = "esp32-mini".to_string();
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("esp32-mini.toml"), "{e}");
    }

    /// 基-25 清单判等反例面：平台事实表缺席 → 打包拒出包并点名册位（清单真值 =
    /// rrd rr3_runtime_truth_set，打包面不自带第二份清单）。
    #[test]
    fn stage_refuses_when_truth_set_file_missing() {
        let (payload, repo) = fixture();
        fs::remove_file(repo.join("contracts/platform/rk3566.toml")).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("真值集缺册"), "{e}");
        assert!(e.contains("contracts/platform/rk3566.toml"), "{e}");
    }

    /// 基-25 打包预验判据（与装载同判据单点）：装配单坏形 → 拒出包，出包时红早于
    /// 装机 boot 红（块-15：装载必需件缺席而装机验证全绿才是最贵的洞）。
    #[test]
    fn stage_rejects_malformed_profile_by_loader_criteria() {
        let (payload, repo) = fixture();
        let bad = DISTRO_PROFILE.replace(
            &format!("schema_version = {}", 5),
            &format!("schema_version = {}", 99),
        );
        assert_ne!(bad, DISTRO_PROFILE, "替换须命中（schema_version 行在册）");
        fs::write(repo.join("profiles/distro.toml"), bad).unwrap();
        let (cfg, _) = cfg_of(&payload, &repo);
        let e = rr3_stage(&cfg).unwrap_err();
        assert!(e.contains("预验失败"), "{e}");
        assert!(e.contains("schema_version"), "{e}");
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
        assert_eq!(
            cfg.profile, "distro",
            "缺省机型 = distro（既有调用方零改动）"
        );
        let cfg = rr3_pack_parse(&v(&[
            "--payload",
            "p",
            "--repo",
            "r",
            "--out",
            "o.deb",
            "--profile",
            "esp32-mini",
        ]))
        .unwrap();
        assert_eq!(cfg.profile, "esp32-mini");
        assert!(rr3_pack_parse(&v(&["--payload", "p"])).is_err());
        assert!(rr3_pack_parse(&v(&["--payload", "p", "--repo", "r"])).is_err());
        assert!(rr3_pack_parse(&v(&["--bogus"])).is_err());
    }
}
