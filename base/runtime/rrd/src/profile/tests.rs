//! `profile` 面的单测（批B14 自 `profile.rs` 尾部内联块原样迁出）。
//!
//! 共享夹具（真档文本、装配单装载、平台表夹具、预算夹具）在本文件；
//! 判据用例按被试面分居 `tests/` 子模块：`boot`（装载输入面）、`budget`
//! （预算与档位）、`parse`（装配单解析面）、`surgery`（键行手术）、
//! `surgery_gates`（结构门与真档钻取）、`surgery_pins`（格位与引脚）。

use super::*;

mod boot;
mod budget;
mod parse;
mod surgery;
mod surgery_gates;
mod surgery_pins;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rr_registry::Rr3AssembleDemand;

/// 装载消费真值集清单逐成员钉死（基-25／实-12：清单真值点的成员判据反例面）。
/// 以真仓两份装配单为源（chip 选册成员随装配单变——distro→rk3566、
/// esp32-mini→esp32），轴档案目录成员恒在清单（在则随装、缺目录合法由消费方裁）。
/// 成员名单有变必须改 [`rr3_truth_set`] 本体并同步三处消费方，本用例红即拦截。
#[test]
fn runtime_truth_set_members_are_pinned() {
    let distro = rr3_profile_parse(include_str!("../../../../../profiles/distro.toml"))
        .expect("真仓 distro 装配单必须合法");
    let t = rr3_truth_set(&distro, "distro");
    assert_eq!(
        t.files,
        vec![
            "profiles/distro.toml",
            "contracts/topics.toml",
            "contracts/slots.toml",
            "contracts/roles.toml",
            "contracts/devices.toml",
            "contracts/platform/rk3566.toml",
            "hardware/distro/wiring.toml",
        ]
    );
    assert_eq!(t.dirs, vec!["hardware/distro/axis"]);
    let rtos = rr3_profile_parse(include_str!("../../../../../profiles/esp32-mini.toml"))
        .expect("真仓 esp32-mini 装配单必须合法");
    let t2 = rr3_truth_set(&rtos, "esp32-mini");
    assert!(
        t2.files
            .contains(&"contracts/platform/esp32.toml".to_string()),
        "chip 选册成员随装配单 [platform].chip 变（esp32-mini → esp32）"
    );
    assert_eq!(t2.dirs, vec!["hardware/esp32-mini/axis"]);
}

/// 真值集三方成员判等（基-40／⑨-z 判据实体）：装载面清单
/// （[`rr3_truth_set`]）与 boot 记账定位（[`rr3_truth_locations`]
/// 的成员名）逐条相等——成员名单点生成（[`rr3_truth_names`]），两
/// 消费方手抄即漂移温床，本用例红即拦截。定位形状一并钉死：装配单本体 =
/// `--profile` canonicalize 产物、契约四册 = `--contracts` join、平台事实表 =
/// platform/ join 选册、接线档 = 接线布局推导原样记账。
#[test]
fn truth_set_members_and_boot_locations_are_the_same_set() {
    let distro = rr3_profile_parse(include_str!("../../../../../profiles/distro.toml"))
        .expect("真仓 distro 装配单必须合法");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let profile_path = repo.join("profiles/distro.toml");
    let contracts_dir = repo.join("contracts");
    let wiring_path = repo.join("hardware/distro/wiring.toml");
    let t = rr3_truth_set(&distro, "distro");
    let locs = rr3_truth_locations(
        &profile_path,
        &contracts_dir,
        &wiring_path,
        &distro.platform_chip,
    )
    .expect("真仓七成员应全部定位成功");
    let members: Vec<String> = locs.iter().map(|l| l.member.clone()).collect();
    assert_eq!(
        t.files, members,
        "装载面清单与 boot 记账定位成员名逐条相等（⑨-z 判据实体——非空差即红）"
    );
    // 定位形状抽三处钉死（装配单/平台选册/接线档——四册 join 形状由
    // contracts.rs 单测覆盖）
    assert_eq!(locs[0].path, profile_path.canonicalize().unwrap());
    assert_eq!(
        locs[5].path,
        contracts_dir.join(format!("platform/{}.toml", distro.platform_chip))
    );
    assert_eq!(locs[6].path, wiring_path);
}

