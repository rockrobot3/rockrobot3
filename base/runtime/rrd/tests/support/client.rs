//! 裸 UDS 测试客户端：按连接排队、按谓词取帧与诊断。

use super::*;

/// 一条连接上的裸客户端 + **按连接存在的待处理队列**（F1）。
///
/// 为什么队列必须挂在连接上而不是全局：fd 会被复用，全局 map 会把上一条连接遗留的帧投给新连接
/// （评审裁定里明令不换 fd-keyed 全局 map）。也不动生产代码——顺序无关性本就该由读侧承担。
pub struct RrdClient {
    stream: UnixStream,
    pending: VecDeque<Rr3Frame>,
}

/// 一次"挑帧"最多扫多少帧。扫不到就是**夹具 bug**（谓词写错、或 rrd 真没发这一帧），
/// 不是把预算调大就能糊过去的偶发；提为常量是因为 Task 8 加 Event 分支、Task 10 加 watchdog
/// 广播之后一条连接上几秒内的帧数会超过旧的字面值 16（M-4）。
///
/// **预算的口径与它的前提（Task 8 评审遗留 ④）**：这颗预算**同时**计"从队列弹帧"与"从 socket
/// 读帧"——`rr3_try_recv` 先弹 `pending` 再读 socket，所以队列里有存货时，后续读法会先在队列上
/// 花掉帧数。之所以不把它改成"只计 socket 读"：那样一次挑帧的**总**工作量就没有上界了
/// （队列可以被反复扫过），而本常量存在的目的正是给"扫不到即夹具 bug"这一判定一个可终止的边界。
/// 于是这里立一条硬前提：**传给 `rr3_read_where` 的谓词必须是纯函数**——同一帧重复求值结果相同、
/// 且不改外部状态。原因就在上面那句：pending 里的帧会被下一轮、下下轮重新求值，带计数器/时间戳/
/// 内部缓存的谓词会在"第二次扫到同一帧"时给出不同答案，那时预算就不再是"扫了多少帧"而是
/// "谓词心情如何"。本目标现有谓词全部是字段/类型比较（`rr3_body_split` 的枚举比对、
/// `rr3_json_of(f)["字段"] == 值`），无一例外满足此条件；新增读法请照此约束写。
const SCAN_BUDGET: usize = 64;

/// 帧的一行摘要，只用于失败诊断
fn rr3_frame_brief(f: &Rr3Frame) -> String {
    format!("{:?} {}", f.header, String::from_utf8_lossy(&f.payload))
}

/// 队列现状的一句话诊断（失败文案里"没等到 X"之后紧跟它，省得再猜哪些帧被扫过）
fn rr3_pending_dump(c: &RrdClient) -> String {
    if c.pending.is_empty() {
        return "队列里已积压 0 帧".to_string();
    }
    format!(
        "队列里已积压 {} 帧:\n{}",
        c.pending.len(),
        c.pending
            .iter()
            .map(rr3_frame_brief)
            .collect::<Vec<_>>()
            .join("\n---\n")
    )
}

/// 非 panic 版读帧：`Ok` 一帧，`Err` 把底层错误原样交回（EOF 与超时都在此分流）。
/// 队列里有存货就先交存货——这是"不匹配的帧绝不丢"的落点。
pub fn rr3_try_recv(c: &mut RrdClient) -> Result<Rr3Frame, Rr3FrameError> {
    if let Some(f) = c.pending.pop_front() {
        return Ok(f);
    }
    rr3_frame_read(&mut c.stream)
}

pub fn rr3_send(c: &mut RrdClient, frame: &Rr3Frame) {
    rr3_frame_write(&mut c.stream, frame).unwrap_or_else(|e| panic!("写帧失败: {e:?}"));
}

pub fn rr3_recv(c: &mut RrdClient) -> Rr3Frame {
    match rr3_try_recv(c) {
        Ok(f) => f,
        Err(e) => panic!("读帧失败: {e:?}"),
    }
}

/// 拆出类型 + JSON 体（本套件的体一律 JSON）
pub fn rr3_json_of(frame: &Rr3Frame) -> Value {
    let (_, body) = rr3_body_split(&frame.payload).unwrap_or_else(|e| panic!("前缀非法: {e:?}"));
    serde_json::from_slice(&body).expect("体必须是 JSON")
}

