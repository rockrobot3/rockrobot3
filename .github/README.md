# .github —— GitHub Actions 工作流

| 文件 | 内容 |
|---|---|
| `workflows/ci.yml` | push 到 `main` / `dev/**` 与 PR 时执行三个独立 job：①`conventions`——`bash tools/ci/check.sh`（规约七步，步序与每步条数见 `docs/process/conventions.md` §2，脚本说明见 `tools/ci/README.md`）；②`lint`——rustfmt `--check` 与 clippy `-D warnings` 拆步（覆盖 base/runtime 与全部独立工程）；③`brain-unittest`——rr3-brain 26 条 stdlib unittest，ubuntu + macos 双矩阵 |

> **实况注记**：主远端已迁至 gitcode（`git@gitcode.com:rockrobot/rockrobot3.git`），
> 其 Actions 兼容运行器的实际触发情况属运行实况（见 `docs/status.md`）。
> **门禁的确定性执行入口始终是本地 `bash tools/ci/check.sh`**（约定见
> `docs/process/conventions.md`）。合入前必须本地跑绿，不依赖远端流水线。
