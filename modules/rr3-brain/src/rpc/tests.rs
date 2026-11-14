use super::*;
use crate::boot::{
    rr3_brain_cfg_parse, rr3_repo_cfg_fixture, Rr3BrainCfg, RR3_CFG_TAIL_TOML, RR3_CLOUD_TOML,
    RR3_MODEL_ROWS_TOML,
};
use serde_json::{json, Value};

/// 与仓内 config.toml 同形状的文档树夹具（boot 解析产出，写面真载体）
fn repo_cfg() -> Rr3BrainCfg {
    rr3_brain_cfg_parse(&rr3_repo_cfg_fixture()).unwrap()
}

/// live 写模型行 model_id 的公共底座（快照不回填 vs doc 真值两用例同款）
fn write_llm_model(cfg: &mut Rr3BrainCfg, v: serde_json::Value) {
    rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &v).unwrap();
}

// ---------- ops/list 读面 ----------

/// 白名单组体：四组六槽逐键在册、**零多余键**（密钥/api_key_ref 不进应答，
/// B-12 值零回显守脑面）
#[test]
fn ops_snapshot_whitelist_groups_exactly() {
    let cfg = repo_cfg();
    let v = rr3_ops_snapshot(&cfg).unwrap();
    assert_eq!(
        v,
        json!({
            "asr": {
                "asr_model": "iic/SenseVoiceSmall",
                "base_url": "https://api.siliconflow.cn",
                "timeout_s": 20,
            },
            "llm": {
                "llm_model": "deepseek-chat",
                "base_url": "https://api.deepseek.com/v1",
                "timeout_s": 20,
            },
            "tts": {
                "tts_model": "FunAudioLLM/CosyVoice2-0.5B",
                "base_url": "https://api.siliconflow.cn",
                "timeout_s": 20,
            },
            "dialog": {
                "max_utterance_ms": 8000,
            },
        }),
        "四组六槽逐键逐字（timeout_s 是 TOML 整数 20 的原样透传）"
    );
}

/// dialog 键缺席 → 快照缺省 8000（Python .get("max_utterance_ms", 8000) 同形状）
#[test]
fn ops_snapshot_dialog_defaults_8000_when_key_absent() {
    let mut cfg = repo_cfg();
    cfg.doc["rr3-brain"]
        .as_object_mut()
        .unwrap()
        .remove("dialog");
    // boot 面要求 dialog 段必填，此处直造写面载体测读面缺省路径
    let v = rr3_ops_snapshot(&cfg).unwrap();
    assert_eq!(v["dialog"]["max_utterance_ms"], 8000);
}

/// llm 组回显随 boot 投影（Python base_url/model_id 局部变量同形状）：live 写
/// 模型行后快照**不回填**、重启回新值——B-14① 双源分裂的如实镜像，本测钉死
#[test]
fn ops_snapshot_llm_group_stays_at_boot_projection_after_live_write() {
    let mut cfg = repo_cfg();
    // live 写模型行 model_id（写源 = active 行）
    write_llm_model(&mut cfg, json!("deepseek-reasoner"));
    let v = rr3_ops_snapshot(&cfg).unwrap();
    assert_eq!(
        v["llm"]["llm_model"],
        json!("deepseek-chat"),
        "快照回显 boot 投影，live 写不回填（Python 同形状）"
    );
    assert_eq!(
        cfg.doc["rr3-brain"]["model"][0]["model_id"],
        json!("deepseek-reasoner"),
        "doc 真值已写（客户端重建/重启面读新值）"
    );
}

/// asr/tts/dialog 组快照随 live 写即见（读源在 cloud/dialog 段）
#[test]
fn ops_snapshot_asr_tts_dialog_reflect_live_writes() {
    let mut cfg = repo_cfg();
    rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!("new-asr")).unwrap();
    rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
    let v = rr3_ops_snapshot(&cfg).unwrap();
    assert_eq!(v["asr"]["asr_model"], json!("new-asr"));
    assert_eq!(v["dialog"]["max_utterance_ms"], 9000);
}

// ---------- ops/set 写面 ----------

/// 组/槽白名单两红（文案逐字对位 Python，含 Python 列表 repr 形状）
#[test]
fn ops_set_rejects_unknown_group_and_slot() {
    let mut cfg = repo_cfg();
    let e = rr3_ops_write(&mut cfg.doc, "memory", "x", &json!(1)).unwrap_err();
    assert_eq!(
        e, "组不认: 'memory'（白名单 ['asr', 'dialog', 'llm', 'tts']）",
        "组不认文案逐字"
    );
    let e = rr3_ops_write(&mut cfg.doc, "asr", "sample_rate", &json!(1)).unwrap_err();
    assert_eq!(
        e, "组 asr 槽不认: 'sample_rate'（白名单 ['asr_model', 'base_url', 'timeout_s']）",
        "槽不认文案逐字"
    );
}

