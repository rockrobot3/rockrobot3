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
| ⑤ | 遍历 `git -c core.quotepath=false ls-files '*.md'`（全集进门，无排除），逐个判 `[ -s ]`，取 markdown 链接括号里的相对路径目标判其是否存在；再把 `docs/README.md` 的链接归一成仓根路径，对每个已跟踪 README 查它在导航里有没有一条链接 | 已跟踪 md **不得为空**、相对链接**不得断**、README **不得漏进 `docs/README.md`**：三类违规逐条打印（`空文档` / `断链` / `导航漏链`）后整步 FAIL；md 列表与 `docs/README.md` 的链接列表**打空即判门禁失效、不放行**（与 ④ 同口径） |
| ⑥ | 遍历 `modules tools base` 下各独立 Rust 工程 `cargo test -q`（find 口径与 ④ 全同，含 esp 两工程排除；brain 是 Python 无 Cargo.toml 自然跳过，其 tests/ 由 CI brain-unittest job 覆盖） | architecture §13 **格格双达标·接线 CI**（0.0.5 批E 立步，收口批 E' 扩全量）：全部独立工程的 tests 必须真跑在门禁里——批E 深挖发现 tools/sim 的 #[test] 处于"④ 只 check 不跑"的死角，扩口径封死；0 测试工程在此红，≥1 通过断言与 ①b 同口径 |
| ⑦ | 五件静态扫描（详见脚本 ⑦ 段注释）：⑦-a 机制核与件面目录（rr-mech 全部（0.0.5 批M 起，MC-10 L-4 补）+ rr-core 全部 + rr-hal-api + rr-ipc protocol/ + transport/mod.rs + **`modules/*/atoms/*/*.rs`**，批K H2 起）剥行注释后 grep 厂商/产品/芯片/器件禁词与 `if.*(vendor\|product)`；⑦-b 遍历 `modules/*/composite.toml` 的 `[assemble].atoms` 逐成员判 `atoms/<名>/atom.toml` 存在；⑦-c 逐原子件按 module.toml `lang` 数据位选件形（rust 五件套 / python 两件式）+ 逐组合件判入口/实现实体；⑦-d `wc -l` 断言全部装配入口 ≤80 行（入口路径亦按 `lang` 推导——零件名零硬编码，0.0.5 批M MC-10 L-2）；⑦-e 门禁清单自指判等（⑦-a 扫面名集 A 逐名须在 conventions §2 与本文 ⑦ 行齐 + `rr-core-xtensa-check.sh` 的 `-p` 目标集 ⊆ A——代码唯一真值、文档向代码看齐，批M MC-10 L-4） | architecture §13 **静态执法子集**（0.0.5 批E）：零策略载荷 / `F⊗F` 非法+禁薄胶水存在性 / 五件套齐全 / 入口体量 / 门禁清单自指判等，全 **fail-closed**、逐条点名后整步 FAIL。"每格一件·不留空格"反向断言在 ② 步 rr-registry 仓扫描测试（查册真值处执法，不抄第二份）；契约冻结在 ①b/②、层界单向属链接图断言随主承载层实施批接线 |
| ⑧ | 三门（详见脚本 ⑧ 段注释）：⑧-a 两腿对拍计数门（格两轴读 `contracts/slots.toml`：`status` 判分母 + `tier_scope` 记档位适用并校验非空/档名在册 join atom.toml provides——normal 格须 `fn parity_leg1_*` + `fn parity_leg2_*` 各 ≥1，placeholder 记不适用、retired 退出分母；件形按 module.toml `lang` 判，lang=python 两件式记不适用，不再用 `[ ! -f mech.rs ]` 存在性启发式；C10 stub 反例自证内置且须由**对拍计数**拦下，批M MC-10 L-2）；⑧-b 依赖闭包 grep 门（`modules/*/atoms/*/*.rs` 非 test 面，行注释与 `#[cfg(test)]` 块先剥——L0 = `std::`（含 `use std::`/`extern crate std`/全限定内联），L1 = L0 + `serde_json`）；⑧-c 注入面校验门（rr-registry contract injection 用例**显式过滤器**，打空即红——形状级含 **L-5 空面规则**：`handles`/`ops` 两数组均空须明写 `no_injection = true`） | architecture §4.0.3 判据② + §13 表（0.0.5 批K H2）：件级对拍可机器计数（只有腿一记未达标）/ 件面依赖分级执法（L0/L1 两级）/ 注入面永远有真值单点在门里，全 **fail-closed**、逐条点名后整步 FAIL |
| ⑨-d | `cd tools/rr3-codegen && cargo run -q -- --check` | 生成物一致性门（批N1 n1-8 建成，全案 §8 门 f）：真值点 `contracts/topics.toml` + `contracts/face-expressions.toml` → 板侧常量表 `tools/esp32-periph-base/src/topics_gen.rs`，重跑生成器 diff 为空；漂移即 FAIL 并点名重生成命令 |
| ⑨-w | 三判据静态扫描（详见脚本 ⑨-w 段注释） | 依赖面禁令（基-35，0.0.5 增补轮六第 0.5 步建门，全 **fail-closed**、枚举打空即门禁失效）：① 逐单元 `modules/*/Cargo.toml` 必须直依 `rr-hal-api`——契约符号的依赖面在 Cargo 清单可证，不经 rr-hal re-export 隐性传递；② `modules/**` 全部 `.rs` 的 use 面（行注释先剥）`rr_hal::` 引用仅许**冻结白名单四类**（`backend::*` 装配选择函数／`sim` 承载／`provision_store` 第 8 能力面 trait 住实现 crate／`Rr3PcmStream` std 侧承载类型），白名单外（含 crate 根花括号多导 `rr_hal::{`——多导可把契约符号与白名单混进一行，扫描面不可分词）逐行点名即 FAIL；③ 板侧 `tools/esp32-periph-base/Cargo.toml` 禁实现 crate `rr-hal`/`rr-bus`/`rrd`（`名 =` 键形状匹配，不误伤 `rr-hal-api`；architecture §14 全量口径；`tools/sim` 是 CI/无头开发承载工具，豁免）。窄化两敞口（装配上移／`rr-bus` 契约面化）随收口行入 [status.md](../../docs/status.md) 敞口表 |
| ⑨-n | 判据静态扫描（详见脚本 ⑨-n 段注释） | 注入路径唯一门 n 静态面执法位（n1-11，全案 §8 门 n / 施工图 Task 8 Step 2）：键名单自代码常量 `RR3_PIN_DERIVED_KEYS`（`rrd/src/profile.rs`）awk 导出——单真值点不抄第二份，键集扩缩自动跟随；扫面前件 fail-closed 四查（声明数非空 / 实解析数非零 / 两数判等 / profiles 份数非零，任一不成立即"门禁失效，不放行"）；逐 `profiles/*.toml` 扫 `[hardware]` 段（行注释先剥）出现物理键任一即逐行点名（file:line）后 FAIL——稳态＝物理键一律 `[pins]` 派生，`[hardware]` 只留非引脚键 `alsa_device`/`link_ifname`（批N3 件六 SPI 词形收口：`spi_device` 已进派生键集、扫面 8 键，敞口 18 行已结）；装载期判据实体与反例用例在 `rrd/src/profile.rs` 门 n 段（经步 ①b 进 CI） |

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

