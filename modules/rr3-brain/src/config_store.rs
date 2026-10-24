//! 模型行落盘写面（ADR-0023，面板模型面 `rr3-brain/model/set` 的写盘半边）。
//!
//! 两条职责严格分开：
//! - **块重写是纯函数**（[`rr3_model_block_rewrite`]）：只替换
//!   `[[rr3-brain.model]]` 块，块外文本逐字保留（cloud/bus/dialog 段与所有注释
//!   不动，含表头前导小节注释）；无表时 EOF 追加。块内行间注释无处存放（模型行
//!   是面板 CRUD 的对象），重写时随之丢弃。
//! - **落盘是 IO**（[`rr3_config_persist`]）：先 `config.toml.bak` 存旧全量快照，
//!   再走全仓唯一原子写序 [`rr_ipc::rr3_atomic_store`]（tmp 全量 → fsync →
//!   rename）——断电时目标要么旧全量、要么新全量，无半截态。
//!
//! 落盘前的"新文本必须能被 boot 解析器吃下"由调用方（[`crate::rpc`]）先过
//! [`crate::boot::rr3_brain_config_parse`]；本模块只负责文本形状与字节落地。

use std::path::{Path, PathBuf};

use serde_json::Value;

/// 模型行字段渲染序（与 `config.toml` 注释表的字段说明同序；`active` 恒在末位）
const RR3_MODEL_ROW_KEYS: [&str; 6] = [
    "name",
    "mode",
    "api_format",
    "base_url",
    "model_id",
    "api_key_ref",
];

/// TOML 标量编码：交给 `toml::Value` 的 Display（引号与转义由库承担，不手写
/// 转义器——手写转义是错误面）
fn rr3_toml_encode_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// 渲染 `[[rr3-brain.model]]` 块（纯函数，行序即表序）。字符串字段缺失即跳过
/// （`api_key_ref` 可空/可缺）；非字符串形状 Err 点名行号与键名。
pub(crate) fn rr3_model_block_render(rows: &[Value]) -> Result<String, String> {
    let mut blocks = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let obj = row
            .as_object()
            .ok_or_else(|| format!("模型行 #{i} 须为表（TOML 行表同构）"))?;
        let mut out = String::from("[[rr3-brain.model]]\n");
        for key in RR3_MODEL_ROW_KEYS {
            if let Some(v) = obj.get(key) {
                let s = v
                    .as_str()
                    .ok_or_else(|| format!("模型行 #{i}.{key} 须字符串"))?;
                out.push_str(&format!("{key} = {}\n", rr3_toml_encode_str(s)));
            }
        }
        let active = obj.get("active").and_then(Value::as_bool).unwrap_or(false);
        out.push_str(&format!("active = {active}\n"));
        blocks.push(out);
    }
    // 行间空行（TOML 合法且与仓内 config.toml 排版同形状）
    Ok(blocks.join("\n"))
}

/// 定位 `[[rr3-brain.model]]` 块的字节区间 `[start, end)`：从首个行首
/// `[[rr3-brain.model]]` 到下一个**别的**表头行（`[...]` 开头）之前；表到文件尾
/// 则 end = 文本长度。无表返 None。多行表头本身不算块边界。
fn rr3_model_block_bounds(text: &str) -> Option<(usize, usize)> {
    let mut start: Option<usize> = None;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let head = bare.trim();
        match start {
            None => {
                if head == "[[rr3-brain.model]]" {
                    start = Some(offset);
                }
            }
            Some(s) => {
                if head.starts_with('[') && head != "[[rr3-brain.model]]" {
                    return Some((s, offset));
                }
            }
        }
        offset += line.len();
    }
    start.map(|s| (s, offset))
}

/// 用 `rows` 渲染的块替换 `full_text` 里的模型行块（纯函数）。无表 → EOF 追加
/// （空行分隔，保证 TOML 合法）。块外文本逐字保留。
pub(crate) fn rr3_model_block_rewrite(full_text: &str, rows: &[Value]) -> Result<String, String> {
    let block = rr3_model_block_render(rows)?;
    let Some((start, end)) = rr3_model_block_bounds(full_text) else {
        let mut out = full_text.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&block);
        return Ok(out);
    };
    let mut out = String::with_capacity(full_text.len() + block.len());
    out.push_str(&full_text[..start]);
    out.push_str(&block);
    let rest = &full_text[end..];
    // 块尾与下一表头之间留一个空行（块边界把原空行一并吞进块内）
    if !rest.is_empty() && !rest.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(rest);
    Ok(out)
}

