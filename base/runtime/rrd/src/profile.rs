//! 装配单装载（ADR-0012 裁定 3/5②，0.0.2 方案 D2 / Q3 裁定）。
//!
//! `profiles/<机型>.toml` 是**纯数据**装配单，schema_version=3 起按增补轮六 §4.2
//! 定形为八段：`[os]`（运行期不读）+ `[platform]`（tier 三档 host/rtos/bare）+
//! `[supplies]`（看门狗三键 + 时序预算位三键，修订 4 ② / 修订 2 ③）+ `[slots]`
//! （present = 本板物理在位集）+ `[bindings]` 格位→件、`[enabled_slots]` 模块→本形态
//! 启用的格位集合、`[hardware]` 非引脚键（`alsa_device`/`link_ifname` 两键直给；
//! `spi_device` 批N3 件六起同进派生轨——敞口 18 行收口）、`[[pin]]` 物理绑定（增补轮六
//! §4.2：**引脚侧唯一真值点**——八物理键
//! `pwm_chip`/`pwm_channel`/`gpio_chip`/`gpio_led_line`/`gpio_relay_line`/`i2c_bus`/
//! `ina219_addr`/`spi_device` 自 [hardware] 下移，role 外键 → `contracts/roles.toml`，host 档由
//! 派生器合成注入文本，rtos/bare 档板侧消费归 0.0.7 批G/批D1）。
//! schema_version=4 起新增 `[supplies.budget]` 内存预算子段（批N2 n2c-v10，校验⑩
//! 分母——ADR-0021 修订 3：栈/堆预算不住 platform 表，住 [supplies].budget，
//! 三份数据三个主人）。
//! 本模块只做这些事，全部发生在 boot 之前（裁定 4：boot 前消费一次，
//! 运行期不重排，F⊗F 非法）：
//! 1. 解析装配单文本（fail-closed：形状不合逐条点名，一条不静默放行）——
//!    schema_version=2 起 [platform]/[supplies]/[slots] 三段必填；
//! 2. 查册门（裁定 5②）：bindings/enabled_slots/[slots].present 引用的每个格位必须命中
//!    [`contracts/slots.toml`](../../../contracts/slots.toml)（present 另拒基础层格位——
//!    基础层恒在位，进 present 属自相矛盾）——查册真值是
//!    `rr-registry::Rr3SlotTable`，与 CI 全集层共用同一份，不写第二套；
//! 3. 缺格判定（三源合一，方案 §15）：[slots].present 声明缺（第一源）、enabled_slots
//!    声明的格位若非基础层、又没有 binding 指向本模块（注入缺，j5）、或探测不到
//!    （实测缺，见 [`crate::probe`]）→ (模块, 格位) 进缺格清单，由 daemon 点名 `拒绝启动`——
//!    绝不带病起，provides 仍留在可满足性宇宙（下游不连坐，ADR-0009 同款语义）；
//! 4. 档位一致性（ADR-0018 修订 1：无看门狗的板不允许归 bare 档）：tier=bare 而
//!    [supplies].watchdog_present=false 的装配单自相矛盾 → 拒 boot；
//! 5. 装载期校验①②（增补轮六 §5）：`[[pin]]` 的 role 外键查册（roles.toml）、
//!    per_instance/instance 规则、requires 字段留空即红、所需角色齐（②的后果是
//!    "降"——`role_missing` 点名清单随 boot 带出，Degraded 可见面归批N4 I-15）；
//! 6. 门 n 注入路径唯一（§8）：[hardware] 出现物理键即红（稳态判据），[pins] 与
//!    [hardware] 并存时错误信息点名两份取值；
//! 7. 把 enabled_slots 模块名解析成 manifest 路径：`<modules-root>/profiles/<机>.toml` 的上两级即
//!    modules-root，manifest 在 `<modules-root>/modules/<名>/module.toml`——装机布局由
//!    打包步骤保证（modules/README.md 装机约定 + 0.0.2 T5 deb 打包同一布局）；
//! 8. 器件词表与接线档装载（增补轮六 §4.1b/§4.3）：`contracts/devices.toml` 的
//!    mate 集 + `hardware/<形态>/wiring.toml`（路径由 [`crate::wiring::
//!    rr3_wiring_path`] 按装配单推导）——校验⑦-a/b/d 的跨档判据在
//!    boot 链合龙（⑦-c 待件 demands 数据面，批N2），接线档形状与闭合面在
//!    [`crate::wiring`] 裁；
//! 9. 装载期校验⑨⑩（批N2 本体）：装配扫描一次两用——⑨ 电流预算与覆盖判据
//!    （门 c）+ ⑩ 内存预算判据（[`rr3_memory_budget`]，分子 = [assemble]
//!    内存申报件级直加，分母 = [supplies.budget]），超限即拒 boot（§5 后果 = 拒）。
//!
//! 基础层格位（`net.online` / `power.battery`）**恒在位**（slots.toml 2026-10-03 注记）：
//! 不进 bindings、不进缺格判定，enabled_slots 里声明它们只是"本形态用到"的记账。
//! base 层格的**器件缺席**（板上根本没装电池这类）是另一件事：D3 裁定（decisions.md
//! 裁定 5）给 `power.battery` 加"无器件"合法态——`[slots].device_absent` 登记
//! 格名 → 非空理由，校验⑥认它；`present` 仍是本板物理在位格位声明，与
//! `device_absent` 语义正交（base 格不许进 present，缺席表达只有这一条语法）。
//! `[os].route` 运行期不读（ADR-0012 裁定 3 B3 注记），解析时无视该段。

mod boot;
mod model;
mod parse;

pub(crate) use boot::{
    rr3_assemble_scan, rr3_axis_files_load, rr3_axis_state_text, rr3_bindings_text,
    rr3_demand_budget, rr3_demand_expand, rr3_memory_budget, rr3_power_gaps,
};
pub use boot::{
    rr3_boot_inputs, rr3_boot_with_probe, rr3_domain_apply, rr3_evidence_check, rr3_key_line_apply,
    rr3_truth_locations, rr3_truth_set, Rr3RuntimeTruthSet, Rr3TruthSetLocation,
    RR3_CONTRACT_BOOKS,
};
#[cfg(test)]
pub(crate) use model::RR3_DOMAIN_KEYS;
pub use model::{
    Rr3Budget, Rr3Domain, Rr3PinEntry, Rr3Profile, Rr3ProfileBoot, Rr3Supplies,
    RR3_CONTRACT_DEVICES_PATH, RR3_CONTRACT_ROLES_PATH, RR3_CONTRACT_SLOTS_PATH,
};
pub(crate) use model::{RR3_PIN_DERIVED_KEYS, RR3_PIN_KEYS};
pub use parse::{rr3_profile_abs, rr3_profile_parse};

#[cfg(test)]
mod tests {
    use super::*;

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
        let distro = rr3_profile_parse(include_str!("../../../../profiles/distro.toml"))
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
        let rtos = rr3_profile_parse(include_str!("../../../../profiles/esp32-mini.toml"))
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
        let distro = rr3_profile_parse(include_str!("../../../../profiles/distro.toml"))
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

