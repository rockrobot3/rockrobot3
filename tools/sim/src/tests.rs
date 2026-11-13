//! 仿真入口单测：drill 报告与链/装配判据在主板面的形状。

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn args_parse_defaults_and_overrides() {
    let a = rr3_sim_args_parse(&v(&[])).unwrap();
    assert!(!a.keep);
    assert!(a.socket.is_none() && a.repo.is_none() && a.rrd.is_none());
    let a = rr3_sim_args_parse(&v(&["--keep", "--socket", "/tmp/s.sock", "--repo", "/r"])).unwrap();
    assert!(a.keep);
    assert_eq!(a.socket.unwrap(), std::path::PathBuf::from("/tmp/s.sock"));
    assert_eq!(a.repo.unwrap(), std::path::PathBuf::from("/r"));
    // --brain 开关
    let a = rr3_sim_args_parse(&v(&["--brain"])).unwrap();
    assert!(a.brain);
    // 开关在值的位置收到另一个开关要拒，不许静默吞掉
    let e = rr3_sim_args_parse(&v(&["--socket", "--keep"])).unwrap_err();
    assert!(e.contains("疑似开关"), "{e}");
    assert!(rr3_sim_args_parse(&v(&["--nonsense"])).is_err());
}

#[test]
fn brain_flags_parse_whitelist_and_fail_closed() {
    // 合法组合：mac 真麦 + 真云 + 端点
    let a = rr3_sim_args_parse(&v(&[
        "--brain",
        "--audio-backend",
        "mac",
        "--brain-cloud",
        "real",
        "--brain-base-url",
        "https://api.siliconflow.cn",
    ]))
    .unwrap();
    assert_eq!(a.audio_backend.as_deref(), Some("mac"));
    assert_eq!(a.brain_cloud.as_deref(), Some("real"));
    assert_eq!(
        a.brain_base_url.as_deref(),
        Some("https://api.siliconflow.cn")
    );
    // 缺省：三旗标全 None（CI 纪律——缺省行为零改动）
    let a = rr3_sim_args_parse(&v(&["--brain"])).unwrap();
    assert!(a.audio_backend.is_none() && a.brain_cloud.is_none() && a.brain_base_url.is_none());
    // 取值白名单
    let e = rr3_sim_args_parse(&v(&["--brain", "--audio-backend", "win"])).unwrap_err();
    assert!(e.contains("取值不认"), "{e}");
    let e = rr3_sim_args_parse(&v(&["--brain", "--brain-cloud", "hybrid"])).unwrap_err();
    assert!(e.contains("取值不认"), "{e}");
    // fail-closed：real 缺端点拒起
    let e = rr3_sim_args_parse(&v(&["--brain", "--brain-cloud", "real"])).unwrap_err();
    assert!(e.contains("--brain-base-url"), "{e}");
    // mock 给端点拒（mock 不碰网络，静默忽略是第一错法）
    let e = rr3_sim_args_parse(&v(&["--brain", "--brain-base-url", "https://x.example/v1"]))
        .unwrap_err();
    assert!(e.contains("mock"), "{e}");
    // 端点形状把关
    let e = rr3_sim_args_parse(&v(&[
        "--brain",
        "--brain-cloud",
        "real",
        "--brain-base-url",
        "api.siliconflow.cn",
    ]))
    .unwrap_err();
    assert!(e.contains("http"), "{e}");
    // 专用旗标不许在别的演练里静默生效
    let e = rr3_sim_args_parse(&v(&["--chain", "--audio-backend", "mac"])).unwrap_err();
    assert!(e.contains("仅 --brain"), "{e}");
}

#[test]
fn sink_backend_flag_fails_closed() {
    // 缺省 None（既有链路零改动）；合法取值入库
    let a = rr3_sim_args_parse(&v(&["--brain"])).unwrap();
    assert!(a.sink_backend.is_none());
    let a = rr3_sim_args_parse(&v(&["--brain", "--sink-backend", "mac"])).unwrap();
    assert_eq!(a.sink_backend.as_deref(), Some("mac"));
    // 仅 --brain 适用
    let e = rr3_sim_args_parse(&v(&["--chain", "--sink-backend", "mac"])).unwrap_err();
    assert!(e.contains("仅 --brain"), "{e}");
    // 取值白名单
    let e = rr3_sim_args_parse(&v(&["--brain", "--sink-backend", "win"])).unwrap_err();
    assert!(e.contains("取值不认"), "{e}");
    // fail-closed ②：sink=esp32 拒收（板无扬声器，回放恒宿主）
    let e = rr3_sim_args_parse(&v(&["--brain", "--sink-backend", "esp32"])).unwrap_err();
    assert!(e.contains("板无扬声器"), "{e}");
    // fail-closed ①：audio=esp32 未给 sink 拒起（缺省跟随会令 interact 抢 UART）
    let e = rr3_sim_args_parse(&v(&["--brain", "--audio-backend", "esp32"])).unwrap_err();
    assert!(e.contains("--sink-backend"), "{e}");
    // fail-closed ① 的正门：audio=esp32 + sink=mac 分列通过
    let a = rr3_sim_args_parse(&v(&[
        "--brain",
        "--audio-backend",
        "esp32",
        "--sink-backend",
        "mac",
    ]))
    .unwrap();
    assert_eq!(a.audio_backend.as_deref(), Some("esp32"));
    assert_eq!(a.sink_backend.as_deref(), Some("mac"));
}

