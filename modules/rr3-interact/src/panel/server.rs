//! 面板 HTTP 服务（0.0.4 批C，D3/Q3 裁定落地）：std 手写 HTTP/1.1 子集，
//! **零新 crates.io 依赖**——GET/POST + Content-Length + JSON body 就是全部，
//! 不做 keep-alive、不做 chunked、不做 TLS（局域网可信面声明落 README，
//! 不承诺公网暴露）。
//!
//! 架构边界（ADR-0006 裁定 3 的面板条款）：
//! - **面板不当发布者**——硬件控制只走 `rr3.call` RPC 归属门。这不是口号而是
//!   类型面：[`Rr3PanelDispatch`] 里根本没有 Publish 变体，路由器想越权也造不出
//!   发布动作（"越权拒"的静态形态）。
//! - **ctl 查询走短连**——`rr3.ctl.*` 只对未注册连接开放（ADR-0010），面板在
//!   本进程内另开一条裸 UDS 一问一答即断，与外部运维探针同权同拒。
//! - **beep/face 不出进程**——播放面与表情机归主循环所有，面板经动作通道
//!   （mpsc）请求主循环代执行，串行无锁，与 TTS 点名天然互斥。
//!
//! 端点白名单见 [`RR3_PANEL_ROUTES`]（legacy 四页签语义搬入，"面板：状态与版本/
//! 硬件自检/实时交互流/运行数据/重启"逐项有承接；WiFi 配网页随 0.0.4 批D 挂上）：
//! `GET /`（静态页）、`GET /api/status`（health+link 缓存）、`GET /api/data`
//! （counters）、`GET /api/logs`（rrd 日志环）、`POST /api/relay`（act RPC）、
//! `POST /api/beep`（进程内）、`POST /api/face`（进程内）、`POST /api/selftest`
//! （并列三调，各答各展示）、`POST /api/reboot`（ctl.reboot，双重门缺省拒）、
//! `GET /api/wifi/scan` + `POST /api/wifi/connect` + `POST /api/wifi/forget`
//! （0.0.4 批D：BusCall 透传 connect 三 RPC，参数裁决在 owner 模块）＋模型密钥面
//! 与 brain ops 读口（0.0.6 抽象批/B-12）＋五模块 ops 读口（批N4 件九二，同一
//! BusCall 透传范式）。端点全集与 covers/applies 申报以 [`RR3_PANEL_ROUTES`] 为真值。

use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rr3mod::BusHandle;
use rr_bus_api::Rr3BusClient;
use rr_ipc::{
    opcode, rr3_body_encode, rr3_frame_read_cap, rr3_frame_write, Rr3Frame, Rr3Header,
    Rr3PayloadType, RR3_BUS_PATH_ENV, RR3_FRAMING_PLEN_CAP_DEFAULT, RR3_UDS_PATH_DEFAULT,
};
use serde_json::{json, Value};

use super::exec::{rr3_panel_execute, Rr3PanelReply};
use super::http::{rr3_http_patience, rr3_http_read_req, rr3_http_respond, rr3_panel_body_json};

/// 面板监听面：env `RR3_PANEL_ADDR` 优先，缺省只绑回环（Q3 裁定）。
pub const RR3_PANEL_ADDR_ENV: &str = "RR3_PANEL_ADDR";
pub const RR3_PANEL_ADDR_DEFAULT: &str = "127.0.0.1:8080";

/// 面板五限额/超时（批N4 件五 I-18② 数据化）：装配期自 config
/// `[rr3-interact.panel]` 读出构造（缺键回落 [`Default`] 的 legacy 值），
/// 经 [`Rr3PanelShared`] 贯穿 serve/accept/handle——"现场限并发只能改码"
/// 的无主策略就此清账。值语义不变：超限 503、半包折 Err、超界断连。
#[derive(Debug, Clone, Copy)]
pub struct Rr3PanelLimits {
    pub max_conns: usize,
    pub read_timeout: Duration,
    pub header_limit: usize,
    pub body_limit: usize,
    pub call_timeout: Duration,
}

