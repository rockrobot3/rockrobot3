# ADR-0023：LLM 连接语义统一与模型行写面

- **状态**：已接受（用户 2026-10-24 口径：*"LLM 的使用，统一走 OpenAI 的格式，不绑定具体的厂商，
  通过操作面板来配置，可以手动选择 deepseek，glm，qwen 和其他 openai 兼容格式"*）。
  本 ADR 把"换厂商"从**改代码**降为**改一行配置**，并给模型行一条可落盘、可回滚、可探测的写面。
- **日期**：2026-10-24
- **关联**：[ADR-0017](ADR-0017-模块内部分层-机制与策略分离.md)（机制/策略分离——模型行是策略面，
  客户端是机制面）· [ADR-0019](ADR-0019-原子功能模块模型-格件面与组合代数.md)（格/件/面申报）·
  [ADR-0021](ADR-0021-三独立形态与资源供给面-能力原子化补课.md)（面板覆盖率与"可观测可设置"口径）·

## 背景

三条事实，都是"配置面不完整"的不同症状：

1. **厂商被焊进路径拼装**：`Rr3LlmClient` 曾把 `cloud.base_url` 当主机根，自行拼 `/v1/chat/completions`。
   这在 SiliconFlow 形状下成立，在 GLM（`/api/paas/v4`）、Qwen 兼容模式
   （`/compatible-mode/v1`）、以及"用户给的是完整根"的常见填法下全部错位。路径拼装是**机制**，
   但版本段是**厂商事实**——机制不该猜策略。
2. **模型连接没有写面**：`[[rr3-brain.model]]` 在 `config.toml` 里可读可解析，但面板只能改
   `cloud` 段槽值（`rr3-brain/ops/set`，live 不落盘）。换厂商要么手改 TOML，要么重启回旧值——
   "面板可配置"在产品口径上不成立。
3. **应答面有泄密风险**：密钥面已经收口到仓外 `secrets.json`（只回 ref 名），模型行若把
   `api_key` 一起快照出去，前一轮的护城河会在新端点上被绕过。

## 决策

### 裁定 1：`base_url` = 完整 OpenAI 兼容 API 根（含版本段）

- 模型行 `base_url` 是**完整根**：`https://api.deepseek.com/v1`、`https://open.bigmodel.cn/api/paas/v4`、
  `https://dashscope.aliyuncs.com/compatible-mode/v1`；客户端**只追加** `/chat/completions`。
- `cloud.base_url` **语义不变**：仍是主机根，ASR/TTS 照旧拼 `/v1/audio/*`（两条语义各有其消费方，
  不合并——合并会让 ASR 要么多一段要么少一段）。
- 写面校验（`rr3_model_base_url_validate`）：非空、`http(s)://` 开头、不得以 `/chat/completions` 结尾
  （防止"客户端再拼一段"的双拼）。boot 与写面同一条判据，单真值。
- fallback 路径（无模型行时读 `cloud.llm_model`）：`rr3_model_api_root(cloud.base_url)`——有路径段原样用，
  无路径段补 `/v1`。这是**兼容旧配置**的唯一一处猜测，登记在此，新增写面不再走它。

### 裁定 2：模型行三 RPC（list / set / test）

| topic | 语义 | 应答 |
|---|---|---|
| `rr3-brain/model/list` | 行表快照 | `{models:[{name,mode,api_format,base_url,model_id,key_ref,key_set,active}], active}` |
| `rr3-brain/model/set` | `op = upsert / activate / delete` | `{ok,op,name,active,persisted:true,applies:"live"}` |
| `rr3-brain/model/test` | 按名探测一次 | 成功 `{ok:true,status,latency_ms,mock}`；业务失败 `{ok:false,error}` |

- **只回 `key_set` 布尔，永无密钥明文**；`key_ref` 是引用名不是值（`env:VAR` → 环境变量，裸名 →
  `~/.rr3/secrets.json`），与密钥面同一双源语法。
