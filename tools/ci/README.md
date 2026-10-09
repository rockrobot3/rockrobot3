# ci —— `check.sh`：规约检查的唯一入口

本地与 CI 跑**同一条命令**，避免"CI 绿本地红"的两套逻辑：

    bash tools/ci/check.sh

必须从仓库根或任意子目录执行——脚本自己 `git rev-parse --show-toplevel` 定位仓库根，
不在 git 仓库内则直接拒绝执行。

| 步 | 命令 | 覆盖 |
|---|---|---|
| ① | `cargo check --all-targets --workspace`（在 `base/runtime`） | 编译，含 `#[cfg(test)]` 目标 |
| ①b | `cargo test --workspace` | 全量测试，兜住未被点名的守护用例 |
| ② | `cargo test -p rr-registry --test contract` | manifest schema 校验 + 注册表形状 pin |
| ②b | `cargo test --manifest-path tools/mkmodule/Cargo.toml` | mkmod 产物即契约（独立包，①b 覆盖不到） |
| ③ | `cargo test -p rr-registry topics_prefix` | 主题前缀归属（② 那个测试目标里的筛选子集） |
| ④ | 逐工程 `cargo check -q --all-targets --manifest-path <dir>/Cargo.toml`（`modules/` + `tools/` + `base/ota/`，显式排除 `base/runtime/*`） | 仓内**所有独立 Rust 工程**编译：它们各自成 workspace，①/①b 打不到 |
| ⑤ | 遍历 `git -c core.quotepath=false ls-files '*.md'`（排除 `legacy/`），逐个判 `[ -s ]`，取 markdown 链接括号里的相对路径目标判其是否存在；再把 `docs/README.md` 的链接归一成仓根路径，对每个已跟踪 README 查它在导航里有没有一条链接 | 已跟踪 md **不得为空**、相对链接**不得断**、README **不得漏进 `docs/README.md`**：三类违规逐条打印（`空文档` / `断链` / `导航漏链`）后整步 FAIL。`legacy/` 是冻结区，其内 README 允许残留 1 处已知旧路径（记录见 `docs/status.md`），纳入门禁只会逼人或改冻结区或加豁免名单，两者都不是目的；同一理由，冻结区的 README 也不被要求进导航。md 列表与 `docs/README.md` 的链接列表**打空即判门禁失效、不放行**（与 ④ 同口径） |
| ⑥ | 遍历 `modules tools base` 下各独立 Rust 工程 `cargo test -q`（find 口径与 ④ 全同，含 esp 两工程排除；brain 是 Python 无 Cargo.toml 自然跳过，其 tests/ 由 CI brain-unittest job 覆盖） | architecture §13 **格格双达标·接线 CI**（0.0.5 批E 立步，收口批 E' 扩全量）：全部独立工程的 tests 必须真跑在门禁里——批E 深挖发现 tools/sim 的 #[test] 处于"④ 只 check 不跑"的死角，扩口径封死；0 测试工程在此红，≥1 通过断言与 ①b 同口径 |
| ⑦ | 四件静态扫描（详见脚本 ⑦ 段注释）：⑦-a 机制核目录（rr-core 全部 + rr-ipc protocol/ + transport/mod.rs）剥行注释后 grep 厂商/产品/芯片/器件禁词与 `if.*(vendor\|product)`；⑦-b 遍历 `modules/*/composite.toml` 的 `[assemble].atoms` 逐成员判 `atoms/<名>/atom.toml` 存在；⑦-c 逐原子件数五件套（brain 为 Python 两件式登记豁免）+ 逐组合件判入口/实现实体；⑦-d `wc -l` 断言全部装配入口 ≤80 行 | architecture §13 **静态执法子集**（0.0.5 批E）：零策略载荷 / `F⊗F` 非法+禁薄胶水存在性 / 五件套齐全 / 入口体量，全 **fail-closed**、逐条点名后整步 FAIL。"每格一件·不留空格"反向断言在 ② 步 rr-registry 仓扫描测试（查册真值处执法，不抄第二份）；契约冻结在 ①b/②、层界单向属链接图断言随主承载层实施批接线 |

**每步跑多少条测试不在本文件登记**：唯一落点是
[`docs/process/conventions.md`](../../docs/process/conventions.md) §2（含两平台条数为何不等的
逐条解释）。本文件只写"跑什么命令、为什么这么跑"。

两处设计约束（改动前请读脚本头部注释）：

1. **① 必须带 `--all-targets`**：普通 `cargo check` 不编译测试目标，测试代码里的类型错误
   会一路绿灯（实测注入 E0308 后裸 check 退 0）。
2. **测试步不止看退出码**：`cargo` 对打空的过滤器仍以 0 退出（fail-open）。所以每步还须
   在输出里同时出现行首的 `running N tests`（N≥1）与 `test result: ok. N passed`（N≥1）
   才判 PASS，封死"过滤器笔误/测试集改名"造成的静默放行。

CI 单一事实源在 [`.gitcode/workflows/ci.yml`](../../.gitcode/workflows/ci.yml)
（0.0.5 增补轮四 批W2，R27 裁定：atomgit 原生引擎只拾取 `.gitcode/workflows/`，
批A' 实证；`.github/workflows/ci.yml` 仅存指路存根），本脚本才是实际门禁。
**合入前必须本地跑绿**，退出码非 0 即不合入。
