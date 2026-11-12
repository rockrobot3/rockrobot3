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
/// 板上 `0.0.0.0` 随装配单注入，真机验证后置（不 claim）。
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

/// ctl 短连一问一答（ADR-0010 的面板侧执行面）：裸 UDS 连 rrd，发一条
/// `rr3.ctl.*` 请求（src=0，与外部运维探针同权），收一条应答即断。
/// 错误信封折 `Err`（HTTP 层转 502 语义的 JSON），成功回 result 对象。
/// 读耐心 = [`Rr3PanelLimits::read_timeout`]（I-18②：调用方自 shared 取）。
pub fn rr3_ctl_query(
    bus_path: &str,
    method: &str,
    id: u64,
    read_timeout: Duration,
) -> Result<Value, String> {
    rr3_ctl_send(bus_path, method, &json!({}), id, read_timeout)
}

/// ctl 短连底层（批N22 件一自 [`rr3_ctl_query`] 拆分）：带 params 透传面——
/// 装配侧写口 body 原样转发（参数裁决在 rrd 写口门序单点，第二真值点禁令）；
/// [`rr3_ctl_query`] 是它的空参数特例（八处既有调用点零行为漂移）。
pub fn rr3_ctl_send(
    bus_path: &str,
    method: &str,
    params: &Value,
    id: u64,
    read_timeout: Duration,
) -> Result<Value, String> {
    let mut stream = rr3_ctl_conn(bus_path, read_timeout)?;
    rr3_ctl_roundtrip(&mut stream, method, params, id)
}

/// 连总线短连并设读耐心。
fn rr3_ctl_conn(
    bus_path: &str,
    read_timeout: Duration,
) -> Result<std::os::unix::net::UnixStream, String> {
    let stream = std::os::unix::net::UnixStream::connect(bus_path)
        .map_err(|e| format!("连总线失败: {e}"))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|e| format!("设读超时失败: {e}"))?;
    Ok(stream)
}

/// 一问一答：发 Request 帧、收一条应答帧。
fn rr3_ctl_roundtrip(
    stream: &mut std::os::unix::net::UnixStream,
    method: &str,
    params: &Value,
    id: u64,
) -> Result<Value, String> {
    let frame = rr3_ctl_frame(method, params, id);
    rr3_frame_write(&mut *stream, &frame).map_err(|e| format!("写 ctl 请求失败: {e:?}"))?;
    let frame = rr3_frame_read_cap(&mut *stream, RR3_FRAMING_PLEN_CAP_DEFAULT)
        .map_err(|e| format!("读 ctl 应答失败: {e:?}"))?;
    rr3_ctl_reply(frame, method)
}

/// ctl 请求帧装配（jsonrpc 信封 + 帧头）。
fn rr3_ctl_frame(method: &str, params: &Value, id: u64) -> Rr3Frame {
    let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    let payload = rr3_body_encode(Rr3PayloadType::Request, body.to_string().as_bytes());
    Rr3Frame {
        header: Rr3Header {
            opcode: opcode::SEND,
            flags: 0,
            trace_id: 0,
            timestamp_ns: rr3_now_ns(),
            src_task: 0, // ctl 连接没有 task，与外部运维探针同一写法
            dst_task: rr_core::RR3_TASK_RRD,
            payload_len: payload.len() as u32,
            payload_crc32: rr_ipc::rr3_crc32(&payload),
        },
        payload,
    }
}

/// 应答帧解码：类型必须是 Response。
fn rr3_ctl_reply(frame: Rr3Frame, method: &str) -> Result<Value, String> {
    let (ty, payload) = Rr3BusClient::rr3_frame_body(&frame)?;
    if ty != Rr3PayloadType::Response {
        return Err(format!("ctl 应答类型错误: {ty:?}"));
    }
    rr3_ctl_result(payload, method)
}

/// 应答体折 result：错误信封折 Err，缺 result 也回错。
fn rr3_ctl_result(payload: Value, method: &str) -> Result<Value, String> {
    if let Some(err) = payload.get("error") {
        return Err(format!(
            "ctl {method} 被拒: {}",
            err.get("message").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    payload
        .get("result")
        .cloned()
        .ok_or_else(|| format!("ctl {method} 应答缺 result"))
}

/// 自检预期表值域闭集（批N5 件五 F-31）：固定腿期望三态词表——parse 面对账
/// 在册（词表外拒 boot），verdict 等值判定的合法判值集。
pub const RR3_SELFTEST_LEG_STATES: [&str; 3] = ["pass", "fail", "n-a"];

/// wiring 组预期词表（批N5 件五 F-31）：组内零 fail 即组过（n-a 不拖累不救场）。
/// 单值词表也是闭集——拼错的预期值（如 "zero_fail"）当场拒 boot，不容静默。
pub const RR3_SELFTEST_WIRING_EXPECTS: [&str; 1] = ["zero-fail"];

/// contracts 组预期词表（批N5 件五笔4，Z-4）：契约四册"齐全且 sha 对"的自检
/// 腿组，判据同质恒 zero-fail——词表保持闭集同款（拼错值当场拒 boot）。
pub const RR3_SELFTEST_CONTRACTS_EXPECTS: [&str; 1] = ["zero-fail"];

/// 自检预期表（批N5 件五 F-31）：固定腿 speaker/relay/face 逐名期望三态 +
/// wiring / contracts 组预期——verdict 的判据集，整体红绿是结果集对预期集
/// 等值的派生量。装配期自 config `[rr3-interact.selftest.expect]` 读出构造
/// （parse 已对账词表），字段恒持在册值（'static 来自词表 lookup）。
#[derive(Debug, Clone, Copy)]
pub struct Rr3SelftestExpect {
    pub speaker: &'static str,
    pub relay: &'static str,
    pub face: &'static str,
    /// wiring/* 组预期（`zero-fail` = 组内零 fail 即组过）
    pub wiring: &'static str,
    /// contracts/* 组预期（批N5 件五笔4，Z-4：契约四册齐全且 sha 对）
    pub contracts: &'static str,
}

impl Default for Rr3SelftestExpect {
    fn default() -> Self {
        // 缺省 = 固定腿全 pass + 两组零 fail——speaker 内置测试音（批N5
        // 件五笔2）落地后缺省判据即全绿；形态性缺腿仍可改表如实申报（F-31）
        Self {
            speaker: "pass",
            relay: "pass",
            face: "pass",
            wiring: "zero-fail",
            contracts: "zero-fail",
        }
    }
}

impl Rr3SelftestExpect {
    /// 固定腿三态 lookup（'static 来自词表）；词表外 None，调用方点名拒 boot
    pub fn rr3_state_lookup(s: &str) -> Option<&'static str> {
        RR3_SELFTEST_LEG_STATES.iter().find(|&&v| v == s).copied()
    }

    /// wiring 组预期 lookup（'static 来自词表）；词表外 None，调用方点名拒 boot
    pub fn rr3_wiring_lookup(s: &str) -> Option<&'static str> {
        RR3_SELFTEST_WIRING_EXPECTS
            .iter()
            .find(|&&v| v == s)
            .copied()
    }

    /// contracts 组预期 lookup（'static 来自词表）；词表外 None，调用方点名拒 boot
    pub fn rr3_contracts_lookup(s: &str) -> Option<&'static str> {
        RR3_SELFTEST_CONTRACTS_EXPECTS
            .iter()
            .find(|&&v| v == s)
            .copied()
    }
}

/// 面板服务线程的共享面：总线句柄（[`BusHandle`]，clone = 同一连接的另一句柄，
/// 0.0.8 W-A 批4 换型——面板的 rr3.call 与主循环应答互不干扰）、总线路径
/// （ctl 短连用）、动作通道（beep/face 递主循环）、link 订阅缓存、五限额
/// （I-18②：config 数据化贯穿）与自检期望表情（I-17/M-10：产线取向可配）。
pub struct Rr3PanelShared {
    pub bus: BusHandle,
    pub bus_path: String,
    pub actions: mpsc::Sender<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>,
    pub link: Arc<Mutex<Option<Value>>>,
    /// 面板五限额/超时（批N4 件五 I-18②：装配期自 config 构造）
    pub limits: Rr3PanelLimits,
    /// 自检 face 腿的期望表情（批N4 件五 I-17/M-10：在册白名单值，'static
    /// 来自表情册 lookup，跨线程安全）
    pub selftest_face_expect: &'static str,
    /// 自检预期表（批N5 件五 F-31）：固定腿逐名期望三态 + wiring 组预期，
    /// verdict = 结果集与预期集等值（装配期 parse 已对词表，'static 值）
    pub selftest_expect: Rr3SelftestExpect,
}

