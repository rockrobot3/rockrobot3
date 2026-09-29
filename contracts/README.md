# contracts —— 字节级共享契约（[SC] 唯一事实源）

Rust 基座（`base/runtime/rr-ipc`）与 Python brain（`rr3-brain`，M4）对同一份文件做编解码。
任何一侧改格式，必须先改这里，且只增不改。

## 内容

| 文件 | 作用 | 稳定级 |
|---|---|---|
| `envelope.schema.json` | A-IPC 128B 消息头逐字节偏移表 + payload 类型/flags/错误码枚举 | L1 |
| `golden/event-header-128.hex` | 一条真实头的 hex 向量（IPC-T-001/003 基准） | L1 |
| `topics.toml` | 全部主题注册表 + `[owners]` 前缀归属（rrd 启动与 CI 双重校验） | L2 |
| `events/*.schema.json` | 各模块事件负载 schema（首个真实 schema 随 M3 落） | L2 |

## 演进规则（ADR-0007 决策二）

1. **只增不改**：既有字段、偏移、枚举值不得变更语义
2. 破坏性变更必须升 `schema.version` major，并在 `topics.toml` 登记受影响主题
3. 消息头扩展**只允许写 reserved 区**并经 `flags` 标记启用（保持与 A-IPC 标准逐字节兼容）
4. CI 三检查在 PR 上强制：workspace 编译 / manifest schema 校验 / 主题前缀归属

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
