//! supervisor 的请求类型与 "我能起谁" 条目：manifest 解析与 entry 可执行门。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::manifest::{rr3_manifest_abs, rr3_manifest_parse, Rr3ManifestFacts};
use crate::registry::Rr3RestartPlan;
use crate::rr3_log;

/// 一个"我能起谁"的条目（E-1：`jobs` 的全集只由 `--manifest` 填）。
#[derive(Debug, Clone)]
pub struct Rr3SpawnJob {
    /// 每次重启重读（E-2：不加 `Rr3ManifestFacts.text`，身份来源就是这个路径）
    pub manifest_path: PathBuf,
    /// 已按 manifest 所在目录解析成绝对路径，且 E-3 的存在性 + 可执行位已查过
    pub entry: PathBuf,
    pub facts: Rr3ManifestFacts,
    /// 物理绑定描述符文本（ADR-0018 裁定 4、批D④）：装配单 [hardware] 的序列化
    /// 文本，daemon 组装时从装配链透传（`--manifest` 直起路径为空串 = 无描述符）。
    /// spawn 时空值不注入——模块侧 rr-hal 维持"无描述符"fail-closed 状态。
    pub hal_bindings: String,
    /// 执行轴状态注入文本（批N3 件三，件五消歧扩形）：装配链轴档案
    /// （`hardware/<形态>/axis/*.toml`）中 instance 与轴读数（`RR3_HAL_PERIPH_AXIS`，
    /// 缺省契约常量）相等的恰一份合法装载的投影
    /// （`calibrated=<bool>,hard_stop=<词>,lo=<n>,hi=<n>,center=<n>`），daemon 同位
    /// 透传，spawn 时注入 `RR3_ACT_AXIS_STATE`；空串 = 不注入——模块侧折保守缺省
    /// （未标定不得自动，§6 硬约束），与 hal_bindings 同一条空值不注入纪律。
    pub axis_state: String,
    /// HAL 后端声明（P0-3 根因门）：装配单 [runtime].hal_backend，daemon 透传，
    /// spawn 时注入子进程 `RR3_HAL_BACKEND`。空串 = --manifest 调试路径不注入
    /// （模块侧走平台缺省，CI/本地可显式 env 覆盖）。消除"漏 env 静默落 sim"。
    pub hal_backend: String,
    /// 音频采集面后端覆盖（组合档 Pi×ESP32 用 esp32）：装配单 [runtime].audio_backend，
    /// daemon 透传，spawn 时注入子进程 `RR3_HAL_AUDIO_BACKEND`；None 不注入
    /// （音频面回落全局 hal_backend）。
    pub audio_backend: Option<String>,
    /// 舵机面后端覆盖（P0-4 组合档归属）：装配单 [runtime].servo_backend，daemon
    /// 透传，spawn 时注入子进程 `RR3_HAL_SERVO_BACKEND`；None 不注入（舵机面回落全局）。
    pub servo_backend: Option<String>,
    /// 显示面后端覆盖（P0-4 组合档归属）：装配单 [runtime].display_backend，daemon
    /// 透传，spawn 时注入子进程 `RR3_HAL_DISPLAY_BACKEND`；None 不注入（显示面回落全局）。
    pub display_backend: Option<String>,
}

/// supervisor 的两种请求。`Boot` 来自 daemon 的启动序（E-7），`Restart` 来自
/// conn 收尾与 watchdog 到期那两路交出的计划（`Rr3Death::Broadcast` / `Rr3Expiry.restart`）。
pub enum Rr3SuperMsg {
    Boot(Box<Rr3SpawnJob>),
    Restart(Rr3RestartPlan),
}

/// conn 线程与 watchdog 线程各持一份（`Sender` 可 clone）。发送失败只可能是
/// supervisor 已经死了 —— 那时打一行日志即可，两条调用方都不许 panic：
/// 守护进程不该因为起不了子进程就不服务已连上来的模块。
#[derive(Clone)]
pub struct Rr3Supervisor {
    tx: Sender<Rr3SuperMsg>,
}

impl Rr3Supervisor {
    pub fn rr3_new(tx: Sender<Rr3SuperMsg>) -> Self {
        Self { tx }
    }

    pub fn rr3_boot(&self, job: Rr3SpawnJob) {
        // 失败分支要点名是谁：`send` 吞掉的是整个消息，投递之后再读 `job` 已经晚了。
        // 一次 String clone 换"投递失败也报得出对象"，值得——这条路只在守护正在收尾时走。
        let name = job.facts.name.clone();
        if self.tx.send(Rr3SuperMsg::Boot(Box::new(job))).is_err() {
            rr3_log(&format!("supervisor 已退出，boot 投递丢弃: {name}"));
        }
    }

