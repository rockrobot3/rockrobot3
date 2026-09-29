use std::path::{Path, PathBuf};
use rr3_mkmod::rr3_mkmod_generate;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rr3-mkmod-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// 把真实契约目录软接进临时根，让生成物能做交叉校验（Windows 无软链接 → 复制）
fn with_contract(root: &Path) {
    let contracts = root.join("contracts");
    std::fs::create_dir_all(&contracts).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/topics.toml"),
        contracts.join("topics.toml"),
    )
    .unwrap();
}

#[test]
fn rr3_mkmod_rejects_bad_name() {
    let root = tmp("bad");
    // 方案 §4：manifest.name 必须匹配 ^rr3-[a-z]+$，词干只许小写字母
    for n in ["Sense", "rr3-", "rr3-a-b", "mod_audio", "rr3-demo2", "rr3-1x"] {
        assert!(
            matches!(
                rr3_mkmod_generate(n, &root),
                Err(rr3_mkmod::Rr3MkmodError::InvalidName(_))
            ),
            "{n} 必须被拒"
        );
    }
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn rr3_mkmod_creates_five_files() {
    let root = tmp("files");
    with_contract(&root);
    let made = rr3_mkmod_generate("rr3-demo", &root).expect("生成失败");
    let names: Vec<String> = made
        .iter()
        .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    for want in [
        "rr3-demo/module.toml",
        "rr3-demo/config.toml",
        "rr3-demo/Cargo.toml",
        "rr3-demo/src/main.rs",
        "rr3-demo/README.md",
    ] {
        assert!(names.iter().any(|n| n == want), "缺少 {want}，实际 {names:?}");
    }
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn rr3_mkmod_generated_manifest_passes_contract() {
    let root = tmp("manifest");
    with_contract(&root);
    rr3_mkmod_generate("rr3-tmpdemo", &root).unwrap();
    let report = rr_registry::rr3_registry_validate_manifest(
        &root.join("rr3-tmpdemo/module.toml"),
    );
    assert!(report.ok(), "生成的 manifest 必须直接合规: {:?}", report.errors);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn rr3_mkmod_refuses_existing_dir() {
    let root = tmp("dup");
    with_contract(&root);
    rr3_mkmod_generate("rr3-demo", &root).unwrap();
    assert!(matches!(
        rr3_mkmod_generate("rr3-demo", &root),
        Err(rr3_mkmod::Rr3MkmodError::AlreadyExists(_))
    ));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn rr3_mkmod_accepts_short_name_and_stamps_manifest_fields() {
    let root = tmp("short");
    with_contract(&root);
    rr3_mkmod_generate("arm", &root).unwrap();
    let text = std::fs::read_to_string(root.join("rr3-arm/module.toml")).unwrap();
    assert!(text.contains("name = \"rr3-arm\""), "{text}");
    assert!(text.contains("type = \"module\""), "{text}");
    assert!(text.contains("lang = \"rust\""), "{text}");
    assert!(text.contains("schema_version = 1"), "{text}");
    assert!(text.contains("watchdog_timeout_ms = 10000"), "{text}");
    std::fs::remove_dir_all(&root).ok();
}
