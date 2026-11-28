// SPDX-License-Identifier: Apache-2.0
//! counters 单测（文件头：本文件只放测试，生产面在 `counters.rs`）。
//!
//! 拆分动因：`counters.rs` 生产面（路径判定 / 原子写序 / 闸门 / 异步线程）与测试
//! 面同文件已近 600 行门；续轮二十一加状态位前先做纯移位，避免门禁触顶。

use super::*;

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "rrd-counters-t-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 路径判定三态：env 优先、linux 缺省、皆无不持久化
#[test]
fn path_env_then_def() {
    // 期望值由 `Path::join` 生成：分隔符随平台（Windows 是 `\`），硬编码 `/`
    // 在 Windows 上假红（2026-11-16 平台腿实证）。
    let want = |dir: &str| {
        Some(
            Path::new(dir)
                .join(COUNTERS_FILE)
                .to_string_lossy()
                .into_owned(),
        )
    };
    assert_eq!(
        rr3_path_with(Some("/tmp/rr3data"), Some("/data/rr3")),
        want("/tmp/rr3data"),
        "env 有值时优先于平台缺省"
    );
    assert_eq!(
        rr3_path_with(None, Some("/data/rr3")),
        want("/data/rr3"),
        "env 无值吃平台缺省（linux rr_data 分区）"
    );
    assert_eq!(rr3_path_with(None, None), None, "皆无不持久化");
    // 空 env 等价于无 env（线上入口 filter 掉）
    assert_eq!(rr3_path_with(Some(""), None), None);
}

