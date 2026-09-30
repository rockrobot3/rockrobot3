#![cfg(unix)]
//! M3 音频链三真进程端到端：rrd + rr3-sense(sim) + rr3-interact(sim) 全是**真二进制**。
//!
//! 前面的 e2e 用的都是 sim_peer 夹具（行为脚本）；这一组把仓里两个真模块拉进同一条总线，
//! 钉的是音频链的整体形状：
//! 1. boot spawn → 注册 → running（supervisor 的进程语义对真 manifest 成立）；
//! 2. `rr3-sense/audio/vad_start` 事件上总线（sim 正弦必过 VAD 起音阈值，恰一次 onset）；
//! 3. `rr3-sense/audio/pcm` stream 有序——seq 从旁观连接看到的任意一点起逐帧 +1
//!    （旁观者看到的是流的**连续后缀**，起点不是 1 是正形，起点错序才是 bug）；
//! 4. `rr3.call` 回路穿过真进程：旁观连接点名 `rr3-interact/tts/text` 与
//!    `rr3-interact/face/expression`，应答体（played_ms/chars、expression/previous）
//!    由 interact 进程里的播放面与表情机真实算出——模块侧 rr3_reply_call 的
//!    trace_id 语义在这条线上被端到端复证。
//!
//! 模块二进制**由测试现场 cargo build**（独立工程，`CARGO_BIN_EXE` 够不着它们）：
//! 增量编译下缓存命中，代价只在首次全量。

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use rr_ipc::{rr3_body_split, Rr3PayloadType};
use serde_json::{json, Value};

use support::{
    rr3_field_from_log, rr3_json_of, rr3_manifest, rr3_read_where,
    rr3_read_rpc, rr3_register_frame, rr3_rpc_frame, rr3_send, rr3_try_recv, E2E_TICK_MS,
    RrdClient, RrdTest, SimManifest,
};

