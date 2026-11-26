# GitHub Actions 工作流（.github/workflows）

CI 单一事实源在 GitHub Actions（R30 裁定：atomgit 无流水线功能；R27 时代
`.gitcode/workflows/` 单真源路径作废，`.gitcode/` 已删除）。判据仍以
`tools/ci/*.sh` 为唯一真源，工作流只负责装工具、跑脚本、把失败压成注解。

## 作业组织（2026-11-17 结构治理）

- **每条流水线 ≤ 10 个作业**（含矩阵腿的执行实例口径）：现行 gate 7 /
  integration 7 / platforms 6（执行实例 9）/ publish 3 / buildroot 3 / coverage 1。
  新作业先论证归属流水线，塞不下就拆新流水线，不回流"一条大 ci.yml"。
- **职责域六分**：门禁（gate）／平台（platforms）／集成（integration）／
  发布（publish）／出厂轨（buildroot）／覆盖率（coverage），互不混装。
- **触发面按需**：gate 永远全量（规约门含文档门 ⑤，不设路径过滤）；
  integration 与 platforms 对纯文档改动跳过（`paths-ignore: docs/**`、
  `**.md`——集成/平台面对文档零敏感，文档变化由 gate 全量把守）；
  buildroot 按自身路径白名单触发；publish 只吃 tag / main。
- **缓存面口径**：`Swatinem/rust-cache` 一律显式 `workspaces:`（缺参即扫
  仓库根，本仓根无 `Cargo.toml`，等于空转）；凡跨多清单共写仓根
  target-dir（`/tmp/rr3-build`，见 `.cargo/config.toml`）的作业再加
  `cache-all-crates: "true"`，否则共享依赖每拍冷编。esp 工具链缓存在
  `.github/actions/esp-toolchain` 内三处，单点维护。apt 安装（归档偶发单包
  404/索引滞后）统一走 `.github/actions/apt-retry`（三次有界重试，末次经
  log-digest 压注解）——workflow 只给包名、标题与日志名。
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
| `gate.yml`（6） | conventions / release / supply / alsa / ota-bundle / scripts | 门禁主线（与本地 `tools/ci/check.sh` 同一入口）：规约全步（含 rustfmt/clippy、质量度量与仓面卫生）、release 档溢出检查测试、supply 供应链审计（cargo-deny --strict，周程定时复核公告）、ALSA 真后端编译门、OTA bundle 装配门、脚本卫生与基线棘轮审计、SKIP 纪律自检、门禁步骤清单审计。风格/lint 不单列 job：判据体只在 check.sh ①c/①d/④b，避免第二份 find/clippy 形状 |
| `platforms.yml`（8） | cross-artifacts ×3 / riscv-image / esp32-gates ×2 / autonomy-face / ipct003 / host-windows / host-macos | 跨平台与跨架构面：交叉编译矩阵、RISC-V 运行镜像构建门（交叉产物 → stage → QEMU 两段冒烟，不推送）、ESP32 专用门（含 autonomy/tts 双档构建腿 + 双芯片整片烧录镜像装配）、自治健康面真跑（30 s 模拟 + 复位闸门）、ipct003 跨架构对拍（--strict）、宿主等价面（Windows＋macOS；macOS 单腿含 workspace 与独立工程串行） |
| `integration.yml`（7） | distro-smoke / ota-sign / rk3566-onboard / buildroot-assembly / sim-assemble / sim-chain / matrix | 集成面：deb 装机冒烟、OTA 签名链、板面装配断言、装配与全链闭环演练（含 60s 稳态与阻塞面判据）、点验矩阵出数 |
| `coverage.yml`（2） | workspace / independent ×9 | 覆盖率观测面：周程/手动跑 `cargo llvm-cov`，workspace 与独立工程（六模块 + rrctl / ota-ctl / pack）分开出 summary，artifact 留存；非阻断，先积累趋势再决定 ratchet |

