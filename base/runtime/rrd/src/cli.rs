//! 命令行与总线路径解析（纯逻辑，平台无关）。

use std::path::{Path, PathBuf};

use rr_ipc::{RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT};

/// rrd 运行配置
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RrdConfig {
    pub socket: PathBuf,
    pub overlay: Option<PathBuf>,
    /// 装配单路径（`--profile`，0.0.2 方案 D2 / Q3 裁定）。装机**正门**：enabled_slots 决定起谁。
    /// 与 `--manifest` **同给当场拒**（Q3 裁定细则：两路径共用 plan_boot 仲裁与 supervisor
    /// spawn，不双真值——两参数同给即非 0 退出，不留"谁赢"的歧义）。
    pub profile: Option<PathBuf>,
    /// 待启动模块的 manifest（绝对路径，可重复 `--manifest`）。
    /// **已落地**（Task 11b）：`supervisor` 按 `start_priority` **升序、稳定**排后逐条 boot spawn，
    /// 同优先级保持本 CLI 顺序（`sort_by_key` 是稳定排序）。CLI 序只是"同优先级之间的 tie-break"，
    /// 不是 spawn 序本身。
    pub manifests: Vec<PathBuf>,
    /// 重启退避基数（表值 × base）。生产 1000ms；集成测试压到 20ms 才跑得完上限序列。
    /// 下界 1 与上界 `RR3_RESTART_BASE_MS_MAX` 都在 parse 处强制（见 `parse_restart_base`）。
    pub restart_base_ms: u64,
    /// 连接接受后多久没完成注册判为启动失败（legacy 教训 76c0ad8：注册前不判活）
    pub pre_register_timeout_ms: u64,
    /// 唯一 watchdog 线程的扫描间隔。生产默认 200ms：远小于任何真实
    /// `watchdog_timeout_ms`，所以 tick 误差不参与"判活/判死"的时机争议。
    /// **0 在 parse 处就拒，不 `.max(1)` 静默夹**：0 的语义是"每毫秒全表扫一遍"，
    /// 那是一条烧核的忙循环，不是"差一点点"。静默夹成 1 等于横幅与实况各说一套
    /// ——对照 `--restart-base-ms` 的上界处理：策略结论必须在启动时让运维听到。
    pub watchdog_tick_ms: u64,
    /// 注入给 boot 子进程的 `RR3_SIM_PEER_ROLE` 值（spec §5.4"由 rrd 注入"的那件环境）。
    /// **原样透传、rrd 不校验取值**（E-6）：role 的合法集是**夹具**的约定，不是守护进程的契约；
    /// rrd 一旦校验就要跟着夹具新增的脚本改，那是第二处真值。默认 `resident`。
    pub boot_role: String,
    /// 契约目录（`--contracts`，0.0.2 T5）：运行期定位 `topics.toml` 与 `slots.toml`。
    /// 默认 None → 编译期常量（仓内开发零变化）；deb 装机后编译期路径不存在，
    /// payload 由 `--contracts /usr/lib/rockrobot3/contracts` 指向包内真值。
    pub contracts: Option<PathBuf>,
}

/// 装配面必选门（基-23，L-31 的出口）：既无 `--profile` 又无 `--manifest` 即拒。
/// 没有装配面 = 装载门整体被跳过（None 臂空 hal_bindings 空降级集、十条校验一个
/// 判定位都不跑而 rrd 退出码 0）——那正是"门在什么条件下被跳过"这道缝本身。
/// 正门（--profile，装机）与后门（--manifest，调试）二选一，Q3 裁定的两路径形状
/// 不变，只是"两可都不选"从此不是合法启动方式。拒绝发生在 parse 处，非 0 退出。
const ASSEMBLY_SURFACE_REQUIRED: &str = "启动缺装配面：--profile（装机正门）与 --manifest（调试后门）至少给其一——两者都不给时装载门整体不跑（rrd 空起、十条校验零判定位），基-23 起不再合法";