/// 仓根（CARGO_MANIFEST_DIR = base/runtime/rrd，上溯三级）
fn rr3_repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// 现场编译一个独立工程模块，返回其二进制绝对路径（`modules/<name>/target/debug/<name>`）。
/// 编译失败把 cargo 输出原样 panic 出来——夹具烂和被测行为红必须分得开。
fn rr3_build_module_bin(name: &str) -> PathBuf {
    let dir = rr3_repo_root().join("modules").join(name);
    let manifest = dir.join("Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(&cargo)
        .args(["build", "-q", "--manifest-path"])
        .arg(&manifest)
        .output()
        .unwrap_or_else(|e| panic!("起 {cargo} 失败: {e}"));
    assert!(
        out.status.success(),
        "编译 {name} 失败:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let bin = dir.join("target/debug").join(name);
    let bin = bin.canonicalize().unwrap_or_else(|e| panic!("编译产物 {} 不存在: {e}", bin.display()));
    assert!(bin.is_file(), "{} 不是文件", bin.display());
    bin
}

/// 读真模块的 module.toml（生产契约工件），把 entry 换成刚编译出的二进制绝对路径，
/// 落到 pid+tag 唯一的 /tmp（Global Constraints 8）。替换没命中即 panic：module.toml
/// 的 entry 形状变了时，静默不改会让 rrd 去spawn 一个不存在的 `rr3-sense` 命令。
fn rr3_module_manifest(tag: &str, name: &str, bin: &Path) -> SimManifest {
    let src = rr3_repo_root().join("modules").join(name).join("module.toml");
    let text = std::fs::read_to_string(&src)
        .unwrap_or_else(|e| panic!("读模块 manifest {} 失败: {e}", src.display()));
    let needle = format!(r#"entry = "{name}""#);
    let rendered = text.replace(&needle, &format!(r#"entry = "{}""#, bin.display()));
    assert!(
        rendered != text && rendered.contains(&format!(r#"entry = "{}""#, bin.display())),
        "module.toml 里没有 {needle:?} 可替换——契约工件形状变了，本夹具前提失效"
    );
    let path = PathBuf::from(format!(
        "/tmp/rr3-m3-module-{}-{tag}-{name}.toml",
        std::process::id()
    ));
    std::fs::write(&path, rendered)
        .unwrap_or_else(|e| panic!("写夹具 manifest {} 失败: {e}", path.display()));
    SimManifest { path }
}

/// 旁观连接：注册一个最小模块身份，专收广播与转发（sim_peer 用例的同一口径）
fn connect_observer(rrd: &mut RrdTest) -> (RrdClient, u64) {
    let mut obs = rrd.rr3_connect();
    // 裸测试客户端**不喂狗**（没有心跳线程），而本用例的 rrd 以 E2E_TICK_MS=20ms 扫表：
    // 默认 100/500ms 的观察者注册后 ~0.5s 就被 watchdog 判死摘册——之后 pcm 广播不再投它、
    // rr3.call 以"调用方已不在册"被拒，症状是"恰收到 1 帧 registered 通知后永久静默"。
    // 心跳/看门狗窗口放大到用例寿命之外（hb < wd 是校验硬规），判据是"别在用例中途死"。
    rr3_send(
        &mut obs,
        &rr3_register_frame(&rr3_manifest("rr3-obs", 3_600_000, 3_600_001, "never"), "rr3-obs"),
    );
    let v = rr3_read_rpc(&mut obs);
    let task = v["result"]["src_task"].as_u64().unwrap_or_else(|| panic!("注册应答缺 src_task: {v}"));
    (obs, task)
}

/// 在 pcm 洪流里等一条 rr3.call 应答（按 JSON-RPC id 挑）。
///
/// 不能用 `rr3_read_where`：它那条"64 帧预算扫不到即夹具 bug"的口径，前提是连接上
/// **没有持续帧流**。这里 50fps 的 pcm 广播把队列塞满——前几段读法存回队列的旧 pcm 帧
/// 会被应答读法整段重扫，64 帧预算几乎全耗在旧帧上，留给真应答的 socket 读只剩十几帧
/// （~0.3s），而 interact 的 poll_wait 一拍就是 250ms。所以本用例的挑选不走队列回存：
/// pcm / state 广播直接丢弃（后半段断言读的是 socket 上的新帧，不依赖它们），
/// Response 按 id 挑；预算 600 帧（12s 流量）只是防挂死的上界，应答真的不来会在这里
/// 如实 panic，而不是被预算掩盖成"夹具 bug"。
fn read_call_reply(c: &mut RrdClient, id: u64) -> Value {
    for _ in 0..600 {
        match rr3_try_recv(c) {
            Ok(f) => {
                let hit = match rr3_body_split(&f.payload) {
                    Ok((Rr3PayloadType::Response, body)) => serde_json::from_slice::<Value>(&body)
                        .map(|v| v["id"].as_u64() == Some(id))
                        .unwrap_or(false),
                    _ => false,
                };
                if hit {
                    return rr3_json_of(&f);
                }
            }
            Err(e) => panic!("没等到 rr3.call 应答 id={id}（读帧提前结束: {e:?}）"),
        }
    }
    panic!("600 帧内没等到 rr3.call 应答 id={id}——应答没来或晚于 12s 的流量上界");
}

#[test]
fn audio_chain_sense_to_interact_end_to_end() {
    // ---------- 夹具：真二进制 + 真 manifest ----------
    let sense_bin = rr3_build_module_bin("rr3-sense");
    let interact_bin = rr3_build_module_bin("rr3-interact");
    let m_sense = rr3_module_manifest("chain", "rr3-sense", &sense_bin);
    let m_inter = rr3_module_manifest("chain", "rr3-interact", &interact_bin);

    // ---------- ① boot spawn → 注册 → running ----------
    // boot 用例必须传小 tick（support 注释：sweep 复用同一旋钮，安静 tick 会让收尸永不发生）
    let mut rrd = RrdTest::rr3_start_boot(
        "e2e-chain",
        &[m_sense.path.as_path(), m_inter.path.as_path()],
        "chain",
        E2E_TICK_MS,
    );

    // ---------- ① 旁观连接先就位，再看模块起不起来 ----------
    // 顺序不能反：vad_start 是**一次性边沿**（sim 正弦常幅，onset 恰一次），广播只发给
    // 当时在册的连接——等注册行到了再连，边沿早就广播完了。这里在 rr3_start_boot 返回后
    // 立即注册旁观者：模块那边要付 fork/exec 的整条链（毫秒级），旁观者只付一次 connect
    // +register 往返（亚毫秒），差着两个数量级，注册必落在 sense 第一帧发布之前。
    let (mut obs, obs_task) = connect_observer(&mut rrd);
    let sense_line = rrd.rr3_wait_log("注册 rr3-sense task=");
    let inter_line = rrd.rr3_wait_log("注册 rr3-interact task=");
    rrd.rr3_wait_log("rr3-sense → running");
    rrd.rr3_wait_log("rr3-interact → running");
    let sense_task = rr3_field_from_log(&sense_line, "task=");
    let inter_task = rr3_field_from_log(&inter_line, "task=");
    assert_ne!(sense_task, inter_task, "两个真模块各得一个 task 号");

    // ---------- ② vad_start 上总线：Event + edge=onset + src_task 是 sense 的分配值
    let vad = rr3_read_where(&mut obs, " vad_start 事件", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/vad_start"
    });
    assert_eq!(vad.header.src_task, sense_task, "事件帧 src 必须是发布者的分配值");
    let v = rr3_json_of(&vad);
    assert_eq!(v["edge"], "onset", "sim 正弦常幅，首个边沿必是 onset: {v}");
    assert!(v["rms_peak"].as_i64().unwrap_or(0) > 1500, "rms_peak 必须过起音阈值: {v}");

    // ---------- ③ STREAM 有序：旁观者视角的连续后缀，seq 逐帧 +1 ----------
    // 顺带钉住事件形状：s16le/16k/mono、20ms 一帧（320 采样）
    let mut last: Option<u64> = None;
    for _ in 0..10 {
        let f = rr3_read_where(&mut obs, " pcm 帧", |f| {
            rr3_json_of(f)["topic"] == "rr3-sense/audio/pcm"
        });
        let body = rr3_json_of(&f);
        assert_eq!(body["format"], "s16le");
        assert_eq!(body["rate"], 16000);
        assert_eq!(body["channels"], 1);
        assert_eq!(body["ms"], 20);
        let seq = body["seq"].as_u64().unwrap_or_else(|| panic!("pcm 体缺 seq: {body}"));
        if let Some(p) = last {
            assert_eq!(seq, p + 1, "STREAM 必须逐帧递增（旁观者看到连续后缀）");
        }
        last = Some(seq);
    }

    // ---------- ④ rr3.call 回路穿过真进程 ----------
    // TTS："你好"=2 字符 × 60ms = 120ms（占位合成公式在 interact 进程里真实执行）
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            42,
            json!({"topic": "rr3-interact/tts/text", "args": {"text": "你好"}}),
            obs_task,
        ),
    );
    let v = read_call_reply(&mut obs, 42);
    assert!(
        v.get("error").is_none(),
        "TTS 点名不该被拒: {v}"
    );
    assert_eq!(v["result"]["played_ms"], 120, "60ms/字符的占位时长公式: {v}");
    assert_eq!(v["result"]["chars"], 2);

    // 表情：五态白名单切换，应答带回切换前后
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            43,
            json!({"topic": "rr3-interact/face/expression", "args": {"expression": "happy"}}),
            obs_task,
        ),
    );
    let v = read_call_reply(&mut obs, 43);
    assert_eq!(v["result"]["expression"], "happy", "{v}");
    assert_eq!(v["result"]["previous"], "idle", "TTS 的 talk 挂起已恢复，切换前是 idle: {v}");

    // 坏参数折成 result.error 应答而不是断链（业务性失败 ≠ 传输性失败的线上分界；
    // rr3_reply_call 走的是 Response.result 槽，JSON-RPC error 槽留给 rrd 的路由拒绝）
    rr3_send(
        &mut obs,
        &rr3_rpc_frame(
            rr_bus::RR3_METHOD_CALL,
            44,
            json!({"topic": "rr3-interact/tts/text", "args": {}}),
            obs_task,
        ),
    );
    let v = read_call_reply(&mut obs, 44);
    let err = v["result"]["error"].as_str().unwrap_or_else(|| panic!("缺 text 必须回 result.error 字符串: {v}"));
    assert!(err.contains("text"), "错误信息要点名缺的参数: {err}");

    // 回路打完，sense 的流还活着（interact 的 120ms 阻塞没拖垮任何一方）
    let f = rr3_read_where(&mut obs, " 回路后的又一帧 pcm", |f| {
        rr3_json_of(f)["topic"] == "rr3-sense/audio/pcm"
    });
    let seq = rr3_json_of(&f)["seq"].as_u64().unwrap_or(0);
    assert!(seq > last.unwrap_or(0), "回路之后流必须继续推进: {seq} <= {last:?}");
}
