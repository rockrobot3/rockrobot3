//! 执行件档案写入面（批N3 件四，全案 §6 路线 A"立即可用，随批N3 落手册与档案
//! 写入"；路线 C 的起步前置同面——§6 前置列"档案写入面即可起步"）。
//!
//! 职责边界（机制与策略分离）：装载、校验、序列化、原子落盘的**机制**全在
//! [`crate::axis`] 与 [`crate::profile`]——本模块只做**写入编排的策略层**：
//! 谁能写（档案外键 pin 必须 ∈ 装配单 [[pins]] 的 pin_id 全集，F-4 写侧半边）、
//! 写到哪（`hardware/<形态>/axis/<instance>.toml`，形态名 = 装配单文件名 stem，
//! 与 [`crate::wiring::rr3_wiring_path_for_profile`] 同一布局）、什么不许写
//! （坏目标不覆盖——件二装载域"不许覆盖病灶"的写侧镜像；同 pin 异名副本不
//! 制造——副本会让 boot 链"恰一份合法才投影"拒注入，件三已定性，写入时先挡）。
//!
//! 为什么是独立产线小工具而不是 rrd daemon 的子命令：daemon 的 CLI 是守护进程
//! 参数面（socket/tick/manifest），混进一次性写档案会让两种生命周期各说一套；
//! 产线工人（路线 A 治具流程）与批N4 面板动作面要的是同一个写路径——本工具是
//! [`crate::axis::rr3_act_profile_store`] 的第一个接线位，批N4 面板接线时复用
//! 同一函数（§7"落盘只许有一份实现"，第二条写路径即红）。
//!
//! rtos 档不在本面射程内：`Rr3Store` 三项载体性质（工单 S-14）实测前
//! `calibrated` 只许运行期内存态，档案在 rtos 形态今天无处可落（§4.4 载体
//! 语义②）——本工具只服务 host 档，rtos 行如实写"重启即失"。
//!
//! 标定动作纪律（本工具不越权）：N2 收口前只准单步点动＋人工确认（施工计划
//! §3 批N3 行）；本工具只落盘**人工流程的产物**，不驱动任何运动——标定动作
//! 的拒动判据在 [`rr_hal_api::rr3_act_motion_allowed`]，写入面与运动面零交叠。

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::axis::{rr3_act_profile_from_text, rr3_act_profile_load, rr3_act_profile_store};

/// 写入参数：`--profile` 装配单（pin_id 全集与形态名的唯一输入）＋
/// `--candidate` 候选档案（十四键平表全文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3AxisWriteArgs {
    /// 装配单路径（`<root>/profiles/<形态>.toml`）
    pub profile: PathBuf,
    /// 候选档案路径（完整十四键，装载判据全绿才落盘）
    pub candidate: PathBuf,
}

const USAGE: &str = "用法: axis_write --profile <装配单.toml> --candidate <候选档案.toml>\n说明: 执行件档案写入面（批N3 件四）——候选档案过装载全判据（十四键全键必填、类型域界、词表在册、行程自洽、pin 外键 ∈ 装配单 [[pins]]）后，原子落盘到 hardware/<形态>/axis/<instance>.toml（tmp→fsync→rename 唯一写路径）。坏目标不覆盖；同 pin 异名副本拒写（boot 链恰一份合法才投影）。产线标定流程见 docs/process/dev/真件装配与标定手册.md 件2b。";

/// 路径型开关取值：缺参数与疑似开关的值都拒（与 rrd cli 的
/// `need_path_value` 同一条口径——fail-open 的形状两处都不许有）。
fn need_path_value(flag: &str, args: &[String], i: usize) -> Result<PathBuf, String> {
    let raw = args
        .get(i + 1)
        .ok_or_else(|| format!("{flag} 缺参数\n{USAGE}"))?;
    if raw.starts_with('-') {
        return Err(format!(
            "{flag} 缺参数：收到疑似开关的 {raw}（路径值不得以 - 开头）\n{USAGE}"
        ));
    }
    Ok(PathBuf::from(raw))
}

