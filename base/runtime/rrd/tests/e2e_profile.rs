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
/// 校验②只记 role_missing 非致命，boot 照常。
const GOOD: &str = r#"
schema_version = 3

[os]
route = "distro"

[platform]
tier = "host"

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
