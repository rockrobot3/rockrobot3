# .github —— GitHub Actions 工作流

| 文件 | 内容 |
|---|---|
| `workflows/ci.yml` | push 到 `main` / `dev/**` 与 PR 时执行 `bash tools/ci/check.sh`（规约六步，步序与每步条数见 `docs/process/conventions.md` §2，脚本说明见 `tools/ci/README.md`） |

> **实况注记**：本仓库的主远端是 atomgit，其 Actions 运行器从不执行——`ci.yml` 只是
> GitHub 侧的镜像声明，**门禁的实际执行入口始终是本地 `bash tools/ci/check.sh`**
> （约定见 `docs/process/conventions.md`）。合入前必须本地跑绿，不依赖远端流水线。
