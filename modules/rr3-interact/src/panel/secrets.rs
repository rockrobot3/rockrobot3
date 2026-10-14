//! 面板密钥面（0.0.6 模型连接抽象批）：模型 API key 的面板维护面。
//!
//! key 只落**仓外** secrets 文件（JSON `{"keys": {ref: value}}`，0600）——代码/
//! 仓库/配置里只有引用名，永不落值。brain（Python）每次调用现读同一文件，路径
//! 契约双端同义：env `RR3_SECRETS_FILE` 优先，缺省 `$HOME/.rr3/secrets.json`。
//! 值在本模块不进日志、不进任何应答负载（GET 只回 ref 名与是否已设）。
//! 目标无 HOME（板上裸环境）时显式回错——本机制定位宿主/开发机面，板上模型
//! 接入随装配演进（注记，不 claim）。

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// secrets 路径环境变量（brain 侧同名同义——双端单一契约）
pub const RR3_SECRETS_FILE_ENV: &str = "RR3_SECRETS_FILE";

/// secrets 文件路径：env 优先，缺省 `$HOME/.rr3/secrets.json`；无 HOME 回错。
pub fn rr3_secrets_path() -> Result<PathBuf, String> {
    rr3_secrets_path_with(
        std::env::var(RR3_SECRETS_FILE_ENV).ok(),
        std::env::var("HOME").ok(),
    )
}