**数值回传面（续轮二十八；续轮四十三起加丢帧面）**：job 日志匿名不可读（403 已实证），
check-run annotation 是唯一无需凭据的取数通道。`sim-chain` 的稳态摘要（`since_max` /
`peak_max` / `wd_min` / `fd` / `wr` / `slow` / `log_max_us` / `boot_max`）以 notice
注解回传（失败拍以 error + 失败拍摘要回传）；读法 = `GET /repos/{owner}/{repo}/
check-runs/{id}/annotations`，无需 token；取数收成单点
`python3 tools/ci/audit/ci_steady.py [--head <sha>] [--assert]`（只读；`--assert` 按暂行
阈值逐键判据，缺键/越界退 3——积压丢帧/写超时/慢写必须为 0）。
| `publish.yml`（4） | build ×2 / build-riscv64 / merge / firmware | 发布面：原生架构双腿构建 + riscv64 交叉腿（QEMU）→ merge 去重（三架构）→ firmware 双芯片（release asset + ghcr OCI） |
| `buildroot.yml`（3） | dtb-backport / image / release | Buildroot 装配轨：交叉工具链与装配冒烟、出厂镜像轨 |

## 版本纪律与发布点执法（2026-11-25）

版本唯一真源是仓根 `VERSION`（现 `0.0.8`），它只能在标题为 `release(vX.Y.Z)` 的提交内变更；
tag 与它同名并打在该提交上。atomgit 是主仓，github 是镜像仓（只借流水线），
tag 先推 atomgit 再镜像 github。口径全文见 [conventions §7 版本纪律](conventions.md)。

执法点只有一处：`tools/ci/lib/release-tag.sh`——publish 的 build / build-riscv64 / merge
三步与 buildroot 的 release 腿都先调它取 `tag=` / `version=`；tag 与 `VERSION`
不同源、或 tag 未指向 release 提交即非 0 退出，不出制品。`workflow_dispatch`
与分支推送两条路径不做版本判定（它们不发 Release，行为不变）。自检
`release-tag.sh --self-test` 挂在 gate `scripts` job。

## 工具链

各工作流一律显式钉版 `toolchain: "1.100.0"`（`dtolnay/rust-toolchain` 的
with 入参），不依赖 runner 镜像预装版本；交叉 job 同时显式声明所需
`targets`。例外：`distro-smoke` 全程在 `rust:bookworm` 容器内构建，宿主侧
不编一行 Rust，故不装宿主工具链。本地开发者对应执行
`rustup toolchain install 1.100.0`（交叉目标按需 `rustup target add`）。

步序与每步判据见 `conventions.md` §2；脚本说明见
[`../../tools/ci/README.md`](../../tools/ci/README.md)。

本文件的**结构判据**由 [`tools/ci/audit/workflows_audit.py`](../../tools/ci/audit/workflows_audit.py)
在 gate `scripts` job 执法（工具链钉版 / 缓存参数 / 触发过滤 / 作业上限 / needs 图 /
并发语义 / 权限面 / 失败显形 / 五芯片矩阵腿 / 作业覆盖契约 / 发布面与正式制品托管），
政策变更须同批改脚本与自检夹具。**作业覆盖契约（2026-11-20 补）**：gate /
integration / platforms / publish / buildroot 的 job 名按 `REQUIRED_JOBS` 钉住——
删一个 job 就是某类验证静默停跑，必须同批评审改表，不得只靠记忆。
**YAML/表达式语法面（2026-11-24 补盲）**：`workflows_audit` 是文本扫描器，workflow
被改坏缩进或表达式类型时 GitHub 会直接不跑/整文件失败，而文本判据仍绿——gate
`scripts` job 因此增 `actionlint`（官方 schema 解析；关 shellcheck/pyflakes，只留
语法与上下文判据）。现行基线 **actionlint 1.7.7**（`gate.yml` 的
`ACTIONLINT_VER` 单点，官方 release + sha256 校验）；升级走显式提交改该 env。

