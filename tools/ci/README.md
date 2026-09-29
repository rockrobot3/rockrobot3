# ci —— `check.sh`：规约检查的唯一入口

本地与 CI 跑**同一条命令**，避免"CI 绿本地红"的两套逻辑：

    bash tools/ci/check.sh

必须从仓库根或任意子目录执行——脚本自己 `git rev-parse --show-toplevel` 定位仓库根，
不在 git 仓库内则直接拒绝执行。

| 步 | 命令 | 覆盖 |
|---|---|---|
| ① | `cargo check --all-targets --workspace`（在 `base/runtime`） | 编译，含 `#[cfg(test)]` 目标 |
| ①b | `cargo test --workspace` | 全量测试（2026-09-29 实测 56 条），兜住未被点名的守护用例 |
| ② | `cargo test -p rr-registry --test contract` | manifest schema 校验 + 注册表形状 pin（22 条） |
| ②b | `cargo test --manifest-path tools/mkmodule/Cargo.toml` | mkmod 产物即契约（5 条） |
| ③ | `cargo test -p rr-registry topics_prefix` | 主题前缀归属（② 那 22 条的子集） |

两处设计约束（改动前请读脚本头部注释）：

1. **① 必须带 `--all-targets`**：普通 `cargo check` 不编译测试目标，测试代码里的类型错误
   会一路绿灯（实测注入 E0308 后裸 check 退 0）。
2. **测试步不止看退出码**：`cargo` 对打空的过滤器仍以 0 退出（fail-open）。所以每步还须
   在输出里同时出现行首的 `running N tests`（N≥1）与 `test result: ok. N passed`（N≥1）
   才判 PASS，封死"过滤器笔误/测试集改名"造成的静默放行。

`.github/workflows/ci.yml` 只是 GitHub 侧镜像声明（主远端 atomgit 不执行 Actions），
本脚本才是实际门禁。**合入前必须本地跑绿**，退出码非 0 即不合入。
