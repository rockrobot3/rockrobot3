//! 面板密钥面（0.0.6 模型连接抽象批）：模型 API key 的面板维护面。
//!
//! key 只落**仓外** secrets 文件（JSON `{"keys": {ref: value}}`，0600）——代码/
//! 仓库/配置里只有引用名，永不落值。brain（Python）每次调用现读同一文件，路径
//! 契约双端同义：env `RR3_SECRETS_FILE` 优先，缺省 `$HOME/.rr3/secrets.json`。
//! 值在本模块不进日志、不进任何应答负载（GET 只回 ref 名与是否已设）。
//! 目标无 HOME（板上裸环境）时显式回错——本机制定位宿主/开发机面，板上模型
//! 接入随装配演进（注记，不 claim）。

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rr_ipc::persist::rr3_store_perm;
use serde_json::{json, Value};

/// secrets 读-改-写串行锁：面板每连接一线程，两个并发 `POST /api/model/key`
/// 若各自读旧表再各自原子写，后写者覆盖前写者（丢键），且共用同一个
/// `secrets.json.tmp` 会互相踩。upsert 是低频运维动作，进程级互斥代价可忽略。
static RR3_SECRETS_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// secrets 路径环境变量（brain 侧同名同义——双端单一契约）
pub const RR3_SECRETS_FILE_ENV: &str = "RR3_SECRETS_FILE";

/// secrets 文件路径：env 优先，缺省 `$HOME/.rr3/secrets.json`；无 HOME 回错。
pub fn rr3_secrets_path() -> Result<PathBuf, String> {
    rr3_secrets_resolve(
        std::env::var(RR3_SECRETS_FILE_ENV).ok(),
        std::env::var("HOME").ok(),
    )
}

/// 路径解析纯函数（tests 直测，免 env 竞争）：env 非空优先，否则 HOME 拼接。
fn rr3_secrets_resolve(
    secrets_env: Option<String>,
    home: Option<String>,
) -> Result<PathBuf, String> {
    if let Some(p) = rr3_secrets_env_path(secrets_env) {
        return Ok(p);
    }
    rr3_secrets_home(home)
}

/// HOME 回落后备路径（空 HOME 显式回错）。
fn rr3_secrets_home(home: Option<String>) -> Result<PathBuf, String> {
    let home = home
        .filter(|h| !h.trim().is_empty())
        .ok_or_else(|| "secrets 路径无着落：缺 HOME 且未设 RR3_SECRETS_FILE".to_string())?;
    Ok(PathBuf::from(home).join(".rr3").join("secrets.json"))
}

/// env 覆盖路径（空白值等同未设）。
fn rr3_secrets_env_path(p: Option<String>) -> Option<PathBuf> {
    p.filter(|s| !s.trim().is_empty()).map(PathBuf::from)
}

/// 读全表：缺文件=空表（首次使用是常态）；坏 json/坏形状=回错**不静默覆盖**
/// （覆盖会把用户已存的其他 key 一把抹掉）。
fn rr3_secrets_read(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let Some(text) = rr3_secrets_text(path)? else {
        return Ok(BTreeMap::new());
    };
    rr3_secrets_parse(path, &text)
}

/// 读 secrets 文件全文；文件不存在回 None（首次使用是常态）。
fn rr3_secrets_text(path: &Path) -> Result<Option<String>, String> {
    let mut text = String::new();
    match File::open(path) {
        Ok(mut f) => {
            f.read_to_string(&mut text)
                .map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
        }
        Err(e) => return rr3_secret_open_err(path, &e),
    }
    Ok(Some(text))
}

/// 打开失败分类：文件不存在回 None（首次使用是常态），其余回错。
fn rr3_secret_open_err(path: &Path, e: &std::io::Error) -> Result<Option<String>, String> {
    if e.kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
    }
    Err(format!("开 {} 失败: {e}", path.display()))
}

/// 文本 → keys 全表：坏 json/坏形状回错**不静默覆盖**（覆盖会抹掉已存 key）。
fn rr3_secrets_parse(path: &Path, text: &str) -> Result<BTreeMap<String, String>, String> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("{} 不是合法 JSON: {e}", path.display()))?;
    let keys = v.get("keys").and_then(Value::as_object).ok_or_else(|| {
        format!(
            "{} 形状不符（须 {{\"keys\": {{ref: 值}}}}）",
            path.display()
        )
    })?;
    rr3_secret_map(path, keys)
}

/// keys 全表落位：值须为字符串（坏形状 = 回错，与坏 json 同一口径）。
fn rr3_secret_map(
    path: &Path,
    keys: &serde_json::Map<String, Value>,
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (k, val) in keys {
        match val.as_str() {
            Some(s) => {
                out.insert(k.clone(), s.to_string());
            }
            // 坏形状 = 回错（与坏 json 同一口径）：写路径是整表重写，静默忽略
            // 非字符串条目会让下一次 upsert 把它永久抹掉——数据丢失伪装成成功。
            None => {
                return Err(format!(
                    "{} 的 keys.{k} 不是字符串（坏形状：写回会丢键，故拒读）",
                    path.display()
                ))
            }
        }
    }
    Ok(out)
}