impl Default for Rr3PanelLimits {
    fn default() -> Self {
        // legacy 常量值——一个事实一个真值点：默认值只在此处与 config.toml
        // 注释对表（composite.rs 的 Rr3PanelCfg::default 是 config 缺键回落，
        // 与此处同值同源叙述）
        Self {
            max_conns: 8,
            read_timeout: Duration::from_secs(5),
            header_limit: 8 * 1024,
            body_limit: 4 * 1024,
            call_timeout: Duration::from_secs(5),
        }
    }
}

/// 连接线程 spawn 失败的运维计数键（批M M-β I-4）：失败点账后续连，
/// `rr3.ctl.counters` 读面键 `rr3-interact/panel_spawn_fail`。
const RR3_PANEL_SPAWN_FAIL_KEY: &str = "panel_spawn_fail";

/// 并发槽位 RAII 守卫（批M M-β I-4）：acquire 即 +1、Drop 即 -1——连接线程
/// 正常返回与 panic 展开都归还槽位；spawn 失败时闭包（连同守卫）被丢弃，
/// 走同一 Drop 路径归还。旧实现 fetch_add 在 spawn 前、归还只在闭包尾手动
/// fetch_sub，panic/失败双路泄漏。
struct Rr3ConnSlot {
    live: Arc<AtomicUsize>,
}

impl Rr3ConnSlot {
    fn rr3_acquire(live: &Arc<AtomicUsize>) -> Self {
        live.fetch_add(1, Ordering::Relaxed);
        Self { live: live.clone() }
    }
}

