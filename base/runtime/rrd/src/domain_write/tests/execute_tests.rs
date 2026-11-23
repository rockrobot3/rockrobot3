// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 装配单最小夹具（profile.rs GOOD 锚同族缩减：一域 + 一舵机轴 pin +
/// budget/slots 段全显式）——装载链预核要全链过，段缺即红。
/// 文本 = 数据夹具单一真值（`<manifest>/src/testdata/`）+ 本夹具独有的 hardware/ 尾段。
const PROFILE: &str = concat!(
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/testdata/profile_host_prefix.toml"
    )),
    r#"
[bindings]
"act.servo" = "rr3-act"

[hardware]
alsa_device = "default"
link_ifname = "wlan0"

"#,
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/testdata/pin_servo_pan.toml"
    )),
    r#"
[[pin]]
role = "power.charger.meter"
controller = "i2c1"
addr = "0x70"
device = "INA219"
direction = "in"
mux_group = []

[enabled_slots]
rr3-act = ["act.servo", "power.charger"]

[slots]
present = ["act.servo", "power.charger"]
"#,
);

/// 接线档夹具（§4.3 九字段，profile.rs 测试区 GOOD_WIRING 同形状）：单一真值
/// `<manifest>/src/testdata/wiring_bottom.toml`。单线组只挂 vcc/gnd（对 [[pin]] 增删
/// 免疫——⑦-a 不点名 pin_id），net=rail5v 在夹具域在册（⑦-b）。不复制真仓
/// default/wiring.toml——它引 batt/rail3v3 域与真机 pin 集，与最小装配单必然
/// 失配，写口预核红例必须由真实判据产生、不由夹具失配产生。
const WIRING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/testdata/wiring_bottom.toml"
));

/// rr3-act 的 servo 件装配清单（profile.rs 测试区 stage_servo_module 同形状）：
/// 装载链门 c 对 kind=power 挂轨角色执法⑨ 覆盖判据——无 stage 件则一切
/// "合法过写"用例被 ⑨ 误红（写口预核要全链过，夹具必须供件申报面）。
const ASSEMBLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/testdata/servo_assemble.toml"
));

struct Fixture {
    dir: PathBuf,
    paths: Rr3DomainWritePaths,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// tmp 仓形状夹具：profiles/ + contracts/ 四册 + platform/ + hardware/axis/
/// + docs/evidence/。
///
/// 契约册用最小合法形状（装载链要读）；平台事实表取
/// contracts/platform/rk3566.toml 的仓内真册（⑤-a/⑧-b 词表判据源）。
fn fixture() -> Fixture {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rr3-dw-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("profiles")).unwrap();
    std::fs::create_dir_all(dir.join("contracts/platform")).unwrap();
    std::fs::create_dir_all(dir.join("hardware/test/axis")).unwrap();
    std::fs::create_dir_all(dir.join("docs/evidence")).unwrap();
    let profile = dir.join("profiles/test.toml");
    std::fs::write(&profile, PROFILE).unwrap();
    std::fs::write(
        dir.join("contracts/slots.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../contracts/slots.toml"
        )),
    )
    .unwrap();
    std::fs::write(
        dir.join("contracts/roles.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../contracts/roles.toml"
        )),
    )
    .unwrap();
    std::fs::write(
        dir.join("contracts/devices.toml"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../contracts/devices.toml"
        )),
    )
    .unwrap();
    // 平台事实表：从仓根复制真册（test 面相对 CARGO_MANIFEST_DIR）
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let plat = std::fs::read(repo.join("contracts/platform/rk3566.toml"))
        .expect("仓内 rk3566 平台册必须在");
    std::fs::write(dir.join("contracts/platform/rk3566.toml"), plat).unwrap();
    std::fs::write(dir.join("hardware/test/wiring.toml"), WIRING).unwrap();
    // 件申报面：门 c（⑨ 覆盖判据）要求 kind=power 挂轨角色的绑定模块有
    // demands 申报——stage 一份 servo 件（与 profile.rs 测试区同形状）
    let asm = dir.join("modules/rr3-act/atoms/servo/assemble.toml");
    std::fs::create_dir_all(asm.parent().expect("必有父目录")).unwrap();
    std::fs::write(asm, ASSEMBLE).unwrap();
    let paths = Rr3DomainWritePaths {
        profile: profile.clone(),
        slots: dir.join("contracts/slots.toml"),
        roles: dir.join("contracts/roles.toml"),
        devices: dir.join("contracts/devices.toml"),
        wiring: dir.join("hardware/test/wiring.toml"),
        platform_dir: dir.join("contracts/platform"),
    };
    Fixture { dir, paths }
}

