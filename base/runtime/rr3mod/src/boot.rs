//! bootstrap 收敛序列 + config 路径解析单点（0.0.8 方案 §2.4）。
//!
//! 收敛序列（ADR-0017 裁定 1 序列的前四段）：读 RR3_MANIFEST →
//! [`Rr3BusClient::rr3_connect`]（连接/注册/起心跳，rr-bus 既有，含喂狗失败退出
//! 语义）→ [`load_config_text`] → parse_config。返回 `(BusHandle, C)`——config
//! 形状交还装配段（各模块 schema 不同），总线句柄是唯一公共产物。打印
//! "总线就绪: name=… task=…"一行（现行文案逐字保留）。
//!
//! HAL 选择 + 探活不进 bootstrap（归各模块 boot.rs，方案 §2.4 原文）；
//! SURFACE 启动自述也不进（裁定 3：自述留 main）。

use rr_bus_api::Rr3BusClient;

use crate::{BusHandle, Rr3ModConfig, RR3_MANIFEST_ENV};

/// 收敛序列（方案 §2.4）：装配段构造 [`Rr3ModConfig`] 调入；HAL 探活与
/// SURFACE 自述留在模块装配段（见模块头注）。
pub fn bootstrap<C>(cfg: Rr3ModConfig<C>) -> Result<(BusHandle, C), String> {
    let manifest_path = std::env::var(RR3_MANIFEST_ENV).map_err(|_| {
        format!("缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    })?;
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读 manifest {manifest_path} 失败: {e}"))?;
    let client = Rr3BusClient::rr3_connect(&manifest_text)?;
    eprintln!("总线就绪: name={} task={}", client.rr3_name(), client.rr3_task());
    let text = load_config_text(cfg.config_env, &manifest_path)?;
    let parsed = (cfg.parse_config)(&text)?;
    Ok((BusHandle { client }, parsed))
}

/// config 路径解析单点（方案 §2.4）：`config_env` 设了读 env 路径；未设读
/// manifest 同目录 config.toml。读失败 Err（文案沿用现行形状：
/// "读 config {path} 失败: {e}"）。rr3-act / rr3-connect / rr3-power 三处逐字
/// 重复的路径解析随批0 单点化（模块侧随批1–批5 改调本函数）。
pub fn load_config_text(
    config_env: Option<&'static str>,
    manifest_path: &str,
) -> Result<String, String> {
    let path = rr3_config_path(config_env, manifest_path);
    std::fs::read_to_string(&path).map_err(|e| format!("读 config {path} 失败: {e}"))
}

/// config 路径投影（批N28 批3 扩展，与 [`load_config_text`] 同一解析单点）：
/// 返回解析出的 config 路径本身。sense 装配段「config 就绪: 来源={path}」回显
/// 消费——`parse_config` 是 fn 指针拿不到 path，投影是唯一不重复解析、不改
/// 逐字文案的通路（新增不改旧签名，power/connect 已落地面零返工）。
pub fn rr3_config_path(config_env: Option<&'static str>, manifest_path: &str) -> String {
    match config_env {
        Some(env) => std::env::var(env).unwrap_or_else(|_| rr3_config_default_path(manifest_path)),
        None => rr3_config_default_path(manifest_path),
    }
}

/// 缺省路径：manifest 同目录 config.toml（现行三模块逐字形状，
/// rr3-act composite `rr3_config_load` 同款）。
fn rr3_config_default_path(manifest_path: &str) -> String {
    std::path::Path::new(manifest_path)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("config.toml")
        .to_string_lossy()
        .into_owned()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::{env_lock, rr3_tmp_dir};

    /// 方案 §2.8-1 用例一：env 覆盖优先（config_env 设了且 env 在，读 env 路径，
    /// 不碰 manifest 同目录）。
    #[test]
    fn load_config_env_overrides_manifest_dir() {
        let _g = env_lock().lock().unwrap();
        let dir = rr3_tmp_dir("cfg-env");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("manifest.toml");
        std::fs::write(&manifest, "[manifest]\nname = \"x\"\n").unwrap();
        std::fs::write(dir.join("config.toml"), "from = \"dir\"\n").unwrap();
        let alt = dir.join("override.toml");
        std::fs::write(&alt, "from = \"env\"\n").unwrap();
        std::env::set_var("RR3MOD_TEST_CFG_ENV_A", &alt);
        let text =
            load_config_text(Some("RR3MOD_TEST_CFG_ENV_A"), manifest.to_str().unwrap()).unwrap();
        assert_eq!(text, "from = \"env\"\n", "env 路径覆盖 manifest 同目录");
        std::env::remove_var("RR3MOD_TEST_CFG_ENV_A");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 方案 §2.8-1 用例二：未设 env（None）走 manifest 同目录 config.toml。
    #[test]
    fn load_config_falls_back_to_manifest_dir() {
        let dir = rr3_tmp_dir("cfg-fallback");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("manifest.toml");
        std::fs::write(&manifest, "[manifest]\nname = \"x\"\n").unwrap();
        std::fs::write(dir.join("config.toml"), "from = \"dir\"\n").unwrap();
        let text = load_config_text(None, manifest.to_str().unwrap()).unwrap();
        assert_eq!(text, "from = \"dir\"\n", "缺省读 manifest 同目录");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 方案 §2.8-1 用例三：读失败文案点名路径（现行形状"读 config {path} 失败"；
    /// io error 尾巴是动态文案，按 repo 口径 contains 断言）。
    #[test]
    fn load_config_missing_file_names_path() {
        let err =
            load_config_text(None, "/nonexistent-rr3mod/manifest.toml").unwrap_err();
        assert!(err.starts_with("读 config "), "{err}");
        assert!(
            err.contains("/nonexistent-rr3mod/config.toml"),
            "文案须点名解析出的路径: {err}"
        );
    }

    /// 批N28 批3：`rr3_config_path` 投影与 `load_config_text` 实读路径一致
    /// （缺省/env 覆盖两例）——投影单点漂移当场红。
    #[test]
    fn config_path_projection_matches_load_config_text() {
        let _g = env_lock().lock().unwrap();
        let dir = rr3_tmp_dir("cfg-proj");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("manifest.toml");
        std::fs::write(&manifest, "[manifest]\nname = \"x\"\n").unwrap();
        std::fs::write(dir.join("config.toml"), "from = \"dir\"\n").unwrap();
        // 缺省：投影 = manifest 同目录 config.toml（实读同路径）
        let p = rr3_config_path(None, manifest.to_str().unwrap());
        assert_eq!(p, dir.join("config.toml").to_string_lossy());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "from = \"dir\"\n");
        // env 覆盖：投影 = env 路径（实读同路径）
        let alt = dir.join("override.toml");
        std::fs::write(&alt, "from = \"env\"\n").unwrap();
        std::env::set_var("RR3MOD_TEST_CFG_ENV_P", &alt);
        let p = rr3_config_path(Some("RR3MOD_TEST_CFG_ENV_P"), manifest.to_str().unwrap());
        assert_eq!(p, alt.to_string_lossy());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "from = \"env\"\n");
        std::env::remove_var("RR3MOD_TEST_CFG_ENV_P");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 方案 §2.8-3：缺 RR3_MANIFEST 错误文案逐字断言（现行形状）。
    #[test]
    fn bootstrap_errors_without_manifest_env() {
        let _g = env_lock().lock().unwrap();
        std::env::remove_var(crate::RR3_MANIFEST_ENV);
        // BusHandle 无 Debug（Rr3BusClient 不 derive），unwrap_err 用不了——match 取 Err
        let err = match bootstrap::<()>(Rr3ModConfig {
            config_env: None,
            parse_config: |_| Ok(()),
        }) {
            Err(e) => e,
            Ok(_) => panic!("缺 RR3_MANIFEST 应失败"),
        };
        assert_eq!(
            err,
            "缺环境变量 RR3_MANIFEST（rrd spawn 注入；直跑请指向 module.toml）",
            "缺 manifest 文案逐字（方案 §2.8-3）"
        );
    }
}