impl Drop for Rr3ConnSlot {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

/// covers 申报的集合词表（I-19，批N4 件三）：covers 元素形制 = `<集合>/<字段>`，
/// 集合只认本表五路——与门 d 分母五路一一对应（`ops[].slot` ∪ `pin_id.field` ∪
/// `roles.requires` 每项 ∪ `domains` 字段 ∪ 轴档案字段）。check.sh 步 ⑩ 自本常量
/// 导出词表做 covers 形制校验（单真值点，不抄第二份）；词表外的集合 = 申报无效，
/// fail-closed 当场红。
// 门禁消费：check.sh 步 ⑩ awk 自本常量导出 covers 词表，Rust 侧无调用点。
#[allow(dead_code)]
pub const RR3_PANEL_COVER_SETS: [&str; 5] = ["ops", "pin", "role", "domain", "axis"];

/// applies 生效路径词表（I-20，批N4 件四，全案 §19.3 判据 1）：面板写口动了
/// 可配置集时必填的三态申报——`live` 即时生效｜`reboot` 重启进程生效｜
/// `reflash` 重烧固件生效。申报住 [`RR3_PANEL_ROUTES`] 第四列（单真值点），
/// check.sh 步 ⑩ 自本常量导出词表校验（词表外红、写口带 covers 申报而缺
/// applies 红）；UI 面对操作者显式告知（写口应答带 `applies` 字段，前端文案
/// 同义转述——密钥面即"brain 下一轮对话生效"）。
// 门禁消费：check.sh 步 ⑩ awk 自本常量导出 applies 词表，Rust 侧无调用点。
#[allow(dead_code)]
pub const RR3_PANEL_APPLY_MODES: [&str; 3] = ["live", "reboot", "reflash"];

/// 端点总表（I-9/I-12 唯一真值，0.0.5 批M MC-8③；covers 申报面随 I-19 批N4 件三
/// 升三元组，applies 申报面随 I-20 批N4 件四升四元组）：`(方法, 路径, covers,
/// applies)`——第三列是**数据化申报**：本端点真实渲染/写入的可配置集元素（元素
/// 形制见 [`RR3_PANEL_COVER_SETS`]），第四列是写口的**生效路径**（词表见
/// [`RR3_PANEL_APPLY_MODES`]，空串 = 无可配置量写入，读口/动作口皆空）。与件
/// `handles`/`ops` 同一申报范式：**未申报就不计入门 d 分子**（不是"默认全算"，
/// 端点数 ≠ 面板项数）。自述文案与 serve 行的端点数由本表长度推导（文案不手写
/// 数字），match 分派与本表的漂移由 `route_table_matches_dispatch` 单测钉死，
/// covers 词表形制由 `covers_use_declared_vocabulary_only` 单测钉死，applies 词表
/// 由 `applies_use_declared_vocabulary_only` 单测钉死——新增端点 = 加表 + 过三门，
/// 不再改第三处文案。
///
/// 今日实况（ADR-0023 模型面后）：28 端点（26 + 2026-10-31 新增存活/版本两探针
/// `/api/healthz`、`/api/version`——不触总线、可高频轮询，供监控与客户端判活），
/// 读配置者 7 = `GET /api/brain/ops`
/// （brain ops 六槽）＋ `GET /api/brain/models`（模型行快照）＋五模块 ops 读口五
/// 端点（`GET /api/{act,connect,interact,power,sense}/ops`，BusCall 透传各模块
/// `rr3-<mod>/ops/list`，白名单组体应答）；写口 3 =
/// `POST /api/brain/ops`（BusCall 透传 `rr3-brain/ops/set`，组.槽寻址、owner 白名单
/// fail-closed）＋ `POST /api/brain/model`（ADR-0023 模型行 CRUD，落盘 + live 生效）＋
/// `POST /api/profile/domain`（批N22 件一，ctl.domain.set 透传——
/// 装配侧写口，confirm/证据/key 白名单门序全在 rrd domain_write.rs 单点裁决，面板
/// 只透传 body；covers 申报四键 = conventions §6.1 三写口点名集，applies=reboot
/// 如实申报"下次 rrd boot 生效"）——ops 路分母 36 槽全部转绿（covers 申报 30 项随
/// 件九二上表，写口申报同集合 ∪ 不增）。接线图读口 `GET /api/wiring`（批N4 件九笔4）：ctl.wiring
/// 透传（判据真值单点 = rrd assembly.rs 的装配快照，面板只渲染不判——第二真值点
/// 禁令），covers 申报 44 项 = pin 11 + role 7 + domain 12 + axis 14（门 d 分母
/// 四路字段级全集，与 rrd 渲染完整性测试同源钉死；domain 12 = 批N21 件二 evidence
/// 键入 RR3_DOMAIN_KEYS 后件四同批补申报——assembly 快照批N21 件二起渲染 evidence，
/// 读面可读即申报，写面半边随批N22 件一 `/api/profile/domain` 收口）⇒ 红账 43 全转绿，基线
/// tools/ci/gate-d-baseline.txt 行 1 同批 43→0；密钥面两端点（keys/key）不申报
/// covers——密钥不在五路分母（secrets 不住 assemble.toml 槽表），如实空。
/// applies 今日四条申报 = `POST /api/model/key`（secrets 凭据写入，brain 现读
/// ⇒ live）＋ `POST /api/brain/ops`（写臂重建受影响客户端，live 内存生效**不落盘**，
/// 应答 `persisted:false` 如实申报——重启回 config.toml 真值；Python 无 TOML
/// writer、Y-2 单写路径协商点归后续批）＋ `POST /api/brain/model`（ADR-0023：
/// config.toml 原子落盘 + live 换客户端，`persisted:true` 如实申报）＋
/// `POST /api/profile/domain`（reboot——
/// 装配单落盘，下次 rrd 进程装载投影，运行态不动；三态词表取最近义的 reboot）；
/// 其余写口（beep/face/selftest/reboot/
/// relay/wifi）写的是动作与运行态，不动可配置集，如实空。**表项一律单行书写**
/// （硬约束）：check.sh 步 ⑩ 的 applies 解析器逐行按引号段切本表——跨行表项/
/// 行内注释都会被拆行判坏（批N4 件六二首跑即红的实证）。
#[rustfmt::skip]
pub const RR3_PANEL_ROUTES: &[(&str, &str, &[&str], &str)] = &[
    ("GET", "/", &[], ""),
    ("GET", "/api/status", &[], ""),
    ("GET", "/api/healthz", &[], ""),
    ("GET", "/api/version", &[], ""),
    ("GET", "/api/data", &[], ""),
    ("GET", "/api/logs", &[], ""),
    ("POST", "/api/relay", &[], ""),
    ("POST", "/api/beep", &[], ""),
    ("POST", "/api/face", &[], ""),
    ("POST", "/api/selftest", &[], ""),
    ("POST", "/api/reboot", &[], ""),
    ("GET", "/api/wifi/scan", &[], ""),
    ("POST", "/api/wifi/connect", &[], ""),
    ("POST", "/api/wifi/forget", &[], ""),
    ("GET", "/api/model/keys", &[], ""),
    ("POST", "/api/model/key", &[], "live"),
    ("GET", "/api/brain/ops", &["ops/asr_model", "ops/base_url", "ops/llm_model", "ops/max_utterance_ms", "ops/timeout_s", "ops/tts_model"], ""),
    ("POST", "/api/brain/ops", &["ops/asr_model", "ops/base_url", "ops/llm_model", "ops/max_utterance_ms", "ops/timeout_s", "ops/tts_model"], "live"),
    ("GET", "/api/brain/models", &[], ""),
    ("POST", "/api/brain/model", &[], "live"),
    ("POST", "/api/brain/model/test", &[], ""),
    ("GET", "/api/act/ops", &["ops/blink_period_ms", "ops/led_effects", "ops/move_timeout_ms", "ops/state_min_interval_ms"], ""),
    ("GET", "/api/connect/ops", &["ops/up", "ops/link_report_period_ms", "ops/timeout_ms", "ops/min_pass_len"], ""),
    ("GET", "/api/interact/ops", &["ops/non_pointable", "ops/default_expression", "ops/face_tick_ms", "ops/selftest_face_expect", "ops/panel_max_conns", "ops/panel_read_timeout_ms", "ops/panel_header_limit", "ops/panel_body_limit", "ops/panel_call_timeout_ms", "ops/prebuffer_ms", "ops/volume_pct"], ""),
    ("GET", "/api/power/ops", &["ops/sample_period_ms", "ops/low_voltage_mv", "ops/cutoff_voltage_mv", "ops/state_period_ms"], ""),
    ("GET", "/api/sense/ops", &["ops/sample_rate", "ops/channels", "ops/frame_ms", "ops/doa_bin_threshold_deg", "ops/doa_window_ms", "ops/doa_window_rms_min", "ops/doa_mic_baseline_mm", "ops/rms_start", "ops/silence_tail_ms", "ops/min_speech_ms"], ""),
    ("GET", "/api/wiring", &["pin/role", "pin/instance", "pin/controller", "pin/channel", "pin/physical", "pin/direction", "pin/rail", "pin/addr", "pin/device", "pin/failsafe", "pin/mux_group", "role/controller", "role/channel", "role/physical", "role/rail", "role/failsafe", "role/addr", "role/device", "domain/name", "domain/nominal_mv", "domain/mild_mv", "domain/severe_mv", "domain/current_cap_ma", "domain/sense", "domain/current_sense", "domain/switch", "domain/switchable", "domain/armed", "domain/max_age_ms", "domain/evidence", "axis/schema", "axis/pin", "axis/center_deg", "axis/lo_deg", "axis/hi_deg", "axis/dir_sign", "axis/slew_deg_s", "axis/settle_ms", "axis/hold", "axis/current_ceil_ma", "axis/warmup_ms", "axis/hard_stop", "axis/calibrated", "axis/source"], ""),
    ("POST", "/api/profile/domain", &["domain/armed", "domain/evidence", "domain/severe_mv", "axis/current_ceil_ma"], "reboot"),
];

/// 启动自述（端点数由 [`RR3_PANEL_ROUTES`] 长度推导——I-9/I-12 同一真值）。
pub fn rr3_panel_surface() -> Vec<String> {
    vec![format!(
        "panel: http {}端点（缺省 127.0.0.1:8080，RR3_PANEL_ADDR 可改）",
        RR3_PANEL_ROUTES.len()
    )]
}

/// 主循环代执行的面板动作（beep/face），HTTP 线程经 mpsc 递给主循环。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rr3PanelAction {
    /// 喇叭测试音（批N5 件五笔2 起内置测试音）：主循环合成短正弦进程内直喂
    /// 播放面，回 `{played_ms}`——selftest 的 speaker 腿由此转 pass（波形源
    /// 进程内自持，不再依赖 tts_pcm 流）
    Beep,
    /// 表情切换：进程内直调表情机（名字合法性由 Rr3Face 白名单裁决）
    Face { expression: String },
}