[supply-audit.sh](supply-audit.sh) 不发散策略，只枚举工程根并逐项调用
`cargo deny check --locked`：工程根 = `base/runtime` 工作区 + `modules/`、`tools/`、
`base/ota/` 下各自带锁文件的独立工程（工作区成员不重复执法）。判据唯一真源是
[`.cargo/deny.toml`](../../.cargo/deny.toml)——许可证显式白名单、RustSec 公告、来源白名单、通配依赖。
本机未装 cargo-deny 时 SKIP 并明说；CI 以 `--strict` 执法，工具缺位即红（不由 SKIP
冒充通过）。

## CI 漂红巡检入口：`ci-status.sh`

[ci-status.sh](ci-status.sh) 把"每拍核对漂红"固化成一条只读命令：取目标分支最近
40 次 run，按 workflow 名取最新一条，打印结论并按**本地头**判定——与本地头同提交
的 run 有 `failure` / `timed_out` / `action_required` / `stale` 即退出 1，仍在跑
退出 2，全 success 退出 0。`cancelled` 不算红（同 ref 新推送由并发组取消，由下一拍
补覆盖）。匿名可读 runs 列表；job 日志需鉴权（403 已实证），脚本不碰。
`GITHUB_TOKEN` 可选，仅用于提高 API 限额。