/// 存取 round-trip：键序确定（BTreeMap）、值逐键一致
#[cfg(unix)]
#[test]
fn store_load_trips() {
    let dir = tmpdir("round");
    let path = dir.join(COUNTERS_FILE);
    let mut t = BTreeMap::new();
    t.insert("rr3-brain/chat_rounds".to_string(), 7);
    t.insert("rr3-act/relay_on".to_string(), 3);
    t.insert(RR3_COUNTERS_BOOTS_KEY.to_string(), 2);
    rr3_counters_store(&path, &t).expect("写盘应成功");
    assert!(
        !dir.join(COUNTERS_TMP).exists(),
        "原子写序走完不得留 tmp 残留"
    );
    let (loaded, note) = rr3_counters_load(&path);
    assert_eq!(loaded, t, "round-trip 逐键一致");
    assert!(note.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 原子写序覆盖旧值：rename 之后读到的是新表，不存在半新半旧
#[cfg(unix)]
#[test]
fn store_atomically() {
    let dir = tmpdir("over");
    let path = dir.join(COUNTERS_FILE);
    let mut t1 = BTreeMap::new();
    t1.insert("a".to_string(), 1);
    t1.insert("b".to_string(), 2);
    rr3_counters_store(&path, &t1).unwrap();
    let mut t2 = BTreeMap::new();
    t2.insert("a".to_string(), 5);
    rr3_counters_store(&path, &t2).unwrap();
    let (loaded, _) = rr3_counters_load(&path);
    assert_eq!(loaded.len(), 1, "全量覆盖不是合并");
    assert_eq!(loaded["a"], 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 数据目录自建（批349）：出厂镜像 rr_data 分区出厂为空，`/data/rr3` 无人预建——
/// 落盘须自己建出父目录，否则首启每次 ENOENT、boots 每次开机重置为 1。
#[cfg(unix)]
#[test]
fn store_creates_dir() {
    let dir = tmpdir("mkdir");
    let path = dir.join("rr3").join("nested").join(COUNTERS_FILE);
    let mut t = BTreeMap::new();
    t.insert(RR3_COUNTERS_BOOTS_KEY.to_string(), 3);
    rr3_counters_store(&path, &t).expect("缺目录时应自建并落盘");
    assert!(path.is_file(), "落盘后目标档必须在场");
    let (loaded, note) = rr3_counters_load(&path);
    assert!(note.is_none(), "自建目录写出的档应可直接读回: {note:?}");
    assert_eq!(loaded[RR3_COUNTERS_BOOTS_KEY], 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 父路径是文件 = 建目录必败：错误交回调用方点名，不许 panic、不许当成功。
#[cfg(unix)]
#[test]
fn store_names_bad_dir() {
    let dir = tmpdir("baddir");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let err = rr3_counters_store(&blocker.join(COUNTERS_FILE), &BTreeMap::new())
        .expect_err("父路径是文件必败");
    assert!(err.contains("建数据目录"), "错误须点名目录面: {err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 加载的容错：首启（不存在）= 空表无怨言；坏文件 = 空表 + 原因
#[test]
fn load_miss_files() {
    let dir = tmpdir("tol");
    let (t, note) = rr3_counters_load(&dir.join("nope.json"));
    assert!(t.is_empty() && note.is_none(), "首启静默起空账");
    let bad = dir.join(COUNTERS_FILE);
    std::fs::write(&bad, b"{not json").unwrap();
    let (t, note) = rr3_counters_load(&bad);
    assert!(t.is_empty());
    let note = note.expect("坏文件必须点原因");
    assert!(note.contains("不是 JSON 对象"), "{note}");
    // 非对象 JSON 同拒
    std::fs::write(&bad, b"[1,2]").unwrap();
    let (_, note) = rr3_counters_load(&bad);
    assert!(note.is_some());
    // 非整数值跳过，不毒化整表
    std::fs::write(&bad, br#"{"ok": 3, "bad": "x", "neg": -1}"#).unwrap();
    let (t, note) = rr3_counters_load(&bad);
    assert!(note.is_none());
    assert_eq!(t.len(), 1);
    assert_eq!(t["ok"], 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// R26 裁定判据（逐字）：1s 心跳拍 1000 拍、窗口 M=10s → 落盘次数
/// ≤ ceil(1000×间隔/M)+1 = 101（首拍即刷是那个 +1）。合并写的量化收益
/// 就钉在这条上：1000 → 100 次 fsync 整文件写。
#[test]
fn gate_heartbeats() {
    let mut g = Rr3CountersGate::rr3_new(10_000, 64);
    let mut flushes = 0;
    for beat in 0..1000u64 {
        if g.rr3_on_change(beat * 1000) {
            flushes += 1;
            g.rr3_on_flushed(beat * 1000);
        }
    }
    assert_eq!(flushes, 100, "10s 窗口 1000s 跨度应恰 100 次");
    assert!(
        flushes <= (1000 * 1000 + 10_000 - 1) / 10_000 + 1,
        "裁定判据上界 ceil(1000×间隔/M)+1"
    );
}

/// N 拍兜底：10ms 高频拍下 M 窗口（10s）拦不住，pending 攒到 64 强制透一次
/// （首拍 + 每 64 拍一次 = ceil 语义；兜底路径与窗口路径独立成立）。
#[test]
fn gate_floor_burst() {
    let mut g = Rr3CountersGate::rr3_new(10_000, 64);
    let mut flushes = 0;
    for beat in 0..1000u64 {
        if g.rr3_on_change(beat * 10) {
            flushes += 1;
            g.rr3_on_flushed(beat * 10);
        }
    }
    // 首拍（last=None）+ 此后每攒满 64 拍一次：(1000-1)/64 = 15 段 → 共 16
    assert_eq!(flushes, 16, "64 拍兜底应把 10ms 拍 × 1000 压到 16 次");
}

/// 落盘失败不记账：窗口判据下一拍仍成立（自然重试），成功才推进 last_flush
#[test]
fn gate_fail_next_beat() {
    let mut g = Rr3CountersGate::rr3_new(10_000, 64);
    assert!(g.rr3_on_change(0), "首拍必刷");
    // 模拟 store 失败：不调 rr3_on_flushed → 下一拍仍在窗口内也照刷
    assert!(g.rr3_on_change(100), "失败后下一拍重试");
    g.rr3_on_flushed(100); // 这次成功
    assert!(
        !g.rr3_on_change(10_099),
        "窗口内不刷（last=100，窗口到 10_100）"
    );
    assert!(g.rr3_on_change(10_100), "窗口到照刷");
    g.rr3_on_flushed(10_100);
    assert!(!g.rr3_on_change(20_099), "第二个窗口内不刷");
    assert!(g.rr3_on_change(20_100), "第二个窗口到照刷");
}

/// 并发落盘（心跳闸门与 SIGTERM flush 同形）：固定 tmp 名的原子写序必须有
/// 单写者互斥，否则 create/truncate/rename 交错会留下半截 JSON（下次 boot 空账）。
#[cfg(unix)]
#[test]
fn concurrent_file() {
    let dir = tmpdir("conc");
    let path = dir.join("counters.json");
    let mut hs = Vec::new();
    for i in 0..8u64 {
        let p = path.clone();
        hs.push(std::thread::spawn(move || {
            for n in 0..20u64 {
                let mut t = BTreeMap::new();
                t.insert(format!("k{i}"), n);
                rr3_counters_store(&p, &t).expect("并发写应成功");
            }
        }));
    }
    for h in hs {
        h.join().expect("写线程不应 panic");
    }
    let (t, note) = rr3_counters_load(&path);
    assert!(note.is_none(), "落盘必须是可解析的完整 JSON: {note:?}");
    assert_eq!(t.len(), 1, "整表覆盖，最后写胜出: {t:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 在飞位（续轮十三）：投递与回执之间不排新的一轮——否则落盘期间到达的
/// 每一拍都会再触发一次 fsync，R26 的窗口节流被打回逐拍写。
#[test]
fn gate_until_receipt() {
    let mut g = Rr3CountersGate::rr3_new(10_000, 64);
    assert!(g.rr3_on_change(0), "首拍该排一轮");
    g.rr3_mark_inflight();
    assert!(!g.rr3_on_change(1), "在飞未回执时不排新的一轮");
    assert!(!g.rr3_on_change(10_000), "窗口到点也由在飞的一轮覆盖");
    g.rr3_on_flushed(10_000);
    assert!(g.rr3_on_change(20_000), "回执后窗口已过，下一拍照排");
}

/// 失败回执：清在飞位但不推进窗口——下一拍立即重试，不白等一个窗口
#[test]
fn gate_fail_window() {
    let mut g = Rr3CountersGate::rr3_new(10_000, 64);
    assert!(g.rr3_on_change(0));
    g.rr3_mark_inflight();
    g.rr3_on_failed();
    assert!(g.rr3_on_change(50), "失败后下一拍立即重试");
}

/// 单轮 panic 隔离（续轮二十二）：在飞位必须释放、线程续跑——否则 reader 侧
/// 永远不再排新一轮，账本静默停写（panic 文案由 daemon 的 panic hook 负责）。
#[cfg(unix)]
#[test]
fn round_keeps_alive() {
    let mut g = Rr3CountersGate::rr3_new(10_000, 64);
    assert!(g.rr3_on_change(0), "首拍该排一轮");
    g.rr3_mark_inflight();
    let gate = std::sync::Mutex::new(g);
    rr3_ctr_round_guard(&gate, || panic!("注入：单轮炸"));
    assert!(
        crate::rr3_lock(&gate).rr3_on_change(1),
        "在飞位必须释放，下一拍才能再排"
    );
}

/// 异步落盘线程（续轮十三）：一次投递 → 盘上出现当下快照 → gate 收到成功
/// 回执。快照在投递之后取，故落的是最新表而不是投递时刻的副本。
#[cfg(unix)]
#[test]
fn async_marks_gate() {
    use std::sync::{Arc, Mutex};
    let dir = tmpdir("async");
    let path = dir.join(COUNTERS_FILE);
    let shared = Arc::new(Mutex::new(crate::registry::RrdRegistry::new(100)));
    crate::rr3_lock(&shared).rr3_reg_ctr_bump("k", 7);
    let gate = Arc::new(Mutex::new(Rr3CountersGate::rr3_new(10_000, 64)));
    let tx = rr3_ctr_store_spawn(
        shared.clone(),
        gate.clone(),
        path.clone(),
        std::time::Instant::now(),
    )
    .expect("落盘线程应起得来");
    assert_eq!(rr3_ctr_store_state(), "on", "线程起得来即定格 on");
    tx.try_send(()).expect("首次投递应成功");
    crate::rr3_lock(&gate).rr3_mark_inflight();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let ok = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|v| v["k"] == 7)
            .unwrap_or(false);
        let receipt = !crate::rr3_lock(&gate).in_flight;
        if ok && receipt {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "异步落盘或 gate 回执未在期限内完成"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // 发送端 drop → 通道 EOF → 线程退出（不 join：线程随通道自然收场）；
    // 收场守卫 disarm 后置 off——正常关停不得被读成 dead（续轮二十一）。
    drop(tx);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while rr3_ctr_store_state() != "off" {
        assert!(
            std::time::Instant::now() < deadline,
            "通道 EOF 后线程应收场并置 off"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