/// 路由器的产物。**没有 Publish 变体**——"面板不当发布者"由类型面钉死，
/// 越权发布在路由器里就无从构造（ADR-0006 裁定 3）。
#[derive(Debug, Clone, PartialEq)]
pub enum Rr3PanelDispatch {
    /// `GET /`：单文件内嵌静态页
    Html,
    /// `GET /api/status`：ctl.health + net/link 订阅缓存
    StatusQuery,
    /// `GET /api/healthz`：进程存活探针（不触总线，零依赖）
    HealthQuery,
    /// `GET /api/version`：模块版本与端点计数（面板自描述）
    VersionQuery,
    /// `GET /api/data`：ctl.counters（运维计数账）
    CountersQuery,
    /// `GET /api/logs`：ctl.logs（rrd 日志环尾部）
    LogsQuery,
    /// `GET /api/wiring`：ctl.wiring（装配快照读口——四路底账 + [pins]×wiring
    /// 全交叉逐条判果，批N4 件九笔4；判据真值单点 = rrd assembly.rs）
    WiringQuery,
    /// `POST /api/profile/domain`：ctl.domain.set（装配侧写口，批N22 件一）——
    /// body 原样透传，confirm/证据四道核/key 白名单门序全在 rrd
    /// domain_write.rs 单点裁决（面板零业务语义，wiring 读口同一条第二真值点
    /// 禁令的写半边）；effective:boot 语义由 rrd 应答如实申报
    ProfileDomainSet {
        params: Value,
    },
    /// 硬件控制：`rr3.call` 经 rrd 归属门转发（relay → `rr3-act/relay/set`）
    BusCall {
        topic: String,
        args: Value,
    },
    /// 进程内动作：主循环代执行（beep/face）
    PanelAct(Rr3PanelAction),
    /// `POST /api/selftest`：并列三调（beep + relay + face），各答各展示，
    /// 单项失败不连锁、无依赖链无状态机——**不算编排**（brain 唯一编排者，
    /// ADR-0007 边界注记）
    SelfTest,
    /// `POST /api/reboot`：ctl.reboot（rrd 侧双重门，缺省恒拒）
    Reboot,
    /// `GET /api/model/keys`：secrets 已存 ref 清单（只报名字，永不回值——
    /// 0.0.6 模型连接抽象批，面板密钥面）
    ModelKeysQuery,
    /// `POST /api/model/key`：secrets upsert（value 空=删）；落仓外 0600 文件，
    /// 应答只回 ref 不回值
    ModelKeySet {
        ref_name: String,
        value: String,
    },
    NotFound,
    MethodNotAllowed,
    BadRequest(String),
}

