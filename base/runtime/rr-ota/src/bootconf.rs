//! 引导配置（extlinux.conf）改写面（0.0.3 批D，方案 Q3 裁定）。
//!
//! 本版本范围 = **判定与文件改写逻辑**，临时文件夹具主机可测、字节级断言：
//! `switch` 改写 `append root=LABEL=rr_X` 行做 rr_a/rr_b 持久切换；`factory`
//! 把该行切到 `rr_factory` 做一次性引导（出厂恢复环境自带回切职责，
//! rr_factory 分区本体永不写入——partition-layout.md 只读约束不破）。
//! 一切落盘走"读 → 改写 → 同目录临时文件 → rename"（rename 同文件系统原子，
//! 断电只得到旧文本或新文本——partition-layout.md 断电安全约束的文件系统面）。
//!
//! 本版本**明确不做**（不 claim 板上语义）：U-Boot 环境变量写侧
//!（`fw_setenv` bootcount/bootlimit/altbootcmd/upgrade_available——回滚链的
//! 一次性兜底）与块设备/挂载探测。这些是板上件，随板上批次过 cfg(linux) 门
//! 实装；标准语义与板上核对项清单登记在 `base/ota/partition-layout.md` 增补段。

use std::fs;
use std::io;
use std::path::Path;

use crate::Rr3Slot;

/// 根定位 token 前缀（extlinux `append` 行与内核 cmdline 同形）
const RR3_ROOT_TOKEN: &str = "root=LABEL=";

/// 一处根定位 token 在文本中的字节区间与标签
struct Rr3RootSpan {
    label: String,
    start: usize,
    end: usize,
}

/// 扫出全部**生效行**中的 `root=LABEL=<label>` token（逐行手扫，不引正则依赖）。
/// `#` 开头（允许前导空白）的注释行跳过——extlinux.conf 头部注释里提到
/// `root=LABEL=rr_a` 属解说文字，不算生效配置。标签 = token 尾部到首个空白；
/// span 只覆盖**标签字节区间**（`root=LABEL=` 前缀保留在原文里，改写时不必重造）。
fn rr3_root_spans(text: &str) -> Vec<Rr3RootSpan> {
    let mut out = Vec::new();
    let mut base = 0usize; // 当前行起始的绝对字节偏移
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('#') {
            base += line.len();
            continue;
        }
        let mut from = 0usize;
        while let Some(pos) = line[from..].find(RR3_ROOT_TOKEN) {
            let tok = from + pos;
            let rest = &line[tok + RR3_ROOT_TOKEN.len()..];
            let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let label_start = base + tok + RR3_ROOT_TOKEN.len();
            out.push(Rr3RootSpan {
                label: rest[..len].to_string(),
                start: label_start,
                end: label_start + len,
            });
            from = tok + RR3_ROOT_TOKEN.len() + len;
        }
        base += line.len();
    }
    out
}

/// 根定位 token 恰好一处、标签在册——否则报形状漂移（配置烂了不许猜）
fn rr3_root_span_unique(text: &str) -> Result<Rr3RootSpan, String> {
    let spans = rr3_root_spans(text);
    match spans.len() {
        1 => Ok(spans.into_iter().next().expect("len==1 已断言")),
        0 => Err("引导配置没有 root=LABEL=rr_ token（append 行缺失或形状漂移）".into()),
        n => Err(format!(
            "引导配置有 {n} 处 root=LABEL=rr_ token——形状歧义，拒改写"
        )),
    }
}

/// 恰好一处且标签在册（rr_a / rr_b / rr_factory），返回（当前槽，区间）
fn rr3_root_span_current(text: &str) -> Result<(Rr3Slot, Rr3RootSpan), String> {
    let span = rr3_root_span_unique(text)?;
    let slot = Rr3Slot::rr3_slot_from_label(&span.label).ok_or_else(|| {
        format!(
            "引导配置 root=LABEL={} 不在 rr-ota 在册标签（rr_a/rr_b/rr_factory）——形状漂移，拒改写",
            span.label
        )
    })?;
    Ok((slot, span))
}

/// 改写核心：把唯一生效的 root=LABEL 标签换成 target 的标签，其余字节一字不动
/// （含注释——工具不碰解说文字）
fn rr3_bootconf_rewrite(text: &str, target: Rr3Slot) -> Result<(String, Rr3Slot), String> {
    let (current, span) = rr3_root_span_current(text)?;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..span.start]);
    out.push_str(target.rr3_slot_label());
    out.push_str(&text[span.end..]);
    Ok((out, current))
}

/// 当前槽判定：引导配置里唯一的 root=LABEL=rr_ token 反查槽位
pub fn rr3_bootconf_current_slot(text: &str) -> Result<Rr3Slot, String> {
    rr3_root_span_current(text).map(|(slot, _)| slot)
}