/// 路径解析纯函数（tests 直测，免 env 竞争）：env 非空优先，否则 HOME 拼接。
fn rr3_secrets_path_with(secrets_env: Option<String>, home: Option<String>) -> Result<PathBuf, String> {
    if let Some(p) = secrets_env {
        if !p.trim().is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let home = home
        .filter(|h| !h.trim().is_empty())
        .ok_or_else(|| "secrets 路径无着落：缺 HOME 且未设 RR3_SECRETS_FILE".to_string())?;
    Ok(PathBuf::from(home).join(".rr3").join("secrets.json"))
}

/// 读全表：缺文件=空表（首次使用是常态）；坏 json/坏形状=回错**不静默覆盖**
/// （覆盖会把用户已存的其他 key 一把抹掉）。
fn rr3_secrets_read(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut text = String::new();
    match File::open(path) {
        Ok(mut f) => {
            f.read_to_string(&mut text)
                .map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(format!("开 {} 失败: {e}", path.display())),
    }
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} 不是合法 JSON: {e}", path.display()))?;
    let keys = v
        .get("keys")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{} 形状不符（须 {{\"keys\": {{ref: 值}}}}）", path.display()))?;
    let mut out = BTreeMap::new();
    for (k, val) in keys {
        if let Some(s) = val.as_str() {
            out.insert(k.clone(), s.to_string());
        }
    }
    Ok(out)
}

/// upsert 一条（`POST /api/model/key` 的落点）：value 非空=写入/覆盖，空=删除
/// 该 ref（轮换与撤销一个语义）；ref 只认非空无空白。落盘 0600，父目录 0700。
pub fn rr3_secrets_upsert(ref_name: &str, value: &str) -> Result<(), String> {
    rr3_secrets_upsert_at(&rr3_secrets_path()?, ref_name, value)
}

pub fn rr3_secrets_upsert_at(path: &Path, ref_name: &str, value: &str) -> Result<(), String> {
    let ref_name = ref_name.trim();
    if ref_name.is_empty() {
        return Err("ref 不能为空".to_string());
    }
    if ref_name.chars().any(char::is_whitespace) {
        return Err(format!("ref 含空白: {ref_name:?}"));
    }
    let mut keys = rr3_secrets_read(path)?;
    if value.trim().is_empty() {
        keys.remove(ref_name);
    } else {
        keys.insert(ref_name.to_string(), value.to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("建 {} 失败: {e}", parent.display()))?;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    let body = json!({ "keys": keys }).to_string();
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
    f.write_all(body.as_bytes())
        .map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("收 {} 权限失败: {e}", path.display()))?;
    Ok(())
}

/// 已存 ref 清单（`GET /api/model/keys` 的负载）：只回名字与已设标记，**无值**。
pub fn rr3_secrets_list() -> Result<Vec<Value>, String> {
    rr3_secrets_list_at(&rr3_secrets_path()?)
}

pub fn rr3_secrets_list_at(path: &Path) -> Result<Vec<Value>, String> {
    let keys = rr3_secrets_read(path)?;
    Ok(keys.keys().map(|k| json!({ "ref": k, "set": true })).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_secrets(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rr3-panel-secrets-{}-{tag}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join("secrets.json")
    }

    #[test]
    fn path_resolution_env_wins_then_home() {
        // env 非空优先
        assert_eq!(
            rr3_secrets_path_with(Some("/tmp/x.json".into()), Some("/home/u".into()))
                .unwrap(),
            PathBuf::from("/tmp/x.json")
        );
        // env 空串回退 HOME
        assert_eq!(
            rr3_secrets_path_with(Some("  ".into()), Some("/home/u".into())).unwrap(),
            PathBuf::from("/home/u/.rr3/secrets.json")
        );
        // 无 HOME 且无 env → 显式回错（板上裸环境面）
        assert!(rr3_secrets_path_with(None, None).is_err());
        assert!(rr3_secrets_path_with(None, Some("".into())).is_err());
    }

    #[test]
    fn upsert_insert_overwrite_and_delete_with_mode_0600() {
        let path = tmp_secrets("upsert");
        rr3_secrets_upsert_at(&path, "DeepSeek官方", "sk-a").unwrap();
        rr3_secrets_upsert_at(&path, "SiliconFlow", "sk-b").unwrap();
        // 权限 0600（敏感面铁律）
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "secrets 文件必须 0600");
        // 覆盖同名 ref
        rr3_secrets_upsert_at(&path, "DeepSeek官方", "sk-a2").unwrap();
        // 空值 = 删除
        rr3_secrets_upsert_at(&path, "SiliconFlow", "").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("sk-a2"));
        assert!(!text.contains("sk-b"));
        assert!(!text.contains("SiliconFlow"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn upsert_rejects_blank_or_spaced_ref() {
        let path = tmp_secrets("refs");
        assert!(rr3_secrets_upsert_at(&path, "", "v").is_err());
        assert!(rr3_secrets_upsert_at(&path, "  ", "v").is_err());
        assert!(rr3_secrets_upsert_at(&path, "a b", "v").is_err());
        // 文件未被创建（fail 早于落盘）
        assert!(!path.exists());
    }

    #[test]
    fn corrupt_existing_file_errors_instead_of_overwrite() {
        let path = tmp_secrets("corrupt");
        std::fs::write(&path, "不是json").unwrap();
        let err = rr3_secrets_upsert_at(&path, "r", "v").unwrap_err();
        assert!(err.contains("JSON"), "坏文件必须回错保护既有 key: {err}");
        // 误形状同罚
        std::fs::write(&path, r#"{"keys": "不是字典"}"#).unwrap();
        assert!(rr3_secrets_upsert_at(&path, "r", "v").is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn list_returns_names_only_never_values() {
        let path = tmp_secrets("list");
        rr3_secrets_upsert_at(&path, "r1", "sk-secret-value").unwrap();
        rr3_secrets_upsert_at(&path, "r2", "sk-other").unwrap();
        let out = rr3_secrets_list_at(&path).unwrap();
        let text = json!(out).to_string();
        assert!(!text.contains("sk-secret-value"), "清单永不回显值: {text}");
        assert!(!text.contains("sk-other"));
        let refs: Vec<&str> = out
            .iter()
            .filter_map(|e| e.get("ref").and_then(Value::as_str))
            .collect();
        assert_eq!(refs, vec!["r1", "r2"]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
