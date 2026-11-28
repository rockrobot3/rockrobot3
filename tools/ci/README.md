# ci —— `check.sh`：规约检查的唯一入口

本地与 CI 跑**同一条命令**，避免"CI 绿本地红"的两套逻辑：

    bash tools/ci/check.sh

必须从仓库根或任意子目录执行——脚本自己 `git rev-parse --show-toplevel` 定位仓库根，
不在 git 仓库内则直接拒绝执行。

**目录地图（2026-11-25 分层）**：根目录只留入口与本文，其余按职责归四层——
新件按这张表落位，不再往根目录平铺。

| 目录 | 放什么 | 件 |
|---|---|---|
| 根 | 入口 | `check.sh`（规约门）、`ci-status.sh`（漂红巡检）、本文 |
| `steps/` | `check.sh` 的步骤体，由入口显式 source | `<门号>.sh` |
| `lib/` | 被 ≥3 处共用的公共件 | `skip.sh`、`cargo-artifact.sh`、`cargo-net.sh`、`log-digest.sh`、`release-tag.sh`、`runtime-members.sh` |
| `audit/` | 静态审计与自检（Python，同目录平铺 import） | `metrics*.py`、`workflows_audit*.py`、`workflow_*.py`、`baselines_audit.py`、`gate_steps_audit.py`、`lockio_audit.py`、`unsafe_audit.py`、`ci_steady.py` |
| `baseline/` | 棘轮基线数据（只降不升） | `gate-*-baseline.txt`、`gate_steps.txt`、`metrics-baseline.json`、`unsafe-baseline.txt` |
| `platform/` | 工作流直调的平台腿、esp 专用门与自检 | `cross-*.sh`、`sim-*.sh`、`*-check.sh`、`matrix.sh`、`docs_gate_selftest.sh` 等 |

搬动规则：脚本文件名是工作流审计的判据锚（`workflow_rules.py` 的 `JOB_STEPS` / `CHIP_MATRIX`
按名钉住），改名须同批评审；基线内容不按路径登记，搬动不改数值。

**步骤体拆分（2026-11-20）**：`check.sh` 保留 `step` 标题与调用序；各步实现按族放
`tools/ci/steps/<id>.sh`，由入口显式 source。`gate_steps_audit.py` 仍解析入口的
`step` 行，步骤清单与顺序不因搬文件而失锚。当前已拆：`4b`（独立工程 clippy）、
`5`（文档门）、`6`（测试接线）、`7`（§13 静态断言五门）、`8`（件级判据三门）、
`9d`/`9e`（生成物一致性 / 行程真值单点）、`9y`（跨平台可编面）、`9a`（角色覆盖）、
`9n`/`9c`/`9r`/`9z`（四个静态门）、`9w`（依赖面禁令）、`10`（门 d 面板覆盖率）、
`13`（仓面卫生门）、`14`（装载门体量基线）、`16`（源文件行数门）。

