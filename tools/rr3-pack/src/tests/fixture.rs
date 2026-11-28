// SPDX-License-Identifier: Apache-2.0
//! 打包夹具：临时 payload/repo、装配单样本与配置构造函数。

use std::fs;
use std::path::{Path, PathBuf};

use crate::Rr3PackConfig;

/// 真仓装配单（基-25：打包预验走 rrd 解析器，夹具必须给合法装配单——空文本过不了
/// 解析器。include_str! 相对源文件 src/tests/，到仓根五级）。distro chip=rk3566、
/// esp32-mini chip=esp32，平台事实表册位随之。
pub(super) const DISTRO_PROFILE: &str = include_str!("../../../../profiles/distro.toml");
pub(super) const ESP32_MINI_PROFILE: &str = include_str!("../../../../profiles/esp32-mini.toml");

/// 夹具根唯一序号：并行测试各占一个目录（ZST 指针恒同、pid 同批相同，都当不得唯一键）
pub(super) fn fixture_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// 夹具：临时 payload + repo。
pub(super) fn fixture() -> (PathBuf, PathBuf) {
    let base =
        std::env::temp_dir().join(format!("rr3-pack-{}-{}", std::process::id(), fixture_seq()));
    let _ = fs::remove_dir_all(&base);
    let payload = base.join("payload");
    let repo = base.join("repo");
    // 两个根先就位：三件宿主产物与 repo 子目录的写入都依赖它们（批338 红因二的
    // 根因——目录建在循环里，写在循环前的 payload 根没人建）。
    fs::create_dir_all(&payload).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(repo.join("contracts")).unwrap();
    fs::write(payload.join("rrd"), "rrd-bin-bytes").unwrap();
    fs::write(payload.join("rr3-ctl"), "ctl-bin-bytes").unwrap();
    fs::write(payload.join("rr3-ota-ctl"), "ota-ctl-bin-bytes").unwrap();
    let manifest = "[manifest]\nschema_version = 1\nname = \"rr3-act\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"rr3-act\" # 在仓相对\n\n[provides]\ncapabilities = [\"act.servo\"]\n\n[depends]\nrequired = [\"bus.publish\"]\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 1000\nwatchdog_timeout_ms = 5000\n\n[resources]\nrestart_policy = \"always\"\nmem_limit_mb = 16\nstart_priority = 40\n";
    for m in ["rr3-act", "rr3-connect", "rr3-brain"] {
        // 目录随循环建：模块名单是唯一真值，加模块不必记得另补两处 create_dir_all
        // （2026-11-28 批338 红因——python 面退役时漏建 rr3-brain 目录，写 manifest 即 NotFound）。
        fs::create_dir_all(payload.join(format!("rr3-modules/{m}"))).unwrap();
        fs::create_dir_all(repo.join(format!("modules/{m}"))).unwrap();
        fs::write(
            payload.join(format!("rr3-modules/{m}/manifest.toml")),
            manifest.replace("rr3-act", m),
        )
        .unwrap();
        fs::write(
            payload.join(format!("rr3-modules/{m}/bin")),
            format!("{m}-bin"),
        )
        .unwrap();
        fs::write(repo.join(format!("modules/{m}/config.toml")), "[x]\n").unwrap();
    }
    fs::write(repo.join("profiles/distro.toml"), DISTRO_PROFILE).unwrap();
    fs::write(repo.join("contracts/topics.toml"), "# topics\n").unwrap();
    fs::write(repo.join("contracts/slots.toml"), "# slots\n").unwrap();
    // boot 真值集（基-25 清单 files 成员）：roles/devices 契约 + 平台事实表
    //（按装配单 [platform].chip 选册）+ 机型接线档
    fs::write(repo.join("contracts/roles.toml"), "# roles\n").unwrap();
    fs::write(repo.join("contracts/devices.toml"), "# devices\n").unwrap();
    fs::create_dir_all(repo.join("contracts/platform")).unwrap();
    fs::write(
        repo.join("contracts/platform/rk3566.toml"),
        "# platform rk3566\n",
    )
    .unwrap();
    fs::create_dir_all(repo.join("hardware/distro")).unwrap();
    fs::write(repo.join("hardware/distro/wiring.toml"), "# wiring\n").unwrap();
    (payload, repo)
}

pub(super) fn cfg_of(payload: &Path, repo: &Path) -> (Rr3PackConfig, PathBuf) {
    let out = payload.parent().unwrap().join("rockrobot3.deb");
    (
        Rr3PackConfig {
            payload: payload.to_path_buf(),
            repo: repo.to_path_buf(),
            out: out.clone(),
            arch: "arm64".to_string(),
            profile: "distro".to_string(),
            stage_only: true,
            install_root: None,
        },
        out,
    )
}