/// 面板入口：绑 [`rr3_panel_addr`] 起 accept 循环（阻塞，调用方起线程收容）。
/// 每连接一线程，超 [`Rr3PanelLimits::max_conns`] 直接 503。
pub fn rr3_panel_serve(shared: Arc<Rr3PanelShared>) -> Result<(), String> {
    let addr = rr3_panel_addr();
    let listener = TcpListener::bind(&addr).map_err(|e| format!("绑面板 {addr} 失败: {e}"))?;
    eprintln!(
        "面板就绪: http://{addr}（{}端点，控制全走 RPC 归属门）",
        RR3_PANEL_ROUTES.len()
    );
    rr3_panel_warn_addr(&addr);
    rr3_accept_loop(listener, shared, Arc::new(AtomicUsize::new(0)), || {
        std::thread::Builder::new().name("rr3-panel-conn".into())
    })
}

/// 安全面自述：面板写口无令牌鉴权，非回环监听是"可信局域网"决策，启动即点名。
fn rr3_panel_warn_addr(addr: &str) {
    if !rr3_panel_loopback(addr) {
        eprintln!(
            "面板警告: 监听 {addr} 非回环——写口无令牌鉴权（继电器/密钥/配网/重启），仅限可信局域网"
        );
    }
}

/// 监听是否只绑回环：可解析 IP 才判 loopback；主机名/非法形状按非回环处理
/// （保守告警，不猜解析结果）。
fn rr3_panel_loopback(addr: &str) -> bool {
    addr.parse::<std::net::SocketAddr>()
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false)
}

/// 面板动作入队通道：动作 + 代执行回执发送端（主循环 on_tick 排空）
pub type Rr3PanelActRx = mpsc::Receiver<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>;

/// 面板状态页 link 槽（主循环 on_event 写、面板线程读）
pub type Rr3PanelLink = Arc<Mutex<Option<Value>>>;

/// [`spawn_host`] 的装配返回：动作入队端 + 状态页 link 槽
pub type Rr3PanelHost = (Rr3PanelActRx, Rr3PanelLink);

/// 面板线程宿主（0.0.8 W-A 批4 函数化，方案 §4.6）：link Arc＋动作通道＋
/// 五限额毫秒→Duration 换算＋[`Rr3PanelShared`] 构造＋线程 spawn 收进一处，
/// main.rs 一行装配。返回 `(act_rx, link)` 给 Dispatcher——act_rx 主循环
/// on_tick 排空（beep/face 代执行回执）、link 主循环 on_event 只写/面板线程
/// 只读（状态页卡片数据源，Mutex 语义不变）。线程名 rr3-panel；spawn 失败
/// Err 上抛（fail-closed，装配段显式回错）。
pub fn spawn_host(
    bus: &BusHandle,
    panel: &crate::boot::Rr3PanelCfg,
    selftest: &crate::boot::Rr3SelftestCfg,
) -> Result<Rr3PanelHost, String> {
    let link = Arc::new(Mutex::new(None::<Value>));
    let (act_tx, act_rx) = mpsc::channel();
    // 面板五限额/超时（批N4 件五 I-18②）：config 毫秒值转 Duration，装配期
    // 一次换算，server 侧机制面只见 Duration 不再携带策略
    let limits = Rr3PanelLimits {
        max_conns: panel.max_conns as usize,
        read_timeout: Duration::from_millis(panel.read_timeout_ms),
        header_limit: panel.header_limit as usize,
        body_limit: panel.body_limit as usize,
        call_timeout: Duration::from_millis(panel.call_timeout_ms),
    };
    let shared = Arc::new(Rr3PanelShared {
        bus: bus.clone(),
        bus_path: std::env::var(RR3_BUS_PATH_ENV)
            .unwrap_or_else(|_| RR3_UDS_PATH_DEFAULT.to_string()),
        actions: act_tx,
        link: Arc::clone(&link),
        limits,
        selftest_face_expect: selftest.face_expect,
        selftest_expect: selftest.expect,
    });
    rr3_panel_spawn(shared)?;
    Ok((act_rx, link))
}

/// 面板线程起转：绑核失败点名为装配期错（服务退出只落日志，不拉主循环）。
fn rr3_panel_spawn(shared: Arc<Rr3PanelShared>) -> Result<(), String> {
    std::thread::Builder::new()
        .name("rr3-panel".into())
        .spawn(move || rr3_panel_guard(shared))
        .map_err(|e| format!("起面板线程失败: {e}"))?;
    Ok(())
}

/// 面板线程体：serve 退出即点名（面板故障不带走主循环）。
fn rr3_panel_guard(shared: Arc<Rr3PanelShared>) {
    if let Err(e) = rr3_panel_serve(shared) {
        eprintln!("面板服务退出: {e}");
    }
}

/// accept 循环本体（批M M-β I-4 抽缝）：listener、槽位账与线程工厂显式注入——
/// 测试用 `make_builder` 第一次返回必败 builder（`stack_size(usize::MAX)`
/// 触发 pthread_attr_setstacksize EINVAL，实测确定性；小值如 1 会被 macOS
/// 静默 clamp 成功，不可作注入）注入 spawn 失败，钉"起不来一个连接后面板
/// 仍可访问"（工单回归判据）。生产路径工厂返回普通 builder，行为与旧 serve
/// 零漂移。
fn rr3_accept_loop(
    listener: TcpListener,
    shared: Arc<Rr3PanelShared>,
    live: Arc<AtomicUsize>,
    mut make_builder: impl FnMut() -> std::thread::Builder,
) -> Result<(), String> {
    // accept 失败退避（与 rrd 守护 accept 同款策略，200ms）：单次失败不许带走
    // 服务面——面板是运维的最后一扇门，EMFILE/ENFILE 等瞬态错误后必须还能接客。
    const RR3_ACCEPT_BACKOFF: Duration = Duration::from_millis(200);
    loop {
        let (mut stream, _) = match listener.accept() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("面板 accept 失败（继续服务）: {e}");
                std::thread::sleep(RR3_ACCEPT_BACKOFF);
                continue;
            }
        };
        // 连接 IO 耐心（读侧已在 rr3_http_read_req 设，写侧在此按同一预算补）：
        // 对端只连不读时 write_all 会一直堵在“发送缓冲已满”，8 个槽位被钉死＝
        // 面板整体失联（运维最后一扇门自锁，静态审计 B16）。
        if let Err(e) = rr3_http_patience(&stream, shared.limits.read_timeout) {
            eprintln!("面板连接写耐心未设上（连接仍按既有语义处理）: {e}");
        }
        if live.load(Ordering::Relaxed) >= shared.limits.max_conns {
            // 拒连必须留痕（信息面）：面板"打得开但一直 503"时，只有这行能指出
            // 是对端洪水还是自家槽位泄漏——对端地址是最小充分证据。
            let peer = stream
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|_| "?".to_string());
            eprintln!(
                "面板并发已达上限 {}（在服 {}），拒连对端 {peer}",
                shared.limits.max_conns,
                live.load(Ordering::Relaxed)
            );
            let body = br#"{"error":"conn limit"}"#;
            let _ = rr3_http_respond(
                &mut stream,
                503,
                "Service Unavailable",
                "application/json",
                body,
            );
            continue;
        }
        let slot = Rr3ConnSlot::rr3_acquire(&live);
        let shared = shared.clone();
        let shared_bump = shared.clone(); // Err 分支记账用（闭包 move 走主句柄）
                                          // spawn 失败（批M M-β I-4）：闭包（连同 stream 与守卫）被丢弃——守卫
                                          // Drop 归还槽位、连接自然关闭。**不退循环**：面板是运维的最后一扇门，
                                          // 一条连接起不来不能把整个面板带走（工单判据"起不来一个连接后面板仍
                                          // 可访问"）；点账 counters + stderr 点名，服务继续。
        if let Err(e) = make_builder().spawn(move || {
            rr3_panel_handle(&mut stream, &shared);
            drop(slot); // RAII 守卫：正常返回或 panic 展开都在此归还槽位
        }) {
            shared_bump.bus.counter_bump(RR3_PANEL_SPAWN_FAIL_KEY, 1);
            eprintln!("面板连接线程起失败（连接弃置，服务继续）: {e}");
        }
    }
}