| 步 | 命令 | 覆盖 |
|---|---|---|
| ① | `cargo check --all-targets --workspace`（在 `base/runtime`） | 编译，含 `#[cfg(test)]` 目标 |
| ①b | `cargo test --workspace` | 全量测试，兜住未被点名的守护用例 |
| ② | `cargo test -p rr-registry --test contract` | manifest schema 校验 + 注册表形状 pin |
| ②b | `cargo test --manifest-path tools/mkmodule/Cargo.toml` | mkmod 产物即契约（独立包，①b 覆盖不到） |
| ③ | `cargo test -p rr-registry topics_prefix` | 主题前缀归属（② 那个测试目标里的筛选子集） |
| ④ | 逐工程 `cargo check -q --all-targets --manifest-path <dir>/Cargo.toml`（`modules/` + `tools/` + `base/ota/`，显式排除 `base/runtime/*`） | 仓内**所有独立 Rust 工程**编译：它们各自成 workspace，①/①b 打不到 |
| ⑤ | 遍历 `git -c core.quotepath=false ls-files '*.md'`（全集进门，无排除），逐个判 `[ -s ]`，取 markdown 链接括号里的相对路径目标判其是否存在；再把 `docs/README.md` 的链接归一成仓根路径，对每个已跟踪 README 查它在导航里有没有一条链接；第四判据查 docs/ 之外的 md 是否全为 `README.md`（`base/os/` 与仓根 `README_zh.md` 豁免），第五判据查 docs/ 根是否只留 `README.md` / `status.md` / `status-history.md` | 已跟踪 md **不得为空**、相对链接**不得断**、README **不得漏进 `docs/README.md`**、docs/ 之外与 docs/ 根**不得散落**专题文档（2026-11-25 批46 / 2026-11-27 批59）：四类违规逐条打印（`空文档` / `断链` / `导航漏链` / `散落文档`）后整步 FAIL；md 列表与 `docs/README.md` 的链接列表**打空即判门禁失效、不放行**（与 ④ 同口径） |
| ⑥ | 遍历 `modules tools base` 下各独立 Rust 工程 `cargo test -q`（find 口径与 ④ 全同，含 esp 两工程排除；brain 是 Python 无 Cargo.toml 自然跳过，其 tests/ 由 CI brain-unittest job 覆盖） | architecture §13 **格格双达标·接线 CI**（0.0.5 批E 立步，收口批 E' 扩全量）：全部独立工程的 tests 必须真跑在门禁里——批E 深挖发现 tools/sim 的 #[test] 处于"④ 只 check 不跑"的死角，扩口径封死；0 测试工程在此红，≥1 通过断言与 ①b 同口径 |
| ⑦ | 五件静态扫描（详见脚本 ⑦ 段注释）：⑦-a 机制核与件面目录（rr-mech 全部（0.0.5 批M 起，MC-10 L-4 补）+ rr-core 全部 + rr-hal-api + rr-ipc protocol/ + transport/mod.rs + **`modules/*/atoms/*/*.rs`**，批K H2 起）剥行注释后 grep 厂商/产品/芯片/器件禁词与 `if.*(vendor\|product)`；⑦-b 遍历 `modules/*/composite.toml` 的 `[assemble].atoms` 逐成员判 `atoms/<名>/atom.toml` 存在；⑦-c 逐原子件按 module.toml `lang` 数据位选件形（rust 五件套 / python 两件式）+ 逐组合件判入口/实现实体；⑦-d `wc -l` 断言全部装配入口 ≤80 行（入口路径亦按 `lang` 推导——零件名零硬编码，0.0.5 批M MC-10 L-2）；⑦-e 门禁清单自指判等（⑦-a 扫面名集 A 逐名须在 conventions §2 与本文 ⑦ 行齐 + `rr-core-xtensa-check.sh` 的 `-p` 目标集 ⊆ A——代码唯一真值、文档向代码看齐，批M MC-10 L-4） | architecture §13 **静态执法子集**（0.0.5 批E）：零策略载荷 / `F⊗F` 非法+禁薄胶水存在性 / 五件套齐全 / 入口体量 / 门禁清单自指判等，全 **fail-closed**、逐条点名后整步 FAIL。"每格一件·不留空格"反向断言在 ② 步 rr-registry 仓扫描测试（查册真值处执法，不抄第二份）；契约冻结在 ①b/②、层界单向属链接图断言随主承载层实施批接线 |
| ⑧ | 三门（详见脚本 ⑧ 段注释）：⑧-a 两腿对拍计数门（格两轴读 `contracts/slots.toml`：`status` 判分母 + `tier_scope` 记档位适用并校验非空/档名在册 join atom.toml provides——normal 格须 `fn parity_leg1_*` + `fn parity_leg2_*` 各 ≥1，placeholder 记不适用、retired 退出分母；件形按 module.toml `lang` 判，lang=python 两件式记不适用，不再用 `[ ! -f mech.rs ]` 存在性启发式；C10 stub 反例自证内置且须由**对拍计数**拦下，批M MC-10 L-2）；⑧-b 依赖闭包 grep 门（`modules/*/atoms/*/*.rs` 非 test 面，行注释与 `#[cfg(test)]` 块先剥——L0 = `std::`（含 `use std::`/`extern crate std`/全限定内联），L1 = L0 + `serde_json`）；⑧-c 注入面校验门（rr-registry contract injection 用例**显式过滤器**，打空即红——形状级含 **L-5 空面规则**：`handles`/`ops` 两数组均空须明写 `no_injection = true`） | architecture §4.0.3 判据② + §13 表（0.0.5 批K H2）：件级对拍可机器计数（只有腿一记未达标）/ 件面依赖分级执法（L0/L1 两级）/ 注入面永远有真值单点在门里，全 **fail-closed**、逐条点名后整步 FAIL |
| ⑨-d | `steps/9d.sh`（`cd tools/rr3-codegen && cargo run --locked -q -- --check`） | 生成物一致性门（批N1 n1-8 建成，全案 §8 门 f；第十三批扩探针布局）：三份生成物同一判据——真值点 `contracts/{topics,face-expressions}.toml` → `tools/esp32-periph-base/src/topics_gen.rs`、`hardware/esp32-mini/axis/*.toml` → `axis_gen.rs`、`contracts/envelope.schema.json` → `tools/pi-periph-probe/envelope_gen.py`；重跑生成器 diff 为空，漂移即 FAIL 并点名重生成命令 |
| ⑨-w | 三判据静态扫描（详见脚本 ⑨-w 段注释） | 依赖面禁令（基-35，0.0.5 增补轮六第 0.5 步建门，全 **fail-closed**、枚举打空即门禁失效）：① 逐单元 `modules/*/Cargo.toml` 必须直依 `rr-hal-api`——契约符号的依赖面在 Cargo 清单可证，不经 rr-hal re-export 隐性传递；② `modules/**` 全部 `.rs` 的 use 面（行注释先剥）`rr_hal::` 引用仅许**冻结白名单四类**（`backend::*` 装配选择函数／`sim` 承载／`provision_store` 第 8 能力面 trait 住实现 crate／`Rr3PcmStream` std 侧承载类型），白名单外（含 crate 根花括号多导 `rr_hal::{`——多导可把契约符号与白名单混进一行，扫描面不可分词）逐行点名即 FAIL；③ 板侧 `tools/esp32-periph-base/Cargo.toml` 禁实现 crate `rr-hal`/`rr-bus`/`rrd`（`名 =` 键形状匹配，不误伤 `rr-hal-api`；architecture §14 全量口径；`tools/sim` 是 CI/无头开发承载工具，豁免）。窄化两敞口（装配上移／`rr-bus` 契约面化）随收口行入 [status.md](../../docs/status.md) 敞口表 |
| ⑨-n | 判据静态扫描（详见脚本 ⑨-n 段注释） | 注入路径唯一门 n 静态面执法位（n1-11，全案 §8 门 n / 施工图 Task 8 Step 2）：键名单自代码常量 `RR3_PIN_DERIVED_KEYS`（`rrd/src/profile.rs`）awk 导出——单真值点不抄第二份，键集扩缩自动跟随；扫面前件 fail-closed 四查（声明数非空 / 实解析数非零 / 两数判等 / profiles 份数非零，任一不成立即"门禁失效，不放行"）；逐 `profiles/*.toml` 扫 `[hardware]` 段（行注释先剥）出现物理键任一即逐行点名（file:line）后 FAIL——稳态＝物理键一律 `[pins]` 派生，`[hardware]` 只留非引脚键 `alsa_device`/`link_ifname`（批N3 件六 SPI 词形收口：`spi_device` 已进派生键集、扫面 8 键，敞口 18 行已结）；装载期判据实体与反例用例在 `rrd/src/profile.rs` 门 n 段（经步 ①b 进 CI） |