/// 证据文件夹具（frontmatter 六键全，form 与目录一致——四道核全过的形状）
fn write_evidence(dir: &Path, form: &str, num: u32) {
    let fdir = dir.join("docs/evidence").join(form);
    std::fs::create_dir_all(&fdir).unwrap();
    std::fs::write(
                fdir.join(format!("{num:04}.md")),
                format!(
                    "+++\nform = \"{form}\"\nslot_or_role = \"rail5v\"\nmethod = \"板上实测\"\nconclusion = \"阈值实测\"\ndate = \"2026-10-23\"\nrecorder = \"批N22\"\n+++\n\n正文。\n"
                ),
            )
            .unwrap();
}

fn params_armed_true() -> Value {
    json!({
        "target": "domain", "name": "rail5v", "key": "armed",
        "value": true, "evidence": "实测/0001"
    })
}

/// 红一：无确认拒——confirm 缺/false 一律 Permission，零字节落盘
#[cfg(unix)]
#[test]
fn rejects_without_confirm_and_writes_nothing() {
    let fx = fixture();
    let before = std::fs::read_to_string(&fx.paths.profile).unwrap();
    let mut p = rr3_write_parse(&params_armed_true()).unwrap();
    p.confirm = None;
    let (code, msg) = rr3_write_execute(&fx.paths, &p).expect_err("无确认必须拒");
    assert_eq!(code, Rr3StatusCode::Permission);
    assert!(msg.contains("二次确认"), "{msg}");
    let after = std::fs::read_to_string(&fx.paths.profile).unwrap();
    assert_eq!(before, after, "拒写零字节落盘");
}

/// 红二：证据引用非法拒写——形制坏/文件缺都拒，零字节落盘
#[cfg(unix)]
#[test]
fn rejects_bad_evidence_and_writes_nothing() {
    let fx = fixture();
    let before = std::fs::read_to_string(&fx.paths.profile).unwrap();
    // 文件缺（形制合法但四道核第二道红）
    let mut p = rr3_write_parse(&params_armed_true()).unwrap();
    p.confirm = Some(true);
    let (code, msg) = rr3_write_execute(&fx.paths, &p).expect_err("证据缺必须拒");
    assert_eq!(code, Rr3StatusCode::Permission);
    assert!(msg.contains("证据引用核验不过"), "{msg}");
    // 形制坏
    let mut p = rr3_write_parse(&json!({
        "target": "domain", "name": "rail5v", "key": "armed",
        "value": true, "evidence": "not-a-ref"
    }))
    .unwrap();
    p.confirm = Some(true);
    let (code, _) = rr3_write_execute(&fx.paths, &p).expect_err("形制坏必须拒");
    assert_eq!(code, Rr3StatusCode::Permission);
    assert_eq!(
        before,
        std::fs::read_to_string(&fx.paths.profile).unwrap(),
        "两次拒写零字节落盘"
    );
}

/// 绿：合法过写——armed=true + 证据四道核全过，档内 armed/evidence 联动
/// 成对落盘，装载预核过，应答带 effective:boot。武装前提（⑥-c：有测点 +
/// 龄期上界 > 0）由夹具覆写供齐——profile.rs 测试区
/// armed_domain_evidence_file_check_gate 同一构造（sense 指在册 i2c 测点）。
#[cfg(unix)]
#[test]
fn armed_write_persists_paired_shape_and_answers_boot() {
    let fx = fixture();
    write_evidence(&fx.dir, "实测", 1);
    // 武装前提：域 sense 绑在册测点 pin + max_age_ms > 0（⑥-c 两前件）
    let base = std::fs::read_to_string(&fx.paths.profile).unwrap();
    let text = base
        .replace("sense = \"none\"", "sense = \"power.charger.meter\"")
        .replace("max_age_ms = 0", "max_age_ms = 30000");
    std::fs::write(&fx.paths.profile, text).unwrap();
    let mut p = rr3_write_parse(&params_armed_true()).unwrap();
    p.confirm = Some(true);
    let v = rr3_write_execute(&fx.paths, &p).expect("合法过写应成功");
    assert_eq!(v["ok"], true);
    assert_eq!(v["old"], false);
    assert_eq!(v["new"], true);
    assert_eq!(v["effective"], "boot");
    let after = std::fs::read_to_string(&fx.paths.profile).unwrap();
    assert!(after.contains("armed = true"), "{after}");
    assert!(after.contains("evidence = \"实测/0001\""), "{after}");
}

