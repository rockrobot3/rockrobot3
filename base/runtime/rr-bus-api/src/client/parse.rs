//! manifest 身份/entry 绝对化与帧 payload 解析。

use std::path::Path;

use rr_ipc::{rr3_body_split, Rr3Frame, Rr3PayloadType};
use serde_json::Value;

use super::conn::Rr3BusClient;

/// 从 manifest 文本里取模块身份（name + heartbeat_ms）。
/// 只取客户端自己需要的两个字段：**校验是 rrd 的事**（rr-registry 的校验器 +
/// conn.rs 的注册门禁），客户端不重复也不抢先——manifest 非法时注册会被 rrd
/// 拒掉，错误从那边回来才是唯一真值。
pub struct Rr3BusIdentity {
    pub name: String,
    pub heartbeat_ms: u64,
}

pub fn rr3_manifest_ident(text: &str) -> Result<Rr3BusIdentity, String> {
    let root: toml::Value = text
        .parse()
        .map_err(|e| format!("manifest 不是合法 TOML: {e}"))?;
    let name = root
        .get("manifest")
        .and_then(|m| m.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or("manifest 缺 [manifest].name")?
        .to_string();
    let heartbeat_ms = root
        .get("health")
        .and_then(|h| h.get("heartbeat_ms"))
        .and_then(toml::Value::as_integer)
        .and_then(|v| u64::try_from(v).ok())
        .ok_or("manifest 缺 [health].heartbeat_ms（须为非负整数）")?;
    Ok(Rr3BusIdentity { name, heartbeat_ms })
}

/// 注册门禁要求 `manifest.entry` 是绝对路径（conn.rs），而在仓的 module.toml 是
/// 契约工件、entry 由装机安装步骤写入。模块直跑（测试 / 开发）时走本助手：把
/// entry 改写成进程自己的可执行文件路径再注册——对应"装机时写绝对 entry"的
/// 生产路径的等价物。
pub fn rr3_entry_absolutize(text: &str, exe: &Path) -> Result<String, String> {
    let exe = exe
        .to_str()
        .ok_or_else(|| "可执行文件路径不是合法 UTF-8".to_string())?
        .to_string();
    if !Path::new(&exe).is_absolute() {
        return Err(format!("entry 绝对化失败：{exe} 不是绝对路径"));
    }
    let mut root: toml::Value = text
        .parse()
        .map_err(|e| format!("manifest 不是合法 TOML: {e}"))?;
    root.get_mut("manifest")
        .ok_or("manifest 缺 [manifest] 表")?
        .as_table_mut()
        .ok_or("[manifest] 不是 TOML 表")?
        .insert("entry".into(), toml::Value::String(exe));
    toml::to_string(&root).map_err(|e| format!("manifest 序列化失败: {e}"))
}

impl Rr3BusClient {
    /// 解出帧的 payload 类型与 JSON 体；非 JSON 体返回 `Err`。
    /// 模块分派循环的第一步（payload 前缀 + 体的形状在这里一并落地）。
    pub fn rr3_frame_body(frame: &Rr3Frame) -> Result<(Rr3PayloadType, Value), String> {
        let (ty, body) =
            rr3_body_split(&frame.payload).map_err(|e| format!("payload 前缀非法: {e:?}"))?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| format!("帧体不是 JSON: {e}"))?;
        Ok((ty, v))
    }
}