/// 持久切换：目标只收 rr_a/rr_b。同槽幂等拒（改写请求必须移动到对侧，
/// 不提供"原地重写"这个无意义的副作用）；rr_factory 不走本入口——
/// 恢复出厂是一次性引导语义，走 [`rr3_bootconf_factory`]。
pub fn rr3_bootconf_switch(text: &str, target: Rr3Slot) -> Result<String, String> {
    if target == Rr3Slot::Factory {
        return Err(
            "switch 不接受 rr_factory——恢复出厂走 factory 子命令（一次性引导，rr_factory 分区本体不写）"
                .into(),
        );
    }
    let (out, current) = rr3_bootconf_rewrite(text, target)?;
    if current == target {
        return Err(format!(
            "已在该槽（root=LABEL={}）——同槽幂等拒，不重写引导配置",
            target.rr3_slot_label()
        ));
    }
    Ok(out)
}

/// 一次性出厂引导：把根定位切到 rr_factory。**只改引导配置**，rr_factory
/// 分区本体任何代码路径不得写入（partition-layout.md 只读约束）。
/// "一次性"的兜底链（恢复流程回切 / U-Boot bootcount-altbootcmd）见
/// partition-layout.md 增补段——属板上核对项，本版本不 claim。
pub fn rr3_bootconf_factory(text: &str) -> Result<String, String> {
    let (out, current) = rr3_bootconf_rewrite(text, Rr3Slot::Factory)?;
    if current == Rr3Slot::Factory {
        return Err("引导配置已是 rr_factory——同槽幂等拒".into());
    }
    Ok(out)
}

/// 原子落盘：同目录临时文件写满后 rename 覆盖（同文件系统内 rename 原子，
/// 断电只得到旧文本或新文本，不存在半截文件）。板上加固（写后 fsync）列
/// partition-layout.md 板上核对项，主机可测面不含。
pub fn rr3_bootconf_write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("extlinux.conf");
    let tmp = dir.join(format!(".{name}.rr3-ota-tmp"));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// 从内核 cmdline 文本解析运行槽（板上"当前槽"的权威来源——extlinux.conf
/// 只写进引导配置，真正生效的是启动时的 cmdline）。只认 `root=LABEL=rr_`
/// token 形态；root=/dev/mmcblk… 裸设备形态不覆盖，如实报"板上核对项"。
pub fn rr3_cmdline_current_slot(text: &str) -> Result<Rr3Slot, String> {
    let mut found: Option<Rr3Slot> = None;
    for tok in text.split_whitespace() {
        let Some(rest) = tok.strip_prefix(RR3_ROOT_TOKEN) else {
            continue;
        };
        let slot = Rr3Slot::rr3_slot_from_label(rest)
            .ok_or_else(|| format!("cmdline root=LABEL={rest} 不在在册标签——形状漂移，拒判定"))?;
        if found.is_some() {
            return Err("cmdline 有多处 root=LABEL=rr_ token——形状歧义，拒判定".into());
        }
        found = Some(slot);
    }
    found.ok_or_else(|| {
        "cmdline 无 root=LABEL=rr_ token（root=/dev/… 裸设备形态不覆盖）——板上核对项".into()
    })
}

