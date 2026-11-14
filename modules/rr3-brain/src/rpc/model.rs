//! 模型行面（ADR-0023）：行表快照、CRUD 落盘、连通性探测。

use serde_json::{json, Value};

use crate::boot::{
    rr3_base_url_check, rr3_brain_cfg_parse, rr3_cloud_cfg_of, Rr3BrainCfg, Rr3LlmMode,
};
use crate::cloud::{rr3_key_set_of, Rr3LlmClient};
use crate::config_store::{rr3_block_rewrite, rr3_config_persist};

// ---------- 模型行面（ADR-0023：面板 CRUD + 切换 active + 连通性测试） ----------

/// 模型行表快照（doc 内，缺表 = 空表；非数组点名拒——与 boot 同一文案）
fn rr3_model_rows(doc: &Value) -> Result<Vec<Value>, String> {
    match doc.get("rr3-brain").and_then(|b| b.get("model")) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(rows)) => Ok(rows.clone()),
        Some(_) => Err("config [[rr3-brain.model]] 须为行表（数组）".to_string()),
    }
}

/// 行内字符串字段（缺键/Null 回落空串；非串点名行号与键名）
fn rr3_row_str<'a>(row: &'a Value, key: &str, idx: usize) -> Result<&'a str, String> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(""),
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(_) => Err(format!("模型行 #{idx}.{key} 须字符串")),
    }
}

/// 表内 active 行名（无 → None：空表走 cloud fallback，没有行可点名）
fn rr3_row_active_name(rows: &[Value]) -> Option<String> {
    rows.iter()
        .find(|r| r.get("active").and_then(Value::as_bool) == Some(true))
        .and_then(|r| r.get("name").and_then(Value::as_str))
        .map(|s| s.to_string())
}

/// 模型行列表（`rr3-brain/model/list` 应答体）：每行回 名称/API 根/模型/格式/
/// 模式/key_ref/key 是否就位/active——**只回 `key_set` 布尔，永无密钥明文**。
pub(crate) fn rr3_model_list(cfg: &Rr3BrainCfg) -> Result<Value, String> {
    let rows = rr3_model_rows(&cfg.doc)?;
    let models = rows
        .iter()
        .enumerate()
        .map(|(i, row)| rr3_model_row(row, i))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({ "models": models, "active": rr3_row_active_name(&rows) }))
}

/// 单行投影（B9 自 list 抽出）：名称/API 根/模型/格式/模式/key_ref/key 就位/
/// active——只回 `key_set` 布尔，永无密钥明文。
fn rr3_model_row(row: &Value, i: usize) -> Result<Value, String> {
    let v = rr3_row_view(row, i)?;
    let mode_enum = Some(if v.mode == "local" {
        Rr3LlmMode::Local
    } else {
        Rr3LlmMode::Api
    });
    Ok(json!({
        "name": v.name,
        "base_url": rr3_row_str(row, "base_url", i)?,
        "model_id": rr3_row_str(row, "model_id", i)?,
        "api_format": v.api_format,
        "mode": v.mode,
        "key_ref": v.key_ref,
        "key_set": rr3_key_set_of(&v.key_ref, mode_enum),
        "active": row.get("active").and_then(Value::as_bool).unwrap_or(false),
    }))
}

/// 行视图（B9 自 model_row 抽出）：name/mode/api_format 的缺省归一与 key_ref
/// 三态读取同址——投影面只做拼装。
struct Rr3RowView<'a> {
    name: &'a str,
    mode: &'a str,
    api_format: &'a str,
    key_ref: Option<String>,
}

fn rr3_row_view<'a>(row: &'a Value, i: usize) -> Result<Rr3RowView<'a>, String> {
    let name = rr3_row_str(row, "name", i)?;
    if name.trim().is_empty() {
        return Err(format!("模型行 #{i} 缺 name"));
    }
    let mode = rr3_row_str(row, "mode", i)?.trim();
    let mode = if mode.is_empty() { "api" } else { mode };
    let api_format = rr3_row_str(row, "api_format", i)?.trim();
    let api_format = if api_format.is_empty() {
        "openai"
    } else {
        api_format
    };
    let key_ref = Some(rr3_row_str(row, "api_key_ref", i)?.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    Ok(Rr3RowView {
        name,
        mode,
        api_format,
        key_ref,
    })
}

