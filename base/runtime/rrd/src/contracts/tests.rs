// SPDX-License-Identifier: Apache-2.0
//! contracts 单测（文件头：本文件只放测试，生产面在 `contracts.rs`）。
//!
//! 拆分动因：`contracts.rs` 588 行、距 600 行门只剩十余行余量，先把测试面搬出。

use super::*;

/// NIST FIPS 180-4 标准测试向量（含空串 / 单字节 / 一块 / 填充临界 56 字节
/// [恰好 448 bit，填充须另起一块] / 跨三块 112 字节）——自持实现的正确性锚。
#[test]
fn sha256_nist_vectors() {
    assert_eq!(
        rr3_sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        rr3_sha256_hex(b"a"),
        "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
    );
    assert_eq!(
        rr3_sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        rr3_sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        rr3_sha256_hex(
            // 续行符吃行首空白，拼接回原 112 字节串
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno\
              ijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
        ),
        "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
    );
}

/// 记账 + 自检判果三形 + 渲染（Z-4 四册账＋基-40 真值集账），单函数顺序跑：
/// 判果/渲染共享进程级 OnceLock 快照，拆并行用例会让"在册/不在册"依赖执行序
/// ——判据面不赌运气。三形 = 定格后全 pass；改册 = sha 对破；删册 = 齐全破
/// （同一份快照，判据定义域 = "这份文件"，改/删只反映在重读对比里）。快照
/// 未在册的 available:false 形由 --manifest e2e 路径覆盖（e2e_profile）。
#[test]
fn contracts_renders() {
    let root = std::env::temp_dir().join(format!("rrd-contracts-audit-{}", std::process::id()));
    let stem = "audit-robot";
    let chip = "rk3566";
    let (profile_path, contracts, wiring_path) = audit_fixture(&root, stem, chip);
    let miss_dir = audit_booked(&profile_path, &contracts, &wiring_path, stem, chip);
    audit_drift_missing(&contracts);
    audit_render(chip);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&miss_dir);
}

/// fixture = 最小真值集环境（capture 只定位不解析，装配单/接线档内容随意）：
/// profiles/<stem>.toml + 契约四册 + contracts/platform/<chip>.toml +
/// hardware/<stem>/wiring.toml，布局与装载消费真值集成员判据同名同位。
fn audit_fixture(
    root: &std::path::Path,
    stem: &str,
    chip: &str,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let contracts = root.join("contracts");
    std::fs::create_dir_all(contracts.join("platform")).expect("建临时契约目录");
    let profile_path = root.join("profiles").join(format!("{stem}.toml"));
    std::fs::create_dir_all(profile_path.parent().unwrap()).expect("建 profiles 目录");
    std::fs::write(&profile_path, "# audit fixture\n").expect("写装配单");
    for (f, body) in [
        ("topics.toml", "# t1\n"),
        ("slots.toml", "# s1\n"),
        ("roles.toml", "# r1\n"),
        ("devices.toml", "# d1\n"),
    ] {
        std::fs::write(contracts.join(f), body).expect("写契约册");
    }
    std::fs::write(
        contracts.join("platform").join(format!("{chip}.toml")),
        "# p1\n",
    )
    .expect("写平台事实表");
    let wiring_path = root.join("hardware").join(stem).join("wiring.toml");
    std::fs::create_dir_all(wiring_path.parent().unwrap()).expect("建 hardware 目录");
    std::fs::write(&wiring_path, "# w1\n").expect("写接线档");
    (profile_path, contracts, wiring_path)
}