    /// 校验⑨ 判据实体反例组（批N2 首件基-36 立，门 c ⑨-c 补并发口径，判据进 ①b）：
    /// 正例（servo 五键申报形状 Σ700 ≤ cap 2000 ⇒ 绿）＋反例——同域加总超 cap ⇒ 红、
    /// rail 不在 domains ⇒ 红、critical=false 不进 Σ；并发口径三态——显式空组 =
    /// 全体同时（最坏缺省）、恰一组归属 = 组内互斥取 max（收窄须是数据，F-12）、
    /// 多组归属 = 不收窄（宁超算不漏算）。
    #[test]
    fn demand_budget_check_nine() {
        let rail5v = || Rr3Domain {
            name: "rail5v".into(),
            current_cap_ma: 2000,
            ..Default::default()
        };
        let d = |rail: &str, peak: i64, critical: bool, groups: &[&str]| Rr3AssembleDemand {
            rail: rail.into(),
            peak_ma: peak,
            sustained_ma: peak.clamp(1, 120),
            critical,
            concurrency_group: groups.iter().map(|g| g.to_string()).collect(),
        };

        // 正例：servo 五键申报形状（rail5v peak 700 sustained 120 critical，空组）⇒ 无红
        let ok = vec![(
            "rr3-act/servo".to_string(),
            vec![d("rail5v", 700, true, &[])],
        )];
        assert!(
            rr3_demand_budget(&ok, &[rail5v()]).is_empty(),
            "Σ700 ≤ cap 2000 必须绿"
        );

        // 反例①：两件同域加总超 cap（Σ2400 > 2000）⇒ 红点名域 + 最坏值 + cap + 申报者
        let over = vec![
            (
                "rr3-act/servo".to_string(),
                vec![d("rail5v", 700, true, &[])],
            ),
            (
                "rr3-x/dummy".to_string(),
                vec![d("rail5v", 1700, true, &[])],
            ),
        ];
        let errs = rr3_demand_budget(&over, &[rail5v()]);
        assert!(
            errs.iter().any(|e| e.contains("电流预算超限")
                && e.contains("rail5v")
                && e.contains("2400")
                && e.contains("2000")
                && e.contains("rr3-act/servo")
                && e.contains("rr3-x/dummy")),
            "{errs:?}"
        );

        // 反例②：rail 不在 domains ⇒ 红（域缺即 cap 缺，预算无分母）
        let ghost = vec![("rr3-x/dummy".to_string(), vec![d("rail9v", 1, true, &[])])];
        let errs = rr3_demand_budget(&ghost, &[rail5v()]);
        assert!(
            errs.iter()
                .any(|e| e.contains("rail9v") && e.contains("不在 [[supplies.domain]]")),
            "{errs:?}"
        );

        // 反例③：critical=false 不进 Σ（判据只算 critical=true）——5000 mA 也绿
        let noncrit = vec![(
            "rr3-x/dummy".to_string(),
            vec![d("rail5v", 5000, false, &[])],
        )];
        assert!(
            rr3_demand_budget(&noncrit, &[rail5v()]).is_empty(),
            "critical=false 不得进加总"
        );

        // 并发口径（F-12：不重叠必须是数据）——恰一组归属 = 组内互斥取 max：
        // 三件各 700 同挂 "axis" 组 ⇒ 最坏 = max(700) = 700 ≤ 2000 绿（不加总 2100）
        let grouped = vec![
            (
                "rr3-a/servo".to_string(),
                vec![d("rail5v", 700, true, &["axis"])],
            ),
            (
                "rr3-b/servo".to_string(),
                vec![d("rail5v", 700, true, &["axis"])],
            ),
            (
                "rr3-c/servo".to_string(),
                vec![d("rail5v", 700, true, &["axis"])],
            ),
        ];
        assert!(
            rr3_demand_budget(&grouped, &[rail5v()]).is_empty(),
            "同组互斥按 max 计（700 ≤ 2000）必须绿"
        );
        // 多组归属 = 不收窄（宁超算不漏算）：条目同时挂两组 ⇒ 全额进 Σ
        let multi = vec![
            (
                "rr3-a/x".to_string(),
                vec![d("rail5v", 700, true, &["g1", "g2"])],
            ),
            ("rr3-b/y".to_string(), vec![d("rail5v", 700, true, &["g1"])]),
            ("rr3-c/z".to_string(), vec![d("rail5v", 700, true, &["g2"])]),
        ];
        let errs = rr3_demand_budget(&multi, &[rail5v()]);
        assert!(
            errs.iter().any(|e| e.contains("2100")),
            "多组归属不收窄，全额加总 2100 > 2000 ⇒ 红 {errs:?}"
        );
    }

    /// 全部规则走真的 contracts/slots.toml 与 contracts/roles.toml（查册真值同源），
    /// 夹具落 pid 唯一临时目录
    struct Root {
        dir: PathBuf,
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
        include_str!("testdata/domain_batt_none.toml"),
        include_str!("testdata/domain_rail5v.toml"),
        r#"
# [supplies.budget]（schema 4，校验⑩ 分母）：host 档合法不适用必须显式写，不许留空
[supplies.budget]
not_applicable = true

"#,
        include_str!("testdata/mod3_bindings_enabled.toml"),
        r#"
[slots]
present = ["act.servo", "cognition.llm", "power.charger"]

"#,
        include_str!("testdata/pin_servo_pan.toml"),
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
    const GOOD_WIRING: &str = include_str!("testdata/wiring_bottom.toml");