const USAGE: &str = "用法: rrd [--socket <path>] [--contracts <dir>] (--profile <path> | --manifest <path> | --manifest <path> --manifest <path> ...) [--topics-overlay <path>] [--restart-base-ms <n>] [--pre-register-timeout-ms <n>] [--watchdog-tick-ms <n>] [--boot-role <role>]\n说明: --contracts 定位包内契约目录（topics.toml/slots.toml，缺省编译期仓内路径——deb 装机必给）。--boot-role 的值原样注入 boot 子进程的 RR3_SIM_PEER_ROLE，取值由夹具解释，rrd 不校验（E-6）。装配面必选（基-23）：--profile（装机正门）与 --manifest（调试后门）至少给其一；两者同给也拒（Q3：两路径共用仲裁与 spawn，不双真值）。";

/// `--profile` 与 `--manifest` 同给当场拒的共用文案（两处分支同一句，不许各说一套）。
/// 拒绝发生在 parse 处：非 0 退出，没有任何"后给者赢"或"合并两份"的歧义路径（Q3 裁定细则）。
const PROFILE_MANIFEST_CONFLICT: &str = "--profile 与 --manifest 不得同给：profile 是装机正门（enabled_slots 决定起谁），--manifest 是调试后门（0.0.2 方案 Q3 裁定，两路径共用仲裁与 spawn，不双真值）";

/// `--restart-base-ms` 的上界：退避表末档（60）乘上去仍要不溢出 u64。
/// 为什么在 CLI 就拒、而不是让 `rr3_reg_death` 的饱和乘法默默兜住：饱和成 `u64::MAX`
/// 意思是"永不重启"，那是策略结论而不是算术意外，运维必须在启动时就听到，
/// 而不是模块崩了之后才发现它永远不会回来（`rr3_reg_death` 的饱和仍在，作为纵深防御）。
/// 分母 60 = `RR3_RESTART_BACKOFF` 的末档，由 `cli::tests::restart_base_ms_ceiling_is_tied_to_the_backoff_table`
/// 按表重算并钉住——改表不改这里会让那条用例红，不会出现两处各说一套。
pub const RR3_RESTART_BASE_MS_MAX: u64 = u64::MAX / 60;

fn need_value(flag: &str, args: &[String], i: usize) -> Result<String, String> {
    let Some(v) = args.get(i + 1) else {
        return Err(format!("{flag} 缺参数\n{USAGE}"));
    };
    Ok(v.clone())
}

/// 路径型开关取值：以 `-` 开头的一律当"下一个开关名"拒掉，不作值。
/// 否则 `rrd --socket --topics-overlay` 会把 `--topics-overlay` 当成套接字路径吞下，
/// 叠加层静默丢失（fail-open）。数字型开关由 `parse_ms` 拒 `-…`，口径一致。
fn need_path_value(flag: &str, args: &[String], i: usize) -> Result<String, String> {
    let raw = need_value(flag, args, i)?;
    if raw.starts_with('-') {
        return Err(format!(
            "{flag} 缺参数：收到疑似开关的 {raw}（路径值不得以 - 开头）\n{USAGE}"
        ));
    }
    Ok(raw)
}

/// 默认总线路径：env 覆盖优先（与 rr-ipc 常量一致）。
/// 环境值的读取由调用方注入（`env`），本函数**不碰进程环境**——理由见 `rr3_cli_parse_env`。
fn rr3_default_socket(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    PathBuf::from(env(RR3_BUS_PATH_ENV).unwrap_or_else(|| RR3_UDS_PATH_DEFAULT.to_string()))
}

/// 解析参数（生产入口：默认值从进程环境取）。
pub fn rr3_cli_parse(args: &[String]) -> Result<RrdConfig, String> {
    rr3_cli_parse_env(args, &|k| std::env::var(k).ok())
}