/// upsert 入参（一次解析；`key_ref` 三态：缺席 = 保留现值、空串 = 清除、
/// 有值 = 校验后写——`key_ref_clear` 承载"空串清除"这一态）。
struct Rr3ModelRowInput<'a> {
    base_url: &'a str,
    model_id: &'a str,
    api_format: &'a str,
    mode: &'a str,
    key_ref: Option<String>,
    key_ref_clear: bool,
    active: Option<bool>,
}

/// `api_format` 词表核（缺省 openai；本批仅 openai）。
fn rr3_api_format_of<'a>(name: &str, args: &'a Value) -> Result<&'a str, String> {
    let api_format = args
        .get("api_format")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("openai");
    if api_format != "openai" {
        return Err(format!(
            "模型行 {name} api_format 暂不支持: {api_format}（本批仅 openai）"
        ));
    }
    Ok(api_format)
}

/// `mode` 词表核（缺省 api；允许 api/local）。
fn rr3_mode_of<'a>(name: &str, args: &'a Value) -> Result<&'a str, String> {
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("api");
    if mode != "api" && mode != "local" {
        return Err(format!("模型行 {name} mode 不认: {mode}（允许 api/local）"));
    }
    Ok(mode)
}

/// `api_key_ref` 三态核：缺席 = 保留现值（None,false）；空串 = 清除（None,true）；
/// 有值 = 无空白且 `env:` 前缀后是合法变量名后写入（Some(s),false）。
fn rr3_key_ref_input(name: &str, args: &Value) -> Result<(Option<String>, bool), String> {
    let Some(v) = args.get("api_key_ref") else {
        return Ok((None, false));
    };
    let s = v
        .as_str()
        .map(str::trim)
        .ok_or_else(|| format!("模型行 {name} api_key_ref 须字符串"))?;
    if s.is_empty() {
        return Ok((None, true));
    }
    if s.chars().any(char::is_whitespace) {
        return Err(format!("模型行 {name} api_key_ref 不得含空白: '{s}'"));
    }
    if s.strip_prefix(crate::cloud::RR3_KEY_REF_ENV_PREFIX)
        .is_some_and(|var| var.is_empty() || var.chars().any(char::is_whitespace))
    {
        return Err(format!(
            "模型行 {name} api_key_ref 的 env: 后须变量名（非空、无空白）: '{s}'"
        ));
    }
    Ok((Some(s.to_string()), false))
}

/// 模型行进参解析：必填/域名/键引用/active 逐项按旧文案 fail-closed。
fn rr3_model_row_input<'a>(name: &str, args: &'a Value) -> Result<Rr3ModelRowInput<'a>, String> {
    let base_url = args
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let model_id = args
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if base_url.is_empty() || model_id.is_empty() {
        return Err("upsert 须同时给 base_url 与 model_id（非空）".to_string());
    }
    rr3_base_url_check(base_url).map_err(|e| format!("模型行 {name} {e}"))?;
    let api_format = rr3_api_format_of(name, args)?;
    let mode = rr3_mode_of(name, args)?;
    let (key_ref, key_ref_clear) = rr3_key_ref_input(name, args)?;
    let active = args
        .get("active")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| format!("模型行 {name} active 须布尔"))
        })
        .transpose()?;
    Ok(Rr3ModelRowInput {
        base_url,
        model_id,
        api_format,
        mode,
        key_ref,
        key_ref_clear,
        active,
    })
}

/// upsert 一行（同名即更新；新行在表内无 active 时自动接任）
fn rr3_model_upsert(rows: &mut Vec<Value>, name: &str, args: &Value) -> Result<(), String> {
    let inp = rr3_model_row_input(name, args)?;
    let (i, is_new) = rr3_row_slot(rows, name);
    let others_active = rows
        .iter()
        .enumerate()
        .any(|(j, r)| j != i && r.get("active").and_then(Value::as_bool) == Some(true));
    let prev_active = rows[i].get("active").and_then(Value::as_bool) == Some(true);
    let active = inp.active.unwrap_or(if is_new {
        !others_active // 表内无 active（空表/结构坏）→ 新行接任
    } else {
        prev_active
    });
    rr3_row_fill(rows, i, name, &inp, active)?;
    if active {
        rr3_row_deactivate(rows, i);
    }
    Ok(())
}

/// 行槽定位（B9 自 upsert 抽出）：同名行下标；缺席即补空行、返回 (下标, 是否新增)
fn rr3_row_slot(rows: &mut Vec<Value>, name: &str) -> (usize, bool) {
    let Some(i) = rows
        .iter()
        .position(|r| r.get("name").and_then(Value::as_str) == Some(name))
    else {
        return (rr3_row_push(rows), true);
    };
    (i, false)
}

