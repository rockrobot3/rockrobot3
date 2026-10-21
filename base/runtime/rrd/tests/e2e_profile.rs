#![cfg(unix)]
//! 装配单正门（`--profile`）的线级证据（0.0.2 方案 D2/T3，Q3 裁定；ADR-0009/0012）。
//!
//! 与 `profile.rs` 的 9 条单测分工：单测在库层钉解析/查册/缺格判定的**判定逻辑**
//! （真 contracts/slots.toml + 夹具文本，库层不 spawn）；这里起**真 rrd 子进程**走
//! 完整装配链 → plan_boot 仲裁 → supervisor spawn，钉的是"环节接起来之后线上
//! 看得见的三件事"：
//! - P1 满配：enabled_slots 决定起谁——三条 spawn 行 + ctl.status 轮询到全 running；
//! - P2 缺格：缺格模块逐条点名"拒绝启动"、不 spawn，其余模块照常（缺格≠拒 boot，
//!   ADR-0009 同款"provides 留在可满足性宇宙、下游不连坐"语义）；
//! - P3 非法装配单：查册未命中 → bind 之前 fatal，非 0 退出、套接字无残迹。
//! - P4 环容量落数据（批N5 件二 F-30）：[runtime].log_ring_cap = 4 → `rr3.ctl.logs`
//!   恰回 4 行、最早一行已丢——环容量是装配单数据不是代码常量（512 编译期写死即红）。
//! - P5 rrd 自身日志落盘（批N5 件二笔2，§17 运维留痕同轨判据）：第一世日志 append 进
//!   `<socket>.log/rrd.log`；SIGKILL 掉 rrd 后同 socket 重启第二世，启动回填文件尾
//!   进环 → `rr3.ctl.logs` 取得到**第一世独有**的收尸行（独有性是判据核心：spawn/
//!   装配单行两世同文，判不了"行来自崩溃前的文件"）。
//!
//! 三条口径（与 `e2e_spawn_restart.rs` 同源，不重复论证）：
//! 1. **tick 必传小值**（`E2E_TICK_MS`）：supervisor 的扫尸节拍复用 `--watchdog-tick-ms`，
//!    安静 tick 会让 boot 用例的收尸永不发生；
//! 2. 夹具装机布局 `<root>/profiles/<机>.toml` + `<root>/modules/<名>/module.toml`
//!    与 0.0.2 T5 deb 打包同一布局——本文件同时是"装配单按装机布局能跑通"的证据；
//! 3. role 走默认 `resident`（不传 `--boot-role`）：夹具注册后按 heartbeat 喂狗常驻，
//!    rrd 被 drop 杀掉后夹具按 EOF 自退（S5 的同一条理据，不留孤儿）。

mod support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::json;

use support::*;

/// 满配装配单（P1/P2/P3 的底稿）：三个格位各绑一件、bindings 与 enabled_slots 互认领。
/// 格位名取自真 `contracts/slots.toml`（act.servo / sensor.env / cognition.llm）——
/// 查册真值与 CI 全集层同源，夹具不得自造格位。`[os]` 段运行期不读（ADR-0012 裁定 3），
/// 放在这里恰恰是为了证明"带着它也能过"。schema_version=2 起三新段必填
///（[platform]/[supplies]/[slots]，ADR-0018 修订 1）；present 只声明 bindings 用的
/// 非 base 格位——P2 删 binding 行后 sensor.env 仍 declared present，缺格判定照走注入缺。
/// schema_version=3 起 [[pin]] 进场（增补轮六批N1）——本夹具不声明 [pins]，
/// 校验②只记 role_missing 非致命，boot 照常。schema_version=4 起 [supplies.budget]
/// 必填（批N2 校验⑩ 分母）——host 档合法不适用显式写，boot 照常。schema_version=5
/// 起 [runtime] 必填（批N5 件二 F-30）——log_ring_cap 进场，P4 换小容量钉环判据。
const GOOD: &str = r#"
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
present = ["act.servo", "sensor.env", "cognition.llm"]

[bindings]
"act.servo" = "rr3-pa"
"sensor.env" = "rr3-pb"
"cognition.llm" = "rr3-pc"

[enabled_slots]
rr3-pa = ["act.servo"]
rr3-pb = ["sensor.env"]
rr3-pc = ["cognition.llm"]
"#;

