# GitHub Actions 工作流（.github/workflows）

CI 单一事实源在 GitHub Actions（R30 裁定：atomgit 无流水线功能；R27 时代
`.gitcode/workflows/` 单真源路径作废，`.gitcode/` 已删除）。判据仍以
`tools/ci/*.sh` 为唯一真源，工作流只负责装工具、跑脚本、把失败压成注解。

## 作业组织（2026-11-17 结构治理）

- **每条流水线 ≤ 10 个作业**（含矩阵腿的执行实例口径）：现行 gate 6 /
  integration 7 / platforms 6（执行实例 9）/ publish 3 / buildroot 3。
  新作业先论证归属流水线，塞不下就拆新流水线，不回流"一条大 ci.yml"。
- **职责域五分**：门禁（gate）／平台（platforms）／集成（integration）／
  发布（publish）／出厂轨（buildroot），互不混装。
- **触发面按需**：gate 永远全量（规约门含文档门 ⑤，不设路径过滤）；
  integration 与 platforms 对纯文档改动跳过（`paths-ignore: docs/**`、
  `**.md`——集成/平台面对文档零敏感，文档变化由 gate 全量把守）；
  buildroot 按自身路径白名单触发；publish 只吃 tag / main。
- **缓存面口径**：`Swatinem/rust-cache` 一律显式 `workspaces:`（缺参即扫
  仓库根，本仓根无 `Cargo.toml`，等于空转）；凡跨多清单共写仓根
  target-dir（`/tmp/rr3-build`，见 `.cargo/config.toml`）的作业再加
  `cache-all-crates: "true"`，否则共享依赖每拍冷编。esp 工具链缓存在
  `.github/actions/esp-toolchain` 内三处，单点维护。
- **矩阵并行**：跨平台按芯片/架构拆腿（cross ×3、esp 按芯片 ×2、publish
  原生架构 ×2），fail-fast: false 保证单腿红不掩盖其余腿结论；host-macos
  **合并为单腿**（workspace + 独立工程串行，续轮二十五）——托管 macOS arm64
  执行位长期紧张，第二腿反复以 "The job was not acquired by Runner of type
  hosted" 被取消并把整个 run 判红；一个 job 只发一次执行位请求，覆盖不变，
  代价是该腿墙钟变长（超时上调到 75min）。
- **并发组**：五条流水线顶层 concurrency 同 ref 互踩即取消；publish 例外
  （cancel-in-progress: false，发布不可中断）。
- **死重不留**：构建不经过宿主侧的作业不装宿主工具链、不挂缓存
  （如 distro-smoke 全程在容器内）；大体量外部源码包按版本钉 key 缓存
  （如 buildroot dtb 门的内核 tarball），解包与补丁每拍重做保确定性。

| 工作流 | 作业 | 职责 |
|---|---|---|
| `gate.yml`（6） | conventions / lint / supply / alsa / ota-bundle / scripts | 门禁主线（与本地 `tools/ci/check.sh` 同一入口）：规约全步（含质量度量与仓面卫生）、rustfmt＋clippy 全工程、supply 供应链审计（cargo-deny --strict，周程定时复核公告）、ALSA 真后端编译门、OTA bundle 装配门、脚本卫生三自检 |
| `platforms.yml`（6） | cross-artifacts ×3 / esp32-gates ×2 / ipct003 / host-windows / host-macos | 跨平台与跨架构面：交叉编译矩阵、ESP32 专用门、ipct003 跨架构对拍（--strict）、宿主等价面（Windows＋macOS；macOS 单腿含 workspace 与独立工程串行） |
| `integration.yml`（7） | distro-smoke / ota-sign / rk3566-onboard / buildroot-assembly / sim-assemble / sim-chain / matrix | 集成面：deb 装机冒烟、OTA 签名链、板面装配断言、装配与全链闭环演练（含 60s 稳态与阻塞面判据）、点验矩阵出数 |

**数值回传面（续轮二十八；续轮四十三起加丢帧面）**：job 日志匿名不可读（403 已实证），
check-run annotation 是唯一无需凭据的取数通道。`sim-chain` 的稳态摘要（`since_max` /
`peak_max` / `wd_min` / `fd` / `wr` / `slow` / `log_max_us` / `boot_max`）以 notice
注解回传（失败拍以 error + 失败拍摘要回传）；读法 = `GET /repos/{owner}/{repo}/
check-runs/{id}/annotations`，无需 token；取数收成单点
`python3 tools/ci/ci_steady.py [--head <sha>] [--assert]`（只读；`--assert` 按暂行
阈值逐键判据，缺键/越界退 3——积压丢帧/写超时/慢写必须为 0）。
| `publish.yml`（3） | build ×2 / merge / firmware | 发布面：原生架构双腿构建 → merge 去重 → firmware 打包 |
| `buildroot.yml`（3） | dtb-backport / image / release | Buildroot 装配轨：交叉工具链与装配冒烟、出厂镜像轨 |

## 工具链

各工作流一律显式钉版 `toolchain: "1.100.0"`（`dtolnay/rust-toolchain` 的
with 入参），不依赖 runner 镜像预装版本；交叉 job 同时显式声明所需
`targets`。例外：`distro-smoke` 全程在 `rust:bookworm` 容器内构建，宿主侧
不编一行 Rust，故不装宿主工具链。本地开发者对应执行
`rustup toolchain install 1.100.0`（交叉目标按需 `rustup target add`）。

步序与每步判据见 `conventions.md` §2；脚本说明见
[`../../tools/ci/README.md`](../../tools/ci/README.md)。

本文件的**结构判据**由 [`tools/ci/workflows_audit.py`](../../tools/ci/workflows_audit.py)
在 gate `scripts` job 执法（工具链钉版 / 缓存参数 / 触发过滤 / 作业上限 / needs 图 /
并发语义 / 权限面 / 失败显形），政策变更须同批改脚本与自检夹具。

**权限与失败显形（2026-11-18 续轮十六）**：

- 每个 workflow 必须有顶层 `permissions:`，至少 `contents: read`，不得
  `write-all`；publish 需要 `packages: write` 的按需声明，job 级额外授权
  （如 release 写）写在 job 上，不放大到整个 workflow。
- 任何 `continue-on-error` 非 `false` 即红：漂红巡检与门禁都读退出码，掩码
  等于把治理面掏空；需要"允许失败"的探针应显式落 `|| true` 并自行点名，不走
  `continue-on-error`。

**门禁的确定性执行入口始终是本地 `bash tools/ci/check.sh`**——合入前必须
本地跑绿，不依赖远端流水线。
