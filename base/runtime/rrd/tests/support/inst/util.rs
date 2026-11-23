// SPDX-License-Identifier: Apache-2.0
//! 子进程日志与等待助手：逐行收取、按内容等行、按 key 抽数、有界轮询。
//!
//! 与 `RrdTest`/`SimPeer` 共用一份口径（两处各写一份逐行读码，EOF 与超时的
//! 处理迟早分叉）。等待判据永远是内容，`dur` 只是"不再等"的上限。
//!
//! 拆分动因：`inst.rs` 559 行，无状态助手与夹具主体分属两簇。

use super::*;

/// 从子进程 stdout 逐行收集（`RrdTest` 与 `SimPeer` 共用一份口径）。
/// 为什么必须共用：两处各写一份逐行读码，EOF 与超时的处理迟早分叉——而这两件事正是
/// "跨进程用例为什么红"最常见的真实原因（管道坏了 vs 被测行为不对，得分得开）。
/// `tag` 只出现在"读流本身报错"这一条诊断行里——读不到日志时"是谁的管道坏了"必须一眼可辨。
pub fn rr3_spawn_line_reader(stdout: ChildStdout, tag: &'static str) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut log = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match log.read_line(&mut line) {
                Ok(0) => break, // EOF：子进程已退出
                Ok(_) => {
                    let t = line.trim_end().to_string();
                    if !t.is_empty() && tx.send(t).is_err() {
                        break; // 接收端已 drop
                    }
                }
                Err(e) => {
                    // 不静默 break：把坏流本身当一条"日志"送出去，等待方在超时诊断里能看到它
                    let _ = tx.send(format!("{tag} 读输出失败: {e}"));
                    break;
                }
            }
        }
    });
    rx
}

/// 等到一条含 needle 的行（先查已缓冲的）；超时把全部已读行打进 panic，便于定位
pub fn rr3_wait_line(
    lines: &mpsc::Receiver<String>,
    seen: &mut Vec<String>,
    needle: &str,
    dur: Duration,
) -> String {
    if let Some(i) = seen.iter().position(|l| l.contains(needle)) {
        return seen.remove(i);
    }
    let deadline = Instant::now() + dur;
    loop {
        match lines.recv_timeout(Duration::from_millis(50)) {
            Ok(l) if l.contains(needle) => return l,
            Ok(l) => seen.push(l),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected)
                if Instant::now() > deadline =>
            {
                panic!("{dur:?} 内没等到 {needle:?}；已读到:\n{}", seen.join("\n"))
            }
            Err(_) => {}
        }
    }
}

/// 从一行日志里抽 `key=<数字>` 的那个数（`pid=` / `task=` / `attempt=`）。
///
/// **一律抽取、不写字面量**（A-5① 的处置）：boot 之后 task 号跟着 spawn 序走，pid 更是
/// 每次都不一样，写死就是猜。它是 `e2e_sim_peer.rs` 里那段 inline `split(..).nth(1)` 的
/// 提取版——同一段代码出现第三次就该是个函数（那一处本轮不改：既有 28 条线级用例的断言
/// 一字不许动，等 Task 13 的夹具整合再切过来）。
/// 抽不到即 panic 并回显整行：把"needle 与日志格式对不上"这种夹具 bug 当场报出来，
/// 而不是让它退化成 `unwrap_or(0)` 之后比出一个假相等。
pub fn rr3_field_from_log(line: &str, key: &str) -> u64 {
    let raw = line
        .split(key)
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_else(|| panic!("日志行 {line:?} 里没有 {key:?}"));
    raw.parse::<u64>()
        .unwrap_or_else(|e| panic!("日志行 {line:?} 里的 {key:?} 不是数字（{raw}）: {e}"))
}

/// 有界轮询一个条件：每 20ms 求值一次，最多 `dur`。返回"是否在期限内成立"。
///
/// 与 `rr3_wait_line` 同一口径：**判据是内容**（条件成立与否），`dur` 只是"不再等"的上限，
/// 所以它不是 `sleep` 的别名——`sleep(2s)` 断言的是"过了两秒"，这条断言的是"两秒内成真"。
/// 不 panic 而是返回 bool：让调用方把"没成真时该看到什么"写在自己的断言消息里
/// （例如 `/proc/{pid}` 那一格，pid 才是现场）。
pub fn rr3_wait_until<F: FnMut() -> bool>(mut pred: F, dur: Duration) -> bool {
    let started = Instant::now();
    loop {
        if pred() {
            return true;
        }
        if started.elapsed() >= dur {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}