/// 绿：severe_mv 过写——只动目标键行，联动 evidence 不入档（仅留痕）
#[cfg(unix)]
#[test]
fn severe_mv_write_touches_target_line_only() {
    let fx = fixture();
    write_evidence(&fx.dir, "实测", 1);
    let mut p = rr3_write_parse(&json!({
        "target": "domain", "name": "rail5v", "key": "severe_mv",
        "value": 4300, "evidence": "实测/0001"
    }))
    .unwrap();
    p.confirm = Some(true);
    let v = rr3_write_execute(&fx.paths, &p).expect("合法过写应成功");
    assert_eq!(v["old"], 4400);
    assert_eq!(v["new"], 4300);
    let after = std::fs::read_to_string(&fx.paths.profile).unwrap();
    assert!(after.contains("severe_mv = 4300"), "{after}");
    assert!(
        after.contains("evidence = \"none\""),
        "evidence 不入档: {after}"
    );
}

/// 并发串行化（B38 审计）：两个不同键的写并发——执行段锁下两改都在，
/// 无丢写、无候选 tmp 互踩（不加锁时候选档被对方先清后建，预核与提交
/// 各读一份文本，偶发丢写）。窗口极窄，本用例是串行化回归钉不是竞态证明。
#[cfg(unix)]
#[test]
fn concurrent_writes_serialize_and_keep_both() {
    let fx = fixture();
    write_evidence(&fx.dir, "实测", 1);
    let base = std::fs::read_to_string(&fx.paths.profile).unwrap();
    let text = base
        .replace("sense = \"none\"", "sense = \"power.charger.meter\"")
        .replace("max_age_ms = 0", "max_age_ms = 30000");
    std::fs::write(&fx.paths.profile, text).unwrap();
    let mut armed = rr3_write_parse(&params_armed_true()).unwrap();
    armed.confirm = Some(true);
    let mut severe = rr3_write_parse(&json!({
        "target": "domain", "name": "rail5v", "key": "severe_mv",
        "value": 4300, "evidence": "实测/0001"
    }))
    .unwrap();
    severe.confirm = Some(true);
    let (pa, pb) = (fx.paths.clone(), fx.paths.clone());
    let h1 = std::thread::spawn(move || rr3_write_execute(&pa, &armed));
    let h2 = std::thread::spawn(move || rr3_write_execute(&pb, &severe));
    h1.join()
        .expect("写线程不得 panic")
        .expect("armed 写应成功");
    h2.join()
        .expect("写线程不得 panic")
        .expect("severe_mv 写应成功");
    let after = std::fs::read_to_string(&fx.paths.profile).unwrap();
    assert!(after.contains("armed = true"), "{after}");
    assert!(
        after.contains("severe_mv = 4300"),
        "并发写不得丢写: {after}"
    );
    assert!(after.contains("evidence = \"实测/0001\""), "{after}");
}

/// 键/值型不配（B39 审计）：参数结构破损落成 Invalid 拒，不再 panic——
/// 正常路径由 parse 层白名单拦在前面，本用例钉的是拒面表达方式：写面住在
/// 守护线程里，panic = 断连接、丢应答。
#[cfg(unix)]
#[test]
fn plan_rejects_key_value_mismatch_without_panic() {
    let fx = fixture();
    let dom = crate::profile::Rr3Domain {
        armed: false,
        severe_mv: 4400,
        ..Default::default()
    };
    let p = Rr3DomainWriteParams {
        target: Rr3WriteTarget::Domain,
        name: "rail5v".to_string(),
        key: "nominal_mv".to_string(),
        value: Rr3WriteValue::Num(1),
        evidence: None,
        confirm: Some(true),
    };
    let (code, msg) = rr3_domain_plan(&fx.paths, &p, &dom).expect_err("白名单外组合必须拒");
    assert_eq!(code, Rr3StatusCode::Invalid);
    assert!(msg.contains("不配"), "{msg}");
}

