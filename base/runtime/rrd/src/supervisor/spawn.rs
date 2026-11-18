//! 起进程的唯一漏斗：低电闸、旧 handle 清理、环境注入与成功/失败记账。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::job::Rr3SpawnJob;
use super::schedule::{rr3_absorb_exit, rr3_low_power_retry, rr3_schedule_due};
use super::unit_log::rr3_open_unit_log;
use super::RR3_LOW_POWER_RETRY_MS;
use crate::daemon::rr3_now_ms;
use crate::registry::{Rr3RestartPlan, RrdRegistry};
use crate::{rr3_lock, rr3_log};

/// 起进程的唯一漏斗：boot 与到期重启都过这里（Task 10 的 D-7 同一条教训——
/// 护栏长在唯一的入口上，比在每个调用点各写一遍强）。
///
/// 次序**固定**（spec §5.4）：`rr3_reg_own(name)`（在锁里）→ `Command::spawn` →
/// 成功后 `children.insert` + `spawn …` 日志；起进程失败 → `rr3_reg_unown` +
/// 立刻 `rr3_reg_child_exit(&facts, false, now)` 走 EarlyExit 记账（E-3 末段：吃预算、有上限）。
/// **为什么 own 在前**：子进程可能在同一毫秒内注册完并死掉，晚一步 own 会让 conn 那一路
/// 先看到 EOF 并按"非 owned"处理 → 广播 `unregistered`，把退出码能给出的 `failed` 抢掉。
/// 这是本设计里唯一一处"次序决定语义"的地方，故测试里有对应判据（S1 的相位断言）。
// 参数是"启动一次所需的全部现场"（registry/子进程表/到期堆/任务单各司其职），
// 收拢成结构体只为让 clippy 闭嘴反而增加一层无谓的打包拆包——显式豁免。
#[allow(clippy::too_many_arguments)]
pub(super) fn rr3_boot_spawn(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    job: &Rr3SpawnJob,
    attempt: u32,
    bus_path: &Path,
    boot_role: &str,
) {
    let name = job.facts.name.clone();
    // 低电兜底（0.0.2 方案 Q4 裁定）：全局低电旗生效后**停止一切新 spawn**——boot 与
    // 重启共用本漏斗（D-7：护栏长在唯一入口上），一问全挡。挂死的旧进程不在此杀：
    // "杀而不重启"属关机策略，归 rr3-power 演进与 M6；本面只保证不再起新的。
    if rr3_boot_lp_hold(shared, due, job, &name, attempt) {
        return;
    }
    rr3_boot_kill_old(children, &name);
    rr3_boot_launch(
        shared, t0, children, due, job, &name, attempt, bus_path, boot_role,
    );
}

/// own → spawn → 成功入表 / 失败记账（own 必须在前：子进程可能同毫秒内注册并死掉）。
#[allow(clippy::too_many_arguments)]
fn rr3_boot_launch(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    children: &mut BTreeMap<String, Child>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    job: &Rr3SpawnJob,
    name: &str,
    attempt: u32,
    bus_path: &Path,
    boot_role: &str,
) {
    rr3_lock(shared).rr3_reg_own(name);
    match rr3_spawn_build(job, bus_path, boot_role).spawn() {
        Ok(child) => rr3_spawn_ok(children, name, child, job, attempt),
        Err(e) => rr3_spawn_err(shared, t0, due, job, name, e),
    }
}

/// 低电旗门：置位时留痕并把计划延后重试（返回 true = 本拍不起进程）。
fn rr3_boot_lp_hold(
    shared: &Arc<Mutex<RrdRegistry>>,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    job: &Rr3SpawnJob,
    name: &str,
    attempt: u32,
) -> bool {
    if !rr3_lock(shared).rr3_reg_lp_is_set() {
        return false;
    }
    rr3_log(&format!(
        "低电兜底：拒绝 spawn {name}（全局低电旗已置位，停止新 spawn）"
    ));
    // 计划不丢：旗解除后模块必须回得来。丢了就是"一次低电段永久少一个模块"，
    // rr3-power 自己死掉更会锁死解除路径（只有它能发 battery/ok）。
    rr3_log(&format!(
        "低电兜底：{name} 的启动计划保留，{RR3_LOW_POWER_RETRY_MS}ms 后重试（旗解除即放行）"
    ));
    rr3_schedule_due(due, rr3_low_power_retry(name, attempt, &job.facts.entry));
    true
}

