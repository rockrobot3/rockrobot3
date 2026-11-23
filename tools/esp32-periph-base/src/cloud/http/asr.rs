// SPDX-License-Identifier: Apache-2.0
//! ASR 上行（W7-b3a/b 自 `http.rs` 拆出）：PCM 源抽象 + multipart 分段写。
//!
//! 机制缝是 [`Rr3PcmSrc`]：调用方给"能按 4 KB 窗重复产出 s16le 字节"的源，
//! 传输面负责头/尾、长度对账与有限重试。两条源实现——内存切片（PSRAM 路径）
//! 与 flash spool（无 PSRAM 路径）——与传输完全解耦；源必须可 `rr3_reset`
//! 重读，满足重试契约。
//!
//! 弱网策略（S3 余量③）：单跳 8 s 不放宽（< dialog 心跳窗 10 s，阻塞读期间
//! 无法续拍），最多 3 次、退避 500/1500 ms——韧性来自"多一次尝试"。
//!
//! 失败语义：源短读/超长都拒发并点名（绝不把半段 PCM 送给云端）；重试判据与
//! 其他云接口同源（`rr_cloud_proto::rr3_cloud_retryable`，含传输层负状态码）。

use std::thread;
use std::time::Duration;

use esp_idf_sys::{esp_http_client_cleanup, esp_http_client_handle_t};

use super::{
    rr3_client, rr3_cstrs, rr3_head, rr3_http_read, rr3_open, rr3_write, ASR_ATTEMPTS,
    ASR_RETRY_DELAY_MS, HTTP_TIMEOUT_MS,
};

/// ASR 请求体的 PCM 源：按 4 KB 窗搬 s16le，不要求整段驻留内存。
///
/// 实现方保证顺序、连续、恰好 `expect` 字节；短读/超长由 [`rr3_write_src`] 拒收
/// 并点名——上传半段 PCM 会让云端识别出错误文本，必须显式失败。
pub(crate) trait Rr3PcmSrc {
    /// 回到段首（每次重试前调用；源必须可重复读）。
    fn rr3_reset(&mut self);
    /// 填 `out`（s16le 字节）直到段尾；返回字节数，0 = 段尾。
    fn rr3_fill(&mut self, out: &mut [u8]) -> Result<usize, String>;
}

/// 内存切片源（mic 有 PSRAM 的既有路径）。
pub(crate) struct Rr3SliceSrc<'a> {
    pcm: &'a [i16],
    off: usize,
}

impl<'a> Rr3SliceSrc<'a> {
    pub(crate) fn rr3_new(pcm: &'a [i16]) -> Self {
        Self { pcm, off: 0 }
    }
}

impl Rr3PcmSrc for Rr3SliceSrc<'_> {
    fn rr3_reset(&mut self) {
        self.off = 0;
    }

    fn rr3_fill(&mut self, out: &mut [u8]) -> Result<usize, String> {
        let mut n = 0usize;
        while n + 2 <= out.len() && self.off < self.pcm.len() {
            out[n..n + 2].copy_from_slice(&self.pcm[self.off].to_le_bytes());
            self.off += 1;
            n += 2;
        }
        Ok(n)
    }
}

/// spool 段源（无 PSRAM 路径）：按偏移向写线程要块，边读边发。
pub(crate) struct Rr3SpoolSrc {
    total: u32,
    off: u32,
}

impl Rr3SpoolSrc {
    pub(crate) fn rr3_new(total: u32) -> Self {
        Self { total, off: 0 }
    }
}

impl Rr3PcmSrc for Rr3SpoolSrc {
    fn rr3_reset(&mut self) {
        self.off = 0;
    }

    fn rr3_fill(&mut self, out: &mut [u8]) -> Result<usize, String> {
        if self.off >= self.total {
            return Ok(0);
        }
        let want = out.len().min((self.total - self.off) as usize);
        let n = crate::spool::rr3_spool_read(self.off, &mut out[..want])?;
        if n == 0 {
            return Err(format!("spool 提前到段尾 off={}", self.off));
        }
        self.off += n as u32;
        Ok(n)
    }
}