/// 解析出的一个 HTTP 请求（HTTP/1.1 子集：方法/路径/body）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rr3HttpRequest {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

/// 五模块 ops 读面（批N4 件九二）：同一 BusCall 透传范式——参数裁决在 owner
/// （各模块白名单组体快照，槽名 = assemble/composite.toml 槽表申报同源），
/// 面板只透传。表驱动：路径 → 模块名，新增模块只加一行。
const RR3_OPS_ROUTES: [(&str, &str); 5] = [
    ("/api/act/ops", "rr3-act"),
    ("/api/connect/ops", "rr3-connect"),
    ("/api/interact/ops", "rr3-interact"),
    ("/api/power/ops", "rr3-power"),
    ("/api/sense/ops", "rr3-sense"),
];

/// ops 读面路径 → 名单点名 topic（非 ops 路径返回 None）
fn rr3_ops_topic(path: &str) -> Option<String> {
    RR3_OPS_ROUTES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, m)| format!("{m}/ops/list"))
}

/// 无参读口 BusCall（空对象参数）
fn rr3_get_call(topic: &str) -> Rr3PanelDispatch {
    Rr3PanelDispatch::BusCall {
        topic: topic.to_string(),
        args: json!({}),
    }
}

/// 带 body 写口 BusCall：body 非 JSON 即 400，参数裁决归 owner
fn rr3_post_call(topic: &str, body: &[u8]) -> Rr3PanelDispatch {
    match rr3_panel_body_json(body) {
        Ok(args) => Rr3PanelDispatch::BusCall {
            topic: topic.to_string(),
            args,
        },
        Err(e) => Rr3PanelDispatch::BadRequest(e),
    }
}