/// 同名旧 handle 先杀后收：覆盖 `Child` 不等于杀进程（std 的 drop 既不 kill 也不 wait）。
fn rr3_boot_kill_old(children: &mut BTreeMap<String, Child>, name: &str) {
    if let Some(mut old) = children.remove(name) {
        // 走到这里只有一种形状：到期判死挂住的子进程（conn/收尸那两路都会先把 handle 交出来）。
        // 不杀就是"两份同名进程并存 + 旧 handle 被 insert 覆盖成永久孤儿"。
        let _ = old.kill();
        let _ = old.wait(); // 当场收掉，绝不留僵尸；这里阻塞是故意的（就等这一个）
        rr3_log(&format!("杀掉挂死的 {name} pid={}", old.id()));
    }
}

/// 起进程命令的组装：E-6 三件环境（连哪儿/是谁/做什么）+ 四路后端覆盖与描述符、
/// 执行轴状态，空值一律不注入；stderr 接 per-unit 日志（append），打不开回退 inherit。
fn rr3_spawn_build(job: &Rr3SpawnJob, bus_path: &Path, boot_role: &str) -> Command {
    let name = job.facts.name.as_str();
    // E-6 的三件环境：连哪儿（rrd 自己的 socket 绝对路径）、是谁（该 job 的 manifest
    // 绝对路径，每次重启重读）、做什么（`--boot-role` 的值，原样透传、rrd 不解释）。
    // 物理绑定描述符（批D④）只在非空时追加注入：空 = 装配单未给 [hardware]（或
    // --manifest 直起），模块侧 rr-hal 维持"无描述符"fail-closed，应急 env 可救。
    let mut cmd = Command::new(&job.entry);
    cmd.env("RR3_BUS_PATH", bus_path)
        .env("RR3_MANIFEST", &job.manifest_path)
        .env("RR3_SIM_PEER_ROLE", boot_role);
    rr3_spawn_env(&mut cmd, job);
    // 见文件头的 stdout 纪律：子进程输出绝不进 rrd 自己那条管道（stdout null 不动）。
    // stderr 接 per-unit 日志文件（门 r 留痕通道定形，批N4 件八）：模块留痕唯一可达
    // 通道——append 语义让"一次重启后仍能取到崩溃前的最后 N 行"（§17 同轨判据）天然
    // 成立；文件打不开回退 inherit 并点名，观测通道故障降级、不挡起跑。
    let stderr_stdio = rr3_unit_stderr(bus_path, name);
    cmd.stdout(Stdio::null()).stderr(stderr_stdio);
    // 父死子收（Linux）：rrd 崩溃/被杀时子进程不能变孤儿继续占 UART/声卡。
    // 2026-11-09 真板实录：部署停旧栈后新 sense 连报两次 `HAL 设备忙`——旧 sense
    // 的音频读线程仍持有 /dev/ttyAMA0 flock。PDEATHSIG 让内核在父进程死亡时给
    // 子进程补 SIGTERM，孤儿窗口从"等模块自己发现总线断了"缩到信号投递延迟。
    // pre_exec 只调 prctl（async-signal-safe，无分配），失败按 spawn 错误上抛。
    #[cfg(target_os = "linux")]
    rr3_pdeath_signal(&mut cmd);
    cmd
}