/// 流式 ASR 上行（W7-b3a/W7-b3b）：multipart 头/WAV 头/尾 + PCM 4 KB 边取边写，
/// 不再构造整段 s16le 与整段 multipart。PCM 字节数由计划反推（head/wav/tail
/// 之外的部分），与 `content_len` 天然一致，不另设易漂移的参数。
/// 返回 JSON 应答（≤ `MAX_JSON`）。
pub(crate) fn rr3_post_asr(
    interface: &str,
    url: &str,
    key: &str,
    content_type: &str,
    plan: &rr_cloud_proto::Rr3AsrPlan,
    src: &mut dyn Rr3PcmSrc,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for attempt in 0..ASR_ATTEMPTS {
        crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
        let attempt_rc = rr3_asr_once(interface, url, key, content_type, plan, src, max_body);
        crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
        match attempt_rc {
            Ok(v) => return Ok(v),
            Err(e) => {
                if !rr_cloud_proto::rr3_cloud_retryable(interface, &e) {
                    return Err(e);
                }
                last = e;
            }
        }
        if attempt + 1 < ASR_ATTEMPTS {
            let delay = ASR_RETRY_DELAY_MS[attempt.min(ASR_RETRY_DELAY_MS.len() - 1)];
            println!(
                "[cloud] {interface} 第 {} 次失败（{last}），{delay}ms 后重试",
                attempt + 1
            );
            thread::sleep(Duration::from_millis(delay));
        }
    }
    Err(last)
}

/// 单次 ASR 尝试：init → 分段写 → 状态码 → 读 JSON；cleanup 成对。
fn rr3_asr_once(
    interface: &str,
    url: &str,
    key: &str,
    content_type: &str,
    plan: &rr_cloud_proto::Rr3AsrPlan,
    src: &mut dyn Rr3PcmSrc,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    src.rr3_reset();
    let (url_c, auth, ctype) = rr3_cstrs(url, key, content_type)?;
    let client = rr3_client(&url_c, auth.as_deref(), &ctype, HTTP_TIMEOUT_MS)?;
    crate::mem::rr3_heap_mark(interface, "http-open");
    let rc = rr3_asr_do(client, plan, src, interface, max_body);
    crate::mem::rr3_heap_mark(interface, "http-done");
    // SAFETY：client 由刚 init 且只在此处 cleanup。
    unsafe { esp_http_client_cleanup(client) };
    rc
}

/// 分段写请求体 → 取状态码 → 读应答。
fn rr3_asr_do(
    client: esp_http_client_handle_t,
    plan: &rr_cloud_proto::Rr3AsrPlan,
    src: &mut dyn Rr3PcmSrc,
    interface: &str,
    max_body: usize,
) -> Result<Vec<u8>, String> {
    let expect = plan
        .content_len
        .saturating_sub(plan.head.len() + plan.wav.len() + plan.tail.len());
    rr3_open(client, plan.content_len)?;
    rr3_write(client, &plan.head)?;
    rr3_write(client, &plan.wav)?;
    rr3_write_src(client, src, expect)?;
    rr3_write(client, &plan.tail)?;
    rr3_head(client, interface)?;
    rr3_http_read(client, interface, max_body)
}

/// PCM 源 → 4 KB 搬运窗 → 请求体；字节数与 `expect` 不符即拒发（不送半段）。
fn rr3_write_src(
    client: esp_http_client_handle_t,
    src: &mut dyn Rr3PcmSrc,
    expect: usize,
) -> Result<(), String> {
    let mut buf = [0u8; 4_096];
    let mut total = 0usize;
    loop {
        // 大语音段（数秒/上百 KB）上行时逐 4 KB 续一拍：单跳虽已收在 9 s，
        // 持续续拍让监督壳看到 dialog 仍在推进（不靠单跳窗口赌心跳）。
        crate::autonomy::rr3_beat(rr_autonomy::Rr3Task::Dialog);
        let n = src.rr3_fill(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n;
        if total > expect {
            return Err(format!("PCM 源超长 >{expect}B"));
        }
        rr3_write(client, &buf[..n])?;
    }
    if total != expect {
        return Err(format!("PCM 源长度不符 {total}/{expect}B"));
    }
    Ok(())
}