    impl Root {
        fn new(tag: &str) -> Root {
            let dir =
                std::env::temp_dir().join(format!("rr3-profile-{}-{tag}", std::process::id()));
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
            std::fs::write(&a, include_str!("testdata/servo_assemble.toml"))
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
            std::fs::create_dir_all(w.parent().expect("接线档必有父目录"))
                .expect("建 hardware 目录");
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
        include_str!("../../../../contracts/platform/esp32.toml")
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
    fn boot_inputs_in(
        root: &Root,
        p: &Path,
        platform_dir: &Path,
    ) -> Result<Rr3ProfileBoot, String> {
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
        include_str!("testdata/domain_batt_meter.toml"),
        include_str!("testdata/domain_rail5v.toml"),
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
        include_str!("testdata/mod3_bindings_enabled.toml"),
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

    /// 门 c 装载拒链·红例①（§5 ⑨ 后果 = 拒：扩算后 Σ critical > cap ⇒ boot Err）：
    /// GOOD 单轴（servo 700，角色 pin 归因实例 1）+ 两件显示件（oled/face 各 700，
    /// 无对应角色 pin 走归因地板 1，空组最坏口径）⇒ 2100 > 2000，红条点名域、
    /// 最坏值与申报者——"舵机堵转叠加满亮屏"这一 ADR-0021 裁定 2 立项红例在
    /// 装载链可达（多件同域叠加逐件全额计）。
    #[test]
    fn demand_budget_chain_boot_rejects_over_cap() {
        let root = Root::new("budget-red");
        root.stage_display_atom("face", "[]");
        root.stage_display_atom("oled", "[]");
        let p = root.write_profile("budget-red.toml", GOOD);
        let e = boot_inputs(&p).expect_err("扩算 2100 > cap 2000 必须致命");
        assert!(
            e.contains("电流预算超限")
                && e.contains("rail5v")
                && e.contains("2100")
                && e.contains("2000")
                && e.contains("rr3-pa/servo")
                && e.contains("rr3-pa/oled")
                && e.contains("rr3-pa/face"),
            "{e}"
        );
    }

    /// 门 c 装载拒链·红例②（§5 ⑨ 缺失语义：件未申报 demands 而角色 kind=power ⇒ 红）：
    /// 覆写 servo 为无申报——act.axis.pwm（全仓唯一 kind=power 角色）挂轨 rail5v 而
    /// rr3-pa 申报面无 rail5v 需求，红条点名 pin、模块与判据原文。
    #[test]
    fn demand_budget_chain_boot_rejects_uncovered_power_pin() {
        let root = Root::new("budget-undeclared");
        root.stage_servo_undeclared();
        let p = root.write_profile("undeclared.toml", GOOD);
        let e = boot_inputs(&p).expect_err("kind=power pin 无申报覆盖必须致命");
        assert!(
            e.contains("件未申报 demands") && e.contains("rail5v") && e.contains("rr3-pa"),
            "{e}"
        );
    }

    /// 门 c 装载拒链·绿例（F-12：收窄必须是数据）：同一红例布局（servo+oled+face），
    /// 两件显示件申报同挂 "axis" 组 ⇒ 组内互斥取 max = 700，Σ = 700（servo 未分组
    /// 全额）+ 700 = 1400 ≤ 2000，boot 过——与红例①对照钉住"同布局、只多一条申报
    /// 数据，红转绿"，收窄是申报者自证声明，判据只认数据不验物理。
    #[test]
    fn demand_budget_chain_boot_accepts_group_narrowing() {
        let root = Root::new("budget-grouped");
        root.stage_display_atom("face", "[\"axis\"]");
        root.stage_display_atom("oled", "[\"axis\"]");
        let p = root.write_profile("budget-grouped.toml", GOOD);
        let boot = boot_inputs(&p).expect("组收窄 1400 ≤ 2000 必须过");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
    }

    /// 校验⑩ 判据实体反例组（n2c-v10）：not_applicable 绿（host 显式不适用）、
    /// 边界等号绿（Σ = 预算不算超）、栈超红 ⑩-a、堆超红 ⑩-b——F-13 形状
    /// （堆值不计栈）下只红 ⑩-b 不级联 ⑩-a，两条红文各点名判据号与两侧数值。
    #[test]
    fn memory_budget_check_entity_green_and_red() {
        let budget = Rr3Budget {
            not_applicable: false,
            task_stack_budget_bytes: 65536,
            heap_total_bytes: 327680,
            heap_reserved_bytes: 16384,
        };
        // host 显式不适用 ⇒ 绿（多大都绿——判据关闭是显式的）
        let host = Rr3Budget {
            not_applicable: true,
            ..Rr3Budget::default()
        };
        assert!(rr3_memory_budget(i64::MAX, i64::MAX, &host, "host").is_empty());
        // 边界等号 ⇒ 绿（≤ 判，> 才红）；栈绿堆不超
        assert!(rr3_memory_budget(65536, 311296, &budget, "rtos").is_empty());
        // ⑩-a 栈超红：点名判据号、两侧数值与档位
        let e = rr3_memory_budget(65537, 0, &budget, "rtos");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(
            e[0].contains("校验⑩-a")
                && e[0].contains("65537")
                && e[0].contains("65536")
                && e[0].contains("rtos"),
            "{e:?}"
        );
        // ⑩-b 堆超红（F-13 形状：栈值不超 ⇒ 栈绿，只红堆）：可用堆 = 327680 − 16384
        let e = rr3_memory_budget(0, 311297, &budget, "rtos");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(
            e[0].contains("校验⑩-b")
                && e[0].contains("311297")
                && e[0].contains("311296")
                && e[0].contains("16384"),
            "{e:?}"
        );
        // 双超各一条，互不遮蔽
        let e = rr3_memory_budget(70000, 400000, &budget, "rtos");
        assert_eq!(e.len(), 2, "{e:?}");
    }

    /// [supplies.budget] 分档自洽反例组（parse 层，ADR-0021 修订 3 / F-6 留空反例）：
    /// host 档 not_applicable 缺失或写 false ⇒ 红；host 档并存数字键 ⇒ 红；
    /// rtos 档 not_applicable = true ⇒ 红；rtos 档缺三数键 ⇒ 红；段缺 ⇒ 红
    /// （DoD 3：每个缺失语义有留空反例）。
    #[test]
    fn budget_tier_consistency_errors_are_named() {
        // host 档 not_applicable 写 false（合法不适用不显式写 = 留空即红）
        let bad = GOOD.replace("not_applicable = true", "not_applicable = false");
        let errs = rr3_profile_parse(&bad).expect_err("host 档不写不适用必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("分档自洽校验失败") && e.contains("not_applicable = false")),
            "{errs:?}"
        );

        // host 档并存数字键（与 not_applicable = true 自相矛盾）
        let bad = GOOD.replace(
            "[supplies.budget]\nnot_applicable = true",
            "[supplies.budget]\nnot_applicable = true\ntask_stack_budget_bytes = 65536",
        );
        let errs = rr3_profile_parse(&bad).expect_err("host 档并存数字键必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("并存数字预算键") && e.contains("task_stack_budget_bytes")),
            "{errs:?}"
        );

        // rtos 档 not_applicable = true（有真栈/堆预算却写不适用）
        let bad = ESP32_GOOD.replace("not_applicable = false", "not_applicable = true");
        let errs = rr3_profile_parse(&bad).expect_err("rtos 档写不适用必须红");
        assert!(
            errs.iter().any(|e| e.contains("tier = rtos/bare 但")),
            "{errs:?}"
        );

        // rtos 档缺三数键（留空反例：必填键缺失逐条点名）
        let bad = ESP32_GOOD
            .replace("task_stack_budget_bytes = 65536\n", "")
            .replace("heap_total_bytes = 327680\n", "")
            .replace("heap_reserved_bytes = 98816\n", "");
        let errs = rr3_profile_parse(&bad).expect_err("rtos 档缺数键必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("task_stack_budget_bytes 缺失")),
            "{errs:?}"
        );
        assert!(errs.iter().any(|e| e.contains("heap_total_bytes 缺失")));
        assert!(errs.iter().any(|e| e.contains("heap_reserved_bytes 缺失")));

        // 段缺（schema 4 起必填，不许整个子段留空）
        let bad = GOOD.replace("[supplies.budget]\nnot_applicable = true\n\n", "");
        let errs = rr3_profile_parse(&bad).expect_err("缺 [supplies.budget] 必须红");
        assert!(
            errs.iter().any(|e| e.contains("缺少 [supplies.budget]")),
            "{errs:?}"
        );

        // 未知键点名（词表封闭）
        let bad = GOOD.replace(
            "[supplies.budget]\nnot_applicable = true",
            "[supplies.budget]\nnot_applicable = true\nstack_budget = 1",
        );
        let errs = rr3_profile_parse(&bad).expect_err("budget 未知键必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("[supplies.budget] 未知键 stack_budget")),
            "{errs:?}"
        );

        // rtos 档物理矛盾：total ≤ reserved（可用堆 ≤ 0，⑩-b 无分母）
        let bad = ESP32_GOOD.replace("heap_total_bytes = 327680", "heap_total_bytes = 8192");
        let errs = rr3_profile_parse(&bad).expect_err("total ≤ reserved 必须红");
        assert!(
            errs.iter()
                .any(|e| e.contains("可用堆 = total − reserved ≤ 0")),
            "{errs:?}"
        );
    }

    /// 门 c 装载拒链·校验⑩ 红例①（§5 ⑩-a 后果 = 拒）：ESP32_GOOD（rtos 档，
    /// task_stack_budget_bytes = 65536）+ 合成内存申报件 98304 B ⇒ boot Err 点名
    /// 判据号与两侧数值（申报义务未触发，红例用合成数据——见 stage_memory_atom
    /// 诚实注记）。
    #[test]
    fn memory_budget_chain_boot_rejects_stack_over_budget() {
        let root = Root::new("mem-stack-red");
        root.stage_memory_atom("stackhog", 98304, 0);
        let p = root.write_profile("mem-stack-red.toml", ESP32_GOOD);
        let e = boot_inputs(&p).expect_err("Σ栈 98304 > 65536 必须致命");
        assert!(
            e.contains("校验⑩-a") && e.contains("98304") && e.contains("65536"),
            "{e}"
        );
    }

    /// 门 c 装载拒链·校验⑩ 红例②（F-13 形状：堆值不计栈 ⇒ 栈绿堆红）：栈申报
    /// 16384 B 在预算内（⑩-a 绿），堆申报 327680 B > 可用堆 228864 B（327680 −
    /// 98816，N-13 ② 全账后）⇒ 只红 ⑩-b——只加总栈会击穿半边的反例在装载链可达。
    #[test]
    fn memory_budget_chain_boot_rejects_heap_over_budget_while_stack_green() {
        let root = Root::new("mem-heap-red");
        root.stage_memory_atom("stacklegit", 16384, 0);
        root.stage_memory_atom("heaphog", 0, 327680);
        let p = root.write_profile("mem-heap-red.toml", ESP32_GOOD);
        let e = boot_inputs(&p).expect_err("Σ堆 327680 > 可用 228864 必须致命");
        assert!(e.contains("校验⑩-b") && e.contains("327680"), "{e}");
        assert!(!e.contains("校验⑩-a"), "栈在预算内不得级联红：{e}");
    }

    /// 门 c 装载拒链·校验⑩ 绿例：Σ栈 = 预算（等号不算超）且堆在可用堆内 ⇒ boot 过
    /// ——与红例①对照钉住边界语义（≤ 判，> 才红）。
    #[test]
    fn memory_budget_chain_boot_accepts_boundary_sums() {
        let root = Root::new("mem-green");
        root.stage_memory_atom("stacklegit", 65536, 228864);
        let p = root.write_profile("mem-green.toml", ESP32_GOOD);
        let boot = boot_inputs(&p).expect("等号边界必须过");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
    }

    /// 件级直加语义钉子（非每实例扩算，assemble.rs 头注同源）：同一内存申报件
    /// 在多实例角色下不扩算——GOOD（host）换 ESP32_GOOD 载体不便，此处以
    /// ESP32_GOOD 单轴（实例 1）+ 双件同值申报对照红例①直接钉数值（98304 =
    /// 件级两件直加），扩算语义由 rr3_demand_expand 的 ⑨ 用例反向对照。
    #[test]
    fn memory_budget_sums_are_per_atom_not_per_instance() {
        let root = Root::new("mem-peratom");
        root.stage_memory_atom("stacka", 49152, 0);
        root.stage_memory_atom("stackb", 49152, 0);
        let p = root.write_profile("mem-peratom.toml", ESP32_GOOD);
        let e = boot_inputs(&p).expect_err("两件直加 98304 > 65536 必须致命");
        assert!(e.contains("Σ task_stack_bytes = 98304"), "{e}");
    }

    #[test]
    fn good_profile_parses() {
        let p = rr3_profile_parse(GOOD).expect("应能解析");
        assert_eq!(p.bindings.len(), 3);
        assert_eq!(p.enabled_slots.len(), 3);
        assert_eq!(p.enabled_slots["rr3-pa"], vec!["act.servo".to_string()]);
        // 描述符八段（schema_version=4）：[platform]/[supplies]/[slots]/[[pin]] 逐字段收齐
        assert_eq!(p.platform_tier, "host");
        // 平台事实表外键（§4.6，校验③④⑤⑧判据前件选册）
        assert_eq!(p.platform_chip, "rk3566");
        assert!(p.supplies.watchdog_present);
        assert_eq!(p.supplies.watchdog_timeout_ms, 15000);
        assert_eq!(p.supplies.watchdog_feeder, "rrd");
        assert_eq!(p.supplies.tick_period_ms, 200);
        assert_eq!(p.supplies.tick_jitter_ms, 100);
        assert_eq!(p.supplies.safety_core_interval_ms, 1000);
        // [supplies.budget]（schema 4，校验⑩ 分母）：host 档显式不适用态收齐
        assert!(p.supplies.budget.not_applicable);
        assert_eq!(p.supplies.budget.task_stack_budget_bytes, 0);
        assert_eq!(p.supplies.budget.heap_total_bytes, 0);
        assert_eq!(p.supplies.budget.heap_reserved_bytes, 0);
        assert_eq!(
            p.slots_present,
            ["act.servo", "cognition.llm", "power.charger"]
        );
        assert!(
            p.slots_device_absent.is_empty(),
            "GOOD 无缺席登记 → 空 map（D3 合法态是可选键）"
        );
        // [[pin]]：逐字段收齐，pin_id 按 per_instance 语法拼装（device 批N3 件八进场，
        // GOOD 的 servo pin 无 I2C 面不填 = 空串；env pin 满配语义全字段收齐）
        assert_eq!(p.pins.len(), 2, "{:?}", p.pins);
        let pin = &p.pins[0];
        assert_eq!(pin.rr3_pin_id(), "act.axis.pwm.pan");
        assert_eq!(pin.controller, "pwm0");
        assert_eq!(pin.channel, Some(0));
        assert_eq!(pin.physical, vec![12]);
        assert_eq!(pin.direction, "out");
        assert_eq!(pin.rail, "rail5v");
        assert_eq!(pin.failsafe, "release");
        assert!(pin.mux_group.is_empty());
        assert_eq!(pin.device, "", "GOOD servo pin 无器件面，device = 空串");
        // 第二条 pin：power.charger.meter（I2C 满配，device 键非空）
        let env = &p.pins[1];
        assert_eq!(env.role, "power.charger.meter");
        assert_eq!(env.controller, "i2c1");
        assert_eq!(env.addr, "0x70");
        assert_eq!(env.device, "INA219");
        assert_eq!(env.direction, "in");
        // [os].route 被无视（运行期不读），但它的存在不构成错误
    }

    #[test]
    fn parse_errors_are_all_collected_and_named() {
        let text = GOOD
            .replace("schema_version = 5", "schema_version = 9")
            .replace("\"act.servo\" = \"rr3-pa\"", "\"act.servo\" = \"\"")
            .replace("rr3-pb = [\"power.charger\"]", "rr3-pb = [\"\"]")
            .replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = 3");
        let errs = rr3_profile_parse(&text).expect_err("形状不合必须报错");
        assert_eq!(errs.len(), 4, "{errs:?}");
        assert!(
            errs.iter().any(|e| e.contains("schema_version")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("bindings.act.servo")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("enabled_slots.rr3-pb 的每一项")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("enabled_slots.rr3-pc 应为字符串数组")),
            "{errs:?}"
        );
    }

    #[test]
    fn missing_sections_and_empty_enabled_slots_are_errors() {
        let errs = rr3_profile_parse("schema_version = 5\n").expect_err("缺段必须报错");
        assert!(
            errs.iter().any(|e| e.contains("缺少 [bindings]")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("缺少 [enabled_slots]")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("缺少 [runtime]")),
            "[runtime] 必填段缺也必须点名（schema 5 起，批N5 件二 F-30）: {errs:?}"
        );
        // [supplies] 段缺时 [supplies.budget] 的点名不级联（budget 错误只在
        // supplies 段在场时裁）——budget 段缺的留空反例归
        // budget_tier_consistency_errors_are_named。

        let errs = rr3_profile_parse("schema_version = 5\n[bindings]\n\n[enabled_slots]\n")
            .expect_err("空 enabled_slots 必须报错");
        assert!(
            errs.iter().any(|e| e.contains("[enabled_slots] 为空")),
            "{errs:?}"
        );
    }

    /// [runtime].log_ring_cap 判据（批N5 件二 F-30"装载期缺即红"）：键缺/段缺/零/负/
    /// 类型错逐条点名；合法值如实收进结构体（换成 64 仍解析成功——证明容量是
    /// 装配单数据不是解析器写死）。
    #[test]
    fn runtime_log_ring_cap_missing_or_nonpositive_is_red_and_value_flows() {
        // 段缺（GOOD 定点删除）
        let errs = rr3_profile_parse(&GOOD.replace("[runtime]\nlog_ring_cap = 512\n", ""))
            .expect_err("缺 [runtime] 必须报错");
        assert!(
            errs.iter().any(|e| e.contains("缺少 [runtime]")),
            "{errs:?}"
        );
        // 键缺
        let errs = rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", ""))
            .expect_err("缺 log_ring_cap 必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("runtime.log_ring_cap 缺失或不是整数")),
            "{errs:?}"
        );
        // 0 与负数（正整数判据）
        for bad in ["log_ring_cap = 0", "log_ring_cap = -8"] {
            let errs = rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", bad))
                .expect_err("非正容量必须报错");
            assert!(
                errs.iter().any(|e| e.contains("须为正整数")),
                "{bad} 必须点名正整数判据: {errs:?}"
            );
        }
        // 类型错（字符串不是整数）
        let errs =
            rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", "log_ring_cap = \"many\""))
                .expect_err("字符串容量必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("runtime.log_ring_cap 缺失或不是整数")),
            "{errs:?}"
        );
        // 未知键点名
        let errs =
            rr3_profile_parse(&GOOD.replace("[runtime]", "[runtime]\nlog_level = \"debug\""))
                .expect_err("未知键必须报错");
        assert!(
            errs.iter()
                .any(|e| e.contains("[runtime] 未知键 log_level")),
            "{errs:?}"
        );
        // 合法值如实收进结构体（64 ≠ 编译期兜底 512——数据流证明）
        let p = rr3_profile_parse(&GOOD.replace("log_ring_cap = 512", "log_ring_cap = 64"))
            .expect("64 是合法容量");
        assert_eq!(p.log_ring_cap, 64, "容量必须来自装配单数据");
        assert_eq!(
            rr3_profile_parse(GOOD).expect("GOOD 合法").log_ring_cap,
            512
        );
    }