/// 监听面判定：env 优先，缺省回环（Q3）。独立成函数供测试钉形状。
pub fn rr3_panel_addr() -> String {
    std::env::var(RR3_PANEL_ADDR_ENV).unwrap_or_else(|_| RR3_PANEL_ADDR_DEFAULT.to_string())
}

/// 单连接处理：读请求 → 路由 → 执行 → 应答。任何错误都折 JSON 应答——
/// 面板连接没有"协议违规掉线"语义，坏请求回 400 就是它的全部惩罚。
fn rr3_panel_handle(stream: &mut TcpStream, shared: &Rr3PanelShared) {
    let req = match rr3_http_read_req(stream, &shared.limits) {
        Ok(r) => r,
        Err(e) => {
            let reply = Rr3PanelReply::json(400, "Bad Request", json!({ "error": e }));
            rr3_reply_send(stream, &reply);
            return;
        }
    };
    let reply = rr3_panel_execute(&req, shared);
    rr3_reply_send(stream, &reply);
}

/// 应答写出（坏请求与正常应答同一出口，写失败不打断连接语义）。
fn rr3_reply_send(stream: &mut TcpStream, reply: &Rr3PanelReply) {
    let _ = rr3_http_respond(
        stream,
        reply.status,
        reply.reason,
        reply.content_type,
        &reply.body,
    );
}

