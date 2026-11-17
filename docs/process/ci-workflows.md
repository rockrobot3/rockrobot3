# GitHub Actions 工作流（.github/workflows）

CI 单一事实源在 GitHub Actions（R30 裁定：atomgit 无流水线功能；R27 时代
`.gitcode/workflows/` 单真源路径作废，`.gitcode/` 已删除）。push 到 `main` /
`dev/**` 与 PR 时触发；供应链公告随时间漂移，另设周程定时复核。判据仍以
`tools/ci/*.sh` 为唯一真源，工作流只负责装工具、跑脚本、把失败压成注解。

| 工作流 | 职责 |
|---|---|
| `gate.yml` | 门禁主线（与本地 `tools/ci/check.sh` 同一入口）：规约全步（含质量度量与仓面卫生）、lint（rustfmt --check ＋ clippy -D warnings，仓内全工程）、supply 供应链审计（cargo-deny --strict）、ALSA 真后端编译门、OTA bundle 装配门；周程定时复核供应链公告 |
| `platforms.yml` | 跨平台与跨架构面：cross 交叉编译矩阵、ESP32 专用门、ipct003 跨架构对拍（--strict）、host-windows / host-macos |
| `integration.yml` | 集成面：distro-smoke 装机冒烟、matrix 点验矩阵出数（调 `tools/ci/matrix.sh`） |
| `publish.yml` | 发布面：镜像矩阵与 firmware 打包 |
| `buildroot.yml` | Buildroot 装配轨：交叉工具链与装配冒烟 |

## 工具链

各工作流一律显式钉版 `toolchain: "1.100.0"`（`dtolnay/rust-toolchain` 的
with 入参），不依赖 runner 镜像预装版本；交叉 job 同时显式声明所需
`targets`。本地开发者对应执行 `rustup toolchain install 1.100.0`（交叉目标
按需 `rustup target add`）。

步序与每步判据见 `conventions.md` §2；脚本说明见
[`../../tools/ci/README.md`](../../tools/ci/README.md)。

**门禁的确定性执行入口始终是本地 `bash tools/ci/check.sh`**——合入前必须
本地跑绿，不依赖远端流水线。