#[test]
fn repo_root_found_upward_by_contract_marker() {
    let base = std::env::temp_dir().join(format!("rr3-sim-root-{}", std::process::id()));
    let deep = base.join("a/b/c");
    std::fs::create_dir_all(deep.join("contracts")).unwrap();
    std::fs::write(deep.join("contracts/topics.toml"), "schema_version = 1\n").unwrap();
    std::fs::create_dir_all(deep.join("a/b/c")).unwrap();
    let found = rr3_find_repo_root(&deep).expect("向上找仓根");
    assert_eq!(found, deep);
    // 找不到返回 None，不 panic
    let none = std::env::temp_dir().join(format!("rr3-sim-root-absent-{}", std::process::id()));
    assert_eq!(rr3_find_repo_root(&none), None);
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn default_bins_share_target_dir() {
    // target-dir 由仓根配置移出源码区：三个缺省产物必须同层且同名，
    // 不得再按 `<crate>/target/debug` 推（源码区没有产物）。
    let (rrd, peer, rrctl) = rr3_default_bins(std::path::Path::new("/repo"));
    assert_eq!(rrd.file_name().unwrap(), "rrd");
    assert_eq!(peer.file_name().unwrap(), "sim_peer");
    assert_eq!(rrctl.file_name().unwrap(), "rr3-ctl");
    assert_eq!(rrd.parent(), peer.parent(), "rrd/sim_peer 同 target-dir");
    assert_eq!(rrd.parent(), rrctl.parent(), "rr3-ctl 同 target-dir");
    // 缺失检查点名 cargo build 命令
    let e = rr3_require_bins(
        std::path::Path::new("/nope/rrd"),
        std::path::Path::new("/nope/sim_peer"),
        std::path::Path::new("/nope/rr3-ctl"),
    )
    .unwrap_err();
    assert!(e.contains("cargo build"), "报错要给构建指引: {e}");
    // 扩展版同一真值：自定义 (路径, 指引) 对照查
    let e = rr3_require_bins_ext(&[
        (std::path::Path::new("/nope/x"), "cargo build -p x"),
        (std::path::Path::new("/nope/y"), "cargo build -p y"),
    ])
    .unwrap_err();
    assert!(
        e.contains("/nope/x") && e.contains("cargo build -p x"),
        "{e}"
    );
    assert!(rr3_require_bins_ext(&[]).is_ok());
}

#[test]
fn manifest_text_has_absolute_entry_and_real_topic() {
    // C14① 后演练主题 vad_start 的 owner 是 rr3-sense——名字与主题前缀同侧
    let t = rr3_manifest_text(
        "rr3-sense",
        std::path::Path::new("/abs/target/debug/sim_peer"),
        200,
        1000,
        "never",
        &[RR3_SIM_B_TOPIC],
    );
    assert!(t.contains("entry = \"/abs/target/debug/sim_peer\""), "{t}");
    assert!(
        t.contains(&format!("publishes = [\"{RR3_SIM_B_TOPIC}\"]")),
        "{t}"
    );
    assert!(t.contains("name = \"rr3-sense\""));
    assert!(t.contains("watchdog_timeout_ms = 1000"));
    // publishes 为空时是空数组而不是残缺语法
    let t = rr3_manifest_text(
        "rr3-sense",
        std::path::Path::new("/abs/x"),
        1,
        2,
        "always",
        &[],
    );
    assert!(t.contains("publishes = []"), "{t}");
}

#[test]
fn status_table_parses_and_skips_non_rows() {
    let out = "NAME          TASK  PHASE\n\
                   rr3-sense     2     running\n\
                   denied_publish = 0\n";
    let rows = rr3_parse_status(out);
    assert_eq!(rows, vec![("rr3-sense".into(), 2, "running".into())]);
    // 空在册表：只有表头 + 计数行
    assert!(rr3_parse_status("NAME  TASK  PHASE\ndenied_publish = 0\n").is_empty());
    assert!(rr3_parse_status("").is_empty());
}

#[test]
fn wait_modules_polls_until_all_wanted_phases_appear() {
    // 前两轮缺 A，第三轮齐了
    let mut calls = 0;
    rr3_wait_modules(
        || {
            calls += 1;
            if calls < 3 {
                vec![]
            } else {
                vec![("rr3-sense".to_string(), 2, "running".to_string())]
            }
        },
        &[("rr3-sense", "running")],
        std::time::Duration::from_secs(5),
    )
    .expect("第三轮应达成");
    assert_eq!(calls, 3);
    // 超时路径：点名缺谁并带上最后一次快照
    let e = rr3_wait_modules(
        || vec![("rr3-sense".to_string(), 2, "registered".to_string())],
        &[("rr3-sense", "running")],
        std::time::Duration::from_millis(100),
    )
    .unwrap_err();
    assert!(
        e.contains("rr3-sense/running") && e.contains("registered"),
        "{e}"
    );
}

#[test]
fn sim_module_identities_exist_in_the_real_contract() {
    // 钉住"演练用的名字/主题必须真实在契约里"——契约改名时这里红，不是演练跑到一半红。
    // 路径从 CARGO_MANIFEST_DIR 上去（tools/sim → 仓根）
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let contract = std::path::Path::new(manifest_dir).join("../../contracts/topics.toml");
    let text = std::fs::read_to_string(&contract)
        .unwrap_or_else(|e| panic!("读 {}: {e}", contract.display()));
    for needle in [RR3_SIM_MODULE_A, RR3_SIM_MODULE_B, RR3_SIM_B_TOPIC] {
        assert!(text.contains(needle), "契约缺 {needle}");
    }
}