- **恰一 active** 是写面自守的结构门（不是靠 boot 兜底）：0 行或多行直接拒；`delete` 拒删 active 行
  （先 `activate` 再删），保证任何时刻都有可用的连接。
- `test` 的**业务失败不折 RPC 错误**：面板要能区分"RPC 不通"与"厂商接口不通"，后者用 `ok:false` 表达。

### 裁定 3：落盘序（备份 → 验解析 → 原子写 → 提交内存）

写面顺序固定，任一步失败**内存与旧文件都不变**：

1. 克隆行表，施加 op，过结构门；
2. 读旧全文，`rr3_model_block_rewrite` 只替换 `[[rr3-brain.model]]` 块（块外逐字保留）；
3. 新文本先过 `rr3_brain_config_parse`（写盘前的最后一道门，避免写出自己都读不回的配置）；
4. `config.toml.bak` 存旧全量 → `rr_ipc::rr3_atomic_store`（tmp 全量 → fsync → rename）；
5. 提交 `cfg.doc` / `cfg.model`，并重建 LLM 客户端（与 `ops/set` 的 llm 组同一重建函数）。

**备份不成功就不动正式文件**；内容无变化时 no-op（不产生 `.bak`）。

### 裁定 4：面板申报（端点 23→26）

| 端点 | 分派 | covers | applies |
|---|---|---|---|
| `GET /api/brain/models` | BusCall `rr3-brain/model/list` | `[]` | `""` |
| `POST /api/brain/model` | BusCall `rr3-brain/model/set` | `[]` | `live` |
| `POST /api/brain/model/test` | BusCall `rr3-brain/model/test` | `[]` | `""` |

covers 空是**如实申报**：模型行不是 `assemble.toml` 槽表里的键，不进五路分母，不动门 d 基线。
前端新增"模型行"卡片（四预设 + 手填）、密钥卡加 `<datalist>` 预设名、原 ops 卡正名为
"通用 cloud 槽（ASR/TTS/超时/话轮）"。

## 登记（S 面，写进代码注释与测试）

1. **块外文本逐字保留，含表头前导注释**：它是小节说明，不该被 CRUD 抹掉；**块内行间注释**无处存放
   （模型行是面板对象），重写时丢弃——如实登记，不做注释保真。
2. **`ops/set` 的 llm 组与模型行是同一真值的两个写源**：模型行 active 在则写行、否则写 cloud 段；
   `ops/list` 的 llm 槽仍回 boot 投影（不回填 live 写），重启回新值——既有 B-14① 语义不变。
3. **`mode = "local"` 免鉴权**：请求不带 `Authorization` 头；`key_set` 恒 true（无密钥需求）。
4. **`api_format` 只认 `openai`**：词表留了位置，本拍不做第二格式——未知格式一律拒，不静默夹值。
5. **`test` 不写历史**：探测请求独立于对话历史，`max_tokens=1`，只判 2xx 与耗时；错误文案不带密钥。

## 后果

- 换厂商 = 面板填三格（API 根/模型/密钥引用）+ 保存；`config.toml` 立即可回滚（`.bak`）。
- 新增厂商（任何 OpenAI 兼容服务）= 手填完整根，零代码。
- `cloud.base_url` 与模型行 `base_url` 语义不同**仍是双轴**，但各有明确消费方与校验，不再互相猜。
- 遗留：`ops/set` 仍 live 不落盘（本拍不动它的 `persisted:false` 契约）；模型行注释保真只做到块外。

## 回写清单

- `modules/rr3-brain/src/config_store.rs`（新）、`cloud.rs`（`rr3_probe`/`rr3_key_set_of`）、
  `boot.rs`（root 归一 + 校验）、`rpc.rs`（CRUD 臂）、`dispatch.rs`（三臂 + exposes 6）、
  `module.toml`、`config.toml`（注释与完整根）；
- `contracts/topics.toml`（39→42，request 19→22）、`rr-registry` 两处 pin；
- `modules/rr3-interact/src/panel/server.rs`（23→26 + 三门单测）、`www/index.html`；
- `tools/sim/src/brain.rs`（演练档同语义）。