/// 记账与齐全/未漂判定：缺成员 fail-closed → 七成员记账断言 → 定格 → 五条判果。
fn audit_booked(
    profile_path: &std::path::Path,
    contracts: &std::path::Path,
    wiring_path: &std::path::Path,
    stem: &str,
    chip: &str,
) -> std::path::PathBuf {
    // 记账 fail-closed：真值集成员缺席拒记账（boot 定格点据此带因拒 boot）——
    // 四册齐而平台事实表缺席，第二段记账照样红（基-40 成员完整性是记账前提）
    let miss_dir = std::env::temp_dir().join(format!("rrd-contracts-miss-{}", std::process::id()));
    std::fs::create_dir_all(&miss_dir).expect("建缺册目录");
    let e = rr3_contracts_scan(profile_path, &miss_dir, wiring_path, chip)
        .expect_err("缺真值集成员必须拒记账");
    assert!(e.contains("记账读失败") && e.contains("topics.toml"), "{e}");

    let snap = rr3_contracts_scan(profile_path, contracts, wiring_path, chip)
        .expect("真值集七成员齐全应记账成功");
    assert_eq!(snap.entries.len(), 4);
    assert_eq!(snap.entries[0].file, "topics.toml");
    assert_eq!(snap.entries[0].bytes, 5);
    // 基-40：七成员逐条点名（成员名 = 装载面真值集相对名，顺序 = 成员名单点序）
    assert_eq!(snap.truth_set.len(), 7);
    assert_eq!(snap.truth_set[0].member, format!("profiles/{stem}.toml"));
    assert_eq!(
        snap.truth_set[5].member,
        format!("contracts/platform/{chip}.toml")
    );
    assert_eq!(
        snap.truth_set[6].member,
        format!("hardware/{stem}/wiring.toml")
    );

    // OnceLock 进程级：本测试进程只这一份定格（首份胜出），三形全在同快照上判
    assert!(rr3_contracts_set(snap));
    let checks = rr3_contracts_checks();
    assert_eq!(checks.len(), 5, "四册逐条＋truth_set 汇总");
    assert!(
        checks.iter().all(|c| c.pass == RR3_CHECK_PASS),
        "{checks:?}"
    );
    assert_eq!(checks[0].item, "contract:topics.toml");
    assert_eq!(checks[4].item, "truth_set");
    miss_dir
}

/// sha 对破（改册 → 还原）与齐全破（删册）两形判果。
fn audit_drift_missing(contracts: &std::path::Path) {
    // sha 对破：boot 后内容被改（四册账与真值集账同抓一病，truth_set 逐枚点名）
    std::fs::write(contracts.join("topics.toml"), "# t2\n").expect("改册");
    let checks = rr3_contracts_checks();
    let drifted = checks
        .iter()
        .find(|c| c.item == "contract:topics.toml")
        .unwrap();
    assert_eq!(drifted.pass, RR3_CHECK_FAIL);
    assert!(drifted.reason.contains("sha256 漂移"), "{}", drifted.reason);
    let ts = checks.iter().find(|c| c.item == "truth_set").unwrap();
    assert_eq!(ts.pass, RR3_CHECK_FAIL);
    assert!(
        ts.reason.contains("漂移: contracts/topics.toml"),
        "{}",
        ts.reason
    );
    std::fs::write(contracts.join("topics.toml"), "# t1\n").expect("还原册");

    // 齐全破：boot 后文件被删；其余册逐条点名不连坐
    std::fs::remove_file(contracts.join("roles.toml")).expect("删册");
    let checks = rr3_contracts_checks();
    let missing = checks
        .iter()
        .find(|c| c.item == "contract:roles.toml")
        .unwrap();
    assert_eq!(missing.pass, RR3_CHECK_FAIL);
    assert!(missing.reason.contains("文件缺失"), "{}", missing.reason);
    let slots = checks
        .iter()
        .find(|c| c.item == "contract:slots.toml")
        .unwrap();
    assert_eq!(slots.pass, RR3_CHECK_PASS);
    let ts = checks.iter().find(|c| c.item == "truth_set").unwrap();
    assert_eq!(ts.pass, RR3_CHECK_FAIL);
    assert!(
        ts.reason.contains("缺册: contracts/roles.toml"),
        "{}",
        ts.reason
    );
}

/// 渲染（快照在册形）：available:true + 四行四册账 + 七行真值集账 + 五条判果。
fn audit_render(chip: &str) {
    let v = rr3_contracts_json();
    assert_eq!(v["available"], true, "{v}");
    assert_eq!(v["contracts"].as_array().map(Vec::len), Some(4));
    assert_eq!(v["truth_set"].as_array().map(Vec::len), Some(7));
    assert_eq!(
        v["truth_set"][5]["member"],
        format!("contracts/platform/{chip}.toml")
    );
    assert_eq!(v["checks"].as_array().map(Vec::len), Some(5));
}