## 构建失败归因入口：`log-digest.sh`

[log-digest.sh](log-digest.sh) 是 CI 失败日志的**单点归因**（2026-11-17 立）：
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

[unsafe_audit.py](unsafe_audit.py) 静态扫描 `base/`、`modules/`、`tools/` 的生产面
`.rs`（跳过测试路径），要求每个 `unsafe` 关键字上方有 `SAFETY` 前言（同一行或紧邻
注释段，到空行为止；`unsafe fn` 的 `/// # Safety` 调用方契约同权）。允许条数住
[unsafe-baseline.txt](unsafe-baseline.txt)，**只降不升**：新增未注释 unsafe 即红。
基线现为 **0**（Linux 目标面、esp32 板侧与 macaudio 开发面均已补前言）；gate 的
`scripts` job 先跑 `--self-test`（生产/测试/extern 三类扫描面各一条夹具）再执法。
`--report` 是**只读扩面报告**：单列测试路径未注释 unsafe 与生产面 `extern "C"`
声明缺 SAFETY 的清单，不计基线、不改退出码——扩面观察不制造假红，清理对象进下一批。
首份报告 6 处声明块（bridge `conn.rs`、rrd `ctl.rs`、pi `ports.rs`×2、ipc `excl.rs`、
macaudio `ffi.rs`）已逐块核对：调用点 SAFETY 全覆盖（生产面审计 0 即证），结论＝
**豁免**（声明面契约住调用点）；报告保留为信息项，新增声明块即须复核。

## 流水线结构审计入口：`workflows_audit.py`

[workflows_audit.py](workflows_audit.py) 把 CI 自身的结构面纳入门禁（2026-11-17 立）：
每个 workflow 必须有顶层 `on:` 与 `concurrency:`、每个 job 必须有 `timeout-minutes:`、
第三方 `uses:` 必须命中 `ACTION_MATRIX`（`checkout@v7`、`upload-artifact@v7`、
`download-artifact@v8`、`cache@v4`、`setup-qemu@v4` 等）；本地 action（`./`）不查。
**版本升级 = 显式改矩阵**，同一工作流两版 action 并存即红（首查抓到 ipct003 的
qemu v3 残留并修复）。**触发与依赖图**：`push:` 必须收窄到 `branches`/`tags`
（全分支推送会白烧 CI），`publish.yml` 必须由 tag 触发；job `needs` 不得指向
不存在的 job、不得成环。**新结构政策**（真源 = [ci-workflows.md](../../docs/process/ci-workflows.md)）：
`dtolnay/rust-toolchain` 必须 `with.toolchain: "1.100.0"`；`Swatinem/rust-cache`
必须带 `workspaces`，跨清单构建作业另须 `cache-all-crates: "true"`；每条 workflow
job 定义 ≤10；integration/platforms 的 push/PR 必须 `paths-ignore` 含 `docs/**`
与 `**.md`；publish 必须 `cancel-in-progress: false`。**权限与失败显形**：顶层
`permissions:` 必须存在、含 `contents: read`、不得 `write-all`；任何
`continue-on-error` 非 `false` 即红（掩码会把漂红巡检变成假绿）。`--self-test` 给
好例 + 缺并发/缺超时/版本漂移/needs 悬空/needs 成环/push 无范围/工具链未钉版/
缓存缺参/paths-ignore 缺失/publish 可取消/job 超限/continue-on-error/缺 permissions
十三条负例，gate 的 `scripts` job 先自检后执法。