/// 补空行并回其下标（upsert 新行落位）。
fn rr3_row_push(rows: &mut Vec<Value>) -> usize {
    rows.push(json!({}));
    rows.len() - 1
}

/// 行写入（B9 自 upsert 抽出）：六键 + key_ref 三态（有值写/空串清/缺席保留）
fn rr3_row_fill(
    rows: &mut [Value],
    i: usize,
    name: &str,
    inp: &Rr3ModelRowInput<'_>,
    active: bool,
) -> Result<(), String> {
    let obj = rows[i]
        .as_object_mut()
        .ok_or_else(|| format!("模型行 #{i} 须为表（TOML 行表同构）"))?;
    rr3_row_put(obj, name, inp, active);
    Ok(())
}

/// 行内六键落位（key_ref 三态随 `inp`，active 由调用方裁定）。
fn rr3_row_put(
    obj: &mut serde_json::Map<String, Value>,
    name: &str,
    inp: &Rr3ModelRowInput<'_>,
    active: bool,
) {
    obj.insert("name".to_string(), json!(name));
    obj.insert("mode".to_string(), json!(inp.mode));
    obj.insert("api_format".to_string(), json!(inp.api_format));
    obj.insert("base_url".to_string(), json!(inp.base_url));
    obj.insert("model_id".to_string(), json!(inp.model_id));
    rr3_row_key_ref(obj, inp);
    obj.insert("active".to_string(), json!(active));
}

/// key_ref 三态：有值写 / 空串清 / 缺席保留。
fn rr3_row_key_ref(obj: &mut serde_json::Map<String, Value>, inp: &Rr3ModelRowInput<'_>) {
    if let Some(k) = &inp.key_ref {
        obj.insert("api_key_ref".to_string(), json!(k));
    } else if inp.key_ref_clear {
        obj.remove("api_key_ref");
    }
}

/// active 唯一性（B9 自 upsert 抽出）：新 active 行就位后其余行一律置 false
fn rr3_row_deactivate(rows: &mut [Value], keep: usize) {
    for (j, r) in rows.iter_mut().enumerate() {
        if j != keep {
            rr3_row_clear(r);
        }
    }
}

/// 单行 active 清零。
fn rr3_row_clear(r: &mut Value) {
    if let Some(o) = r.as_object_mut() {
        o.insert("active".to_string(), json!(false));
    }
}

/// 切换 active（其余行一律置 false）
fn rr3_model_activate(rows: &mut [Value], name: &str) -> Result<(), String> {
    let Some(i) = rows
        .iter()
        .position(|r| r.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Err(format!("模型行不认: '{name}'"));
    };
    rr3_rows_activate(rows, i);
    Ok(())
}

/// active 唯一性（切换语义）：仅 `keep` 行为 true，其余一律置 false。
fn rr3_rows_activate(rows: &mut [Value], keep: usize) {
    for (j, r) in rows.iter_mut().enumerate() {
        if let Some(o) = r.as_object_mut() {
            o.insert("active".to_string(), json!(j == keep));
        }
    }
}

/// 删除非 active 行（active 行拒删——先切换再删，保证表内始终有连接）
fn rr3_model_delete(rows: &mut Vec<Value>, name: &str) -> Result<(), String> {
    let Some(i) = rows
        .iter()
        .position(|r| r.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Err(format!("模型行不认: '{name}'"));
    };
    if rows[i].get("active").and_then(Value::as_bool) == Some(true) {
        return Err(format!("模型行 {name} 是 active，先切换 active 再删"));
    }
    rows.remove(i);
    Ok(())
}

/// 模型行 CRUD 写臂（`rr3-brain/model/set`）：克隆行表改 → 渲染块 → 解析验证 →
/// 原子落盘（`.bak` + rename）→ 提交内存。任一步失败：内存与旧文件都不变。
pub(crate) fn rr3_model_set_arm(cfg: &mut Rr3BrainCfg, args: &Value) -> Result<Value, String> {
    let (op, name) = rr3_model_pick(args)?;
    let rows = rr3_model_rows(&cfg.doc)?;
    rr3_model_write(cfg, rows, op, name, args)
}

/// 写臂：落改动 → 结构门（写面自守，不依赖 boot 兜底：恰一 active）→ 提交。
fn rr3_model_write(
    cfg: &mut Rr3BrainCfg,
    mut rows: Vec<Value>,
    op: &str,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    rr3_model_op_apply(&mut rows, op, name, args)?;
    rr3_model_one_active(&rows)?;
    rr3_model_commit(cfg, rows, op, name)
}