**五芯片覆盖（2026-11-20）**：`platforms.yml` 的 `cross-artifacts` 必须含
英特尔/AMD `x86_64-unknown-linux-gnu`、瑞芯微 `aarch64-unknown-linux-musl`、
进迭时空 `riscv64gc-unknown-linux-gnu` 三条腿；`esp32-gates` 必须含 `esp32` 与
`esp32s3` 两腿。矩阵腿是"该芯片环境仍被验证"的唯一回执，删除/改名即红。
**brain 产物面（同日）**：有目标 C 工具链的腿（原生 x86_64、apt 交叉 riscv64）
带 `with-brain` 构建 `rr3-brain` 并单独上传；瑞芯微 musl 腿走 rust-lld 无目标 C
工具链，brain 的 ureq+rustls(ring) 归板上原生构建——覆盖差异由矩阵 `extra`
数据位表达，脚本机制只有一份。**riscv-image 腿（同日）**：在每次 push 上把
riscv64 发布镜像路径跑通（交叉产物 → `rr3-pack` stage → QEMU `docker build`
→ 文件面/运行面两段冒烟），不推送 ghcr；`publish.yml` 的 `build-riscv64` 腿
复用同一脚本，tag/main 时推送并把 riscv64 并入三架构清单（merge 判据同步）。
**发布面判据（2026-11-20 补）**：`build` 矩阵必须同时含 amd64/arm64 原生腿
（`ubuntu-24.04-arm`），且必须经 `stage-image.sh` 铺根与 `image-smoke.sh file|run`
两段冒烟；`build-riscv64` 必须复用 `build-riscv-image.sh --push` 并推送；`merge`
必须对合并清单再跑 `image-smoke.sh file`；`firmware` 的 S3 档须同时带目标三元组、
`MCU=esp32s3` 与 `esp32s3` feature——原生腿/发布路径/装机面验证/芯片档任一缩水即红。
**证明面（2026-11-24 补）**：三条镜像腿以 `docker buildx build --push` 构建并附
`--provenance=mode=max --sbom=true`，单架构清单各带 provenance + SBOM attestation
（`docker build` 直推不会产生）；窗口判据见 runbook B 的 attestation 计数。
**签名（2026-11-25 接入）**：`merge` 对合并 index digest、`firmware` 对固件 OCI
digest 执行 cosign keyless 签名（`id-token: write`，cosign 固定 v2.6.5），并在同一步
做正/负身份验证（错误 identity 必须被拒）；签名只对 `refs/tags/v*` 执行。身份约束
与验证命令见 runbook B.1；设计接入不等于已验证，正式 tag 回执前 status 仍记敞口。
S3 门的 ⑨ release 与 `firmware` job 逐字同参（`esp32s3,autonomy,tts`），并在产出树
断言同版 bootloader：tag 不是该形态的首次 release 编译。
**固件制品（同日；2026-11-22 补同版 bootloader）**：tag 发布的 `firmware` job
同批构建 classic `safe`（默认档）与 ESP32-S3 `full`（`esp32s3,autonomy,tts`）
release ELF，并从同一 IDF 5.2.2 构建树取两份 `bootloader.bin`——**ELF 不内嵌
bootloader/分区表**，espflash 未显式给 `--bootloader` 时用它自带的镜像（版本
与 app 不保证一致）。资产集 = 双 ELF + 双整片镜像 + 双 bootloader + `rr3-partitions.csv` +
`FLASHING.md` + sha256 清单；release asset 无凭据可下载，同时以 oras 推
`ghcr.io/<owner>/rockrobot3-firmware:<tag>`（不可变标签），五类芯片的正式发布
制品从此全部落在 ghcr。**资产集缺一即红**（`workflows_audit` 的 firmware 断言
与 `fail_on_unmatched_files` 双重拦截）。**制品托管审计（同日补）**：
`buildroot.yml` 的 `release` 腿必须 tag 门（`refs/tags/v`）、登录 ghcr、带
`packages: write`，并以 oras 推 `rockrobot3-ota` 与 `rockrobot3-buildroot`；
正式制品只发 release asset 不进 ghcr 即红。
**整片烧录镜像（2026-11-26 补）**：classic safe 与 S3 full 各出单文件整片镜像
（4 MB / 16 MB，bootloader + 分区表 + app，`write_flash 0x0` 直烧），由
`tools/deploy/image-esp32.sh` 装配并自检三点 magic 与整片尺寸；espflash 固定
4.6.0 官方预编译件 + sha256（`cargo install` 慢且版本漂移不可见）。同一脚本在
`platforms` 双芯片腿逐推执行（制品 `esp32-classic-merged-image` /
`esp32s3-merged-image`），发布面只多"进资产集 + oras"两步——装配参数漂移在
推送时就红，不等 tag 才发现。
**缓存键治理（2026-11-22 v0.0.16 实锤）**：`actions/cache` 的 key **禁止逗号**；
esp-toolchain 的 `targets` 允许逗号（espup 语法），归一必须在 action 内做
（逗号→短横），不得让调用方各自清洗。组合档键为
`esp-Linux-esp32-esp32s3-v0.17.1-idf5.2.2`；单 target 键字面不变、既有缓存不失效。

