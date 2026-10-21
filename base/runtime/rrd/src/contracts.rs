//! 契约完整性审计（批N5 件五笔4·缺口④，Z-4）：boot 装载链验收后的契约四册
//! sha256 记账 + `rr3.ctl.contracts` 应答渲染——出厂自检验"包内四份契约文件
//! 齐全且 sha 对"（全案 §20.1 Z-4 / §22.6 归属：批N1 修打包面，批N5 落自检判）。
//!
//! 为什么是"记账快照 + 自检复演"而不是查询时算一遍：与装配快照（[`crate::
//! assembly`]）同一条纪律——ctl 应答必须展示**装载面验收通过的那份**。boot 时
//! 四册已真实装载（topics 在 main 装载链、slots/roles/devices 在校验①—⑩ 链），
//! 定格四册 sha256 后，自检时点重读对比：缺 = "boot 后被删"，sha 漂 = "boot 后
//! 内容被改"——两者都是"快照与装载面漂移"，正是自检要抓的病。
//!
//! sha256 本体（FIPS 180-4）仓内自持：判据字面是"sha 对"，rrd 零密码学依赖
//! 纪律下手写纯函数，正确性由 NIST 标准测试向量钉死（[`tests`]）——完整性
//! 对账不是安全面，不需要引入一个 crate 来证明 64 轮压缩写对了。

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::assembly::{Rr3AssemblyCheck, RR3_CHECK_FAIL, RR3_CHECK_PASS};

/// 契约四册（Z-4 判据的"四份"，与 [`crate::profile::rr3_runtime_truth_set`]
/// 的 contracts 四册成员同名同序）：打包面（rr3-pack）与装载面共用那份清单，
/// 本文件是自检判据面的点名清单——判据对象以"装载期读的文件"为成员判据。
pub const RR3_CONTRACT_FILES: [&str; 4] =
    ["topics.toml", "slots.toml", "roles.toml", "devices.toml"];

/// 一册契约的记账行：boot 时定格的文件名、定位路径与 sha256 十六进制串、字节数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3ContractsEntry {
    /// 文件名（[`RR3_CONTRACT_FILES`] 成员，应答 `contract:<名>` 点名用）
    pub file: String,
    /// 定位路径（boot 记账时的 `--contracts` join 产物，绝对化由调用方保证；
    /// 自检复演重读同一路径——漂移判据的定义域就是"这份文件"）
    pub path: PathBuf,
    /// sha256 十六进制小写（64 字符）
    pub sha256: String,
    /// 字节数（"齐全"的半边证据：空文件在装载链活不过解析，这里如实记账）
    pub bytes: u64,
}

/// 一份契约记账快照（boot 装载链验收后一次定格）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rr3ContractsSnapshot {
    /// 四册记账（顺序 = [`RR3_CONTRACT_FILES`] 清单序，确定性渲染）
    pub entries: Vec<Rr3ContractsEntry>,
}

static RR3_CONTRACTS_SNAPSHOT: OnceLock<Rr3ContractsSnapshot> = OnceLock::new();

/// boot 装载链成功后一次定格（daemon.rs --profile 臂单点调用）。重复 set
/// 首份胜出并回 false（与装配快照 OnceLock 语义一致）；--manifest 调试路径
/// 不定格——`rr3.ctl.contracts` 如实报不可得，不造假数据。
pub fn rr3_contracts_snapshot_set(snapshot: Rr3ContractsSnapshot) -> bool {
    RR3_CONTRACTS_SNAPSHOT.set(snapshot).is_ok()
}

/// 快照只读口（conn.rs `rr3.ctl.contracts` 渲染源）。None = 未以 `--profile`
/// 启动（`--manifest` 调试路径无契约记账）。
pub fn rr3_contracts_snapshot() -> Option<&'static Rr3ContractsSnapshot> {
    RR3_CONTRACTS_SNAPSHOT.get()
}