/// 解析参数，默认值来源是注入的 `env` 而不是进程环境。
///
/// 为什么要有这一层：`RrdConfig.socket` 的默认值来自 `RR3_BUS_PATH_ENV`，而进程 env 是
/// **全局可变态**。上一版的测试为了断言这条默认值，只能在用例里 `set_var`/`remove_var`，
/// 再挂一把 `OnceLock<Mutex<()>>`（ENV_LOCK）把同批用例串行化——那是把生产代码的环境依赖
/// 搬进测试来还债：锁一漏，并行时"默认值"读到的就是别人刚设的值（假绿）；edition 升 2024 后
/// `set_var` 还得写 unsafe。现在默认值走参数：测试塞一张表即可，不碰全局态、也不需要锁；
/// 生产路径（`rr3_cli_parse`）的形状对 `main.rs` 与既有调用方保持不变。
///
/// 其余门禁不变：未知选项 / 缺参数 / 值以 `-` 开头（路径型）/ 非数字一律报错，
/// 不静默回落默认值（fail-closed）。
pub fn rr3_cli_parse_env(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<RrdConfig, String> {
    let mut cfg = RrdConfig {
        socket: rr3_default_socket(env),
        overlay: None,
        profile: None,
        manifests: Vec::new(),
        restart_base_ms: 1000,
        pre_register_timeout_ms: 3000,
        watchdog_tick_ms: 200,
        boot_role: "resident".to_string(),
        contracts: None,
    };
    let mut i = 0;
    while i < args.len() {
        i = rr3_cli_arg(&mut cfg, args, i)?;
    }
    rr3_cli_finish(cfg)
}

/// 末尾两道语义门：socket 非空 → 装配面必选（形状错误已在解析循环内拒）。
fn rr3_cli_finish(cfg: RrdConfig) -> Result<RrdConfig, String> {
    rr3_socket_ok(&cfg)?;
    rr3_surface_ok(&cfg)?;
    Ok(cfg)
}

/// socket 非空门。
fn rr3_socket_ok(cfg: &RrdConfig) -> Result<(), String> {
    if cfg.socket.as_os_str().is_empty() {
        return Err("--socket 不得为空".to_string());
    }
    Ok(())
}

/// 装配面必选门（基-23）：放在 socket 空检查之后——参数形状错误（缺值/未知开关）
/// 在循环内已拒，双缺是"形状合法但语义不合法"的最后一道，报错也要最后说。
fn rr3_surface_ok(cfg: &RrdConfig) -> Result<(), String> {
    if cfg.profile.is_none() && cfg.manifests.is_empty() {
        return Err(format!("{ASSEMBLY_SURFACE_REQUIRED}\n{USAGE}"));
    }
    Ok(())
}

/// 解析单个参数位，返回下一个待解析下标：形状门（未知选项 / 缺参数 / 值以 `-`
/// 开头 / 非数字）全在此处拒，不静默回落默认值（fail-closed）。
fn rr3_cli_arg(cfg: &mut RrdConfig, args: &[String], i: usize) -> Result<usize, String> {
    let opt = args[i].as_str();
    let Some((_, handle)) = RR3_ARG_HANDLERS.iter().find(|(name, _)| *name == opt) else {
        return Err(format!("未知选项 {opt}\n{USAGE}"));
    };
    handle(cfg, args, i)
}

/// 开关 → 族处理器（路径/数值/杂项三族；未知开关由查表失败点名）。
type Rr3ArgHandler = fn(&mut RrdConfig, &[String], usize) -> Result<usize, String>;
const RR3_ARG_HANDLERS: [(&str, Rr3ArgHandler); 11] = [
    ("--socket", rr3_cli_path),
    ("--topics-overlay", rr3_cli_path),
    ("--contracts", rr3_cli_path),
    ("--profile", rr3_cli_path),
    ("--manifest", rr3_cli_path),
    ("--restart-base-ms", rr3_cli_num),
    ("--pre-register-timeout-ms", rr3_cli_num),
    ("--watchdog-tick-ms", rr3_cli_num),
    ("--boot-role", rr3_cli_misc),
    ("-h", rr3_cli_misc),
    ("--help", rr3_cli_misc),
];

/// 路径型参数族：值走 `need_path_value`（缺值/疑似开关一律拒，不静默回落）。
fn rr3_cli_path(cfg: &mut RrdConfig, args: &[String], i: usize) -> Result<usize, String> {
    let opt = args[i].as_str();
    let Some((_, set)) = RR3_PATH_OPTS.iter().find(|(name, _)| *name == opt) else {
        return Err(format!("未知选项 {opt}\n{USAGE}"));
    };
    let p = PathBuf::from(need_path_value(opt, args, i)?);
    set(cfg, p)?;
    Ok(i + 2)
}

/// 路径型参数表：`--名称 → 落位函数`（含 profile/manifest 互斥判据）。
type Rr3PathSet = fn(&mut RrdConfig, PathBuf) -> Result<(), String>;
const RR3_PATH_OPTS: [(&str, Rr3PathSet); 5] = [
    ("--socket", |c, p| {
        c.socket = p;
        Ok(())
    }),
    ("--topics-overlay", |c, p| {
        c.overlay = Some(p);
        Ok(())
    }),
    ("--contracts", |c, p| {
        c.contracts = Some(p);
        Ok(())
    }),
    ("--profile", |c, p| {
        rr3_profile_conflict(c)?;
        c.profile = Some(p);
        Ok(())
    }),
    ("--manifest", |c, p| {
        rr3_mf_conflict(c)?;
        c.manifests.push(p);
        Ok(())
    }),
];

/// `--profile` 与已收 `--manifest` 互斥（反之同）。
fn rr3_profile_conflict(cfg: &RrdConfig) -> Result<(), String> {
    if cfg.manifests.is_empty() {
        Ok(())
    } else {
        Err(format!("{PROFILE_MANIFEST_CONFLICT}\n{USAGE}"))
    }
}

fn rr3_mf_conflict(cfg: &RrdConfig) -> Result<(), String> {
    if cfg.profile.is_none() {
        Ok(())
    } else {
        Err(format!("{PROFILE_MANIFEST_CONFLICT}\n{USAGE}"))
    }
}

/// 数值型参数族：非负整数毫秒；watchdog 额外拒 0（忙循环不是"更快一点"）。
fn rr3_cli_num(cfg: &mut RrdConfig, args: &[String], i: usize) -> Result<usize, String> {
    let opt = args[i].as_str();
    let Some((_, set)) = RR3_NUM_OPTS.iter().find(|(name, _)| *name == opt) else {
        return Err(format!("未知选项 {opt}\n{USAGE}"));
    };
    set(cfg, &need_value(opt, args, i)?)?;
    Ok(i + 2)
}

/// 数值开关表：`--名称 → 落位函数`（各值自己的门禁住落位函数内）。
type Rr3NumSet = fn(&mut RrdConfig, &str) -> Result<(), String>;
const RR3_NUM_OPTS: [(&str, Rr3NumSet); 3] = [
    ("--restart-base-ms", |c, raw| {
        c.restart_base_ms = parse_restart_base(raw)?;
        Ok(())
    }),
    ("--pre-register-timeout-ms", |c, raw| {
        c.pre_register_timeout_ms = parse_ms("--pre-register-timeout-ms", raw)?;
        Ok(())
    }),
    ("--watchdog-tick-ms", |c, raw| {
        let n = parse_ms("--watchdog-tick-ms", raw)?;
        if n == 0 {
            return Err(
                "--watchdog-tick-ms 需为正整数毫秒（0 是忙循环，不是\"更快一点\"）".to_string(),
            );
        }
        c.watchdog_tick_ms = n;
        Ok(())
    }),
];

/// 杂项参数族：boot-role 的两道拒法 + help。
fn rr3_cli_misc(cfg: &mut RrdConfig, args: &[String], i: usize) -> Result<usize, String> {
    match args[i].as_str() {
        "--boot-role" => rr3_cli_role(cfg, args, i),
        "-h" | "--help" => {
            println!("{USAGE}");
            Ok(i + 1)
        }
        other => Err(format!("未知选项 {other}\n{USAGE}")),
    }
}

/// boot-role 取值：形状门过后原样透传（**不校验取值**，E-6）。
fn rr3_cli_role(cfg: &mut RrdConfig, args: &[String], i: usize) -> Result<usize, String> {
    let raw = need_value("--boot-role", args, i)?;
    rr3_boot_role_guard(&raw)?;
    // **不校验取值**（E-6）：合法集是夹具的约定，rrd 只透传。
    cfg.boot_role = raw;
    Ok(i + 2)
}

/// boot-role 取值形状门：空值与疑似开关一律拒。
///
/// 两道拒法都只为一个目的：**role 进了子进程才炸，就查不出是谁给的**。
/// 空值：`sim_peer` 会打"未知 RR3_SIM_PEER_ROLE: "（一个空名字的错误行，
/// 既指不出参数也指不出用例）；疑似开关的值：`--boot-role --socket` 会把
/// `--socket` 当成 role 吞下、于是**下一个真开关**丢了（与 `need_path_value`
/// 当年修的"叠加层被静默丢掉"同一形状，只是这里丢的是 socket 路径）。
fn rr3_boot_role_guard(raw: &str) -> Result<(), String> {
    if raw.is_empty() {
        return Err(format!(
            "--boot-role 不得为空（role 由夹具解释，空值只会得到一条查不出来源的\"未知 RR3_SIM_PEER_ROLE\"）\n{USAGE}"
        ));
    }
    if raw.starts_with('-') {
        return Err(format!(
            "--boot-role 缺参数：收到疑似开关的 {raw}（role 不得以 - 开头）\n{USAGE}"
        ));
    }
    Ok(())
}

fn parse_ms(flag: &str, raw: &str) -> Result<u64, String> {
    raw.parse::<u64>()
        .map_err(|_| format!("{flag} 需为非负整数毫秒，收到 {raw}"))
}

/// 把 `--socket` 解析成绝对路径：横幅回显与 `UnixListener::bind` 必须共用同一个值。
///
/// 用 `std::path::absolute` 而不是 `topics_load` 那套 `std::fs::canonicalize`：bind 之前
/// 套接字文件**必然还不存在**，canonicalize 会以 ENOENT 拒掉一个完全合法的待创建路径。
/// 代价是这条路径不解析符号链接、也不保证唯一——对"横幅说的就是绑的地方"这个目的足够。
/// 失败（极少数畸形路径，如空字符串配空 CWD）如实报错，不回落原词：静默回落等于把
/// "运维照横幅连错地方"这条缝留在原地。
pub fn rr3_socket_abs(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path)
        .map_err(|e| format!("套接字路径 {} 无法解析为绝对路径: {e}", path.display()))
}