    #[test]
    fn unparsable_toml_is_single_error() {
        let errs = rr3_profile_parse("[bindings\n").expect_err("必须报错");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].starts_with("装配单 TOML 解析失败"), "{errs:?}");
    }

    /// 满配：manifest 路径按 <root>/modules/<名>/module.toml 解析，缺格为空
    #[test]
    fn full_profile_resolves_manifests_and_finds_no_missing() {
        let root = Root::new("full");
        let p = root.write_profile("full.toml", GOOD);
        let boot = boot_inputs(&p).expect("满配装配单应通过");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
        assert_eq!(boot.manifests.len(), 3);
        // 比对基准必须是**规范化后**的根目录：boot_inputs 拿到的 manifest 路径派生自
        // canonicalize 过的装配单路径（macOS 的 /tmp 是 /private/tmp 的符号链接），
        // 拿夹具的原始路径比对就会红在系统布局上，不是红在被测行为。
        let modules_root = std::fs::canonicalize(&root.dir)
            .expect("夹具根目录必须可规范化")
            .display()
            .to_string();
        for (i, name) in ["rr3-pa", "rr3-pb", "rr3-pc"].iter().enumerate() {
            assert_eq!(
                boot.manifests[i],
                Path::new(&modules_root)
                    .join("modules")
                    .join(name)
                    .join("module.toml"),
                "manifest 路径必须按 modules-root 解析且升序"
            );
        }
    }