/// 对一份契约目录记账四册：逐册读 + sha256。任何一册读不到 / 不可读即 Err
/// ——调用点是 boot 装载链验收之后，四册刚被装载面读过一遍，这里再读失败
/// 是文件系统级异常，fail-closed 让 boot 带因退出（不带病定格）。
pub fn rr3_contracts_snapshot_from_dir(
    dir: &std::path::Path,
) -> Result<Rr3ContractsSnapshot, String> {
    let mut entries = Vec::with_capacity(RR3_CONTRACT_FILES.len());
    for f in RR3_CONTRACT_FILES {
        let path = dir.join(f);
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("契约四册记账读失败 {}: {e}", path.display()))?;
        entries.push(Rr3ContractsEntry {
            file: f.to_string(),
            path,
            sha256: rr3_sha256_hex(&bytes),
            bytes: bytes.len() as u64,
        });
    }
    Ok(Rr3ContractsSnapshot { entries })
}

/// 自检判果（纯函数，判据真值单点；Z-4 判据 = "包内四份契约文件齐全且 sha 对"）：
/// 四册逐条按记账路径重读对比——缺 = 齐全破、sha 不等 = sha 对破、等 = pass。
/// 四册判据同质，无 not-applicable 豁免面。快照未在册（--manifest 路径）回空表，
/// 渲染层如实报 available:false，不造假判果。
pub fn rr3_contracts_checks() -> Vec<Rr3AssemblyCheck> {
    let Some(s) = rr3_contracts_snapshot() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in &s.entries {
        let item = format!("contract:{}", e.file);
        match std::fs::read(&e.path) {
            Ok(bytes) => {
                let now = rr3_sha256_hex(&bytes);
                if now == e.sha256 {
                    out.push(Rr3AssemblyCheck {
                        item,
                        pass: RR3_CHECK_PASS,
                        reason: format!(
                            "sha256 对上 boot 装载面记账（{} 字节）——齐全且未漂移（Z-4 自检判）",
                            e.bytes
                        ),
                    });
                } else {
                    out.push(Rr3AssemblyCheck {
                        item,
                        pass: RR3_CHECK_FAIL,
                        reason: format!(
                            "sha256 漂移：boot 记账 {} → 现读 {}（Z-4 sha 对破——boot 后内容被改）",
                            e.sha256, now
                        ),
                    });
                }
            }
            Err(_) => out.push(Rr3AssemblyCheck {
                item,
                pass: RR3_CHECK_FAIL,
                reason: "文件缺失——boot 装载面验收后包内契约被删（Z-4 齐全破）".to_string(),
            }),
        }
    }
    out
}

/// `rr3.ctl.contracts` 应答渲染（conn.rs 单点调用；形状与 `rr3.ctl.wiring`
/// 同族：available + checks）。快照未在册（--manifest 调试路径）如实报不可得。
pub fn rr3_contracts_json() -> Value {
    match rr3_contracts_snapshot() {
        Some(s) => {
            let checks: Vec<Value> = rr3_contracts_checks()
                .iter()
                .map(|c| json!({ "item": c.item, "pass": c.pass, "reason": c.reason }))
                .collect();
            json!({
                "available": true,
                "contracts": s.entries.iter().map(|e| json!({
                    "file": e.file,
                    "sha256": e.sha256,
                    "bytes": e.bytes,
                })).collect::<Vec<Value>>(),
                "checks": checks,
            })
        }
        None => json!({
            "available": false,
            "reason": "未以 --profile 启动（--manifest 调试路径无契约记账），契约审计不可得",
        }),
    }
}