/// 红三：手术后装载预核不过拒落盘——未标定域（sense=none、max_age_ms=0）
/// 武装即撞装载链 ⑥-c：写口证据门过 ≠ 预核免（双门串联），tmp 产物零字节
/// 进正档、旧档原封。mild>severe 次序判据归批N2 门 c（消费者侧，未落装载
/// 链）——预核红例必须用装载链真判，不许造"判据还不在"的假红例。
#[cfg(unix)]
#[test]
fn rejects_when_preboot_check_fails() {
    let fx = fixture();
    write_evidence(&fx.dir, "实测", 1);
    let before = std::fs::read_to_string(&fx.paths.profile).unwrap();
    let mut p = rr3_write_parse(&params_armed_true()).unwrap();
    p.confirm = Some(true);
    let (code, msg) = rr3_write_execute(&fx.paths, &p).expect_err("预核红必须拒");
    assert_eq!(code, Rr3StatusCode::Invalid);
    assert!(msg.contains("预核不过"), "{msg}");
    assert!(msg.contains("⑥-c"), "预核红须点名装载链判据: {msg}");
    assert_eq!(
        before,
        std::fs::read_to_string(&fx.paths.profile).unwrap(),
        "预核拒零字节落盘"
    );
    // tmp 不残留
    let stem = fx.paths.profile.file_stem().unwrap().to_str().unwrap();
    assert!(
        !fx.paths
            .profile
            .with_file_name(format!("{stem}.tmp-write.toml"))
            .exists(),
        "拒写须清 tmp"
    );
}

/// axis 落点：合法过写复用产线写入面——轴档 current_ceil_ma 变更落盘
#[cfg(unix)]
#[test]
fn axis_write_reuses_production_writer() {
    let fx = fixture();
    write_evidence(&fx.dir, "对拍", 1);
    // 轴档夹具（十四键，pin 对账装配单 [[pin]]）
    std::fs::write(
                fx.dir.join("hardware/test/axis/pan.toml"),
                "schema = 1\npin = \"act.axis.pwm.pan\"\ncenter_deg = 90\nlo_deg = 60\nhi_deg = 120\ndir_sign = 1\nslew_deg_s = 100\nsettle_ms = 200\nhold = \"release\"\ncurrent_ceil_ma = 750\nwarmup_ms = 0\nhard_stop = \"yes\"\ncalibrated = true\nsource = \"manual\"\n",
            )
            .unwrap();
    let mut p = rr3_write_parse(&json!({
        "target": "axis", "name": "pan", "key": "current_ceil_ma",
        "value": 700, "evidence": "对拍/0001"
    }))
    .unwrap();
    p.confirm = Some(true);
    let v = rr3_write_execute(&fx.paths, &p).expect("axis 过写应成功");
    assert_eq!(v["old"], 750);
    assert_eq!(v["new"], 700);
    assert_eq!(v["effective"], "boot");
    let after = std::fs::read_to_string(fx.dir.join("hardware/test/axis/pan.toml")).unwrap();
    assert!(after.contains("current_ceil_ma = 700"), "{after}");
}

/// axis 落点红面：轴档不在场拒（立档走产线写入面，写口不立新档）
#[cfg(unix)]
#[test]
fn axis_write_rejects_missing_axis_file() {
    let fx = fixture();
    write_evidence(&fx.dir, "对拍", 1);
    let mut p = rr3_write_parse(&json!({
        "target": "axis", "name": "pan", "key": "current_ceil_ma",
        "value": 700, "evidence": "对拍/0001"
    }))
    .unwrap();
    p.confirm = Some(true);
    let (code, msg) = rr3_write_execute(&fx.paths, &p).expect_err("档缺必须拒");
    assert_eq!(code, Rr3StatusCode::Invalid);
    assert!(msg.contains("不在场"), "{msg}");
}