**运行时基线（2026-11-24 弃用面巡检）**：GitHub 已强制 Node 24 运行 action，
Node 20 档引用会在 annotations 里持续告警并将于后续移除。现行矩阵（真源 =
`tools/ci/audit/workflow_rules.py` 的 `ACTION_MATRIX`）：`actions/cache@v5`、
`docker/login-action@v4`、`docker/setup-buildx-action@v4`、
`softprops/action-gh-release@v3`（release 路径三件原本只在 tag 时执行，告警此前
未暴露）；`setup-buildx@v4` 移除的输入本仓未使用，升级无行为变化。升级动作
仍走"显式提交改矩阵 + 同批换引用"，与 rust-toolchain 钉版同一纪律。

**发布触发（需仓库凭据，本地不可代劳）**：正式发布 = 一次 tag 事件，两条前提：

1. 仓库 Secrets 配置 `RR3_OTA_SIGNING_KEY`（64 hex）——`buildroot` 发布腿缺它即
   fail-closed（设计如此，不降级为未签名）；
2. tag 必须与 `VERSION` 同源且指向 `release(vX.Y.Z)` 提交（`release-tag.sh` 执法）：

```bash
git commit --allow-empty -m 'release(v0.0.8)：<发布说明>'
git tag v0.0.8 && git push origin v0.0.8 && git push github v0.0.8
```

验证（匿名可读的部分）：`gh api repos/<owner>/rockrobot3/actions/runs` 看 publish /
buildroot 两条流水线；`oras manifest fetch ghcr.io/<owner>/rockrobot3-firmware:v0.0.8`
与 `rockrobot3-ota` / `rockrobot3-buildroot` 同标签 `v<VER>`（v0.0.8 首发时后两者
漏 `v` 前缀、以 `0.0.8` 名留存，此后统一）。不带凭据时可用 `workflow_dispatch`
输入 `edge` 只验运行镜像腿（不产 Release、不签名）。

**权限与失败显形（2026-11-18 续轮十六）**：

- 每个 workflow 必须有顶层 `permissions:`，至少 `contents: read`，不得
  `write-all`；publish 需要 `packages: write` 的按需声明，job 级额外授权
  （如 release 写）写在 job 上，不放大到整个 workflow。
- 任何 `continue-on-error` 非 `false` 即红：漂红巡检与门禁都读退出码，掩码
  等于把治理面掏空；需要"允许失败"的探针应显式落 `|| true` 并自行点名，不走
  `continue-on-error`。

**门禁的确定性执行入口始终是本地 `bash tools/ci/check.sh`**——合入前必须
本地跑绿，不依赖远端流水线。