const GOOD: &str = concat!(
    r#"
schema_version = 5

[os]
route = "distro"

[runtime]
log_ring_cap = 512
hal_backend = "sim"

[platform]
tier = "host"
chip = "rk3566"

[supplies]
watchdog_present = true
watchdog_timeout_ms = 15000
watchdog_feeder = "rrd"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

"#,
    include_str!("../testdata/domain_batt_none.toml"),
    include_str!("../testdata/domain_rail5v.toml"),
    r#"
# [supplies.budget]（schema 4，校验⑩ 分母）：host 档合法不适用必须显式写，不许留空
[supplies.budget]
not_applicable = true

"#,
    include_str!("../testdata/mod3_bindings_enabled.toml"),
    r#"
[slots]
present = ["act.servo", "cognition.llm", "power.charger"]

"#,
    include_str!("../testdata/pin_servo_pan.toml"),
    r#"
[[pin]]
role = "power.charger.meter"
controller = "i2c1"
addr = "0x70"
device = "INA219"
direction = "in"
mux_group = []
"#,
);

/// 接线档底稿（§4.3 九字段，与 wiring.rs 测试区 GOOD 同形状）：单一真值
/// `testdata/wiring_bottom.toml`。单线组只挂 vcc/gnd 与电力导体（net=rail5v
/// 在 GOOD 两域在册）——对 [[pin]] 增删免疫，删 pin 的用例不因 ⑦-a 误红。
/// 校验⑦ 反例用 `write_wiring` 覆写。
const GOOD_WIRING: &str = include_str!("../testdata/wiring_bottom.toml");