    /// 轴档案文本帮手（十四键，值面参照件二 §4.4 示例档）：pin/calibrated/hard_stop
    /// 三键参数化——pin 走 GOOD 的 pin_id 语法（外键对账 F-4），状态两键即执行轴
    /// 投影的输入面。
    fn axis_text(pin: &str, calibrated: bool, hard_stop: &str) -> String {
        axis_text_range(pin, calibrated, hard_stop, 60, 120)
    }

    /// 行程可变的投影输入面（件五双轴用例：pan/tilt 各给一行程，注入文本可辨源）。
    fn axis_text_range(pin: &str, calibrated: bool, hard_stop: &str, lo: u16, hi: u16) -> String {
        format!(
            "schema = 1\npin = \"{pin}\"\ncenter_deg = 90\nlo_deg = {lo}\nhi_deg = {hi}\ndir_sign = 1\nslew_deg_s = 100\nsettle_ms = 200\nhold = \"release\"\ncurrent_ceil_ma = 750\nwarmup_ms = 0\nhard_stop = \"{hard_stop}\"\ncalibrated = {calibrated}\nsource = \"manual\"\n"
        )
    }

    /// 双轴装配单（ESP32_GOOD + tilt pin）：rtos 档不派生 [hardware]，双 pin 可在册
    /// （host 档派生键同值校验会把双轴拒在门外——host 模块实例是单轴单描述符，模型
    /// 自洽；板侧双轴真形貌在 rtos 档，板侧描述符消费归 0.0.7）。channel/physical 与
    /// pan 错开（③-a/③-b 双占各守一维）。件五消歧用例的输入面。
    fn esp32_good_tilt() -> String {
        format!(
            "{ESP32_GOOD}\n[[pin]]\nrole = \"act.axis.pwm\"\ninstance = \"tilt\"\n\
             controller = \"ledc0\"\nchannel = 1\nphysical = [33]\ndirection = \"out\"\n\
             rail = \"rail5v\"\nfailsafe = \"release\"\nmux_group = []\n"
        )
    }

    /// 执行轴状态投影·恰一份（批N3 件三建，件五扩形）：合法档案装载成功 → 注入
    /// 文本按 `calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>` 投影，
    /// 点名清单干净——rr3-act 拒动判据与行程裁决的输入面就是这五键的档案真值。
    #[test]
    fn axis_state_projects_when_exactly_one_profile_loads() {
        let root = Root::new("axis-one");
        let p = root.write_profile("full.toml", GOOD);
        root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
        let boot = boot_inputs(&p).expect("恰一份合法档案不改变 boot 绿面");
        assert_eq!(
            boot.axis_state, "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
            "恰一份合法档案必须投影执行轴状态（rr3-act 拒动判据的输入面）"
        );
        assert!(boot.axis_notes.is_empty(), "{:?}", boot.axis_notes);
    }

    /// 执行轴状态投影·坏件（批N3 件三）：档案形状错 → 注入文本空串 = 不注入
    /// （模块侧折保守缺省，§6 复位语义④），点名清单逐条入账且**不拒 boot**——
    /// 档案装载是降级面，与缺格/缺角色同档。
    #[test]
    fn axis_state_stays_empty_and_notes_broken_profile() {
        let root = Root::new("axis-broken");
        let p = root.write_profile("full.toml", GOOD);
        root.write_axis(&p, "pan", "schema = 1\npin = \"act.axis.pwm.pan\"\n");
        let boot = boot_inputs(&p).expect("坏档案不拒 boot（降级面，与缺格同档）");
        assert!(
            boot.axis_state.is_empty(),
            "坏档案不得投影注入文本——缺真相按最窄面起跑"
        );
        assert!(
            boot.axis_notes
                .iter()
                .any(|n| n.contains("pan.toml") && n.contains("坏形")),
            "坏件必须点名到件: {:?}",
            boot.axis_notes
        );
    }

    /// 执行轴状态投影·同 pin 副本（批N3 件三建，件五收口）：两份合法档案同一 pin
    /// → 注入文本空串并点名副本——同 pin 副本不是消歧对象是病灶（恰一份合法才投影
    /// 的 boot 链防线；写侧同款拒面在 axis_write 件四，读侧点名指清理）。
    #[test]
    fn axis_state_stays_empty_and_notes_duplicate_pin_profiles() {
        let root = Root::new("axis-two");
        let p = root.write_profile("full.toml", GOOD);
        root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
        root.write_axis(&p, "pan-copy", &axis_text("act.axis.pwm.pan", true, "yes"));
        let boot = boot_inputs(&p).expect("两份合法档案不改变 boot 绿面");
        assert!(
            boot.axis_state.is_empty(),
            "同 pin 副本不得投影注入文本——恰一份合法才投影"
        );
        assert!(
            boot.axis_notes
                .iter()
                .any(|n| n.contains("2 份合法档案（副本）") && n.contains("先清理副本")),
            "同 pin 副本必须点名病灶: {:?}",
            boot.axis_notes
        );
    }

    /// 执行轴状态投影·双轴消歧（批N3 件五，收"多轴未消歧"挂账）：pan + tilt 两份
    /// 合法档案，轴读数 = pan（env 缺省同源）→ 只投影 pan 的行程真值（五键文本
    /// 可辨源：pan [60,120] ≠ tilt [80,100]）；点名清单干净——注入的行程必须属于
    /// 被驱动的那根轴。
    #[test]
    fn axis_state_projects_driven_axis_when_two_axes_present() {
        let root = Root::new("axis-dual");
        let p = root.write_profile("dual.toml", &esp32_good_tilt());
        root.write_axis(&p, "pan", &axis_text("act.axis.pwm.pan", true, "yes"));
        root.write_axis(
            &p,
            "tilt",
            &axis_text_range("act.axis.pwm.tilt", true, "yes", 80, 100),
        );
        let boot = boot_inputs(&p).expect("双轴两份合法档案不改变 boot 绿面");
        assert_eq!(
            boot.axis_state, "calibrated=true,hard_stop=yes,lo=60,hi=120,center=90",
            "双轴在场只投影被驱动轴（env 缺省 pan）的行程真值"
        );
        assert!(boot.axis_notes.is_empty(), "{:?}", boot.axis_notes);
    }

    /// 执行轴状态投影·轴读数与档案 instance 错配（批N3 件五）：装配单只挂 tilt
    /// pin、档案只有 tilt，轴读数 = 缺省 pan → 单份也对轴，"注入 tilt 行程、驱动
    /// pan 轴"是错配，宁可空串折保守缺省并点名。
    #[test]
    fn axis_state_stays_empty_when_axis_env_misses_every_instance() {
        let text = GOOD.replace("instance = \"pan\"", "instance = \"tilt\"");
        let root = Root::new("axis-mismatch");
        let p = root.write_profile("tilt-only.toml", &text);
        root.write_axis(
            &p,
            "tilt",
            &axis_text_range("act.axis.pwm.tilt", true, "yes", 80, 100),
        );
        let boot = boot_inputs(&p).expect("轴错配不改变 boot 绿面（降级面）");
        assert!(
            boot.axis_state.is_empty(),
            "轴读数对不上任何 instance 不得投影——错配行程比无行程更危险"
        );
        assert!(
            boot.axis_notes
                .iter()
                .any(|n| n.contains("无一 instance = \"pan\"")),
            "错配必须点名轴读数与 instance 集: {:?}",
            boot.axis_notes
        );
    }