/// upsert 一条（`POST /api/model/key` 的落点）：value 非空=写入/覆盖，空=删除
/// 该 ref（轮换与撤销一个语义）；ref 只认非空无空白。落盘走
/// [`rr3_store_perm`]（批N4 件四 I-20：原 `truncate(true)` 原地覆盖无 tmp/
/// 无 fsync，掉电卡写中间 ⇒ 凭据文件恒 Err 且写侧不能自愈——全案 §19.2 Y-2
/// 判据 2"落盘原子序单点"），tmp **建时即 0600**（消除 0644 写窗口，外部注入
/// 路径下同机他用户读不到），父目录 0700 是第二道。
pub fn rr3_secrets_upsert(path: &Path, ref_name: &str, value: &str) -> Result<(), String> {
    let ref_name = rr3_secrets_ref_gate(ref_name)?;
    // 读-改-写整段持锁：丢键与 tmp 互踩都由此封死（见静态锁注释）。
    let _write = RR3_SECRETS_WRITE_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut keys = rr3_secrets_read(path)?;
    rr3_secrets_apply(&mut keys, ref_name, value);
    rr3_secrets_dir_perm(path)?;
    rr3_secrets_write(path, keys)
}

/// ref 形制：非空、无空白（落盘前的 fail-closed 门）。
fn rr3_secrets_ref_gate(ref_name: &str) -> Result<&str, String> {
    let ref_name = ref_name.trim();
    if ref_name.is_empty() {
        return Err("ref 不能为空".to_string());
    }
    if ref_name.chars().any(char::is_whitespace) {
        return Err(format!("ref 含空白: {ref_name:?}"));
    }
    Ok(ref_name)
}

/// value 非空=写入/覆盖，空=删除（轮换与撤销一个语义）。
fn rr3_secrets_apply(keys: &mut BTreeMap<String, String>, ref_name: &str, value: &str) {
    if value.trim().is_empty() {
        keys.remove(ref_name);
    } else {
        keys.insert(ref_name.to_string(), value.to_string());
    }
}

/// 父目录准备：缺省 `~/.rr3` 收 0700（第二道）；外部注入路径不动别人的目录。
fn rr3_secrets_dir_perm(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent).map_err(|e| format!("建 {} 失败: {e}", parent.display()))?;
    if rr3_secrets_own(
        std::env::var(RR3_SECRETS_FILE_ENV).ok(),
        path,
        std::env::var("HOME").ok(),
    ) {
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("收 {} 目录权限失败: {e}", parent.display()))?;
    }
    Ok(())
}

/// 整表落盘：tmp 建时即 0600，rename 原子替换后目标继承该权限（敏感面铁律）。
fn rr3_secrets_write(path: &Path, keys: BTreeMap<String, String>) -> Result<(), String> {
    let body = json!({ "keys": keys }).to_string();
    let tmp = path.with_extension("json.tmp");
    rr3_store_perm(path, &tmp, body.as_bytes(), 0o600)
}

/// 目录归属（策略面，纯函数）：只有缺省 `~/.rr3` 是面板自己建的，目录权限由
/// 面板负责；外部显式指定的路径（`RR3_SECRETS_FILE`，运维/测试注入）只对**文件**
/// 收 0600，绝不对别人的目录 chmod——`RR3_SECRETS_FILE=/tmp/x/secrets.json`
/// 这类配置曾把 `/tmp` 收成 0700，属破坏性副作用。
fn rr3_secrets_own(env: Option<String>, path: &Path, home: Option<String>) -> bool {
    rr3_secrets_resolve(env, home)
        .map(|d| d == path)
        .unwrap_or(false)
}