/// 备份路径 = 原路径字面追加 `.bak`（`config.toml` → `config.toml.bak`；不用
/// `with_extension`——它会把 `.toml` 换掉而不是追加）
fn rr3_config_backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".bak");
    PathBuf::from(s)
}

/// 落盘（IO）：内容无变化即 no-op（不产生 `.bak`）；否则先写 `.bak` 旧全量快照，
/// 再原子写新全量。`.bak` 写失败即中止——备份不成功就不动正式文件。
pub(crate) fn rr3_config_persist(
    path: &Path,
    old_text: &str,
    new_text: &str,
) -> Result<(), String> {
    if old_text == new_text {
        return Ok(());
    }
    let bak = rr3_config_backup_path(path);
    std::fs::write(&bak, old_text).map_err(|e| format!("写备份 {} 失败: {e}", bak.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("config 路径无名: {}", path.display()))?;
    let tmp = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));
    rr_ipc::rr3_atomic_store(path, &tmp, new_text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 与仓内 config.toml 同形状的文本（模型块在 bus/dialog 段之前——块尾边界
    /// 必须认 `[rr3-brain.bus]` 表头，而不是被下一行 model 表头误截）
    fn repo_text() -> String {
        r#"[rr3-brain]
log_level = "info"

[rr3-brain.cloud]
# 主机根注释保留
base_url = "https://api.siliconflow.cn"
asr_model = "a"
llm_model = "m"
tts_model = "t"
timeout_s = 20

# 模型行表头注释（块边界外，重写保留）
[[rr3-brain.model]]
name = "DeepSeek官方"
mode = "api"
api_format = "openai"
base_url = "https://api.deepseek.com/v1"
model_id = "deepseek-chat"
api_key_ref = "DeepSeek官方"
active = true

[rr3-brain.bus]
call_timeout_s = 30.0

[rr3-brain.dialog]
max_utterance_ms = 8000
"#
        .to_string()
    }

    fn row(name: &str, base: &str, model: &str, active: bool) -> Value {
        json!({
            "name": name,
            "mode": "api",
            "api_format": "openai",
            "base_url": base,
            "model_id": model,
            "api_key_ref": name,
            "active": active,
        })
    }

    /// 块替换：块外文本逐字保留、块内按新行表渲染；多行表不误截；重写结果仍能
    /// 被 boot 解析器吃下
    #[test]
    fn rewrite_replaces_block_and_keeps_outside_text() {
        let old = repo_text();
        let rows = vec![
            row("DeepSeek官方", "https://api.deepseek.com/v1", "deepseek-chat", true),
            row("GLM", "https://open.bigmodel.cn/api/paas/v4", "glm-4.5", false),
        ];
        let new = rr3_model_block_rewrite(&old, &rows).unwrap();
        // 块外文本（cloud 段注释、bus/dialog 段）逐字保留
        assert!(new.contains("# 主机根注释保留\nbase_url = \"https://api.siliconflow.cn\""), "{new}");
        assert!(new.contains("[rr3-brain.bus]\ncall_timeout_s = 30.0\n"), "{new}");
        assert!(new.contains("max_utterance_ms = 8000\n"), "{new}");
        // 旧行被替换、新两行都在，且 active 恰一行
        assert_eq!(new.matches("[[rr3-brain.model]]").count(), 2, "{new}");
        assert!(new.contains("name = \"GLM\""), "{new}");
        assert!(new.contains("base_url = \"https://open.bigmodel.cn/api/paas/v4\""), "{new}");
        assert_eq!(new.matches("active = true").count(), 1, "{new}");
        // 块与下一表头之间留空行（排版与仓内一致）
        assert!(new.contains("active = false\n\n[rr3-brain.bus]"), "{new}");
        // 表头前导注释在块边界外：逐字保留（它是小节说明，不该被 CRUD 抹掉）
        assert!(new.contains("# 模型行表头注释"), "{new}");
        // 但原来的"一行"（旧表体）被整体换掉，不会残留旧字段
        assert!(new.contains("# 模型行表头注释（块边界外，重写保留）\n[[rr3-brain.model]]\nname = \"DeepSeek官方\""), "{new}");
        // 结果可被 boot 解析（写盘前的最后一道门）
        let cfg = crate::boot::rr3_brain_config_parse(&new).unwrap();
        assert_eq!(cfg.model.model_id, "deepseek-chat");
    }

    /// 无模型表 → EOF 追加（末行无换行也补断行），追加后仍可解析
    #[test]
    fn rewrite_appends_table_when_absent() {
        let old = "[rr3-brain.cloud]\nbase_url = \"https://x\"\nasr_model = \"a\"\nllm_model = \"m\"\ntts_model = \"t\"\ntimeout_s = 20\n\n[rr3-brain.dialog]\nmax_utterance_ms = 8000";
        let new = rr3_model_block_rewrite(
            old,
            &[row("Qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1", "qwen-plus", true)],
        )
        .unwrap();
        assert!(new.ends_with("active = true\n"), "{new}");
        assert!(new.contains("max_utterance_ms = 8000\n\n[[rr3-brain.model]]"), "{new}");
        assert!(crate::boot::rr3_brain_config_parse(&new).is_ok());
    }

    /// 空行表（删到零行）→ 块被整段摘除，文件仍合法（boot 走 cloud fallback）
    #[test]
    fn rewrite_removes_block_when_rows_empty() {
        let old = repo_text();
        let new = rr3_model_block_rewrite(&old, &[]).unwrap();
        assert!(!new.contains("[[rr3-brain.model]]"), "{new}");
        assert!(new.contains("[rr3-brain.bus]"), "{new}");
        assert!(crate::boot::rr3_brain_config_parse(&new).is_ok());
    }

    /// 字符串转义交给 toml 库（引号/反斜杠值不破坏 TOML），无表渲染为空串
    #[test]
    fn render_escapes_strings_and_handles_empty() {
        assert_eq!(rr3_model_block_render(&[]).unwrap(), "");
        let out = rr3_model_block_render(&[json!({
            "name": "a\"b\\c",
            "mode": "api",
            "api_format": "openai",
            "base_url": "https://x/v1",
            "model_id": "m",
            "active": true,
        })])
        .unwrap();
        // 转义由 toml 库承担：断言"渲染结果能被 TOML 解析回原值"，不锁引号风格
        let parsed: toml::Value = toml::from_str(&out).unwrap();
        let rows_tbl = parsed
            .get("rr3-brain")
            .and_then(|v| v.get("model"))
            .and_then(toml::Value::as_array)
            .expect("块渲染后须是 model 行表");
        assert_eq!(rows_tbl[0].get("name").and_then(toml::Value::as_str), Some("a\"b\\c"));
        // 缺 active 折 false（渲染面不猜"首行即 active"——那是写面策略）
        let out = rr3_model_block_render(&[json!({
            "name": "n",
            "base_url": "https://x/v1",
            "model_id": "m",
        })])
        .unwrap();
        assert!(out.ends_with("active = false\n"), "{out}");
    }

    /// 落盘：写 `.bak` 旧全量 + 正式文件新全量；内容无变化不落盘（不产生 .bak）
    #[test]
    fn persist_writes_backup_then_new_content() {
        let dir = std::env::temp_dir().join(format!("rr3-cfgstore-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let old = repo_text();
        std::fs::write(&path, &old).unwrap();
        let rows = vec![row("GLM", "https://open.bigmodel.cn/api/paas/v4", "glm-4.5", true)];
        let new = rr3_model_block_rewrite(&old, &rows).unwrap();
        rr3_config_persist(&path, &old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), new);
        let bak = dir.join("config.toml.bak");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), old, "备份 = 旧全量");
        assert!(!dir.join("config.toml.tmp").exists(), "tmp 不残留");
        // 无变化 no-op：备份时间戳语义不追（只断言不重写备份——先把备份改脏）
        std::fs::write(&bak, "sentinel").unwrap();
        rr3_config_persist(&path, &new, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "sentinel");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