    /// MC-4 门禁用例①：注入非 linux 生产探针（有硬件面的格位恒缺）→ 这些格位进缺格
    /// 清单，且**点名到件与键**（(模块, 格位) 成对，模块不错配到别的格）——这条钉的
    /// 是"缺格 → Degraded 点名件与键"的检出真值（探测拿不到答案不当作在位的下游）。
    /// 无硬件面可反证的格位（cognition.llm）不在缺格清单：那是"没有反证"不是"拿不到答案"。
    #[test]
    fn injected_absent_probe_names_missing_module_and_slot() {
        let root = Root::new("probe-absent");
        let p = root.write_profile("absent.toml", GOOD);
        let boot = boot_inputs_with_probe(&p, &crate::probe::Rr3AbsentProbe)
            .expect("缺格不是致命项，boot 输入必须成功返回");
        assert_eq!(
            boot.missing,
            vec![
                ("rr3-pa".to_string(), "act.servo".to_string()),
                ("rr3-pb".to_string(), "power.charger".to_string()),
            ],
            "有硬件面的格位必须点名件与键、升序去重（软件格 cognition.llm 不在内）: {:?}",
            boot.missing
        );
    }

    /// MC-4 门禁用例②：探测件只对一格报缺 → 只有那一格的点名模块进缺格，
    /// 其余同在位——证明缺格判定确实**逐格读探测结果**，不是整单连坐。
    #[test]
    fn injected_selective_probe_marks_only_its_slot() {
        struct OnlyServoAbsent;
        impl crate::probe::Rr3Probe for OnlyServoAbsent {
            fn rr3_probe_slot(&self, slot: &str) -> bool {
                slot != "act.servo"
            }
        }
        let root = Root::new("probe-selective");
        let p = root.write_profile("selective.toml", GOOD);
        let boot = boot_inputs_with_probe(&p, &OnlyServoAbsent).expect("部分缺格不致命");
        assert_eq!(
            boot.missing,
            vec![("rr3-pa".to_string(), "act.servo".to_string())],
            "只有探到缺的那一格点名件与键: {:?}",
            boot.missing
        );
    }

    /// 缺格：bindings 移除任一格 → (模块, 格位) 进缺格清单，其余不受牵连
    #[test]
    fn removed_binding_is_missing_for_its_module_only() {
        let root = Root::new("missing");
        let p = root.write_profile(
            "missing.toml",
            &GOOD.replace("\"power.charger\" = \"rr3-pb\"\n", ""),
        );
        let boot = boot_inputs(&p).expect("缺格不是致命项");
        assert_eq!(
            boot.missing,
            vec![("rr3-pb".to_string(), "power.charger".to_string())],
            "只有点名模块缺格"
        );
        assert_eq!(
            boot.manifests.len(),
            3,
            "缺格模块的 manifest 仍在解析结果里（供日志点名）"
        );
    }

    /// enabled_slots 声明了格位但 binding 指向别的模块：对点名模块算缺格，同时构成"binding 未被认领"
    /// 的结构不自洽 → 致命（fail-closed，装配单各说一套必须当场听见）
    #[test]
    fn binding_claimed_by_another_module_is_fatal() {
        let root = Root::new("cross");
        let text = GOOD.replace(
            "\"power.charger\" = \"rr3-pb\"",
            "\"power.charger\" = \"rr3-pc\"",
        );
        let p = root.write_profile("cross.toml", &text);
        let e = boot_inputs(&p).expect_err("binding 被他人认领必须致命");
        assert!(e.contains("未被 enabled_slots 认领"), "{e}");
    }

    /// 基础层格位进 bindings = 违反"基础层由基座内建承载"（slots.toml 注记）→ 致命
    #[test]
    fn base_layer_binding_is_fatal() {
        let root = Root::new("base");
        let text = GOOD.replace("[bindings]\n", "[bindings]\n\"bus.publish\" = \"rr3-pa\"\n");
        let p = root.write_profile("base.toml", &text);
        let e = boot_inputs(&p).expect_err("基础层 binding 必须致命");
        assert!(e.contains("bindings 不得绑定基础层格位 bus.publish"), "{e}");
    }

    /// 查册门：enabled_slots/bindings 引用未在册格位 → 致命并点名（真契约上跑）
    #[test]
    fn unknown_slot_is_fatal_and_named() {
        let root = Root::new("unknown");
        let text = GOOD.replace("rr3-pc = [\"cognition.llm\"]", "rr3-pc = [\"sensor.wifi\"]");
        let p = root.write_profile("unknown.toml", &text);
        let e = boot_inputs(&p).expect_err("未在册格位必须致命");
        assert!(
            e.contains("enabled_slots 模块 rr3-pc 的格位 sensor.wifi 未在册"),
            "{e}"
        );
    }

    /// 基础层格位进 enabled_slots 只记账、不算缺格（power.battery 恒在位，default 机同款形状）
    #[test]
    fn base_layer_enabled_slot_is_never_missing() {
        let root = Root::new("baseslot");
        let text = GOOD.replace(
            "rr3-pc = [\"cognition.llm\"]",
            "rr3-pc = [\"cognition.llm\", \"power.battery\"]",
        );
        let p = root.write_profile("baseslot.toml", &text);
        let boot = boot_inputs(&p).expect("基础层 enabled_slots 格位不缺格");
        assert!(boot.missing.is_empty(), "{:?}", boot.missing);
    }

    /// [hardware] 段（schema 3 只余非引脚键）：字符串/整数标量收、整数字符串化、
    /// 缺段可选；坏类型逐条点名。物理键的"出现即红"在 boot 链裁（门 n），解析层
    /// 不带契约真值不越权。
    #[test]
    fn hardware_section_is_optional_scalars_stringified() {
        // 缺段：合法（纯 sim/无硬件格形态），hal_bindings 为空文本
        let p = rr3_profile_parse(GOOD).expect("缺 [hardware] 段必须合法");
        assert!(p.hardware.is_empty(), "{:?}", p.hardware);

        // 字符串 + 整数混合：整数按十进制字符串化（[hardware] 不设键白名单，
        // 键位语义归 rr-hal 描述符解析，rrd 只管装配形状）
        let text =
            format!("{GOOD}\n[hardware]\nspi_device = \"/dev/spidev0.0\"\nlink_metric = 100\n");
        let p = rr3_profile_parse(&text).expect("标量 [hardware] 必须合法");
        assert_eq!(
            p.hardware.get("spi_device").map(String::as_str),
            Some("/dev/spidev0.0")
        );
        assert_eq!(
            p.hardware.get("link_metric").map(String::as_str),
            Some("100")
        );

        // 布尔/浮点/数组/表逐条点名，一次全收集
        let text = format!(
            "{GOOD}\n[hardware]\nflag = true\nratio = 0.5\nlines = [4]\nsub = {{ k = 1 }}\n"
        );
        let errs = rr3_profile_parse(&text).expect_err("坏类型必须报错");
        assert_eq!(errs.len(), 4, "{errs:?}");
        for key in ["flag", "ratio", "lines", "sub"] {
            assert!(
                errs.iter().any(|e| e.contains(&format!("hardware.{key} "))),
                "{errs:?}"
            );
        }

        // 空键点名
        let text = format!("{GOOD}\n[hardware]\n\"\" = 1\n");
        let errs = rr3_profile_parse(&text).expect_err("空键必须报错");
        assert!(errs.iter().any(|e| e.contains("键不得为空串")), "{errs:?}");
    }

    /// [hardware] → 注入文本：BTreeMap 升序确定性、整数已是字符串形状、空表空串。
    #[test]
    fn hardware_bindings_text_is_sorted_and_escaped() {
        assert_eq!(rr3_bindings_text(&BTreeMap::new()), "", "空表 → 空串");
        let mut hw = BTreeMap::new();
        hw.insert("spi_device".to_string(), "0".to_string());
        hw.insert("link_ifname".to_string(), "wl\"x\\".to_string());
        let text = rr3_bindings_text(&hw);
        // BTreeMap 升序：link_ifname < spi_device
        assert_eq!(
            text, "link_ifname = \"wl\\\"x\\\\\"\nspi_device = \"0\"\n",
            "升序 + 转义（双引号/反斜杠）"
        );
    }

