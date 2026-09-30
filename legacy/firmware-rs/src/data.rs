//! data：运行数据计数器，RAM 缓存 + NVS 持久化（移植自 legacy/core/data）
//!
//! 与 C 版一致：namespace=rtdata，10→9 个计数器（errors 之后的 key 待语音链路落地）。
//! esp-idf-svc 的 set_u32 内部自带 nvs_commit，无需显式提交。

use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use esp_idf_svc::sys::EspError;

/// 计数器数量（与 NAMES 长度一致）
pub const COUNT: usize = 9;

const NAMES: [&str; COUNT] = [
    "boots",
    "chat_rounds",
    "asr_ok",
    "asr_fail",
    "llm_fail",
    "tts_fail",
    "relay_on",
    "relay_off",
    "errors",
];

/// 计数器键（C 版 data_key_t）
#[derive(Copy, Clone, Debug)]
#[repr(usize)]
pub enum Key {
    Boots = 0,
    ChatRounds,
    AsrOk,
    AsrFail,
    LlmFail,
    TtsFail,
    RelayOn,
    RelayOff,
    Errors,
}

impl Key {
    pub fn name(self) -> &'static str {
        NAMES[self as usize]
    }
}

pub struct Data {
    nvs: EspNvs<NvsDefault>,
    vals: [u32; COUNT],
}

impl Data {
    /// 打开 NVS namespace 并加载全部计数器；boots 自增并立即持久化
    pub fn new(partition: EspDefaultNvsPartition) -> Result<Self, EspError> {
        let nvs = EspNvs::new(partition, "rtdata", true)?;
        let mut vals = [0u32; COUNT];
        for (i, name) in NAMES.iter().enumerate() {
            vals[i] = nvs.get_u32(name)?.unwrap_or(0);
        }
        vals[Key::Boots as usize] += 1;
        nvs.set_u32(NAMES[Key::Boots as usize], vals[Key::Boots as usize])?;
        Ok(Self { nvs, vals })
    }

    /// 计数器自增并持久化（每次 set_u32 自带 commit）
    pub fn inc(&mut self, key: Key) {
        let i = key as usize;
        self.vals[i] = self.vals[i].wrapping_add(1);
        if let Err(e) = self.nvs.set_u32(NAMES[i], self.vals[i]) {
            println!("data inc {} 失败: {:?}", NAMES[i], e);
        }
    }

    /// 读取当前值（RAM 缓存，不等价 NVS）
    pub fn get(&self, key: Key) -> u32 {
        self.vals[key as usize]
    }

    /// 导出为 JSON 对象（对应 C 版 data_get_json）
    pub fn json(&self) -> String {
        let mut out = String::from("{");
        for (i, name) in NAMES.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('"');
            out.push_str(name);
            out.push_str("\":");
            out.push_str(&self.vals[i].to_string());
        }
        out.push('}');
        out
    }
}
