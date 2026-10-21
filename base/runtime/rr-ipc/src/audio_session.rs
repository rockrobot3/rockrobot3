//! 板面音频会话握手（A 批 c2 自 tools/rr3-audio-rec 上提的机制单点）。
//!
//! 板面音频会话的行命令协议（`AUDIO OFF`/`AUDIO ON` + 行应答）与开口/断流
//! 重连纪律常量，收流端（rr3-audio-rec）与宿主音频源后端（rr-hal esp32audio）
//! 共用——机制与策略分离：本模块只持"怎样跟板子把音频流开起来"的协议事实，
//! 各消费者的收流节奏/落盘/记账归各面。
//!
//! 纪律（d4/p2/p3 系列实锤，随迁原文）：
//! - 开口 settle：open 后停 [`SETTLE`] 让开口翻转的板复位走完，再
//!   `rr3_flush_input` 净化残留；
//! - 防御 AUDIO OFF：上一会话异常退出的板面残流会抢在 OK 应答前进线（d4
//!   重跑实锤），停流冲净才 AUDIO ON；OFF 应答读了即弃但**超时可见**（d4
//!   run#2 排障教训：静默丢弃害排查）；
//! - ON 应答判据 = [`REPLY_TIMEOUT_MS`] 总预算内逐行扫 [`ok_reply_line`]
//!   （p2/p3 实锤放宽：`OK audio on` 可与首帧流字节同行进线）。
//!
//! cfg 门与 [`crate::transport::uart::Rr3UartConn`] 同径（linux/macos）。

use std::eprintln;
use std::format;
use std::io::Write as _;
use std::println;
use std::string::String;
use std::time::{Duration, Instant};
use std::vec::Vec;

use crate::transport::uart::Rr3UartConn;

/// 开口 settle（CP2102 DTR/RTS 翻转复位板子的既定时序，批C 实锤）。
pub const SETTLE: Duration = Duration::from_millis(1200);
/// 应答行逐字节超时（毫秒）：板面应答整行一次写出（write_all + wait_tx_done），
/// 3s 无字节 = 板面哑了（握手路径点名失败，不再无限等）。
pub const REPLY_TIMEOUT_MS: i32 = 3000;
/// 帧读超时（毫秒，机制面 `rr3_read_timeout`）：单次等就绪时长。正常流分片
/// 背靠背（921600 线速 ~89ms/片），200ms 静默 = 此读无数据（EOF 形状上交帧层
/// → `Closed`/`Torn*` 报错计 err_streak）；连续 3 次 → 重同步。缺省阻塞读
/// 语义留给其余用户，收流面全走超时档。
pub const READ_TIMEOUT_MS: i32 = 200;
/// 读线程断流判据：见过数据后连续 8 次 200ms 零读 = 1.6s 连续静默 → 线上断流
/// （扫描救不了），读线程就地转重连（第 ③ 级）。数据在流时大块 ~10ms 即满，
/// 零读必然是真静默（连续计数，数据一恢复即清零）。
pub const RESYNC_ZERO_BAIL: u32 = 8;
/// 读线程大块裸读窗（判据② A/B 定案的节奏）：单次 poll 就绪后 read 一次的
/// 请求宽度——python read(65536) 同款 600s 满速零污染；分相读（128B 头→plen
/// 载荷两段间让出）在同等链路 2-16s 起逐帧损坏风暴。
pub const READ_BLOCK: usize = 65536;
/// 重连握手重试次数（d4 系列实锤定案）：宿主侧 CP2102 RX 路径冻结——板→宿主
/// 字节全无而宿主→板命令畅通（板面 seq=sent/drop=0 四轮洗冤）；自愈在宿主软件
/// 之外，逐次**重开 fd** 是唯一捅法。run#3/nulltest 3 次重试（~26s 骑行窗）
/// 全败，自愈时长上限未知（∈(26s, 11min]）——预算 10 次 ×（settle 1.2s +
/// OFF/ON 应答 2×3s）+ 9×2s 歇口 ≈ 90s：窗内自愈则续收（冻结空洞由 seq 对账
/// 如实记账，判据不虚报），仍不愈则各消费者按自身语义收场（工具 fail-honest
/// exit(4)；常驻源续轮重试）。
pub const HANDSHAKE_TRIES: u32 = 10;
/// 两次握手尝试之间的歇口（给宿主 USB 栈缓冲；每轮都重新 close/open fd）。
pub const HANDSHAKE_RETRY_PAUSE: Duration = Duration::from_secs(2);