| ⑩ | `steps/10.sh`（读 contracts 快照与分母基线） | 面板覆盖率对账（门 d）：分子 = covers 申报 + applies 生效路径，分母 = 字段级五路并集；红账 0、分母下限 83，缩分母即红 |
| ⑫ | `python3 tools/ci/audit/metrics.py`（真值 `metrics-baseline.json`） | 质量度量门：重复率 ≤3%、CC 均值 ≤2.50 / 最大 ≤11、生产函数 >80 行 =0、>60 行 =0、>50 行 ≤4（目标档棘轮，只降不升）、生产 panic ≤60、>20B 生产函数名 0、≥1000ms 长睡 0；基线只降不升 |
| ⑬ | `steps/13.sh`（git grep + git status） | 仓面卫生门：追踪文件无本机绝对路径、无构建目录、源码类新件必须入册、shell `$name` 后接非 ASCII 即红 |
| ⑭ | `steps/14.sh` | 装载门体量基线门（裁-13/基-38）：装配单锚函数的块行数、`read_to_string` 样板数、rr-registry 同形函数数按 `gate-size-baseline.txt` 只降不升；锚函数缺失或基线形状坏即红 |
| ⑮ | `python3 tools/ci/audit/lockio_audit.py`（先 `--self-test`） | 锁内 I/O 门：`lock·log` 同行 / 条件位临时 guard 延寿到块体 / 直调会写待落盘日志的 registry 方法；扫面打空即门禁失效；gate `scripts` 同源执法 |
| ⑯ | `steps/16.sh` | 源文件行数门：每个 `.rs/.py/.sh` <500 行（铁律，2026-11-22 收紧）；按 `gate-lines-baseline.txt` 只降不升 |
| ⑰ | `python3 tools/ci/audit/release_sets.py`（自检在 gate `scripts`） | 发布/部署面清单对拍：仓内真值（`modules/rr3-*/module.toml`）/ 发布策略（`PUBLISH_MODULES` + publish.yml 构建循环）/ 镜像（stage-image 两处装机 + image-smoke 起齐与阈值）/ 板侧派生（pack-bundle、pack-arm64 走 `runtime-members` 且留 brain 例外）必须同一集合；缺/多逐项点名 |

