# rock3 开发规约（v0.0.1 起生效）

来源：`docs/dev/0.0.1版本洛克三号开发方案.md` §5。本文只收录**可机械检查**或与
评审判定直接相关的条目；架构语义以 `docs/architecture/architecture.md` 与 ADR 为准。

## 1 分支与提交

- `main` 受保护；日常在 `dev/*`。功能 `feat/<topic>`、修复 `fix/<topic>`、文档 `docs/<topic>`
- Conventional Commits：`feat|fix|docs|chore|refactor|security`（scope 用 crate/目录名，如 `feat(rr-ipc)`）
- 涉及架构决策的提交 body **必须**引用 ADR 编号（`引用：ADR-0006`）
- 一次提交只做一件事；`git mv` 类纯移动不得混入内容改动

## 2 机器可检查项（CI 五步，见 `tools/ci/check.sh`）

| 步 | 检查 | 命令 |
|---|---|---|
| ① | workspace 编译（含测试目标） | `cargo check --all-targets --workspace`（CWD = `base/runtime`） |
| ①b | workspace 全量测试 | `cargo test --workspace`（CWD = `base/runtime`） |
| ② | manifest schema + 注册表形状 pin | `cargo test -p rr-registry --test contract` |
| ②b | mkmod 输出即契约用例（方案 P3） | `cargo test --manifest-path tools/mkmodule/Cargo.toml` |
| ③ | 主题前缀归属 | `cargo test -p rr-registry topics_prefix` |

入口脚本的标题写作"规约三检查"（沿用方案 D7 的三条语义命名），实际执行的是**五步**：
②b 是 ② 的姊妹步，把生成器的产物喂回同一份 manifest 契约，防 mkmod 与 CI 漂移；
①b 把 workspace 的 56 条测试全部跑一遍，②/③ 只是其中被点名两批的定点诊断。
测试步（①b/②/②b/③）除退出码外还断言"至少一个测试真实通过"——cargo 对打空的过滤器仍以 0
退出，只看退出码会在过滤器笔误或测试集改名时静默放行。

测试面如实登记：①b **56** 条（`base/runtime` workspace 全量，含 rr-ipc 的 17 个 envelope 契约
守护——13 条偏移/枚举对齐 + `tests/envelope_meta.rs` 的 4 条**读 JSON** 的元数据守护
（`x-byte-size` / `x-offset-sum` / `payload_len.minimum|maximum` / `x-schema-version`）；
`contracts/README.md` 称那批用例是契约的执行入口，若 ① 只是普通 `cargo check`，
它们连编译都不做，更谈不上执行）；② **22** 条（含 `registry_shape_is_pinned` 的注册表形状
硬 pin：7 owner / 21 topic / kind 7-8-3-3 / 21 条三元组）、③ **5** 条（均为 ①b 的子集）、
②b **5** 条（`tools/mkmodule` 是独立包，不在 workspace 内，故 ①b 覆盖不到它）。
① 用 `--all-targets` 而非 `cargo check`：普通 check 不编译 `#[cfg(test)]` 目标，
实测在测试代码里注入一个类型错误，`cargo check --workspace` 照样退 0。
M2 起随运行时语义落地再扩点名范围。

其余规约（命名、分层、职责边界）靠评审；命名唯一权威是 [naming.md](naming.md)。

## 3 结构纪律

- 新模块一律由生成器产出：`cargo run --manifest-path tools/mkmodule/Cargo.toml --bin rr3-mkmod -- rr3-<名>`
  （模块名形如 `rr3-arm`，第二参数是输出根，默认 `./modules`）。生成器落盘**五个文件**：
  `module.toml`、`config.toml`、`Cargo.toml`、`src/main.rs`、`README.md`——[naming.md](naming.md) §4
  所称"四件套"指四类结构角色（manifest / 运行参数 / 源码 / 说明），`Cargo.toml` 属于源码那件的
  crate 半边。五件齐备且 manifest 过契约校验才可合入 `modules/`；校验不过则整体不落盘，
  不留半成品模块
- 新增/变更主题必须先改 `contracts/topics.toml`（改契约 = 走 review），再在 manifest 里引用
- `contracts/` 只增不改；破坏性变更升 `contracts/envelope.schema.json` 的顶层元数据键
  `x-schema-version`（Rust 侧同名常量 `RR3_CONTRACT_SCHEMA_VERSION`，由 `cargo test -p rr-ipc
  --test envelope_meta` 逐值钉住）并在 `contracts/README.md` 的"演进登记"留痕；
  `contracts/topics.toml` 与 `module.schema.toml` 用的是另一个键名 `schema_version`，别混写
- 模块 crate 之间**不得**互相依赖，只能经 `rr-ipc`/`rr-bus`（一票否决项）
- 密钥、token 不进仓库：构建期环境变量注入（`SILICONFLOW_API_KEY`，见 `legacy/core/README.md`）
- 反射环（1k–10kHz 闭环）留在模块进程内，永不进总线（ADR-0007 裁定 1）

## 4 Definition of Done

- [ ] `bash tools/ci/check.sh` 本地 exit=0
- [ ] 涉及 `contracts/` 的变更附带演进说明（只增不改 / major 升级）
- [ ] 涉及架构语义的变更已更新或新增 ADR
- [ ] 新模块由 rr3-mkmod 生成并通过 schema 校验
- [ ] 无新增 warning（`cargo check` 干净，不留 `#[allow]` 兜底）

## 5 文档纪律

- 状态、进度写 ADR/方案，不写散落的周报式文档
- `docs/architecture/architecture.md` 是唯一架构图；目录树变更必须同步其 §3