/// 字符串槽三径：非串点名（Python 类型词）、空白拒、strip 落值
#[test]
fn ops_set_string_slots_validate() {
    let mut cfg = repo_cfg();
    let e = rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!(42)).unwrap_err();
    assert_eq!(
        e, "槽 asr.asr_model 须字符串，收到 int",
        "类型词 Python 对位"
    );
    let e = rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!(null)).unwrap_err();
    assert_eq!(e, "槽 asr.asr_model 须字符串，收到 NoneType");
    let e = rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!("   ")).unwrap_err();
    assert_eq!(e, "槽 asr.asr_model 不得为空", "strip 后空拒");
    let (old, new) =
        rr3_ops_write(&mut cfg.doc, "asr", "asr_model", &json!("  new-asr  ")).unwrap();
    assert_eq!(old, json!("iic/SenseVoiceSmall"));
    assert_eq!(new, json!("new-asr"), "strip 落值");
    assert_eq!(
        cfg.doc["rr3-brain"]["cloud"]["asr_model"],
        json!("new-asr"),
        "写源 = cloud 段"
    );
}

/// timeout_s 槽：布尔拒（先于数值——Python isinstance(bool) 同序）、非数点名、
/// ≤0 拒、数值转 float 落盘（写源 = cloud 段，四组任一组的 timeout_s 同路）
#[test]
fn ops_set_timeout_s_validates_and_writes_cloud() {
    let mut cfg = repo_cfg();
    let e = rr3_ops_write(&mut cfg.doc, "asr", "timeout_s", &json!(true)).unwrap_err();
    assert_eq!(
        e, "槽 asr.timeout_s 须数值，收到 bool",
        "布尔先拒（Python 同序）"
    );
    let e = rr3_ops_write(&mut cfg.doc, "llm", "timeout_s", &json!("30")).unwrap_err();
    assert_eq!(e, "槽 llm.timeout_s 须数值，收到 str");
    let e = rr3_ops_write(&mut cfg.doc, "tts", "timeout_s", &json!(0)).unwrap_err();
    assert_eq!(
        e, "槽 tts.timeout_s 须 >0，收到 0.0",
        ">0 执法（Python float 形状）"
    );
    let e = rr3_ops_write(&mut cfg.doc, "tts", "timeout_s", &json!(-1.5)).unwrap_err();
    assert_eq!(e, "槽 tts.timeout_s 须 >0，收到 -1.5");
    let (old, new) = rr3_ops_write(&mut cfg.doc, "llm", "timeout_s", &json!(25)).unwrap();
    assert_eq!(old, json!(20), "old 是 doc 原值（TOML 整数 20）");
    assert_eq!(
        new,
        json!(25.0),
        "new 转 float（Python float(value) 同形状）"
    );
    assert_eq!(
        cfg.doc["rr3-brain"]["cloud"]["timeout_s"],
        json!(25.0),
        "llm 组的 timeout_s 也写 cloud 段（写源映射；dialog 组白名单无 timeout_s）"
    );
}

/// max_utterance_ms 槽：布尔拒、非整数拒、≤0 拒、整数落盘（写源 = dialog 段）
#[test]
fn ops_set_max_utterance_validates_and_writes_dialog() {
    let mut cfg = repo_cfg();
    let e = rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(false)).unwrap_err();
    assert_eq!(e, "槽 dialog.max_utterance_ms 须数值，收到 bool");
    let e = rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(8000.5)).unwrap_err();
    assert_eq!(
        e, "槽 dialog.max_utterance_ms 须整数，收到 8000.5",
        "小数拒"
    );
    let e = rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(0)).unwrap_err();
    assert_eq!(e, "槽 dialog.max_utterance_ms 须 >0，收到 0");
    let (old, new) =
        rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
    assert_eq!(old, json!(8000));
    assert_eq!(new, json!(9000));
    assert_eq!(
        cfg.doc["rr3-brain"]["dialog"]["max_utterance_ms"],
        json!(9000),
        "写源 = dialog 段"
    );
}

/// old 键缺席 = Null（Python dict.get 同形状）：dialog 段无键时首写 old=null
#[test]
fn ops_set_old_is_null_when_key_absent() {
    let mut cfg = repo_cfg();
    cfg.doc["rr3-brain"]["dialog"]
        .as_object_mut()
        .unwrap()
        .remove("max_utterance_ms");
    let (old, new) =
        rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
    assert_eq!(
        old,
        Value::Null,
        "键缺席 old = null（Python dict.get 同形状）"
    );
    assert_eq!(new, json!(9000));
}

/// dialog 段整段缺席 → setdefault 建段后照写（Python setdefault 同义）
#[test]
fn ops_set_creates_missing_dialog_section() {
    let mut cfg = repo_cfg();
    cfg.doc["rr3-brain"]
        .as_object_mut()
        .unwrap()
        .remove("dialog");
    rr3_ops_write(&mut cfg.doc, "dialog", "max_utterance_ms", &json!(9000)).unwrap();
    assert_eq!(
        cfg.doc["rr3-brain"]["dialog"]["max_utterance_ms"],
        json!(9000),
        "缺段建段照写"
    );
}