**基线只降不升审计（`baselines_audit.py`，gate `scripts` job）**：把 `tools/ci/baseline/*-baseline.*`
与 base ref 的同名文件逐键比对——已存在的键上调即红；新键/缺键告警；未登记基线文件即红。
比对点取 `merge-base(HEAD, base)`，避免把 base 分支自身的前进误判成回涨。

**SKIP 纪律（`skip.sh`，gate `scripts` job）**：esp 专用门的工具链缺位判据统一走
`rr3_skip`——本地开发机缺工具链 = SKIP（exit 0），CI 内缺工具链 = FAIL（exit 1）。
理由：这些工程被 `check.sh` ④/⑥ 显式排除，专用门是唯一编译执法位；门一 SKIP，
它们就变成"零覆盖但流水线绿"。

**发布点执法（`release-tag.sh`，gate `scripts` job 自检）**：publish / buildroot 的发布腿
经它取 `tag=` / `version=`——tag 必须与仓根 `VERSION` 同源且指向 `release(vX.Y.Z)` 提交，
否则非 0 退出。版本纪律全文见 [conventions §7](../../docs/process/conventions.md)。

**门禁步骤清单（`gate_steps_audit.py`，gate `scripts` job）**：把 `check.sh` 的
`step`/`printf` 出口解析成有序清单，与 `gate_steps.txt` 逐字比对——删除、新增、
改名、换序任一发生即红；并断言门号唯一（同一 token 两次即双义红，子号 `①b`/`⑨-a`
各有独立身份）。它是"删掉一个 step 不会红"与"一号两门"两个盲区的机器判据。

**⑨-a…⑨-z 各门**（门 a/c/r/y/z 等，后续批次续增）：判据以 `check.sh` 各 `step`
段注释为唯一真值，本表不逐行抄录——抄一份就多一处漂移面。

**CI 稳态取数（`ci_steady.py`，只读，无需 token）**：`python3 tools/ci/audit/ci_steady.py
[--head <sha>] [--repo owner/name]` 按 head 找 `sim-chain` check-run、读 annotations
打出一行稳态摘要（since / 续命峰值 / 看门狗下限 / 写超时 / 慢写 / 落盘峰值 / `boot_max`）。
job 日志匿名 403，注解是唯一无需凭据的取数通道；**取不到数值即非 0 退出**——不拿
"没查到"当"健康"。

**每步跑多少条测试不在本文件登记**：唯一落点是
[`docs/process/conventions.md`](../../docs/process/conventions.md) §2（含两平台条数为何不等的
逐条解释）。本文件只写"跑什么命令、为什么这么跑"。

两处设计约束（改动前请读脚本头部注释）：

1. **① 必须带 `--all-targets`**：普通 `cargo check` 不编译测试目标，测试代码里的类型错误
   会一路绿灯（实测注入 E0308 后裸 check 退 0）。
2. **测试步不止看退出码**：`cargo` 对打空的过滤器仍以 0 退出（fail-open）。所以每步还须
   在输出里同时出现行首的 `running N tests`（N≥1）与 `test result: ok. N passed`（N≥1）
   才判 PASS，封死"过滤器笔误/测试集改名"造成的静默放行。

CI 单一事实源在 [`.github/workflows/`](../../.github/workflows)（gate / platforms / integration / publish 四条）
（R30 裁定，2026-10-11：atomgit 无流水线功能，CI 改走 GitHub Actions；R27 时代
`.gitcode/workflows/` 路径作废，`.gitcode/` 已删除），本脚本才是实际门禁。
**合入前必须本地跑绿**，退出码非 0 即不合入。

## 供应链审计入口：`supply-audit.sh`

