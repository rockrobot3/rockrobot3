# .github —— GitHub Actions 工作流

| 文件 | 内容 |
|---|---|
| `workflows/ci.yml` | push 到 `main` / `dev/**` 与 PR 时执行七个独立 job：①`conventions`——`bash tools/ci/check.sh`（规约九步，步序与每步条数见 `docs/process/conventions.md` §2，脚本说明见 `tools/ci/README.md`）；②`lint`——rustfmt `--check` 与 clippy `-D warnings` 拆步（覆盖 base/runtime 与全部独立工程）；③`brain-unittest`——rr3-brain 26 条 stdlib unittest，ubuntu + macos 双矩阵；④`ipct003`——IPC-T-003 跨架构对拍 `--strict`（ubuntu + QEMU arm64 容器真跑，ADR-0006 后果项）；⑤`distro-smoke`——deb 装机冒烟（rust:bookworm 容器，T5 / ADR-0013 裁定 5）；⑥`buildroot-assembly`——post-build.sh 装配冒烟（纯 bash，保底轨修复批）；⑦`sim-assemble`——rr3-sim `--profile` 装配演练六模块真进程（D9） |

> **实况注记**：双远端推送（origin = atomgit，`git@atomgit.com:rockrobot/rockrobot3.git`；
> gitcode，`git@gitcode.com:rockrobot/rockrobot3.git`），其运行器对 GitHub Actions 工作流的
> 实际触发情况属运行实况（见 `docs/status.md`"本机无法收口"第 1 条）。
> **门禁的确定性执行入口始终是本地 `bash tools/ci/check.sh`**（约定见
> `docs/process/conventions.md` §2）。合入前必须本地跑绿，不依赖远端流水线。
