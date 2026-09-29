# contracts —— 字节级共享契约（[SC] 唯一事实源）

Rust 基座（`base/runtime/rr-ipc`）与 Python brain（`rr3-brain`）对同一份文件做编解码；brain 侧
何时接入属实况，见 [docs/status.md](../docs/status.md)。
任何一侧改格式，必须先改这里，且只增不改。

## 内容

| 文件 | 作用 | 稳定级 |
|---|---|---|
| `envelope.schema.json` | A-IPC 128B 消息头逐字节偏移表 + payload 类型/flags/错误码枚举 | L1 |
| `golden/event-header-128.hex` | 一条真实头的 hex 向量（IPC-T-001 的**同机**基准；跨架构等值另见下） | L1 |
| `topics.toml` | 主题所有权注册表：`[[owner]]` + `[[topic]]` 两组表（校验入口共三处——`rr3-mkmod` 生成自检、`rr-registry` 契约测试、rrd 在册表判定 `RrdRegistry::rr3_reg_publish_check`；职责与顺序见 [docs/architecture/architecture.md §6.3](../docs/architecture/architecture.md)，第三个入口的线上接线进度属实况，见 [docs/status.md](../docs/status.md)） | L2 |
| `events/*.schema.json` | 各模块事件负载 schema（命名规则与模板见 [`events/README.md`](events/README.md)） | L2 |

> **golden 向量的效力边界**：它钉住的是"本机 Rust 编解码与契约字节逐位一致"（IPC-T-001 侧），
> 跨架构（x86 与 aarch64 布局一致，即 IPC-T-003）的等值证明需要交叉编译两端对拍，那属**契约外**的
> 验证环节、本契约未覆盖它，故 IPC-T-003 记在 `envelope.schema.json` 的 `x-rock3-v0.0.1-uncovered` 里；
> 它何时补上属实况，见 [docs/status.md](../docs/status.md)。

> **`envelope.schema.json` 是"给人读、给测试对照"的数据契约，不是喂校验器的 schema**：它该读、
> 该对照，但**不该**喂给 JSON Schema 校验器——文件里的 `"$schema": ".../draft/2020-12/schema"` 键
> 是从模板带过来的残迹（vestigial），文件真正承载的是 `x-rock3-*` 偏移表与冻结枚举值，按 JSON
> Schema 语义解释会得出错误结论。（`events/*.schema.json` 落定时则相反：那才是给校验器用的负载
> schema。）契约的执行入口分两层说：envelope 字节级由 `cargo test -p rr-ipc`（偏移/枚举对齐
> 与 `tests/envelope_meta.rs` 的元数据守护，读 JSON 而非 Rust 自比；守护条数的唯一登记处是
> [docs/process/conventions.md](../docs/process/conventions.md) §2，本文件不复述数字）与
> `bash tools/ci/check.sh`（manifest、主题前缀与注册表形状）兜住；主题前缀归属则在生成/契约
> 两路自检之外有第三处入口——rrd 在册表 `RrdRegistry::rr3_reg_publish_check`（判定逻辑已交付，
> 是否接到线上连接路径以 [docs/status.md](../docs/status.md) 为准；三入口职责见
> [docs/architecture/architecture.md](../docs/architecture/architecture.md) §6.3）。

## topics.toml 裁定

1. 主题字符串**不进 128B 消息头**（ADR-0006 §2：topic 置于 payload 内）。本文件是"谁有权发布什么"的**离线注册表**，运行时用于校验与文档生成，不参与线格式。
2. 规范形式：`<owner>/<域>/<事件>`，三段。`owner` 必须等于某个模块的 `[manifest].name`（方案 §4：`^rr3-[a-z]+$`，**词干只许小写字母**，不含数字与连字符）；域与事件段用小写字母 + 下划线（`^[a-z_]+$`，与 capability 段同规则）。**逐条列举而非正则通配**，是为了让"新增主题必须改契约文件"成为显式动作（改契约 = 走 review）。
3. 每个 topic 的 owner **必须**在 `[[owner]]` 中登记；一个 topic 只有一个 owner（发布方唯一），订阅方可多个。
4. `kind` 取值 `request|event|stream|notification`（**4 种**），A-IPC 的 `payload_type` 枚举是
   **5 种**（REQUEST=1 / RESPONSE=2 / EVENT=3 / STREAM=4 / NOTIFICATION=5），两者**不是一一对应**：
   `kind` 说的是主题的语义用途，`payload_type` 是线上字节值，映射关系为——

   | `kind` | 对应 `payload_type`（冻结值） | 说明 |
   |---|---|---|
   | `request` | REQUEST(1) **+** RESPONSE(2) | 一次 RPC 往返在线上占两个 payload 类型；注册表按"主题"登记，故折叠为一项 |
   | `event` | EVENT(3) | 尽力投递，允许丢失 |
   | `stream` | STREAM(4) | 有序投递（ADR-0007 实时控制面候选） |
   | `notification` | NOTIFICATION(5) | 心跳 / 系统通知 |

   即：4 个 `kind` 覆盖 5 个 `payload_type`，唯一的折叠处是 `request`。注册表当前的 owner 数、
   topic 数与 kind 分布**就是 `topics.toml` 本身**，本文件不抄副本（抄来的数一次 append 就漂）。
   STREAM 类主题 v0.0.1 仅登记、不实现传输。