/// 模块 manifest 模板：身份（name）来自目录名，entry 占位在落盘前替换为 sim_peer
/// 构建产物的绝对路径（与 `rr3_fixture_manifest` 同一条"占位符永不上线"的纪律）。
/// publishes 留空：本文件不走叠加层，生产主题集下空 publishes 是唯一不撞
/// "未注册主题"注册门的形状；subscribes 的 rr3-runtime/module/state 是内建命名空间。
/// heartbeat(200) < watchdog(1000)：resident 按 200ms 喂狗，20ms tick 的扫描不会误判死。
const MANIFEST_TEMPLATE: &str = r#"[manifest]
schema_version = 1
name = "@NAME@"
version = "0.0.1"
type = "module"
lang = "rust"
entry = "@ENTRY@"

[events]
publishes = []
subscribes = ["rr3-runtime/module/state"]

[resources]
restart_policy = "always"
start_priority = 40

[health]
heartbeat_ms = 200
watchdog_timeout_ms = 1000
"#;

/// 接线档底稿（增补轮六 §4.3）：装载期校验⑦要求装机档必在——vcc/gnd-only
/// 单线组（net=rail5v 在装配单域在册），对 [[pin]] 增删免疫。
const WIRING: &str = r#"
[[wire]]
connector = "J-T"
hdr_pin = [1, 2]
signal = ["vcc", "gnd"]
net = "rail5v"
feeds = []
mate = "none"
keyed = false
rated_a = 1.0
gnd_common = "star-1"
"#;