[supply-audit.sh](platform/supply-audit.sh) 不发散策略，只枚举工程根并逐项调用
`cargo deny check --locked`：工程根 = `base/runtime` 工作区 + `modules/`、`tools/`、
`base/ota/` 下各自带锁文件的独立工程（工作区成员不重复执法）。判据唯一真源是
[`.cargo/deny.toml`](../../.cargo/deny.toml)——许可证显式白名单、RustSec 公告、来源白名单、通配依赖。
本机未装 cargo-deny 时 SKIP 并明说；CI 以 `--strict` 执法，工具缺位即红（不由 SKIP
冒充通过）。

## CI 漂红巡检入口：`ci-status.sh`

[ci-status.sh](ci-status.sh) 把"每拍核对漂红"固化成一条只读命令：取目标分支最近
10 次 run，按 workflow 名取最新一条，判据在 [lib/ci_verdict.py](lib/ci_verdict.py)
（`--self-test` 四例不触网）：任一最新一条 `failure` / `timed_out` /
`action_required` / `stale` 退出 1；任一最新一条仍在跑、或本地头没有任何 run 退出 2
（未定不冒充绿——纯文档提交被 paths-ignore 跳过时，上一头的平台腿可能还在跑）；
全 success 退出 0。`cancelled` 不算红（同 ref 新推送由并发组取消，由下一拍补覆盖）。
匿名可读 runs 列表；job 日志需鉴权（403 已实证），脚本不碰。`GITHUB_TOKEN` 可选，
仅用于提高 API 限额。

## 发布面齐套巡检入口：`lib/release-verify.sh`

[release-verify.sh](lib/release-verify.sh) 是发版后的**总账**（2026-11-27 批178）：
一条只读命令按 tag 核对五类 ghcr 制品——运行镜像三架构与 in-toto 证明面、固件九件、
distro 两件与 artifactType、OTA 六件（含 `keys/*.pub`）、buildroot 四件；缺件/不合规
逐项点名并打印 digest，不发写请求。`--classes` 供两条发布流水线分账（publish 核
image/firmware/distro，buildroot 核 ota/buildroot），默认五类给操作者做发版总检。
退出码：`0` 所选类别齐套 / `1` 缺件或不合规（远端明确 `not found` 也算缺件）/
`2` 用法错误或取数失败（401/403/网络——"看不到"不等于"没有"，按未定处理，不与缺件
混成同一结论）。`--with-signature` 追加 cosign keyless 验签（镜像总账/固件/distro，
身份约束与 runbook B.1 同源，缺 cosign 即退 2，不放宽）；platforms 周程腿已启用它，
公开签名匿名可验。`--tag auto` 取 registry 上
最新的正式 semver tag——周程用它，避免"VERSION 已 bump、tag 未推"的发布前窗口常红。
结尾输出"制品 digest 汇总表"（逐类 ref + digest），发版回执（runbook B.2）直接引用。
`--deep` 追加 distro 深检：从 registry 拉回 tar 再过 A10 契约（按 `any` 跳 commit 锚，
tag 真伪由签名背书），层名对不等于内容可装。
`--self-test` 用假 oras/manifest/cosign 夹具跑十八例（含类别过滤、缺件/未定分账、
签名、auto 取 tag、digest 表、深检三例、拼错即拒），在 gate 的 scripts 步实跑；
公开包可匿名回读，私有包需 ghcr 凭据（runbook B）。

## 构建失败归因入口：`log-digest.sh`

[log-digest.sh](lib/log-digest.sh) 是 CI 失败日志的**单点归因**（2026-11-17 立）：
信号提取（error/fatal/failed/…）→ 噪音过滤（`Error 1 (ignored)`、makeinfo 版本
提示、`pyerrors.h` 等不指向失败的命中）→ 单行 `::error` 注解 → 末段回显。
buildroot 轨的 `stage-build`/`br-defconfig`/`br-uboot`/`br-build`/`ota-build`
与 gate 轨的 `check.sh`/`supply`/`clippy`/`alsa`/`apt-alsa` 都用它，workflow
只传标题、日志路径与信号面（策略），正则不再逐 step 复制。
判据自检由 `--self-test` 给出（正例 + 缺日志/截断/回显/取首条四条负例），gate
流水线 `scripts` job 执法；自检走 `--plain`，不产生假注解。gate 的
check.sh / supply / clippy / alsa / ota-build 各步也用同一入口，`--head` 用于
"命令级错误在前、逐项 FAIL 在后"的日志（供给审计）。

## unsafe 注释审计入口：`unsafe_audit.py`