    /// 派生器正例（host 档）：[pins] 派生视图 ∪ [hardware] 非引脚键合成注入文本——
    /// 取值与旧 [hardware] 逐字节对齐（pwm<N> 派生 pwm_chip=N、行号取 physical[0]、
    /// addr 原样），键名不变，rr-hal descriptor.rs 解析端零改动；四角色全派生 +
    /// 多条 pin 共键同值（led/relay 同 gpiochip0）不误报。
    #[test]
    fn boot_inputs_derives_and_merges_hal_bindings() {
        let root = Root::new("derive");
        let text = format!(
            "{GOOD}\n[hardware]\nalsa_device = \"default\"\nlink_ifname = \"wlan0\"\n\
             [[pin]]\nrole = \"act.led.gpio\"\ncontroller = \"gpiochip0\"\nphysical = [4]\n\
             direction = \"out\"\nrail = \"none\"\nfailsafe = \"hold\"\nmux_group = []\n\
             [[pin]]\nrole = \"act.relay.gpio\"\ncontroller = \"gpiochip0\"\nphysical = [17]\n\
             direction = \"out\"\nrail = \"none\"\nfailsafe = \"driven_low\"\nmux_group = []\n\
             [[pin]]\nrole = \"power.battery.meter\"\ncontroller = \"i2c1\"\naddr = \"0x40\"\n\
             direction = \"in\"\nmux_group = []\n"
        );
        let p = root.write_profile("derive.toml", &text);
        let boot = boot_inputs(&p).expect("满配装配单应通过");
        assert_eq!(
            boot.hal_bindings,
            "alsa_device = \"default\"\n\
             gpio_chip = \"0\"\n\
             gpio_led_line = \"4\"\n\
             gpio_relay_line = \"17\"\n\
             i2c_bus = \"1\"\n\
             ina219_addr = \"0x40\"\n\
             link_ifname = \"wlan0\"\n\
             pwm_channel = \"0\"\n\
             pwm_chip = \"0\"\n",
            "派生视图与非引脚键按 BTreeMap 升序合并，取值与旧 [hardware] 一致"
        );
        assert!(boot.role_missing.is_empty(), "{:?}", boot.role_missing);
    }