/// 假 rrd 的 ctl 应答器（单测夹具）：accept 一条连接，读一帧，按预置的**完整
/// JSON-RPC 应答**（result 或 error 信封，测试自组——与 rrd 真实两态同形）回一帧。
#[cfg(test)]
fn rr3_fake_ctl_rrd(
    listener: std::os::unix::net::UnixListener,
    rpc: Value,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            if let Ok(req) = rr_ipc::rr3_frame_read(&mut stream) {
                let payload = rr3_body_encode(Rr3PayloadType::Response, rpc.to_string().as_bytes());
                let frame = Rr3Frame {
                    header: Rr3Header {
                        opcode: opcode::SEND,
                        flags: 0,
                        trace_id: 0,
                        timestamp_ns: rr3_now_ns(),
                        src_task: rr_core::RR3_TASK_RRD,
                        dst_task: req.header.src_task, // 按对端自报回（注册期 rrd 同口径）
                        payload_len: payload.len() as u32,
                        payload_crc32: rr_ipc::rr3_crc32(&payload),
                    },
                    payload,
                };
                let _ = rr3_frame_write(&mut stream, &frame);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    use rr_ipc::rr3_frame_read;

    use crate::panel::exec::{
        rr3_bus_reply_status, rr3_leg_relay, rr3_relay_expect, rr3_selftest_verdict,
    };

    /// 面板测试底座（四处同款）：缺省限流/期望 + 无主循环 link 的共享面
    fn panel_shared(
        bus: BusHandle,
        bus_path: String,
        actions_tx: mpsc::Sender<(Rr3PanelAction, mpsc::Sender<Result<Value, String>>)>,
    ) -> Arc<Rr3PanelShared> {
        Arc::new(Rr3PanelShared {
            bus,
            bus_path,
            actions: actions_tx,
            link: Arc::new(Mutex::new(None)),
            limits: Rr3PanelLimits::default(),
            selftest_face_expect: "happy",
            selftest_expect: Rr3SelftestExpect::default(),
        })
    }

    /// 单连接服务底座（三处同款）：一次 accept 交给真 handler，带回地址与线程句柄
    fn panel_serve_once(
        shared: Arc<Rr3PanelShared>,
    ) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = tcp.local_addr().unwrap();
        let jh = std::thread::spawn(move || {
            let (mut sock, _) = tcp.accept().unwrap();
            rr3_panel_handle(&mut sock, &shared)
        });
        (addr, jh)
    }

    /// 自检 HTTP roundtrip 底座（两处同款）：POST /api/selftest 单请求，
    /// 读全应答等线程收摊，解析出 JSON 体
    fn selftest_roundtrip(addr: std::net::SocketAddr, jh: std::thread::JoinHandle<()>) -> Value {
        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(b"POST /api/selftest HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}")
            .unwrap();
        let _ = c.shutdown(std::net::Shutdown::Write);
        let mut buf = Vec::new();
        c.read_to_end(&mut buf).expect("自检应答必须完整可读");
        jh.join().unwrap();
        let text = String::from_utf8_lossy(&buf);
        let body = text.split("\r\n\r\n").nth(1).expect("应答带 body");
        serde_json::from_str(body).expect("自检应答是 JSON: {body}")
    }

    /// BusCall 应答状态码裁定（I-21③）：载荷含 error = 422 业务失败，其余 200
    #[test]
    fn bus_reply_status_splits_business_error_from_success() {
        assert_eq!(
            rr3_bus_reply_status(&json!({ "played_ms": 120 })),
            (200, "OK"),
            "正常应答 200"
        );
        assert_eq!(
            rr3_bus_reply_status(&json!({ "expression": "talk", "previous": "idle" })),
            (200, "OK"),
            "多字段成功应答 200"
        );
        assert_eq!(
            rr3_bus_reply_status(&json!({ "error": "舵机越界拒收" })),
            (422, "Unprocessable Entity"),
            "业务错误 422（不得折 200+ok:true 假成功）"
        );
    }

    #[test]
    fn route_covers_all_table_entries() {
        // 端点逐一点名（全集 = RR3_PANEL_ROUTES，表↔分派漂移由下一条门钉）：
        assert_eq!(rr3_panel_route("GET", "/", b""), Rr3PanelDispatch::Html);
        assert_eq!(
            rr3_panel_route("GET", "/api/status", b""),
            Rr3PanelDispatch::StatusQuery
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/data", b""),
            Rr3PanelDispatch::CountersQuery
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/logs", b""),
            Rr3PanelDispatch::LogsQuery
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/relay", br#"{"on":true}"#),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-act/relay/set".into(),
                args: json!({ "on": true })
            }
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/beep", b""),
            Rr3PanelDispatch::PanelAct(Rr3PanelAction::Beep)
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/face", br#"{"expression":"happy"}"#),
            Rr3PanelDispatch::PanelAct(Rr3PanelAction::Face {
                expression: "happy".into()
            })
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/selftest", b""),
            Rr3PanelDispatch::SelfTest
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/reboot", b""),
            Rr3PanelDispatch::Reboot
        );
        // 配网三端点（0.0.4 批D）：scan/forget 无参 BusCall，connect body 透传
        assert_eq!(
            rr3_panel_route("GET", "/api/wifi/scan", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/scan".into(),
                args: json!({})
            }
        );
        assert_eq!(
            rr3_panel_route(
                "POST",
                "/api/wifi/connect",
                br#"{"ssid":"home","pass":"password8"}"#
            ),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/connect".into(),
                args: json!({ "ssid": "home", "pass": "password8" })
            }
        );
        assert_eq!(
            rr3_panel_route("POST", "/api/wifi/forget", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/wifi/forget".into(),
                args: json!({})
            }
        );
        // 五模块 ops 读口（批N4 件九二）：无参 BusCall 透传，各点名 owner 模块
        assert_eq!(
            rr3_panel_route("GET", "/api/brain/ops", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-brain/ops/list".into(),
                args: json!({})
            }
        );
        // 配置面写口（件九三）：group/slot/value body 原样透传，点名 owner 写臂
        assert_eq!(
            rr3_panel_route(
                "POST",
                "/api/brain/ops",
                br#"{"group":"asr","slot":"asr_model","value":"m2"}"#
            ),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-brain/ops/set".into(),
                args: json!({ "group": "asr", "slot": "asr_model", "value": "m2" })
            }
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/act/ops", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-act/ops/list".into(),
                args: json!({})
            }
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/connect/ops", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-connect/ops/list".into(),
                args: json!({})
            }
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/interact/ops", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-interact/ops/list".into(),
                args: json!({})
            }
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/power/ops", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-power/ops/list".into(),
                args: json!({})
            }
        );
        assert_eq!(
            rr3_panel_route("GET", "/api/sense/ops", b""),
            Rr3PanelDispatch::BusCall {
                topic: "rr3-sense/ops/list".into(),
                args: json!({})
            }
        );
        // 装配快照读口（批N4 件九笔4）：GET /api/wiring → ctl 七号方法透传
        assert_eq!(
            rr3_panel_route("GET", "/api/wiring", b""),
            Rr3PanelDispatch::WiringQuery
        );
        // 装配侧写口（批N22 件一）：POST /api/profile/domain → ctl.domain.set
        // 透传，body 原样进分派参数（门序裁决在 rrd 单点）
        assert_eq!(
            rr3_panel_route(
                "POST",
                "/api/profile/domain",
                r#"{"target":"domain","name":"rail5v","key":"armed","value":true,"confirm":true,"evidence":"实测/0001"}"#.as_bytes()
            ),
            Rr3PanelDispatch::ProfileDomainSet {
                params: json!({
                    "target": "domain", "name": "rail5v", "key": "armed",
                    "value": true, "confirm": true, "evidence": "实测/0001"
                })
            }
        );
        // 写口非 POST = 405（方法面收窄，与 relay/face 同口径）
        assert!(matches!(
            rr3_panel_route("GET", "/api/profile/domain", b""),
            Rr3PanelDispatch::MethodNotAllowed
        ));
    }

    /// 端点表 ↔ 路由分派漂移门（MC-8③/I-9）：表内每条必须分派成功（不是
    /// 404/405），表长 pin 与自述文案同源（端点数由 len() 推导）——新增端点 =
    /// 加表 + 改本 pin + 过本门，文案零改动。
    #[test]
    fn route_table_matches_dispatch() {
        assert_eq!(
            RR3_PANEL_ROUTES.len(),
            28,
            "端点表计数 pin：增删端点须同批改本 pin 与设计册 I-9 口径（26→28：增 GET /api/healthz + GET /api/version 探针端点，15b4556）"
        );
        for (method, path, _covers, _applies) in RR3_PANEL_ROUTES {
            let body: &[u8] = match (*method, *path) {
                ("POST", "/api/relay") => br#"{"on":true}"#,
                ("POST", "/api/face") => br#"{"expression":"happy"}"#,
                ("POST", "/api/wifi/connect") => br#"{"ssid":"s","pass":"password8"}"#,
                ("POST", "/api/model/key") => r#"{"ref":"测试键","value":"sk-x"}"#.as_bytes(),
                ("POST", "/api/brain/ops") => br#"{"group":"asr","slot":"asr_model","value":"m2"}"#,
                ("POST", "/api/brain/model") => {
                    r#"{"op":"activate","name":"DeepSeek官方"}"#.as_bytes()
                }
                ("POST", "/api/brain/model/test") => {
                    r#"{"name":"DeepSeek官方"}"#.as_bytes()
                }
                ("POST", "/api/profile/domain") => {
                    r#"{"target":"domain","name":"rail5v","key":"armed","value":true,"confirm":true,"evidence":"实测/0001"}"#.as_bytes()
                }
                _ => b"",
            };
            let d = rr3_panel_route(method, path, body);
            assert!(
                !matches!(
                    &d,
                    Rr3PanelDispatch::NotFound | Rr3PanelDispatch::MethodNotAllowed
                ),
                "{method} {path} 在端点表内但路由拒绝: {d:?}"
            );
        }
        // 反向：表外路径一律 404，不因加表而放开
        assert!(matches!(
            rr3_panel_route("GET", "/api/not-in-table", b""),
            Rr3PanelDispatch::NotFound
        ));
    }

    /// covers 申报词表形制门（I-19，批N4 件三）：每端点 covers 元素形制必须 =
    /// `<RR3_PANEL_COVER_SETS 词表集合>/<单段字段>`——集合词表外、缺斜杠、多段
    /// 都是申报面错，当场红（fail-closed；check.sh 步 ⑩ 侧另以同一词表导出复验，
    /// 两处真值同源都是本常量）。
    #[test]
    fn covers_use_declared_vocabulary_only() {
        for (method, path, covers, _applies) in RR3_PANEL_ROUTES {
            for c in *covers {
                let (set, field) = c
                    .split_once('/')
                    .unwrap_or_else(|| panic!("{method} {path} 的 covers {c:?} 缺 '/'"));
                assert!(
                    RR3_PANEL_COVER_SETS.contains(&set),
                    "{method} {path} 的 covers {c:?} 集合 {set:?} 不在 RR3_PANEL_COVER_SETS"
                );
                assert!(
                    !field.is_empty() && !field.contains('/'),
                    "{method} {path} 的 covers {c:?} 字段段须非空且单段"
                );
            }
        }
    }

    /// applies 申报词表门（I-20，批N4 件四，全案 §19.3 判据 1）：第四列非空
    /// 必须在 [`RR3_PANEL_APPLY_MODES`] 三态内；写口（非 GET）动了可配置集
    /// （covers 非空）则 applies 必填——缺失即红。今日 covers 全空 ⇒ 判据
    /// 后半 vacuous，但门先建：件九三面扩端点写 covers 的当场被兜住。
    #[test]
    fn applies_use_declared_vocabulary_only() {
        for (method, path, covers, applies) in RR3_PANEL_ROUTES {
            if !applies.is_empty() {
                assert!(
                    RR3_PANEL_APPLY_MODES.contains(applies),
                    "{method} {path} 的 applies {applies:?} 不在 RR3_PANEL_APPLY_MODES"
                );
            }
            if *method != "GET" && !covers.is_empty() {
                assert!(
                    !applies.is_empty(),
                    "{method} {path} 写口动了可配置集（covers 非空）却缺 applies 申报"
                );
            }
        }
        // 今日申报实况 pin：三条 applies = model-key 写口（live——secrets 落盘
        // 后 brain 每次现读，无需重启/重烧）＋ brain ops 写口（件九三，live 内存
        // 生效不落盘）＋ profile/domain 写口（批N22 件一，reboot——装配单落盘
        // 下次 rrd 装载投影）。改申报 = 同批改本 pin 与设计册 I-20。
        let declared: Vec<_> = RR3_PANEL_ROUTES
            .iter()
            .filter(|(_, _, _, a)| !a.is_empty())
            .collect();
        assert_eq!(
            declared,
            vec![
                &("POST", "/api/model/key", &[][..], "live"),
                &(
                    "POST",
                    "/api/brain/ops",
                    &[
                        "ops/asr_model",
                        "ops/base_url",
                        "ops/llm_model",
                        "ops/max_utterance_ms",
                        "ops/timeout_s",
                        "ops/tts_model"
                    ][..],
                    "live"
                ),
                &("POST", "/api/brain/model", &[][..], "live"),
                &(
                    "POST",
                    "/api/profile/domain",
                    &[
                        "domain/armed",
                        "domain/evidence",
                        "domain/severe_mv",
                        "axis/current_ceil_ma"
                    ][..],
                    "reboot"
                ),
            ],
            "applies 申报面漂移：新增写口申报须同批改本 pin"
        );
    }

    /// 越权拒的静态形态：路由器里没有"发布"这种产物——relay 只能以
    /// BusCall（rr3.call 经 rrd 归属门）出去。这是类型级断言，不是约定。
    #[test]
    fn hardware_control_is_bus_call_only_never_publish() {
        match rr3_panel_route("POST", "/api/relay", br#"{"on":false}"#) {
            Rr3PanelDispatch::BusCall { topic, .. } => {
                assert_eq!(topic, "rr3-act/relay/set", "面板控制走 act 既有 exposes");
            }
            other => panic!("relay 必须折 BusCall（无发布面）: {other:?}"),
        }
    }

    #[test]
    fn route_rejects_unknown_path_wrong_method_and_bad_body() {
        assert_eq!(
            rr3_panel_route("GET", "/api/none", b""),
            Rr3PanelDispatch::NotFound
        );
        assert_eq!(
            rr3_panel_route("DELETE", "/api/relay", b""),
            Rr3PanelDispatch::MethodNotAllowed
        );
        assert_eq!(
            rr3_panel_route("GET", "/", b""),
            Rr3PanelDispatch::Html,
            "GET / 不受其他端点方法约束"
        );
        // relay 缺 on / body 坏 JSON → 400 语义
        assert!(matches!(
            rr3_panel_route("POST", "/api/relay", b"{}"),
            Rr3PanelDispatch::BadRequest(_)
        ));
        assert!(matches!(
            rr3_panel_route("POST", "/api/relay", b"not-json"),
            Rr3PanelDispatch::BadRequest(_)
        ));
        assert!(matches!(
            rr3_panel_route("POST", "/api/face", b"{}"),
            Rr3PanelDispatch::BadRequest(_)
        ));
    }

    #[test]
    fn http_read_request_reads_full_body_and_rejects_half() {
        // 全量：声明 12B 读满 12B
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            rr3_http_read_req(&sock, &Rr3PanelLimits::default()).unwrap()
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let full = b"POST /api/relay HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"on\":true}";
        c.write_all(full).unwrap();
        let req = t.join().unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/relay");
        assert_eq!(req.body, b"{\"on\":true}");

        // 半包：声明 12B 只发 4B → 读超时折 Err，不 panic 不挂死
        let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr2 = listener2.local_addr().unwrap();
        let t2 = std::thread::spawn(move || {
            let (sock, _) = listener2.accept().unwrap();
            rr3_http_read_req(&sock, &Rr3PanelLimits::default())
        });
        let mut c2 = TcpStream::connect(addr2).unwrap();
        c2.write_all(b"POST /api/relay HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"on")
            .unwrap();
        let got = t2.join().unwrap();
        assert!(got.is_err(), "半包必须折 Err（读超时），不能装作读完");
    }

    #[test]
    fn ctl_query_roundtrips_and_reports_errors() {
        let dir = std::env::temp_dir().join(format!("panel-ctl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ctl.sock");

        // 成功回路：假 rrd 回 {"modules":[]}，result 原样回来
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let jh = rr3_fake_ctl_rrd(
            listener,
            json!({ "jsonrpc": "2.0", "id": 1, "result": { "modules": [] } }),
        );
        let v = rr3_ctl_query(
            path.to_str().unwrap(),
            "rr3.ctl.health",
            1,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(v["modules"], json!([]));
        jh.join().unwrap();

        // 错误信封：假 rrd 以 error 信封应答 → 折 Err 且文案带原文
        //（UDS 文件不随 listener drop 清理，重绑前先删）
        let _ = std::fs::remove_file(&path);
        let listener2 = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let jh2 = rr3_fake_ctl_rrd(
            listener2,
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "error": { "code": 3, "message": "reboot 执行面未解锁（需 cfg(linux) 且 env RR3_REBOOT_OK=1）" }
            }),
        );
        // 假 rrd 回的是 error 信封，rr3_ctl_query 读到 error 字段 → Err
        let got = rr3_ctl_query(
            path.to_str().unwrap(),
            "rr3.ctl.reboot",
            4,
            Duration::from_secs(5),
        );
        assert!(got.is_err(), "error 信封必须折 Err");
        assert!(
            got.unwrap_err().contains("reboot"),
            "拒因原文要在错误里（面板如实转达 rrd 文案）"
        );
        jh2.join().unwrap();
    }

    // ---------- 批M M-β I-4：spawn 失败不退 accept 循环 + 槽位 RAII ----------

    /// 假 rrd（I-4 夹具）：注册回 src_task=7、心跳回 accepted，并把每拍心跳
    /// params 原样记账——测试轮询 params.counters 断言 panel_spawn_fail 上报。
    const I4_MANIFEST: &str = "[manifest]\nschema_version = 1\nname = \"rr3-interact\"\nversion = \"0.0.1\"\ntype = \"module\"\nlang = \"rust\"\nentry = \"/bin/true\"\n\n[events]\npublishes = []\nsubscribes = []\n\n[health]\nheartbeat_ms = 50\n";

    #[cfg(test)]
    fn rr3_i4_fake_rrd(
        listener: std::os::unix::net::UnixListener,
        beats: Arc<Mutex<Vec<Value>>>,
        wiring: Value,
        contracts: Value,
    ) {
        // 每连接一线程：总线心跳是长连接（占住内层读环），而自检腿四/腿五的
        // ctl.wiring / ctl.contracts 查询会另开短连接——串行 accept 会让短连接
        // 永远排在长连接后面，5s 读超时把整个自检拖挂（真 rrd 也是并发连接，
        // 假件不得更弱）
        while let Ok((stream, _)) = listener.accept() {
            let beats = beats.clone();
            let wiring = wiring.clone();
            let contracts = contracts.clone();
            std::thread::spawn(move || rr3_i4_fake_rrd_conn(stream, beats, wiring, contracts));
        }
    }

    /// 假 rrd 单连接处理体（注册回 src_task=7、心跳回 accepted，并把每拍心跳
    /// params 原样记账；ctl.wiring / ctl.contracts 回各自预置应答）
    #[cfg(test)]
    fn rr3_i4_fake_rrd_conn(
        mut stream: std::os::unix::net::UnixStream,
        beats: Arc<Mutex<Vec<Value>>>,
        wiring: Value,
        contracts: Value,
    ) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        while let Ok(frame) = rr3_frame_read(&mut stream) {
            let Ok((ty, body)) = rr3_body_split_of(&frame) else {
                break;
            };
            let Ok(v) = serde_json::from_slice::<Value>(&body) else {
                break;
            };
            if ty != Rr3PayloadType::Request {
                continue;
            }
            let method = v["method"].as_str().unwrap_or("");
            let result = match method {
                "rr3.register" => json!({ "src_task": 7 }),
                "rr3.heartbeat" => {
                    beats.lock().unwrap().push(v["params"].clone());
                    json!({ "beat": "accepted" })
                }
                "rr3.ctl.wiring" => wiring.clone(),
                "rr3.ctl.contracts" => contracts.clone(),
                // 夹具按 classic 形态在册 rr3-act：relay 腿应走 on/off 真调用；
                // host-only 的 n-a 分支由纯函数单测覆盖。
                "rr3.ctl.status" => json!({
                    "modules": [{"name": "rr3-act", "task": 5, "phase": "running"}],
                    "denied_publish": 0,
                }),
                _ => json!({ "noop": true }),
            };
            let id = v["id"].as_u64().unwrap_or(0);
            let payload = rr3_body_encode(
                Rr3PayloadType::Response,
                json!({"jsonrpc": "2.0", "id": id, "result": result})
                    .to_string()
                    .as_bytes(),
            );
            let out = Rr3Frame {
                header: Rr3Header {
                    opcode: opcode::SEND,
                    flags: 0,
                    trace_id: 0,
                    timestamp_ns: rr3_now_ns(),
                    src_task: rr_core::RR3_TASK_RRD,
                    dst_task: 7,
                    payload_len: payload.len() as u32,
                    payload_crc32: rr_ipc::rr3_crc32(&payload),
                },
                payload,
            };
            if rr3_frame_write(&mut stream, &out).is_err() {
                break;
            }
        }
    }

    /// rr3_body_split 的 Result 化薄封装（夹具内让 `let Ok(..) .. else` 可用）
    #[cfg(test)]
    fn rr3_body_split_of(frame: &Rr3Frame) -> Result<(Rr3PayloadType, Vec<u8>), ()> {
        rr_ipc::rr3_body_split(&frame.payload).map_err(|_| ())
    }

    /// RAII 守卫的 panic 路径：连接线程 panic 展开时槽位必须归还（旧实现
    /// fetch_sub 在闭包尾，panic 即泄漏）。
    #[test]
    fn conn_slot_guard_releases_on_panic_unwind() {
        let live = Arc::new(AtomicUsize::new(0));
        let r = std::panic::catch_unwind(|| {
            let _slot = Rr3ConnSlot::rr3_acquire(&live);
            assert_eq!(live.load(Ordering::Relaxed), 1, "acquire 记 +1");
            panic!("boom（展开即走 Drop 归还）");
        });
        assert!(r.is_err(), "panic 应被 catch_unwind 捕获");
        assert_eq!(
            live.load(Ordering::Relaxed),
            0,
            "panic 展开必须归还槽位（守卫 Drop）"
        );
    }

    /// 工单回归判据：起不来一个连接后面板仍可访问。spawn 必败注入
    /// （stack_size(usize::MAX)）打头一条连接，后续连接照常 200；槽位账平；
    /// 失败进 counters（panel_spawn_fail 随心跳自报）。
    #[test]
    fn panel_survives_spawn_failure_and_bumps_counter() {
        // 前置自证：stack_size(usize::MAX) 的 spawn 在本平台确定性失败
        //（attr_setstacksize EINVAL；实测小值如 1 会被 macOS 静默 clamp，
        // 不可作注入，故取上溢值）
        assert!(
            std::thread::Builder::new()
                .stack_size(usize::MAX)
                .spawn(|| {})
                .is_err(),
            "stack_size(usize::MAX) 必须起不来线程（注入有效性自证）"
        );

        // 假 rrd + 真总线客户端（RR3_BUS_PATH 注入，连上即撤 env）
        let dir = std::env::temp_dir().join(format!("panel-i4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bus_path = dir.join("i4.sock");
        let beats: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
        {
            let beats = beats.clone();
            std::thread::spawn(move || rr3_i4_fake_rrd(listener, beats, json!({}), json!({})));
        }
        std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
        let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
        std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);

        let (actions_tx, _rx) = mpsc::channel();
        let shared = panel_shared(bus, bus_path.to_string_lossy().into_owned(), actions_tx);

        let live = Arc::new(AtomicUsize::new(0));
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = tcp.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let shared2 = shared.clone();
        let live2 = live.clone();
        let loop_jh = std::thread::spawn(move || {
            rr3_accept_loop(tcp, shared2, live2, move || {
                if calls2.fetch_add(1, Ordering::Relaxed) == 0 {
                    std::thread::Builder::new().stack_size(usize::MAX) // 第 1 次：必败注入
                } else {
                    std::thread::Builder::new().name("rr3-panel-conn".into())
                }
            })
        });

        // 第 1 条连接：spawn 失败 → 连接被弃置（无应答；服务端未读请求即关，
        // 客户端按平台收到 RST 或 FIN——两态共同点是**没有任何应答字节**）
        let mut c1 = TcpStream::connect(addr).unwrap();
        c1.write_all(b"GET / HTTP/1.1\r\nHost: t\r\n\r\n").unwrap();
        let _ = c1.shutdown(std::net::Shutdown::Write);
        let mut dropped = Vec::new();
        let _ = c1.read_to_end(&mut dropped);
        assert!(
            dropped.is_empty(),
            "spawn 失败的连接必须被弃置（无应答）: {dropped:?}"
        );

        // 第 2 条连接：正常 builder → 200 OK（工单判据：面板仍可访问）
        let mut c2 = TcpStream::connect(addr).unwrap();
        let _ = c2.set_read_timeout(Some(Duration::from_secs(5)));
        c2.write_all(b"GET / HTTP/1.1\r\nHost: t\r\n\r\n").unwrap();
        let mut rbuf = [0u8; 64];
        let n = c2.read(&mut rbuf).expect("spawn 失败后面板必须仍有应答");
        let head = String::from_utf8_lossy(&rbuf[..n]).to_string();
        assert!(
            head.starts_with("HTTP/1.1 200"),
            "spawn 失败后面板必须仍可访问: {head:?}"
        );

        // 槽位账平：失败连接（闭包连守卫被丢）与正常连接（守卫 Drop）都归还。
        // c2 的 handler 线程写完应答到 Drop 归还槽位之间无同步点——读到 200
        // 不等于线程已退出（门禁全量并行下撞过一回调度窗），故有界等待账平
        // （1s 上界；RAII 保证终会归还，此处不等调度器）。
        let settled = (0..100).any(|_| {
            if live.load(Ordering::Relaxed) == 0 {
                true
            } else {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        assert!(
            settled,
            "失败与正常连接都必须归还槽位（RAII 账平）: live={}",
            live.load(Ordering::Relaxed)
        );

        // counters 记账：假 rrd 收到的心跳 params.counters 带上 panel_spawn_fail
        //（心跳快照是累计值，多拍取 max；心跳周期 50ms，4s 窗口足量）
        let mut seen = 0u64;
        for _ in 0..40 {
            seen = beats
                .lock()
                .unwrap()
                .iter()
                .filter_map(|p| p.get("counters"))
                .filter_map(|c| c.get(RR3_PANEL_SPAWN_FAIL_KEY))
                .filter_map(Value::as_u64)
                .max()
                .unwrap_or(0);
            if seen >= 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            seen >= 1,
            "spawn 失败必须进 counters 心跳自报（panel_spawn_fail）: {seen}"
        );
        drop(loop_jh); // accept 循环不退出（判据本身），随测试进程收尾
    }

    /// 自检逐腿输出（批N4 件五 I-18③/D15）：应答 `{legs:[{leg,pass,reason?}],verdict}`——
    /// speaker 腿（主循环缺席场景）如实 fail 不折算通过；verdict = 结果集
    /// 与预期集等值（批N5 件五 F-31），不等值即 fail。
    #[test]
    fn selftest_reports_legs_and_honest_speaker_fail() {
        // 假 rrd + 真总线客户端（同 I4 夹具口径）：relay 腿经假 rrd 应答走通；
        // actions 接收者即刻 drop——speaker/face 两腿走"主循环不在"业务 Err，
        // 假 rrd 不预置 wiring（json!({})，available 缺）→ wiring/取数 如实 fail，
        // 四腿混合 pass/fail，verdict 必 fail
        let dir = std::env::temp_dir().join(format!("panel-selftest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bus_path = dir.join("st.sock");
        let listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
        std::thread::spawn(move || {
            rr3_i4_fake_rrd(
                listener,
                Arc::new(Mutex::new(Vec::new())),
                json!({}),
                json!({}),
            )
        });
        std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
        let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
        std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);

        let (actions_tx, _rx) = mpsc::channel(); // 接收者即刻 drop（下行）：主循环不在
        drop(_rx); // 显式 drop 保 send 失败立即折 Err，不撞 5s 调用耐心
        let shared = panel_shared(bus, bus_path.to_string_lossy().into_owned(), actions_tx);
        let (addr, jh) = panel_serve_once(shared);
        let v = selftest_roundtrip(addr, jh);

        let legs = v["legs"].as_array().expect("逐腿数组");
        assert_eq!(legs.len(), 5, "并列五腿（三调＋wiring 腿＋contracts 腿）");
        assert_eq!(legs[0]["leg"], "speaker");
        assert_eq!(
            legs[0]["pass"], "fail",
            "主循环缺席腿如实 fail，不折算通过（D15）"
        );
        assert!(
            legs[0]["reason"].as_str().unwrap().contains("主循环"),
            "失败原因进 reason（本场景主循环缺席；内置测试音的播放结果由活主循环回）"
        );
        assert_eq!(legs[1]["leg"], "relay");
        assert_eq!(legs[1]["pass"], "pass", "假 rrd 应答 noop，on/off 两步都成");
        assert_eq!(legs[2]["leg"], "face");
        assert_eq!(legs[2]["pass"], "fail", "主循环不在，face 腿走业务 Err");
        assert_eq!(
            legs[3]["leg"], "wiring/取数",
            "假 rrd 未预置 wiring 应答（available 缺）→ 取数单腿 fail"
        );
        assert_eq!(legs[3]["pass"], "fail");
        assert_eq!(
            legs[4]["leg"], "contracts/取数",
            "假 rrd 未预置 contracts 应答（available 缺）→ 取数单腿 fail（腿五同口径）"
        );
        assert_eq!(legs[4]["pass"], "fail");
        assert_eq!(
            v["verdict"], "fail",
            "混合态 verdict 必 fail（speaker/face 不等值 + wiring/contracts 组含 fail）"
        );
    }

    /// 自检腿四展开（批N4 件九笔4，全案 §7 ⑤ + F-31）：假 rrd 预置
    /// available:true + 三态混合 checks——pass 如实 pass、not-applicable 映射
    /// n-a（不拖累不救场）、未知值 fail-closed 折 fail（恒败腿不得折算通过）；
    /// 判果是 rrd 的产出，面板只转述（leg = "wiring/<item>" 逐条摊开）。
    #[test]
    fn selftest_wiring_leg_expands_checks_with_three_states() {
        let dir = std::env::temp_dir().join(format!("panel-wlegs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bus_path = dir.join("wl.sock");
        let listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
        let canned = json!({
            "available": true,
            "profile": "default",
            "checks": [
                { "item": "net:J-T:rail5v", "pass": "pass" },
                { "item": "signal:J-T[1]:gnd", "pass": "not-applicable",
                  "reason": "gnd-only 单线组不判导体" },
                { "item": "pin:J-T[2]:vcc", "pass": "weird-unknown", "reason": "未知值兜底" }
            ]
        });
        // 腿五 contracts 的预置应答（批N5 件五笔4）：available:true + 两条
        // 全 pass 判果——顺带钉 contracts/<item> 的逐条摊开形（与 wiring 同构）
        let canned_contracts = json!({
            "available": true,
            "contracts": [
                { "file": "topics.toml", "sha256": "64hex", "bytes": 1 }
            ],
            "checks": [
                { "item": "contract:topics.toml", "pass": "pass", "reason": "sha256 对上" },
                { "item": "contract:slots.toml", "pass": "pass", "reason": "sha256 对上" }
            ]
        });
        std::thread::spawn(move || {
            rr3_i4_fake_rrd(
                listener,
                Arc::new(Mutex::new(Vec::new())),
                canned,
                canned_contracts,
            )
        });
        std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
        let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
        std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);

        let (actions_tx, _rx) = mpsc::channel();
        drop(_rx); // 接收者即刻 drop：speaker/face 走业务 Err
        let shared = panel_shared(bus, bus_path.to_string_lossy().into_owned(), actions_tx);
        let (addr, jh) = panel_serve_once(shared);
        let v = selftest_roundtrip(addr, jh);

        let legs = v["legs"].as_array().expect("逐腿数组");
        assert_eq!(
            legs.len(),
            8,
            "三调腿＋wiring 三条逐条摊开＋contracts 两条: {v}"
        );
        assert_eq!(legs[0]["leg"], "speaker");
        assert_eq!(legs[0]["pass"], "fail", "主循环不在，如实 fail");
        assert_eq!(legs[1]["leg"], "relay");
        assert_eq!(legs[1]["pass"], "pass", "假 rrd noop 兜底应答，两步都成");
        assert_eq!(legs[2]["leg"], "face");
        assert_eq!(legs[2]["pass"], "fail", "主循环不在，如实 fail");
        assert_eq!(legs[3]["leg"], "wiring/net:J-T:rail5v");
        assert_eq!(legs[3]["pass"], "pass", "rrd 判 pass 如实转述");
        assert_eq!(legs[4]["leg"], "wiring/signal:J-T[1]:gnd");
        assert_eq!(
            legs[4]["pass"], "n-a",
            "not-applicable 映射 n-a，不折算 fail"
        );
        assert_eq!(legs[4]["reason"], "gnd-only 单线组不判导体", "reason 透传");
        assert_eq!(legs[5]["leg"], "wiring/pin:J-T[2]:vcc");
        assert_eq!(legs[5]["pass"], "fail", "未知 pass 值 fail-closed 折 fail");
        assert_eq!(legs[5]["reason"], "未知值兜底");
        // 腿五 contracts（批N5 件五笔4）：预置判果逐条摊 contracts/<item>，
        // reason 透传——与 wiring 腿同构的第二真值点禁令口径
        assert_eq!(legs[6]["leg"], "contracts/contract:topics.toml");
        assert_eq!(legs[6]["pass"], "pass");
        assert_eq!(legs[6]["reason"], "sha256 对上");
        assert_eq!(legs[7]["leg"], "contracts/contract:slots.toml");
        assert_eq!(legs[7]["pass"], "pass");
        assert_eq!(
            v["verdict"], "fail",
            "wiring 组含 fail → verdict fail（零 fail 判据；n-a 不救场也不拖累）"
        );
    }

    /// 自检 verdict 预期表等值判定（批N5 件五 F-31，纯函数钉门）：等值 pass／
    /// 固定腿不等值 fail／缺腿 fail／wiring 组含 fail fail／腿册外未知腿名
    /// fail-closed fail；预期可改数据——恒败腿如实申报期望 fail 即等值过
    /// （改数据不改码）。
    #[test]
    fn selftest_verdict_compares_result_set_against_expect_table() {
        let leg = |name: &str, pass: &str| json!({ "leg": name, "pass": pass });
        // 等值 pass：固定腿全 pass + wiring 组零 fail（pass/n-a 混合不拖累）
        let legs = vec![
            leg("speaker", "pass"),
            leg("relay", "pass"),
            leg("face", "pass"),
            leg("wiring/net:J-T:rail5v", "pass"),
            leg("wiring/signal:J-T[1]:gnd", "n-a"),
        ];
        assert_eq!(
            rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
            "pass",
            "结果集与缺省预期全等值"
        );
        // 预期改数据（F-31）：恒败形态如实申报期望 fail／n-a，等值即过
        let expect = Rr3SelftestExpect {
            speaker: "fail",
            face: "n-a",
            ..Default::default()
        };
        let legs = vec![
            leg("speaker", "fail"),
            leg("relay", "pass"),
            leg("face", "n-a"),
        ];
        assert_eq!(
            rr3_selftest_verdict(&legs, &expect),
            "pass",
            "预期表改数据即改判据，结果与预期等值即过"
        );
        // 固定腿不等值：speaker 结果 fail 而预期 pass → 红
        let legs = vec![
            leg("speaker", "fail"),
            leg("relay", "pass"),
            leg("face", "pass"),
        ];
        assert_eq!(
            rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
            "fail",
            "固定腿三态不等值即红"
        );
        // wiring 组含 fail → 红（组内零 fail 判据）
        let legs = vec![
            leg("speaker", "pass"),
            leg("relay", "pass"),
            leg("face", "pass"),
            leg("wiring/pin:J-T[2]:vcc", "fail"),
        ];
        assert_eq!(
            rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
            "fail",
            "wiring 组零 fail 判据"
        );
        // contracts 组（批N5 件五笔4，Z-4）：组内零 fail 即组过；含 fail 即红
        let legs = vec![
            leg("speaker", "pass"),
            leg("relay", "pass"),
            leg("face", "pass"),
            leg("contracts/contract:topics.toml", "pass"),
            leg("contracts/contract:roles.toml", "pass"),
        ];
        assert_eq!(
            rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
            "pass",
            "contracts 组零 fail 即组过"
        );
        let legs = vec![
            leg("speaker", "pass"),
            leg("relay", "pass"),
            leg("face", "pass"),
            leg("contracts/contract:roles.toml", "fail"),
        ];
        assert_eq!(
            rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
            "fail",
            "contracts 组零 fail 判据"
        );
        // 结果集缺固定腿 → 红（缺的腿无从对表）
        let legs = vec![leg("speaker", "pass"), leg("relay", "pass")];
        assert_eq!(
            rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
            "fail",
            "缺腿即红"
        );
        // 腿册外未知腿名 → fail-closed 红（结果集多腿不容静默）
        let legs = vec![
            leg("speaker", "pass"),
            leg("relay", "pass"),
            leg("face", "pass"),
            leg("arm", "pass"),
        ];
        assert_eq!(
            rr3_selftest_verdict(&legs, &Rr3SelftestExpect::default()),
            "fail",
            "腿册外未知腿名 fail-closed"
        );
    }

    /// relay 腿能力面（N44 后续）：rr3-act 缺席或配置显式 n-a → n-a；
    /// 在册且未显式 n-a → 沿用预期表（classic 仍 pass/fail 照跑）。
    #[test]
    fn selftest_relay_expect_n_a_when_capability_absent_or_configured() {
        assert_eq!(rr3_relay_expect(false, "pass"), "n-a");
        assert_eq!(rr3_relay_expect(false, "fail"), "n-a");
        assert_eq!(rr3_relay_expect(true, "pass"), "pass");
        assert_eq!(rr3_relay_expect(true, "fail"), "fail");
        assert_eq!(rr3_relay_expect(true, "n-a"), "n-a");
    }

    /// relay 腿 ctl 取数失败必须 fail-closed（0.0.8c 审计）：腿如实 fail，
    /// 期望态不得记成 fail 抵消等值——能力面不可核验时 verdict 必红。
    #[test]
    fn selftest_relay_ctl_failure_is_fail_closed() {
        let dir = std::env::temp_dir().join(format!("panel-relay-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 总线连接只用于构造 BusHandle：本腿在 ctl 失败分支不会触总线
        let bus_path = dir.join("bus.sock");
        let bus_listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
        std::thread::spawn(move || {
            rr3_i4_fake_rrd(
                bus_listener,
                Arc::new(Mutex::new(Vec::new())),
                json!({}),
                json!({}),
            )
        });
        std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
        let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
        std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);

        // ctl 短连指向错误信封：ctl.status 取数失败
        let ctl_path = dir.join("ctl.sock");
        let ctl_listener = std::os::unix::net::UnixListener::bind(&ctl_path).unwrap();
        let jh = rr3_fake_ctl_rrd(
            ctl_listener,
            json!({
                "jsonrpc": "2.0", "id": 6,
                "error": { "code": 3, "message": "ctl.status 不可用" }
            }),
        );

        let (actions_tx, _rx) = mpsc::channel();
        let shared = panel_shared(bus, ctl_path.to_string_lossy().into_owned(), actions_tx);
        let (leg, want) = rr3_leg_relay(&shared);
        assert_eq!(leg["pass"], "fail", "取数失败如实记 fail");
        assert_eq!(want, "pass", "期望态不得被记成 fail 抵消");
        let legs = vec![
            json!({ "leg": "speaker", "pass": "pass" }),
            leg,
            json!({ "leg": "face", "pass": "pass" }),
        ];
        let expect = Rr3SelftestExpect {
            relay: want,
            ..Default::default()
        };
        assert_eq!(
            rr3_selftest_verdict(&legs, &expect),
            "fail",
            "能力核验故障不得假绿"
        );
        jh.join().unwrap();
    }

    /// verdict 对 relay=n-a 的 host-only 形态等值即过（n-a 不拖累）。
    #[test]
    fn selftest_verdict_accepts_n_a_relay_for_host_only() {
        let legs = vec![
            json!({ "leg": "speaker", "pass": "pass" }),
            json!({ "leg": "relay", "pass": "n-a" }),
            json!({ "leg": "face", "pass": "pass" }),
            json!({ "leg": "wiring/net:J0:rail5v", "pass": "pass" }),
        ];
        let expect = Rr3SelftestExpect {
            relay: "n-a",
            ..Default::default()
        };
        assert_eq!(rr3_selftest_verdict(&legs, &expect), "pass");
    }

    /// 接线图 HTTP 透传（批N4 件九笔4）：GET /api/wiring → ctl.wiring 一问一答，
    /// rrd 应答原样 200 回浏览器（available:true 快照在册面）。
    #[test]
    fn wiring_endpoint_passes_through_canned_snapshot() {
        let dir = std::env::temp_dir().join(format!("panel-wpass-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bus_path = dir.join("wp.sock");
        let listener = std::os::unix::net::UnixListener::bind(&bus_path).unwrap();
        let canned = json!({
            "available": true,
            "profile": "default",
            "pins": [],
            "domains": [ { "name": "rail5v", "nominal_mv": 5000 } ],
            "wires": [ { "connector": "J-T", "net": "rail5v" } ],
            "checks": [ { "item": "net:J-T:rail5v", "pass": "pass" } ]
        });
        let expect = canned.clone();
        std::thread::spawn(move || {
            rr3_i4_fake_rrd(
                listener,
                Arc::new(Mutex::new(Vec::new())),
                canned,
                json!({}),
            )
        });
        std::env::set_var(rr_ipc::RR3_BUS_PATH_ENV, bus_path.to_str().unwrap());
        let bus = BusHandle::wrap(Rr3BusClient::rr3_connect(I4_MANIFEST).expect("连假 rrd 应成功"));
        std::env::remove_var(rr_ipc::RR3_BUS_PATH_ENV);

        let (actions_tx, _rx) = mpsc::channel();
        let shared = panel_shared(bus, bus_path.to_string_lossy().into_owned(), actions_tx);
        let (addr, jh) = panel_serve_once(shared);

        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(b"GET /api/wiring HTTP/1.1\r\n\r\n").unwrap();
        let _ = c.shutdown(std::net::Shutdown::Write);
        let mut buf = Vec::new();
        c.read_to_end(&mut buf).expect("接线图应答必须完整可读");
        jh.join().unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "透传 200: {text}");
        let body = text.split("\r\n\r\n").nth(1).expect("应答带 body");
        let v: Value = serde_json::from_str(body).expect("应答是 JSON: {body}");
        assert_eq!(v, expect, "rrd 应答原样透传，面板不重判不改写");
    }

    /// covers 计数 pin（批N4 件九笔4，门 d 分母同源）：/api/wiring 申报
    /// [pins]×wiring 全交叉读口的四族词表全集——pin 11 + role 7 + domain 12 +
    /// axis 14 = 44，与 check.sh 步 ⑩ 的分母导出逐项对齐（红账 43 全转绿的
    /// 面板侧申报表；domain 12 = 批N21 件二 evidence 键随批N21 件四补申报）；
    /// 增删字段 = 改端点表 + 改本 pin + 过门 d。
    #[test]
    fn wiring_endpoint_declares_full_cross_cover_vocabulary() {
        let entry = RR3_PANEL_ROUTES
            .iter()
            .find(|(_, p, _, _)| *p == "/api/wiring")
            .expect("端点表必含 /api/wiring");
        let covers: Vec<&str> = entry.2.to_vec();
        assert_eq!(covers.len(), 44, "四族全集 44 项: {covers:?}");
        let count = |prefix: &str| covers.iter().filter(|c| c.starts_with(prefix)).count();
        assert_eq!(count("pin/"), 11, "pin 族 11 字段（RR3_PIN_KEYS 同源）");
        assert_eq!(
            count("role/"),
            7,
            "role 族 7 字段（roles.toml requires 同源）"
        );
        assert_eq!(
            count("domain/"),
            12,
            "domain 族 12 字段（RR3_DOMAIN_KEYS 同源）"
        );
        assert_eq!(
            count("axis/"),
            14,
            "axis 族 14 字段（RR3_ACT_PROFILE_FIELDS 同源）"
        );
        let mut sorted = covers.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), covers.len(), "covers 申报不得重复");
    }

    /// 回环判定：IP 才判 loopback；主机名/非法形状按非回环（保守告警）。
    #[test]
    fn panel_loopback_only_accepts_loopback_ips() {
        assert!(rr3_panel_loopback("127.0.0.1:8080"));
        assert!(rr3_panel_loopback("[::1]:8080"));
        assert!(!rr3_panel_loopback("0.0.0.0:8080"));
        assert!(!rr3_panel_loopback("192.168.1.10:8080"));
        assert!(!rr3_panel_loopback("localhost:8080"));
        assert!(!rr3_panel_loopback("not-an-addr"));
    }
}