/// 按谓词挑帧（最多 `SCAN_BUDGET` 帧）。为什么必须挑：见本文件头注释的"顺序约定"。
/// 谓词不成立的帧**存回队列**（`push_front` 回原位，保持到达次序），后来的读法还能读到它们。
///
/// 实现要点：本轮扫过的帧先攒在 `deferred` 里、**扫完才回队列**。要是边扫边 `push_back`，
/// 下一轮 `rr3_try_recv` 会优先弹队列，于是同一帧被反复弹出又塞回，永远读不到 socket 上
/// 后面的帧（第一版就栽在这里：`duplicate_name…` 等 6 条用例以"64 帧内没等到"的形式变红）。
/// 又因为"塞回队列的帧之后还会被重新求值"，**谓词必须是纯函数**（见 `SCAN_BUDGET` 的注释）。
pub fn rr3_read_where<F: FnMut(&Rr3Frame) -> bool>(
    c: &mut RrdClient,
    what: &str,
    mut pred: F,
) -> Rr3Frame {
    let mut deferred: Vec<Rr3Frame> = Vec::new();
    let mut outcome: Result<Rr3Frame, String> = Err(format!(
        "{SCAN_BUDGET} 帧内没等到{what}（超出预算即视为夹具 bug：谓词写错或 rrd 确实没发这一帧）"
    ));
    for _ in 0..SCAN_BUDGET {
        match rr3_try_recv(c) {
            Ok(frame) => {
                if pred(&frame) {
                    outcome = Ok(frame);
                    break;
                }
                deferred.push(frame);
            }
            // 对端关闭（Closed / Torn*）与 5 s 读超时（Io os error 11）都不是"没等到 X"的
            // 替代品：要说清没等到什么、哪些帧还压在队列里。
            Err(e) => {
                outcome = Err(format!("没等到{what}（读帧提前结束: {e:?}）"));
                break;
            }
        }
    }
    // 本轮取走的帧按到达次序回到队列头部（倒序 push_front 正好还原正序）
    for f in deferred.into_iter().rev() {
        c.pending.push_front(f);
    }
    match outcome {
        Ok(f) => f,
        // 本轮扫过的帧此刻已经在队列里，所以 `rr3_pending_dump` 就是"积压 + 本轮读到"的完整现场
        Err(msg) => panic!("{}；{}", msg, rr3_pending_dump(c)),
    }
}

/// 一直读到 EOF 或读超时，返回收到的全部帧（队列里的也算）。
/// 用于"这条连接该被关掉/不该出现某帧"的断言——它不会 panic，所以能把
/// "rrd 有没有回 accepted"这类否定式判据钉住。终止靠的是 `rr3_connect` 设的 5 s 读超时。
pub fn rr3_drain_until_closed(c: &mut RrdClient) -> Vec<Rr3Frame> {
    let mut got = Vec::new();
    while let Ok(f) = rr3_try_recv(c) {
        got.push(f);
    }
    got
}

/// 收集侧的硬墙钟上限：`rr3_collect_within` 的终止条件是"安静 dur"，一条持续发帧的连接
/// 让它永远安静不下来。真出现那种形状（风暴广播）时，正确的结论是"帧流在 cap 内没停过"
/// 并把手上收到的帧打出来，而不是让整条测试目标挂死。
const COLLECT_WALL_CLOCK_CAP: Duration = Duration::from_secs(5);