/// `--restart-base-ms` 专用门禁：`0` 与"末档乘溢出"都在 parse 处拒掉，不落默认值。
///
/// 不复用 `parse_ms`：那条口径是"非负整数"（`--pre-register-timeout-ms 0` 合法，conn 层
/// 按 `max(1)` 用），而退避基数为 0 是**有语义的坏值**——它不会让 rrd 报错，只会让
/// `always` 策略的模块以零延迟无限重启。上界同理：过了界 `delay_ms` 饱和成 `u64::MAX`，
/// 表现是"模块再也不回来"，两者都必须在启动那一刻就听见。
fn parse_restart_base(raw: &str) -> Result<u64, String> {
    let v = parse_ms("--restart-base-ms", raw)?;
    rr3_restart_base_ok(v)?;
    Ok(v)
}

/// 退避基数两侧界：下界 1（0 = 热重启循环）、上界防末档乘算溢出 u64。
fn rr3_restart_base_ok(v: u64) -> Result<(), String> {
    const FLAG: &str = "--restart-base-ms";
    if v == 0 {
        return Err(format!(
            "{FLAG} 需为正整数毫秒：0 会让重启退避恒为 0（热重启循环，spec §5.3）\n{USAGE}"
        ));
    }
    if v > RR3_RESTART_BASE_MS_MAX {
        return Err(format!(
            "{FLAG} {v} 过大：退避末档乘上它会溢出 u64（饱和 = 永不重启），最大 {RR3_RESTART_BASE_MS_MAX}\n{USAGE}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
