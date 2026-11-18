//! 结果打印与退出码：PASS/FAIL 点名、直方图与 C12 证据文件。

use serde_json::Value;

/// ⑦ 直方图（10ms 桶）紧凑串：`[200-209ms)=12` 按桶升序空格连接。
pub(super) fn histogram_text(intervals: &[u128]) -> String {
    let mut buckets: std::collections::BTreeMap<u128, usize> = Default::default();
    for &ms in intervals {
        *buckets.entry(ms / 10).or_insert(0) += 1;
    }
    let parts: Vec<String> = buckets
        .iter()
        .map(|(k, n)| format!("[{}-{}ms)={n}", k * 10, k * 10 + 9))
        .collect();
    parts.join(" ")
}

/// 目标三元组（host 侧拼装；与 matrix.sh C12 证据门的正则同口径）。
fn target_triple() -> String {
    const SUFFIX: [(&str, &str); 2] = [("macos", "apple-darwin"), ("linux", "unknown-linux-gnu")];
    let os = std::env::consts::OS;
    let suffix = SUFFIX
        .iter()
        .find(|(k, _)| *k == os)
        .map_or_else(|| format!("unknown-{os}"), |(_, s)| (*s).to_string());
    format!("{}-{suffix}", std::env::consts::ARCH)
}

/// C12 证据文件（三样契约：环境指纹/采样窗口/样本数/原始数据，四标记词逐一
/// 在文；目标三元组正则由 target_triple 满足）。rustc -Vv 由操作面追记——
/// harness 不做进程外调用。
fn write_evidence(
    path: &str,
    device: &str,
    intervals: &[u128],
    hist: &str,
    parity_board: Option<&Value>,
    t0_unix: u64,
    t1_unix: u64,
) -> std::io::Result<()> {
    let mut text = String::new();
    text.push_str("=== 环境指纹 ===\n");
    text.push_str(&format!(
        "harness 编译目标: {}（{} / {}）\n",
        target_triple(),
        std::env::consts::ARCH,
        std::env::consts::OS
    ));
    text.push_str(&format!("串口设备: {device}\n"));
    text.push_str("对端固件: rr3-sentinel（xtensa-esp32-espidf，espidf-sentinel crate）\n");
    text.push_str("rustc -Vv: 由操作面在本文件追记（harness 不做进程外调用）\n");
    text.push_str("=== 采样窗口 ===\n");
    text.push_str(&format!(
        "UNIX 起止: {t0_unix} .. {t1_unix}（{}s）\n",
        t1_unix.saturating_sub(t0_unix)
    ));
    text.push_str("对表名义: heartbeat_ms（注册 manifest 在场值）/ tick_jitter_ms=100（profiles/esp32-mini.toml）\n");
    text.push_str("=== 样本数 ===\n");
    text.push_str(&format!(
        "心跳间隔 {} 拍；parity 逐拍对拍 1 次（{} 八组原语）\n",
        intervals.len(),
        rr3_parity_word(parity_board)
    ));
    text.push_str("=== 原始数据 ===\n");
    text.push_str(&format!("逐拍间隔 ms: {intervals:?}\n"));
    text.push_str(&format!("直方图(10ms桶): {hist}\n"));
    if let Some(p) = parity_board {
        text.push_str(&format!("板回 parity 向量: {p}\n"));
    }
    std::fs::write(path, text)
}

/// parity 面的证据档位词（有板回向量才算有效）。
fn rr3_parity_word(parity_board: Option<&Value>) -> &'static str {
    if parity_board.is_some() {
        "板回向量"
    } else {
        "未完成——证据无效"
    }
}

/// 判据输出：PASS/FAIL 逐条点名，返回 ok 供调用方累计。
pub(super) fn verdict(ok: bool, label: &str) -> bool {
    if ok {
        println!("PASS  {label}");
    } else {
        eprintln!("FAIL  {label}");
    }
    ok
}

/// C12 证据文件（三样契约）：fail 也写，原始数据留痕。
pub(super) fn rr3_evidence_write(
    ep: Option<&String>,
    device: &str,
    intervals: &[u128],
    hist: &str,
    board_parity: Option<&Value>,
    t0_unix: u64,
) {
    let Some(ep) = ep else {
        return;
    };
    let t1_unix = rr3_now_unix();
    rr3_evidence_flush(ep, device, intervals, hist, board_parity, t0_unix, t1_unix);
}

/// 证据文件写出并按结果留痕（失败不 exit——判据面已记账）。
fn rr3_evidence_flush(
    ep: &str,
    device: &str,
    intervals: &[u128],
    hist: &str,
    board_parity: Option<&Value>,
    t0_unix: u64,
    t1_unix: u64,
) {
    match write_evidence(ep, device, intervals, hist, board_parity, t0_unix, t1_unix) {
        Ok(_) => println!("OK    证据文件已写: {ep}"),
        Err(e) => eprintln!("FAIL  证据文件写出失败: {e}"),
    }
}

/// 当前 UNIX 秒（采样窗口与证据档共用）。
pub(super) fn rr3_now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 判据汇总出口：全过退 0，有 FAIL 退 1。
pub(super) fn rr3_exit_summary(fail: u32) -> ! {
    println!("---- D1 harness 判据汇总: {fail} 项 FAIL ----");
    if fail == 0 {
        println!("OK    层③ 宿主半边判据全过（编译面/三点堆采样见板侧 monitor 日志）");
        std::process::exit(0);
    }
    std::process::exit(1);
}