/// 收一行（逐字节超时读 [`REPLY_TIMEOUT_MS`]；'\r' 剥离；换行即返回）。
/// 超时无字节即 Err（板面哑了——握手路径据此点名，不再无限等）。
pub fn read_reply_line(conn: &mut Rr3UartConn) -> Result<String, std::io::Error> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = conn.rr3_read_timeout(&mut byte, REPLY_TIMEOUT_MS)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("应答 {REPLY_TIMEOUT_MS}ms 无字节"),
            ));
        }
        match byte[0] {
            b'\n' => {
                while matches!(line.last(), Some(b'\r')) {
                    line.pop();
                }
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            b => line.push(b),
        }
    }
}

/// 日志安全截断（按**字符**不按字节）：应答行是 `from_utf8_lossy` 产物，行内
/// U+FFFD 等多字节字符按字节切（`&line[..80]`）会切进字符内部 panic——
/// b3c-③ 实录实锤：失步乱行打诊断日志时整条读线程毙命，「重连永不放弃」
/// 被一行日志格式化打破。截断只影响日志可读性，按字符取永不 panic。
fn rr3_log_head(line: &str, max_chars: usize) -> String {
    line.chars().take(max_chars).collect()
}

/// ON 应答行判据：OK 可在行中——流前缀混行合法（p3 实录形状）。
/// 判 `OK audio on` 全串而非裸 `OK`：b64 字符表含 O/K，裸两字符在流 payload 里
/// 误配率不可忽略；9 字节全串在 b64/JSON 流里现实上不可撞。
pub fn ok_reply_line(line: &str) -> bool {
    line.contains("OK audio on")
}