[unsafe_audit.py](audit/unsafe_audit.py) 静态扫描 `base/`、`modules/`、`tools/` 的生产面
`.rs`（跳过测试路径），要求每个 `unsafe` 关键字上方有 `SAFETY` 前言（同一行或紧邻
注释段，到空行为止；`unsafe fn` 的 `/// # Safety` 调用方契约同权）。允许条数住
[unsafe-baseline.txt](baseline/unsafe-baseline.txt)，**只降不升**：新增未注释 unsafe 即红。
基线现为 **0**（Linux 目标面、esp32 板侧与 macaudio 开发面均已补前言）；gate 的
`scripts` job 先跑 `--self-test`（生产/测试/extern 三类扫描面各一条夹具）再执法。
`--report` 是**只读扩面报告**：单列测试路径未注释 unsafe 与生产面 `extern "C"`
声明缺 SAFETY 的清单，不计基线、不改退出码——扩面观察不制造假红，清理对象进下一批。
首份报告 6 处声明块（bridge `conn.rs`、rrd `ctl.rs`、pi `ports.rs`×2、ipc `excl.rs`、
macaudio `ffi.rs`）已逐块核对：调用点 SAFETY 全覆盖（生产面审计 0 即证），结论＝
**豁免**（声明面契约住调用点）；报告保留为信息项，新增声明块即须复核。

## 流水线结构审计入口：`workflows_audit.py`

[workflows_audit.py](audit/workflows_audit.py) 把 CI 自身的结构面纳入门禁（2026-11-17 立）：
每个 workflow 必须有顶层 `on:` 与 `concurrency:`、每个 job 必须有 `timeout-minutes:`、
第三方 `uses:` 必须命中 `ACTION_MATRIX`（`checkout@v7`、`upload-artifact@v7`、
`download-artifact@v8`、`cache@v5`、`setup-qemu@v4` 等）；本地 action（`./`）不查。
规则核按职责分三件（2026-11-24）：[workflow_common.py](audit/workflow_common.py) 文本切分、
[workflow_release.py](audit/workflow_release.py) 发布面/正式制品面、[workflow_rules.py](audit/workflow_rules.py)
结构面与入口；切法单点，两处规则不各写一套扫描。
**版本升级 = 显式改矩阵**，同一工作流两版 action 并存即红（首查抓到 ipct003 的
qemu v3 残留并修复）。**触发与依赖图**：`push:` 必须收窄到 `branches`/`tags`
（全分支推送会白烧 CI），`publish.yml` 必须由 tag 触发；job `needs` 不得指向
不存在的 job、不得成环。**新结构政策**（真源 = [ci-workflows.md](../../docs/process/ci-workflows.md)）：
`dtolnay/rust-toolchain` 必须 `with.toolchain: "1.100.0"`；`Swatinem/rust-cache`
必须带 `workspaces`，跨清单构建作业另须 `cache-all-crates: "true"`；每条 workflow
job 定义 ≤10；integration/platforms 的 push/PR 必须 `paths-ignore` 含 `docs/**`
与 `**.md`；publish 必须 `cancel-in-progress: false`。**五芯片覆盖**：platforms.yml
的 `cross-artifacts` 必须含英特尔/AMD `x86_64-unknown-linux-gnu`、瑞芯微
`aarch64-unknown-linux-musl`、进迭时空 `riscv64gc-unknown-linux-gnu` 三条腿，
`esp32-gates` 必须含 `esp32`/`esp32s3` 两腿；每腿的验证脚本（`periph-base-check.sh`
等）也按名钉住——腿在而步骤被删即"名存实亡"红；`riscv-image` 构建门（腿 +
`build-riscv-image.sh`）同样钉住。**权限与失败显形**：顶层
`permissions:` 必须存在、含 `contents: read`、不得 `write-all`；任何
`continue-on-error` 非 `false` 即红（掩码会把漂红巡检变成假绿）。`--self-test` 给
好例 + 缺并发/缺超时/版本漂移/needs 悬空/needs 成环/push 无范围/工具链未钉版/
缓存缺参/paths-ignore 缺失/publish 可取消/job 超限/continue-on-error/缺 permissions/
芯片矩阵缩水/芯片 job 缺失/芯片验证步骤缺失/riscv 镜像腿缺失/发布面缺腿/发布矩阵缺 arm64/
发布镜像冒烟缺失/门禁 job 缩水/固件缺 S3/制品面缺名二十三条负例，gate 的 `scripts` job
先自检后执法。
**作业覆盖契约**：gate / integration / platforms / publish / buildroot / coverage /
release-anon 的 job 名全部按 `REQUIRED_JOBS` 钉住（删名即红、整文件删也红，改名须同批
评审）；coverage 两腿的**覆盖率入口**按名钉住（`cargo llvm-cov` 被换掉 = 数据静默
消失，upload-artifact 默认只在缺文件时告警）；gate 主线各 job 的**判据入口**也按名钉住（conventions→`check.sh`、
supply→`supply-audit.sh --strict`、ota-bundle→`ota-bundle.sh`、scripts→四个审计、
release/alsa 的 cargo 判据——入口被换掉即红，整套门禁不得空转）；集成腿的**验证脚本**按名钉住（distro-smoke / install-smoke / ota-sign /
rk3566-onboard / buildroot-assembly / sim-assemble / sim-chain——step 换成 echo 即红）；`release-anon` 的**命令**也按判据钉住（必须复用 `release-verify.sh`、带
`--with-signature`、`--tag auto`、`--classes image,firmware`——只删类别或改固定 tag
等于公开面覆盖静默消失）；发布面完整性：`publish.yml` 必须有 build / build-riscv64 / merge / firmware /
distro-bundle 五条产出腿与 release-verify 总账腿；build 矩阵必须
含 amd64/arm64 原生腿与 `ubuntu-24.04-arm`；build 必须走 `stage-image.sh` 与
`image-smoke.sh file|run`（发布镜像按装机面验证）；build-riscv64 必须复用
`build-riscv-image.sh` 并推送；merge 必须等两条构建腿、合并命令含三架构单镜像并
对合并清单跑 `image-smoke.sh file`；
firmware 必须带 `packages: write`、推 `rockrobot3-firmware`，且 S3 目标
（`xtensa-esp32s3-espidf` + `MCU=esp32s3` + `esp32s3` feature）与双 ELF 资产按名
钉住——双芯片发布面不得缩成单芯片，发布路径不得分叉。**制品托管**：`buildroot.yml` 的 `release` 腿必须 tag 门、
登录 ghcr、带 `packages: write`，并以 oras 推 `rockrobot3-ota` 与
`rockrobot3-buildroot`——正式制品只发 release asset 不算托管完成；两包标签口径必须
`v<VER>`（v0.0.8 首发曾漏 v 推成 `0.0.8`，现由审计当场拦）。