/// 在 `dur` 内把这一条连接上**能读到的帧全部收走**（不 panic：读不到就是这一轮结束）。
/// 队列里的存货先算进返回值（`rr3_try_recv` 的既有口径），用于"这条连接上不该出现 X"的否定断言。
/// 它是**破坏性**读法：取走的帧**不回队列**（与本文件头和 `rr3_read_where` 那种"未匹配帧存回队列、
/// 可重读"的口径正相反）。
/// **只准用在用例末尾**——之后同一连接不得再有读法，否则后续否定断言会空转、正向读法会白等 5 s。
///
/// 为什么不能直接用 `rr3_drain_until_closed`：那条的终止条件是 `rr3_connect` 设的 **5 s 读超时**
/// （`support/mod.rs` 的 `rr3_connect`），拿它做"150 ms 内没回显即通过"的判据会让每条这样的用例
/// 白等 4.85 s，Task 10/11 的否定断言一多就把测试时长推到分钟级。这里临时把读超时压到 `dur`、
/// **收完把原值设回去**：设回去不是洁癖——同一连接后来的读法要靠 5 s 等到慢副作用（重启、
/// watchdog），留下 150 ms 就等于把别的用例改成随机红。
///
/// 终止条件之外还有一条**墙钟上限**（`COLLECT_WALL_CLOCK_CAP`，Task 8 验收项 A-1）：
/// "安静 dur"这一条只挡得住"帧流会停"的连接，一条持续发帧的连接能让它永远收不完、
/// 于是整条测试目标挂到 cargo 超时且诊断信息为零。超限即 panic 并打印已收到的帧数与类型。
pub fn rr3_collect_within(c: &mut RrdClient, dur: Duration) -> Vec<Rr3Frame> {
    let saved = c.stream.read_timeout().ok();
    c.stream
        .set_read_timeout(Some(dur))
        .expect("临时读超时不可设");
    let started = Instant::now();
    let mut got = Vec::new();
    while let Ok(f) = rr3_try_recv(c) {
        got.push(f);
        if started.elapsed() > COLLECT_WALL_CLOCK_CAP {
            // 恢复超时要尽力做：panic 路径上管不上，至少不是"把一个 5 s 读超时留给别的用例"
            c.stream.set_read_timeout(saved.flatten()).ok();
            let kinds: Vec<String> = got
                .iter()
                .map(|f| {
                    rr3_body_split(&f.payload)
                        .map(|(ty, _)| format!("{ty:?}"))
                        .unwrap_or_else(|_| "坏前缀".into())
                })
                .collect();
            panic!(
                "帧流在 {COLLECT_WALL_CLOCK_CAP:?} 内没停过（收到 {} 帧）: {}",
                got.len(),
                kinds.join(",")
            );
        }
    }
    c.stream
        .set_read_timeout(saved.flatten())
        .expect("恢复读超时失败");
    got
}

/// 第一条 Response
pub fn rr3_read_rpc_frame(c: &mut RrdClient) -> Rr3Frame {
    rr3_read_where(c, " Response", |f| {
        rr3_body_split(&f.payload)
            .map(|(ty, _)| ty == Rr3PayloadType::Response)
            .unwrap_or(false)
    })
}

pub fn rr3_read_rpc(c: &mut RrdClient) -> Value {
    let f = rr3_read_rpc_frame(c);
    rr3_json_of(&f)
}

/// 第一条 `rr3-runtime/module/state` 广播且 module/phase 匹配，返回体
///
/// **按到达序取第一帧**。要判"某一世的帧"（重启用例里同一 module/phase 会出现在
/// 两世）必须用 [`rr3_read_state_task`]——否则队列里一条上一世的残留帧就会当成
/// 本世的，负载下表现为偶发红（实测 `e2e_spawn_restart` 即此形态）。
pub fn rr3_read_state(c: &mut RrdClient, module: &str, phase: &str) -> Value {
    rr3_read_state_task(c, module, phase, None)
}

/// 同 [`rr3_read_state`]，但可要求 `src_task` 必须等于给定值（`None` = 不限定）。
/// 这是"按身份认帧、不按到达序猜"的可执行版本：不匹配的帧被跳过而不是当成结果。
pub fn rr3_read_state_task(
    c: &mut RrdClient,
    module: &str,
    phase: &str,
    task: Option<u64>,
) -> Value {
    let f = rr3_read_where(
        c,
        &format!("state 广播 {module}/{phase}（task={task:?}）"),
        |f| match rr3_body_split(&f.payload) {
            Ok((Rr3PayloadType::Notification, body)) => {
                let Ok(v) = serde_json::from_slice::<Value>(&body) else {
                    return false;
                };
                v["topic"] == "rr3-runtime/module/state"
                    && v["module"] == module
                    && v["phase"] == phase
                    && task.is_none_or(|t| v["src_task"].as_u64() == Some(t))
            }
            _ => false,
        },
    );
    rr3_json_of(&f)
}