/// 已存 ref 清单（`GET /api/model/keys` 的负载）：只回名字与已设标记，**无值**。
pub fn rr3_secrets_list_at(path: &Path) -> Result<Vec<Value>, String> {
    let keys = rr3_secrets_read(path)?;
    Ok(keys
        .keys()
        .map(|k| json!({ "ref": k, "set": true }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_secrets(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("rr3-panel-secrets-{}-{tag}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join("secrets.json")
    }

    /// 并发 upsert 不丢键（本轮审计）：面板每连接一线程，读-改-写必须串行。
    #[test]
    fn concurrent_upserts_do_not_lose_keys() {
        let path = tmp_secrets("concurrent");
        let mut hs = Vec::new();
        for i in 0..16 {
            let p = path.clone();
            hs.push(std::thread::spawn(move || {
                rr3_secrets_upsert(&p, &format!("ref-{i}"), "v").expect("并发写应成功");
            }));
        }
        for h in hs {
            h.join().expect("写线程不应 panic");
        }
        let keys = rr3_secrets_read(&path).expect("读回全表");
        for i in 0..16 {
            assert!(
                keys.contains_key(&format!("ref-{i}")),
                "并发写丢键 ref-{i}: {:?}",
                keys.keys().collect::<Vec<_>>()
            );
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// 目录归属判据（本轮审计）：只有缺省 `~/.rr3` 由面板管权限；显式注入路径
    /// 一律不 chmod 父目录——否则 `RR3_SECRETS_FILE=/tmp/x.json` 会把 /tmp 收成
    /// 0700，属破坏性副作用。
    #[test]
    fn own_dir_only_for_default_path() {
        let def = PathBuf::from("/home/u/.rr3/secrets.json");
        assert!(rr3_secrets_own(None, &def, Some("/home/u".into())));
        assert!(!rr3_secrets_own(
            Some("/tmp/x.json".into()),
            &def,
            Some("/home/u".into())
        ));
        assert!(!rr3_secrets_own(
            None,
            &PathBuf::from("/tmp/x.json"),
            Some("/home/u".into())
        ));
        assert!(!rr3_secrets_own(None, &def, None));
    }

    /// 坏形状 fail-closed（本轮审计）：非字符串值原先被静默忽略，而写路径是
    /// 整表重写——下一次 upsert 会把它永久抹掉（数据丢失伪装成成功）。读须
    /// 回错点名 ref，写路径随之拒且原文件逐字不动。
    #[test]
    fn read_rejects_non_string_values_without_data_loss() {
        let path = tmp_secrets("shape");
        let bad = r#"{"keys":{"a":"v","b":123}}"#;
        std::fs::write(&path, bad).unwrap();
        let e = rr3_secrets_read(&path).unwrap_err();
        assert!(e.contains("keys.b") && e.contains("不是字符串"), "{e}");
        assert!(
            rr3_secrets_upsert(&path, "c", "v2").is_err(),
            "坏形状下写必须拒"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            bad,
            "原文件不得被改写"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn path_resolution_env_wins_then_home() {
        // env 非空优先
        assert_eq!(
            rr3_secrets_resolve(Some("/tmp/x.json".into()), Some("/home/u".into())).unwrap(),
            PathBuf::from("/tmp/x.json")
        );
        // env 空串回退 HOME
        assert_eq!(
            rr3_secrets_resolve(Some("  ".into()), Some("/home/u".into())).unwrap(),
            PathBuf::from("/home/u/.rr3/secrets.json")
        );
        // 无 HOME 且无 env → 显式回错（板上裸环境面）
        assert!(rr3_secrets_resolve(None, None).is_err());
        assert!(rr3_secrets_resolve(None, Some("".into())).is_err());
    }

    #[test]
    fn upsert_insert_overwrite_and_delete_with_mode_0600() {
        let path = tmp_secrets("upsert");
        rr3_secrets_upsert(&path, "DeepSeek官方", "sk-a").unwrap();
        rr3_secrets_upsert(&path, "SiliconFlow", "sk-b").unwrap();
        // 权限 0600（敏感面铁律）
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "secrets 文件必须 0600");
        // 覆盖同名 ref
        rr3_secrets_upsert(&path, "DeepSeek官方", "sk-a2").unwrap();
        // 空值 = 删除
        rr3_secrets_upsert(&path, "SiliconFlow", "").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("sk-a2"));
        assert!(!text.contains("sk-b"));
        assert!(!text.contains("SiliconFlow"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn upsert_rejects_blank_or_spaced_ref() {
        let path = tmp_secrets("refs");
        assert!(rr3_secrets_upsert(&path, "", "v").is_err());
        assert!(rr3_secrets_upsert(&path, "  ", "v").is_err());
        assert!(rr3_secrets_upsert(&path, "a b", "v").is_err());
        // 文件未被创建（fail 早于落盘）
        assert!(!path.exists());
    }

    #[test]
    fn corrupt_existing_file_errors_instead_of_overwrite() {
        let path = tmp_secrets("corrupt");
        std::fs::write(&path, "不是json").unwrap();
        let err = rr3_secrets_upsert(&path, "r", "v").unwrap_err();
        assert!(err.contains("JSON"), "坏文件必须回错保护既有 key: {err}");
        // 误形状同罚
        std::fs::write(&path, r#"{"keys": "不是字典"}"#).unwrap();
        assert!(rr3_secrets_upsert(&path, "r", "v").is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn list_returns_names_only_never_values() {
        let path = tmp_secrets("list");
        rr3_secrets_upsert(&path, "r1", "sk-secret-value").unwrap();
        rr3_secrets_upsert(&path, "r2", "sk-other").unwrap();
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
