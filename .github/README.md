# .github —— GitHub Actions 工作流

| 文件 | 内容 |
|---|---|
| `workflows/ci.yml` | CI 单一事实源（R30 裁定，2026-10-11：atomgit 无流水线功能，CI 改走 GitHub Actions；R27 时代 `.gitcode/workflows/` 单真源路径作废，`.gitcode/` 已删除）。push 到 `main` / `dev/**` 与 PR 时执行九个独立 job：①`conventions`——`bash tools/ci/check.sh`（规约十步，步序与每步条数见 `docs/process/conventions.md` §2，脚本说明见 `tools/ci/README.md`）；②`lint`——rustfmt `--check` 与 clippy `-D warnings` 拆步（覆盖 base/runtime 与全部独立工程，esp 三工程 clippy 排除）；③`brain-unittest`——rr3-brain 26 条 stdlib unittest，ubuntu + macos 双矩阵；④`ipct003`——IPC-T-003 跨架构对拍 `--strict`（ubuntu + QEMU arm64 容器真跑，ADR-0006 后果项）；⑤`distro-smoke`——deb 装机冒烟（rust:bookworm 容器，T5 / ADR-0013 裁定 5）；⑥`buildroot-assembly`——post-build.sh 装配冒烟（纯 bash，保底轨修复批）；⑦`sim-assemble`——rr3-sim `--profile` 装配演练六模块真进程（D9）；⑧`sim-chain`——rr3-sim `--chain` 全链闭环演练（批F4，§5.2 判据 10 sim 档）；⑨`matrix`——点验矩阵出数 `tools/ci/matrix.sh`（architecture §12.6 七列，**非 `--strict`**、如实出数不冒领 PASS，产出人工搬进 `docs/status.md` 矩阵行；0.0.5 批M MC-10 L-1，此前该脚本无任何调用方） |

> **实况注记**：三远端口径——origin = atomgit（`git@atomgit.com:rockrobot/rockrobot3.git`）
> 与 gitcode（`git@gitcode.com:rockrobot/rockrobot3.git`）为推远端；GitHub Actions 流水线
> 待 GitHub 仓库建立 + SSH key 配置后推送触发首跑（回执核对与结项见 `docs/status.md`
> "本机无法收口"第 1 条）。九 job 首跑回执尚未取得，不以 SKIP 冒充绿。
> **门禁的确定性执行入口始终是本地 `bash tools/ci/check.sh`**（约定见
> `docs/process/conventions.md` §2）。合入前必须本地跑绿，不依赖远端流水线。