/// 读 /proc/cmdline（linux 门：/proc 是 linux 特有面；解析逻辑全平台可测）
#[cfg(target_os = "linux")]
pub fn rr3_cmdline_read() -> io::Result<String> {
    fs::read_to_string("/proc/cmdline")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 真工件逐字对拍基准（buildroot 板级目录的那份；文件挪动即编译失败）
    const REAL: &str =
        include_str!("../../../../base/os/buildroot/board/radxa-zero3/extlinux.conf");

    /// 临时目录守卫：断言 panic 后 /tmp 不留残迹（rrd e2e 的 Root 同款纪律）
    struct TmpDir(PathBuf);

    impl TmpDir {
        fn new(tag: &str) -> TmpDir {
            let dir =
                std::env::temp_dir().join(format!("rr3-ota-bootconf-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("建临时目录");
            TmpDir(dir)
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 真工件的当前槽就是 rr_a（extlinux.conf 的 append 行默认从 A 槽起）
    #[test]
    fn current_slot_reads_the_real_artifact() {
        assert_eq!(
            rr3_bootconf_current_slot(REAL).unwrap(),
            Rr3Slot::SlotA,
            "真工件当前槽判定漂移"
        );
    }

    /// switch rr_a→rr_b：生效行的根标签被换、注释一字不动、可逆性逐字还原
    #[test]
    fn switch_to_peer_is_byte_exact_on_the_real_artifact() {
        let got = rr3_bootconf_switch(REAL, Rr3Slot::SlotB).unwrap();
        assert!(
            got.contains("\n    append root=LABEL=rr_b rootwait rw console=ttyS2,1500000"),
            "生效行必须逐字换成 B 槽根: {got}"
        );
        assert!(
            got.contains("# root=LABEL=rr_a：默认从 A 槽起。"),
            "注释是解说文字，工具不得触碰: {got}"
        );
        // 可逆律：把新标签换回去必须逐字还原原文（除生效标签外零字节变动）
        assert_eq!(
            got.replace("root=LABEL=rr_b", "root=LABEL=rr_a"),
            REAL,
            "改写必须只动生效行那一个标签"
        );
        // 对合律：B 上再切回 A，逐字回到原文（peer(peer(s)) == s 的文件面）
        let back = rr3_bootconf_switch(&got, Rr3Slot::SlotA).unwrap();
        assert_eq!(back, REAL, "两次对侧切换必须逐字还原");
    }

    #[test]
    fn switch_rejects_same_slot_and_factory_target() {
        let e = rr3_bootconf_switch(REAL, Rr3Slot::SlotA).unwrap_err();
        assert!(e.contains("同槽幂等拒"), "同槽拒因要点名: {e}");
        let e = rr3_bootconf_switch(REAL, Rr3Slot::Factory).unwrap_err();
        assert!(
            e.contains("factory 子命令"),
            "factory 目标要点名正确入口: {e}"
        );
    }

    #[test]
    fn factory_switches_root_and_is_idempotent() {
        let got = rr3_bootconf_factory(REAL).unwrap();
        assert!(got.contains("root=LABEL=rr_factory"), "{got}");
        assert!(
            !got.contains("append root=LABEL=rr_a"),
            "生效行的原根标签必须被换掉: {got}"
        );
        // 其余字节一字不动：长度差恰好是 rr_a→rr_factory 的标签差
        assert_eq!(
            got.len(),
            REAL.len() - "rr_a".len() + "rr_factory".len(),
            "只许动根标签"
        );
        let e = rr3_bootconf_factory(&got).unwrap_err();
        assert!(e.contains("已是 rr_factory"), "factory 同槽也要幂等拒: {e}");
    }

    #[test]
    fn missing_ambiguous_or_foreign_root_tokens_are_errors() {
        for bad in ["", "\n\n", "label rockrobot3\n    kernel /Image\n"] {
            let e = rr3_bootconf_switch(bad, Rr3Slot::SlotB).unwrap_err();
            assert!(e.contains("root=LABEL"), "缺 token 要点名: {e}");
        }
        let two = format!("{REAL}\nlabel other\n    append root=LABEL=rr_a rootwait\n");
        let e = rr3_bootconf_switch(&two, Rr3Slot::SlotB).unwrap_err();
        assert!(e.contains("歧义"), "多 token 要报歧义: {e}");
        let foreign = REAL.replace("root=LABEL=rr_a", "root=LABEL=rr_x");
        let e = rr3_bootconf_current_slot(&foreign).unwrap_err();
        assert!(e.contains("不在 rr-ota 在册标签"), "杂牌标签要拒: {e}");
    }

    /// 原子写落位后内容正确、临时文件零残留（rename 已把它消耗掉）
    #[test]
    fn atomic_write_renames_into_place_without_tmp_leftover() {
        let tmp = TmpDir::new("atomic");
        let path = tmp.0.join("extlinux.conf");
        fs::write(&path, "root=LABEL=rr_a").expect("写初版");
        let text =
            rr3_bootconf_switch(&fs::read_to_string(&path).unwrap(), Rr3Slot::SlotB).expect("改写");
        rr3_bootconf_write_atomic(&path, &text).expect("原子落盘");
        assert_eq!(fs::read_to_string(&path).unwrap(), "root=LABEL=rr_b");
        let leftovers: Vec<_> = fs::read_dir(&tmp.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("rr3-ota-tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "临时文件必须被 rename 消耗: {leftovers:?}"
        );
    }

    #[test]
    fn cmdline_parse_covers_label_form_and_names_the_gap() {
        let slot =
            rr3_cmdline_current_slot("console=ttyS2,1500000 root=LABEL=rr_b rootwait rw quiet\n")
                .unwrap();
        assert_eq!(slot, Rr3Slot::SlotB);
        let e = rr3_cmdline_current_slot("root=/dev/mmcblk0p3 rootwait\n").unwrap_err();
        assert!(e.contains("板上核对项"), "裸设备形态要点名不覆盖: {e}");
        let e = rr3_cmdline_current_slot("quiet\n").unwrap_err();
        assert!(e.contains("板上核对项"), "无 token 同样不覆盖: {e}");
    }
}
