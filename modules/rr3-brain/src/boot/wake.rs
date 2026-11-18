//! 唤醒词配置段（`[rr3-brain.wake]`，2026-10-31）：结构、缺省与四键解析。
//!
//! 语义：ASR 文本前缀匹配（不是本地 KWS）；命中即唤醒，`enable=false` 时
//! 行为与未加唤醒词逐点等价。解析 fail-closed——类型错/短语表空/应答空一律
//! 点名拒 boot，配置劣化不静默收编。
//!
//! 拆分动因：`boot.rs` 563 行，唤醒段独占一簇（结构 + 缺省 + 解析），
//! 与模型行/cloud 投影无共享状态。

use serde_json::Value;

/// 唤醒词段的缺省应答（`[rr3-brain.wake].ack` 缺键回落）：用户口径
/// "我在呢 / 有什么可以帮您" 的合并版，一句话即可（TTS 合成一次）。
pub(crate) const RR3_WAKE_ACK_DEFAULT: &str = "我在呢，有什么可以帮您？";
/// 唤醒后跟进窗缺省（`followup_ms` 缺键回落）：唤醒应答后 15s 内继续说话
/// 不必重复唤醒词；每接受一轮即续窗（对话式追问）。
pub(crate) const RR3_WAKE_FOLLOWUP_DEFAULT_MS: u64 = 15_000;

/// 唤醒词配置（`[rr3-brain.wake]` 段，2026-10-31）：**ASR 文本前缀匹配**，
/// 不是本地 KWS（仓内无端侧关键词引擎；云端 ASR 已在 VAD 起音后必跑）。
/// 语义：命中任一短语（去标点/空白归一后的前缀）→ 唤醒；命中且无后续文本 →
/// 回固定应答（不进 LLM）；命中带后续文本 → 后续文本进 LLM；未命中但在
/// 唤醒后的跟进窗内 → 原文本进 LLM；窗外 → 静默丢弃（不 LLM、不 TTS）。
/// `enable=false`（缺省）= 关闭门控，行为与未加唤醒词逐点等价。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rr3WakeCfg {
    pub(crate) enable: bool,
    pub(crate) phrases: Vec<String>,
    pub(crate) ack: String,
    pub(crate) followup_ms: u64,
}

impl Default for Rr3WakeCfg {
    fn default() -> Self {
        Self {
            enable: false,
            phrases: vec!["你好洛克".to_string()],
            ack: RR3_WAKE_ACK_DEFAULT.to_string(),
            followup_ms: RR3_WAKE_FOLLOWUP_DEFAULT_MS,
        }
    }
}

/// `[rr3-brain.wake]` 段解析（可选段：缺段 = 关闭门控，缺省值见 [`Rr3WakeCfg`]；
/// 类型错/短语表空/应答空一律 Err 点名——配置劣化不静默收编）。
pub(super) fn rr3_wake_cfg_of(doc: &Value) -> Result<Rr3WakeCfg, String> {
    let mut cfg = Rr3WakeCfg::default();
    let Some(w) = doc.get("rr3-brain").and_then(|b| b.get("wake")) else {
        return Ok(cfg);
    };
    rr3_wake_apply(w, &mut cfg)?;
    Ok(cfg)
}

/// 唤醒段逐键应用（键表顺序即应用序，坏值逐条点名）。
pub(super) fn rr3_wake_apply(w: &Value, cfg: &mut Rr3WakeCfg) -> Result<(), String> {
    for (key, set) in RR3_WAKE_SETS {
        if let Some(v) = w.get(key) {
            set(cfg, v)?;
        }
    }
    Ok(())
}

/// 唤醒段四键 → 落位（未给键保留缺省；未知键维持原状不拒）。
type Rr3WakeSet = fn(&mut Rr3WakeCfg, &Value) -> Result<(), String>;
const RR3_WAKE_SETS: [(&str, Rr3WakeSet); 4] = [
    ("enable", |c, v| {
        c.enable = v
            .as_bool()
            .ok_or_else(|| "config [rr3-brain.wake] enable 须布尔".to_string())?;
        Ok(())
    }),
    ("phrases", |c, v| {
        c.phrases = rr3_wake_phrases(v)?;
        Ok(())
    }),
    ("ack", |c, v| {
        c.ack = rr3_wake_ack(v)?;
        Ok(())
    }),
    ("followup_ms", |c, v| {
        c.followup_ms = v
            .as_u64()
            .ok_or_else(|| "config [rr3-brain.wake] followup_ms 须非负整数".to_string())?;
        Ok(())
    }),
];

/// `ack`：非空字符串（trim 后空即拒）。
pub(super) fn rr3_wake_ack(v: &Value) -> Result<String, String> {
    let s = v
        .as_str()
        .ok_or_else(|| "config [rr3-brain.wake] ack 须字符串".to_string())?
        .trim();
    if s.is_empty() {
        return Err("config [rr3-brain.wake] ack 不得为空".to_string());
    }
    Ok(s.to_string())
}

/// `phrases` 项形状核：字符串数组、项去空白、空项跳过——全空即拒（空短语表
/// 等于"永不唤醒"却挂着 enable=true，是误配不是关闭语义）。
pub(super) fn rr3_wake_phrases(v: &Value) -> Result<Vec<String>, String> {
    let arr = v
        .as_array()
        .ok_or("config [rr3-brain.wake] phrases 须字符串数组")?;
    let out: Vec<String> = arr
        .iter()
        .filter_map(|item| rr3_wake_item(item).transpose())
        .collect::<Result<Vec<String>, String>>()?;
    if out.is_empty() {
        return Err("config [rr3-brain.wake] phrases 须至少一个非空短语".to_string());
    }
    Ok(out)
}

/// 单项：必为字符串，trim 后空返回 None（空项跳过）。
pub(super) fn rr3_wake_item(item: &Value) -> Result<Option<String>, String> {
    let s = item
        .as_str()
        .ok_or("config [rr3-brain.wake] phrases 项须字符串")?
        .trim();
    Ok(if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    })
}