/// 写面取参：op/name 去空白（缺省空串），空名点名拒。
fn rr3_model_pick(args: &Value) -> Result<(&str, &str), String> {
    let op = args
        .get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() {
        return Err("模型行 name 不得为空".to_string());
    }
    Ok((op, name))
}

/// op 分派（upsert/activate/delete；空串与词表外点名拒）。
fn rr3_model_op_apply(
    rows: &mut Vec<Value>,
    op: &str,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    match op {
        "upsert" => rr3_model_upsert(rows, name, args),
        "activate" => rr3_model_activate(rows, name),
        "delete" => rr3_model_delete(rows, name),
        "" => Err("缺 op（允许 upsert/activate/delete）".to_string()),
        other => Err(format!("op 不认: '{other}'（允许 upsert/activate/delete）")),
    }
}

/// 结构门：恰一行 active=true（多 active/零 active 都是误配，写面自守）。
fn rr3_model_one_active(rows: &[Value]) -> Result<(), String> {
    let active_n = rows
        .iter()
        .filter(|r| r.get("active").and_then(Value::as_bool) == Some(true))
        .count();
    if active_n != 1 {
        return Err(format!("模型行表须恰一行 active=true（现 {active_n} 行）"));
    }
    Ok(())
}

/// 落盘 + 内存提交：文本手术 → boot 解析器预核 → 原子落盘 → 文档树/模型行同步。
/// 预核在落盘前，解析不过零字节进正档（失败面全部走完才动文件）。
fn rr3_model_commit(
    cfg: &mut Rr3BrainCfg,
    rows: Vec<Value>,
    op: &str,
    name: &str,
) -> Result<Value, String> {
    let path = cfg
        .config_path
        .clone()
        .ok_or("模型行写面未接 config 路径（cfg.config_path 未注入）——本次未改任何文件")?;
    let old_text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读 config {} 失败: {e}", path.display()))?;
    let new_text = rr3_block_rewrite(&old_text, &rows)?;
    // 落盘前验证：新文本必须被 boot 解析器吃下（恰一 active/格式/URL 全过）
    let parsed = rr3_brain_cfg_parse(&new_text)?;
    rr3_config_persist(&path, &old_text, &new_text)?;
    // 提交内存（解析产物与行表同源；失败面已在落盘前全部走完）
    let mut doc = cfg.doc.clone();
    let brain = doc
        .get_mut("rr3-brain")
        .and_then(Value::as_object_mut)
        .ok_or("config 缺 [rr3-brain] 段")?;
    brain.insert("model".to_string(), Value::Array(rows.clone()));
    cfg.doc = doc;
    cfg.model = parsed.model;
    Ok(json!({
        "ok": true,
        "op": op,
        "name": name,
        "active": rr3_row_active_name(&rows),
        "persisted": true,
        "applies": "live",
    }))
}

/// 连通性测试（`rr3-brain/model/test`）：按名取行造临时客户端探测——业务失败
/// （缺 key/HTTP 非 2xx/不可达）也回正常应答，用 `ok:false` 表达，不折 Err。
pub(crate) fn rr3_model_test_arm(cfg: &Rr3BrainCfg, args: &Value) -> Result<Value, String> {
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if name.is_empty() {
        return Err("模型行 name 不得为空".to_string());
    }
    let rows = rr3_model_rows(&cfg.doc)?;
    let Some(row) = rows
        .iter()
        .find(|r| r.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Err(format!("模型行不认: '{name}'"));
    };
    let base_url = row
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let model_id = row
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if base_url.is_empty() || model_id.is_empty() {
        return Err(format!("模型行 {name} 缺 base_url/model_id"));
    }
    let mode = if row.get("mode").and_then(Value::as_str).unwrap_or("api") == "local" {
        Rr3LlmMode::Local
    } else {
        Rr3LlmMode::Api
    };
    let key_ref = row
        .get("api_key_ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let timeout_s = rr3_cloud_cfg_of(&cfg.doc)?.timeout_s;
    let client = Rr3LlmClient::new(base_url, model_id, timeout_s, key_ref, Some(mode));
    Ok(client.rr3_probe().map_or_else(
        |e| {
            json!({
                "ok": false,
                "name": name,
                "latency_ms": 0,
                "error": e,
            })
        },
        |(latency_ms, mock)| {
            json!({
                "ok": true,
                "name": name,
                "status": 200,
                "latency_ms": latency_ms,
                "mock": mock,
            })
        },
    ))
}