    pub fn rr3_restart(&self, plan: Rr3RestartPlan) {
        let name = plan.name.clone();
        if self.tx.send(Rr3SuperMsg::Restart(plan)).is_err() {
            rr3_log(&format!("supervisor 已退出，重启投递丢弃: {name}"));
        }
    }
}

/// boot 期的配置错误不吃预算（E-3）：entry 解析后不存在 / 不是文件 / 没有可执行位 →
/// 打一行 `拒绝启动 {name}：entry {绝对路径} 不存在或不可执行`，**不 own、不 spawn、不记账**。
/// 理由：那是确定性配置错误，吃预算会在 1 秒内烧光 5 次上限并把一个可修的笔误升级成
/// "运行时降级"，而 spec §5.4 的原话是"否则注册阶段直接拒"——语义是**拒**，不是**重试到放弃**。
///
/// 为什么返回 `Option` 而不是 `Result<_, String>`：**两种失败点名的对象不同**——
/// manifest 读不动/解析失败时 rrd 连 `name` 都不知道，只能点名"是哪个文件"（E-7 末句）；
/// entry 不合格时点名的是"哪个模块的哪条 entry"。把文案塞进返回值再让 daemon 打，
/// 就等于逼调用方去猜这两种形状，日志出口反而分散。
pub fn rr3_boot_job_parse(manifest_path: &Path) -> Option<Rr3SpawnJob> {
    let facts = rr3_job_facts(manifest_path)?;
    let entry = rr3_job_entry(manifest_path, &facts)?;
    Some(Rr3SpawnJob {
        manifest_path: manifest_path.to_path_buf(),
        entry,
        facts,
        // `--manifest` 直起路径没有装配单，描述符/执行轴/HAL 后端由 daemon 组装时统一透传
        hal_bindings: String::new(),
        axis_state: String::new(),
        hal_backend: String::new(),
        audio_backend: None,
        servo_backend: None,
        display_backend: None,
    })
}

/// manifest 读取 + 解析：两种失败点名对象不同（读不动点名文件，解析失败附逐条原因）。
fn rr3_job_facts(manifest_path: &Path) -> Option<Rr3ManifestFacts> {
    let text = rr3_job_text(manifest_path)?;
    rr3_parse_facts(&text, manifest_path)
}

/// manifest 文本读取：读不动点名文件并回 None。
fn rr3_job_text(manifest_path: &Path) -> Option<String> {
    match std::fs::read_to_string(manifest_path) {
        Ok(t) => Some(t),
        Err(e) => {
            rr3_log(&format!(
                "拒绝启动：manifest {} 读取失败: {e}",
                manifest_path.display()
            ));
            None
        }
    }
}

fn rr3_parse_facts(text: &str, manifest_path: &Path) -> Option<Rr3ManifestFacts> {
    match rr3_manifest_parse(text) {
        Ok(f) => Some(f),
        Err(errs) => {
            rr3_log(&format!(
                "拒绝启动：manifest {} 解析失败: {}",
                manifest_path.display(),
                errs.join("; ")
            ));
            None
        }
    }
}

/// entry 定位 + E-3 两项检查（存在 / 可执行位）：不合格点名模块与 entry。
fn rr3_job_entry(manifest_path: &Path, facts: &Rr3ManifestFacts) -> Option<PathBuf> {
    let entry = rr3_entry_path(manifest_path, facts);
    // E-3 的两项检查。`PermissionsExt::mode()` 在 `std::os::unix::fs` 里，仍是零外部依赖。
    if rr3_entry_runnable(&entry) {
        Some(entry)
    } else {
        rr3_log(&format!(
            "拒绝启动 {}：entry {} 不存在或不可执行",
            facts.name,
            entry.display()
        ));
        None
    }
}

/// E-7：绝对则原样，否则相对 **manifest 所在目录** join（不是 rrd 的 CWD——装机后模块的
/// 相对 entry 是相对它自己的 manifest 写的，跟着 CWD 走就成了一台机器一个目录树）。
/// 父目录取不到（`--manifest` 给的是裸文件名）时按空串 join，即由 rrd 的 CWD 展开——
/// 那是 CLI 原词本身有歧义，而回显出去的永远是最终解析后的那一条路径，不猜。
fn rr3_entry_path(manifest_path: &Path, facts: &Rr3ManifestFacts) -> PathBuf {
    if rr3_manifest_abs(&facts.entry) {
        PathBuf::from(&facts.entry)
    } else {
        manifest_path
            .parent()
            .unwrap_or(Path::new(""))
            .join(&facts.entry)
    }
}

/// E-3：entry 存在、是文件且可执行。
fn rr3_entry_runnable(entry: &Path) -> bool {
    let Ok(md) = std::fs::metadata(entry) else {
        return false;
    };
    md.is_file() && md.permissions().mode() & 0o111 != 0
}