## 发布/部署面清单对拍：`release_sets.py`

[release_sets.py](audit/release_sets.py)（2026-11-28 批250 立）只核一件事：模块清单
在四处写法里是否同一集合——仓内真值 `modules/rr3-*/module.toml`
（[runtime-members.sh](lib/runtime-members.sh) 派生）、发布策略
（`workflow_release.PUBLISH_MODULES` 与 `publish.yml` 构建循环）、镜像面
（`stage-image.sh` 两处装机循环、`image-smoke.sh` 起齐循环与计数阈值）、板侧包
（`pack-bundle.sh` / `pack-arm64.sh` 必须派生并保留 brain 例外，出现字面量清单即红）；
并核真值面成对——`modules/rr3-*` 的 `Cargo.toml` 与 `module.toml` 缺一即点名。
判据是**集合相等**而非包含：新增模块不入选发布集必须显式改策略，不许默默漏件。
`python3 tools/ci/audit/release_sets.py [--root DIR]`；`--self-test` 给 1 正例 + 8 负例。
门位：check.sh ⑰（本地与 gate `conventions` 同源执法），自检与即时执法另在 gate
`scripts` job 跑一遍（不编译，秒级），失败归因不再混进度量门。

## 平台覆盖矩阵（2026-11-17 首立，按 workflow 事实源抄录）

"每个芯片平台都在流水线里验证"的对照表（**结构真源** =
[`docs/process/ci-workflows.md`](../../docs/process/ci-workflows.md)；本表只映射"哪个平台
被哪条腿覆盖"）。**编译腿**只证明目标 ISA 能构建，**执行腿**才证明产物在目标环境
跑得起来；两者都缺的格子必须如实留空。

