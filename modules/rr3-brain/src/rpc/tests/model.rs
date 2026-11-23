// SPDX-License-Identifier: Apache-2.0
//! 模型行面单测（ADR-0023；自 rpc/tests.rs 同判据抽出）。
//!
//! 覆盖行实效重解析、列表快照零密钥、upsert/activate/delete 规则与写面自守。

use crate::boot::{
    rr3_brain_cfg_parse, Rr3BrainCfg, RR3_CFG_TAIL_TOML, RR3_CLOUD_TOML, RR3_MODEL_ROWS_TOML,
};
use crate::rpc::{rr3_model_list, rr3_model_row_of, rr3_model_set_arm, rr3_model_test_arm};
use serde_json::json;

use super::{repo_cfg, write_llm_model};

/// 模型行实效重解析（迁移件三客户端重建的取数口）：live 写后读新值
#[test]
fn model_row_live_write() {
    let mut cfg = repo_cfg();
    write_llm_model(&mut cfg, json!("deepseek-reasoner"));
    let row = rr3_model_row_of(&cfg).unwrap();
    assert_eq!(row.model_id, "deepseek-reasoner", "重解析读 doc 真值");
}

// ---------- 模型行面（ADR-0023） ----------

/// set 臂的真载体：磁盘上一份与 repo_cfg 同文本的 config.toml + 注入路径
fn temp_cfg() -> (std::path::PathBuf, std::path::PathBuf, Rr3BrainCfg) {
    // 无 [rr3-brain] 头：三段常量拼装（与 boot 夹具同一份文本判据）
    let text = format!(
        "{}\n{}\n{}",
        RR3_CLOUD_TOML, RR3_MODEL_ROWS_TOML, RR3_CFG_TAIL_TOML
    );
    let dir = std::env::temp_dir().join(format!(
        "rr3-model-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, &text).unwrap();
    let mut cfg = rr3_brain_cfg_parse(&text).unwrap();
    cfg.config_path = Some(path.clone());
    (dir, path, cfg)
}

/// 列表快照：逐键形状齐备、只回 key_ref/key_set（无密钥明文）、active 点名
#[test]
fn model_rows_secret() {
    let cfg = repo_cfg();
    let v = rr3_model_list(&cfg).unwrap();
    assert_eq!(v["active"], json!("DeepSeek官方"));
    let row = &v["models"][0];
    assert_eq!(row["name"], json!("DeepSeek官方"));
    assert_eq!(row["mode"], json!("api"));
    assert_eq!(row["api_format"], json!("openai"));
    assert_eq!(row["base_url"], json!("https://api.deepseek.com/v1"));
    assert_eq!(row["model_id"], json!("deepseek-chat"));
    assert_eq!(row["key_ref"], json!("DeepSeek官方"));
    assert!(row["key_set"].is_boolean(), "只回布尔，不回密钥值");
    assert_eq!(row["active"], json!(true));
}

/// 空行表：models 空、active = null（cloud fallback 无行可点名）
#[test]
fn model_has_active() {
    let mut cfg = repo_cfg();
    cfg.doc["rr3-brain"]
        .as_object_mut()
        .unwrap()
        .remove("model");
    let v = rr3_model_list(&cfg).unwrap();
    assert_eq!(v, json!({ "models": [], "active": null }));
}

/// upsert：新行落盘（.bak 备份 + 正式文件新值）、内存同步、应答 persisted/live
#[test]
fn model_set_hot_active() {
    let (dir, path, mut cfg) = temp_cfg();
    let reply = rr3_model_set_arm(
        &mut cfg,
        &json!({
            "op": "upsert",
            "name": "GLM",
            "base_url": "https://open.bigmodel.cn/api/paas/v4",
            "model_id": "glm-4.5",
            "api_key_ref": "GLM",
            "active": true,
        }),
    )
    .unwrap();
    assert_eq!(
        reply,
        json!({
            "ok": true,
            "op": "upsert",
            "name": "GLM",
            "active": "GLM",
            "persisted": true,
            "applies": "live",
        })
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("name = \"GLM\""), "{text}");
    assert!(
        text.contains("base_url = \"https://open.bigmodel.cn/api/paas/v4\""),
        "{text}"
    );
    assert_eq!(text.matches("active = true").count(), 1, "{text}");
    // .bak = 旧全量；内存投影同步为新 active 行
    let bak = std::fs::read_to_string(dir.join("config.toml.bak")).unwrap();
    assert!(bak.contains("name = \"DeepSeek官方\""), "{bak}");
    assert!(!bak.contains("name = \"GLM\""), "{bak}");
    assert_eq!(cfg.model.model_id, "glm-4.5");
    let _ = std::fs::remove_dir_all(&dir);
}

/// activate 切行 / active 行拒删 / 删后剩行仍恰一 active
#[test]
fn model_set_rules() {
    let (dir, _path, mut cfg) = temp_cfg();
    rr3_model_set_arm(
        &mut cfg,
        &json!({
            "op": "upsert",
            "name": "Qwen",
            "base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "model_id": "qwen-plus",
            "active": false,
        }),
    )
    .unwrap();
    // active 行拒删（先切换再删）
    let e =
        rr3_model_set_arm(&mut cfg, &json!({"op": "delete", "name": "DeepSeek官方"})).unwrap_err();
    assert!(e.contains("先切换 active 再删"), "{e}");
    // 切换后旧 active 变可删
    rr3_model_set_arm(&mut cfg, &json!({"op": "activate", "name": "Qwen"})).unwrap();
    assert_eq!(cfg.model.model_id, "qwen-plus");
    let reply =
        rr3_model_set_arm(&mut cfg, &json!({"op": "delete", "name": "DeepSeek官方"})).unwrap();
    assert_eq!(reply["active"], json!("Qwen"));
    let v = rr3_model_list(&cfg).unwrap();
    assert_eq!(v["models"].as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 写面自守两红：base_url 非完整 API 根 / 未注入 config 路径（都不动文件）
#[test]
fn model_rej_bad_miss() {
    let (dir, _path, mut cfg) = temp_cfg();
    let e = rr3_model_set_arm(
        &mut cfg,
        &json!({
            "op": "upsert",
            "name": "Bad",
            "base_url": "https://x/v1/chat/completions",
            "model_id": "m",
            "active": false,
        }),
    )
    .unwrap_err();
    assert!(e.contains("不得含 /chat/completions"), "{e}");
    cfg.config_path = None;
    let e = rr3_model_set_arm(
        &mut cfg,
        &json!({
            "op": "upsert",
            "name": "Ok",
            "base_url": "https://x/v1",
            "model_id": "m",
            "active": false,
        }),
    )
    .unwrap_err();
    assert!(e.contains("未接 config 路径"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 连通性测试：mock 档回 ok:true + mock:true（业务失败走 ok:false，不折 Err）
#[test]
fn model_arm_mock_row() {
    // env 纪律：云档是进程级现读值，改档必须持全 crate 的 env 锁并在结尾复位——
    // 此前裸 set_var 不还原，会把并发用例（wake_ack_pcm_reused 等）的 ASR 翻成
    // mock（2026-11-24 Linux/macOS 交替假红的根因）。
    let _g = crate::cloud::testkit::env_lock();
    std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "mock");
    let cfg = repo_cfg();
    let v = rr3_model_test_arm(&cfg, &json!({"name": "DeepSeek官方"})).unwrap();
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["mock"], json!(true));
    let e = rr3_model_test_arm(&cfg, &json!({"name": "不存在"})).unwrap_err();
    assert!(e.contains("模型行不认"), "{e}");
    std::env::remove_var(crate::cloud::RR3_CLOUD_MODE_ENV);
}