/// 开口握手（首连/重连同款，批C settle 纪律）：停 1.2s 让开口翻转的板复位走完
/// → 冲残留 → **防御停流** → 再冲一次 → `AUDIO ON` → 核验应答。
///
/// 防御停流实锤（d4 重跑）：上一会话看门狗 exit(3) 不发 AUDIO OFF（conn 被帧层
/// 借用），板面 audio_on 残留开态续流——残流字节抢在 OK 应答之前进线，读到的
/// "应答行"是 b64 音频载荷，握手必挂。先发 OFF（幂等）并吞掉应答行（残流混行
/// 不解析），流停后冲净，AUDIO ON 的应答才是干净行首。板本来就没流时 OFF 照
/// 常应答，路径不变。
///
/// ON 应答判据放宽（p2/p3 冒烟实锤，批D④ 续）：板 `OK audio on` 应答与首帧字节
/// 背靠背进线时，读窗口可能先抓到流/失步字节再扫得 OK（p3 实录：seq=10604 帧的
/// b64 payload + `OK audio on` 同行）——"首行必须 OK 开头"把合法起流误判为拒收。
/// 改为 [`REPLY_TIMEOUT_MS`] 总预算内逐行扫 [`ok_reply_line`]，任一行含
/// `OK audio on` 即过；预算耗尽才点名。板静默拒收仍走 [`read_reply_line`] 的
/// 3s 无字节 Err，失败行为不变。
pub fn handshake(conn: &mut Rr3UartConn) -> Result<String, String> {
    std::thread::sleep(SETTLE);
    conn.rr3_flush_input()
        .map_err(|e| format!("flush 失败: {e}"))?;
    // 防御停流：残流开着也停、没流也幂等；应答行（可能混残流）读了即弃
    if let Err(e) = conn.write_all(b"AUDIO OFF\n") {
        return Err(format!("发防御 AUDIO OFF 失败: {e}"));
    }
    // OFF 应答读了即弃，但**超时可见**（d4 run#2 排障教训：`let _ =` 静默丢弃
    // 害排查——宿主侧 USB RX 冻结时这条读无声超时，故障面全靠 seq 对账反推）
    match read_reply_line(conn) {
        Ok(l) => eprintln!("[audio] 防御 OFF 应答: {l:?}"),
        Err(e) => eprintln!("[audio] 防御 OFF 应答未收到（残流/宿主侧静默，继续）: {e}"),
    }
    conn.rr3_flush_input()
        .map_err(|e| format!("二次 flush 失败: {e}"))?;
    conn.write_all(b"AUDIO ON\n")
        .map_err(|e| format!("发 AUDIO ON 失败: {e}"))?;
    // 逐行扫（判据见 [`ok_reply_line`]；每行超时同 [`read_reply_line`] 口径，
    // 总预算 [`REPLY_TIMEOUT_MS`] 兜底防板持续推流把握手拖死）
    let deadline = Instant::now() + Duration::from_millis(REPLY_TIMEOUT_MS as u64);
    loop {
        let line = read_reply_line(conn).map_err(|e| format!("读应答失败: {e}"))?;
        if ok_reply_line(&line) {
            let (pfx, _) = line.split_at(line.find("OK audio on").unwrap());
            println!(
                "[audio] board reply: OK（行内扫得；前置 {}B 流/残渣字节）",
                pfx.len()
            );
            return Ok(line);
        }
        eprintln!(
            "[audio] 应答行未见 OK（{}B，继续扫）: {:?}",
            line.len(),
            rr3_log_head(&line, 80)
        );
        if Instant::now() >= deadline {
            return Err(format!(
                "应答 {REPLY_TIMEOUT_MS}ms 预算内未见 OK audio on（末行 {line:?}）"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ON 应答判据：OK 可在行中（p3 冒烟实录形状——seq=10604 帧的 b64 payload
    /// 与 `OK audio on` 同行）；裸 `OK` 不误配（b64 字符表含 O/K）。
    #[test]
    fn ok_reply_line_accepts_stream_prefixed_mixed_line() {
        assert!(ok_reply_line("OK audio on"));
        assert!(ok_reply_line(
            "CR//D/MQAwAFEAcAARALD/8f8wAPH/MAAxALD/UQBQADEA...,\"rate\":16000,\"seq\":10604,\"topic\":\"rr3-sense/audio/pcm\"}OK audio on"
        ));
        assert!(!ok_reply_line("OK audio off"));
        assert!(!ok_reply_line("\u{5d4}\u{14}()QI"));
        assert!(!ok_reply_line("Qr//D/MQAwAFEAcAARALD/OK"));
    }

    /// b3c-③ 回归：日志截断按字符不按字节。实录形状——失步乱行经
    /// `from_utf8_lossy` 产出 U+FFFD（3 字节），79 个 ASCII 后紧跟一个时字节
    /// 80 恰在字符内部（bytes 79..82），旧实现 `&line[..80]` 在此直接 panic
    /// （读线程毙命）；本件截断语义保真且永不 panic。
    #[test]
    fn log_head_never_panics_on_multibyte_boundary() {
        let line = format!("{}\u{FFFD}", "a".repeat(79)).repeat(2);
        let head = rr3_log_head(&line, 80);
        assert_eq!(head.chars().count(), 80);
        assert!(head.starts_with(&"a".repeat(79)));
        assert!(head.ends_with('\u{FFFD}'));
        // 对照组：短行原样返回，长全 ASCII 行按字符数截断
        assert_eq!(rr3_log_head("short", 80), "short");
        assert_eq!(rr3_log_head(&"x".repeat(100), 80), "x".repeat(80));
    }
}