| 平台 | 编译/构建腿 | 目标环境执行 | 契约字节面 | 发布制品 | 缺口（不冒领） |
|---|---|---|---|---|---|
| 英特尔 / AMD（x86_64） | gate + integration 全量；`cross` x86_64 | 原生直跑 `rrd` / `rr3-ctl` | `ipct003_dump` golden | `publish` linux/amd64 镜像 | 工控真板未上（A 轨） |
| 瑞芯微 RK3566/RK3588（aarch64） | `cross` aarch64-musl 七件；`buildroot` 全量镜像（aarch64） | alpine arm64 容器真跑 | 容器内 `ipct003_dump` golden | buildroot 镜像 artifact；`publish` linux/arm64 | 实烧/串口 shell 不在 CI |
| 进迭时空 K1/K3（riscv64） | `cross` riscv64-gnu 七件；`riscv-image` QEMU 构建门 | `qemu-riscv64` 真跑 | qemu 内 golden 对拍（第 5 路编译期 const 已接） | `publish` linux/riscv64（并入三架构清单） | 真板环境缺（上板归板批次） |
| ESP32（classic） | `esp32-gates`：periph-base / uart-echo / espidf-std / sentinel / act-sense | **无运行腿**（xtensa 无 qemu，运行归真板） | 编译期 const + `ipct003` 第④路 | tag release 固件 ELF（`firmware` job） | 真板运行证据不在 CI |
| ESP32-S3 | `esp32-gates`：periph-base S3 / act-sense S3 | **无运行腿** | xtensa 编译期 | tag release 固件 ELF（`firmware` 双芯片 + ghcr） | 真板运行证据不在 CI |
| 宿主 Windows x86_64 | `host-windows`：workspace check + test；独立工程中性子集（`mkmodule` / `rr3-codegen` / `rrctl` / `rr3-pack`） | 原生 | 由测试面覆盖 | — | 依赖 UDS/termios/HAL 实现面的独立工程按平台缺省排除（理由在缺口④） |
| 宿主 macOS | `host-macos`：workspace 全量测试 + 独立工程测试（单腿串行，续轮二十五合并——托管 macOS 执行位紧张时第二腿会被取消并染红整个 run） | 原生 | 由测试面覆盖 | — | esp 条件工程按门禁口径排除 |

**已知缺口两条**（下一轮补腿对象，不冒充已覆盖）：① 三块真板（esp32 / rock3 /
spacemit）的运行证据不属 CI 能力，归板上批次；② Windows 独立工程门**部分收口**
（续轮十六起，续轮十八扩面）：
已覆盖 4 个可跨平台编译的工程（`tools/mkmodule` / `tools/rr3-codegen` /
`tools/rrctl` / `tools/rr3-pack`，Unix 专属动作各自 `cfg` 门控；rrctl 的 UDS
查询在非 Unix 如实返回"仅 Unix 可用"）；
`tools/rr3-sentinel-harness` 依赖 `rr_ipc::transport::uart`（仅 linux/macos 编译，
raw termios），`base/ota/rr3-ota-ctl` 依赖 `rr-ota::bootenv`（模块实现按 `cfg(unix)`
收口，CLI 子命令尚无非 Unix 分支），`modules/*`、`tools/sim`、`tools/rr3-bridge`
等依赖 UDS 或 HAL 实现面，按平台缺省不纳入 Windows 腿——纳入前先要它们各自具备
非 Unix 编译面。

## 点验矩阵出数入口：`matrix.sh`

[matrix.sh](platform/matrix.sh) 不改判据、只出数（0.0.5 批K H3/C12/C11）：按 architecture §12.6
七列（①字节契约 ②机制核编译面 ③供给自证 ④描述符装载 ⑤安全核生效 ⑥生命周期
⑦时序达标）把"本环境实跑了什么"记成一行矩阵，每行带环境指纹三件套（载体/镜像
+ `rustc -Vv` + 目标三元组）。`--strict` 口径下 SKIP/无载体计未跑、不放行（退 2）；
人工实测走 `--manual <列号> --evidence <仓内证据文件>` 回填（证据三样：环境指纹/
采样窗口与样本数/原始数据，缺一判证据不足；手测行不覆盖自动 FAIL 格）。
判据唯一真源仍是本目录各 `*.sh` 脚本，矩阵只是出数入口。

**CI 接线（0.0.5 批M MC-10 L-1）**：本脚本此前**无任何调用方**（不在 `check.sh`、不在任何工作流里），
0.0.5 批M 起由 `.github/workflows/integration.yml` 的 `matrix` job（ubuntu-latest）调用——**非 `--strict`**
（⑦ 时序达标无固定载体，默认口径如实记"未跑"、退 0；出现 FAIL 列才退 1），产出**人工搬进**
`docs/status.md` 矩阵行。出数 ≠ 判据：判据真源仍 `check.sh` 全步 + 本目录各专用门。