/// sha256（FIPS 180-4）——纯函数，无查表依赖，正确性由 [`tests`] 的 NIST
/// 标准测试向量钉死（空串 / 单字节 / 一块 / 填充临界 56 字节 / 跨三块 112 字节）。
pub fn rr3_sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    // 消息填充：0x80 + 零 + 64 位大端位长（|msg| ≡ 56 mod 64）
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    for block in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, wi) in w.iter_mut().enumerate().take(16) {
            *wi = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (hi, xi) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *hi = hi.wrapping_add(xi);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// sha256 十六进制小写串（记账与应答的呈现面）。
pub fn rr3_sha256_hex(data: &[u8]) -> String {
    rr3_sha256(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NIST FIPS 180-4 标准测试向量（含空串 / 单字节 / 一块 / 填充临界 56 字节
    /// [恰好 448 bit，填充须另起一块] / 跨三块 112 字节）——自持实现的正确性锚。
    #[test]
    fn sha256_matches_nist_vectors() {
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

    /// 记账 + 自检判果三形 + 渲染（Z-4），单函数顺序跑：判果/渲染共享进程级
    /// OnceLock 快照，拆并行用例会让"在册/不在册"依赖执行序——判据面不赌运气。
    /// 三形 = 定格后四册全 pass；改册 = sha 对破；删册 = 齐全破（同一份快照，
    /// 判据定义域 = "这份文件"，改/删只反映在重读对比里）。快照未在册的
    /// available:false 形由 --manifest e2e 路径覆盖（e2e_profile）。
    #[test]
    fn contracts_audit_judges_full_drift_missing_and_renders() {
        // 记账 fail-closed：缺册目录拒记账（boot 定格点据此带因拒 boot）
        let miss = std::env::temp_dir().join(format!("rrd-contracts-miss-{}", std::process::id()));
        std::fs::create_dir_all(&miss).expect("建临时契约目录");
        let e = rr3_contracts_snapshot_from_dir(&miss).expect_err("空目录必须拒记账");
        assert!(e.contains("记账读失败") && e.contains("topics.toml"), "{e}");
        let _ = std::fs::remove_dir_all(&miss);

        let dir = std::env::temp_dir().join(format!("rrd-contracts-audit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时契约目录");
        for (f, body) in [
            ("topics.toml", "# t1\n"),
            ("slots.toml", "# s1\n"),
            ("roles.toml", "# r1\n"),
            ("devices.toml", "# d1\n"),
        ] {
            std::fs::write(dir.join(f), body).expect("写契约册");
        }
        let snap = rr3_contracts_snapshot_from_dir(&dir).expect("四册齐全应记账成功");
        assert_eq!(snap.entries.len(), 4);
        assert_eq!(snap.entries[0].file, "topics.toml");
        assert_eq!(snap.entries[0].bytes, 5);

        // OnceLock 进程级：本测试进程只这一份定格（首份胜出），三形全在同快照上判
        assert!(rr3_contracts_snapshot_set(snap));
        let checks = rr3_contracts_checks();
        assert_eq!(checks.len(), 4);
        assert!(
            checks.iter().all(|c| c.pass == RR3_CHECK_PASS),
            "{checks:?}"
        );
        assert_eq!(checks[0].item, "contract:topics.toml");

        // sha 对破：boot 后内容被改
        std::fs::write(dir.join("topics.toml"), "# t2\n").expect("改册");
        let checks = rr3_contracts_checks();
        let drifted = checks
            .iter()
            .find(|c| c.item == "contract:topics.toml")
            .unwrap();
        assert_eq!(drifted.pass, RR3_CHECK_FAIL);
        assert!(drifted.reason.contains("sha256 漂移"), "{}", drifted.reason);

        // 齐全破：boot 后文件被删；其余册逐条点名不连坐
        std::fs::remove_file(dir.join("roles.toml")).expect("删册");
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

        // 渲染（快照在册形）：available:true + 四行记账 + 四条判果
        let v = rr3_contracts_json();
        assert_eq!(v["available"], true, "{v}");
        assert_eq!(v["contracts"].as_array().map(Vec::len), Some(4));
        assert_eq!(v["checks"].as_array().map(Vec::len), Some(4));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