/// pre_exec 装 PDEATHSIG：父进程死亡由内核补 SIGTERM。
#[cfg(target_os = "linux")]
fn rr3_pdeath_signal(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY：闭包内仅一次 prctl 调用并读 errno，不触碰父进程状态。
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// per-unit 日志 stderr；打不开回退 inherit 并点名（观测降级不挡起跑）。
fn rr3_unit_stderr(bus_path: &Path, name: &str) -> Stdio {
    match rr3_open_unit_log(bus_path, name) {
        Ok(f) => Stdio::from(f),
        Err(e) => {
            rr3_log(&format!(
                "per-unit 日志打不开（{name}）: {e} —— stderr 回退 inherit"
            ));
            Stdio::inherit()
        }
    }
}

/// 可选面注入：物理绑定描述符、HAL 后端与音频/舵机/显示三面覆盖、执行轴状态，
/// 空值/None 一律不注入（模块侧折 fail-closed 缺省）。
fn rr3_spawn_env(cmd: &mut Command, job: &Rr3SpawnJob) {
    if !job.hal_bindings.is_empty() {
        cmd.env("RR3_HAL_BINDINGS", &job.hal_bindings);
    }
    // P0-3 根因门：装配单声明的 HAL 后端注入子进程 `RR3_HAL_BACKEND`——后端选择
    // 是部署事实，必须进装配单显式数据，不靠隐式缺省（漏 env 静默落 sim = 生产
    // 假验证）。空串 = --manifest 调试路径不注入（模块侧走平台缺省）。
    if !job.hal_backend.is_empty() {
        cmd.env("RR3_HAL_BACKEND", &job.hal_backend);
    }
    // 音频采集面后端覆盖（组合档 Pi×ESP32 用 esp32）：注入 `RR3_HAL_AUDIO_BACKEND`，
    // 让音频面独占 UART 而不影响全局后端。None 不注入（音频面回落全局 hal_backend）。
    rr3_env_opt(cmd, "RR3_HAL_AUDIO_BACKEND", job.audio_backend.as_deref());
    // 舵机面后端覆盖（P0-4 组合档归属）：组合档舵机在 ESP32 板上时设 esp32。
    // None 不注入（舵机面回落全局 hal_backend）。
    rr3_env_opt(cmd, "RR3_HAL_SERVO_BACKEND", job.servo_backend.as_deref());
    // 显示面后端覆盖（P0-4 组合档归属）：组合档 OLED/点阵在 ESP32 板上时设 esp32。
    // None 不注入（显示面回落全局 hal_backend）。
    rr3_env_opt(
        cmd,
        "RR3_HAL_DISPLAY_BACKEND",
        job.display_backend.as_deref(),
    );
    // 执行轴状态（批N3 件三）：空值不注入——模块侧折保守缺省（未标定不得自动），
    // 与 hal_bindings 同一条"空 = 无真值，fail-closed"纪律
    if !job.axis_state.is_empty() {
        cmd.env("RR3_ACT_AXIS_STATE", &job.axis_state);
    }
}

/// 可选后端覆盖注入：None 不注入（对应能力面回落全局后端）。
fn rr3_env_opt(cmd: &mut Command, key: &str, v: Option<&str>) {
    if let Some(v) = v {
        cmd.env(key, v);
    }
}

/// 起进程成功：入子进程表 + 点名日志；`attempt>0` 才附尾注（boot 那次没有
/// "第几次重启"可说——S1 正是靠这个后缀把第一世与第二世分开）。
fn rr3_spawn_ok(
    children: &mut BTreeMap<String, Child>,
    name: &str,
    child: Child,
    job: &Rr3SpawnJob,
    attempt: u32,
) {
    let pid = child.id();
    children.insert(name.to_string(), child);
    let tail = if attempt > 0 {
        format!(" attempt={attempt}")
    } else {
        String::new()
    };
    rr3_log(&format!(
        "spawn {name} pid={pid} entry={}{}",
        job.entry.display(),
        tail
    ));
}

/// 起进程失败：撤所有权标记 + EarlyExit 记账（已知边界②：瞬态错误或被外部占名，
/// 走吃预算、有上限那一路）。撤标记是必须的——否则同名新实例的 EOF 会被永远钉在
/// `DeferredToExitStatus` 上。
fn rr3_spawn_err(
    shared: &Arc<Mutex<RrdRegistry>>,
    t0: Instant,
    due: &mut Vec<(Instant, Rr3RestartPlan)>,
    job: &Rr3SpawnJob,
    name: &str,
    e: std::io::Error,
) {
    rr3_lock(shared).rr3_reg_unown(name);
    let reason = format!("起进程失败: {e}");
    let verdict = rr3_lock(shared).rr3_reg_child_exit(&job.facts, false, rr3_now_ms(t0));
    rr3_absorb_exit(shared, name, &reason, verdict, due);
}