/// 路由纯函数：方法 + 路径 + body → 分派。**端点白名单 = [`RR3_PANEL_ROUTES`]**，
/// 未知路径 404、方法不对 405、body 坏 400——面板不是攻击面兜底者，一切都显式。
///
/// 结构约束（B8 分面）：按资源归属分 sys/mod/cfg/act 四组匹配，每条路径只在
/// 一组出现（组内仍一端点一行、无第二层 match）；方法收窄走
/// [`rr3_get_only`]/[`rr3_post_only`]，body 解析走 `rr3_*_set` 小函数。
/// 分面只切判读面，不复制申报列——covers/applies 真值仍单点在端点表。
pub(super) fn rr3_panel_route(method: &str, path: &str, body: &[u8]) -> Rr3PanelDispatch {
    if let Some(d) = rr3_route_sys(method, path) {
        return d;
    }
    if let Some(d) = rr3_route_mod(method, path, body) {
        return d;
    }
    rr3_route_writes(method, path, body)
}

/// 写口两组（cfg/act）：都未命中即 404。
fn rr3_route_writes(method: &str, path: &str, body: &[u8]) -> Rr3PanelDispatch {
    if let Some(d) = rr3_route_cfg(method, path, body) {
        return d;
    }
    if let Some(d) = rr3_route_act(method, path, body) {
        return d;
    }
    Rr3PanelDispatch::NotFound
}

/// 系统读面：静态页、状态/存活/版本/计数/日志、装配快照。
/// 四条路由组（sys/mod/cfg/act）按资源归属划面，每条路径**只在一组**出现；
/// 端点真值仍是 [`RR3_PANEL_ROUTES`]（本组表不复制申报列），漂移由
/// `route_table_matches_dispatch` 逐表项钉死。
fn rr3_route_sys(method: &str, path: &str) -> Option<Rr3PanelDispatch> {
    let d = match path {
        "/" => Rr3PanelDispatch::Html,
        "/api/status" => Rr3PanelDispatch::StatusQuery,
        // 存活/版本探针（2026-10-31）：不触总线、零依赖，可高频轮询
        "/api/healthz" => Rr3PanelDispatch::HealthQuery,
        "/api/version" => Rr3PanelDispatch::VersionQuery,
        _ => return rr3_route_sys_tail(method, path),
    };
    Some(rr3_get_only(method, d))
}

/// 系统读面续组：计数/日志/接线图（未知路径仍 404）。
fn rr3_route_sys_tail(method: &str, path: &str) -> Option<Rr3PanelDispatch> {
    let d = match path {
        "/api/data" => Rr3PanelDispatch::CountersQuery,
        "/api/logs" => Rr3PanelDispatch::LogsQuery,
        // 接线图读口：ctl.wiring 透传，判据真值单点在 rrd assembly.rs
        "/api/wiring" => Rr3PanelDispatch::WiringQuery,
        _ => return None,
    };
    Some(rr3_get_only(method, d))
}