    /// SPI 词形派生正例（批N3 件六，敞口 18 行收口）：spidev<N> 词形（总线号）+
    /// channel（片选号）合成 /dev/spidev<N>.<M>——与链④真件档旧 [hardware] 直给值
    /// 逐字节对齐（profiles/distro.toml 迁移前 = "/dev/spidev0.0"），注入文本形状不变。
    #[test]
    fn boot_inputs_derives_spi_device_wordform() {
        let root = Root::new("derive-spi");
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"ui.face.spi\"\ncontroller = \"spidev0\"\nchannel = 0\n\
             direction = \"out\"\nmux_group = []\n"
        );
        let p = root.write_profile("derive-spi.toml", &text);
        let boot = boot_inputs(&p).expect("SPI 显示 pin 应通过装载");
        assert_eq!(
            boot.hal_bindings,
            "pwm_channel = \"0\"\n\
             pwm_chip = \"0\"\n\
             spi_device = \"/dev/spidev0.0\"\n",
            "spidev0 + channel 0 派生 /dev/spidev0.0，与旧 [hardware] 直给值逐字节对齐"
        );
    }

    /// 校验①-a：role 未在 roles.toml 在册 → 致命点名（真契约上跑）
    #[test]
    fn pin_unknown_role_is_fatal() {
        let root = Root::new("pin-role");
        let text = GOOD.replace("role = \"act.axis.pwm\"", "role = \"act.axis.magical\"");
        let p = root.write_profile("pin-role.toml", &text);
        let e = boot_inputs(&p).expect_err("自造角色必须致命");
        assert!(e.contains("角色 act.axis.magical 未在册"), "{e}");
    }

    /// 校验①-b per_instance 两侧：true 角色缺 instance / false 角色填 instance 都红
    #[test]
    fn pin_per_instance_rules_are_fatal_both_sides() {
        let root = Root::new("pin-perinst");
        // act.axis.pwm per_instance=true，删 instance → 必填缺
        let text = GOOD.replace("instance = \"pan\"\n", "");
        let p = root.write_profile("perinst-missing.toml", &text);
        let e = boot_inputs(&p).expect_err("per_instance=true 缺 instance 必须致命");
        assert!(e.contains("instance 必填"), "{e}");

        // act.led.gpio per_instance=false，填 instance → 禁填
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"act.led.gpio\"\ninstance = \"x\"\n\
             controller = \"gpiochip0\"\nphysical = [4]\ndirection = \"out\"\nfailsafe = \"hold\"\n"
        );
        let p = root.write_profile("perinst-extra.toml", &text);
        let e = boot_inputs(&p).expect_err("per_instance=false 填 instance 必须致命");
        assert!(e.contains("instance 禁填"), "{e}");
    }

    /// 校验①-b 留空反例（§5 缺失语义）：requires 必填字段删键即红——channel 以
    /// None = 缺、failsafe 以空串 = 缺，一次收集、逐条点名，一条不静默放行
    #[test]
    fn pin_requires_left_blank_is_fatal() {
        let root = Root::new("pin-blank");
        let text = GOOD
            .replace("channel = 0\n", "")
            .replace("failsafe = \"release\"\n", "");
        let p = root.write_profile("pin-blank.toml", &text);
        let e = boot_inputs(&p).expect_err("requires 字段留空必须致命");
        assert!(e.contains("缺 channel"), "{e}");
        assert!(e.contains("缺 failsafe"), "{e}");
    }

    /// 校验① direction 相等：pin.direction 与角色 direction 各说一套 → 致命点名
    #[test]
    fn pin_direction_mismatch_is_fatal() {
        let root = Root::new("pin-direction");
        let text = GOOD.replace("direction = \"out\"", "direction = \"in\"");
        let p = root.write_profile("pin-direction.toml", &text);
        let e = boot_inputs(&p).expect_err("direction 与角色不一致必须致命");
        assert!(
            e.contains("direction = in 与角色 act.axis.pwm direction = out 不一致"),
            "{e}"
        );
    }

    /// [[pin]] 形状：未知键/坏类型/缺身份键逐条点名，一次收集完
    #[test]
    fn pin_section_shape_errors_are_named() {
        let text = format!(
            "{GOOD}\n[[pin]]\nrole = \"\"\ndirection = \"out\"\nwat = 1\nphysical = [\"x\"]\n"
        );
        // GOOD 现役两条 pin，追加的坏 pin 序号 = #2
        let errs = rr3_profile_parse(&text).expect_err("形状不合必须报错");
        assert!(
            errs.iter().any(|e| e.contains("pin #2.role 缺失或为空")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("pin #2 未知键 wat")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("pin #2.physical 的每一项都必须是整数")),
            "{errs:?}"
        );
        // 缺 role / 缺 direction（身份键缺席，解析期即红）
        let errs = rr3_profile_parse(format!("{GOOD}\n[[pin]]\ncontroller = \"pwm1\"\n").as_str())
            .expect_err("缺 role/direction 必须报错");
        assert!(
            errs.iter().any(|e| e.contains("pin #2 缺 role")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("pin #2 缺 direction")),
            "{errs:?}"
        );
    }

    /// 校验⑥-a/⑥-b 反例（§18 F-5 的自造域名 + 功率面挂 none）：未在册 rail 与
    /// kind=power 而 rail=none 都必须拒 boot 并点名
    #[test]
    fn pin_rail_domain_gate_is_fatal() {
        let root = Root::new("rail-gate");
        // ⑥-a：自造域名（F-5：rail24 不在任何一处在册集——文档示例值会被装机档抄）
        let text = GOOD.replace("rail = \"rail5v\"", "rail = \"rail24\"");
        let p = root.write_profile("rail-unknown.toml", &text);
        let e = boot_inputs(&p).expect_err("自造域名必须致命");
        assert!(
            e.contains("rail = rail24 未在 [[supplies.domain]] 在册"),
            "{e}"
        );

        // ⑥-b：kind=power 角色挂 none（act.axis.pwm 是 power，none 是 signal 侧显式值）
        let text = GOOD.replace("rail = \"rail5v\"", "rail = \"none\"");
        let p = root.write_profile("rail-none-power.toml", &text);
        let e = boot_inputs(&p).expect_err("功率面挂 none 必须致命");
        assert!(e.contains("kind = power"), "{e}");
    }

    /// 校验⑥-c 反例：armed=true 而 sense=none / max_age_ms=0——未标定即不武装，
    /// 无测点/无龄期上界的域不得武装拒动（§3.1 降级语义 + §19 Y-4）。
    /// GOOD 的 batt 域恰是 sense=none + max_age_ms=0 的最小反例，armed 一开即双红。
    #[test]
    fn domain_armed_without_sense_is_fatal() {
        let root = Root::new("armed-gate");
        let text = GOOD.replace(
            "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "switchable = false\narmed = true\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("armed.toml", &text);
        let e = boot_inputs(&p).expect_err("武装而无测点/龄期必须致命");
        assert!(e.contains("校验⑥-c"), "{e}");
    }

    /// 校验⑥-d 反例：域绑定位写第二套语法（F-2 的 adc:gp34）——不在 pin_id 集
    /// 即红，"一律写 pin_id，不再各造语法"
    #[test]
    fn domain_binding_uses_pin_id_not_adhoc_syntax() {
        let root = Root::new("pid-gate");
        let text = GOOD.replace(
            "sense = \"none\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "sense = \"adc:gp34\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("pid.toml", &text);
        let e = boot_inputs(&p).expect_err("第二套语法必须致命");
        assert!(e.contains("不在 [pins] 的 pin_id 集"), "{e}");
        assert!(e.contains("sense = adc:gp34"), "{e}");
    }

    /// 校验⑥-e 形状反例一（批N21 件二）：armed=true 而 evidence = none——武装
    /// 拒动必须引用一条真板实测证据（规则住 conventions §6.1），"none" 即未
    /// 标定，须 armed = false。
    #[test]
    fn domain_armed_without_evidence_is_fatal() {
        let root = Root::new("evid-none-gate");
        let text = GOOD.replace(
            "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "switchable = false\narmed = true\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("evid-none.toml", &text);
        let e = boot_inputs(&p).expect_err("武装而无证据引用必须致命");
        assert!(e.contains("校验⑥-e"), "{e}");
        assert!(e.contains("evidence = none"), "{e}");
    }

    /// 校验⑥-e 形状反例二（批N21 件二）：引用形制坏（三位编号）——
    /// `<形态>/<四位编号>` 是唯一词形（机制真值 rr-ipc::evidence），装载即拒。
    #[test]
    fn domain_armed_with_malformed_evidence_ref_is_fatal() {
        let root = Root::new("evid-form-gate");
        let text = GOOD.replace(
            "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "switchable = false\narmed = true\nevidence = \"实测/1\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("evid-form.toml", &text);
        let e = boot_inputs(&p).expect_err("形制坏的引用必须致命");
        assert!(e.contains("形制不合法"), "{e}");
    }

    /// 校验⑥-e 成对反例三（批N21 件二）：armed=false 却携引用——证据位与
    /// 武装位成对（同 switch/switchable 范式），未武装显式写 "none"，不留过期引用。
    #[test]
    fn domain_disarmed_with_evidence_ref_is_fatal() {
        let root = Root::new("evid-pair-gate");
        let text = GOOD.replace(
            "switchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "switchable = false\narmed = false\nevidence = \"实测/0001\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("evid-pair.toml", &text);
        let e = boot_inputs(&p).expect_err("未武装携证据必须致命");
        assert!(e.contains("未武装不携证据"), "{e}");
    }

    /// 校验⑥-e 文件半边（批N21 件二，boot 装载链持有容器根调用）：armed=true 的
    /// 合法形制引用，证据文件缺席即红（fail-closed，容器缺不静默放行）；同一
    /// 装配单落一份六键齐且 form 与目录一致的证据文件后 boot 过＝绿例——D11
    /// 界面门的装载期半边全链打通。绿例顺带钉死 ⑥-c 的武装前提（有测点＋龄期
    /// 上界 > 0），证据门不孤立于武装门。
    #[test]
    fn armed_domain_evidence_file_check_gate() {
        let root = Root::new("evid-file-gate");
        let text = GOOD.replace(
            "sense = \"none\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = false\nevidence = \"none\"\nmax_age_ms = 0\n\n[[supplies.domain]]\nname = \"rail5v\"",
            "sense = \"power.charger.meter\"\ncurrent_sense = \"none\"\nswitch = \"none\"\nswitchable = false\narmed = true\nevidence = \"实测/0001\"\nmax_age_ms = 30000\n\n[[supplies.domain]]\nname = \"rail5v\"",
        );
        let p = root.write_profile("evid-file.toml", &text);

        // 红例：容器缺——armed 域的引用查无文件即拒
        let e = boot_inputs(&p).expect_err("证据文件缺席必须致命");
        assert!(e.contains("校验⑥-e"), "{e}");
        assert!(e.contains("读不到"), "{e}");

        // 绿例：容器里落一份形制合法的证据文件（frontmatter 六键齐，form 与
        // 形态目录名一致）→ boot 过
        let ev = root.dir.join("docs/evidence/实测/0001.md");
        std::fs::create_dir_all(ev.parent().expect("证据路径必有父目录")).expect("建证据容器");
        std::fs::write(
            &ev,
            concat!(
                "+++\n",
                "form = \"实测\"\n",
                "slot_or_role = \"board.firmware\"\n",
                "method = \"真板实测\"\n",
                "conclusion = \"域武装依据成立\"\n",
                "date = \"2026-10-23\"\n",
                "recorder = \"lei_huang\"\n",
                "+++\n\n正文。\n",
            ),
        )
        .expect("写证据文件");
        boot_inputs(&p).expect("证据文件四道核全过后 boot 必须过");
    }

    /// 仓内证据容器锁定（批N21 件二，首批真档形制回归）：rrd 装载面消费的容器
    /// 根与仓内 `docs/evidence` 是同一处——首批两份证据必须四道核全过，真档
    /// 形制漂移即刻在本仓测试红。
    #[test]
    fn repo_evidence_container_frontline_files_verify() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/evidence");
        rr_ipc::evidence::rr3_evidence_verify(&root, "实测/0001").expect("0001 必须是合法证据");
        rr_ipc::evidence::rr3_evidence_verify(&root, "实测/0002").expect("0002 必须是合法证据");
    }

    /// [[supplies.domain]] 形状门（解析层）：缺键逐键点名、未知键点名、
    /// 缺整个域数组点名——一次收集完，运维一次改对
    #[test]
    fn domain_section_shape_errors_are_named() {
        // 缺整个域数组（schema 3 起必填）：取 GOOD 的 [supplies] 段头做最小文本
        let minimal_head = GOOD.split("[[supplies.domain]]").next().unwrap_or(GOOD);
        let errs = rr3_profile_parse(minimal_head).expect_err("缺域数组必须报错");
        assert!(
            errs.iter().any(|e| e.contains("缺少 [[supplies.domain]]")),
            "{errs:?}"
        );

        // 域条目缺键 + 未知键逐条点名
        let errs = rr3_profile_parse(&format!(
            "{minimal_head}[[supplies.domain]]\nname = \"batt\"\nnominal_mv = 3700\nghost = 1\n"
        ))
        .expect_err("缺键/未知键必须报错");
        for want in [
            "supplies.domains[0].mild_mv",
            "supplies.domains[0].severe_mv",
            "supplies.domains[0].current_cap_ma",
            "supplies.domains[0].sense",
            "supplies.domains[0].current_sense",
            "supplies.domains[0].switch",
            "supplies.domains[0].switchable",
            "supplies.domains[0].armed",
            "supplies.domains[0].evidence",
            "supplies.domains[0].max_age_ms",
            "supplies.domains[0] 未知键 ghost",
        ] {
            assert!(errs.iter().any(|e| e.contains(want)), "缺 {want}: {errs:?}");
        }
    }
}