/// 装机布局夹具：`<root>/profiles/<机>.toml` + `<root>/modules/<名>/module.toml`。
/// Drop 清整树——守卫口径与 `SimManifest` 同一条教训：断言 panic 后 /tmp 不许留残迹。
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new(tag: &str) -> Root {
        let dir =
            std::env::temp_dir().join(format!("rr3-profile-e2e-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); // pid 复用留下的上次残根不许干扰本次
        std::fs::create_dir_all(dir.join("profiles")).expect("建 profiles 目录");
        Root { dir }
    }

    /// 落一份装配单。三个用例各写各的变形，文件名统一 default.toml——
    /// 解析只看内容不看名字，<root>/profiles/ 这一级才是 modules-root 的定位依据。
    fn write_profile(&self, text: &str) -> PathBuf {
        let p = self.dir.join("profiles").join("default.toml");
        std::fs::write(&p, text).expect("写装配单");
        // 同批 stage 接线档（hardware/<形态>/wiring.toml，rr3_wiring_path_for_profile
        // 推导的同一布局）：装载期校验⑦要求装机档必在
        let w = self
            .dir
            .join("hardware")
            .join("default")
            .join("wiring.toml");
        std::fs::create_dir_all(w.parent().unwrap()).expect("建 hardware 目录");
        std::fs::write(&w, WIRING).expect("写接线档");
        p
    }

    /// 落一个模块目录 + manifest。**必须每个用例都落全三件**（连缺格用例的 rr3-pb）：
    /// P2 要证明的是"manifest 在、job 装了、boot 队列跳过它"——少了 manifest 就退化成
    /// 另一条"manifest 读取失败"的失败路径，钉不住缺格语义。
    fn write_module(&self, name: &str) {
        let text = MANIFEST_TEMPLATE
            .replace("@NAME@", name)
            .replace("@ENTRY@", env!("CARGO_BIN_EXE_sim_peer"));
        let dir = self.dir.join("modules").join(name);
        std::fs::create_dir_all(&dir).expect("建模块目录");
        std::fs::write(dir.join("module.toml"), text).expect("写模块 manifest");
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 一问一答拉一次 ctl.status 快照：(module, phase) 列表。
/// ctl 连接不注册也能得到应答（e2e_ctl 的口径），应答后连接即被收掉——轮询就开新连接。
/// 为什么轮询 ctl.status 而不是等 running 广播：本文件的测试连接不注册，unregistered
/// 连接收不到广播；fresh 连接一问一答是与到达序无关的唯一读法。
fn rr3_status(rrd: &RrdTest) -> Vec<(String, String)> {
    let mut c = rrd.rr3_connect();
    rr3_send(&mut c, &rr3_rpc_frame("rr3.ctl.status", 1, json!({}), 0));
    let f = rr3_read_where(&mut c, "ctl.status 应答", |f| {
        rr3_body_split(&f.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    v["result"]["modules"]
        .as_array()
        .map(|ms| {
            ms.iter()
                .map(|m| {
                    (
                        m["name"].as_str().unwrap_or_default().to_string(),
                        m["phase"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// P1 — 满配：enabled_slots 决定起谁。三个格位各有一件、bindings 与 enabled_slots 互认领 →
/// 三条 spawn 行 + ctl.status 轮询到三个全 running。
#[test]
fn full_profile_spawns_every_face_module_and_all_reach_running() {
    let root = Root::new("full");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        root.write_module(name);
    }
    let profile = root.write_profile(GOOD);
    // env 显式钉 sim 探针（MC-4：非 test 面不再默认 sim）：act.servo/sensor.env 归
    // PWM/I2C 硬件面，开发机与 CI runner 无这些 linux sysfs 设备，不显式选 sim 会被
    // fail-closed 判缺格、模块不 spawn（被测行为之外的夹具前提）。
    let mut rrd =
        RrdTest::rr3_start_profile_env("prof-full", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);

    // 装配单凭证行（横幅之后、监听行之前的 stdout 事实）：证明这条 boot 走的是正门，
    // 不是"给了 --profile 却按 --manifest 空表静默起了一个空 daemon"。
    rrd.rr3_wait_log("rrd: 装配单已加载: ");

    // 三条 spawn 行：模块名来自 enabled_slots，entry 回显夹具里的 sim_peer 绝对路径——
    // 证明"profile 解析出的 manifest 真的被装载、真按它起了进程"，不是碰巧的同名者。
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        let spawn = rrd.rr3_wait_log(&format!("spawn {name} pid="));
        assert!(
            spawn.contains(&format!("entry={}", env!("CARGO_BIN_EXE_sim_peer"))),
            "spawn 行必须回显装配单解析出的 manifest 里的绝对 entry: {spawn}"
        );
    }

    // running 的可观察面：轮询到三个模块全 running 为止。有界等待（判据是"成真"，
    // dur 只是不再等的上限，与 rr3_wait_line 同一口径）。
    let all_running = rr3_wait_until(
        || {
            let status = rr3_status(&rrd);
            ["rr3-pa", "rr3-pb", "rr3-pc"]
                .iter()
                .all(|name| status.iter().any(|(n, p)| n == name && p == "running"))
        },
        Duration::from_secs(20),
    );
    assert!(
        all_running,
        "三个 face 模块必须全部到达 running；最后一次快照: {:?}",
        rr3_status(&rrd)
    );

    // 装配快照读口（批N4 件九笔4）：--profile 正门 boot 成功 → `rr3.ctl.wiring`
    // 应答 available:true，四路底账（形态名/域/线组/角色）与 [pins]×wiring 全交叉
    // 判果同帧在线。夹具无 [[pin]]/axis 目录——零 pin 零轴是如实投影（装载面
    // role_missing 非致命），vcc/gnd 豁免导体判 not-applicable、在册域判 pass。
    let mut ctl = rrd.rr3_connect();
    rr3_send(&mut ctl, &rr3_rpc_frame("rr3.ctl.wiring", 2, json!({}), 0));
    let f = rr3_read_where(&mut ctl, "ctl.wiring 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    let w = &v["result"];
    assert_eq!(w["available"], true, "{v}");
    assert_eq!(w["profile"], "default", "形态名 = 装配单文件 stem: {v}");
    assert_eq!(w["domains"][0]["name"], "rail5v", "{v}");
    assert_eq!(w["wires"][0]["connector"], "J-T", "{v}");
    assert_eq!(w["roles"].as_array().map(Vec::len), Some(10), "{v}");
    let checks = w["checks"].as_array().expect("checks 必须在场: {v}");
    let net_pass = checks
        .iter()
        .any(|c| c["item"] == "net:J-T:rail5v" && c["pass"] == "pass");
    let gnd_na = checks
        .iter()
        .any(|c| c["item"] == "signal:J-T[1]:gnd" && c["pass"] == "not-applicable");
    assert!(net_pass && gnd_na, "全交叉判果必须在场: {checks:?}");

    // 契约完整性审计（批N5 件五笔4，Z-4；批N8 件一②扩面，基-40）：--profile 正门
    // boot 成功 → 真值集记账定格，`rr3.ctl.contracts` available:true，四行四册记账
    // （真仓契约目录，e2e 不传 --contracts）+ 七行真值集成员记账（基-40 成员名 =
    // 装载面真值集相对名）+ 五条判果"齐全且 sha 对"全 pass。
    let mut ctl2 = rrd.rr3_connect();
    rr3_send(
        &mut ctl2,
        &rr3_rpc_frame("rr3.ctl.contracts", 3, json!({}), 0),
    );
    let f = rr3_read_where(&mut ctl2, "ctl.contracts 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    let c = &v["result"];
    assert_eq!(c["available"], true, "{v}");
    let books = c["contracts"].as_array().expect("契约记账必须在场: {v}");
    assert_eq!(
        books
            .iter()
            .map(|b| b["file"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["topics.toml", "slots.toml", "roles.toml", "devices.toml"],
        "四册点名 = RR3_CONTRACT_FILES 清单序: {books:?}"
    );
    assert!(
        books
            .iter()
            .all(|b| b["sha256"].as_str().map(|s| s.len()) == Some(64)),
        "每册带 64 字符 sha256 十六进制: {books:?}"
    );
    // 基-40 成员完整性段：七成员点名（成员名 = rr3_runtime_truth_set 相对名，顺序 =
    // 成员名单点序），每成员带 64 字符 sha256——与四册账同源同值（四册成员重复记账）。
    let ts = c["truth_set"].as_array().expect("真值集记账必须在场: {v}");
    assert_eq!(
        ts.iter()
            .map(|t| t["member"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec![
            "profiles/default.toml",
            "contracts/topics.toml",
            "contracts/slots.toml",
            "contracts/roles.toml",
            "contracts/devices.toml",
            "contracts/platform/rk3566.toml",
            "hardware/default/wiring.toml",
        ],
        "七成员点名 = 装载面真值集成员序: {ts:?}"
    );
    assert!(
        ts.iter()
            .all(|t| t["sha256"].as_str().map(|s| s.len()) == Some(64)),
        "每成员带 64 字符 sha256 十六进制: {ts:?}"
    );
    let cchecks = c["checks"].as_array().expect("契约判果必须在场: {v}");
    assert_eq!(cchecks.len(), 5, "四册逐条＋truth_set 汇总: {cchecks:?}");
    assert!(
        cchecks.iter().all(|x| x["pass"] == "pass"),
        "boot 未动包内契约 → 五条判果全 pass: {cchecks:?}"
    );
}

/// P2 — 缺格：enabled_slots 声明的 sensor.env 没有指向 rr3-pb 的 binding → 逐条点名
/// "拒绝启动"、boot 队列跳过；pa/pc 照常起（缺格≠拒 boot，不连坐）。
#[test]
fn missing_slot_module_is_refused_by_name_while_others_still_boot() {
    let root = Root::new("missing");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        root.write_module(name);
    }
    // 只删 bindings 里的 sensor.env 一行：enabled_slots 的 rr3-pb 声明还在 → 缺格判定命中它。
    // （删 enabled_slots 侧则是另一条路径：binding 未被认领 = 结构不自洽 = 致命，库层单测已钉。）
    let profile = root.write_profile(&GOOD.replace("\"sensor.env\" = \"rr3-pb\"\n", ""));
    // env 显式钉 sim 探针（同 prof-full：缺格来自被删的 binding，不由探测缺引入——
    // 若默认恒缺会把 pa/pc 也一起判缺，用例语义被污染）。
    let mut rrd = RrdTest::rr3_start_profile_env(
        "prof-missing",
        &profile,
        E2E_TICK_MS,
        &[("RR3_PROBE", "sim")],
    );

    // 缺格点名在 bind 之前（装配链先于监听行），到达序与 spawn 行无关——内容判据。
    rrd.rr3_wait_log("拒绝启动 rr3-pb：缺格 sensor.env");

    // 其余两件照常走完整 boot（entry 回显的断言 P1 已钉，这里只认"起过的是谁"）。
    rrd.rr3_wait_log("spawn rr3-pa pid=");
    rrd.rr3_wait_log("spawn rr3-pc pid=");

    // 否定断言：rr3-pb 确实一个进程都没起。needle 选 `spawn rr3-pb pid=`——
    // `rr3-pb` 不是 `rr3-pa`/`rr3-pc` 的子串，不会被同伴的 spawn 行顶掉。
    rrd.rr3_assert_no_log("spawn rr3-pb pid=", Duration::from_millis(300));
}

/// P3 — 非法装配单：enabled_slots 引用未在册格位（sensor.wifi 不在 contracts/slots.toml，
/// 在册的是 net.wifi——差一个字符，恰好证明查册不是前缀匹配）→ 查册门在 bind 之前
/// 致命，非 0 退出、套接字无残迹。直接 Command 起 rrd：这一路连监听行都不会有，
/// `RrdTest` 的就绪等待根本等不到，管道收两端才是对的读法。
#[test]
fn unregistered_slot_in_enabled_slots_is_fatal_before_bind_with_no_socket_residue() {
    let root = Root::new("unknown");
    // 只往 rr3-pc 的 enabled_slots 里**加**一个未在册格位：bindings 三条仍全部被认领，
    // 致命原因单一（未在册），不混入"binding 未被认领"的第二种错误。
    let profile = root.write_profile(&GOOD.replace(
        "rr3-pc = [\"cognition.llm\"]",
        "rr3-pc = [\"cognition.llm\", \"sensor.wifi\"]",
    ));
    let socket = PathBuf::from(format!(
        "/tmp/rr3-m2-{}-prof-unknown.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket);

    let out = Command::new(env!("CARGO_BIN_EXE_rrd"))
        .arg("--socket")
        .arg(&socket)
        .arg("--profile")
        .arg(&profile)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("起 rrd 失败");

    assert!(
        !out.status.success(),
        "查册未命中必须非 0 退出；stdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("enabled_slots 模块 rr3-pc 的格位 sensor.wifi 未在册"),
        "致命文案必须点名未在册的格位: {stderr}"
    );
    assert!(
        !socket.exists(),
        "装配链致命发生在 bind 之前，套接字不得留残迹"
    );
}

/// P4 — 环容量落数据（批N5 件二 F-30）：`[runtime].log_ring_cap = 4` → 日志环只留
/// 最后 4 行。判据两截：`rr3.ctl.logs` 恰回 4 行（容量来自装配单——若还是编译期
/// 512 兜底，这里会是 boot 期全部 ~9 行）；最早一行（装配单已加载）已被丢（环形
/// 语义：满则丢最旧，不是"到顶即停"）。boot 自身 ≥9 行（装配单已加载/容量定格/
/// 缺角色×3/监听/三条 spawn）保证两截都判得到，等待点钉在第三条 spawn 行——
/// 缺角色点名在 spawn 之前（bind 之后立即打），到达序无歧义。
#[test]
fn log_ring_cap_from_profile_bounds_ctl_logs() {
    let root = Root::new("cap");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        root.write_module(name);
    }
    let profile = root.write_profile(&GOOD.replace("log_ring_cap = 512", "log_ring_cap = 4"));
    let mut rrd =
        RrdTest::rr3_start_profile_env("prof-cap", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        rrd.rr3_wait_log(&format!("spawn {name} pid="));
    }

    let mut ctl = rrd.rr3_connect();
    rr3_send(&mut ctl, &rr3_rpc_frame("rr3.ctl.logs", 2, json!({}), 0));
    let f = rr3_read_where(&mut ctl, "ctl.logs 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    let lines = v["result"]["lines"]
        .as_array()
        .expect("logs 应带 result.lines");
    let joined: Vec<&str> = lines.iter().filter_map(|l| l.as_str()).collect();
    assert_eq!(
        joined.len(),
        4,
        "环容量 = 装配单 [runtime].log_ring_cap = 4，ctl.logs 必须恰回 4 行: {joined:?}"
    );
    assert!(
        !joined.iter().any(|l| l.contains("装配单已加载")),
        "最早的装配单行必须已被环形淘汰（满则丢最旧）: {joined:?}"
    );
}

/// P5 — rrd 自身日志落盘（批N5 件二笔2，§17 运维留痕同轨判据）：第一世的每一行
/// `rr3_log` 同步 append 进 `<socket>.log/rrd.log`；杀掉 rrd 后**同 socket** 重启
/// 第二世（spawn_inner 的 remove_file 只清 socket 文件，落盘目录 `.sock.log/` 不在
/// 清列——崩溃前行因此幸存，这正是被测语义），启动回填把文件尾置回环头 →
/// `rr3.ctl.logs` 取得到第一世的收尸行。
///
/// 独有性设计：第一世 kill 掉已 running 的 rr3-pb，supervisor 收尸打
/// `rr3-pb 子进程退出 status=...`——第二世模块全活，不可能有这行；若环里的它
/// 不是来自文件回填，本断言必红。时序断言：收尸行必须早于第二世最后一条 spawn
/// 行（回填行在环头、第二世行在其后——push_front 次序错的写法在这里红）。
/// cap=64：第一世 boot ~10 行 + 第二世 ~10 行远小于容量，两世行全在环内。
/// SIGTERM 优雅关停即可证：同步 append 下行早已当场在文件里，重启回收与
/// 关停形状无关（SIGKILL 崩溃只会更早截尾，不会多出别的路径）。
#[test]
fn rrd_reboot_recovers_previous_run_lines_from_disk() {
    let root = Root::new("sink");
    for name in ["rr3-pa", "rr3-pb", "rr3-pc"] {
        root.write_module(name);
    }
    // 落盘目录残根清掉（append 不 truncate：同 pid 同 tag 重跑时上一世的文件
    // 会混进判据面——本用例的独有行必须来自本世的第一世）
    let sink_dir = PathBuf::from(format!(
        "/tmp/rr3-m2-{}-prof-sink.sock.log",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&sink_dir);

    let profile = root.write_profile(&GOOD.replace("log_ring_cap = 512", "log_ring_cap = 64"));

    // —— 第一世：boot → rr3-pb running → kill -9 → 收尸行 → kill rrd ——
    let mut rrd1 =
        RrdTest::rr3_start_profile_env("prof-sink", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
    let spawn_pb = rrd1.rr3_wait_log("spawn rr3-pb pid=");
    let pb_running = rr3_wait_until(
        || {
            rr3_status(&rrd1)
                .iter()
                .any(|(n, p)| n == "rr3-pb" && p == "running")
        },
        Duration::from_secs(20),
    );
    assert!(
        pb_running,
        "rr3-pb 必须先到 running 再 kill：提前杀会走未注册即退出的另一条文案，判据不再独有；最后快照: {:?}",
        rr3_status(&rrd1)
    );
    let killed = Command::new("/bin/kill")
        .arg("-9")
        .arg(rr3_field_from_log(&spawn_pb, "pid=").to_string())
        .status()
        .expect("kill 前置失败");
    assert!(killed.success(), "kill 前置失败");
    rrd1.rr3_wait_log("rr3-pb 子进程退出");
    rrd1.rr3_sigterm_and_wait(); // 杀掉第一世；同步 append 下崩溃前行已全在文件里
    drop(rrd1);

    // —— 第二世：同 tag = 同 socket；ctl.logs 应含第一世崩溃前的行 ——
    let mut rrd2 =
        RrdTest::rr3_start_profile_env("prof-sink", &profile, E2E_TICK_MS, &[("RR3_PROBE", "sim")]);
    rrd2.rr3_wait_log("spawn rr3-pa pid=");
    let mut ctl = rrd2.rr3_connect();
    rr3_send(&mut ctl, &rr3_rpc_frame("rr3.ctl.logs", 2, json!({}), 0));
    let f = rr3_read_where(&mut ctl, "ctl.logs 应答", |fr| {
        rr3_body_split(&fr.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    });
    let v = rr3_json_of(&f);
    let joined: Vec<String> = v["result"]["lines"]
        .as_array()
        .expect("logs 应带 result.lines")
        .iter()
        .filter_map(|l| l.as_str().map(str::to_string))
        .collect();
    assert!(
        joined.iter().any(|l| l.contains("rr3-pb 子进程退出")),
        "第二世 ctl.logs 必须取到第一世崩溃前的收尸行（rrd 自身日志落盘＋启动回填）: {joined:?}"
    );
    let corpse_idx = joined
        .iter()
        .position(|l| l.contains("rr3-pb 子进程退出"))
        .expect("上面断言已保证在场");
    assert!(
        joined
            .iter()
            .rposition(|l| l.contains("spawn rr3-pa pid="))
            .is_some_and(|last| corpse_idx < last),
        "第一世的行必须在第二世的行之前（回填行置环头，push_front 次序错了在这里红）: {joined:?}"
    );
}