/// 模块面：五模块 ops 读口（表驱动 [`RR3_OPS_ROUTES`]）＋ brain ops/模型行
/// ＋ 模型密钥面（只报 ref 名，POST 应答不回值）。
fn rr3_route_mod(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    if let Some(topic) = rr3_ops_topic(path) {
        return Some(rr3_get_only(method, rr3_get_call(&topic)));
    }
    rr3_route_brain(method, path, body)
}

/// brain 读/写面（ops 同源、模型行、密钥面在 [`rr3_route_keys`]）。
fn rr3_route_brain(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    match path {
        // brain ops 读/写同源（读渲染与写同一批 ops 槽）
        "/api/brain/ops" => Some(rr3_brain_ops(method, body)),
        "/api/brain/models" => Some(rr3_get_only(method, rr3_get_call("rr3-brain/model/list"))),
        "/api/brain/model" => Some(rr3_post_only(
            method,
            rr3_post_call("rr3-brain/model/set", body),
        )),
        "/api/brain/model/test" => Some(rr3_post_only(
            method,
            rr3_post_call("rr3-brain/model/test", body),
        )),
        _ => rr3_route_keys(method, path, body),
    }
}

/// 模型密钥面（只报 ref 名，POST 应答不回值）。
fn rr3_route_keys(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    Some(match path {
        "/api/model/keys" => rr3_get_only(method, Rr3PanelDispatch::ModelKeysQuery),
        "/api/model/key" => rr3_post_only(method, rr3_key_set(body)),
        _ => return None,
    })
}

/// 配置面：装配侧写口（rrd 门序单点裁决）＋ 配网三 RPC（参数裁决在 owner 模块）。
fn rr3_route_cfg(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    Some(match path {
        // 装配侧写口：body 原样透传 rrd 写口门序（confirm/证据/key 白名单）
        "/api/profile/domain" => rr3_post_only(method, rr3_domain_set(body)),
        "/api/wifi/scan" => rr3_get_only(method, rr3_get_call("rr3-connect/wifi/scan")),
        "/api/wifi/connect" => {
            rr3_post_only(method, rr3_post_call("rr3-connect/wifi/connect", body))
        }
        // forget 无参数：空对象 args（legacy 语义，body 不读）
        "/api/wifi/forget" => rr3_post_only(method, rr3_get_call("rr3-connect/wifi/forget")),
        _ => return None,
    })
}

/// 动作面：继电器（归属门在 rrd）、进程内 beep/face、自检、重启（双重门缺省拒）。
fn rr3_route_act(method: &str, path: &str, body: &[u8]) -> Option<Rr3PanelDispatch> {
    Some(match path {
        // relay：面板只裁决布尔 on，归属门在 rrd（topic 属 act exposes）
        "/api/relay" => rr3_post_only(method, rr3_relay_set(body)),
        "/api/beep" => rr3_post_only(method, Rr3PanelDispatch::PanelAct(Rr3PanelAction::Beep)),
        "/api/face" => rr3_post_only(method, rr3_face_set(body)),
        "/api/selftest" => rr3_post_only(method, Rr3PanelDispatch::SelfTest),
        "/api/reboot" => rr3_post_only(method, Rr3PanelDispatch::Reboot),
        _ => return None,
    })
}

/// GET 限定：表内读口统一收窄（非 GET = 405）
fn rr3_get_only(method: &str, d: Rr3PanelDispatch) -> Rr3PanelDispatch {
    if method == "GET" {
        d
    } else {
        Rr3PanelDispatch::MethodNotAllowed
    }
}

/// POST 限定：动作/写口统一收窄（非 POST = 405）
fn rr3_post_only(method: &str, d: Rr3PanelDispatch) -> Rr3PanelDispatch {
    if method == "POST" {
        d
    } else {
        Rr3PanelDispatch::MethodNotAllowed
    }
}