5. **上述形状是机器钉住的**：`cargo test -p rr-registry --test contract` 里的
   `registry_shape_is_pinned` 按 `contracts/topics.toml` 的解析结果断言 owner 数、topic 数、
   `kind` 分布，以及全部 `(name, owner, kind)` 三元组逐条在册。计数做成硬 pin 是
   **刻意的，不是禁止 append**：新增主题合法，但必须是一次有意识的编辑——同一次改动里同时更新
   `contracts/topics.toml`、该测试的期望值，以及任何把这些数字当事实抄进正文的文档
   （用例条数的唯一登记处是 [docs/process/conventions.md](../docs/process/conventions.md) §2）。
   删主题、改 kind、改 owner 数都会红。

## 演进规则（ADR-0007 决策二）

1. **只增不改**：既有字段、偏移、枚举值不得变更语义
2. 破坏性变更必须升 `envelope.schema.json` 顶层的 **`x-schema-version`** major（该键是 2026-09-29
   追加的元数据键，当前值 1，由 `cargo test -p rr-ipc --test envelope_meta` 与 Rust
   `RR3_CONTRACT_SCHEMA_VERSION` 逐值钉住），并在 `topics.toml` 登记受影响主题。
   `topics.toml` 与 `base/runtime/rr-registry/src/module.schema.toml` 各自另有同名整型键
   `schema_version`（当前同为 1），三者不要混写
3. 消息头扩展**只允许写 reserved 区**并经 `flags` 标记启用（保持与 A-IPC 标准逐字节兼容）
4. 七步检查以 `bash tools/ci/check.sh` 为**唯一入口**（本地与 CI 同一命令，命名沿用方案 D7 的"三检查"；
   远程在 atomgit，平台自动触发待确认，见方案 §7 遗留项）：
   ① workspace 编译（`--all-targets`，含测试目标）/ ①b workspace 全量测试 /
   ② manifest schema 校验 + 注册表形状 pin / ②b mkmod 输出即契约用例 / ③ 主题前缀归属 /
   ④ 仓内所有独立 Rust 工程编译 / ⑤ 文档门（已跟踪 md 不得为空、相对链接不得断）
   （步序、命令与每步条数的权威表见 [docs/process/conventions.md](../docs/process/conventions.md) §2，
   本文件不抄数字）

### 演进登记（逐条留痕）

- **2026-09-29 · `envelope.schema.json` 追加顶层元数据键 `x-schema-version`（值 1）**：
  只加一个给人读/给测试对照的整型元数据键，**不动任何偏移、字段宽度、枚举值与线上行为**，
  故属"只增不改"允许范围，**不是** schema major 升级（版本仍为 1）。加它的理由是演进规则 2 此前
  无实物可指：`RR3_CONTRACT_SCHEMA_VERSION`（rrd 横幅的 "contracts schema v1"）当时无人断言。
- **2026-09-29 · `envelope.schema.json` 的 `x-rock3-v0.0.1-coverage` 去掉 `IPC-T-003`**，并新增
  `x-rock3-v0.0.1-uncovered` 记录 IPC-T-003（跨架构字节布局一致）待 M2 的交叉编译等值测试。
  理由一句话：**coverage 列表是"关于测试的陈述"，纠正它不是契约变更**——它不触碰偏移、枚举、
  magic 与 golden 向量，因此不适用"只增不改"的冻结约束，而是评审裁定的许可项。

## 标准歧义登记（每半年随 ADR-0004 复查核对）

- **magic 的字节序**：A-IPC 标准 §2.2/§2.3 以 C 语义声明 `magic` **字段值**为 `0x41524531`
  并规定所有多字节字段小端序（AOS-STD-IPC-003/020，L1 稳定级），据此线上字节为
  `31 45 52 41`；而同一文档的注释 `'A''R''E''1'` 与 §7.2 Wireshark 伪代码
  `buffer(0,4):uint()`（大端读）给出 `41 52 45 31`。两处结论相反。
  **rock3 采规范条目（L1）为准：字段值 0x41524531，LE 存储，线上字节 31 45 52 41。**
  理由是与真实 C 实现互通（`hdr.magic = AIRY_IPC_MAGIC` 在小端机落地即此字节序），
  而非与 ASCII 注释互通。若标准维护方认定注释为准，本文件、`envelope.schema.json`
  与 `golden/*.hex` 一起升 major。
- **payload type 前缀占几字节**：标准 §7.2/§7.3 说"payload 类型由 payload 体首字段携带"，
  但 §3.2–3.6 的 C 结构体首字段分别是 `request_id`/`event_id`/`stream_id`，未写清该前缀
  的宽度与对齐。**v0.0.1 只冻结 128B 头与 5 个 payload 类型枚举值，payload 视为不透明
  JSON 字节**；前缀编码留 M2 决策（届时同步影响 IPC-T-004 的 golden 向量）。
  Rust 侧实现按 **2 字节小端**取值（`rr-ipc` 的 `RR3_BODY_TYPE_PREFIX_LEN`，可照抄的字面量见
  [docs/reference/rrd-rpc.md](../docs/reference/rrd-rpc.md) §1），**契约本体尚未把这一宽度写进
  `envelope.schema.json` 的冻结范围**——把它升进契约是一次契约变更（走 review），不是实现细节。