/// llm 组写源两态：行表在 → 写 active 行（llm_model → model_id 键名换算）；
/// 行表缺 → 写 cloud 段（fallback 路径）
#[test]
fn ops_set_llm_write_source_two_states() {
    let mut cfg = repo_cfg();
    let (old, new) = rr3_ops_write(
        &mut cfg.doc,
        "llm",
        "llm_model",
        &json!("deepseek-reasoner"),
    )
    .unwrap();
    assert_eq!(old, json!("deepseek-chat"));
    assert_eq!(new, json!("deepseek-reasoner"));
    assert_eq!(
        cfg.doc["rr3-brain"]["model"][0]["model_id"],
        json!("deepseek-reasoner"),
        "行表在 → 写 active 行 model_id 键"
    );
    assert_eq!(
        cfg.doc["rr3-brain"]["cloud"]["llm_model"],
        json!("deepseek-ai/DeepSeek-V3"),
        "cloud 段不动（实效源在行）"
    );
    // 行表缺 → cloud fallback
    let mut cfg = repo_cfg();
    cfg.doc["rr3-brain"]
        .as_object_mut()
        .unwrap()
        .remove("model");
    let (_, new) = rr3_ops_write(&mut cfg.doc, "llm", "llm_model", &json!("new-llm")).unwrap();
    assert_eq!(new, json!("new-llm"));
    assert_eq!(
        cfg.doc["rr3-brain"]["cloud"]["llm_model"],
        json!("new-llm"),
        "行表缺 → 写 cloud 段"
    );
}

/// 行表结构坏（active 数≠1）写面点名拒（"写面不修结构错误"——直造文档树，
/// 绕开 boot 校验，钉写面自身防线）
#[test]
fn ops_set_llm_rejects_broken_row_structure() {
    let mut doc: Value = serde_json::from_str(
        r#"{"rr3-brain": {"cloud": {"base_url": "u"}, "model": [
            {"name": "甲", "active": true}, {"name": "乙", "active": true}]}}"#,
    )
    .unwrap();
    let e = rr3_ops_write(&mut doc, "llm", "llm_model", &json!("x")).unwrap_err();
    assert_eq!(
        e, "[[rr3-brain.model]] 须恰一行 active=true（现 2 行）——写面不修结构错误",
        "两 active 行结构坏点名逐字（单 active 是合法结构会照写）"
    );
}

/// ops/set 臂应答六键体（ok/group/slot/old/new/persisted/applies——live 内存
/// 生效不落盘如实申报）
#[test]
fn ops_set_arm_reply_shape() {
    let mut cfg = repo_cfg();
    let v = rr3_ops_set_arm(
        &mut cfg,
        &json!({"group": "dialog", "slot": "max_utterance_ms", "value": 9000}),
    )
    .unwrap();
    assert_eq!(
        v,
        json!({
            "ok": true,
            "group": "dialog",
            "slot": "max_utterance_ms",
            "old": 8000,
            "new": 9000,
            "persisted": false,
            "applies": "live",
        }),
        "六键应答体逐字（persisted:false 不落盘如实申报）"
    );
    // 业务失败走 Err（serve 折 {"error": …} 应答——Python try/except 同 wire）
    let e =
        rr3_ops_set_arm(&mut cfg, &json!({"group": "nope", "slot": "x", "value": 1})).unwrap_err();
    assert!(e.contains("组不认"), "违规走 Err: {e}");
}

/// group/slot 的 str() coercion（Python str(args.get(key, "")) 同形状）：缺字段
/// 回落空串 → 组不认点名
#[test]
fn ops_set_arm_coerces_group_slot_like_python_str() {
    let mut cfg = repo_cfg();
    let e = rr3_ops_set_arm(&mut cfg, &json!({})).unwrap_err();
    assert_eq!(
        e, "组不认: ''（白名单 ['asr', 'dialog', 'llm', 'tts']）",
        "缺字段回落空串同形状"
    );
    let e = rr3_ops_set_arm(&mut cfg, &json!({"group": 5, "slot": null})).unwrap_err();
    assert_eq!(
        e, "组不认: '5'（白名单 ['asr', 'dialog', 'llm', 'tts']）",
        "str(5)='5' coercion 同形状"
    );
}

/// 模型行实效重解析（迁移件三客户端重建的取数口）：live 写后读新值
#[test]
fn model_row_of_reflects_live_write() {
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
fn model_list_snapshot_reports_rows_without_secret() {
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
fn model_list_snapshot_empty_table_has_null_active() {
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
fn model_set_upsert_persists_and_hot_swaps_active() {
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
fn model_set_activate_and_delete_rules() {
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
fn model_set_rejects_bad_root_and_missing_path() {
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
fn model_test_arm_mock_and_unknown_row() {
    std::env::set_var(crate::cloud::RR3_CLOUD_MODE_ENV, "mock");
    let cfg = repo_cfg();
    let v = rr3_model_test_arm(&cfg, &json!({"name": "DeepSeek官方"})).unwrap();
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["mock"], json!(true));
    let e = rr3_model_test_arm(&cfg, &json!({"name": "不存在"})).unwrap_err();
    assert!(e.contains("模型行不认"), "{e}");
}