/// brain ops 两面同端点：GET 读快照 / POST 写槽（参数裁决在 owner）
fn rr3_brain_ops(method: &str, body: &[u8]) -> Rr3PanelDispatch {
    match method {
        "GET" => rr3_get_call("rr3-brain/ops/list"),
        "POST" => rr3_post_call("rr3-brain/ops/set", body),
        _ => Rr3PanelDispatch::MethodNotAllowed,
    }
}

/// `POST /api/profile/domain`：body 原样透传（参数裁决在 rrd 写口门序）
fn rr3_domain_set(body: &[u8]) -> Rr3PanelDispatch {
    match rr3_panel_body_json(body) {
        Ok(params) => Rr3PanelDispatch::ProfileDomainSet { params },
        Err(e) => Rr3PanelDispatch::BadRequest(e),
    }
}

/// `POST /api/relay`：只认布尔 on
fn rr3_relay_set(body: &[u8]) -> Rr3PanelDispatch {
    let args = match rr3_panel_body_json(body) {
        Ok(v) => v,
        Err(e) => return Rr3PanelDispatch::BadRequest(e),
    };
    rr3_relay_call(&args)
}

/// relay 参数落派发（只认布尔 on）。
fn rr3_relay_call(args: &Value) -> Rr3PanelDispatch {
    match args.get("on").and_then(Value::as_bool) {
        Some(on) => Rr3PanelDispatch::BusCall {
            topic: "rr3-act/relay/set".to_string(),
            args: json!({ "on": on }),
        },
        None => Rr3PanelDispatch::BadRequest("缺布尔参数 on".to_string()),
    }
}

/// `POST /api/face`：只认字符串 expression
fn rr3_face_set(body: &[u8]) -> Rr3PanelDispatch {
    let args = match rr3_panel_body_json(body) {
        Ok(v) => v,
        Err(e) => return Rr3PanelDispatch::BadRequest(e),
    };
    rr3_face_call(&args)
}

/// face 参数落派发（只认字符串 expression）。
fn rr3_face_call(args: &Value) -> Rr3PanelDispatch {
    match args.get("expression").and_then(Value::as_str) {
        Some(expression) => Rr3PanelDispatch::PanelAct(Rr3PanelAction::Face {
            expression: expression.to_string(),
        }),
        None => Rr3PanelDispatch::BadRequest("缺字符串参数 expression".to_string()),
    }
}

/// `POST /api/model/key`：只认字符串 ref/value
fn rr3_key_set(body: &[u8]) -> Rr3PanelDispatch {
    let args = match rr3_panel_body_json(body) {
        Ok(v) => v,
        Err(e) => return Rr3PanelDispatch::BadRequest(e),
    };
    rr3_key_call(&args)
}

/// key 参数落派发（只认字符串 ref/value）。
fn rr3_key_call(args: &Value) -> Rr3PanelDispatch {
    match (
        args.get("ref").and_then(Value::as_str),
        args.get("value").and_then(Value::as_str),
    ) {
        (Some(ref_name), Some(value)) => Rr3PanelDispatch::ModelKeySet {
            ref_name: ref_name.to_string(),
            value: value.to_string(),
        },
        _ => Rr3PanelDispatch::BadRequest("缺字符串参数 ref/value".to_string()),
    }
}

/// 帧头时间戳：与本仓其余进程内副本同式（rr-bus client / rrd conn 各持一份私有实现）
fn rr3_now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

mod ctl;
mod selftest;
mod serve;
#[cfg(test)]
mod tests;

pub use ctl::{rr3_ctl_query, rr3_ctl_send};
pub use selftest::{
    Rr3SelftestExpect, RR3_SELFTEST_CONTRACTS_EXPECTS, RR3_SELFTEST_LEG_STATES,
    RR3_SELFTEST_WIRING_EXPECTS,
};
#[cfg(test)]
use serve::{rr3_accept_loop, rr3_panel_handle, rr3_panel_loopback};
pub use serve::{rr3_panel_addr, rr3_panel_serve, spawn_host, Rr3PanelShared};