/// 解析写入参数。未知开关 / 缺参数 / `-` 开头路径一律报错不回落（fail-closed）。
pub fn rr3_axis_write_parse(args: &[String]) -> Result<Rr3AxisWriteArgs, String> {
    let mut profile: Option<PathBuf> = None;
    let mut candidate: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--profile" => {
                profile = Some(need_path_value("--profile", args, i)?);
                i += 2;
            }
            "--candidate" => {
                candidate = Some(need_path_value("--candidate", args, i)?);
                i += 2;
            }
            "-h" | "--help" => {
                return Err(USAGE.to_string());
            }
            other => return Err(format!("未知选项 {other}\n{USAGE}")),
        }
    }
    let profile = profile.ok_or_else(|| format!("--profile 缺失\n{USAGE}"))?;
    let candidate = candidate.ok_or_else(|| format!("--candidate 缺失\n{USAGE}"))?;
    Ok(Rr3AxisWriteArgs { profile, candidate })
}

/// 写入编排（策略全在本函数，逐条显式）：装配单装载 → 候选全判据 → 拒绝面
/// （坏目标不覆盖 / 同 pin 异名副本不制造）→ 原子落盘。成功回显写入路径与
/// 档案要点；失败逐条点名，**不落任何字节**。
pub fn rr3_axis_write_run(cfg: &Rr3AxisWriteArgs) -> Result<String, String> {
    // ① 装配单装载：绝对路径（与 daemon 的 rr3_profile_absolute_path 同口径，
    //    装机现场读不到装配单，pin_id 全集无从谈起）→ 文本 → 结构。
    //    写入面只吃装载域，不做 boot 的十条校验——整机装配合法性是 boot 正门的
    //    职责，写入时刻不重复判（判了就是第二份裁决点，迟早分叉）。
    let profile_abs = crate::profile::rr3_profile_absolute_path(&cfg.profile)?;
    let profile_text = std::fs::read_to_string(&profile_abs)
        .map_err(|e| format!("读不到装配单 {}: {e}", profile_abs.display()))?;
    let profile = crate::profile::rr3_profile_from_text(&profile_text).map_err(|errs| {
        format!(
            "装配单 {} 形状不合: {}",
            profile_abs.display(),
            errs.join("; ")
        )
    })?;
    let pin_ids: BTreeSet<String> = profile.pins.iter().map(|p| p.rr3_pin_id()).collect();

    // ② 落位：形态名 = 装配单文件名 stem（rr3_wiring_path_for_profile 推导的
    //    同一布局——profiles/<形态>.toml ↔ hardware/<形态>/），axis/ 在其下。
    //    接线档本身不读：写入面的输入只有装配单与候选档案。
    let wiring_path = crate::wiring::rr3_wiring_path_for_profile(&cfg.profile)?;
    let axis_dir = wiring_path
        .parent()
        .expect("wiring 路径必有父目录")
        .join("axis");

    // ③ 候选全判据：十四键全键必填、类型域界、词表在册、行程自洽、pin 外键
    //    ∈ [[pins]]（F-4 写侧半边——装配单没有的 pin，写进去就是 boot 必红的死档）。
    let candidate_path = crate::profile::rr3_profile_absolute_path(&cfg.candidate)?;
    let candidate_text = std::fs::read_to_string(&candidate_path)
        .map_err(|e| format!("读不到候选档案 {}: {e}", candidate_path.display()))?;
    let file = rr3_act_profile_from_text(
        &candidate_text,
        &candidate_path.display().to_string(),
        &pin_ids,
    )
    .map_err(|errs| errs.join("; "))?;

    // ④ 文件名 = instance（pin 末段）。两段式（role.instance）是轴档案的规范词形
    //    ——per_instance 角色的 pin_id 由装配单侧生成，无实例段的 pin 说明装配单
    //    本身形状有问题，写入面不猜（落错名就是副本的另一种形态）。
    let instance = file
        .pin
        .rsplit('.')
        .next()
        .filter(|seg| !seg.is_empty() && *seg != file.pin)
        .ok_or_else(|| {
            format!(
                "候选 pin = {:?} 不是 role.instance 两段式（act.axis.pwm.pan 形），拒绝猜测落位",
                file.pin
            )
        })?;
    let target = axis_dir.join(format!("{instance}.toml"));

    // ⑤ 拒绝面 a——坏目标不覆盖：目标在场但装载判据红，说明上一份已经病了；
    //    覆盖它等于把病灶誊进下一轮（件二装载域的"不许覆盖"是读侧视角，
    //    这里是同一条纪律的写侧视角）。
    if target.exists() {
        match rr3_act_profile_load(&target, &pin_ids) {
            Err(errs) => {
                return Err(format!(
                    "目标 {} 已存在且装载红（{}）——坏档案不许覆盖，先修或手工移除后再写",
                    target.display(),
                    errs.join("; ")
                ));
            }
            Ok(Some(existing)) => {
                // ⑥ 拒绝面 b 前置——同文件名装别的轴：pin 是档案身份键，文件名
                //    不是；同名异 pin 意味着有人拿 instance 名当 free slot 用，
                //    写进去装载链对账照样红，先在这里点名。
                if existing.pin != file.pin {
                    return Err(format!(
                        "目标 {} 已存在且 pin = {:?}（候选 {:?}）——instance 落位与档案身份冲突，拒绝覆盖",
                        target.display(),
                        existing.pin,
                        file.pin
                    ));
                }
            }
            Ok(None) => unreachable!("target.exists() 为真时装载不会回 None"),
        }
    }

    // ⑦ 拒绝面 b——同 pin 异名副本不制造：boot 链"恰一份合法才投影"（件三），
    //    两份同 pin 合法档 = 注入空串 + 点名"消歧归件五"。产线写入先把这道红
    //    挡在写入时，别等 boot 才发现。装载红的散件不在此判（别人的病灶由
    //    boot 链点名，写入面不越权替修）。
    if let Ok(entries) = std::fs::read_dir(&axis_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p == target || p.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            if let Ok(Some(other)) = rr3_act_profile_load(&p, &pin_ids) {
                if other.pin == file.pin {
                    return Err(format!(
                        "axis/ 下已有 {} 装同一 pin {:?}（候选目标 {}）——同 pin 两份合法档会让 boot 链拒注入（恰一份才投影），先清理副本",
                        p.display(),
                        file.pin,
                        target.display()
                    ));
                }
            }
        }
    }

    // ⑧ 原子落盘：tmp 写全量 → fsync → rename，失败清 tmp、旧档原封——
    //    "失败即退回上一份档案"的机制保证就在这条原子序里（§6 硬约束①），
    //    本函数不再叠加第二套备份/回滚。
    std::fs::create_dir_all(&axis_dir)
        .map_err(|e| format!("建档案目录 {} 失败: {e}", axis_dir.display()))?;
    rr3_act_profile_store(&target, &file)
        .map_err(|e| format!("写入 {} 失败: {e}", target.display()))?;

    // ⑨ 回显：产线工人要当场看到"写了什么、多久生效"——档案在 boot 链的
    //    投影时点是 rrd 下次 --profile boot 扫描 axis/ 时（恰一份合法才投影）。
    let p = &file.profile;
    Ok(format!(
        "已写入 {}\n  pin = {}  calibrated = {}  hard_stop = {}  source = {}\n  生效时点：rrd 下次 --profile boot 扫描 axis/ 时投影执行轴状态（恰一份合法才投影）",
        target.display(),
        file.pin,
        p.calibrated,
        p.hard_stop.rr3_act_hard_stop_name(),
        p.source.rr3_act_source_name(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 装配单最小夹具（schema_version = 5，与 e2e_act PROFILE 同形状缩减到
    /// 一条舵机轴 pin）——写入面的输入只有它，接线档与模块装配都不需要。
    const PROFILE: &str = r#"
schema_version = 5

[os]
route = "distro"

[runtime]
log_ring_cap = 512

[platform]
tier = "host"
chip = "rk3566"

[supplies]
watchdog_present = false
watchdog_timeout_ms = 15000
watchdog_feeder = "rrd"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

[[supplies.domain]]
name = "rail5v"
nominal_mv = 5000
mild_mv = 4600
severe_mv = 4400
current_cap_ma = 2000
sense = "none"
current_sense = "none"
switch = "none"
switchable = false
armed = false
max_age_ms = 0

[supplies.budget]
not_applicable = true

[slots]
present = ["act.servo"]

[bindings]
"act.servo" = "rr3-act"

[enabled_slots]
rr3-act = ["act.servo"]

[[pin]]
role = "act.axis.pwm"
instance = "pan"
controller = "pwm0"
channel = 0
physical = [12]
direction = "out"
rail = "rail5v"
failsafe = "release"
mux_group = []
"#;

    /// 合法候选（§4.4 示例档值面，与 axis.rs 单测 sample_text 同形状）：
    /// 手工标定产物——calibrated=true + hard_stop=yes + source=manual。
    fn candidate_text(center: u16) -> String {
        [
            "schema = 1",
            "pin = \"act.axis.pwm.pan\"",
            &format!("center_deg = {center}"),
            "lo_deg = 60",
            "hi_deg = 120",
            "dir_sign = 1",
            "slew_deg_s = 100",
            "settle_ms = 200",
            "hold = \"release\"",
            "current_ceil_ma = 750",
            "warmup_ms = 0",
            "hard_stop = \"yes\"",
            "calibrated = true",
            "source = \"manual\"",
        ]
        .join("\n")
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rrd-axis-write-t-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 装机布局夹具：profiles/default.toml（形态名 = 文件名 stem）。Drop 清树，
    /// 断言 panic 后 /tmp 不留残迹。
    struct Root {
        dir: PathBuf,
    }

    impl Root {
        fn new(tag: &str) -> Root {
            let dir = tmpdir(tag);
            std::fs::create_dir_all(dir.join("profiles")).unwrap();
            Root { dir }
        }

        fn profile(&self) -> PathBuf {
            let p = self.dir.join("profiles").join("default.toml");
            std::fs::write(&p, PROFILE).unwrap();
            p
        }

        fn write_candidate(&self, text: &str) -> PathBuf {
            let p = self.dir.join("candidate.toml");
            std::fs::write(&p, text).unwrap();
            p
        }

        /// 装配链的落位（写入面推导的同一位置）：hardware/default/axis/
        fn axis_dir(&self) -> PathBuf {
            self.dir.join("hardware").join("default").join("axis")
        }

        fn target(&self) -> PathBuf {
            self.axis_dir().join("pan.toml")
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn args_of(profile: &Path, candidate: &Path) -> Rr3AxisWriteArgs {
        Rr3AxisWriteArgs {
            profile: profile.to_path_buf(),
            candidate: candidate.to_path_buf(),
        }
    }

    #[test]
    fn candidate_passing_full_judgement_is_stored_atomically() {
        let root = Root::new("ok");
        let profile = root.profile();
        let candidate = root.write_candidate(&candidate_text(90));

        let out = rr3_axis_write_run(&args_of(&profile, &candidate)).expect("合法候选必须落盘");
        assert!(
            out.contains(&root.target().display().to_string()),
            "回显必须点名写入路径: {out}"
        );
        assert!(out.contains("calibrated = true"), "回显要点名标定位: {out}");

        // 落盘内容过装载闭环：写进去的档案能被 boot 链同一路径装载回来
        let pin_ids: BTreeSet<String> = ["act.axis.pwm.pan".to_string()].into_iter().collect();
        let loaded = rr3_act_profile_load(&root.target(), &pin_ids)
            .expect("写出的档案必须装载绿")
            .expect("档案必须在场");
        assert_eq!(loaded.pin, "act.axis.pwm.pan");
        assert_eq!(loaded.profile.center_deg, 90);
        assert!(loaded.profile.calibrated);
    }

    #[test]
    fn candidate_failing_judgement_writes_nothing() {
        let root = Root::new("badcand");
        let profile = root.profile();
        // 缺 calibrated 键（F-6 全键必填）：判据红 → 一个字节都不许落
        let broken = candidate_text(90).replace("calibrated = true\n", "");
        let candidate = root.write_candidate(&broken);

        let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("缺键必须拒");
        assert!(e.contains("calibrated"), "拒因要点名病灶键: {e}");
        assert!(!root.target().exists(), "判据红的候选不许落盘");
    }

    #[test]
    fn candidate_pin_must_hit_assembly_pins() {
        let root = Root::new("pinfk");
        let profile = root.profile();
        // 装配单只有 pan：tilt 不在 pin_id 全集（F-4 外键对账的写侧半边）
        let text =
            candidate_text(90).replace("pin = \"act.axis.pwm.pan\"", "pin = \"act.axis.pwm.tilt\"");
        let candidate = root.write_candidate(&text);

        let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("外键未命中必须拒");
        assert!(
            e.contains("act.axis.pwm.tilt") && e.contains("pin_id"),
            "拒因要点名 pin 与对账面: {e}"
        );
        assert!(!root.target().exists());
    }

    #[test]
    fn broken_target_is_never_overwritten() {
        let root = Root::new("brokentgt");
        let profile = root.profile();
        let candidate = root.write_candidate(&candidate_text(90));

        // 预置一份装载红的坏档（缺键）在目标位
        let axis = root.axis_dir();
        std::fs::create_dir_all(&axis).unwrap();
        std::fs::write(root.target(), "schema = 1\npin = \"act.axis.pwm.pan\"\n")
            .expect("预置坏档");

        let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("坏目标必须拒覆盖");
        assert!(
            e.contains("不许覆盖") && e.contains("装载红"),
            "拒因要点名不许覆盖与病灶: {e}"
        );
        let after = std::fs::read_to_string(root.target()).unwrap();
        assert_eq!(
            after, "schema = 1\npin = \"act.axis.pwm.pan\"\n",
            "坏档必须原封不动"
        );
    }

    #[test]
    fn same_pin_under_other_name_is_refused() {
        let root = Root::new("duppin");
        let profile = root.profile();
        let candidate = root.write_candidate(&candidate_text(90));

        // 预置一份合法同 pin 异名档（遗留副本的真实形貌——文件名不是身份键，
        // 装载链按 pin 对账；两份合法 = boot 拒注入）
        let axis = root.axis_dir();
        std::fs::create_dir_all(&axis).unwrap();
        std::fs::write(axis.join("pan-copy.toml"), candidate_text(90)).expect("预置副本");

        let e = rr3_axis_write_run(&args_of(&profile, &candidate)).expect_err("同 pin 副本必须拒");
        assert!(
            e.contains("pan-copy.toml") && e.contains("恰一份"),
            "拒因要点名副本文件与投影纪律: {e}"
        );
        assert!(!root.target().exists(), "副本在场时目标位不许再写");
    }

    #[test]
    fn existing_profile_for_same_pin_is_replaced() {
        let root = Root::new("replace");
        let profile = root.profile();

        // 先写一版（center=90），再写修订版（center=95）：同一 pin 的版本化更新
        // 是写入面的正路（档案"版本化、可回滚"，§4.4）——原子序保证要么全新要么全旧。
        let v1 = root.write_candidate(&candidate_text(90));
        rr3_axis_write_run(&args_of(&profile, &v1)).expect("首版必须落盘");
        let v2 = root.write_candidate(&candidate_text(95));
        rr3_axis_write_run(&args_of(&profile, &v2)).expect("修订版必须落盘");

        let pin_ids: BTreeSet<String> = ["act.axis.pwm.pan".to_string()].into_iter().collect();
        let loaded = rr3_act_profile_load(&root.target(), &pin_ids)
            .expect("修订版必须装载绿")
            .expect("档案必须在场");
        assert_eq!(loaded.profile.center_deg, 95, "修订值必须生效");
    }

    #[test]
    fn parse_requires_both_paths_and_rejects_unknown_flags() {
        let ok = rr3_axis_write_parse(&[
            "--profile".into(),
            "/p/default.toml".into(),
            "--candidate".into(),
            "/p/c.toml".into(),
        ])
        .expect("双参数必须收");
        assert_eq!(ok.profile, PathBuf::from("/p/default.toml"));
        assert_eq!(ok.candidate, PathBuf::from("/p/c.toml"));

        let e = rr3_axis_write_parse(&["--profile".into(), "/p/default.toml".into()])
            .expect_err("缺 --candidate 必须拒");
        assert!(e.contains("--candidate 缺失") && e.contains("用法"), "{e}");
        let e = rr3_axis_write_parse(&[]).expect_err("全缺必须拒");
        assert!(e.contains("--profile 缺失"), "{e}");
        let e = rr3_axis_write_parse(&["--bogus".into()]).expect_err("未知开关必须拒");
        assert!(e.contains("未知选项 --bogus"), "{e}");
        // 疑似开关不当值吞下（与 rrd cli 的 need_path_value 同一条口径）
        let e =
            rr3_axis_write_parse(&["--profile".into(), "--candidate".into(), "/p/c.toml".into()])
                .expect_err("开关名不当值必须拒");
        assert!(
            e.contains("--profile 缺参数") && e.contains("--candidate"),
            "{e}"
        );
    }
}