## 平台覆盖矩阵（2026-11-17 首立，按 workflow 事实源抄录）

"每个芯片平台都在流水线里验证"的对照表（**结构真源** =
[`docs/process/ci-workflows.md`](../../docs/process/ci-workflows.md)；本表只映射"哪个平台
被哪条腿覆盖"）。**编译腿**只证明目标 ISA 能构建，**执行腿**才证明产物在目标环境
跑得起来；两者都缺的格子必须如实留空。

| 平台 | 编译/构建腿 | 目标环境执行 | 契约字节面 | 发布制品 | 缺口（不冒领） |
|---|---|---|---|---|---|
| 英特尔 / AMD（x86_64） | gate + integration 全量；`cross` x86_64 | 原生直跑 `rrd` / `rr3-ctl` | `ipct003_dump` golden | `publish` linux/amd64 镜像 | 工控真板未上（A 轨） |
| 瑞芯微 RK3566/RK3588（aarch64） | `cross` aarch64-musl 七件；`buildroot` 全量镜像（aarch64） | alpine arm64 容器真跑 | 容器内 `ipct003_dump` golden | buildroot 镜像 artifact；`publish` linux/arm64 | 实烧/串口 shell 不在 CI |
| 进迭时空 K1/K3（riscv64） | `cross` riscv64-gnu 七件 | `qemu-riscv64` 真跑 | qemu 内 golden 对拍（第 5 路编译期 const 已接） | **无**（publish 仅 amd64/arm64） | 真板环境与发布面缺 |
| ESP32（classic） | `esp32-gates`：periph-base / uart-echo / espidf-std / sentinel / act-sense | **无运行腿**（xtensa 无 qemu，运行归真板） | 编译期 const + `ipct003` 第④路 | tag release 固件 ELF（`firmware` job） | 真板运行证据不在 CI |
| ESP32-S3 | `esp32-gates`：periph-base S3 / act-sense S3 | **无运行腿** | xtensa 编译期 | **无**（firmware 仅 classic） | 真板运行与发布面缺 |
| 宿主 Windows x86_64 | `host-windows`：workspace check + test；独立工程中性子集（`mkmodule` / `rr3-codegen` / `rrctl` / `rr3-pack`） | 原生 | 由测试面覆盖 | — | 依赖 UDS/termios/HAL 实现面的独立工程按平台缺省排除（理由在缺口④） |
| 宿主 macOS | `host-macos`：workspace 全量测试；`host-macos-independent`：独立工程测试（两腿并行） | 原生 | 由测试面覆盖 | — | esp 条件工程按门禁口径排除 |

**已知缺口四条**（下一轮补腿对象，不冒充已覆盖）：① riscv64 无发布镜像；
② S3 无固件 release 资产；③ 三块真板（esp32 / rock3 / spacemit）的运行证据不属 CI
能力，归板上批次；④ Windows 独立工程门**部分收口**（续轮十六起，续轮十八扩面）：
已覆盖 4 个可跨平台编译的工程（`tools/mkmodule` / `tools/rr3-codegen` /
`tools/rrctl` / `tools/rr3-pack`，Unix 专属动作各自 `cfg` 门控；rrctl 的 UDS
查询在非 Unix 如实返回"仅 Unix 可用"）；
`tools/rr3-sentinel-harness` 依赖 `rr_ipc::transport::uart`（仅 linux/macos 编译，
raw termios），`base/ota/rr3-ota-ctl` 依赖 `rr-ota::bootenv`（模块实现按 `cfg(unix)`
收口，CLI 子命令尚无非 Unix 分支），`modules/*`、`tools/sim`、`tools/rr3-bridge`
等依赖 UDS 或 HAL 实现面，按平台缺省不纳入 Windows 腿——纳入前先要它们各自具备
非 Unix 编译面。

## 点验矩阵出数入口：`matrix.sh`

[matrix.sh](matrix.sh) 不改判据、只出数（0.0.5 批K H3/C12/C11）：按 architecture §12.6
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