/// 全部规则走真的 contracts/slots.toml 与 contracts/roles.toml（查册真值同源），
/// 夹具落 pid 唯一临时目录
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new(tag: &str) -> Root {
        let dir = std::env::temp_dir().join(format!("rr3-profile-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(dir.join("profiles")).expect("建 profiles 目录");
        let root = Root { dir };
        root.stage_servo_module();
        root
    }

    /// stage rr3-pa 的 servo 件装配清单（五键 demands，与仓内 servo 件同形状）：
    /// 门 c（⑨-c）起装载链对 kind=power 角色执法预算与覆盖判据，GOOD 夹具的
    /// act.axis.pwm pin 绑 rr3-pa——无 stage 件则所有 boot 用例被 ⑨ 覆盖判据误红。
    /// 反例用例经覆写本文件构造（空申报 / 追加 oled 件）。
    fn stage_servo_module(&self) {
        let a = self.dir.join("modules/rr3-pa/atoms/servo/assemble.toml");
        std::fs::create_dir_all(a.parent().expect("必有父目录")).expect("建 atoms 目录");
        // 件装配清单单一真值（testdata/servo_assemble.toml，与 e2e_act /
        // domain_write 测试区同一底稿）
        std::fs::write(&a, include_str!("../testdata/servo_assemble.toml"))
            .expect("stage servo assemble.toml");
    }

    /// 覆写 stage 件申报面为"未申报"（⑨ 覆盖判据红例的构造器）
    fn stage_servo_undeclared(&self) {
        let a = self.dir.join("modules/rr3-pa/atoms/servo/assemble.toml");
        std::fs::write(
            &a,
            "schema_version = 2\n\n[assemble]\nhandles = [\n  { trait = \"Rr3PwmServo\", desc_key = \"pwm_chip\" },\n]\nops = []\n",
        )
        .expect("覆写 servo assemble.toml");
    }

    /// 追加一件显示件（Rr3Display/spi_device，rail5v 700 critical）——"舵机
    /// 堵转叠加满亮屏超 2A"预算红例与组收窄绿例的构造器（§9 N2 判据列）；
    /// 无对应角色 pin，实例数走归因地板 1（宁超算不漏算）；group 传 TOML
    /// 数组字面量（`[]` = 不分组最坏口径，`["axis"]` = 组互斥申报）
    fn stage_display_atom(&self, name: &str, group: &str) {
        let dir = self.dir.join("modules/rr3-pa/atoms").join(name);
        std::fs::create_dir_all(&dir).expect("建显示件目录");
        std::fs::write(
            dir.join("assemble.toml"),
            concat!(
                "schema_version = 2\n\n[assemble]\n",
                "handles = [\n  { trait = \"Rr3Display\", desc_key = \"spi_device\" },\n]\n",
                "ops = []\n",
                "demands = [\n  { rail = \"rail5v\", peak_ma = 700, sustained_ma = 120, critical = true, concurrency_group = ",
            )
            .to_string()
                + group
                + " },\n]\n",
        )
        .expect("stage 显示件 assemble.toml");
    }

    /// 追加一件内存申报件（校验⑩ 分子面用例构造器）：空注入面（no_injection
    /// 显式）+ 合成内存申报。诚实注记（防 L-24 假绿指控）：申报义务未触发
    /// ——板侧任务映射未建模归 0.0.7 批G/批D1，本批红例用合成申报验判据
    /// 实体与装载拒链，非真实件账。
    fn stage_memory_atom(&self, name: &str, stack: i64, heap: i64) {
        let dir = self.dir.join("modules/rr3-pa/atoms").join(name);
        std::fs::create_dir_all(&dir).expect("建内存申报件目录");
        std::fs::write(
            dir.join("assemble.toml"),
            format!(
                "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\ntask_stack_bytes = {stack}\nheap_peak_bytes = {heap}\n"
            ),
        )
        .expect("stage 内存申报件 assemble.toml");
    }

    /// 落装配单并同批 stage 接线档（`hardware/<形态>/wiring.toml`，与
    /// [`crate::wiring::rr3_wiring_path`] 推导同一布局）——装载期
    /// 校验⑦要求装机档必在，缺省给 GOOD_WIRING，反例随后覆写。
    fn write_profile(&self, name: &str, text: &str) -> PathBuf {
        let p = self.dir.join("profiles").join(name);
        std::fs::write(&p, text).expect("写装配单");
        self.write_wiring(&p, GOOD_WIRING);
        p
    }

    /// 落一份接线档到 `<root>/hardware/<装配单 stem>/wiring.toml`。
    fn write_wiring(&self, profile: &Path, text: &str) {
        let stem = profile
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .expect("夹具装配单须有文件名");
        let w = self.dir.join("hardware").join(stem).join("wiring.toml");
        std::fs::create_dir_all(w.parent().expect("接线档必有父目录")).expect("建 hardware 目录");
        std::fs::write(&w, text).expect("写接线档");
    }

    /// 落一份轴档案到 `<root>/hardware/<装配单 stem>/axis/<stem>.toml`（件二
    /// 装载器落位；批N3 件三执行轴状态投影的用例夹具，与 write_wiring 同一
    /// 形态目录推导）。
    fn write_axis(&self, profile: &Path, stem: &str, text: &str) {
        let shape = profile
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .expect("夹具装配单须有文件名");
        let a = self
            .dir
            .join("hardware")
            .join(shape)
            .join("axis")
            .join(format!("{stem}.toml"));
        std::fs::create_dir_all(a.parent().expect("轴档案必有父目录")).expect("建 axis 目录");
        std::fs::write(&a, text).expect("写轴档案");
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 测试本地帮手：真契约三册（slots/roles/devices）+ 由装配单路径推导的接线档
/// （`hardware/<形态>/wiring.toml`）——与 daemon `--profile` 正门同一条推导路，
/// 顺带把路径推导钉进每条用例。平台表目录同样走编译期真值（rr-registry 常量，
/// 与真仓 contracts/platform 同一册——夹具不复制契约，真值同源纪律）。
fn boot_inputs(p: &Path) -> Result<Rr3ProfileBoot, String> {
    let wiring = crate::wiring::rr3_wiring_path(p).expect("夹具装配单必须能推导接线档");
    rr3_boot_inputs(
        p,
        Path::new(RR3_CONTRACT_SLOTS_PATH),
        Path::new(RR3_CONTRACT_ROLES_PATH),
        Path::new(RR3_CONTRACT_DEVICES_PATH),
        &wiring,
        Path::new(rr_registry::RR3_CONTRACT_PLATFORM_PATH),
    )
}

/// 同 [`boot_inputs`]，探测件注入版（MC-4 门禁用例）。
fn boot_inputs_with_probe(
    p: &Path,
    probe: &dyn crate::probe::Rr3Probe,
) -> Result<Rr3ProfileBoot, String> {
    let wiring = crate::wiring::rr3_wiring_path(p).expect("夹具装配单必须能推导接线档");
    rr3_boot_with_probe(
        p,
        Path::new(RR3_CONTRACT_SLOTS_PATH),
        Path::new(RR3_CONTRACT_ROLES_PATH),
        Path::new(RR3_CONTRACT_DEVICES_PATH),
        &wiring,
        Path::new(rr_registry::RR3_CONTRACT_PLATFORM_PATH),
        probe,
    )
}

/// 平台事实表真值点（校验③④⑤⑧的判据前件，esp32 真形态实例——与
/// rr-registry 平台单测同一 include 路径，真值同源不复制）。
fn platform_esp32_text() -> &'static str {
    include_str!("../../../../../contracts/platform/esp32.toml")
}

/// 平台文本注入版帮手（施工图 Task 4 Interfaces）：platform 文本落 Root 临时
/// 目录的 `platform/<chip>.toml`——与 daemon 的 contracts/platform/ 选册同布局，
/// chip 固定 esp32（本组用例的夹具装配单都声明 chip = "esp32"）。
fn rr3_profile_from_text_with_platform(
    tag: &str,
    profile_text: &str,
    platform_text: &str,
) -> Result<Rr3ProfileBoot, String> {
    let root = Root::new(tag);
    let p = root.write_profile("plat.toml", profile_text);
    let dir = root.dir.join("platform");
    std::fs::create_dir_all(&dir).expect("建 platform 目录");
    std::fs::write(dir.join("esp32.toml"), platform_text).expect("写平台表");
    boot_inputs_in(&root, &p, &dir)
}

/// [`boot_inputs`] 的平台目录显式版（上面两个帮手的共同实现）。
fn boot_inputs_in(root: &Root, p: &Path, platform_dir: &Path) -> Result<Rr3ProfileBoot, String> {
    let _ = root; // Root 只为临时目录的生命周期在场（Drop 清树）
    let wiring = crate::wiring::rr3_wiring_path(p).expect("夹具装配单必须能推导接线档");
    rr3_boot_inputs(
        p,
        Path::new(RR3_CONTRACT_SLOTS_PATH),
        Path::new(RR3_CONTRACT_ROLES_PATH),
        Path::new(RR3_CONTRACT_DEVICES_PATH),
        &wiring,
        platform_dir,
    )
}

/// 校验⑤⑧判据的 esp32 夹具底稿（rtos 档真形态）：LEDC 轴 + ADC 直读电表——
/// 电表 pin（controller = adc1、physical = [34]）是 batt 域的模拟测点
/// （sense 按 pin_id 引用，⑥-d 同一语法），34 ∈ esp32 adc_pins ⇒ 全绿基线；
/// 反例经定点替换构造。rtos 档不派生（host 档派生器不识别板侧词形）。
const ESP32_GOOD: &str = concat!(
    r#"
schema_version = 5

[runtime]
log_ring_cap = 512
hal_backend = "sim"

[platform]
tier = "rtos"
chip = "esp32"

[supplies]
watchdog_present = true
watchdog_timeout_ms = 15000
watchdog_feeder = "rr-core"
tick_period_ms = 200
tick_jitter_ms = 100
safety_core_interval_ms = 1000

"#,
    include_str!("../testdata/domain_batt_meter.toml"),
    include_str!("../testdata/domain_rail5v.toml"),
    r#"
# [supplies.budget]（schema 4，校验⑩ 分母）：栈预算 = 2× 现役 16KB 任务 + 余量
# （board.rs THREAD_STACK/MIC_THREAD_STACK 同源）；堆三键 = ESP32-S3 数据表推算
# 的演练实例，对账归 0.0.7 真板。heap_reserved_bytes 与 profiles/esp32-mini.toml
# 同账同值（批N4 件二 N-13 ②：16384 UART2_TX_RING + 65536 audio_q 峰值 + 512 渲染
# + 16384 板侧配置服务预留 = 98816——演练形状随真值面同批，防夹具与真值分叉）。
[supplies.budget]
not_applicable = false
task_stack_budget_bytes = 65536
heap_total_bytes = 327680
heap_reserved_bytes = 98816

"#,
    include_str!("../testdata/mod3_bindings_enabled.toml"),
    r#"
[slots]
present = ["act.servo", "cognition.llm", "power.charger"]

[[pin]]
role = "act.axis.pwm"
instance = "pan"
controller = "ledc0"
channel = 0
physical = [32]
direction = "out"
rail = "rail5v"
failsafe = "release"
mux_group = []

[[pin]]
role = "power.battery.meter"
controller = "adc1"
addr = "none"
physical = [34]
direction = "in"
mux_group = []
"#,
);

/// G4 装配段扫描直测（ADR-0019 修订 1 ③）：缺文件/坏形/注入缺逐条点名，
/// 无 atoms 目录与合法空数组不产生缺口
#[test]
fn assemble_scan_names_missing_broken_and_injection_gaps() {
    let root = Root::new("assemble-scan");
    // 无 atoms/ 目录 → 空（夹具树/两件式形态）
    assert!(rr3_assemble_scan(&root.dir.join("no-atoms")).1.is_empty());

    // 合法空声明（v2，明写不需注入面）→ 空
    let ok = root.dir.join("ok");
    std::fs::create_dir_all(ok.join("atoms/led")).unwrap();
    std::fs::write(
        ok.join("atoms/led/assemble.toml"),
        "schema_version = 2\n\n[assemble]\nhandles = []\nops = []\nno_injection = true\n",
    )
    .unwrap();
    assert!(rr3_assemble_scan(&ok).1.is_empty(), "合法空声明不得报缺");

    // 缺文件 / 坏形 / 注入缺 三路点名，互不遮蔽
    let bad = root.dir.join("bad");
    std::fs::create_dir_all(bad.join("atoms/nofile")).unwrap();
    std::fs::create_dir_all(bad.join("atoms/broken")).unwrap();
    // 只缺 ops 一键 → 恰一条点名（schema=1 那种坏法会连出两条，不便数总数）
    std::fs::write(
        bad.join("atoms/broken/assemble.toml"),
        "schema_version = 2\n\n[assemble]\nhandles = []\n",
    )
    .unwrap();
    std::fs::create_dir_all(bad.join("atoms/gap")).unwrap();
    std::fs::write(
        bad.join("atoms/gap/assemble.toml"),
        "schema_version = 2\n\n[assemble]\nhandles = []\nops = [ { slot = \"s\", src_key = \"x.y.z\" } ]\n",
    )
    .unwrap();
    std::fs::write(bad.join("config.toml"), "[x.y]\nw = 1\n").unwrap();
    let out = rr3_assemble_scan(&bad).1;
    assert_eq!(out.len(), 3, "{out:?}");
    assert!(
        out.iter()
            .any(|m| m.contains("nofile") && m.contains("缺 assemble.toml")),
        "{out:?}"
    );
    assert!(
        out.iter()
            .any(|m| m.contains("broken") && m.contains("坏形")),
        "{out:?}"
    );
    assert!(
        out.iter()
            .any(|m| m.contains("gap") && m.contains("注入缺") && m.contains("x.y.z")),
        "{out:?}"
    );
}
