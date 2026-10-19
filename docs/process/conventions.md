# rock3 开发规约（v0.0.1 起生效）

来源：`docs/process/dev/0.0.1版本洛克三号开发方案.md` §5。本文只收录**可机械检查**或与
评审判定直接相关的条目；架构语义以 `docs/architecture/architecture.md` 与 ADR 为准。

## 1 分支与提交

- `main` 受保护；日常在 `dev/*`。功能 `feat/<topic>`、修复 `fix/<topic>`、文档 `docs/<topic>`
- Conventional Commits：`feat|fix|docs|chore|refactor|security`（scope 用 crate/目录名，如 `feat(rr-ipc)`）
- 涉及架构决策的提交 body **必须**引用 ADR 编号（`引用：ADR-0006`）
- 一次提交只做一件事；`git mv` 类纯移动不得混入内容改动

## 2 机器可检查项（CI 门禁十步，见 `tools/ci/check.sh`）

| 步 | 检查 | 命令 |
|---|---|---|
| ① | workspace 编译（含测试目标） | `cargo check --all-targets --workspace`（CWD = `base/runtime`） |
| ①b | workspace 全量测试 | `cargo test --workspace`（CWD = `base/runtime`） |
| ② | manifest schema + 注册表形状 pin | `cargo test -p rr-registry --test contract` |
| ②b | mkmod 输出即契约用例（方案 P3） | `cargo test --manifest-path tools/mkmodule/Cargo.toml` |
| ③ | 主题前缀归属 | `cargo test -p rr-registry topics_prefix` |
| ④ | 仓内所有独立 Rust 工程编译 | 逐工程 `cargo check --all-targets --manifest-path <dir>/Cargo.toml`（`modules/` + `tools/` + `base/ota/`，排除 `base/runtime/*`） |
| ⑤ | 文档门：已跟踪 md 不得为空、相对链接不得断、README 不得漏进导航 | 遍历 `git -c core.quotepath=false ls-files '*.md'`（全集进门，无任何排除），逐个判 `[ -s ]`，并取 markdown 链接括号里的相对路径目标判其是否存在；再把 `docs/README.md` 的链接按 `docs/` 归一成仓根路径，逐个已跟踪 README 对号（`docs/README.md` 自身是导航本体，不参与被检），未出现在导航里的打印 `导航漏链  <path>`；违规逐条点名后非 0 退出 |
| ⑥ | **格格双达标·独立工程测试接线**（architecture §13；0.0.5 批E 起，批E' 扩全量口径） | 逐工程 `cargo test -q --manifest-path <dir>/Cargo.toml`（`modules/` + `tools/` + `base/`，排除 `base/runtime/*` 与两个 esp 条件工程）；PASS 计数；`find` 打空即整步 FAIL（"接线了但没测"在此红，不以 SKIP 充数） |
| ⑦ | **§13 静态断言五件**（全 fail-closed，逐条点名） | ⑦-a 机制核与件面零策略载荷（禁词扫 `rr-mech/src/*.rs`（0.0.5 批M 控制原语叶子件下沉，MC-10 L-4 补）+ `rr-core/src/*.rs` + `rr-hal-api/src/*.rs`（0.0.5 批I C8 起）+ `rr-ipc/src/protocol/*.rs` + `transport/mod.rs`，**0.0.5 批K H2 起再加 `modules/*/atoms/*/*.rs`**——件声明面同样不许带厂商/芯片/器件名，行注释先剥离）· ⑦-b `F⊗F`/装配成员存在性（每份 `composite.toml` 的 `atoms` 行须解析到本模块 `atoms/<名>/atom.toml`）· ⑦-c 五件套计数（件形按 `module.toml` 的 `lang` 数据位选：rust 五件 / python 两件式 / 组合件实体按 lang 推导——零件名零硬编码，批M MC-10 L-2）· ⑦-d 入口体量 ≤80 行（入口路径亦按 `lang` 推导）· ⑦-e 门禁清单自指判等（⑦-a 扫面名集 A 逐名须在 §2 本行与 `tools/ci/README.md` 的 ⑦ 行齐 + `tools/ci/rr-core-xtensa-check.sh` 的 `-p` 目标集 ⊆ A；代码为唯一真值、文档向代码看齐；0.0.5 批M MC-10 L-4）。**"每格一件·不留空格"的反向断言不在本步**，在 ② 的 rr-registry 仓扫描测试（`repo_atoms_compliant_and_every_slot_uniquely_covered`） |
| ⑧ | **件级判据三门**（0.0.5 批K H2 接入；全 fail-closed，上线即全绿） | ⑧-a 两腿对拍计数门：格两轴读 `contracts/slots.toml`——`status` 判分母（C9 门只读这一处）+ `tier_scope` 记档位适用并校验非空/档名在册（批M MC-10 L-2 扩读，收 R-8）——normal 格落点件须 mech.rs + 模块 composite.rs 两腿各 ≥1（腿一 `fn parity_leg1_*`、腿二 `fn parity_leg2_*`，命名即机器锚），只有腿一记未达标；placeholder 记"不适用"、retired 退出分母；件形按 `module.toml` 的 `lang` 判（lang=python 两件式记不适用，不再用 `[ ! -f mech.rs ]` 存在性启发式）；C10 stub 反例自证内置（须由**对拍计数**拦下，拦不住空件则门自身 FAIL）· ⑧-b 依赖闭包 grep 门（C7 分级执法）：扫 `modules/*/atoms/*/*.rs` 非 test 面（行注释与 `#[cfg(test)]` 块先剥）——L0 = `std::`（含 `use std::`/`extern crate std`/全限定内联），L1 = L0 + `serde_json` · ⑧-c 注入面校验门：rr-registry contract 的 injection 用例**显式过滤器**，打空即红（run_tests ≥1 断言），封"用例被改名/删除"的 fail-open；形状级含 **L-5 空面规则**（`handles`/`ops` 两数组**均空**须明写 `no_injection = true`，否则与"漏填"不可区分判红——0.0.5 批M MC-10 L-5） |

**拟增三步（2026-10-14 增补轮六登记，尚未建成——建成前不得当作现有强度引用）**：
**第八轮实测（2026-10-15）把"尚未建成"从声明变成计数**：`tools/ci/check.sh` 里可数的 step 只有
①/①b/②/②b/③/④/⑤/⑥/⑦/⑧ 共 **10** 行，⑨/⑩/⑪ **零命中** ⇒ 待建门 **20 把、专属执法位建成 0 把**（全案 §22.1，基线 `b11dd2f`）。
**2026-10-16 对表**：`bbc853d` 起脚本多了 **`step "⑨-d 生成物一致性…"`**（门 f，n1-8 的 `rr3-codegen --check`）⇒ 现在可数的 step 是 **11 行**、
**专属执法位建成 1 把、待建门 19 把**（`⑩`/`⑪` 仍零命中）；**第九轮再 +⑨-s 枚举单源对账 / ⑨-t 契约版本位必读 / ⑨-u 运行期绑定键表单源 ⇒ 待建门 22 把**（全案 §23.4）；**第十轮再 +⑨-w 依赖面禁令 ⇒ 待建门 23 把**（全案 §24.5，取证基线 `ced8946`）。这正好**验证了 实-14 的三层判据**：
门 f 的三层里第 ③ 层（专属执法步）今天落了，前两层早就有——**报数必须说清是哪一层，否则同一把门会被报成"没建"和"建了"两种相反的话**。
**2026-10-18 对表（基线 `f0a1c75`）**：第 0.5 步件一建 **`step "⑨-w 依赖面禁令…"`**（基-35）⇒ 可数 step **12 行**、
**专属执法位建成 2 把、待建门 22 把**；判据＝单元 Cargo 直依 `rr-hal-api` ＋ use 面白名单四类冻结（backend 装配/sim/provision_store/Rr3PcmStream）＋ 板侧禁 `rr-hal`/`rr-bus`/`rrd`；
单元 Cargo 级 `rr-hal` 拆边（装配上移）与 `rr-bus` 契约面化两条窄化敞口随第 0.5 步收口行入 status.md（decisions.md 基-35 行登记）。
**2026-10-18 对表（n1-11，批N1 收尾小件）**：建 **`step "⑨-n 注入路径唯一…"`**（门 n 第 ③ 层专属执法步，全案 §8 门 n / 施工图 Task 8 Step 2）⇒ 可数 step **13 行**、
**专属执法位建成 3 把、待建门 21 把**；键名单自代码常量 `RR3_PIN_DERIVED_KEYS` 导出（单真值点，批N3 扫面 7→8 自动跟随），
扫面前件 fail-closed 四查、`[hardware]` 段行注释先剥后逐行点名；**门 n 三层判据自此齐**（下两段"③ 缺"的历史登记不回改），
`spi_device` 收口仍随批N3（status.md 敞口第 18 行）。
**2026-10-18 对表（批N2 件一，⑨-c 门 c 接链）**：建 **`step "⑨-c 电流预算装载拒链…"`**（校验⑨ 第 ③ 层专属执法步，
全案 §5 ⑨ 行 / §16.4）⇒ 可数 step **14 行**、**专属执法位建成 4 把、待建门 20 把**；
判据＝装载期 per-instance 扩算后 Σ demands.peak_ma (critical=true) ≤ current_cap_ma（`rrd rr3_demand_budget_check`，
接 boot 链致命汇合点）＋ kind=power pin 申报覆盖判据（`rr3_demand_kind_power_gaps`）＋ concurrency_group 恰一组
归属按组互斥取 max（F-12：不重叠必须是数据——空组/多组归属按不收窄计，宁超算不漏算）；
显式过滤器 `demand_budget` 封 fail-open（4 用例＝`demand_budget_check_nine` 并发三态 ＋ `demand_budget_chain_*`
装载拒链三用例；新拒链用例命名须带 `demand_budget` 前缀进本步扫面）。
**2026-10-18 对表（批N2 件六，⑨-a 门 a 报告模式）**：建 **`step "⑨-a 基-39 角色覆盖六表对账·报告模式…"`**
（门 a 第 ③ 层执法位，全案 §8 门 a / §24.1 第 5 条 + 裁-6）⇒ 可数 step **15 行**、
**专属执法位建成 4 把＋报告位 1 把、待建门 19 把**（门 a 转入"已建·报告模式"——报告不置红，转红随基-39
全刀落完）；判据＝红账三段（A 未覆盖 normal 格／B roles 四外键悬空——应恒 0，悬空是错不是账、硬红不进
基线／C TRAITS·PIN_KEYS 差集）对低水位基线文件 `tools/ci/gate-a-baseline.txt`（count>基线即红防回涨、
文件缺/坏即红 fail-closed、count 下降打印须同批下调）；配套 (a) 报告走 stdout 可见面，配套 (b) 转红
终点＝基-39 角色覆盖扩表（N2 那一刀同件落 roles.toml 两角色 power.rail.sense/power.current.sense，
covered normal 4→5、红账 23＝基线初值；switch/charger 归批N3——switch 需新派生键属契约级裁定、
charger 随器件角色进场）；键集四腿全部自代码常量导出（`RR3_ASSEMBLE_HANDLE_TRAITS`/
`RR3_ASSEMBLE_DESC_KEYS`/`RR3_PIN_KEYS` ＋ slots/roles 两契约表，单真值点不抄第二份，⑨-n 先例）；
六表第 5 表已由 ② parse 层执法、第 6 表（平台 role→具名构造）归转红收口件——本步如实不判。
**2026-10-19 对表（批N3 件五，⑨-e 门 g 行程单真值点）**：建 **`step "⑨-e 行程真值单点…"`**
（门 g 第 ③ 层专属执法步，全案 §8 门 g / §16.4；读者集合定义随 act A-17①）⇒ 可数 step **16 行**、
**专属执法位建成 5 把、待建门 18 把**；判据＝旧行程键名（host config 侧）＋板侧三旧手写行程常量名
**全仓零命中 fail-closed**（行程唯一真值点 = 执行轴档案 `hardware/<形态>/axis/<instance>.toml` 的
center/lo/hi 三键，host 经 `RR3_ACT_AXIS_STATE` 装配注入、板侧经 `axis_gen.rs` 生成表两路同源）；
**严格口径：注释/断言提及字面量名同样红**（表述一律用"行程键/手写行程字面量"泛称——防回流靠门
不靠记性）；豁免三处写明非放水：docs/ 历史账（status/conventions/全案/ADR 记录漂移与裁定不改史）、
门自身执法文本（grep 模式必然含字面量）、target/ 构建产物。配套 (a) 档案生成链 = rr3-codegen
`axis_gen.rs`（最小校验面：schema=1/三键非负整数/lo<center<hi/文件名与 pin 末段对账，坏档宁可红
不生成带病副本，件五③ `ee18519`）；配套 (b) host 侧注入面五键扩形（件五② `bfe3505`，e2e 证
分批必红——生产者消费者同批）。⑨-d 相邻行顺带订正：architecture §14 门表 ⑨-d 行"待建"改"已建"
（n1-8 `bbc853d` 建成时漏改，本批发现即订）。
**2026-10-19 对表（批N4 件三，⑩ 门 d 面板覆盖率·报告模式）**：建 **`step "⑩ 门 d 面板覆盖率对账·报告模式…"`**
（门 d 第 ③ 层执法位，全案 §8 门 d / §16.4 / interact 册 I-19；基-19↔拟步 ⑩）⇒ 可数 step **17 行**、
**专属执法位建成 5 把＋报告位 2 把、待建门 17 把**；判据三段：① 分母**降字段级五路并集**
（`ops[].slot` ∪ `RR3_PIN_KEYS` ∪ roles.requires 每项 ∪ `RR3_DOMAIN_KEYS` ∪ `RR3_ACT_PROFILE_FIELDS`，
五路全部自代码/契约常量导出＋声明数判等——单真值点不抄第二份，⑨-a/⑨-n 先例）＝**70**
（ops 27＋pin 11＋role.requires 去重 7＋domain 11＋axis 14）；② 分子＝每端点数据化申报
`covers = [<集合>/<字段>]`（`RR3_PANEL_ROUTES` 升三元组，词表单真值点＝码内
`RR3_PANEL_COVER_SETS` 五名 ops/pin/role/domain/axis；申报制：未申报不计分子、不是"默认全算"；
词表外/缺斜杠/多段＝申报面错硬红不进基线，⑨-a B 段同款）；③ 差集机器生成逐条 "D " 行打印
（ADR-0022 裁定 6.3 开放项的差集产出位；敞口表章程"缺码＋工单号不许混装"，开放项数据结构表
随批N4 件九同批立——本件红账以步 ⑩ 打印为机器真值，不进敞口表）；**红项数不许靠缩分母减少**
（I-19 判据字面）机器化＝双行基线 `tools/ci/gate-d-baseline.txt`（行 1 红账上限：回涨 FAIL、
下降打印须同批下调；行 2 分母下限：缩分母 FAIL——I-19 判据字面、扩面打印须同批上调；文件缺/坏/
非纯整数 FAIL fail-closed）；初值 70/70＝**建即红如实在账**（今日 14 端点读配置者 0＝I-15 实况，
covers 全 `&[]` 如实申报），红项随批N4 件九三面扩端点逐批转绿、转绿终点＝差集空、门 d 转红退役；
fail-closed 四路实测（covers 形制乱写／基线坏／基线缺／分母缩水各一次红）；步 ⑩ 的 EXIT trap 为
⑨-a trap 超集（临时目录＋五基线文件清单原样保留，bash trap 后设覆盖先设）。
**2026-10-19 对表（批N4 件四，I-20 原子序下沉＋applies 申报面）**：两件——
① **落盘原子序单点**（全案 §19.3 判据 2，L-23 写路径形态）：`rr3_atomic_store` 实现自
`rrd/src/counters.rs` 原样迁 `rr-ipc/src/persist.rs`（凭据面 rr3-interact 是独立 workspace
不依赖 daemon crate——共享件住双方公共最低层，evidence.rs 同径先例；rrd 侧 `pub use`
re-export 保 counters/axis 调用点零改动），`panel/secrets.rs` 写侧退役 `truncate(true)`
原地覆盖改走共享序（父目录 0700 = tmp 窗口护城河、rename 后收 0600）——**仓内落盘写路径
仍只有这一份**，persist.rs 模块注释登记三调用方（counters/axis/secrets），改面须同批过注释；
② **applies 生效路径三态申报**（全案 §19.3 判据 1，"缺失即红"）：码内词表
`RR3_PANEL_APPLY_MODES = [live, reboot, reflash]`（单真值点），`RR3_PANEL_ROUTES` 升四元组
`(method, path, covers, applies)`，今日唯一申报 = `POST /api/model/key` → `live`（brain 现读
即时生效；应答带 `applies` 字段＋前端文案同义双面），其余 13 端点如实空（写口全是动作与
运行态，不动可配置集）。执法双门：`applies_use_declared_vocabulary_only` 单测（随 ⑥）＋
步 ⑩ applies 段（词表打空/词表外/写口缺申报/行形制坏四路 fail-closed 实测全红过；申报面
硬红不进红账口径，门 d 账 = covers 差集一路——covers 列整列丢失归 Rust 四元组类型门，
bash 不重复执法）；可数 step 仍 **17 行**（⑩ 是扩段非建步）。persist 单测 1 条随 ①b、
secrets 5 条随 ⑥。
**2026-10-19 对表（批N4 件五，五组硬编码数据化＋门 d 分母上调）**：ops 槽九处进分母——
connect `min_pass_len` 与 interact `default_expression` 走**件级** `assemble.toml`
（消费点在原子件面）；interact `face_tick_ms`/`selftest_face_expect`/panel 五限额走
**组合件级** `composite.toml` `[assemble]` ops（消费点在组合件面无原子件对应——门 j
组合件申报面的第一个消费批，全案 G-3"策略往组合件躲"的规避路径就此封死）；⑩ ops
导出面同批扩"件级＋组合件级"两来源并集（slot 形制相同，并集去重同账）；基线
`gate-d-baseline.txt` 双行 70/70→79/79（ops 27→36，扩面同批动基线有迹可循）。策略
装载沿用 S-5 口径：代码默认 = 文件缺键回落；fail-closed 三路 = 白名单外值点名拒
boot（default_expression/face_expect 过 `RR3_FACE_EXPRESSIONS` 对账）、类型错点名拒、
0 值自残折 1 拒放（节拍/限额下界）。`RR3_PANEL_ADDR_DEFAULT`/ENV 保持 env+常量双真值
不进 config（批I I-1 bind/port 删除同口径，避免第三真值点）；自检逐腿
`{legs:[{leg, pass, reason?}], verdict}` 上线（pass 词表 pass|fail|n-a，恒败腿如实
fail 不折算通过＝D15），前端逐腿渲染同步。
**2026-10-19 对表（批N4 件七，实-6 修法三件——rtos 档"只改数据"补课）**：三笔分立提交，
每笔独立自洽——(a) host 词形规则下移平台事实表（`0082114`：三份 `contracts/platform/*.toml`
加 `[controller_wordform]` 段、`rr-registry` 扩段解析＋键域封闭、rrd 派生器改读表＋①-f 遍历；
词形规则从 rrd 机制侧撤出，⑦-a 禁词扫面逃逸随之闭合）；(b) 桥上配置腿（`b9cc482`：rr-ipc 新增
bindings 模块＝绑定真值单点（no_std 零依赖 serde-free，method 常量＋三键域＋快照结构＋u8 裁决
＋单条目写入，JSON 遍历留两侧各自消费面）＋`Rr3HalBindings` 三字段＋`RR3_ASSEMBLE_DESC_KEYS`
十→十三键（契约级评审同 批N3 件六 spi_device 先例；三键不在 `[pins]` 派生键集——无 role 对应，
rtos 档 `[hardware]` 原样直给进 `RR3_HAL_BINDINGS`）＋`esp32-mini.toml` `[hardware]` 三键
（`i2c_sda="21"`/`i2c_scl="22"`/`adc_rail_pin="34"`，取值同源板侧装配）＋`esp32_uart.rs` 桥上
method 下发（`rr3.set_bindings` 载荷住 `params.args`、值全字符串；连接建立即推；失败仅点名
不上抛不重试——执法点在板侧消费面，板未支持 method 时宿主仅打点，舵机/表情不受扰））；
(c) 板侧消费腿（本笔：`frame.rs` 分发臂逐键过键域/值域校验、坏值整体不落；`state.rs` 进程级
注入单点（引脚号是芯片级物理事实且自愈路径在深层自由函数够不到 `Bus`——单点存取，毒锁不失效
同 `lock_bus`）；`board.rs` 三常量删除（`PIN_I2C_SDA/SCL`、`ADC_RAIL_PIN`），接线表数字降为
文档锚点；`oled.rs` 自愈两路径（`i2c_bus_recover`/`i2c_bus_unstick`）改读注入，未注入拒不碰
GPIO；`runtime.rs` 每拍 force 重同步缺注入即跳并计 `selfheal_skipped`（INFO 观测面，静默防刷屏）；
`main.rs` boot 解卡拒则点名（v3 拍板：消费面级 fail-closed，不做板级等宿主的启动时序重构——
boot 期桥未连注入必缺席，启动期解卡能力随 0.0.7 批G/D1 主驱动具名引脚改造回归）、横幅去引脚号
字面量；绑定注入成功打印三键值＝板侧引脚真值唯一观测点）。**读不到注入值即拒不回退硬编码**
的机制面三处观测：boot 解卡点名 / 运行面 `selfheal_skipped` 计数 / 重试路径终败 Err 冒泡。
行为判据（改档案实录＋抽注入拒起对照）仍归真板批——status 敞口 21 行不结项。
**2026-10-19 对表（批N4 件八，⑨-r 门 r 留痕不进被丢弃的流）**：建 **`step "⑨-r 留痕不进被丢弃的流…"`**
⇒ 可数 step **18 行**、**专属执法位建成 6 把、待建门 16 把**。二选一裁定 **per-unit 日志文件**
（上报窄缝否决：观测面不得依赖被观测面总线健康度；§17 同轨判据"一次重启后仍能取到崩溃前的
最后 N 行"内存环做不到；总线连接前的早期留痕只有文件通道可达）。机制单点（笔1 `07b998d`）：
`supervisor.rs` spawn 处 stderr 由 inherit 改接 `<socket>.log/<unit>.log`（append 不截断，实例按
socket 全路径隔离零新 CLI 键；`rr3_open_unit_log` 非绝对路径 fail-closed 拒开；文件打不开回退
inherit 并点名——观测通道故障降级不放大成可用性故障），stdout `Stdio::null()` 测试管道纪律
不动（RrdTest 读的就是那条管道）；单测三断言＋`e2e_unit_log.rs` 线上证据（子进程 stderr 真
落 per-unit 文件）随 ①b。执法位判据（笔2 同批）：六模块生产码 stdout 留痕零命中——五 Rust
模块 `println!`/`print!` 词边界 grep（防 `eprintln!` 子串误伤）＋ brain Python **AST 判**
stdout `print`（多行调用按行 grep 会误报起始行；python3 缺失/解析崩＝门禁失效不放行，⑨-n
扫面前件同一纪律）；`eprintln!`/`print(file=sys.stderr)` 是声明过的通道（per-unit 日志）不在
禁列。**扩面注记**：brain 原不在门 r "45 处"实测口径内（只数五 Rust 模块），件八同批纳入
（brain stdout print 12 处 → `file=sys.stderr`）；五 Rust 模块现役 52 处 stdout 宏同批清零；
mkmodule 骨架模板同步改 `eprintln!`（新生成模块生来合规）。sim 夹具两处 spawn（brain.rs/
chain.rs）本就 stdout/stderr 双流同文件，brain print 改 stderr 不破 `wait_log_contains` 消费。
**2026-10-19 对表（批N4 件六第一笔，I-21①③＋B-13①＋B-16①②——"成功"的假通过口与超时数据化）**：
三处假通过口同笔闭合——interact `rr3_tts_pcm_integrity` 加零字节下界（(0,0) 不再是合法通过态，
云 200 空体的静默假成功口关死）且 `played_ms==0` 折 `tts_play_reject`＋error 应答不记
`tts_play_ok`；面板 `BusCall` 出口加载荷判等（`rr3_bus_reply_status`：应答含 `error` 即
422+`ok:false`，与 502 传输失败分码——脚本按状态码判成败不再恒真，前端 `r.ok` 失败即
显示拒因）；brain 计账拆两本（`chat_rounds` 退役，`rounds_replied`=应答发布完毕、
`rounds_audible`=回执 `played_ms>0`——"完成一轮"与"用户听见"自此可分，`/api/data`
counters 同表可见）。超时数据化（B-13①，D7 解耦裁定）：`[rr3-brain.bus]`
`call_timeout_s=30.0`/`connect_timeout_s=5.0` 新段，码内 `RR3_CALL_TIMEOUT_S`/
`RR3_CONNECT_TIMEOUT_S` 两常量退役，config 先于总线连接装载（缺键 fail-closed 更早）；
云面传输异常收敛一个类型一个处置点（`rr3_brain_http_post` OSError 族→RuntimeError、
`asr.py` json.loads 进护栏，llm 腿既有护栏回归钉住）——同一类云不可用不再一半弃轮
一半掀进程。单测：interact 55 条（零字节下界两例＋422 判等三例）、brain 拆账四例＋
收敛三例（brain 单测归 CI brain-unittest job，本机 check.sh 不覆盖）。

**2026-10-19 对表（批N4 件六第二笔，B-12——brain ops 槽对门 d 转绿首刀＋值零回显）**：
新 RPC 主题 `rr3-brain/ops/list` 契约注册（contracts/topics.toml，request/**L2**——
件九配置面扩写口时随面演进，MCP-1「先 append 契约再落码」先例），主题计数 pin
七处断言同批续登 31→32（rr-registry topics.rs 三处含 merge 前后、contract.rs 计数
＋kind 分布 request 12→13、rrd topics_load.rs 三处含 overlay 合并）；brain 侧
`RPC_EXPOSES` 与 module.toml `[rpc].exposes` 双写同源＋1，应答体 = 纯函数
`rr3_ops_snapshot` 白名单组体（按 asr/llm/tts/dialog 四原子分组、六槽名单键——
llm 组取模型行实效值，B-14① 双源分裂如实分组不合并；密钥与 `api_key_ref` 不进
任何应答，值零回显守脑面，密钥写读走 panel secrets 端点只报 ref 名）。面板
`GET /api/brain/ops` 读口复用 BusCall 透传（wifi/scan 先例），covers 申报六槽
（ops/asr_model、ops/base_url、ops/llm_model、ops/max_utterance_ms、ops/timeout_s、
ops/tts_model）＝ 门 d 红账 79→73（基线行 1 同批下调，分母不动）；端点数 pin
14→15；前端 tab-sys 加「brain 模型面」只读卡（表行单行书写是 ⑩ awk 逐行解析的
硬约束，跨行表项被拆行判坏——首跑即红的实证）。单测：interact 55 条（表-分派
pin 自动适配＋covers 词表门自动过）、brain 快照四例（fallback/行覆盖/密钥零回显
序列化全文断言/缺段缺省）——brain 单测归 CI。
**"执法位"是三层，报数必须写清是哪一层**（第八轮 实-14 立）：① **判据实体**（代码里判不判）② **反例用例**（有没有一条输入让它变红）
③ **专属执法步**（`check.sh` 里有没有一个点名它的 step）。三层各自可缺，**混报即失真**。
门 n 今天是 ①② 有、③ 缺：两条稳态判据有装载期实体（`rrd/src/profile.rs` 门 n 段——`rr3_profile_boot_inputs_with_probe`
内，取证时点 `e9a764f` :1089-1100；原记 `:1042-1056` 与测试用例 `:1986`/`:1999` 系 `b11dd2f` 时点，漂移即基-37 活体），反例用例
`gate_n_hardware_physical_key_is_fatal`（取证时点 `e9a764f` :2347）与 `gate_n_coexistence_names_both_values`（`:2360`）**已经步 ①b
`cargo test --workspace` 进 CI**（`base/runtime/Cargo.toml:3` members 含 `rrd`），缺的只是点名它的 ⑨-n 步**（2026-10-18 对表：⑨-n 已随 n1-11 建成 ⇒ 门 n 三层判据齐；本句至此为历史时点登记）**。
——**有实现不等于有专属门，但没有专属门也不等于没执法**；把 ①② 有报成三层全缺是**反向失真**（文档小于代码），
与 实-13 的"文档大于代码"同为违尺子。**真正缺的第三样**：反例用例**不按判定位编号组织** ⇒ "31 位每位至少一条反例"
今天机判不了（基-2 那 62 条清单的余量所在）。本段首版写"门 n…没有 CI 执法位"，已由 实-14 就地订正；
入口条件那一族仍是本轮 **L-31**。
**（订正：本行首版把"文档大于代码"挂给 L-23，而 L-23 是"新设计自带第二真值点"；"文档大于代码"是本轮 实-13，已改。
与 实-14 同族——引用一个已存在的编号前回它的定义处核一次。）**
**同轮第二条回写期发现（实-15）立一条文档纪律**：**施工图/方案里凡预写"将来要往台账、契约表或代码某张表里加一行"的句子，
写施工图的那一刻就要回那张表核一次**。理由是执行侧的纪律本身会放大它——施工图要求"每行带证据、无证据的句子删掉"，
执行者照做时**不会去质疑那行断言的真假**，于是假账**带着证据号进台账**，比没有证据的假账更难翻。
实例：施工图 Task 9 Step 2 原稿预写"`spi_*` 派生键未入 `RR3_ASSEMBLE_DESC_KEYS`"，而该表十名里第十名就是 `spi_device`
（`rr-registry/src/assemble.rs:56-67`）；真正缺的是门 n 扫面 `RR3_PIN_DERIVED_KEYS` 七键（`rrd/src/profile.rs`，取证时点 `e9a764f` :92；实-15 时点记 `:91-99`）。
**这是 F-5"文档示例值会被装机档抄"的加重形态**：F-5 抄的是**值**，这里抄的是**一句关于代码状态的断言**。
**这条纪律自己要有执法位，不能停在"下次写仔细点"**：凡施工图里出现"往 X 表加一行"的 Step，
**该 Step 自身必须带一次 grep 回执**——写施工图的那一刻就把命令与命中数贴进 Step 正文，
**没有回执的这类 Step 不许被执行**。理由同上：断言的真假只能由那一次回表核确定，而到了执行阶段，执行者已经没有回表的动机了。
**回执的定义要收紧到不可含糊**：它必须是"**真跑过的那条命令 + 它的实测命中数**"，
不是"能证明该断言的那一类命令"——**命令选错（范围过宽、或核的是另一张表）与不跑同罪**，因为读者无从分辨二者。
（这条收紧不是推演出来的：写它的时候我自己先交了两条没跑的回执，`-- '*.rs'` 实测 21 处指不到"第十名"这件事、
`-- contracts/` 实测 1 处是**格**名不是角色名，见施工图 Task 9 Step 2 的登记。）
**同族第三例**（第八轮回写期撞出，就地订正、不新立发现号）：施工图 §1.1 **D-1** 行的证据列
漏列 `profile.rs:153`（mux_group"platform 表随批N7 落地"）与 `:570`（①-b 的"反推，批N7"理由注），
又多列了 `:148`——那处是 `addr` 的注"校验⑨ 互校随**批N2**"，而 `批N2` 是本表在册序列，**不属该分叉**；
并且把 **5 处文字命中**写成了"**五个判定位**"（实际只落在 **3** 个判定位：⑧-b / ⑥-a / ④-b）。
连带一条落点判据：**"挂敞口账"的落点只有 `docs/status.md` 那张表一处**（它是全仓唯一敞口账，另开一张就是造第二真值点）；
该表第八轮起有两类行——**缺证据**类（要板/器件/环境）与**缺裁定**类（要一次用户裁定 + 一批代码，首条＝第 18 行 `spi_device`），
新行不属前者须在行内明写属后者并给出裁定归属，**不许混装**。
⑨ **资源与接线门**（六表一致 a / 装载期十条校验·**31 个判定位**（第八轮由 24 扩为 31：+4 追认代码已实装
  而设计未登记的四条、+1 ⑦-c 拆两位、+1 ②-a0 元判据、+1 ⑥-c 拆两位；**今天实判 14、未判 17**，全案 §5/§22）
  b（每条须写**缺失语义**与**后果三态**两列，
  留空即红；范式即 `rr-registry/src/assemble.rs` 的 `no_injection`）/ 能源预算 c / 生成物一致性 d /
  行程单真值点 e（**读者集合含 `ops[].src_key` 等数据→数据引用**）/
  **禁词与豁免域单源 f**——`RR3_BOARD_TOKENS` 与 `check.sh` ⑦-a 词表同源，且**两张在册豁免域也同源**
  （2026-10-15 改判：合一词表会红在 `esp32_uart.rs` 这类合法处）/
  **注释不得独承载器件事实 g**（前件 = `contracts/devices.toml`）/ **组合件申报面进 ⑧-c h**
  （前件 = 先给组合件定义 `handles`/`ops`/`no_injection`；六份 `composite.toml` 的 `[assemble]` 段已在但只有 `atoms`））；
**同日三轮（2026-10-15 全代码复盘）再扩五把**：
**i 机制核零部署缺省**（路径型缺省 / 策略型数值窗 / 器件声道缺省 / 基座 crate `include_str!` 某板布局）·
**j 时序窗须来自供给位**（`[supplies]` 已有时序预算位，机制核不许自带另一个数）·
**k 布局档由形态给出**（`rr-ota` 的守护改指形态档而非某块板路径）·
**l 申报面必须可解析 + 查册门扩 `tools/**`**（现逃逸：哨兵内联 manifest exposes 未在册主题；
判据 2026-10-15 改判为"合法 = 落在**门认识的申报文件**上，判在**字面量的读者集合**上，不判文件扩展名"）·
**m 保护性语义须有动作读者 + 执行面用例**（判据 2026-10-15 由**动词表**改为**读者三态标注**
  `{裁决｜动作｜仅回显}`——写"避免堵转"这类换词宣称过去不受检）；
**同日五轮（2026-10-15 跨文档 + 运行期语义）不扩步序，只改写四把门的判据**（⑨-f 词表 + 豁免域、
⑨-g 补 devices 前件、⑨-h 补组合件申报字段前件、⑨-l/⑨-m 分别改判"申报文件合法集"与"读者三态标注"）；
**同日六轮（2026-10-15 端到端流与爆炸半径）再扩三把，⑨ 共 18 把、待建门合计 20 把**：
**p 物理键读者集对账**（绑定键在白名单命名空间外的读者须逐条在册，且与施工改动面一致；
同名跨命名空间的词须显式排除——`i2c_bus` 在板侧是变量名、12 处）·
**q 调用通道申报一致**（有 bus call 的模块，`[depends].required` 必含 `bus.rpc`；
现违例 `rr3-interact` 四次 call 未申报）·
**r 留痕不得进被丢弃的流**（模块生产码 `println!` 计数须为 0——`supervisor.rs:319` 把子进程
stdout 接成了 `Stdio::null()`，实测 45 处留痕按设计不可达；**2026-10-19 对表：批N4 件八已定形并建成**——
二选一裁定 per-unit 日志文件，stderr 接 `<socket>.log/<unit>.log` append、stdout null 纪律不动，
机制 `07b998d`＋执法位 ⑨-r 同批，见 §2 ⑨ 段对表行）；
**n 注入路径唯一**（同一 `handle(trait, key)` 只能有一条来源：`[pins]` 派生或 `[hardware]` 直给，
  并存即拒 boot；**七个**物理键迁移完成后在 `[hardware]` 出现即红（第八轮订正计数：原写"六个"而枚举七键，
  裁定源 `RR3_PIN_DERIVED_KEYS: [&str; 7]`（`rrd/src/profile.rs`，取证时点 `e9a764f` :92；实-9 时点记 `:88-99`），全案 §22 实-9）——本轮最重的一处改判，见全案 §18 F-1）·
**o devices 在册**（`[pins].bus`/`addr`/`baud`、`wire.mate`、注释里的型号词须 ∈ `contracts/devices.toml`；
  `addrs`/`baud_ok` 是取值域不是示例值——**装载期不认型号，评审期与出厂自检必须认**）；
⑩ **面板覆盖率门**（分母**字段级**：`ops[].slot` ∪ `pin_id.field` ∪ `roles.requires` ∪ `domains` 字段 ∪ 档案字段；
分子由**每个端点自报** `covers = [...]`，未声明的端点不计入——端点数 ≠ 面板项数，不定义映射就是让门自己裁）；
⑪ **文档门 ⑤ 扩一条**：md 内出现"同批 / 一并处理 / 随 X 批"而**同一句不含工单号或批次号**即红，
点名到文件与行（本轮实测这种句式既是无主项的生产方式，其事实基础本身也会错）。
  **基-37 再扩一条（2026-10-18）**：现行文字里的**裸行号**（`文件:行` 形状而不带基线号）即红——判据落点
  一律写**函数名／段名／符号名**，行号只作**取证时点注记**（行号随代码漂移，符号名不漂）。存量漂移四处
  随本件同批修（门 n 判定位 `:1042-1056`→`:1089-1100`、`RR3_PIN_DERIVED_KEYS` `:91-99`→`:92`、`board.rs`
  引脚 `:156/158/182`→`:148/150/174`、ADR-0021 "待裁 D12" `:220/:280`→`:235`）；其余存量裸行号不另立清单，
  随各自被触碰时补基线号。
**同日七轮（2026-10-15 设计逻辑闭合性审查，全案 §21）不扩门、不扩步序，只做两件归属动作**：
① 新立**基座轨工单序列 `基-1…基-22`**（二十把待建门逐门一件 + `基-21` 门 e 改排除表 + `基-22` 平台登记表对象件；
与拟步号一一对应，见全案 §21.4）——此前"建这把门"这件事**在全仓没有单号**，
只借它前件的单号（门 i/j/n/o 挂的 S-13/N-11/P-13 全是**数据面对象**的工单，不是执法件的工单，这是 L-29）；
② **三处事实源同批改**入纪律：全案 §16.4 计数表 / 本 §2 待建门清单 / architecture §14 速查表，
**同一轮提交内三处计数不等即红**（执法位 = 基-20 / 拟步 ⑪）。
第七轮实测这三处已滞后两轮：§16.4 停在"17 把 / L-1…L-24"，而实为 20 把 / L-1…L-29（登记为 T-10）。
**同日四轮补第二条**：单号只解决归属，不解决事实——凡"N 册 / 全部 X / 三处 / 六个"这类**集合宣称**
必须带**可数枚举**或引一次实测计数（写成"五册同批（A-15）"照样过门，而 sense 根本没有那个符号）。
**同日八轮（2026-10-15 追溯闭环与实测分母，全案 §22）不扩门，改三处口径 + 立一条纪律**：
① 判定位分母 **24 → 31**（+4 追认代码已实装而设计未登记的 ①-c…①-f、+1 ②-a0 元判据、
  ⑥-c 与 ⑦-c **各拆两位**）；**今天实判 14、未判 17**；
② `基-1…基-22` 扩为 **`基-1…基-25`**（+基-23 出厂轨装载面齐装 · +基-24 桥上配置腿 ·
  +基-25 唯一"装载消费真值集清单"三处判等；三件**不占拟步号**）；
③ 教训类 `L-1…L-30` 扩为 **`L-1…L-31`**；
④ **新纪律（L-31 的执法面）：判据登记时必须写出入口条件，且入口条件不得是可省略的参数**。
  `rrd/src/daemon.rs:248-253` 的 `None` 臂让"不传 `--profile`"成为整张 31 位表的合法旁路，
  而 buildroot 出厂轨（`board/radxa-zero3/post-build.sh:129-155` 的 `S99rrd` 只传 `--socket`+`--manifest`）
  正是这么起 rrd 的，且该脚本从不安装 `profiles/`+`contracts/`+`hardware/` ⇒ **RK3566 出厂路径上这扇门不存在**。
  修法：`--profile` 升必选（缺省＝拒 boot）+ 基-23。**一条能被省略参数绕开的门等于没有门**；
⑤ 门 ⑪ 加两条机判形状：**裸圈号即歧义**（`⑦-c` 在"装载期判定位"与"`check.sh` 步号"两个命名空间里
  指不同对象，引用必须带前缀）· **索引不得与在建事实矛盾**（`docs/README.md:26`/`:67` 与
  `docs/status.md:127` 写"增补轮六未开工、本轮零 `.rs`"，而 `813a5d0…b11dd2f` 五个提交已落 `.rs`
  与三份 `wiring.toml` ⇒ 实-8）。
判据、红例与逐批转绿面见
[0.0.5增补轮六全案](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §5/§8/§15 与
[architecture](../architecture/architecture.md) §13 的"待建"门行；
同批还须把 `tools/esp32-periph-base` 这类独立固件 crate 从 ④/⑥ 的排除表里放进来（否则"格格双达标"
对板侧永远是空话）。本仓规矩不变：**门禁的强度只等于脚本真做的事**。

入口脚本的标题写作"规约三检查"（沿用方案 D7 的三条语义命名），实际执行的是**十步**
（⑥/⑦ 两步随 0.0.5 批E 接入、⑧ 三门随 0.0.5 批K 接入，`check.sh` 由七步经九步扩十步；
步序与 PASS 条数的逐批台账见本文件下方登记条目）：
②b 是 ② 的姊妹步，把生成器的产物喂回同一份 manifest 契约，防 mkmod 与 CI 漂移；
①b 把 workspace 的 237 条（Windows，推算）/294 条（Unix）测试全部跑一遍，②/③ 只是其中被点名两批的定点诊断；
④ 覆盖仓内**所有独立 Rust 工程**（`modules/` 五个 + `tools/mkmodule`、`tools/rrctl`、
`tools/sim`、`base/ota/rr3-ota-ctl`）——它们各自是独立 workspace，①/①b 打不到，
没有 ④ 的话这些目录烂掉无人知晓（无 `Cargo.toml` 的目录如 Python 的 rr3-brain 自然跳过；
`base/runtime/*` 由 ①/①b 负责，④ 显式排除以免重复）。
⑤ 是文档门（空 md、断链与导航漏链），三条判据写在上表那一行（`legacy/` 已整树退役，全集进门）。
测试步（①b/②/②b/③）除退出码外还断言"至少一个测试真实通过"——cargo 对打空的过滤器仍以 0
退出，只看退出码会在过滤器笔误或测试集改名时静默放行。

测试面如实登记（2026-09-30 M2 Task 11b 收口（`rrd` 加**唯一的** supervisor 线程（`supervisor.rs`：
boot spawn、串行收尸 `try_wait`、到期重启与"先杀挂死旧世再起"的唯一漏斗）+ CLI `--boot-role` +
`Rr3Beat::Promoted` 处 `running` 第一次上线广播（E-8）+ 新测试目标 `rrd/tests/e2e_spawn_restart.rs`
5 条 + support 的 boot 起法与 `rr3_field_from_log`/`rr3_wait_until` 两个助手）后复测。
**本轮新增的是"真起子进程、真等退避、真重启"这一类证据**——上一轮"时间判定"与上上轮
"跨进程夹具"那两类的描述原文照旧适用。**两个平台的条数本就应当不等**，所以各记各的、
不合并成单一数字：Windows x86_64-pc-windows-msvc **177**（M1 板上就绪轮在 Task 11b 的 176 之上
再加 `rr-ota` 净增的 1 条 include_str 分区标签守护用例 `ota_labels_match_genimage_cfg`——
平台无关，两平台同加 1）、
WSL2 Linux x86_64 **212**（M1 轮 212 条 = Task 11b 实跑 211 + 同一条守护用例；逐目标相加而非算术倒推：
rr-bus 4 + rr-hal 4 + rr-ipc lib 26 + rr-ipc `envelope_meta` 4 + rr-ota 5 + rr-registry lib 15 +
契约 24 + rrd lib 96 + `src/main.rs` 0 + `src/bin/sim_peer.rs` 0 + `e2e_sim_peer` 5 +
`e2e_spawn_restart` 5 + `e2e_watchdog` 4 + `register_publish` 19 + `socket_banner` 1 = 212；
Task 10 记 197、Task 11a 记 205、Task 11b 记 211）。
`aarch64-unknown-linux-gnu` 交叉 + qemu-user
本轮未重跑，沿用上一轮"与 Linux x86_64 同数"的记录，不据此声称本轮。两数差额 35 条全部可枚举：
Unix 独有的 34 条线级用例（`register_publish` 19 + `socket_banner` 1 + `e2e_sim_peer` 5 +
`e2e_watchdog` 4 + `e2e_spawn_restart` 5）+ `cli::tests` 里那 1 条
`#[cfg(unix)]` 用例。cargo 1.95—1.97：
`#[cfg(unix)]` 的代码在 Windows 上**不参与编译**，故 Windows 条数只覆盖平台无关逻辑，
Unix 侧行为不据此声称；34 条线级用例在 Windows ①b 里显示 `running 0 tests`，那是 cfg
边界的正确结果而不是通过）：
①b **177** 条（Windows）/ **212** 条（Linux x86_64；`base/runtime` workspace 全量，含 rr-ipc 的 30 个
契约守护——13 条偏移/枚举对齐 + 帧层 8 条 + `protocol::body::tests` 的 5 条 payload 类型前缀
（M2 Task 7：前 2 字节小端 `Rr3PayloadType`，round-trip / 截断 / 未知值各一条）
+ `tests/envelope_meta.rs` 的 4 条**读 JSON**
的元数据守护（`x-byte-size` / `x-offset-sum` / `payload_len.minimum|maximum` / `x-schema-version`）；
rr-registry 的 39 条 = lib 15（schema 4 + 主题集 `topics::tests` 6 + 按文本入口
`validate::text_entry_tests` 5）+ 契约 24（M2 Task 6 给 `Rr3TopicSet` 加 `rr3_topic_set_kind`
访问器，实证并进了既有的 merge 用例，故条数不变）；
rrd 的 95 条（Windows）/96 条（Unix；差的那 1 条是 `cli::tests` 里 `#[cfg(unix)]` 的绝对路径幂等用例）
全在 lib 目标（`cli::tests` 13/14 + `topics_load::tests` 16 + `manifest::tests` 7 + 横幅 2
+ `registry::tests` 47 + `rpc::tests` 10；2026-09-29 修复轮新增 5 条：叠加层路径绝对化 2、`rr3-runtime/*` 冒充在装载处被拒 1、
形状错误标注来源 1、路径值不得以 `-` 开头 1；M2 Task 4 再补 `manifest::tests` 7 条；
M2 Task 5 补在册表/状态机/发布归属 29 条与合并后复验 1 条；
M2 Task 5 修复轮 1 再补 `registry::tests` 3 条：在册 owner ≠ 首段时的越权终判 1、
`watchdog_ms = 0` 入库即拦（仅 debug 构建）1、`--restart-base-ms 0` 的 `delay_ms` 现值 1；
M2 Task 6 补上行 RPC `rpc::tests` 10 条，同批 carry-forward 再补 4 条——契约 topic 的 kind
被 merge 改写时在装载处被拒 1、登记 owner 缺失时越权文案回显 `?` 1、三条拒绝门先于
watchdog 断言（仅 debug 构建）1、`rr3_reg_facts` 的取值回显 1；
Task 6 修复轮 1 再补 2 条——两处 `ForeignPrefix` 构造点的 owner 回显都为 `?`（前缀不符 /
owner 比对两支同测）1、被捕获的 watchdog panic 不得改动在册表（仅 debug 构建）1；
Task 7 的 carry-forward 再补 `cli::tests` 4 条——`--restart-base-ms` 下界 1（0 = 退避恒为 0 =
热重启循环）1、上界由退避表末档导出（`u64::MAX / 60`，过界即饱和成"永不重启"，须在启动时拒）1、
默认套接字取自**注入的表**而非进程环境（`ENV_LOCK` 随之一并删除）1、生产入口 `rr3_cli_parse`
仍委托进程环境读取（只读断言，不 `set_var`）1；CF-3 再补 `cli::tests` 2 条——待创建的套接字路径
**不存在**也必须解析成功（这条钉住 `rr3_socket_absolute_path` 用 `std::path::absolute` 而**不是**
`fs::canonicalize`：bind 之前套接字文件必然不存在，canonicalize 会以 ENOENT/os error 2 拒掉一个合法值；
把实现换成 canonicalize 该用例即实测变红）1、已经是绝对路径时原样返回（`#[cfg(unix)]`，幂等——它是
"横幅、`UnixListener::bind`、模块侧看到的 `RR3_BUS_PATH` 是同一个字符串"的前提）1；
CF-4 再补 `topics_load::tests` 1 条——契约路径进装载入口即被 `fs::canonicalize` 规范化，
坏契约的致命文案里不得漏出 `..`（`RR3_CONTRACT_TOPICS_PATH` 由 `CARGO_MANIFEST_DIR` 拼出、
字面就带 `..`，而 `main.rs` 只绝对化叠加层那一条；运维照报错里的 `..` 去查是按他自己的 CWD 展开的）；
这条与 CF-3 恰好用**相反**的工具，理由写进 `rr3_contract_resolved` 的注释：契约必须已存在，
ENOENT 正是该报的那一条，而 bind 前的套接字用 canonicalize 会误拒合法值。
同批既有 15 条 `topics_load` 用例的**逐字文案断言**全部原样通过（含两条全等断言的
`overlay_conflict_refuses_startup_and_names_the_entry` / `overlay_declaring_reserved_namespace_is_rejected`），
故规范化只改了契约那一条路径的来源，没有改写任何错误词汇。
**这后半句到 M2 Task 8 不再成立**：遗留 ③ 把**叠加层**那条路径也收进装载入口规范化，
`overlay_declaring_reserved_namespace_is_rejected` 的全等断言随之从"CLI 原词"改成
"canonicalize 后那一份"（用例本身没增删，仍是 16 条 `topics_load::tests`），并额外断言
文案整串不含 `..`；契约那一支的文案断言仍逐字未动。
Task 8 另补 `registry::tests` 1 条（遗留 ②）——`rr3_reg_add` 容量门里 `superseded.is_none() &&`
那半句**承重**：表满 + 同名条目已是 Failed 时必须放行重注册（旧条目即将腾出槽位），
删掉这半句它就变成 `Capacity` 拒。Task 5 的 17 组变异实验里这一支变红而全绿无人察觉，
因为当时没有任何用例把"表满"与"同名 Failed"两个条件同时凑齐；新用例
`full_table_still_allows_re_registering_a_failed_module` 就是那个组合，另带两条对照
（满表上的新名字仍被容量拒、满表上的同名非 Failed 仍回 `Duplicate`）。
M2 Task 10 补 `registry::tests` 1 条——`Rr3Expiry` 必须回显**判定用的那两个数**：`elapsed_ms` 是
`now - last_beat`（不是 `now - 注册时刻`）、`watchdog_ms` 是**该模块自己**的值（不是全局常量）。
线程侧那行判死日志与线级断言都从这两个数派生；表里少回显一个，线上便只能靠掐时间判对错，
而掐时间正是"随机红"的来源。同批补 `cli::tests` 1 条——`--watchdog-tick-ms` 默认 200、可配、
**拒 0 而不是夹 1**（0 = 每拍都扫 = 忙循环，那是另一种错法）；并把既有默认值用例扩成三值同钉
（`restart_base_ms` / `pre_register_timeout_ms` / `watchdog_tick_ms`）。
M2 Task 11a（崩溃记账收口）净增 `registry::tests` 8 条（39→47）：删掉旧的 `always_policy_backs_off_and_caps`
1 条、补 9 条把"唯一一本账"钉死——`crash_streak` 按 **name** 计、跨重注册连续不归零（这是旧 `Entry.restarts`
被同名替换归零、5 次上限今天撞不到的根因）、退避表 `RR3_RESTART_BACKOFF` × `restart_base_ms` 且饱和不回绕、
`RR3_RESTART_FAILURE_CAP = 5` 到顶即停、`rr3_charge` 是三条死亡路（conn EOF、watchdog 到期、子进程退出码）
唯一共用的记账入口（双路只记一次账）、活满一整条自己的窗口才归零、达上限停止重启。
**D-6**：`rr3_reg_beat` 返回值从 `bool` 改成三态 `Rr3Beat{Rejected, Kept, Promoted}`，只有 `Promoted`
那一次是 `registered → running` 的升级。**相位面（本轮边界）**：`running` 今天**只落到日志面**
（`conn.rs` 打 `{name} → running` 这一行钉住"唯一升级时刻"），`rr3-runtime/module/state` 上那条
`running` 广播**留给 Task 11b**——上线广播属"广播几条"的判据面，11a 不碰。**11b 已上线**：
`Rr3Beat::Promoted` 处 `rr3_state_frame(name, Rr3Phase::Running.as_str(), task)` + `rr3_reg_fanout`，
日志行 `{name} → running（广播到 {n} 个连接）`，判据在 `e2e_spawn_restart` 的 S1/S2/S3。
`Degraded` 分支归 `Kept`
（只续命不升级），结掉"一颗心跳抹平降级"的欠账。**D-8**：`Rr3Expiry` 新增 `restart: Option<Rr3RestartPlan>`
（到期那一路的记账结果），但 11a 的 daemon 侧 `let _ = ex.restart;` **收下不使用**、绝不据此打
"重启计划"日志——一旦打了就当场打死 `e2e_watchdog` 的 W4（判据是观察窗内 stdout 不得出现"重启计划"），
`supervisor` 派生重启归 11b。**本轮终态的欠账（degraded 死相位）**：degraded 是"有 enum 变体、无写入方"的
死相位——它的 setter 与 base 能力门（`depends.required` 的 `bus.publish`/`bus.subscribe`/`bus.rpc` 就位判定，
spec §5.3）都不在 11/11b，一个要新相位上线、一个要给 24 条生产主题建能力字典，均不阻塞 §9 验收；
这句话同时写在 `registry.rs` 文件头与本台账，不许被任何"已实现状态机"的措辞盖过去。
11a 对生产代码的**唯一**非 registry 改动是 `conn.rs` 的 `rr3_reg_beat` 调用点与 `rr3_reg_death` 返回类型适配
（`Rr3Death` 三态），四个 e2e 文件一条用例未改；`conn.rs`/`daemon.rs` 是 `#[cfg(unix)]`，故 Task 11a 的一处
同线程重入自死锁（把 `rr3_lock(..).rr3_reg_death(..)` 直接写进 `match` 被匹配表达式，守卫临时量延寿到整个
match 语句、`Broadcast` 分支再 `rr3_lock` 同一把锁）Windows L1 编不到、看不见，只有 L2 的 3 条 e2e 超时把它抓出——
`running 0 tests` 是盲区不是绿的又一记实证。
M2 Task 11b（supervisor：boot spawn + 按策略重启 + `running` 上线）净增 `cli::tests` 1 条
（`--boot-role` 默认 `resident`、原样透传不校验取值、空值与疑似开关值都拒）与新目标
`rrd/tests/e2e_spawn_restart.rs` 5 条。**S 组判据口径**：S1 DoD 主链（boot `spawn rr3-sima pid=A` 无
attempt 尾注 → 线上 `registered` → 线上 **`running`**（E-8 第一次上线；第二世的 `→ running（广播到 2 个连接）`
写数字断言，第一世不钉广播数——boot 队列在 accept 前投完，旁观者注册与第一颗心跳谁先谁后是两种合法时序，
如实记录）→ `子进程退出 status=signal: 6 → 广播 failed` → 线上 `failed` → `spawn … attempt=1` → 第二世
`registered`+`running`，pid/task 全部 `rr3_field_from_log` 抽取后 `assert_ne!`，不写死字面值（A-5①））；
S2 所有权对照（同一 `abort` role：owned 由退出码定 `failed`、foreign 只有 EOF → `unregistered` +
`跳过重启`，**这条是"崩溃检测按所有权分级"的唯一证据**）；S3 `never` 策略（一条 `failed` 广播 + 三条否定
`spawn rr3-simb`/`重启计划`/`跳过重启` 各钉一层，四文案互不重叠）；S4 watchdog 到期路（判死 →
`杀掉挂死的 rr3-sima pid=A` → `spawn … attempt=1` 新 pid → 第二世**又**被判死——后半否定钉"第一世的
fd/条目确实让位"）；S5 孤儿卫生（`drop(rrd)` 后有界轮询 `/proc/{pid}` 消失，`rr3_wait_until` 内容判据）。
**既有 28 条线级用例只改写简报点名的两条**（其余 26 条一字未动）：W4 改成正反一对（正面等
`跳过重启 rr3-sima` + 负面 300ms 窗内不得出现 `spawn rr3-sima`，`attempt=` 护栏保留）；
`killed_peer_is_only_an_eof_until_task_11_holds_the_child` 改名
`killed_foreign_peer_is_only_an_eof_and_its_plan_is_skipped` 并正面补 `跳过重启 rr3-sima`——没有这条正向，
`sup.rr3_restart(p)` 接线断了与接线在但被判外来分不开。**两处定向变异**（L2 实跑、每次只改一处、复跑后复原）：
①去掉 `rr3_reap` 的 `rr3_reg_child_exit`（只打日志不记账）→ S1/S2/S3 全红，红点在各文件"广播 failed"
断言行（`e2e_spawn_restart.rs` 的 106/148/190）。**与简报的偏差如实记录**：简报预期 S1 红在 `attempt=1` 行，
实际"不记账"同时拿掉 `failed` 广播，S1 判据链在更早的断言处断开；**S4 没有红——真实 finding**：S4 的重启
计划来自 watchdog 到期路（`rr3_reg_expire` 记账 → `Rr3Expiry.restart`），旧世子进程由 `rr3_boot_spawn` 的
kill+wait 当场收掉、从不经过 `rr3_reap`，故 S4 对"退出码那一路在记账"不承重，该判据的承重用例是 S1/S2/S3。
②去掉 E-1 的 `jobs` 成员判据（外来模块也起）→ W4 与 `killed_foreign_peer_…` 红（3s 等待超时的日志 dump 里
正多出 `spawn rr3-sima pid=… attempt=1` 一条），顺带暴露配套事实：外来子进程进了 `children` 后
`rr3_reap` 的 `expect("children 里的 name 必有 job")` panic、supervisor 线程死掉——E-1 破口的账在记账面同样有。**所有权边界一句话**：
只有本守护起的模块（`jobs` 由 `--manifest` 填）才会被 rrd 重启；外来自注册的照样记账、照样出计划，
执行时只有一行 `跳过重启 {name}：不是本守护起的进程`（spec §5.4"自动重启要求 rrd 是父进程"；代价是
外来永不可被 rrd 重启，见 E-1/E-2）。**conn.rs:216 同形残留**（11a 评审点名）本轮自加项修掉：
`rr3_reg_add` 不再写进 `match` 被匹配表达式，改先 `let added = …` 绑定。`e2e_spawn_restart` 在 L2
连跑 10 次 **10/10**（每次 5 passed，1.15–1.18s 无漂移）——本目标全项目唯一"真起子进程 + 真等退避"，
负载敏感性最高，m-8 的教训记在这里。
M2 Task 7 的 UDS 层没有新增 lib 用例（`conn.rs`/`daemon.rs` 是 `#[cfg(unix)]` 的线程与 IO，
纯逻辑仍在 `registry.rs`/`rpc.rs` 里已被钉住；Task 8 是唯一动到 lib 的那一支——上面那段
`registry::tests` 的容量门实证 1 条，其余全在测试目标里），它的 10 条（Task 8 之后 20 条）在两个独立测试目标
`rrd/tests/register_publish.rs`（顶层 `#![cfg(unix)]`，只在 Linux/aarch64 真跑：task 号与上线广播、
重复名 EBUSY、保留名 EPERM、坏 manifest 点名字段、首帧必须 `rr3.register`、`src_task` 伪造拒帧不断连、
上行只许 Request、未注册连接按读超时收掉、`rr3.unregister` 干净退场不重启，共 9 条；
修复轮 1 再补 3 条门禁线级用例——`conn.rs:149` 首帧 `dst_task` 非 rrd 时拒帧、
`conn.rs:262` 注册后 `dst_task=0` 上行帧按崩溃收尾不回 accepted、
`conn.rs:87` `rr3_reply_ok` 的应答 dst 点对点不广播，共 **12** 条；
M2 Task 8 的发布面再补 7 条，共 **19** 条——合法发布被转发给别的连接且**不回 ACK**
（头三项逐个断言：`src_task` 是 rrd 分配给发布者的号、`dst_task=0`、体原样）、
越权前缀 → EPERM 且点名 owner（两个变量：在册的 `rr3-simb/pong/state` 与既不在册、
首段也不是自己的 `rr3-simb/pong/typo`，后者才让三层顺序里"前缀"那一道在线上级承重）、
冒充 `rr3-runtime/*` 两支 → EPERM 且文案含"冒充"、在册但主题没登记 → EINVAL"未注册主题"、
**不带 `--topics-overlay`** 时夹具主题根本不在册而生产主题由 owner 挡住（K4：叠加层不是生产
放行口）、`Event` 体缺 `topic` 只回错误帧且同一条连接之后照常喂狗（裁定 R5：不按崩溃收尾）、
两条不同 seq 的转发在收侧**按内容逆序**挑帧再回头挑一条 Response（遗留 ⑤：Task 8 之后同一条
连接上同时有应答/状态广播/发布转发三类帧，顺序无关性得再证一型））与
`rrd/tests/socket_banner.rs`（CF-3 新增 1 条：子进程 CWD 设为 `/tmp`、`--socket` 只给文件名，
断言**版本横幅的 bus 字段与"监听 …"行都回显解析后的绝对路径**，并且按那个路径真能连上、注册得了——
只断言"字符串像绝对路径"抓不到"横幅与 bind 各说一套"这个原始缺陷）与
`rrd/tests/e2e_sim_peer.rs`（M2 Task 9 新增 5 条**跨进程**用例，客户端是另一个二进制 `sim_peer`
而不是测试自己：注册应答里的 `src_task` 与 rrd 日志里那个分配值逐字相等并连喂 3 次心跳被接受、
发布被转发给**早已在册**的旁观连接（体原样、类型仍是 Event、`src_task` 是发布者的分配值）、
主动 `rr3.unregister` → 恰好一条 `unregistered` 广播且 `never` 策略不产生重启计划、
**特征化**用例"被打死的 peer 今天只是一次 EOF"（`unregistered` + `重启计划 attempt=1 delay_ms=20`
+ 回显 manifest 里那条绝对 entry；夹具在 `abort` 前先把接收队列排空——队列里留着未读的广播帧时
内核会把这次关闭改成 RST，rrd 读到的就不是 EOF 而是 ECONNRESET、广播相位随之变成 `failed`，
实测不排空时该用例 10 次红 1 次；那一维量的是"进程有没有把 socket 读干净"，不是"有没有体面退场"，
FIN/RST 双路连同退出码一起归 Task 11。Task 11 拿到退出码后必须改写本条并在提交里点名）、
同一个二进制配两份 manifest → 两个身份而 `RR3_SIM_PEER_ROLE` 只改行为不改身份。
身份的唯一来源是 `RR3_MANIFEST` 指向的那份 `tests/fixtures/sim{a,b}/module.toml`，
两份的 `entry` 是占位符 `@SIM_PEER_ENTRY@`，交 rrd 之前才换成构建产物的绝对路径——仓里不留假路径）与
`rrd/tests/e2e_watchdog.rs`（M2 Task 10 新增 4 条**时间判定**用例，客户端同样是 `sim_peer` 真进程——
"不喂狗但 fd 还开着"这个形状只有真进程维持得住，手搓帧的用例一注册完就 EOF，watchdog 根本无从判活：
静默 peer 被 `watchdog 判死 rr3-sima task=…` 点名，并且把**日志里那两个数**读回来断言
`elapsed ≥ 500` 与"≥ 超时 500ms"（把"判得对不对"钉成内容判据，而不是掐时间）；
健康 peer 连喂 3 次、同一时刻静默 peer 已被点名，而 `watchdog 判死 rr3-simb` 在 300ms 观察窗内
**一条都没有**（`rr3_assert_no_log` 是否证写法，不是"sleep 之后还没死"那种猜时间），
且两条连接都还活着——判死改的是在册相位，不关你的 fd（spec §5.0 只说"超时即判 crash"，
断开连接是另一件事，本轮没做也不该被顺带做掉）；
**恰好一条** `failed`：判死之后再跨 3 拍（60ms，跨度由已知 tick 导出而非猜行为）按**内容**挑
`phase=="failed" && module=="rr3-sima"` 的 Notification 计数 `== 1`，日志侧同样不得出现第二条判死行；
判死后连喂 5 次心跳**全部被拒**且**不复活**——只判第 5 次那颗（那一刻已静默 ≥780ms，远超到期上界
520ms，"被拒"不是运气；前四次落在哪一拍是合法时序，不判），且**整个 300ms 观察窗内 rrd 一条
"重启计划"都没打**（`rr3_assert_no_log`，与 `register_publish` / `e2e_sim_peer` 同一条否证写法）：
到期路径今天不查 `restart_policy`、不产重启计划，这笔欠账显式留给 Task 11，钉在这里是为了让它不能被
悄悄宣布成"崩溃检测已完成"。判死行里不含 `attempt=` 只是**第二道**护栏（防"把计划塞进判死行"那种
写法）——它自己钉不住这件事：判死行的格式里本就没有那个字段，任何实现下都不可能红，而 Task 11
最自然的接线是另起一行打计划，那时只有窗口否证会红。**没有第 5 条**（brief 的 W5"CLI 的 tick 接到了线程"）：它与 support
末尾那句 `rr3_wait_log("watchdog 启动 tick=…ms")` 是同一条判据——本文件把它钉在 `20`、既有
19+1+5 条线级用例把它钉在 `3_600_000`，两个互不相同的值都只能由 CLI 传进来才对得上（线程里硬编码
200、或把 `pre_register_timeout_ms` 当 tick 传，会让这两批同时超时红），再加一条只断言
"套接字文件还在"的用例不新增任何判据，按"不许空转断言"就不写。四条的等待**全是内容判据**
（`rr3_wait_log` 3s / `SimPeer::rr3_wait` 5s），唯一的时长断言在第一条且是粗上限。
**时间能进判据邻域而不让整套用例变随机红**，靠的是两处口径各收在一处：
`RR3_QUIET_WATCHDOG_TICK_MS = 3_600_000` 让**既有**每一批线级用例里的 watchdog"起了但不扫"
（Task 10 的 diff 不得改变 Task 7/8/9 用例的可观察行为——真按生产 200ms 起跑会在它们中间插入
判死日志与 `failed` 广播），只有 `e2e_watchdog` 走 `rr3_start_with_tick` 显式要小 tick；
`E2E_TICK_MS = 20` 与夹具 manifest 的 `watchdog_ms = 500`（一起决定"到期上界 520ms"这条算式）
都放在 support 而不是测试文件里，两处各写一个数就是两处真值。
`rr3_collect_within` 本轮加 **5s 墙钟上限**（`COLLECT_WALL_CLOCK_CAP`）并在 panic 里点名已收帧的
kind：一条连接上有**两个写者**（RPC 应答由该连接的读线程写、状态广播由该连接的写线程写），
帧整体是原子的但**相对顺序无保证**，所以 brief 里 A-1 那种"先收一条 RPC 应答当栅栏帧、
再断言状态广播已到齐"的写法**不成立**、被否掉；替代口径只有"按内容挑帧 + 墙钟兜底"——
广播停不下来这类形状过去会卡在 64 帧的扫描预算里出不来（整个目标表现为超时），现在是一条点名帧流的
panic。四条判据的非空转由三处定向变异实证（本轮记录，逐条对应实跑）：去掉 `rr3_reg_expire`
的 skip-failed → 第三条红（并触发 `COLLECT_WALL_CLOCK_CAP` 的 panic，顺带把那条上限也实证了）；
watchdog 判死但不广播 → 第一/三条红；广播里的相位写错 → 第一/三条红。控制器另独立复跑第四处：
把 `rr3_reg_fanout` 重复调用一次（同一拍发两条 failed）→ 第三条红并报"3 条"，
即"恰好一条"这条判据抓得住重复回报。
（每次变异只改一处、复跑后即复原）`e2e_watchdog` 的时间余量由 fixture 的
`heartbeat_ms 100 / watchdog_timeout_ms 500` 与 `E2E_TICK_MS = 20` 共同决定
（最坏情况每 ~400ms 一次判死），高负载下是真实到期而非测试缺陷，Task 11 的 `task=` 号段断言
与 EBUSY 窗口复评时同批处理。）；
本任务使 `docs/status.md` 的"watchdog 线程还不存在 / 没有 tick 循环"与
`docs/reference/rrd-registry.md` 的 `rr3_lock` 定位（`conn.rs:61-64`）、`Rr3Expiry` 字段清单
三处断言失效：前者是在途文件（本轮归属纪律要求不动别人的文件），后者随 Task 13 文档收口一并改写，
本轮不据此声称已同步；
`src/main.rs` 自 M2 只剩"参数 + 装载 + 平台门 + 把主循环交棒 `daemon`"、其 `[[bin]]` 目标 0 条——纯逻辑放进 lib，
后续的 `rrd/tests/*.rs` 才引用得到（bin 里的模块对外不可见）；M2 Task 9 的第二个 `[[bin]]`
`src/bin/sim_peer.rs`（测试夹具二进制）同样 0 条，它的 5 条全在 `e2e_sim_peer` 目标里跨进程跑。
它落在 `rrd` 包内而不是 spec §8 影响面清单写的 `base/runtime/tests/sim-peer/`，唯一理由是测试要
`env!("CARGO_BIN_EXE_sim_peer")` 这个**只对同包 bin 生效**的绝对路径（独立 workspace 成员只能靠
`current_exe()` 往上猜 target 目录，那是一份假路径）；夹具的**数据文件**（两份 `module.toml` 与
`sim-topics.toml`）仍在 `base/runtime/tests/fixtures/` 下，spec R5 的"不进 modules/、
不进 contracts/topics.toml"不受影响；落点偏离的文字同步归 Task 13；
`contracts/README.md` 称那批用例是契约的执行入口，若 ① 只是普通 `cargo check`，
它们连编译都不做，更谈不上执行）；② **24** 条（含 `registry_shape_is_pinned` 的注册表形状
硬 pin：8 owner / 31 topic / kind 12-11-4-4 / 31 条三元组——0.0.4 批D 随 connect
wifi/scan|connect|forget append 同步（0.0.3 批C 曾随 `rr3-act/relay/state`；0.0.7 批E
随 `rr3-sentinel/diag/probe` append 再同步，28→29；0.0.5 第 IV 轮 MCP-0 随
`rr3-brain/audio/tts_pcm` append 再同步，29→30；增补轮六 批N2 件二随
`rr3-power/rail/state` append 再同步，30→31）；`every_module_manifest_passes_contract`
钉住 modules/ 六模块集合且每份 module.toml 过契约；`every_owner_path_exists` 钉住 topics.toml
每条 owner.path 目录真实存在）、③ **5** 条（均为 ①b 的子集）、
②b **5** 条（`tools/mkmodule` 是独立包，不在 workspace 内，故 ①b 覆盖不到它）、
④ **9** 个独立工程（modules 五模块 + rr3-mkmod + rr3-ctl + rr3-sim + rr3-ota-ctl；
rr3-brain 为 Python，不参与 cargo 检查，其语法检查属人工检查点）。
① 用 `--all-targets` 而非 `cargo check`：普通 check 不编译 `#[cfg(test)]` 目标，
实测在测试代码里注入一个类型错误，`cargo check --workspace` 照样退 0。
M2 起随运行时语义落地再扩点名范围。

测试面如实登记（2026-10-01 M3 音频链收口复测。本轮 ①b 在 macOS x86_64（Unix）实跑 **249** 条，
逐目标相加：rr-bus 10（lib 4 + `client.rs` 新增 6——同步总线客户端的连接/注册/心跳/发布/收帧/应答）+
rr-hal 13（lib 5 + `sim.rs` 7 + `backend.rs` 1——`Rr3AudioSource`/`Rr3AudioSink` 源汇对称、
sim 三种波形、`RR3_HAL_BACKEND` 选择与 `rr3_block_on`）+ rr-ipc lib 38 + `envelope_meta` 4 +
rr-ota 5 + rr-registry lib 15 + 契约 24 + rrd lib 101（`cli::tests` 14 + `topics_load::tests` 16 +
`manifest::tests` 8 + 横幅 2 + `registry::tests` 49 + `rpc::tests` 12）+ `src/main.rs` 0 +
`src/bin/sim_peer.rs` 0 + 线级 `e2e_audio_chain` 1（三真进程音频链）+ `module_call` 4（rr3.call 回路）+
`e2e_sim_peer` 5 + `e2e_spawn_restart` 5 + `e2e_watchdog` 4 + `register_publish` 19 + `socket_banner` 1
= 249）。**rr-ipc 台账勘误**：上一轮（M2 Task 11b）把 rr-ipc lib 记作 26，漏记了 `buf.rs` 6 条、
`uds.rs` 5 条与 `frame.rs` 的第 9 条——锚点 `c49b085` 的树上代码本就带 38 条（本轮实测与逐文件
`grep -c '#\[test\]'` 核对一致），故上一轮两平台实数应读作 Unix **224** / Windows **184**，
"212/177"是台账漏记而非代码回退。本轮两数：Unix **249**（224 + M3 平台无关新增 20：rr-bus 6 +
rr-hal 9 + rrd lib 5——`registry` 2 / `manifest` 1 / `rpc` 2——+ 线级 Unix 独有 5：`e2e_audio_chain` 1 +
`module_call` 4）；Windows **204**（未实跑，按 184 + 平台无关 20 推算）。差额 45 条全部可枚举：
旧 Unix 独有线级 34（`register_publish` 19 + `socket_banner` 1 + `e2e_sim_peer` 5 + `e2e_watchdog` 4 +
`e2e_spawn_restart` 5）+ `cli::tests` 里 1 条 `#[cfg(unix)]` + `uds.rs` 整模块 `#[cfg(unix)]` 的 5 条 +
本轮两个新测试目标整体挂 `#![cfg(unix)]` 门的 5 条。M3 新增的六个测试模块（`client.rs` / `backend.rs` /
`sim.rs` / `registry.rs` / `manifest.rs` / `rpc.rs` 的增量）无一挂平台门，Windows 上照常参与编译。
②/②b/③/④ 本轮无变化（仍是 ② 24 / ②b 5 / ③ 5 / ④ 9 个独立工程）。

测试面如实登记（2026-10-01 F1 依赖仲裁复测。rr-registry 净增 **10** 条，全部平台无关、
两平台同加：lib 15→20——`depends::tests` 5 条（事实提取与 `start_priority` 缺省 100、
缺名与非字符串项逐条点名、同名先行拒绝、层内 `(priority, name)` 确定性次键、
自环整批拒且无辜者不点名）+ 契约 24→29——`manifest_unknown_dependency_rejected`
随 ADR-0009 裁定 1 **迁移**为 `manifest_unknown_dependency_moved_to_resolution_layer`
（单件层放行、仲裁层点名拒"未知能力"，迁移非放宽，条数不变），另新增 5 条：链式
拓扑序与层内次键、环+环下游整批拒、base 能力无需模块提供者、多提供者全排前、
`every_module_dependency_set_is_satisfiable_and_acyclic`（CI 全集补位——单件校验
不再判可满足性后，modules/ 六模块的整体依赖面由这条在 CI 层兜住）。本轮 ①b 推算：
Unix **259**（249 + 10）/ Windows **214**（未实跑，按 204 + 10 推算）。
② **29** 条；③/②b/④ 无变化。语义裁定全文见 ADR-0009）。

测试面如实登记（2026-10-01 F2 依赖仲裁实装复测（rrd boot 接仲裁 + Degraded 相位接线，
语义裁定全文见 ADR-0009）。净增 **10** 条：rr-registry lib 20→23——`depends::tests` 3 条
（`plan_boot` 三分流：不可满足只拒点名模块、被拒模块的 provides 仍计入可满足性宇宙故
下游不连坐、环 fatal 不产出半死集合）+ rrd lib 101→105——`manifest::tests` 1 条
（`[provides].capabilities` 可选段且逐项类型检查）+ `registry::tests` 3 条（提供者全灭才降、
双提供者死一不降、恢复须**全部** required 就位且重注册换新 task）；线级新目标
`rrd/tests/e2e_depends.rs` **3** 条（整体 `#![cfg(unix)]`，Unix 独有）：D1 链式启动序
（dependent 的 `start_priority=10` 故意低于 provider 的 50，拓扑主键压过优先级——旧的全局
优先级排序此处是反的；spawn 行由唯一 supervisor 线程串行打出，按行内位置断言序，注册序因
两个子进程并行 exec 不可用作序证据）、D2 环拒 boot（直接 `Command` 起 rrd 读 stdout 到 EOF，
断言非 0 退出 + `拒绝 boot：`/`依赖环涉及` 点名环成员 + 一个子进程都没起）、D3 降级→恢复
（提供者被测试侧 SIGKILL——死亡时刻收进测试手里，判据无时间赌注，watchdog 全程静默；
依赖者线上注册手动喂到 Running → kill → `依赖断供 → degraded` 且**依赖者自己**收到那条
degraded 帧 + `重启计划` 否定针（裁定 4）→ always 自动重生 attempt=1 → `依赖恢复 → running`）。
**本轮实跑勘误与语义补裁**：① 实装时发现 `plan_boot` 首版把"剔除后重仲裁"做成 fatal——
仲裁引擎的可满足性循环没跳过已剔除模块，同一错误第二轮再报、剔除集合零增长走防御分支；
已修（跳过剔除者），两平台同修、无条数变化。② 降级/解除广播的排除项从依赖者自身 task
改为 `RR3_TASK_RRD`（不排除，E-9 同一理据：依赖者恰恰是最需要知道"我已降级"的一方，
且它没有别的途径查自己的相位），三处调用点（daemon/watchdog、supervisor/收尸、conn/解除）
统一，D3 的线上读帧钉住。③ 既有表级用例 `provider_recovery_undegrades_only_when_all_deps_satisfied`
首版期望写反（按"全灭才降"写）——降级是**断供即降级**的非对称设计（任一 required 能力
失去全部提供者即置位，解除才要求全部就位），已按实现与 ADR 改正断言并把"已 Degraded 不重复
传播"一并钉入。本轮 ①b：Unix **269**（实跑 = F1 的 259 + 10；逐目标：rr-bus 10 + rr-hal 13 +
rr-ipc lib 38 + `envelope_meta` 4 + rr-ota 5 + rr-registry lib 23 + 契约 29 + rrd lib 105 +
线级 `e2e_audio_chain` 1 + `e2e_depends` 3 + `e2e_sim_peer` 5 + `e2e_spawn_restart` 5 +
`e2e_watchdog` 4 + `module_call` 4 + `register_publish` 19 + `socket_banner` 1 = 269）/
Windows **221**（未实跑，按 214 + 平台无关 7 推算）。② 29 条无变化；③/②b/④ 无变化）。

测试面如实登记（2026-10-01 F3 控制通道收口复测（`rr3.ctl.*` 查询面 + rr3-ctl 实装，
语义裁定全文见 ADR-0010）。净增 **10** 条 + 独立工程 **12** 条：rr-registry lib 23→25——
`registry::tests` 2 条（`rr3_reg_health_snapshot` 的判定数回显：task 升序 + `since_beat_ms`
= `now - last_beat` 三数逐一对、`saturating_sub` 对 now < last_beat 夹 0 不回绕）+
rrd lib 105→108——`rpc::tests` 1 条（四个 ctl 方法过同一解析器且 `is_ctl` 判据与方法面一致、
模块四方法全部 `!is_ctl`）+ `registry::tests` 2 条并入上面 rr-registry 段、rrd 侧实为
`cli`/`manifest` 0 条；线级新目标 `rrd/tests/e2e_ctl.rs` **5** 条（整体 `#![cfg(unix)]`，
Unix 独有）：status 列出真注册模块且 **ctl 连接自己不得进快照**（裁定 2）+ 应答后连接被收掉
（一问一答即断）、health 回显条目自己的 heartbeat/watchdog 配置与 since_beat、logs 环里
必须有启动横幅（`rr3_log` 单点落环的线上实证）、ota 恒拒 `Permission(-22)` 且文案点 M6
（契约冻结集不自造 NotSupported）、已注册连接上行 ctl 被 `Permission` 拒但同一条连接
心跳照常被接受（一次笔误不掉线）。本轮 ①b：Unix **279**（实跑 = F2 的 269 + 平台无关 5 +
Unix 独有 5；逐目标：rr-bus 10 + rr-hal 13 + rr-ipc lib 38 + `envelope_meta` 4 + rr-ota 5 +
rr-registry lib 25 + 契约 29 + rrd lib 108 + 线级 `e2e_audio_chain` 1 + `e2e_ctl` 5 +
`e2e_depends` 3 + `e2e_sim_peer` 5 + `e2e_spawn_restart` 5 + `e2e_watchdog` 4 + `module_call` 4 +
`register_publish` 19 + `socket_banner` 1 = 279）/ Windows **226**（未实跑，按 221 + 平台无关 5
推算；`e2e_ctl` 整体挂 Unix 门不计入）。② 29 / ③ 5 / ②b 5 / ④ 9 个独立工程均无变化。
**②b 的姊妹注记**：rr3-ctl 的 12 条（含 2 条 fake-rrd 真 UDS 往返）属独立 workspace，
①/①b 打不到、④ 只 `cargo check` 编译不执行——与 ②b 把 mkmod 的产物喂回契约测试同一
精神，收口轮在本地实跑 `cargo test --manifest-path tools/rrctl/Cargo.toml`（本轮 12/12 绿）。
2026-10-01 F4（rr3-sim 完整化）同口径：`tools/sim` 3→**9** 条（drill 纯逻辑 7：参数解析 /
仓根标记搜索 / 缺二进制点名构建指引 / manifest 文本含绝对 entry 与真契约主题 /
status 表解析跳过非行 / 注入式轮询达成与超时点缺 / 演练身份在真契约里存在——
最后一条把"演练用真 owner 名"钉成机器可检查；devices 2 条原样），**base/runtime 不动，
①b 条数无变化**；编排（Unix-only）与纯逻辑分居两模块，9 条单测全平台可跑。
9/9 绿 + 全链路演练实跑 3/3 OK（见 tools/sim/README.md 五步脚本）。
2026-10-01 F6（shm transport 初版，ADR-0011）同口径：rr-ipc lib 38→**49**
（shm 11：核心 7——头布局常量自洽 / 单读者逐帧往返含大帧跨环折返 / 落后过半圈
重同步且 lost 计数 / 超半容量拒发且区域无损 / 多读者独立游标追读与停读分化 /
阻塞 recv_buf 等到迟到帧 / 坏头如实报 Header 不静默；POSIX 附着 4——cleanup 幂等 /
`O_EXCL` 未清理重复 bind 拒绝 / bind→accept→writer_conn 经真 `shm_open`+`mmap`
全链路往返含读者分化 / 附着缺失对象干净报错）。逐目标：Unix **290**（279 + 11）/
Windows **233**（226 + 核心 7 推算；POSIX 段 cfg 门控不参与编译，与 uds 的
cfg 边界同口径；无 Windows 目标未实跑）。② 29 / ③ 5 / ②b 5 / ④ 9 个独立工程
均无变化。依赖预算零增量：POSIX 附着为手写 6 函数 extern 面，不引 libc crate
（ADR-0011 裁定 5）。
2026-10-01 M4（rr3-brain 实装与四真进程对话闭环，ADR-0014）同口径：①b rr-hal
sim.rs 7→**8**（burst 波形：解析入取值集 / 偶秒门控确定性 / 第 3 秒复现第 1 秒且
有声段与 Sine 同序号逐点相等——恒有声的 sine 全程只发一次 onset，晚注册的 brain
赶不上；burst 1s 说/1s 停让 VAD 周期性发 onset/offset），Unix **291**（290 + 1）/
Windows **234**（233 + 1 推算，sim.rs 平台无关；未实跑）。② 29 / ③ 5 / ②b 5 无变化；
④ tools/sim 9→**12**（brain 演练 3 条：三份 wire manifest 全量形状与绝对 entry /
shim 脚本与演练 config 形状 / brain 演练身份与主题在真契约里存在；
`rr3_require_bins_ext` 并入既有"缺二进制点名构建指引"用例）；rr3-brain（Python
工程，stdlib unittest，本地实跑）**26** 条（test_bus 10 + test_cloud 9 +
test_dialog 7）。e2e：`rr3-sim --brain` 四真进程全语音对话闭环（rrd +
rr3-sense(sim burst) + rr3-brain + rr3-interact，云端 mock——唯一假的一跳是云端
socket）实跑 2/2 OK，判据：三模块 running + brain 日志「对话轮完成」+ interact
日志「tts:」+ `denied_publish = 0`。
2026-10-01 T1（能力格坐标系落盘，ADR-0012）同口径：rr-registry lib 23→**26**（slots 3：
契约解析形状 20 格/层分布 5-11-4 / 坏文本给空表且空表报错 / 形状检查复用 capability
规则函数抓非法名与非法层），①b Unix **294**（291 + 3）/ Windows **237**（234 + 3
推算，slots.rs 平台无关；未实跑）。② 29→**33**（查册门 4：基础层与
`base_capabilities` 钉死等价 / 格位表形状 pin 全部 20 条 `(name, layer)` 在册且层分布
5/11/4 / manifest provides+depends 双侧查册 / default profile bindings-faces 装配自洽）。
③ 5 / ②b 5 / ④ 9 个独立工程均无变化。
2026-10-04 T3（rrd 装配链——`--profile` 装机正门 / faces 决定起谁 / 查册门 / 缺格 fail-closed，
ADR-0009/0012 与 0.0.2 方案 Q3 裁定）同口径：rrd lib 108→**122**——新模块 `profile.rs` 11 条
（解析形状错误全收集逐条点名 / 缺段与空 faces / 坏 TOML 单错 / 满配 manifest 按 modules-root
解析且升序（比对基准用规范化后根目录：macOS `/tmp` 是符号链接，拿原始路径比会红在系统布局上）/
缺 binding 只缺点名模块不连坐 / binding 被他人认领=不自洽致命 / 基础层进 bindings 致命 /
faces 引未在册格位致命点名 / 基础层进 faces 只记账不缺格 / 装配单文件不存在的绝对化文案）
+ 新模块 `probe.rs` 1 条（sim 后端对任何格位回在位——缺格只由装配单缺 binding 表达）
+ `cli.rs` 14→16（`--profile` 默认 None 且与叠加层并存 1、与 `--manifest` 两种顺序同给
都在 parse 处互斥拒 1）。新线级目标 `rrd/tests/e2e_profile.rs` **3** 条（整体 `#![cfg(unix)]`，
Unix 独有）：P1 满配（装配单凭证行 + 三条 spawn 行回显夹具 entry + ctl.status 轮询三模块全
running）/ P2 缺格（点名 `拒绝启动 rr3-pb：缺格 sensor.env` + pa/pc 照常 spawn +
300ms 窗否定断言 `spawn rr3-pb pid=` 不出现——缺格≠拒 boot）/ P3 非法装配单（faces 引
未在册格位 → bind 之前非 0 退出 + stderr 点名未在册 + 套接字无残迹；直接 `Command` 起 rrd，
管道收两端）。①b Unix **313**（macOS arm64 实跑；上轮 T1 登记 294，本批净增 17
（rrd lib 14 + e2e_profile 3），另 2 条为 T1 轮登记的历史口径差——按本轮逐目标复算上轮实数
应为 296，不追溯改史，以本轮实数为准）/ Windows **251**（237 + 14 推算，本批 lib 用例全部
平台无关；`e2e_profile` 挂 Unix 门不计入；未实跑）。② 33 / ③ 5 / ②b 5 / ④ 9 个独立工程
均无变化。
2026-10-05 T4a（rr3-act 实装——舵机/LED/继电器三事务面 + 进程内反射环宿主，
ADR-0007/0012 与 0.0.2 方案 Q1 裁定）同口径：rr-hal lib 14→**17**（+3：sim 舵机
记录末次落角不判行程 / sim 数字输出电平 round-trip / act 设备选择入口回合法后端
对象）；新线级目标 `rrd/tests/e2e_act.rs` **1** 条（整体 `#![cfg(unix)]`，Unix 独有）：
装配单正门起**真 rr3-act**——夹具用仓内真 module.toml 渲染绝对 entry 并**同树写
config.toml**（rr3_config_load 缺省取 manifest 同目录的 config.toml，缺它模块开机
exit(1)，那是夹具烂不是被测行为），判据链：spawn 行回显绝对 entry → 注册 → running →
旁观连接 rr3.call 三事务面（servo 合法角应答 angle_deg/move_ms=800（缺 move_ms 取
config 上限）+ servo/state 限速流 seq=1、src_task=分配值——先读 state 后读应答，
同 socket 上应答与广播不保序，read_call_reply 不回存队列故顺序不可反；越界 200 /
缺参 / 非布尔 `{"on":1}` 折 result.error 不断链；led blink 应答 effect/previous；
relay 吸合回显真值）。真模块夹具 5 函数（repo_root / build_module_bin /
module_manifest_text / connect_observer / read_call_reply）自 e2e_audio_chain 上收
`tests/support`，后者断言一字未动仅换 helper 来源。①b Unix **317**（macOS arm64
实跑；上轮 T3 登记 313，本批净增 4（rr-hal 3 + e2e_act 1））/ Windows **254**
（251 + 3 推算，rr-hal 新用例平台无关；`e2e_act` 挂 Unix 门不计入；未实跑）。
② 33 / ③ 5 / ②b 5 无变化；④ 9 个独立工程均无变化。rr3-act 独立工程单测（④ 步
只 check 不跑测，按 rr3-ctl 先例单独登记）**18** 条：servo 5（参数解析正例含缺
move_ms 取上限 / 缺参·类型·越界点名 / move_ms 超上限与 0 拒收 / 限速门首放-窗内挡-
窗外开（now 合成注入）/ state 体 seq+angle）+ led 5（白名单解析含垃圾拒 / 缺参·
非串点名 / apply 记账 current 与 previous / on·off 即时生效（有界轮询）/
blink 翻转与 off 停翻（Arc 克隆观察 worker 写面））+ reflex 4（窗计划推导 /
episode 恰报一次 / need 下界为 1 / 孤立饱和窗不触发）+ relay 2（只认真布尔 /
apply 落电平且应答回显）+ main config 解析 2（仓形同构可解析 / 缺段缺字段点名）。
2026-10-05 T4b（rr3-power 实装——battery/state 周期流 + battery/low 边沿事件 +
charger/state，rrd 低电兜底四件动作面，0.0.2 方案 Q4 裁定）同口径：rr-hal lib
17→**19**（+2：sim 电池三元组读回与电压曲线两端钉死（3200mV + 10mV/1%）/ 电池
env 取值裁决纯函数（percent 0–100、charging 只认 1/0，未设缺省 80/未充电））；rrd
lib +2：registry 全局低电旗（上升沿恰一次、只升不降——与 denied_publish 同款
"conn 与 supervisor 两路共见"守护态）+ daemon 低电点名帧形状钉死（Notification /
src=rrd / dst=广播 / 体恰四字段——e2e 旁观者接入时首拍已过，帧形状只有单测能钉，
两半拼成完整证据链）；新线级目标 `rrd/tests/e2e_power.rs` **1** 条（整体
`#![cfg(unix)]`）：装配单正门起**真 rr3-power**，env 经夹具新通道
`rr3_start_profile_env` 注入（`RR3_HAL_BACKEND=sim` 显式不赌平台默认 +
`RR3_HAL_SIM_BATTERY_PERCENT=5` → 3250mV ≤ 3300mV 首拍越阈），判据链：兜底触发行
含透传值与点名广播（服务侧真相，conn.rs 钩子先扇出后打日志，该行即"广播已发"
界标）→ 旁观者收 battery/state（percent=5 / voltage_mv=3250 / src=分配值 / seq
严格递增）与 charger/state（charging=false 缺省语义）→ pkill 真模块 →
`低电兜底：拒绝 spawn` 日志 + 1500ms 窗否定断言 `spawn rr3-power pid=`（拒绝行
不含 pid= 不误触）→ 末尾破坏性收尾计数：battery/low 与 system/power 各恰 **0** 帧
新增（"同段恰一次"模块侧边沿机与 rrd 侧上升沿的双重保险，重启真发生了则新实例
首拍再发、现形于此）。①b Unix **322**（macOS arm64 实跑；上轮 T4a 登记 317，
本批净增 5（rr-hal 2 + rrd lib 2 + e2e_power 1））/ Windows **258**（254 + 4 推算，
rr-hal 与 rrd lib 新用例平台无关；`e2e_power` 挂 Unix 门不计入；未实跑）。
② 33 / ③ 5 / ②b 5 / ④ 9 个独立工程均无变化。rr3-power 独立工程单测（④ 步只
check 不跑测，按 rr3-act 先例单独登记）**7** 条：低电边沿 2（越阈首拍恰报一次 /
严格大于才重新武装且武装本身不触发——武装若回 true 就会发出电压高于阈值的"低电"
事件）+ 事件体 3（state 全摘要带自流 seq / low 体恰三字段对齐 schema
additionalProperties=false / charger state 恰两字段）+ config 解析 2（仓形同构
可解析 / 缺段缺字段点名）。
2026-10-05 T4c（rr3-connect 实装——net/link 状态机周期流 + cloud/session 应答面 +
HAL Rr3Link sim 后端，0.0.2 方案 D7）同口径：rr-hal lib 19→**21**（+2：sim 链路
三元组读回且断链时设备面强制抹掉 ssid/rssi——模块侧不许从死链里读出网络信息 /
链路 env 取值裁决纯函数（up 只认 1/0、rssi 只认 -100..=-1，未设缺省在线 / rock-sim /
-50））；新线级目标 `rrd/tests/e2e_connect.rs` **2** 条（整体 `#![cfg(unix)]`，
Unix 独有）：装配单正门起**真 rr3-connect** 的两场景对偶——在线（sim 缺省不注入
UP 键钉缺省语义：net/link 报 up=true / ssid=rock-sim / rssi=-50，seq 严格递增的
周期流；cloud/session open 回 mock 会话应答回显 ssid——云侧凭据联调范围外，
`RR3_BRAIN_CLOUD=mock` 注入点维持）与断链（`RR3_HAL_SIM_LINK_UP=0` 注入：net/link
报 up=false 且 ssid/rssi 为 null——设备面抹掉语义的集成面；session open 折
result.error 点名"链路"，业务性失败不断链）。sim 后端状态 env 定死、运行期不可
跨进程翻转，翻转分支只有 wifi.rs 纯函数单测能钉（e2e_power 的时序前提同款分工）。
①b Unix **326**（macOS arm64 实跑；上轮 T4b 登记 322，本批净增 4（rr-hal 2 +
e2e_connect 2））/ Windows **260**（258 + 2 推算，rr-hal 新用例平台无关；
`e2e_connect` 挂 Unix 门不计入；未实跑）。② 33 / ③ 5 / ②b 5 / ④ 9 个独立工程
均无变化（rr3-connect 由骨架变实装属 ④ 步既有目录，check 口径不变）。rr3-connect
独立工程单测（④ 步只 check 不跑测，按 rr3-act 先例单独登记）**6** 条：链路状态机 2
（首拍基线必发-同态不发-翻转必发的主语义链 / 事件体全状态带自流 seq 且断链时
ssid/rssi 必须是 null）+ 会话面 2（args 只认 action=open 且缺参-类型错-未知值点名 /
result 跟随链路 up 回 mock 会话、down 折 error 点名判据词）+ config 解析 2（仓形
同构可解析 / 缺段缺字段点名）。
2026-10-05 T5（发行版快路轨 deb——tools/rr3-pack 打包器 + units/rrd.service +
profiles/distro.toml + CI distro-smoke，0.0.2 方案 D8 / ADR-0013 裁定 5 首证）同口径：
rrd lib 124→**125**（+1：`--contracts` 运行期契约目录入口——deb 装机后编译期仓内路径
不存在，CLI 缺省 None 保持仓内开发零变化，topics/slots 共用 `rr3_contracts_dir` 解析、
`rr3_profile_boot_inputs` 签名参数化 slots 路径；parse 门四态：缺省 None / 显式值 /
缺参数带用法 / `-` 开头值拒）。①b Unix **328**（macOS arm64 实跑；上轮 T4c 登记 326，
本批净增 2：rrd cli contracts 用例 + rr-registry 契约测试 distro 装配单对账用例）/
Windows **262**（260 + 2 推算，两条新用例均平台无关——contract.rs 读仓内文件与 cli
纯解析）。② 33→**34**（+1：`distro_profile_binds_exactly_the_body_layer`——机型集合
钉住断言同步扩为 default+distro，distro 机对账三条腿：绑定面查册/provides 对账同
default 门、未绑非 base 格位**恰好等于** rr3-brain provides 全集（缺格边界一格不多
不少）、faces 仍覆盖全部在册模块含 brain 缺格声明）/ ③ 5 / ②b 5 无变化；④ **9→10** 个独立工程（新增 tools/rr3-pack，依赖 rrd lib 只为 manifest 解析
——解析真值只有 rrd 运行期那一册，打包器不带第二份读取逻辑）。rr3-pack 独立工程单测
（④ 步只 check 不跑测，按 rr3-act 先例单独登记）**5** 条：staging 全布局落位 + entry
装机改写字节级钉死 1（manifest 除 entry 引号内一个值外与注入原件逐字节一致；control /
postinst / 单元文件 / 装配单 / 契约内容同批断言）+ 注入产物与仓内真值缺件点名 1
（rrd 缺席 / 模块缺 bin / 无模块 / 缺 distro.toml / 缺 config.toml——模块开机即读
manifest 同目录 config，缺件必重启环，随包必带）+ entry 不可装机拒 1（绝对路径 /
`..` 逃逸段）+ 目录名与 manifest.name 不一致拒 1 + CLI parse 1。**entry 装机改写是
本批关键裁定落地**：线上注册门禁（conn.rs）要求 manifest.entry 绝对路径，而模块开机
读的就是装机 manifest 原文上报，相对值必被 AIRY_EINVAL 拒——打包器只改 entry 一个值
（modules/README.md"绝对 entry 由装机环节写入"的实体落点；装配单磁盘装载两侧绝对值
两头都过）。保底轨 post-build.sh 现状不满足该门（manifest 平铺 /usr/lib/rr3/manifest/
且零改写、bin 落 modules/<name>/bin 与相对解析错位）——保底轨"脚本就绪、实测=板上"，
改写留保底轨批次收口，不在本批越界。CI：`distro-smoke` job 新增（ubuntu-latest 单步
`docker run rust:bookworm`，glibc 自洽、本机产物装本机 deb）——构建→rr3-pack 单测→
组装注入产物（两轨同源 rr3-modules/<name>/{bin,manifest.toml}）→出 deb→
`dpkg -i`→装机位断言→rrd 装配单正门冒烟（spawn 5 模块 + rr3-brain 缺格点名（P2/D2
语义首证：快路 payload 无 Python brain，faces 保留声明、cognition.* 不绑定）+
0 拒 boot + `rr3-ctl status` denied_publish=0）。macOS 本机无法真装 deb（方案 P1），
真装只认该 job 远端回执；本地装配演练走 `rr3-pack --stage-only`。
2026-10-05 保底轨修复批（T5 段点名的保底轨缺口收口——entry 装机改写同一裁定落到
buildroot 轨，ADR-0013 裁定 5 两轨同源的第二面）改 `base/os/buildroot/board/radxa-zero3/post-build.sh`：
① manifest 不再平铺 `/usr/lib/rr3/manifest/<name>.toml`——归位
`/usr/lib/rr3/modules/<name>/module.toml`（旧布局下模块 rr3_config_load 读 manifest 同目录
config 必撞同名文件，且 bin 的相对 entry 解析到 manifest 所在目录必错位）；② entry 装机
绝对化（与 rr3-pack `rr3_manifest_install_rewrite` 同语义同三道门：entry 行恰一条
`grep -c`、值可装机 `case` 拒绝对路径 / `.` 与 `..` 段 / 含 `|` 分隔符、改写后 sed 重提
复核）——只改引号内一个值，其余字节原样；③ config 从仓源 `modules/<name>/config.toml`
随装（env `RR3_REPO_ROOT` 可覆盖仓根，缺省按脚本位置上溯五级）；bin 改按 entry 落位
（注入格式固定名 `bin` → entry 指向的装机相对路径）；S99rrd 的 manifest glob 同步改
`/usr/lib/rr3/modules/*/module.toml`。**缺件口径收紧一处**：缺 bin / 缺 manifest / entry
不可装机 = 警告 + **整模块跳过**（manifest 一并不装）——"缺席只警告不阻断"仍指镜像结构
可构建，但缺件还装 manifest 会让 rrd spawn 一个必退出的模块（重启环）；跳过的是模块，
不是镜像。config 缺席仍只警告（镜像可构建，板上收口）。CI：`buildroot-assembly` job 新增
（ubuntu-latest 单步，纯 bash + 系统工具，夹具侧模拟 Buildroot 目标根的 etc/init.d，
不需要 Docker）——`tools/ci/buildroot-assembly-smoke.sh` 断言装机布局 / entry 绝对化且
除 entry 行外与仓源逐字节一致 / bin 落 entry 名可执行 / config 与仓源一致 /
旧平铺位不存在 / S99rrd 新 glob / 两类坏模块（缺 bin、无 entry 行）警告且整模块未装。
本批零 Rust 改动：①b / ② / ②b / ③ / ④ 全不变。
2026-10-05 D9 装配演练批（0.0.2 T6 前半——rr3-sim `--profile` 装配演练模式）：
`tools/sim` 新增 `assemble.rs`，装配单驱动 rrd 装配链正门（`--profile` + `--contracts`，
与 rrd.service 的 ExecStart 同形）boot 全部 face 模块，演练 modules-root 是**装机布局**——
仓源 manifest 原文只把 entry 改写为产物绝对路径（rr3-pack `rr3_manifest_install_rewrite`
同语义同三道门：装机值可装机 / entry 行恰一条 / 旧值与 facts 交叉核验 / 改写后重解析
复核，"entry 在仓相对、装机绝对"的第三面）+ config 同目录就位（Rust 模块仓源 config，
brain 用 M4 先例 mock 演练 config）；python 模块产物 = shim（`python3 -m`，包名 = 目录名
连字符转下划线）。配套：① rrd `Rr3ManifestFacts` 加 `lang` 字段（演练产物推导要它，
解析真值保持一册——不逼调用方另开 TOML 读取；req_str 缺失即点名）；② `which_python3`
从"PATH 第一个"改为**能力验证**（`import tomllib` 成败 = brain 运行门槛的直接证据，
不满足的候选继续向后找、全不满足才报错点名——macOS 本机 3.9 从"60s 超时红"变
"当场点名红"，--brain 同益）；③ sim 依赖 rrd（path，借两处解析真值，rr3-pack 同款先例）；
④ `--brain` 与 `--profile` 同给即拒（Q3 同款）。CI：`sim-assemble` job 新增
（`tools/ci/sim-assemble.sh` 构建 + 运行同一脚本）——**D9 六模块判据（含 brain）归
该 job 远端回执**：macOS 本机 python3 3.9 不过 tomllib 门（无 brew / uv / pyenv 可装），
本机实跑记录：无 brain 子集装配单（5 Rust 模块）装配演练全链绿（装配 boot → 全
running → `denied_publish = 0` → 四命令巡检 ota 预期拒，exit 0）+ default 满配单在
brain 门槛处 fail-fast 点名；六模块真进程判据与 P1 同理（distro-smoke 先例）不据本机
声称。测试面：sim 独立工程单测 12→**16**（+4：entry 改写钉字节 + 三道门拒绝形状 +
夹具布局装配 + **仓内 default faces 装配就绪对账**（六模块 manifest 可解析 / name ==
目录名 / entry 仓相对无 `..` / config 在——装配单↔模块目录漂移让单测红，不等演练跑
一半红）；rrd 无新增用例（lang 是 facts 解析面的自然扩展，既有 manifest::tests /
registry::tests 的文本夹具全带 lang 字段照过）。①b Unix **328** / Windows **262**
不变；② 34 / ③ 5 / ②b 5 / ④ 10 个独立工程不变（tools/sim 加依赖不改工程数）。
2026-10-05 D10 文档收口批（0.0.2 T6 后半——status.md 重锚 + reference 三件与代码同批，
方案 P4"只汇总重锚、不补账"）**零 Rust 改动**：①b / ② / ②b / ③ / ④ 全不变；⑤ 因新增
互链重跑全绿。两件事：
① [status.md](../status.md) 整篇重写（锚点 `540ec0f` → `363e4ed`）——T3 装配链接线、
T4a/b/c 三模块实装、T4b 低电兜底、T5 deb 打包链、保底轨修复批、D9 装配演练全部按锚点树
实况计入能力表（"能力格坐标系"行并入装配链 e2e 三条、"Buildroot OS 层"行改写 post-build.sh
装机布局、新增"发行版快路轨 deb 打包链"与"rrd 低电兜底"两行）；M5 行按 Q2 口径如实改写
（shm transport 达成、三模块 sim 侧实装、"语音控继电器"最后一跳 = 意图→动作映射属策略层
未做 + 板上证据零 = 未收口两条敞口）；"本机无法收口"表第 1/5 条随双远端与 0.0.2 条数更新、
新增第 8 条（deb 真装只认 distro-smoke 回执）与第 9 条（六模块真进程只认 sim-assemble 回执
——本机 python3 3.9 不过 tomllib 门）；在制品段枚举 `540ec0f..363e4ed` 全部 12 提交。
② reference 三件同批：[rrd-cli.md](../reference/rrd-cli.md) 补 `--contracts` 四处
（§1 USAGE 逐字含说明句 / §2 参数表新行 + `RrdConfig` 字段列 / §4 路径绝对化分工新增
"例外面"行——目录不单独规范化、存在性由 join 出的 topics/slots 各自 canonicalize 把关 /
§5 启动序第 5 步契约定位与第 7 步致命面补"格位契约读不到"）；
[rrd-registry.md](../reference/rrd-registry.md) §1 `RrdRegistry` 字段列补 `low_power`、
§10 补 `rr3_reg_low_power_raise` / `rr3_reg_low_power_is_set` 行（上升沿恰一次、只升不降
不设解除口、spawn 漏斗唯一消费者）；[rrd-rpc.md](../reference/rrd-rpc.md) §7 发布处理序
补低电兜底钩子段（挂转发点之后、兜底是额外动作不截断事件流、两段日志逐字）。
同批订正 [.github/README.md](../../.github/README.md) 的实况漂移（四 job → 七 job 清单、
"主远端已迁 gitcode" → 双远端口径）——T5/保底轨/D9 三批挂 job 时该文件漏同步，此处结清。
2026-10-05 0.0.3 批A（0.0.3 收口方案落档 + legacy 处置裁定入规约，Q1–Q4 裁定记录随
方案 §2 落档）**零代码改动，纯文档批**：新增
`docs/process/dev/0.0.3版本洛克三号收口方案.md`（M5 最后一跳 + M6 OTA 写侧主机面 +
legacy 盘点三交付，WBS T1–T5）；[§5](#5-文档纪律) 增补 legacy 处置三阶段口径
（只读消化 → 融合完成 → 退役，替代旧"冻结区不动它"；`docs/process/legacy-digest.md`
在 T2 落档时此处挂链）；docs/README.md 索引切换（0.0.2 行退历史存档、0.0.3 行
为当前版本，两处表同批）。①b / ② / ②b / ③ / ④ 全不变；⑤ 因新增文档与互链重跑全绿。
2026-10-05 0.0.3 批B（D2 legacy 语义盘点落档，Q4 裁定的台账面）**零代码改动，纯文档批**：
新增 [legacy-digest.md](legacy-digest.md)——legacy/core 九组件 + 三棵试验树逐项三分类
（已融合 7 / 待融合 3：data 计数、panel 面板、btctl 蓝牙链路，去向与排期 0.0.4 消化批 /
退役 5：firmware-c 宿主、firmware-rs、rust、link/eth、link/nearlink），每行带证据路径；
退役触发条件与退役批四件收尾（⑤ 门排除撤除、architecture §8 标注、status 已知偏差注销、
本表归零）在册。同批：§5 挂真链（替换批A 的占位句）、architecture §8 尾注挂链、
docs/README.md 两处表挂导航。①b / ② / ②b / ③ / ④ 全不变；⑤ 因新增文档与互链重跑全绿。
2026-10-05 0.0.3 批C（D3 M5 最后一跳，T3 补裁定随方案 §2 落档）：
**契约 append 三件**（`contracts/topics.toml` 第 25 条 `rr3-act/relay/state` event L2——
0.0.2 Q1 "留真实需要时再 append" 的真实需要此刻成立：模块 stdout 接 `Stdio::null()`，无状态
主题则意图执行效果在 e2e 层不可观测；`events/brain-intent-result.schema.json`；
`events/act-relay_state.schema.json`），形状 pin 三处同批（registry `tests/contract.rs`
25 条三元组 / kind 8-10-3-4、registry `src/topics.rs` 25、rrd `src/topics_load.rs` 25+28）；
**brain 侧**：新增 `rr3_brain/intent.py`（user_text → actions[]，精确词表 + 两字符否定护栏，
3.9 兼容语法）+ `tests/test_intent.py` 6 条，`__main__.py` 发布载荷并 `actions`；本机
python 3.9.6 实测 23 条中 22 过（`test_bus` 一条 import error：tomllib 需 py≥3.11，
D9 已登记门槛非本批引入），CI brain-unittest 钉 3.12 跑全量 23 条；**act 侧**：新增
`src/intent.rs`（两道归属门：exposes 归属 + 意图面开放，拒执行折日志不断链）5 条单测、
`relay.rs` 增 state 主题常量与摘要体（形状对齐 schema）1 条单测、`main.rs` RPC 与 intent
两条驱动路接线共用 `relay_seq` 流序、`module.toml` publishes 增 `rr3-act/relay/state`——
模块单测 24 条全绿（增 6）；**e2e**：`rrd/tests/e2e_act.rs` 扩 RPC 路状态回读（先读 state
后读应答，`rr3_read_call_reply` 不回存）+ brain 在册身份发 intent 事件 → relay 落电平 →
`relay/state` 回读（seq 跨两路连贯）+ 越权动作有界窗口否定断言；同批文档：方案三处回改
（§1.2 D3 行、§4 契约句"零改动"→"一条 append"、§6 DoD）、architecture §6.3 对账注记与
§7.2 草图注订正（relay/state 已在册）、act/brain README 目录树与总线面表同步。
**本批门禁增量钉子**：registry lib 单测与 rrd lib 单测各含一份与 `tests/contract.rs`
独立的形状 pin（`include_str!` 直读契约），append 主题必须三处同批改——批C 首跑红两条
（24≠25）即为该钉子咬合，同批结清；批B 遗留断链一处（legacy-digest.md 的
vad_start schema 链少一级 `../`）以独立 docs fix 提交先于本批落库。①a–⑤ 全绿重跑；
brain-unittest 与 lint 归 CI 侧。
2026-10-05 0.0.3 批D（D4 M6 OTA 写侧主机面，Q3 裁定落地）：
**rr-ota 增 `bootconf` 模块**（可测逻辑归 base/runtime workspace 随 ③ 全量跑测试；
CLI 薄壳）——`switch`/`factory` 判定与改写（只动生效行 root=LABEL 标签、注释与其余
字节一字不动；同槽幂等拒 / rr_factory 目标拒走 factory / 缺·歧义·杂牌 token 报形状
漂移拒）、原子落盘（读 → 改写 → 同目录临时文件 → rename）、cmdline 当前槽解析
（只认 `root=LABEL=rr_`，裸设备形态报"板上核对项"不覆盖）；`Rr3Slot::rr3_slot_from_label`
反查 + roundtrip 测试；真工件 `include_str` 字节级对拍（extlinux.conf 可逆律 / 对合律 /
长度差恰为标签差），单测 8 条新增、crate 13 条全绿。**首跑红两处即钉子咬合同批结清**：
①rr3_root_spans 初版把 extlinux.conf 注释行里的 `root=LABEL=rr_a` 也扫进来（2 处→歧义
拒）——改为逐行扫并跳过 `#` 注释行，span 收窄为标签字节区间（rewrite 不再重造
`root=LABEL=` 前缀，初版丢前缀缺陷一并消除）；②两条测试断言同步"注释一字不动"语义。
**rr3-ota-ctl 重写**（恒拒骨架 → 实装）：slots / switch / factory 三子命令，
`--bootconf` 必传不缺省碰任何路径，退出码 0 成功 / 1 业务拒 / 2 用法错；恒拒用例
`switch_is_always_refused_until_m6` 改写为实装语义三件（slot_arg 在册映射+杂牌拒、
CLI 壳临时文件端到端+同槽业务拒不落盘、bootconf_arg 硬门槛），4 条全绿；CLI 实跑冒烟
（slots / switch 落盘 / 同槽拒 / factory 目标拒 / factory 落盘 / 缺参拒 / 临时文件零残留）
逐项对。**文档三件同批**：partition-layout.md 增补"切换与回滚链"段（切换语义、U-Boot
bootcount/bootlimit/altbootcmd/upgrade_available 标准语义表、断电安全序七步、板上核对项
清单七条——首条即 vfat rename 原子性弱于 unix 文件系统的诚实敞口）；extlinux.conf 注释
订正（"由 rr3-ota-ctl 改 U-Boot 环境变量"→实装口径"改写本文件 append 行标签"）；
base/ota/README.md switch 恒拒段改实装口径 + 边界声明（本批交付切槽"翻标志"一步；
槽位镜像写入随升级下载链另批；回滚链与块设备探测属板上件不 claim）；rr3-ota-ctl
Cargo.toml description 同步。①a–⑤ 全绿重跑。
2026-10-05 0.0.3 批E（0.0.3 收口汇报批，纯文档）**零代码改动**：docs/status.md 整篇重写重锚
`363e4ed` → `68220e4`（M5 行改"已收口（sim 侧；板上敞口同 M3）"——批C 意图链闭环计入；
M6 行改"主机面已实装，板上件与升级链未做"；A/B OTA 行骨架 → 已实装（主机面）；schema 行
1 → 3 件；"本机无法收口"新增第 10 条 M6 板上件、第 7 条并 M3/M5 板上证据、第 1 条指针
双端核对口径；在制品段补批A–D 五提交；legacy 已知偏差措辞随 §5 三阶段口径更新——"冻结区
不动它" → "只读消化阶段不改本体，退役批处置"）；本条登记自身。①b / ② / ②b / ③ / ④
全不变；⑤ 因 status/本文件互链重跑全绿。0.0.3 五批次（批A 方案落档 / 批B legacy 盘点 /
批C M5 最后一跳 / 批D M6 写侧主机面 / 批E 重锚汇报）至此全部落库。
2026-10-05 0.0.4 批A（0.0.4 消化方案落档，Q1–Q6 裁定项随方案 §2 在册待裁）**零代码改动，纯文档批**：
`docs/process/dev/0.0.4版本洛克三号消化方案.md`（legacy 待融合三项收敛 + 退役批：D2 data
计数/持久化、D3 panel 本体、D4 配网面、D5 btctl 骨架、D6 整树退役删除（Q6 显式裁定前置）、
D7 收口；WBS T1–T7 与版本 DoD 随文）；docs/README.md 索引切换（0.0.3 行退历史存档、
0.0.4 行进当前）。①a–⑤ 全绿重跑。
2026-10-05 0.0.4 批B（data 消化——心跳 counters 双端镜像 + rrd 聚合持久化 + `rr3.ctl.counters`，
Q1/Q2 裁定落地）**新增 5 条单测 + 2 条 e2e**：rr-bus client（`client.rs`）心跳每拍快照
`rr3_counters_snapshot`、`rr3_counter_bump` 记账，非空才带 `counters`（新用例
`heartbeat_carries_counters_only_after_bump`，lib 实测 11 条含新用例）；rrd 新模块 `counters.rs`
（路径三态 + 原子写序 tmp→fsync→rename，5 单测）、`registry.rs` 加
`rr3_reg_counters_replace/bump/merge/snapshot` 四方法
（merge 按 `模块名/键` 前缀归并、饱和加法）、`rpc.rs` 加 `CtlCounters` 变体 +
`rr3_rpc_heartbeat_counters`（脏值逐对静默跳过）——**九方法白名单**（控制四→五）；
`conn.rs` 心跳分支聚合+write-through（锁内归并锁外写盘，Rejected 之后也聚合），
`daemon.rs` 启动装载 + boots 自增回写（`RR3_DATA_DIR` 优先 → linux 缺省 `/data/rr3` →
皆无不持久化只点名）；brain `bus.py` 逐语义镜像（`_heartbeat_params` 独立成法供测试直调，
manifest 1h 心跳拍等不来真拍）+ `__main__.py` 四计数点，act `relay.rs` 键名唯一落点 +
`main.rs` 两条驱动路记账。文档：rrd-rpc.md（§1/§2/§3 九方法口径、心跳 counters 可选字段、
ctl.counters 行）、rrd-cli.md §2.1（RR3_DATA_DIR）、brain/act README 计数键归属表。
①b/②/②b/③/④ 全绿重跑（e2e_ctl 7 条含新 2 条：聚合与跨重启持久化）；brain Python 本机
3.9 无 tomllib 起不了（既有现状，CI brain-unittest job 覆盖），py_compile 语法验证过。

2026-10-05 0.0.4 批C（panel 本体——interact std 手写 HTTP 九端点 + 单文件静态页 +
`rr3.ctl.reboot`，Q3 裁定落地）**rrd 侧新增 `CtlReboot` 变体**（`rpc.rs`，**十方法
白名单**：控制五→六；`is_ctl` + 显式 OR 臂两处共同看护不变），`conn.rs` 服务臂执行面
**双重门**（`cfg(target_os = "linux")` 且 env `RR3_REBOOT_OK=1` 才真跑 `systemctl reboot`，
缺省恒拒）——e2e 只钉拒绝路径（`ctl_reboot_is_refused_by_default_double_gate`），
解锁面留装机环境由 rr3.service 注入，ubuntu CI 编译 linux 面也拿不到真重启能力；
e2e_ctl 7→8 条。interact 侧：`panel/server.rs` std 手写 HTTP/1.1 子集（GET/POST +
Content-Length，**零新 crates.io 依赖**），九端点路由纯函数 + `Rr3PanelDispatch`
**无 Publish 变体**（面板不当发布者的类型面形态，单测
`hardware_control_is_bus_call_only_never_publish` 钉死）+ ctl 裸 UDS 短连（src=0 与
运维探针同权）+ HTTP 读面（半包/超时折 Err，头区 8KB / body 4KB 上界）+ 连接上限
8（超限 503）+ 监听面 env `RR3_PANEL_ADDR` 优先缺省回环（Q3）；beep/face 经 mpsc
动作通道由主循环串行代执行（sink/face 所有权不出主循环，与 TTS 点名天然互斥无锁），
selftest 并列三调各答各展示（无状态机不算编排，ADR-0007）；静态页 `include_str!`
内嵌四页签单文件（状态/硬件/系统/WiFi 批D 占位），`main.rs` 面板线程 + net/link
事件缓存，`module.toml` subscribes 加 `rr3-connect/net/link`（发布方批D 上线）。
interact 测试 12→17 条（九端点路由、越权静态断言、404/405/400、全量/半包读、
ctl 回路+错误信封）。文档：interact README（九端点表、监听面、局域网可信面声明）、
rrd-rpc.md（§1/§2/§3/§4 九→十方法口径、ctl.reboot 行、Permission 行）。
①b/②/②b/③/④ 全绿（rrd lib 15 条、e2e_ctl 8 条、interact 17 条），fmt/clippy 清零。

2026-10-05 0.0.4 批D（配网面——connect 三 RPC + AP 配网状态机 + 面板 WiFi 页，
D4/Q3 裁定落地）**契约 append 25→28 条**（`rr3-connect/wifi/scan` · `wifi/connect` ·
`wifi/forget` request L1，legacy/core/panel 配网面迁移承接：scan sim 模拟列表 +
`backend:"sim"` 诚实标注、connect 沿用 legacy `ssid`/`pass` 字段名与密码≥8 位语义、
forget 承接 `/api/factory_reset` 的 WiFi 语义且 ≠ M6 整机恢复出厂），topics.toml /
`module.toml` exposes / rr-registry 形状 pin（28 条三元组、kind 11-10-3-4）三处同批改
咬合（契约 34 条含 pin 门全绿；同批订正 rr-registry lib `topics.rs` 2 处与 rrd
`topics_load.rs` 3 处的同源计数断言——两档测试直读契约，25 的旧数字一处不改就红）；
`net/link` 事件体 append `mode` 字段（ap|station，
负载只增不改）。connect 侧：`wifi.rs` 新增 `Rr3Provision` 配网状态机（凭据内存
`Option<(ssid,pass)>`，无凭据=ap 有凭据=station，**模块内策略不进 HAL**——sim 是
设备不是策略，up/ssid/rssi 仍 HAL 如实报数；真凭据持久化板上后置重启即忘，
D4 判据如实标注）+ 三 RPC 参数/结果裁决（session.rs parse_args/result 分离范式，
"AP 态可模拟进出"由纯函数单测 `provision_roundtrips_ap_station` 钉死），
`rr3_link_body` 加 mode 参数；`main.rs` 三常量 + match 四臂（session + 三 wifi），
业务性失败折 error 应答不变。interact 侧：`server.rs` route 三臂（GET scan /
POST connect / POST forget，全部 BusCall 透传——参数裁决在 owner 模块，connect
body 原样透传零翻译），九→十二端点；静态页 WiFi 页签实装（扫描表 + 连接表单 +
忘记卡）+ 系统页"恢复出厂 WiFi"卡（两语义不合并文案点名）；connect 测试 6→10 条、
interact 测试 17 条（路由用例更新为十二端点逐一点名）。文档：contracts/README.md
演进登记（25→28）、connect/interact 两 README、architecture §6.3/§7.2 对账注记。
①b/②/②b/③/④ 全绿（rr-registry lib 26 条、契约 34 条、connect 10 条、
interact 17 条），fmt/clippy 清零。
2026-10-05 0.0.4 批E（btctl 骨架——connect 蓝牙链路状态机 + 退役声明，D5/Q5 裁定落地）：
`bt.rs` 从 3 行占位升级为状态机骨架——`Rr3BtPhase` 三态（Off/Discoverable/Connected）
+ `Rr3BtState` 转移函数（power_on/link_up/link_down/power_off，非法转移返回 false
且状态不动，转移本体与后端无关、纯函数单测 `bt_phase_roundtrips_full_chain` /
`bt_illegal_transitions_are_rejected_without_moving` 钉死）+ sim 后端开机初值
env 注入（`RR3_HAL_SIM_BT_UP=1` 开机即上电进 Discoverable，缺省关=出厂不自动开，
取值不认一律按关——env 是 sim 注入口不是配置面；**【0.0.5 批I N-1 起退役：env 读已删，
初值改 config `[rr3-connect.bt]` up 数据化注入，缺省语义不变】**）；BlueZ 真后端板上后置（cfg 门，
方案 §1.3，本骨架不 claim 真射频链路）。**主题不登记**（方案 D5 显式裁定）：
btctl 的日志回放/远程命令语义已被 rrd 日志环 + `rr3.ctl.logs` 与 rr3-ctl 承接
（digest btctl 行在案），蓝牙链路暂无可发布事件——按"不预先占位"纪律（§3），
启用事件面前须先改契约。`main.rs` 开机 sim 初值接入 + 相位自述打印（蓝牙面
phase=off|discoverable）；`net.bt` 格位绑定核对在册（default.toml / distro.toml
两装配单 [bindings]/[faces] 均已绑定，本批零改）。digest btctl 行"待融合（低
优先级）"→"已收敛（0.0.4 批E）"+ Q5 退役声明（BlueZ 真射频随硬件采购后置板上
批次；data/panel 两行不动，三分类表清零归退役批 D6）；connect README bt 段落
同步。契约与形状 pin 不动（28 条）。①b/②/②b/③/④ 全绿（connect 12 条），
fmt/clippy 清零。
2026-10-05 0.0.4 批F（legacy 退役批——整树删除 + 四件收尾，D6/Q6 裁定落地）**零代码
改动，纯退役批**：`legacy/`（ESP32 冻结区，100 文件）整树删除——待融合清零确认在案
（panel 0.0.4 批C+D 实装、data 批B 实装、btctl 批E 骨架+Q5 退役声明；digest 三分类表
归零归档：已融合 10 / 待融合 0 / 退役 5）；删除前提满足（批E 提交 `586641d` 双远端
`ls-remote` 核对一致在案；git 历史 + tag `v0-pre-legacy` 双锚点回溯）。四件收尾同批：
① ⑤ 门 legacy 排除条款撤除（`check_docs` 的 `grep -v '^legacy/'` 与"为什么排除"注释
段——冻结区不存在后全集进门）；② architecture §8 标注消化完成（表尾注记改写，
行内 `legacy/...` 路径自此指 git 历史位置）；③ status.md"已知偏差"注销 legacy/README
断链条目；④ digest 头部/只读约束/汇总表/退役触发条件/消费指引归档化 + data/panel
两行补证据转"已融合"（data：批B 心跳 counters 双端镜像 + rrd 持久化原子写 +
`rr3.ctl.counters` 读面；panel：批C std 手写 HTTP 面板 + 批D 配网合流，两语义不合并
文案在册）。断链面同批清理：根 README 双语的 legacy 行改指 digest 归档台账、
docs/README 库地图三处描述更新。①b/②/②b/③/④ 全绿（测试条数全不变），⑤ 门以
删除后的 md 全集重跑；fmt/clippy 不涉及。
2026-10-05 0.0.4 批G（status.md 重锚 `68d0da9` + conventions 汇总收口，D7）**零代码
改动，纯文档重锚批**：status.md 按"收口时整篇重写"规矩重锚——锚点行 `68220e4`→`68d0da9`；
RPC 行"四方法"→"十方法（模块四 + 控制六）"（批B `rr3.ctl.counters` / 批C `rr3.ctl.reboot`
append）；主题注册表行补批D 25→28 append；**新增"运维计数面"行**（批B 全证据链：
client.rs 心跳镜像 + bus.py 同语义镜像 + rpc.rs 形状裁决 + registry.rs 聚合 +
counters.rs 原子写持久化）；sense/interact 行补面板证据（批C+D 十二端点 + 四页签）；
connect 行补配网面三 RPC（批D）与蓝牙骨架（批E，主题不登记理由在册）；rr3-ctl 行
改六方法口径（reboot 双重门 cfg(linux)+`RR3_REBOOT_OK=1`、rrctl CLI 子命令面仍四条
如实标注——两新方法的读写面走面板透传）；里程碑表头锚点随改 + M5 行补批D 配网面
收口；在制品段列 0.0.3 批E 与 0.0.4 批A–F 八提交（标题逐一照 git log 抄录）；
无法收口表：第 1 条指针随重锚更新、第 2 条 idf.py 密钥门**随 legacy 删除注销**
（验证对象不在工作树，固件线复活再重开）、第 3 条 key 轮换措辞订正（批F 只清
工作树，历史提交对象里 key 依旧在，提供方侧敞口不闭）。contracts/README 演进登记
批G 零契约改动（25→28 已随批D 登记），无需新改。①b/②/②b/③/④ 全绿（测试条数
全不变），⑤ 门全仓 md 全集重跑；fmt/clippy 不涉及。
2026-10-05 0.0.5 批A（0.0.5 对齐与裁定方案落档，Q1–Q4 裁定项随方案 §2 在册待裁）**零代码
改动，纯文档批**：`docs/process/dev/0.0.5版本洛克三号对齐与裁定方案.md`（0.0.4 消化收官后的
对齐批：D2 rrctl 四子命令→六子命令运维面对齐、D3 brain Rust 迁移设计裁定落档、D4 pcm
STREAM schema 与 shm 接线裁定登记、D5 收口；范围外显式列 rrd ctl.ota 维持恒拒 / pcm 不
冻结 / brain 实施批次不入 0.0.5；WBS T1–T5 与版本 DoD 随文）；docs/README.md 索引切换
（0.0.4 行退历史存档、0.0.5 行进当前，四档落点表 + 库地图两处）；根 README.md /
README_zh.md"当前版本"行订正（原指 0.0.2 属漂移，同批改指 0.0.5）。①a–⑤ 全绿重跑。
2026-10-05 0.0.5 裁定登记（Q1–Q4 逐项裁定，用户授权按 ADR 体系裁定，四问全按建议口径
通过；裁定依据逐条引 ADR 原文，全文见 0.0.5 方案 §2 裁定记录）**零代码改动，纯文档批**：
Q1 按建议——rrctl append `counters`/`reboot` 两子命令（ADR-0010 裁定 2/4/6：一次性查询
连接、已有数据源零新增状态、人读表渲染）；Q2 按建议且定死**新立 ADR-0016**（不增补
0014——决策不可变、新决策新编号）——brain 迁移 0.0.5 只落设计裁定，实施归 0.0.6 专项，
首件 bus.py → rr-bus 复用使 ADR-0014 裁定 1 镜像纪律自然退役；Q3 按建议——pcm STREAM
schema 维持空缺不冻结，冻结与 rrd shm 接线绑定同一份后续 ADR（ADR-0008 裁定 4"不写
半成品"）；Q4 按建议——远端 CI 宿主核实是用户侧唯一悬项，0.0.5 不排批，回执到手后立
结项批。`docs/process/dev/0.0.5版本洛克三号对齐与裁定方案.md` §2 裁定记录段落档。
①a–⑤ 全绿重跑。裁定后 T2–T5 依序解锁。
2026-10-05 0.0.5 批B（T2 rrctl 补 `counters`/`reboot` 两子命令——CLI 四→六与 rrd ctl
白名单对齐，Q1 裁定落地）**纯 rrctl 面改动**：`tools/rrctl/src/commands.rs` COMMANDS
4→6（`counters`/`reboot` append，与 `rr3.ctl.*` 白名单六方法逐一同名对齐，配套单测
钉死方法名不漂移）；`format_counters` 按模块前缀分组成 MODULE/KEY/VALUE 人读表
（`模块名/键` 拆前缀，rrd 自有键如 `boots` 无前缀归 `rrd` 组，空表打"无计数"，
计数非 u64 报错不渲染 `?`——与 status 的 cell 容错口径有意分开）；`format_reboot`
回执一行确认；**CLI 不加本地门**——reboot 的 rrd 执行面双重门（cfg(linux) +
`RR3_REBOOT_OK=1`）是唯一真值点，CLI 加门即第二真值，拒绝文案经错误信封统一
回显非 0 退出；`main.rs` 用法文案与模块头注释同步六命令口径。单测 12→19（新增
7 条：纯函数 5——方法名白名单钉死 / counters 分组渲染 / 空表 / 非 u64 报错 /
reboot 回执，fake-rrd 真 UDS 往返 2——counters 实答渲染、reboot 恒拒回显；往返
共 4 条含既有 status/ota）；
`tools/rrctl/README.md` 命令面与代码逐字对齐（六子命令 + counters/reboot 语义 +
"单测含四条真 UDS 往返用例"）。rrctl 独立工程不在七步门 ①/①b 作用域，④ 步
`cargo check` 覆盖编译；本地实跑 `cargo test --manifest-path tools/rrctl/Cargo.toml`
19 全绿 + fmt/clippy 清零（conventions §2 备注）后提交；七步 ①a–⑤ 全绿。
2026-10-05 0.0.5 批C（T3 brain 迁移设计裁定落档——新立 ADR-0016，Q2 裁定落地）**零代码
改动，纯文档批**：新立 [docs/process/adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md](adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md)
（**不增补 0014**——决策不可变、新决策新编号；0016 引用并收缩 0014 适用范围：
裁定 1 的 bus.py 逐语义镜像纪律随首件迁移退役，裁定 2/3/4 的云 API 形状 / 对话
回路归属 / e2e 编排语义继续有效）。四裁定：①切分 = 语义件渐进（首件 bus.py →
rr-bus 复用零协议代码，次件 cloud 三家，末件对话状态机；迁移件与 Python 对应物
同批一进一退，仓库任何时刻不存在双语言真值）；②功能语义不变量四条（对话闭环
与 mock 注入点 / 失败语义同契约 / 门禁契约校验不减 / config 键值不变形）；
③部署形态目标 = payload 去 python3 运行时（`lang=rust`、≥3.11 门槛消失、
`which_python3` 与 brain-unittest 随最后一件退役、OCI 分发格式裁定 0013 裁定 4
维持不动）；④实施归 **0.0.6 专项**（打包链/装配单/CI 三面联动属独立版本体量，
0.0.5 只落设计裁定，本裁定不等于实施解锁）。conventions §3 主语言条目加 0016
指针；docs/README.md 两处 ADR 区间 0001~0015 → 0001~0016（标题表补 brain 迁移
路线）。①a–⑤ 全绿重跑。
2026-10-05 0.0.5 批D（T4 pcm 传输裁定登记——contracts 演进段口径钉死，Q3 裁定落地）
**零代码改动，零契约改动，纯文档批**：`contracts/README.md` 演进登记 append
2026-10-05 条目——`rr3-sense/audio/pcm` 的 STREAM 负载 schema **维持空缺不冻结**，
原"冻结时机属 M5 shm 迁移裁定"表述升级为 **"schema 冻结与 rrd shm 接线绑定同一份
后续 ADR"**（依据 ADR-0008 裁定 4"不写半成品"：pcm_b64 JSON 是 UDS 承载期编码，
shm 零拷贝后大概率改环形直读，给注定换承载的形状写冻结件即半成品）。topics.toml /
events schema / slots.toml 一字未动（只增不改维持）；接线实施属多模块工程批届时
另立专项，冻结动作在专项内完成。status.md 能力表末行口径同步归 T5 重锚批（收口时
整篇重写，批内不动 status.md）。①a–⑤ 全绿重跑。
2026-10-05 0.0.5 批E（T5 文档收口——status.md 重锚 `c7b91e7` + conventions 汇总，
D5）**零代码改动，纯文档重锚批**：status.md 按"收口时整篇重写"规矩重锚——锚点行
`68d0da9`→`c7b91e7`；rr3-ctl 行 rrctl 侧改六子命令口径（0.0.5 批B `25e90b7` 证据
链：CLI 与 ctl 白名单六方法逐一同名对齐、counters 模块前缀分组渲染、reboot CLI
不加本地门）——原"CLI 子命令面仍四条 / 补子命令属演进项不阻塞"**注销**；能力表
末行 pcm 口径同步批D 裁定（维持空缺不冻结、冻结与 shm 接线绑定同一 ADR）；
`rr3-brain` 行规约注记补 ADR-0016 指针（实施归 0.0.6 专项）；里程碑表头锚点随改 +
M0 行"ADR 已写到 0016"；无法收口表：第 1 条指针随重锚更新、第 5 条补 0.0.5 批A–D
同口径；在制品段列 0.0.4 批G 载体与 0.0.5 批A–D 六提交（标题逐一照 git log 抄录）。
本版本 DoD 对账（0.0.5 方案 §6）：check.sh 七步全绿逐批达成、rrctl 六子命令一一
对应且有 fake-rrd 往返单测、裁定落档四问齐备（Q1–Q4 方案 §2 + 批B/C/D 落地）、
索引一致（docs/README 四档落点表 + 库地图 + 根 README 双语均指 0.0.5）、status.md
重锚完成——本批即 0.0.5 收口汇报，双远端指针一致随本批推送核对。①a–⑤ 全绿重跑。
2026-10-05 0.0.6 批A（方案落档与裁定——树莓派验证单主题，五题全按建议通过）
《0.0.6 版本洛克三号树莓派验证方案》落档（`process/dev/0.0.6版本洛克三号树莓派
验证方案.md`）：0.0.6 主题 = 树莓派真板验证跨平台快速部署可行性（ADR-0013 裁定 1/2
的字面兑现）。Q1 裁定 a——0.0.6 单主题，brain 迁移专项顺延归属 0.0.8、0.0.7 = ESP32
（ADR-0016 加修订记录只修排期，裁定 1/2/3 一字不动）；Q2——用户确认手头已有
Pi 4B / Pi 5，OS 按 Raspberry Pi OS Bookworm 64-bit；Q3——宿主交叉 +
`rr3-pack --stage-only` + 板上 `dpkg-deb --build`（零容器、零远端 CI 依赖）；
Q4——`profiles/distro.toml` 直用零代码改动；Q5——Radxa ZERO 3 采购顺延为可选
（status.md 敞口口径随 0.0.6 收口批重锚改写）。WBS 五批：A 方案落档 / B 交叉出包链
+ deb 版本 0.0.6 / C 部署手册 boards/raspberry-pi-64.md / D 板上部署演练 /
E 收口重锚。docs/README 索引同步（0.0.5 行转历史存档、0.0.6 行入四档落点表与
库地图两处）。落档不等于实施解锁，批B 起逐批过门禁。①a–⑤ 全绿重跑。
2026-10-05 0.0.6 批B（aarch64 交叉出包链 + deb 版本 0.0.6——Q3 裁定落地）
新增 `tools/deploy/pack-arm64.sh`（真板部署出包链，三步：交叉编译 → 注入产物
装配 → rr3-pack --stage-only + staging 布局断言）：交叉目标用
**aarch64-unknown-linux-musl 静态 + rust-lld**——零外部工具链（方案 §5.1 原写
gnu 目标，实施核实时发现 macos host 无 aarch64 glibc sysroot、`link-self-contained`
在 linux-gnu 目标不支持、brew Tier 3 无 bottle 装 GNU 工具链须源码编译，musl std
自带完整 libc 是唯一零安装路；产物全静态即 ADR-0013 裁定 2"静态二进制集"精神，
Bookworm 板上零依赖直跑，方案文字以本登记为准）。注入装配与 CI distro-smoke ③
同一两轨同源约定，不互抄装配；rr3-pack 本机宿主架构跑。deb 版本 0.0.2→0.6
三处：rr3-pack `RR3_DEB_VERSION` 常量（注释同步）、其单测 control 断言、
distro-smoke.sh 产物文件名。本机 stage 演练全绿：staging 布局断言含
"aarch64 静态 ELF 体检"与"entry 已装机绝对化"两条新断言；真装（scp 传板 →
板上 dpkg-deb --build + dpkg -i → 巡检）归批D，手册批C 落。rr3-pack 单测 5 条
全过、七步门禁全绿。
2026-10-05 0.0.6 批C（部署手册 boards/raspberry-pi-64.md——真板操作规程与实测
回填骨架）新增 `base/os/distro/boards/raspberry-pi-64.md`（boards/"一板一文件"
纪律首件）：镜像烧写（Imager + Bookworm Lite 64-bit、SSH 预配）→ 宿主出包
（pack-arm64.sh）→ 传板真装（板上 dpkg-deb --build + dpkg -i）→ 起链与巡检
（systemctl / rr3-ctl 三查 / reboot 自恢复）四段操作规程；§4 验收判据逐条引用
0.0.6 方案 §5.2（六条）；§5 已知边界（probe sim 未接线、RR3_HAL_BACKEND 板上
不设、真硬件批次归属、brain 不在包内）逐条诚实标注；§6 实测回填表留空待批D
填写（未测项不预填）。distro/README 的 boards 条目加指针。零代码改动，纯文档批。
①a–⑤ 全绿重跑。
2026-10-05 0.0.6 批D（板上部署演练——真板六判据全过，跨平台判据成立）
真板（pi@192.168.124.52，主机名 spharx，Pi aarch64 / Debian 13 trixie /
内核 6.12.47+rpt-rpi-v8）实测：宿主 `pack-arm64.sh` 出 staging（23 文件 / 12MB，
musl 静态产物零 glibc 依赖——板上 OS 比 §0 预期 Bookworm 新一个大版本，静态
二进制跨版本无影响，A-0013 裁定 2 兑现）→ 传板 → 板上 `dpkg-deb --build` +
`dpkg -i` 成功（版本 0.0.6，postinst 自动 enable）→ `systemctl start` 五模块
全 running、`denied_publish = 0`、cognition 四格缺格点名 + brain manifest 读取
失败双诚实边界（缺格点名条数按 faces 保留 brain 声明口径为四条，手册 §5.2
原写"dialog 恰一条"随实测订正）→ `rr3-ctl status/health/counters` 三查实值
（心跳 1000/2000ms 在窗、`rrd/boots = 1`）→ `reboot` 自恢复全绿。**判据 5
成立：本批零产品代码改动，产品树 diff 仅本文档与手册回填——"同一 payload、
不改一行产品代码，装进 Layer 0 完全不同的板"实测成立，0.0.6 主题命题闭环**。
真板首发现登记：`/var/run/rock3-ipc.sock = srwxr-xr-x root:root`（rrd root
umask，other 无 w 位）非 root 运维连接被拒——CI distro-smoke 全 root 掩盖；
本批 sudo 巡检完成验收，修复（bind 后 fchmod 或 service 运行时目录权限声明）
归独立小批，不塞本批。boards/raspberry-pi-64.md §6 回填 + 状态行改"已实测
收口"。①a–⑤ 全绿重跑。
批D 门禁留痕（流程失误订正）：批D 提交命令把 check.sh 接了 `| tail -3` 管道
再 `&&` 提交——管道退出码取 tail（恒 0），当时一次 e2e 时序测试偶发 FAIL 的
非零退出码被吞、明细被截断，提交仍继续执行（efd224f），违反"全绿才提交"。
订正：提交后三次全量复跑全绿（16 步全 PASS，含 ①b；"Broken pipe"为 e2e 进程
退出噪音非失败），FAIL 性质判定为时序 flaky、与本批文档内容无关；教训入册——
**门禁命令必须直接执行看完整输出（或 `set -o pipefail`），退出码不得经管道中转**。
2026-10-05 0.0.5 重裁轮批（用户裁定"重新进行一遍 0.0.5"落点为**增量重裁**：批A–E 产出
与 Q1–Q4 裁定记录一字不动，只增两板验证前置裁定）**零代码改动，纯文档批**：三方案件 +
收口面。①0.0.5 方案增补 §2.1 重裁轮（R1 0.0.6 两阶段重排——"零产品代码改动"收窄为阶段一
专属判据、阶段二 = 真机功能链 语音→意图→判向→转向→表情；R2 硬件采购敞口登记不阻塞开发
——麦克风/舵机/显示未到位走 sim 注入口先行；R3 boots 数据目录缺陷归 0.0.6 阶段一收尾件
（unit 注入 `RR3_DATA_DIR` + postinst 建目录 + smoke 断言补 counters 回写）；R4 云联调
mock 先行 + brain 上板 Python 直跑（trixie ≥3.11 过 tomllib 门，与 ADR-0016 排期不冲突）；
R5 0.0.7 四层验证梯子；R6 ctl socket root:root 现状如实登记、板上运维按 root/sudo、加组
放行另批）+ 裁定记录回填（六问全按建议通过）+ D6/D7/D8、T6–T8、P6–P8、DoD 三项随改；
②0.0.6 方案修订版 2（按 R1–R6 重排两阶段：阶段一 = 部署语义收尾（批B/C/D 已实测，余量 =
boots 修复 + boards 补录 ctl socket 现状）、阶段二 = 真机功能链四件 + brain 上板打包链；
§8 修订记录显式列"改了什么/没改什么"，Q1–Q5 裁定实质一字未动）；③0.0.7 方案新立（ESP32
系统级验证四层梯子：①契约 parity xtensa 编译期 const 断言 + golden 对拍 = IPC-T-003
第④路 ②UART transport 第三实现位 ③esp-rs std 哨兵 ④rr3-bridge 跨芯片 e2e；层①②必达、
③④按经典 ESP32 实测 RAM 裁、不可达则 S3 换板建议进采购敞口；Q1–Q6 待裁状态"待评审"）。
收口面：status.md 整篇重锚 `c7b91e7`→`c2beba8`——deb 打包链行补 pack-arm64.sh（0.0.6 批B
`5ecaa8d`：aarch64-unknown-linux-musl 静态 + rust-lld 交叉出包链，deb 版本 0.0.6）与真板
实测证据（0.0.6 批D `efd224f`：板上 dpkg-deb --build + dpkg -i、五模块全 running、reboot
自恢复，跨平台判据实测成立）及两条首发现缺陷（boots 重置 / ctl socket 权限，R3/R6 修复
归属在案）；无法收口表 #1 指针随重锚、#5 批次扩至 0.0.5 批E + 重裁轮 + 0.0.6 批A–D、#8
真装语义改记真板实测已补（CI 容器路回执归 #1 统收）、append #11 阶段二真机功能链硬件
敞口；在制品列 0.0.5 批E + 0.0.6 批A–D 六提交与本重裁轮批；docs/README 两处索引（0.0.5 行
补重裁轮标注、0.0.6 行补修订版 2 两阶段口径、0.0.7 行进四档落点表与库地图）。
①a–⑤ 全绿重跑。
2026-10-05 0.0.6 批E'（阶段一收尾件——boots 修复三件 + boards 补录，R3/R6 裁定落地）
R3 三件：①[`base/os/distro/units/rrd.service`](../../base/os/distro/units/rrd.service) `[Service]` 注入
`Environment=RR3_DATA_DIR=/var/lib/rockrobot3`（附注释：缺注入时回写建 tmp 失败、
boots 每次开机重置为 1——真板批D 实测）；②rr3-pack `rr3_postinst_text()` 增
`mkdir -p /var/lib/rockrobot3` + `chmod 755` + `chown root:root`（建目录置属主，
升级幂等），其 staging 单测同步补两条断言（postinst 含 mkdir 与 chown 同路径）；
③[`tools/ci/distro-smoke.sh`](../../tools/ci/distro-smoke.sh) 断言链三处补强——⑤ 补
`test -d /var/lib/rockrobot3`、⑥ 冒烟注入 `RR3_DATA_DIR`（与 unit Environment 同
路径）、新增 ⑧ counters 回写断言（日志含"counters 持久化已启用: …/counters.json
（boots 自增已回写）"逐字 + counters.json 在位 + 含 `"boots"` 键）。counters 持久化
本体不动（0.0.4 批B 已实装，缺陷纯部署面）。R6 补录：boards/raspberry-pi-64.md
新增 §7"已知现状：ctl socket 权限"（socket 0755 语义 / 运维 sudo 口径 / 影响
面仅运维链 / 修复归阶段二收尾件，本批不修）；§6 偏离行同步指向。**真板复验
（板恢复后补跑，批E' 待办结清）**：staging 树验证含新 unit（RR3_DATA_DIR）与
postinst（mkdir）→ 传板升级安装（Setting up rockrobot3 0.0.6，postinst 建目录
`/var/lib/rockrobot3` root:root 755）→ daemon-reload + restart 后 counters.json
落盘 `{"boots":1}`（修复当场生效）→ reboot 自恢复后 `{"boots":2}` **跨重启累积
成立**，`rr3-ctl counters` 回显 `rrd/boots = 2`、denied_publish = 0，实测回填
boards §6。rr3-pack 单测 5 条全过；七步门禁直跑 exit=0 全绿（16 步全 PASS）。
2026-10-05 0.0.6 批E（阶段一收口批——status.md 重锚 f2597d5）
整篇重锚：锚点 `c2beba8` → `f2597d5`；能力实况表三行随批E' 实况更新（运维计数
面补 unit 注入口径、deb 打包链行改"boots 已修复并真板复验通过 + socket 现状
补录 §7"、rr3-ctl 行 reboot 双重门**实况订正**——distro unit 现未注入
`RR3_REBOOT_OK`，conn.rs 注释"rrd.service 注入"与 unit 实况的漂移收口核对时
核出，板上 ctl.reboot 亦缺省恒拒，unit 补注入归阶段二小批、与 socket 放行
同收）；里程碑表头换锚；无法收口第 1 条指针换 `f2597d5`、第 5 条批次口径
补"0.0.6 批A–E'"、第 8 条补 distro-smoke 断言链批E' ⑧；在制品段重锚
（99c90d6 / a5b3093 / f2597d5 三提交，标题照 git log 逐字抄录）+ R1–R6 段
批E' 条目划线结清（含真板复验实测）。**本批即 0.0.6 阶段一收口汇报**：
阶段一批次链 = 批A（方案+裁定 b99409f）→ 批B（出包链 5ecaa8d）→ 批C（手册
fb292ce）→ 批D（真板六判据 efd224f + 留痕 c2beba8）→ 重裁轮（99c90d6）→
批E'（boots 修复 a5b3093 + 复验 f2597d5）→ 本批（重锚），主题命题"树莓派
验证跨平台快速部署"阶段一半闭（代码零改动装 BCM 板实测成立；阶段二 = 真机
功能链四件 + brain 上板，硬件/云凭据敞口已在案）。双远端指针一致随本批推送
核对。①a–⑤ 全绿重跑。
2026-10-05 0.0.5 增补裁定轮落档（R7–R12 增补裁定，六问全按建议口径通过；裁定依据逐条引
ADR 原文，全文见 0.0.5 方案 §2.2 裁定记录）**零代码改动，纯文档批**：R1–R6 裁定并 0.0.6
阶段一收尾后，按用户要求复核"0.0.5 是否足以支撑 0.0.6 / 0.0.7 验证"，逐面排查发现六处
R1–R6 未覆盖的漏洞，锚点为增补裁定轮 R7–R12（只增不改：Q1–Q4 与 R1–R6 一字不动）。
R7 **按建议**——阶段二明走 Event JSON `pcm_b64` 过渡承载、不引新 schema（ADR-0011 接缝
已定型，但 b64 载荷在 shm 直读后可能整个消失，Q3 裁"不冻结"正是给 shm 期的；承载形状
写进方案文字而非契约，ADR-0008 裁定 4"不写半成品"同款应用）；R8 **按建议**——I2S
（GPIO18/19/20/21）与硬件 PWM（12/13/18/19）在 **18/19 物理互斥**，R2 建议口径硬件面互斥
须先钉引脚预算，软件 PWM / PCA9685 或 USB 麦二选一，采购前预算表钉死；R9 **按建议**——
真/mock 验收矩阵 + 两档 DoD（真麦 / 真舵机 / 真显示件必达，意图 `RR3_BRAIN_CLOUD=mock`
可过，采购前 sim 档诚实标注"未验真件"、sim 档不得充当收口证据）；R10 **按建议**——0.0.7
立**工具链前置批**（层①之前），"装不上"从风险 P1 升为 WBS 显式依赖（ADR-0008 transport
第三实现位是层②字面兑现，无 xtensa 工具链则层①②必达判据不可达）；R11 **按建议**——订正
R2 末句与 0.0.6 §5.4：真件到位需**新写 linux 后端**（舵机/GPIO）+ 先立显示 trait
（rr-hal 现无显示 trait，连插槽都没有）+ 交叉构建带 `alsa` feature；"改 HAL 后端"与"改
bindings"并列属概念错位（ADR-0012：bindings 决定格位→模块装配、不驱动 HAL 后端分型，
二者正交），归 0.0.6 阶段二"HAL 补工"批；R12 **按建议**——socket 放行 + reboot unit 注入
合为 0.0.6 阶段二运维小批（牵动 ADR-0010 权限模型须 ADR 增补或裁定记录）。0.0.5 方案
§2.2 裁定记录段落档，状态行与 §1.2 D9/D10、§3 T9/T10、§6 DoD 随改；§2.1/§2.2 标题
"待裁"→"已裁定"笔误订正（不涉任何裁定记录内容）。①a–⑤ 全绿重跑。裁定后 T9（增补轮
落档）/ T10（下游方案修订）依序解锁——本批即 T9。
2026-10-05 0.0.6 修订版 3（增补轮 R7–R12 落点：批E'/批E 回填 + 阶段二 R7/R8/R9/R11/R12 落点）
**零代码改动，纯文档批**（T10-A 下游方案修订，依 0.0.5 §2.2 R7–R12 裁定）：0.0.6 方案按
修订版 3 更新——①状态行补修订版 3 段；②§1 阶段二硬件注入口径按 **R11 订正**（音频面 linux
后端存在但须**交叉带 `alsa` feature**；舵机/GPIO 面 linux 后端**不存在须新写**；显示面
**须先立 trait**——rr-hal 现仅有 AudioSource/Sink、PwmServo、GpioOut、Power、Link 六个
trait，无显示 trait，连"改后端"的插槽都没有）；③§2.2 阶段一余量两条标
`[已结清·批E' a5b3093]`（真板复验 `f2597d5`）、阶段二缺口由四件扩至 3–11（新增 8 pcm 承载
**R7** / 9 引脚预算表 **R8** / 10 HAL 补工批 **R11** / 11 阶段二运维小批 **R12**）；
④§4 WBS 回填批E'（`a5b3093` + `f2597d5`）+ **新增批E**（阶段一收口 `0be189c`）、批F1–F4
依赖由"批E'"改"批E"、**新增批F5**（HAL 补工 + 运维小批 R11/R12）、批E'' 依赖改"批E + 批F
全部"；⑤§5.2 新增**阶段二真/mock 验收矩阵（R9）**五环节表 + **两档 DoD**（采购前档
sim/mock 全绿且诚实标注"未验真件"、采购后档真麦/真舵机/真显示件矩阵必达、**sim 档不得
充当收口证据**）；⑥§5.4 新增 **R7 pcm 承载**（明走 Event JSON `pcm_b64` 过渡承载、不引
新 schema）/ **R11 HAL 补工口径**（订正 R2 末句"改 HAL 后端 / 改 bindings"概念错位——
ADR-0012 bindings 决定格位→模块装配、不驱动 HAL 后端分型，二者正交）/ **R8 阶段二引脚
预算表**（GPIO18/19 I2S 与硬件 PWM 物理互斥 + 软件 PWM/PCA9685 或 USB 麦二选一，采购前
钉死）/ **R12 阶段二运维小批**（rrd.service 注入 `RR3_REBOOT_OK` + ctl socket 加组放行
牵动 ADR-0010）；⑦§6 DoD 改八条（批E'/批E 标已完成、增批F5）、新增 §8 修订记录（改了什么/
没改什么显式列，Q1–Q5 裁定一字不动）。裁定落点一一回链 0.0.5 §2.2；门禁七步直跑 exit=0
全绿（不经管道）。本批即 T10-A。
2026-10-05 0.0.7 修订版 1（R10 工具链前置批入 WBS 显式依赖）
**零代码改动，纯文档批**（T10-B 下游方案修订，依 0.0.5 §2.2 R10 裁定）：0.0.7 方案按
修订版 1 更新——①状态行补修订版 1 段；②§2.2 缺口 1「xtensa 工具链未装」由"随批B 落环境
的风险注脚"升级为 **WBS 显式依赖 批A'**（不改四层梯子本身，只把"装不上"从风险升为显式
门）；③§4 WBS 依赖链改 批A → **批A'** → 批B，新增 **批A'（工具链前置批，层① 之前）**
行（产出 = esp-rs xtensa 工具链落环境 + `xtensa-esp32-none-elf` target 加装 + 空 crate
冒烟编译；验收 = 冒烟过为绿、受阻**红灯不软锚**转 P1 缓解），批B 内容剔除工具链段改由
批A' 前置；④§6 DoD 改七条（新增批A' 条目，其余顺延）；⑤§7 P1 注明"已由风险升为 WBS
显式依赖 批A'"、P4 缓解措辞随批A' 更新；⑥新增 §8 修订记录（改了什么/没改什么显式列，
Q1–Q6 裁定一字不动、四层梯子与 golden 对拍机制不动）。门禁七步直跑 exit=0 全绿。
本批即 T10-B。
2026-10-05 0.0.5 命名口径收束（批E''/批E 对齐 git 实况，非裁定项）
**零代码改动，纯文档批**（T10 收尾）：0.0.5 方案内三处沿用旧称"批E''"指代阶段一收口批，
与 git log / 本汇总口径（`0be189c` = **批E**）不一致——属**文档措辞落后实况**（非裁定本体，
R7–R12 裁定记录一字不动）。对齐三处：①§1.2 D10 行"批E'/批E'' 回填"→"批E'/批E 回填"；
②§2.2 洞F（非裁定项）"与批E''（`0be189c` 收口）"→"与批E（`0be189c` 收口）"；③§6 下游
回链 checklist 同上。**批E'' 保留**为"0.0.6 全量收口"专用（依赖批E + 批F 全部）。门禁七步
直跑 exit=0 全绿（不经管道）。本批即 T10 收尾。
2026-10-05 0.0.5 增补轮二落档（R13–R16 增补裁定，四问全按建议口径通过；洞K 非裁定项随批回填）
**零代码改动，纯文档批**（T11 增补轮二落档）：R7–R12 落档后用户令"逐一修复检查出来的
漏洞"，再复核发现五处残余（洞G–K）——G（阻塞级）：pack-arm64.sh 实况 musl 全静态与
alsa（C 动态库）结构性互斥，R9"真麦必达"构建路径不存在，裁 R13 音频件（rr-hal 带
alsa feature + sense/interact）改板上原生构建、宿主 musl 静态件不变、staging 合流；
H（阻塞级）：R8"USB 麦"选项与 R9"双麦 DoA 必达"未拉通（同 R7 对 Q3×R4 教训），裁 R14
采购口径加"USB 麦克风阵列（≥2 麦多通道 UAC）或 I2S 2-Mic HAT"限定 + 双麦期总线
pcm_b64 下混 mono、DoA 留 sense 内部；I：0.0.7 Q4 待裁口径带病（桥透传与 rrd
每连接一身份/spoofed src 拒收冲突），裁 R15 桥降格 transport 级连接多路器（哨兵完整
register 语义进城，rrd 零改动零新契约，桥不进注册面归 tools/ 伴生件）；J：0.0.7 P1
缓解依赖的远端 CI runner 是 Q4 未结悬项且本机容器已裁不装，裁 R16 Q4 宿主核实升批A
显式前置（核实出"无 runner"也算结案=本机直跑收口）+ 批B DoD 补 CI 缺位 SKIP 同款口径；
K（非裁定项，同洞F 先例）：引脚预算表缺 I2C 行随 0.0.6 修订版 4 回填。落档 §2.3 +
§1.2 D11–D13 + WBS T11–T13 + §6 checklist 两行。门禁七步直跑 exit=0 全绿（不经管道）。
本批即 T11。
2026-10-05 0.0.6 修订版 4（增补轮二 R13/R14 落点 + 洞K 回填：音频件板上原生构建 + 采购阵列限定/双麦下混 + 预算表 I2C 行）
**零代码改动，纯文档批**（T12 下游方案修订，依 0.0.5 §2.3 R13/R14 裁定）：0.0.6 方案按
修订版 4 更新——①状态行补修订版 4 段；②§2.2 缺口 3/4 随 R13/R14 更新；③§5.1 交叉链
文案按批B 实况订正（gnu→musl 静态 + rust-lld）+ 新增音频件板上原生构建分工（R13：musl
全静态与 alsa 结构性互斥，真件档构建路径由板上原生构建闭环，板上 rustc ≥1.82 实测为
解锁门，备选 gnu 自产 sysroot 存档不启动）；④§5.4 R11 ③音频改板上原生构建 + 新增
R13 构建路径/R14 采购承载两要点（USB 麦克风阵列 ≥2 麦多通道 UAC 或 I2S 2-Mic HAT
二选一；双麦期总线 pcm_b64 下混 mono、DoA 留 sense 内部、brain 消费零改动；USB 阵列
档互斥消失 SG90 可硬件 PWM）+ 引脚预算表补 I2C（GPIO2/3）行/软件 PWM 剔除 GPIO2/3
（洞K）；⑤§4 批F1/批F5 与 §6 DoD 7 同步。前轮裁定实质一字不动；门禁七步直跑
exit=0 全绿（不经管道）。本批即 T12。
2026-10-05 0.0.7 修订版 2（增补轮二 R15/R16 落点：桥降格 transport 级连接多路器 + Q4 宿主核实升批A 前置）
**零代码改动，纯文档批**（T13 下游方案修订，依 0.0.5 §2.3 R15/R16 裁定）：0.0.7 方案按
修订版 2 更新——①状态行补修订版 2 段；②§2.2 缺口 4 与 §3 Q4 建议口径按 R15 订正
（桥 = UART↔UDS 字节管道/连接多路器，每哨兵一条独立 uds 连接、哨兵完整 register 语义
进城，rrd 语义零改动零新契约从结构成立；桥不进 manifest 注册面归 tools/ 伴生件，Q4
仍待裁）；③§4 批A 补 Q4 宿主核实显式前置（R16：核实出"无 runner"也算结案，层①②
收口=本机直跑，P1 缓解不再两头悬空）+ 批A'/批B 验收补 runner 结案与 CI 缺位 SKIP
口径 + 批E 改桥形态；④§5 桥接白名单段改"桥零策略"（放行面归 rrd 现行注册/前缀/
反伪装校验，桥不持主题清单）+ §6 DoD 2/5 与 §7 P1/P5 随 R15/R16 同步。Q1–Q6 裁定
状态与四层梯子不动；门禁七步直跑 exit=0 全绿（不经管道）。本批即 T13。
2026-10-05 执行准备订正（洞 L/M/N 非裁定项：板上构建承载 + 音频件归属钉死 + R16 结案限定）
**零代码改动，纯文档批**（T14 执行准备订正，依 0.0.5 §2.3 洞 L/M/N——判定收敛后的
执行口径级复核，非再挖裁定面）：①**洞L**（0.0.6 修订版 5）：[pack-arm64.sh](../../tools/deploy/pack-arm64.sh)
实况仅宿主交叉 + scp staging 二进制树，R13 板上原生构建的"源码上板 / crates 依赖通道 /
板上产物归位"三件无落点——批F1 增 `tools/deploy/build-onboard.sh` 承载件 + §5.1 新增
承载要点；②**洞M**（0.0.6 修订版 5）：音频件板上构建在 §4 被批F1/批F5 双列且与 R9
"真麦必达"（批F4 验收）成环——钉死"构建能力归批F1、换装验收归批F5"（§4 两行 + §5.4
R11 ③ + §6 DoD 6/7）；③**洞N**（0.0.7 修订版 3）：R16"核实出无 runner 也算结案：层①②
收口=本机直跑"与 R10"受阻=红灯不软锚"可互读为豁免——在 0.0.7 §7 P1 / §4 批A' / §6
DoD 2 各补限定句（无 runner 只结"缓解路径"案；本机亦不可用时层① 如实登记受阻态），
**R16 原文一字不动**。三件均非裁定项，Q1–Q4 / R1–R6 / R7–R12 / R13–R16 裁定文字均不动；
门禁七步直跑 exit=0 全绿（不经管道）。本批即 T14。
2026-10-05 0.0.6 批F1（语音采集链承载：python 模块打包 + 多通道下混 mono + 板上构建承载件）
**代码批**（0.0.6 阶段二首批，依 §4 WBS 批F1；sim 档零硬件依赖）：四件落库——
①[rr3-pack](../../tools/rr3-pack/src/main.rs) 支持 `lang = "python"` 模块 entry 装机：
python 模块注入格式 `rr3-modules/<name>/{pkg/,manifest.toml}`（`pkg/` 为包树），打包器
递归落位 + **生成 shim**（`#!/bin/sh` + `PYTHONPATH=<装机绝对包树> exec python3 -m <包名>`，
包名 = 目录名连字符转下划线，与模拟演练侧 `rr3_brain_shim_script` 同构）+ chmod 755，
entry 改写仍走原三道门，pkg 树含非普通文件点名拒绝。**为何 shim 由打包器生成**：
`PYTHONPATH` 必须是**装机绝对**（staging 在宿主出、板上 `dpkg -i` 才落
`/usr/lib/rockrobot3`），只有打包器知道 `RR3_INSTALL_ROOT`；rust 模块 `bin` 约定不变。
distro.toml 缺格点名语义不动——本批只落**打包能力**（raspi 功能链装配单与 rr3-pack
`--profile` 参数化归后续批）；②[rr3-sense](../../modules/rr3-sense/src/main.rs) 采集按
config 真值开（`channels>1` 即双麦期；默认 1 声道与旧硬编码逐点等价）+ **多通道下混 mono
入总线**（`rr3_pcm_downmix_mono` / `rr3_pcm_downmix_stream`，批F 时落 audio/mod.rs，
0.0.5 批C① 重铺后现居
[atoms/capture/mech.rs](../../modules/rr3-sense/atoms/capture/mech.rs)；
依 R14：双麦原始多通道只供 sense 内部
DoA，总线承载恒 mono、brain 消费面零改动）；③[rr3-sense](../../modules/rr3-sense/Cargo.toml)
与 [rr3-interact](../../modules/rr3-interact/Cargo.toml) 新增 `alsa` feature 透传
`rr-hal/alsa`——补上 R13 "板上 `--features alsa`" 的接线缺口（原两 crate 无 feature，
板上构建无从透传；宿主 musl 交叉不带、板上默认 sim）；④新增
[build-onboard.sh](../../tools/deploy/build-onboard.sh)（洞 L 承载件：板上依赖门
（rustc ≥1.82 + `libasound2-dev`/`pkg-config`）→ rsync 源码排除 `target/` → 板上
`cargo fetch`（无外网则 vendor）→ 板上 `cargo build --features alsa --release` → 产物
归位 `payload/rr3-modules/<name>/bin` → 断言动态 ELF aarch64 + `ldd` 含 libasound；只覆盖
sense/interact 两件，宿主 musl 静态件仍走 pack-arm64.sh）。新增单测：rr3-pack 2 条
（python 装机 + shim 形状/可执行 + 缺 pkg 点名）、rr3-sense audio 2 条（逐帧下混/流级），
rr3-pack 7/7、rr3-sense 16/16 全绿；两 crate `--features alsa` 本机 check（macOS no-op）
exit=0；`alsa` 已进两 lock（板上 `--locked --features alsa` 可复现）；门禁七步直跑
exit=0 全绿（不经管道，16 项 PASS）。真麦档（板上 alsa 构建 + 真麦采集）与 raspi 装配单
待硬件到位/后续批。本批即批F1。

2026-10-06 0.0.5 增补轮三落档（§2.4 R17–R22 六问全按"2026-10-06 全案深化计划批准"
口径裁定回填，冲突豁免七条随批：R19 取代 R5 档位 / R18 扩 R9 / R18 扩 R11 / R18 扩 R12 /
R22 收口 R8/R14 / ADR-0017 不收缩 ADR-0012·0014 / R21 兑现 conventions §5 退役阶段）
**零代码改动，纯文档批**：状态行 + 承接标注 + §2.4 完整表 + WBS T15–T20 + DoD 六项；
落点 [0.0.5 方案 §2.4](dev/0.0.5版本洛克三号对齐与裁定方案.md)
2026-10-06 ADR-0017 落档（模块内部分层——机制与策略分离，R17 落点）：`rr3mod` 公共骨架
crate 设计 + 分层标准 + 五模块拆分映射 + 实施验收判据；architecture.md §2 原则 4 回链 +
docs/README 两处 ADR 索引改 ADR-0001~0017；实施批归 0.0.8，本轮零 `.rs` 改动
2026-10-06 0.0.6 修订版 6（R18/R22 落点）：批F5 拆 F5a（HAL 补工扩围 power/link 两后端）/
F5b（运维链四项）+ 真件矩阵扩八行 + §5.4 采购 BOM 八行定案 + 新增 §5.5 板上工程执行手册
五小节（环境 runbook / 接线手册六列表 / 配置件全文 / 七链验收 runbook / 失败处置总表）
**零代码改动，纯文档批**
2026-10-06 0.0.7 修订版 4（R19/R20 落点）：Q1–Q6 逐题回填裁定记录 + 层③④升必达（后裁
覆盖 R5 档位，R16 原文与修订版 1–3 记录段一字不动）+ 批D 拆 D1/D2 + 批F DoD 改"四层全
真板验收无受阻态" + 新增 §9 四层真板执行手册（层①②③④ 逐层 runbook）**零代码改动，纯文档批**
2026-10-06 legacy 最终退役批（R21）：工作树残留 `legacy/firmware-c/sdkconfig`（未跟踪，
107,202B）清盘 + .gitignore 摘 `!legacy/`/`!legacy/**` 与 ESP-IDF 段忽略规则
（`**/sdkconfig` / `**/sdkconfig.local` / `**/managed_components/` / `**/dependencies.lock` /
`**/sdkconfig.old`）+ architecture §1/§3 树行订正 + status.md 偏差条目核销后除名 +
本节 ⑤ 门排除条款与 §5 三阶段收口注记 + docs/README 冻结区句订正；
`git ls-files legacy` 实测 0 条（历史叙事引用按"叙述不改史"保留）
2026-10-06 0.0.5 增补轮三收口批（T20，纯文档）：status.md 在制品/敞口重锚（0.0.6 批F 重排
F5a/F5b、0.0.7 Q1–Q6 已裁、BOM 定案、legacy 除名）+ 本节 T15–T20 登记 + docs/README
dev 索引行更新；本轮六批合计实测改动：9 文件（8 改 + ADR-0017 新建），零 `.rs`/
module.toml/契约文件改动，linkcheck 全绿
2026-10-07 0.0.6 纪律小批（rrd 心跳顺序倒置修复——先归并后应答）**代码批**：
批F1 收口期实证 `ctl_counters_aggregates_heartbeat_counters` 负载敏感竞态（单跑
稳定绿、全量并行时"期望 5 实得 2"）——[conn.rs](../../base/runtime/rrd/src/conn.rs)
心跳分支旧序"应答先回（D-6 分支内 reply_ok）→ 计数归并在后"，对端收到应答即读
`rr3.ctl.counters` 会抢在归并前读到上一拍的账。本批把归并块整体移到心跳分支最前
（**先归并后应答**），D-6 三态语义与 Q1 聚合语义零改动（计数仍不跟判活走，
Rejected 也照归并；两把锁各自成段不嵌套）；[e2e_ctl.rs](../../base/runtime/rrd/tests/e2e_ctl.rs)
同用例 doc comment 补"顺序契约"三句，把"应答返回时本拍必已进账"钉成观测面。
**不随批做的事**：counters 落盘节流——查实 write-through 逐次刷是 0.0.4 批B Q2
显式裁定（[counters.rs](../../base/runtime/rrd/src/counters.rs) 头注"周期刷被 Q2
显式否决：断电丢尾部"），节流（时间窗跳写）即周期刷变体，改裁定须经裁定不走
暗改；eMMC 磨损顾虑（高频计数变化 × 每次全量 fsync）登记为待裁定问题，归后续
版本方案征集。e2e_ctl 8/8、clippy -D warnings exit=0、门禁七步直跑 exit=0
（16 项 PASS）。
2026-10-07 conventions §2 补录（本条所在批）：批F1 板上收口至批F3 sim 段三个
代码批实施在先、登记漏记，本条按提交实况补齐（内容回填下方三行；补录本身
零代码改动，不影响任何判定）。
2026-10-06 0.0.6 批F1 板上验证收口（`79be827` + 口径对齐 `1d8955e` + 脚手架拆除
复验 `d6059e5`）**代码批 + docs**：真板四项全过——①含 brain 新 deb 装机后
counters 跨整机重启累积（boots 14→15）+ rrd.service 冷启动自恢复；②brain
（python shim）经 distro.toml cognition 四格绑定进 boot 队列，**六模块全
running**（denied_publish = 0）+ reboot 冷启动复验同绿；③build-onboard.sh 全链
exit 0（apt rustc 1.85.1 过 ≥1.82 门 + libasound2-dev → rsync → 板上
`--features alsa` 构建 sense/interact → 归位 + 动态 ELF/libasound 断言；音频件
换装验收仍归批F5a，洞 M）；④真麦未采购诚实标注（R22 定案措辞：USB 麦阵列，
I2S HAT 存档备选）。随批结清真板暴露缺陷四件：pack-arm64/distro-smoke 的
`cp -R` BSD 语义摊平 brain pkg 包树（改"dst 已存在 + 尾斜杠"）；rr3-pack shim 补
`RR3_BRAIN_CONFIG` 装机绝对注入（pkg 多一层致 config 回退指虚位）；build-onboard
改对两 manifest 各自 fetch（仓库根无 Cargo.toml）；rr-hal linux.rs
`alsa::Error::errno()` 裸 i32 解构（alsa-gated 代码首次真编译暴露）。配套
distro-smoke 改六模块 spawn 断言 + rr-registry contract 0.0.2 缺格边界随满配
退役；bootfs 注入脚手架拆除后冷启动复验：NetworkManager keyfile 持久化、
wlan0 自主连网、六模块 running、boots 15→16 跨重启累积（无脚手架自主联网）。
2026-10-07 0.0.6 批F2 sim 段判向-转向编排（`e4ccbaf`）**代码批**（ADR-0007/
0017/0014）：rr-hal sim 源双麦方位注入（`RR3_HAL_SIM_DOA_DEG`，ILD 增益模型
g=1∓sinθ）+ backend 选择接线；rr3-sense 新增 audio/doa.rs——下混**前** ILD 反演
±90° 分三档（|deg|≤30=center），vad_start onset 负载携 doa{deg,bin}（schema
append 零新契约，R14：双麦下混 mono 前判向）；rr3-act 意图执行面开放
servo/command（两道归属门）落 rr3-act/servo/state（限速流）；brain 词表
"向左/向右"→servo 动作 + mock ASR 文本注入 env。e2e 双件：判向（-60° 注入→
bin=left + mono 下混）与转向（intent→servo/state seq=2 angle=45）+ 限速窗纪律
钉（广播前等过 50ms 窗，被挡不补发）；linux PWM 真后端归批F5a（R9 矩阵口径，
sim 档诚实标注）。
2026-10-07 0.0.6 批F3 sim 段表情扩展（`444c764`）**代码批**（ADR-0017）：五态机
扩**七态**（增 listen/turn 入白名单，talk 仍 TTS 自动驱动不可点名）+ 每态 8x8
帧序列（blink/talk 双帧循环；切换与 talk 挂起恢复均归零帧指针从首帧重播）+
主循环 ~250ms 墙钟节拍推帧（帧速率不随总线流量漂移）；契约面零改动
（topics.toml face/expression note 同源追加两态措辞）；显示 trait 与真件适配
随批F5a，本批仅 sim 回归。
2026-10-07 0.0.6 批F4 sim 档全链闭环演练（`379e6de`）**代码批 + docs**（ADR-0007/
0014/0017）：`rr3-sim --chain` 新模式——六真进程全链（rrd boot spawn sense 双麦
burst+DOA 注入 -60° + 外部 interact/brain（mock 云）/act + 演练自带裸 UDS 观察者
**先于外部模块注册**取证 vad_start/servo/state，servo/state 一次性广播晚注册漏收），
无人工干预跑通"采集→意图→判向→转向→表情"一轮以上；判定三段日志留痕（brain
`actions=1` / interact「talk 挂起」/ act `servo angle=45`）+ `denied_publish = 0`；
CI 增 sim-chain job（tools/ci/sim-chain.sh，python ≥3.11 门槛，演练回执远端口径）。
板上首次实跑揪出并结清缺陷两件：①rr-hal sim burst 门控按交错样点序号算偶秒——
多通道下窗口折半 500ms < silence_tail 700ms，offset 永不触发（修：门控按帧序号
idx/声道数，mono 逐点不变 + 双通道回归测试钉住）；②演练实例面板 8080 与板上 deb
系统实例冲突（--brain 同款中招，CI 无系统实例故未暴露；演练统一注入
`RR3_PANEL_ADDR=127.0.0.1:18080` 共享常量）。boards §9 回填 + 方案判据 10/11 内联
标注：R9 **采购前档**，不充当真件收口证据。
2026-10-07 0.0.8 模块分层立项批（纯文档，0.0.5 §2.4 R17"各模块拆分清单"细化）：新建
[0.0.8 方案](dev/0.0.8版本洛克三号模块内部分层实施方案.md)（rr3mod 签名级规格 §2 +
五模块目标目录树与迁移映射 §4 + 批0–批6 序列 §5 + 每批验收四件套 §6，含 rr3-sense
config 路径语义分叉的如实登记与批3 前置核对件）+ ADR-0017 关联段增补回链（裁定原文
不动）+ docs/README dev 索引两处 + status.md 在制品登记；零 `.rs`/module.toml/契约
文件改动，linkcheck 全绿
2026-10-07 0.0.6 批F5a 第一件：linux sysfs PWM 舵机后端（`acc548e`）**代码批**
（R11①，ADR-0007）：新增 [linux_pwm.rs](../../base/runtime/rr-hal/src/linux_pwm.rs)
（单门 `target_os = "linux"` 纯 std，无 feature 门/外部依赖）——sysfs pwmchip 导出
与全量配置落定（50Hz + 中位 90° 脉宽 + 使能，与 sim 后端初始角同观测，R11 换装
编排零改动）+ set_angle 落占空比 + angle 反演回读（域外显式回错不编角）+ Drop
停发脉冲并归还自导出通道；SG90 定案参数 50Hz / 0.5–2.5ms 线性映射 0–180°（R22，
方案 §5.5.2 接线 1），芯片/通道经 `RR3_HAL_PWM_CHIP`/`RR3_HAL_PWM_CHANNEL` 选
（缺省 pwmchip0/pwm0=GPIO12）。backend.rs 订正 resolve 平台门：linux 可用性由
alsa feature 门改判 `target_os`（音频/舵机各能力面各判各的——原门错位会致真件档
板上显式 linux 落 placeholder、act 探活 exit(1) 起不来），音频 alsa 未编进时落
placeholder 显式回错不静默落 sim；`rr3_hal_servo` 补 linux 分支。单测：脉宽映射
边界 / 全域反演往返恒等 / 假 sysfs 树（临时目录确定性，不触真硬件）导出-配置-
回读-Drop 归还；宿主门禁七步全绿 + aarch64 musl 交叉 check（R13 路径编入本文件
及其测试）。**板上 `cargo test -p rr-hal` 留板上线后补跑**（板当前离线，rsync
口径同批F1）；真件档验收（真舵机脉宽/抖动实测）留外设到位随批F5a 真件档，
R9 舵机行诚实标注不变
2026-10-07 0.0.6 批F5a 第二件：linux chardev GPIO 数字输出后端（`da52e7f`）**代码批**
（R11②，ADR-0007）：新增 [linux_gpio.rs](../../base/runtime/rr-hal/src/linux_gpio.rs)
（单门 `target_os = "linux"`，libc 仅取 ioctl/close 声明壳，musl 静态零额外 C 依赖，
R13）——GPIO chardev UAPI v1（`/dev/gpiochipN`，6.12 内核现役；sysfs gpio 已弃用不采）
请求 `GPIO_GET_LINEHANDLE_IOCTL` 取 OUTPUT 句柄（v1 独占 line）+ SET/GET_LINE_VALUES
写读电平；ioctl 号与 `#[repr(C)]` 布局单测对齐内核头文件硬编码值钉死；ensure 语义同
PWM：选择函数构造不触设备，首次 trait 调用请求句柄落初值 0（与 sim 初始 false 同观测，
R11 换装编排零改动），Drop close 句柄即释放 line；line 被占（EBUSY）回 `Busy`、其余
`InvalidParam` 带点名。backend.rs：`rr3_hal_gpio_out` **改带 role 参数**（`led`/`relay`
白名单——main.rs 连调两次原靠调用顺序配键是顺序耦合，带参显式化）+ 补 linux 分支 +
`RR3_HAL_GPIO_CHIP` 键与角色线偏移键 `RR3_HAL_GPIO_LED_LINE`/`RR3_HAL_GPIO_RELAY_LINE`
（键拼装/缺省值纯函数可单测；缺省 4/17 = R8 预算表 GPIO4/17 软件路行，接线手册未单列
LED/继电器行，真件档装配单如改脚位走 env 覆写）；libc 0.2 直依赖进 linux target 表
（锁内已有 0.2.189 传播各独立工程 Cargo.lock）。单测：ioctl 号/布局断言 + temp 树
普通文件冒充 chip 的错误传播路径 + 白名单键/缺省裁决；宿主门禁七步全绿 + aarch64
musl 交叉 check 全绿（glibc/musl 的 ioctl request 参数类型差异经 `as _` 平台推断）。
**板上 `cargo test -p rr-hal` 同①口径留板上线后补跑**；真件档验收（真 LED/继电器
电平实测）留外设到位随批F5a 真件档
2026-10-07 0.0.6 批F5a 第三件：rr-hal 显示 trait `Rr3Display`（`bcc4de2`）**代码批**
（R11③，ADR-0007）：**形状经评审裁定——单 trait 帧面**（方案原文"Rr3Display/
Rr3Face"两名的"Rr3Face"解释为帧类型名，不立同名 trait：show(expression) 实现方
必须自带帧表，而"状态→帧"映射与表情白名单是模块面策略，rr-hal 不能反向依赖模块，
立了即死插槽；同 led 灯效白名单先例"效果清单不进 HAL"）。lib.rs 新增
`Rr3FaceFrame = [u8; 8]`（行主序 bit7=最左点，MAX7219 行寄存器直排；类型事实源
归 HAL——trait 签名需要，interact face.rs 改 re-export 保模块内名）+ `Rr3Display`
trait（`draw(&frame)` 整帧替换 + `frame()` 读回探活/对账）+ 占位件补
`placeholder/display`。sim.rs 新增 `Rr3SimDisplay`（内存记录位图，构造带初始帧
黑屏，Mutex 不拆原子）；backend.rs 新增 `rr3_hal_display()`（sim 分支；linux 分支
落占位——MAX7219 SPI 后端归真件档，显式 linux 探活 exit(1) 是"真件未到位"预期
语义）。interact 接线：开机探活读 `frame()`（Unsupported 即致命）+ 主循环
~250ms 拍出帧交 `draw`（妆面件失败只 eprintln，同 led worker 口径）——换装只动
后端实现，编排零改动。单测：sim 点阵 draw/读回/跨线程写读 + 选择函数 sim 黑屏 +
trait 对象安全；宿主门禁七步全绿 + aarch64 musl 交叉 check 全绿；板上补跑同①
口径欠账；真件档验收（MAX7219 出帧实测）留外设到位随批F5a 真件档

2026-10-07 0.0.6 批F5a 第四件：rr-hal linux I2C INA219 电池后端
（`8f4d928`）**代码批**（R18，ADR-0007）：判据 12 的 HAL 承载面（电压字段
原样报真读数；两点标定偏差 ≤2% 属真件档验收，留外设到位）。`linux_power.rs`
单门 `target_os = "linux"`（libc 声明壳 musl 静态，R13，同②③样式）。接口面
I2C chardev UAPI：`I2C_SLAVE`（0x0703）设从地址——**libc 0.2 的 musl 面
不导出 I2C 常量组**，照② GPIO 批先例自定义常量硬编码 i2c-dev.h 定值并单测
钉死；寄存器读是原生大端（写 1 字节寄存器号 + 读 2 字节），与 `i2cget w`
SMBus 小端序字不同，真件档对拍先字节交换。换算面：bus voltage bits[15:3]
13-bit LSB 4mV（单测钉死）；percent 与 sim 后端电压曲线**同一条线**
（3200–4200mV ↔ 0–100%，反演恒等有测），越界 clamp；**charging 诚实恒
false**——INA219 calibration 默认 0 时电流寄存器读回恒 0，如实报数不编造，
方向语义随真件档接线定案。backend.rs 新增 `RR3_HAL_I2C_BUS`（缺省 1，树莓派
硬件 I2C）与 `RR3_HAL_INA219_ADDR`（缺省 0x40，**只认 `0x` 前缀**——裸写
有十进制歧义（"40" 是 0x28 还是 40？），裁决收窄有测）两键 +
`rr3_hal_power()` linux 分支；ensure 延迟初始化同②③（构造不触设备，errno
在 File drop 前取出）。单测：换算/反演 + ioctl 号 + temp 树假设备 ENOTTY
错误路径 + Drop no-op + addr 裁决；宿主门禁七步全绿 + aarch64 musl 交叉
check 全绿；板上补跑同①口径欠账；真件档验收（i2cget 对拍 + 万用表两点
标定）留外设到位随批F5a 真件档

2026-10-07 0.0.6 批F5a 第五件：rr-hal linux NetworkManager 链路后端
（`faf01be`）**代码批**（R18，ADR-0007）：判据 13 的 HAL 承载面——**配网/
断链重连是 NM 自身 autoconnect 行为，模块侧只读翻转上报，HAL 只如实报数**
（sim.rs 分工口径同条）。`linux_link.rs` 单门 `target_os = "linux"`；承载选
**nmcli 子进程轮询**而非 D-Bus 直连——runbook 件 4 既定口径就是 nmcli 全套
命令，std Command 零新依赖不破 musl 静态纪律（D-Bus 需 zbus 级大库且 async
耦合）；轮询节拍由 connect `link_report_period_ms` 控制，HAL 每次 state()
现读现报，不持 fd 无 ensure（进程面不是设备面）。一轮两调用：`dev show`
（STATE==100 即在链 + CONNECTION 回落 + TYPE 区分 wifi/有线）→ up 且 wifi
再 `dev wifi list --rescan no`（只读缓存不触发秒级扫描；取 IN-USE `*` 行
真广播名 + 信号质量）。**SIGNAL 是 0–100 质量百分比不是 dBm**，经验线性
dBm ≈ q/2 − 100 折算注释钉死，q=0 → None 不报假 −100；terse 模式值转义
（`\:`/`\\`）反转义与 SSID 含冒号行两端夹逼解析有测；断链语义对齐 sim：
`up=false` 时 ssid/rssi 强制 None，模块侧不许从"down 还带着 ssid"发明
语义。backend.rs 新增 `RR3_HAL_LINK_IFNAME` 键（缺省 wlan0，runbook 件 4
口径）+ `rr3_hal_link()` linux 分支 + 过期 doc 订正；选择器测试补 link 行
（sim 在链必带 SSID/RSSI、断链必无）。单测：解析全套 + 假 nmcli 脚本
编排（构造传程序路径，零 PATH 全局污染）+ 非零退出显式回错；宿主门禁
七步全绿 + aarch64 musl 交叉 check 全绿；板上补跑同①口径欠账；判据 13
三断言（配网一次成功/断网 30s 自恢复/翻转经总线可查）属真机验收，
随批F5a 真件档实跑（runbook §5.5.4 链⑥）

2026-10-07 0.0.6 批F5b：阶段二运维链 R12 两件实装（`fab7b6c`）**代码批**
（ADR-0007 + ADR-0010 修订 1 随批落）：①**unit 注入 `RR3_REBOOT_OK=1`**——
conn.rs 双重门（cfg(linux) + env）缺省恒拒的解锁面兑现，sim/容器直跑无此
变量仍恒拒；`Restart=on-failure→always` + `RestartSec=1→2` 与方案目标形态
同批落（reboot 自身退出也拉起，判死退避归 rrd 内层，systemd 只兜底）。
②**ctl socket 加组放行**——真板批D 首发现敞口（`srwxr-xr-x root:root` 非
root 连不上）修复：rrd daemon bind 后 `rr3_socket_acl_grant`（libc 声明壳
chmod/getgrnam/chown 三函数，errno 逐调用取出）`chmod 0o660` + `chgrp
rockrobot3`（postinst 幂等建组，运维用户装机现场入组，deb 不动用户）；
**路线经用户裁定**走方案 §5.5.3 注记二选一的后项（socket 建后 chgrp/chmod，
不挪路径）——一处代码 systemd/S99rrd 两轨共用，RuntimeDirectory 三行不落
库，裁定全文见 ADR-0010 修订 1；组不在位留痕跳过不拒启（紧权限敞口比守护
起不来轻），root 恒可连。单测：socket 真_bind + mode 位 0o660 断言（chgrp
属主组断言归板上）、postinst addgroup 幂等断言；宿主门禁七步全绿 +
aarch64 musl check 全绿 + rr3-pack 7 测试过。**真机三项验收（判据 14 看门狗
SIGSTOP+SIGKILL 两路判死 / 判据 15 断电恢复 boots 累积 + 真 reboot 两档）
与 socket 放行真机复验全部留板上线**（板离线敞口，同板上 cargo test 口径）

2026-10-07 0.0.6 批E''：0.0.6 全版本收口**纯文档批，零 `.rs` 改动**——
status.md 整篇重锚 `f2597d5`→`95d37ef`：0.0.5 增补轮九提交 + 批F1 四提交 +
纪律小批 `b81c986` + 批F2/F3 四提交 + 批F4 两提交 + 0.0.8 立项 `ce07f43` +
merge `5044ca8` + 批F5a 五对 + 批F5b 两提交（本快照新锚）全部按实况计入
能力表/里程碑表/敞口表/在制品段；收口判据实况：判据 1–11 本机/板上已取证
面全绿（判据 10 sim 档 boards §9），判据 12/13/14/15 真机验收 + 板上
`cargo test -p rr-hal` 补跑留板上线敞口（板当前离线，rsync 口径同批F1）；
双远端 `ls-remote` 三端核对随本批推送执行（ADR-0007/0010）

2026-10-07 0.0.7 批A：0.0.7 方案落档**纯文档批，零 `.rs` 改动**
（ADR-0008）——方案状态改"已评审通过、执行中"（2026-10-07 用户裁定通过）；
**Q4 宿主核实结案（R16 显式前置）**：无可用远端 runner——ci.yml 落
`.github/workflows/`（GitHub Actions schema）而 atomgit 原生引擎 AtomGit Action
只拾取 `.gitcode/workflows/`（官方文档实证），0.0.2 起远端回执零取得（敞口
第 1/4/9 条悬置至今——与拾取路径不符的解释一致）→ **层①②收口=本机直跑**，批A' 受阻时
层① 按 R10/洞 N 登记受阻态；ci.yml 迁移 `.gitcode/workflows/` 登记为后续可选
（留用户裁定另立小批）；**Q5 密钥门清理**：ESP-IDF 外置不进仓 + 密钥门沿用
0.0.4 批F 注销（status.md 敞口表第 2 条），版本与获取方式随批D1 实测落档；
docs/README 索引状态行同步

2026-10-07 0.0.7 批A'：xtensa 工具链前置绿**零仓内代码改动，纯环境 + 落档**
（ADR-0008）——esp-rs fork rust 自 v1.91.1.0 起停止 Intel Mac 构建（30 个
release 资产矩阵实测），本机落最后带官方构建的 v1.90.0.0（fork 构建矩阵边界
非本机能力问题，批B/批D1 依赖 MSRV 对表 1.90）；github.com 主站直连超时 →
tarball 经镜像手动安装（`~/.rustup/toolchains/esp-1.90.0.0` + rust-src 入
sysroot + `rustup toolchain link esp`）；冒烟证据：空 `#![no_std]` lib
`cargo +esp build --target xtensa-esp32-none-elf -Z build-std=core` 过（EXIT=0），
rlib 内目标文件 `file` 实证 **Tensilica Xtensa ELF**——真出码非空转；
build-std 构建依赖（gimli v0.32.0 等）crates.io CDN 本机会僵死 → 每次调用级
`--config` 挂 rsproxy sparse 镜像解决，不动全局 cargo 配置；none-elf 裸金属
无 std 属架构必然（std-on-xtensa 走 `xtensa-esp32-espidf` target 归批D1）；
espup/ldproxy 未装成（ldproxy 属批D1 esp-idf-sys 构建依赖，届时重试）；
落档明细见 0.0.7 方案 §4 批A' 记录

2026-10-07 0.0.7 批B：层① 契约 parity 绿（ADR-0008/0006）——rr-ipc no_std 化：
`#![no_std]` + 条件 `extern crate std`，协议层纯 core+alloc（body.rs 取
`alloc::vec::Vec`，std 目标同一类型零行为变化），transport 层按
`not(target_os = "none")` 门控（std::io 面只属 OS 语境，espidf unix 系照常），
std prelude 隐式注入的 vec!/format!/Vec/String/ToString 各模块显式导入；
`to_bytes` const 化（put_le 手写小端，不依赖 copy_from_slice 的 const 版本）+
golden 头 `const _` 编译期对拍（bytes_eq 手写——数组 `==` 在 const 求值是
nightly 面），任意目标编译 rr-ipc 即求值，xtensa 编译器证明编码器与 golden
逐字节一致；ipct003.sh 四路化：[4/4] `cargo +esp check -p rr-ipc --lib
--target xtensa-esp32-none-elf -Z build-std=alloc,core --offline`（rsproxy
镜像注入可 env 关，crates.io CDN 僵死规避沿批A' 口径），四路实况 = 宿主
PASS / aarch64 PASS / arm64 容器 SKIP（Docker 缺位既定）/ **xtensa PASS**；
xtensa 运行期 dump 归批C UART 夹具（第二重证据）；宿主 49+4 tests 全绿 +
七步门禁 16 PASS 随批；落档明细见 0.0.7 方案 §4 批B 记录

2026-10-08 根 README 双语版本行订正**纯文档漂移订正，零代码**（ADR-0007）——
0.0.5 方案逐任务审查批（用户令"每条任务逐一审查"）查出两处门面漂移：
①版本行仍指 0.0.5 为"当前版本"（0.0.5 D1"三处索引"判据的第三处随版本演进
失守——docs/README 两处已随 0.0.6 批E''/0.0.7 批A 切换，根 README 双语漏切）；
②ADR 范围行仍写 0001~0015（ADR-0016/0017 已落档）。订正：版本行切
0.0.6 当前（修订版 6）/ 0.0.7 执行中（修订版 4）/ 0.0.8 立项三行 +
ADR 行补至 0017；同批审查确认 0.0.5 T1–T20 全有实物证据在案

2026-10-08 0.0.5 增补轮四落档（批W0，T21）**纯文档批，零代码改动**
（R23–R29，用户 2026-10-08"全按建议通过"逐项授权）——**定位升格**：
0.0.5 从"裁定批"升"裁定 + 板载开发前技术落地批"，两板（树莓派/ESP32）
开工所需全部技术点在 0.0.5 内落地，不等 0.0.6/0.0.7 收口。新立独立全案
`docs/process/dev/0.0.5增补轮四——板载开发前技术落地全案.md`（体例参考
0.0.8：形状级钉死 + 批W0–W7 WBS + 每技术点深度基准四件套 + 风险回滚）；
0.0.5 方案加 §2.5（R23–R29 七问裁定表 + 记录）+ 承接标注 + 状态行 +
WBS T21–T27；docs/README 两处索引行（0.0.5 状态切"执行中"+ 全案行）。
裁定要点：R25 执行核对发现批F5a 五件已收官（`8f4d928` INA219/`faf01be`
NM，2026-10-07）——前置主张自然满足，批W4a/W4b 作废、0.0.6 修订版 7
只登记实况；R26 counters 节流策略=心跳合并写（批W3 实施）；R27 CI 载体
=gitcode 单真源（批W2）；R28 espidf std 冒烟前置批W1（不等批C④）。
后续批次：W1 espidf std 冒烟 → W2 CI 迁移首跑 → W3 counters 节流 →
W5 真件装配与标定手册 → W6 桥设计/e2e/S3 预案（0.0.7 修订版 5）→
W7 env 总表 + 触发清单 + 收口（0.0.6 修订版 7）

2026-10-08 0.0.5 增补轮四 批W2：CI 单真源迁移 + 八 job 首跑（R27/W5 裁定，
ADR-0007）——`git mv .github/workflows/ci.yml .gitcode/workflows/ci.yml`
（atomgit 原生引擎只拾取 `.gitcode/workflows/`，批A' 实证；gitcode 同路径共用，
双份维护必漂移），`.github/workflows/ci.yml` 收为指路存根（仅注释）；
八 job（conventions / lint / brain-unittest×2 / ipct003 / distro-smoke /
buildroot-assembly / sim-assemble / sim-chain）随推送首跑，回执或逐 job 缺口
据实登记（R16 纪律：不以 SKIP 冒充绿）；tools/ci/README 与 status.md 指路行同步

2026-10-08 0.0.5 增补轮四 批W3：counters 落盘节流改判心跳合并写 + 关停
强制 flush（R26 改判，ADR-0010，Q2 旧裁定原文不覆盖、演进在案）——
闸门形状 = 首拍即刷（冷启动第一笔变化不等窗口）+ M 毫秒窗口至多一次
（env `RR3_COUNTERS_FLUSH_MS` 缺省 10_000）+ N 拍兜底（env
`RR3_COUNTERS_FLUSH_MAX` 缺省 64）+ 失败不记账下一拍自然重试；SIGTERM/
SIGINT 优雅关停强制 flush（self-pipe 信号模式：处理器内只 write 一字节，
快照/写盘/日志落 `counters-term-flush` 线程；手写 libc `signal`/`write`
extern 壳不引依赖，同 rr-ipc shm 六函数先例）；数据面零改动（归并/快照/
原子写序原样），eMMC 写频次 1s 心跳拍下 1000 拍 1000 次 → 100 次（10× 削减）；
e2e 双判据（节流本体：第二拍后 300ms 盘面不变；关停完整性：SIGTERM 退 0 +
尾差落盘 + 无 tmp 残留）随既有单测全绿

2026-10-08 0.0.5 增补轮四 批W1：espidf std 构建链冒烟（R28，全案 §3.3 C1，
ELF Tensilica Xtensa 32-bit 静态链接出树实证整链通）——`cargo +esp build`
（xtensa-esp32-espidf + build-std std/panic_abort）+ esp-idf-sys 0.35.0 ↔
ESP-IDF v5.2.2 版本对表钉死（0.36+ 弃 5.1/5.2.x，不再取更高版）；三坑登记：
①bindgen 的 libclang 须乐鑫 LLVM fork（上游含 CLT 的均无 Xtensa 后端，报
unknown target triple 'xtensa'）——esp-clang 的 clang 与 libs 两资产合并解包
（后者才含 libclang.dylib，espup 两件都装）+ `~/.espup/esp-clang` symlink，
esp-idf-sys `setup_clang_env` 自动设 LIBCLANG_PATH；②`cargo:rustc-link-arg`
只作用于发射包自身 bins 面，esp-idf-sys 是 lib 发射全丢——根 crate build.rs
经 `links="esp_idf"` 元数据接力（`LinkArgs::output_propagated`，官方模板同款），
缺位症状 ldproxy panic `Cannot locate argument '--ldproxy-linker'`；
③`--cfg espidf_time64` rustflags 必设（IDF 5.x time_t 64 位面）；新门
tools/ci/espidf-std-check.sh（SKIP 纪律同 esp32-uart-echo 门，另带 venv
dist-info 改名兜底与 pip/GitHub 镜像注入），check.sh ④ 显式排除本工程；
版本钉死登记落 tools/espidf-std-hello/README.md

2026-10-08 0.0.5 增补轮四 批W5：真件装配与标定手册落档（全案 D9/B2）——
docs/process/dev/真件装配与标定手册.md，六件到货桌面级验收/装配/标定 runbook
（USB 麦 UAC≥2ch 免驱+非全零抓样、SG90 三点行程 0.5/1.5/2.5ms↔0/90/180°+
UBEC 1S 输入档与单点共地、MAX7219 SPI 出帧与 Rr3FaceFrame=[u8;8] 帧面逐行
对齐、TP4056 三保护核对（过充 4.20V/过放 ~2.45V/短路断出）、INA219 万用表
两点标定偏差 ≤2%、ESP32 UART2 四件套 3.3V TTL 严禁 5V），每件四件套+
到货差异登记表；权威边界引用不複述（型号=0.0.6 §5.4 BOM 定案、接线=§5.5.2、
配置=§5.5.3、逐链=§5.5.4），HAL 键位锚定 rr-hal 代码实况
（RR3_HAL_I2C_BUS/RR3_HAL_INA219_ADDR，8f4d928 收官面）；docs/README
三处登记（四档落点/库地图/动作索引）

2026-10-08 0.0.5 增补轮四 批W6：0.0.7 修订版 5 落档（全案 §3.3 C2/C3/C4
前置技术点）——§5.1 桥件形状钉死八条（连接模型 UART↔UDS 1:1、字节透传
零解析、帧级握手实证（rr3_handshake 门序无连接级 creds 门，桥对握手不可见）、
寿命管理三态循环（UDS 未就绪丢弃+计数自愈依据 rrd-rpc §5 TimedOut 语义）、
下行带宽核算、socket 权限注记、零策略断言四条，R15/R20 复核零冲突无裁定点
回上游）；§9.3 ⑥ S3 切换预案四件套（触发=剩余堆<20% 或 std 不可起、target
差异点三处、到货验收单四项、S3 无二级降级）；§9.4 ⑤ e2e 双档判据（macOS 档
sim_peer 作总线对端 / 树莓派档批F4 口径 RR3_SIM_PANEL_ADDR 注入 +
denied_publish=0）；§9.3 ① target 订正 none-elf→espidf（批W1 实证）+ 版本
组合回链批W1 钉死登记；docs/README 两处修订版 5 同步

2026-10-08 0.0.5 增补轮四 批W7：本轮收口（T27，全案 §6 DoD 兑现——技术点
零悬空）——A3 全部 RR3_HAL_* env 键位总表落 base/os/distro/README.md
（后端选择 1 键 + linux 档 9 键 + sim 档 7 键 + role 泛化注记，每键与
backend.rs 符号逐字一致，I2C 设备根常量与显示件占位归注记）；A4 板上线
触发件清单四件套落 boards/raspberry-pi-64.md §10（板上 cargo test -p rr-hal
补跑 / socket 放行真机复验 / 部署链回归演练，板上线即照单执行）；0.0.6
修订版 7 落档（R25 只登记实况：F5a 五件收官提交号 + W4a/W4b 作废确认 +
批W5/W7 落点指针，批次结构零改动）；0.0.5 §2.5 补增补轮四执行对账 +
全案状态行改收官态；敞口余量两项登记：批W2 八 job 首跑回执待网页核对、
板上三项欠账与真件档维持触发态

2026-10-08 0.0.5 第 II 轮 批A：坐标系对齐（ADR-0019 裁定 1 / R16，全案 §7）——
批A① 二十一格原子归属清单落 0.0.5 方案 §12（12.1 逐格 layer/唯一归属原子件/
现落点/备注，net.online 基座内建、占位件 env/imu/charger 诚实标注；12.2 无格件
三问预分类；12.3 塌缩归一候选）。批A② `[faces]`→`[enabled_slots]` 消歧改名
（避免与职责面 Facade 撞名）：profiles 两份、rrd/profile.rs（struct 字段/解析/
报错文案/测试全量）、rrd cli.rs（PROFILE_MANIFEST_CONFLICT 文案逐字同步）与
daemon.rs 注释、rrd e2e 四件、rr-registry 查册门两段、sim/assemble.rs、
profiles/default.toml 与 distro.toml、architecture.md（目录树 + §4.3 注记改
已落口径）、ADR-0012（状态注记改已执行 + 裁定 3 sketch/裁定注记/对表三处）、
rrd-cli.md（开关表/常量引文/设计裁定三处）、本节 §3 装配单纪律行；
§2 历史条目与 ADR-0019 决策原文不改写；代码侧 `grep -rn 'faces'`（排除
enabled_slots）归零，`cargo check -p rrd -p rr-registry --all-targets` 与
sim `cargo check --all-targets` 全绿

2026-10-09 0.0.5 第 II 轮 批B：原子件契约（ADR-0019 裁定 1/5/8，全案 §8 批B 行）——
批B① Atom manifest 契约定形：`rr-registry/src/atom.schema.toml`（六字段：atom.name/
atom.version/provides.slots/uses.slots/events.publishes/events.subscribes，与
module.schema.toml 同构 `[[field]]` 数组）+ `atom.rs` 三入口
（`rr3_registry_validate_atom_text` / `rr3_registry_validate_atom` /
`rr3_registry_board_free_assert`）：查册门共用 slots 表真值（空表 fail-closed）、
provides.slots 跨字段钉"每格一件恰一项"（裁定 1）、板无关断言 rock3 自有禁词表
（引脚/总线/厂商/产品 token 子串命中即拒，裁定 5 硬规 2 的 manifest 侧）；
`validate.rs` 的 `rr3_registry_check` 增 `atom_name` 规则分支（只增不改）。
批B② `rr3-mkmod` 扩产 `rr3_mkmod_generate_atom`：原子五件套
（atom.toml + 入口 `<名>.rs` + `mech.rs` + `assemble.toml` + `private.rs`），
staging→`rr3_registry_validate_atom` 回灌→rename，bin 增 `--atom <名> <格位>` 子命令。
批B③ `rr3-power` 打样重铺（先打样验骨架，方案 §8）：`atoms/battery`、`atoms/charger`
各五件（atom.toml + 入口 + mech + assemble.toml + private.rs）；`composite.toml`
（Composite 层：⊗ 链 battery ⊗ charger、unit=process，机器校验 schema 归批E）；
`src/composite.rs` 组合件实现（原 main.rs `rr3_run`/config 解析/两单测逐字迁入）；
`main.rs` 248→17 行（≤80 门，组合件五件套②）；SURFACE 字符串与启动打印序列逐字
冻结，module.toml/config.toml/Cargo.toml 零改动，原 `src/battery.rs`/`charger.rs`
删除（逻辑零改动迁入原子目录）；rr-registry 契约测试钉死两份打样 atom.toml
（漂移当场红）。判据达成：④ 独立工程 7 测试绿、mkmod 回灌 7 测试绿、
rr-registry 33+34 全绿、七步门禁直跑 GATE_EXIT=0

2026-10-09 0.0.5 第 II 轮 批C：组合件与四面逐模块重铺（ADR-0019 裁定 2/8，全案 §8 批C 行；
逐模块一提交、每批七步门禁直跑 GATE_EXIT=0 十六 PASS、三端指针核对一致）——
批C① sense：塌缩先行 `4002303`（base64 编码收编 `rr-ipc/protocol/b64.rs`，方案 §12.3
候选第一件，§10 权威向量测试随迁）+ 重铺 `a82d8b6`（capture/vad/env/imu 四原子五件套 +
composite.toml ⊗ 链，main.rs 133→16 行，旧 src/audio 树退役，rrd e2e 音频链真二进制复证；
rr3-sense 22 测试）；批C② act `e17974b`（servo/led/relay 三原子，intent/reflex 留组合件
——§12.2 接入/守护面成分，main.rs 405→16 行；rr3-act 27 测试）；批C③ interact
`a1723b5`（face/player 两原子，panel 状态面留组合件且 panel/ 目录零改动，main.rs 213→16
行；rr3-interact 23 测试）；批C④ connect `708e5ad`（wifi/bt 两原子，session 会话事务面
留组合件、`RR3_SESSION_TOPIC` 常量随 session.rs，main.rs 306→17 行；rr3-connect 12 测试）；
批C⑤ brain `b3aafa2`（dialog/asr/llm/tts 四原子**两件式** manifest——Python 面只落
atom.toml + assemble.toml，机制本体 .py 原位，Rust 五件套拆分归 ADR-0016 线；
`__main__.py` 组合件分装吸收改 17 行瘦入口、机制主体迁 `rr3_brain/composite.py`，
pyproject console script 面零波及；brain 本地 unittest 25 过 + 1 条 tomllib 门槛 error
= 既有 py3.9 平台差额，CI brain-unittest 3.12 覆盖全量）。批C⑥ 收官（本条目）：方案
§12 落点列对账至重铺后实况（21 格全表 + §12.2 终裁两行已落 + §12.3 三候选两落一留）。
共性判据：SURFACE 字符串与启动打印序逐字冻结；六红线（module.toml/config.toml/
Cargo.toml（brain 为 pyproject.toml）零改动、线上字节零改动、策略不进件、F⊗F
fail-closed、sim assemble 六模块编排不变）逐批复核；15 份 atom.toml 全数钉进 rr-registry
契约测试（契约 + 查册门 + 板无关断言，累计 17 份含批B 两份）

2026-10-09 0.0.5 第 II 轮 批D① 总线语义下沉（ADR-0018 裁定 1/5，全案 §8 批D₁ 行；
机制核四层下沉的第二层，七步门禁直跑 GATE_EXIT=0 十六 PASS、三端指针核对一致）——
机制核切片 crate `rr-core` 落地（`base/runtime/rr-core`，`#![no_std]` + core+alloc）：
常量层（task 空间 `RR3_TASK_RRD`/`RR3_TASK_FIRST_MODULE`/广播 0、优先级带 0–139、
`RR3_METHOD_CALL`、`RR3_TRACE_ID_DIRECT`、总线契约版本）+ 寻址规则（`Rr3TaskKind` +
`rr3_task_kind` 纯函数）+ 帧组装（`rr3_frame_compose`/`rr3_event_compose` 时钟注入
纯函数，timestamp_ns 显式入参）；12 测试（task 空间不重叠、优先级占位带、trace_id
直连、Event 线上形状、golden 头区对拍）。配套 `rr-ipc` 帧数据形状下沉：`Rr3Frame`/
`Rr3FrameError`/`rr3_frame_to_bytes` 从 `transport/frame.rs` 迁 `protocol/wire.rs`
（no_std 可达），transport 保留 io 半边（read/write）并 re-export 维持路径兼容；lib.rs
re-export 门控拆分（数据形状无条件导出、io 半边保持 `cfg(not(target_os="none"))`），
rr-ipc 58+4 测试全绿零波及。`rr-bus` 改消费 rr-core：常量真值迁出，lib.rs re-export
保对外路径不变，client.rs 三处帧组装（Request/Event/Response）改走 compose——线上
字节逐字不变（`event_compose_matches_client_wire_shape` 钉形状），8+1 测试
（`reexports_match_rr_core_truth` 对拍 rr-core 真值）。xtensa no_std 线验证：
`cargo +esp check -Zbuild-std=core,alloc --target xtensa-esp32-none-elf`
（rr-core 全量 + rr-ipc `--lib` 跳过需 std 的 ipct003_dump bin）双绿

2026-10-09 0.0.5 第 II 轮 批D② 模块运行时执行环下沉（ADR-0018 裁定 1/3，全案 §8 批D
行；机制核四层下沉的第三层，七步门禁直跑 GATE_EXIT=0 十六 PASS、三端指针核对一致）——
`rr-core` 新增 `runtime` 模块（no_std，纯函数 + 时钟注入）：执行环（连接→注册→心跳→
分发→节拍）的平台无关决策——`Rr3ModuleRuntime` 连接态转移（`Rr3LinkState`：
Registering/Online/Down，**net.online 格机制核切片** = `rr3_link_online`）+ 心跳拍点
调度（相对节拍逐拍 +hb，drift 语义与 std 心跳线程既有行为逐拍一致；间隔保底 ≥1ms
即原 `hb_ms.max(1)` 规则进机制核）+ `rr3_dispatch_class` 收帧分发分类（Response 路由
等待者、其余进 inbox）；7 测试（状态转移、拍点等距序列、断开终态幂等、分发分类表）。
`rr-bus` client 变 std 驱动器：心跳线程拍点真值改由状态机给出（`next_beat_wait_ms`/
`heartbeat_due`/`heartbeat_sent`，驱动器只做 sleep 等拍与 socket 收发——平台供给面），
reader 循环分发判定改走 `rr3_dispatch_class`；线上字节与拍点序列逐字不变，
12 测试零波及守恒（功能守恒红线：client 全用例 + rrd e2e 全绿）。
xtensa no_std 线复验：`cargo +esp check -Zbuild-std=core,alloc --target
xtensa-esp32-none-elf -p rr-core` 绿（runtime 随 lib 入 no_std 面）。同批结清一处
批D① 漏检：`rr-core/README.md` 随批D① 入库时未被 git 跟踪、⑤ 文档门的导航检查
（`git ls-files` 口径）未覆盖，docs/README.md 导航漏链——批D② 门禁红出即补挂

2026-10-09 0.0.5 第 II 轮 批D③ 控制原语+安全核下沉（ADR-0018 裁定 5，全案 §8 批D
行；机制核四层下沉的第四层，七步门禁直跑 GATE_EXIT=0 十六 PASS、三端指针核对一致）——
`rr-core` 新增 `control` 模块（no_std，纯函数 + `u64` 毫秒注入时钟）：四件原语真值
自模块面迁入——`Rr3RateGate`（rr3-act servo/state 限速门，Instant→注入 ms）、
`Rr3LowEdge`（rr3-power battery/low 边沿机）、饱和检测三件（`Rr3ReflexCfg`/
`rr3_reflex_window_plan`/`Rr3SaturationDetector`，另拆 `rr3_reflex_window_saturated`/
`rr3_reflex_rate_hz` 两纯函数供 std 主循环窗结算调用）+ 安全核新原语
`rr3_travel_clamp`（限位钳位：上游裁决被绕过也不越行程，行程端点颠倒收敛到有效
区间、绝不 panic）。9 测试含逐拍一致性判据 `same_control_graph_beat_for_beat_
identical`（同一控制图喂两遍同一合成时钟序列输出逐拍相等——裁定 5 "同一张控制图
Linux 实时环与裸机定时器逐拍一致"的机械判据）。模块侧 re-export 保既有路径零波及：
servo/mech.rs、battery/mech.rs、reflex.rs 真值迁出（越阈/episode/节拍方案等全量用例
随真值迁 rr-core，本地测试薄化为 re-export 检查）；composite.rs 两处限速门调用改
`rr3_now_ms()` 注入（口径同 rr-bus）+ RPC/intent 两路落角前接安全核钳位（输入合法时
恒等=行为守恒）。同批结清 control 面两处测试/实现不一致：rate_hz 零窗从 `max(1)`
虚报改报 0（畸形输入往"不触发动作"方向收敛）、travel_clamp 颠倒行程超界断言订正为
最近端点。xtensa no_std 线复验绿（control 随 lib 入 no_std 面）。rr-core 24 测试
（批D① 8 + 批D② 7 + control 9）、rr3-act 24、rr3-power 6 全绿。

2026-10-09 0.0.5 第 II 轮 批D④ 板特定缺省收口进描述符（ADR-0018 裁定 4，R2-D4 /
Q-P5 缓解序；七步门禁直跑 GATE_EXIT=0 十六 PASS、xtensa 线复验绿）——三段数据链
闭环：① `profiles/default.toml` + `distro.toml` 新增 `[hardware]` 物理绑定段（九键
真值 = 原代码缺省的数据化迁移：alsa_device/pwm_chip/pwm_channel/gpio_chip/
gpio_led_line/gpio_relay_line/i2c_bus/ina219_addr/link_ifname，R22/R8/接线 3/
BOM #5/runbook 件 4 口径，"换板 = 只改本段"）；② rrd 装配链解析 `[hardware]`
（可选段、标量字符串化、坏类型/空键逐条点名）并经 `Rr3SpawnJob.hal_bindings` 于
spawn 注入 `RR3_HAL_BINDINGS`（空值不注入；`--manifest` 调试后门无装配单 = 无描
述符，fail-closed + 应急 env 可救）；③ rr-hal 新增 `descriptor` 模块（`Rr3HalBindings`
九字段 + `from_text`/`compose`/`effective`）：描述符为底、`RR3_HAL_*` env 降应急
覆盖（同键覆盖赢、坏值/未知键 Err 点名）、**代码缺省全部删除**——五个选择函数
（audio_source/audio_sink/servo/gpio_out/power/link）linux 分支改从生效绑定取参调
`*_new`，键缺失/合成失败/白名单外 role 落占位后端使用时显式回错，不再回落树莓派
引脚（`RR3_HAL_ALSA_DEVICE_DEFAULT`、`rr3_gpio_line_default`、各后端 `*_default()`
构造一并退役；sim 系缺省非板特定保留）。`rr-hal` 新增 toml workspace 依赖（描述符
文本解析同版）。"板 = 数据"自此成立（architecture §12.3 注记、§3 落点表划线、方案
§12.3 采集环配置行同线注记同批）。rr-hal 35 测试（descriptor 4 新增）、rrd 138 单
测 + e2e 全绿，模块面与线上字节零改动。

2026-10-09 0.0.5 第 II 轮 批D⑤ xtensa 对拍门 + 无 OS 启动形态点验（ADR-0018 裁定
1/3/5，全案 §8 批D 行 R2-D5 判据；机制核四层下沉收官后把三条验证判据从批次内人工
复验升级为可复跑专用门，七步门禁直跑 GATE_EXIT=0 十六 PASS、三端指针核对一致）——
新增 `tools/ci/rr-core-xtensa-check.sh`（不进七步门：esp fork 工具链属平台条件依赖；
纪律同 espidf-std-check.sh——工具链缺位 SKIP 明说、在位编译失败 FAIL；rr-ipc 零外部
依赖、rr-core 仅依赖 rr-ipc，全程无网络）：①机制核 tripwire——rr-core 源 grep
`global_allocator`/`extern crate std` 命中即红（裁定 3"内存策略属平台四类供给、
机制核不得假设全局分配器"与 no_std 面不失真的静态 tripwire）；②裸机面对拍——
`cargo +esp check -Zbuild-std=core,alloc -p rr-core --target xtensa-esp32-none-elf`，
总线语义/模块运行时/控制/安全核全切片在无 OS 目标编译成立即无 OS 启动形态点验
（fork/exec/socket/线程/墙上时钟任何 OS 假设在该目标必红），rr-ipc golden 头
`const _` 断言随依赖链在该目标编译期重求值即 xtensa 编译期 golden 对拍（0.0.7 层①
同一资产）；③宿主面对拍——`cargo test -p rr-core` 真跑（24 测试含 golden 形状/
控制图逐拍一致判据），0 通过判门禁失效（同 check.sh run_tests 口径）。与 0.0.7
层③④对表结论随批登记：层③ 哨兵（注册/心跳/回应答）要对齐的最小语义子集真值即
rr-core runtime 拍点/状态机（批D②），层④ bridge 经 transport trait 接总线
（ADR-0008 接缝定型）——机制核切片即 0.0.7 两层要复用的语义面，不重复开线。
architecture §12.6 点验矩阵首件机械点验注记与本条同批

2026-10-09 0.0.5 第 II 轮 批E §13 静态执法子集接进 check.sh（ADR-0018 裁定 1/3/5、
ADR-0019 裁定 1/5、architecture §13 表，方案 §8 批E 行；七步门禁扩为**九步**——
①/①b/②/②b/③/④/⑤ 原序不变，⑤ 文档门后新增 ⑥⑦；直跑 GATE_EXIT=0，25 PASS
＝①1 ①b1 ②1 ②b1 ③1 ④10 ⑤1 ⑥5 ⑦4，三端指针核对一致）——**新 ⑥ 模块测试
接线**："每格一件·双达标（有 tests 接 CI）"的 CI 侧执法：`find modules -name
Cargo.toml` 逐工程 `cargo test -q`，打空即 FAIL（门禁失效不放行）；brain 无
Cargo.toml 自然跳过（其 26 条 unittest 由 CI brain-unittest job 覆盖，非 SKIP）。
**新 ⑦ 静态断言四件**（全 fail-closed）：⑦-a 零策略载荷——机制核面（rr-core 全部 +
rr-ipc protocol/ + transport/mod.rs，即 no_std 可达面）grep **外部**板卡厂商/产品/
芯片/器件名与 `if.*(vendor|product)` 条件分支，命中即红；自家产品标识（rock3/洛克）
**不在禁词**——rock3-ipc.sock 契约冻结路径是机制自身的运行时身份，不是策略载荷；
⑦-b F⊗F 非法——逐 `composite.toml` 提 `atoms` 数组，逐成员验 `atoms/<名>/atom.toml`
存在，数组缺失即红；⑦-c 五件套计数——Rust 原子件逐目录断言五件（atom.toml/`<名>`.rs/
mech.rs/assemble.toml/private.rs），brain 两件式属批C⑤ 已登记的**豁免**（非 SKIP），
组合件实体 Rust 断言 `main.rs`+`composite.rs`、brain 断言 `__main__.py`+`composite.py`；
⑦-d 入口体量——modules 各 `main.rs` 与 brain 入口 `wc -l` ≤80。**每格一件反向断言**
不进 check.sh 抄第二份：落 rr-registry 仓扫描测试
`repo_atoms_compliant_and_every_slot_uniquely_covered`（查册真值处执法，② 步覆盖）——
扫 `modules/*/atoms/*/atom.toml` 逐份过契约+查册+板无关门，21 格逐格反查唯一归属、
覆盖账不平即红；内建豁免集 `BUILTIN_NO_ATOM` 钉死四格（bus.publish/bus.subscribe/
bus.rpc/net.online——基座内建或机制核切片承载，不立件；power.battery 虽 base 层但由
battery 原子承担，不在豁免集），仓内新增件不钉即红。业务阈值字面量无可靠静态判据
（注释版本号同为点分数值），执法面＝atom 板无关门 + 描述符/ops 注入纪律 + 评审
（§13"判不了 semantic 的留评审"），已在 ⑦ 段注释与 tools/ci/README.md 登记。
随批 lint 归一（CI lint job 绿色基线延续）：rustfmt 全仓归一（批B/C/D 遗留漂移——
rr-registry atom.rs/contract.rs、rr-core control.rs 六处 bool 断言、rrd daemon.rs
never_loop（while 体必 exit 改 if）、rr-bus、模块面 unused imports、sim chain.rs
match→if let 与 needless borrow）+ clippy `-D warnings` 全仓清零复验；CI lint job
clippy 步骤对 `tools/esp32-uart-echo`/`tools/espidf-std-hello` 两平台条件工程排除
（与七步门 ④ 同口径——xtensa/espidf 专用工具链工程宿主 stable 必红，编译执法在专用门
SKIP 纪律；fmt 步不编译照跑）；机制核注释去板名化三处（lib.rs 厂商表述、header.rs
目标名改指专用门、transport/mod.rs 对端表述），⑦-a 静态门自指解除。rr-registry
39（lib）+34（contract）测试绿

2026-10-09 0.0.5 第 II 轮 收口批 E' 收官深挖结清（批E 落库后对 0.0.5 交付面做漏洞
深挖，三实锤同批结清；九步门禁直跑 GATE_EXIT=0，PASS 25→30，三端指针核对一致）——
**① sim 测试死角封死**：批E 深挖发现 tools/sim 的 23 条 #[test] 处于"④ 只 check
（--all-targets 编译测试目标不跑）+ ⑥ 只扫 modules + CI sim-chain/sim-assemble 只跑
e2e 命令"的三不管死角（②b 只覆盖 mkmodule），重铺五版恰动其钉面而断言零执行；
⑥ 步 find 自 `modules` 扩为与 ④ 同口径（`modules tools base` 全部独立 Rust 工程，
esp 两工程排除同 ④），0 测试工程即红——sim 23 条真跑全绿（重铺与机制核下沉无行为
漂移的实证），门禁 PASS 25→30。**② manifest 未知字段反向断言**：atom/module 两侧
校验此前只正向遍历 schema 字段，manifest 里多出来的键（typo 如 `uses.slost`）静默
放行、依赖静默丢失（fail-open）；新增共用实现
`rr3_registry_unknown_fields_assert`（validate.rs——从字段规格重建允许键树，顶层段
集与段内键集双对账，一段式顶层标量键单独放行），atom 侧（atom.rs 0b 步）与 module
侧（validate_inner 0 步）接线，不建第二份实现。上线即抓出活案例：17 份 atom.toml
实况一直携带顶层 `schema_version = 1` 自报契约版本而 atom.schema.toml **漏登记**该
字段——补登记（kind=int required=true，atom 校验 kind 分派同步补 int 分支，字段数
pin 6→7），三修后 17 份全绿。**③ atom.name == 目录名钉死**：⑦-b/⑦-c 按目录名执法、
契约按 atom.name，两名漂移无红；② 步仓扫描测试补断言（件名两个真源当场红）。
rr-registry 40（lib）+34（contract）测试绿。CI 回执核对（批W2 敞口第 1 条）与
⑦-a 禁词表双份维护（check.sh 硬编码与 atom.rs RR3_BOARD_TOKENS 同步靠纪律）两项
登记敞口未结，见 status.md

2026-10-10 0.0.5 第 III 轮 批I 能力面契约下沉收官（方案 §14 批I 行，ADR-0018 修订 1 /
ADR-0019 修订 3·4；九步门禁直跑 GATE_EXIT=0，PASS 30 全绿）——**F1 契约层立库**：
`base/runtime/rr-hal-api` no_std crate 落地（7 trait 同步签名去 `Send + Sync`、
`#[async_trait]` 退役；值类型去 String/Vec：`Rr3SsidText` 定长 32 octets、`Rr3ErrText`
定长 128 octets 按 char 边界截断、`Rr3AudioSource::read` 改调用方缓冲写返回实填数、
`Rr3PcmStream` 降承载层数据留 rr-hal；`Rr3UnavailableSource` 占位后端随迁 no_std 可达）。
**F2 实现层退位**：rr-hal 改 Linux/sim 实现层 re-export 保对外路径，全仓调用点同步化，
`rr3_block_on` 件面清零。**F3 两处形状违规解掉**：led 先按 R-4 把三条真实等待用例改合成
时钟注入、再拆 worker 线程与 mpsc，`rr3_led_tick(now_ms, …)` 状态机由组合件节拍驱动，
blink 行为序列不变；bt 初值去 env 改 config `[rr3-connect.bt] up` 数据化注入（N-1，缺省
false 语义不变，缺段回落等价旧 env 未设）。**F4 xtensa 有载体真跑**：本机 esp fork 工具链
（0.0.7 批A' 装，esp-1.90.0.0）在位，`rr-core-xtensa-check.sh` 三查真绿（tripwire 扩
rr-hal-api + xtensa none-elf 裸机面编译 + 宿主面 rr-core 24 + rr-hal-api 6 测试对拍）——
Q-R5 预判的"工具链缺位记无载体"未发生，方案 §17 如实改记"有载体"。**F5 载荷组装上移
承载层**（C16）：五册 `*_body`/`*_field`/`*_result` 迁各 `src/composite.rs`（S-7/A-8/
I-8/P-7/N-7 等），件面 serde_json 11 件→0 清零且线上字节逐字不变；I-10 sim 波形查表迁
rr-hal sim 后端（件面去 std 数学，偏差预算 ±100）；S-6 整型化，框架 v1.1 违例清零（预算
±100 内）。**配套结清**：C7 依赖闭包口径钉死（L1 执法面 = L0 + serde_json，禁 `use std::`
/`use rr_hal::` 非 test 面；测试面假后端放行；件面 rr_hal:: 现状 = 契约名 re-export +
I-10 数据面）；C8 门查门（rr-hal-api 同批进 ⑦-a 文件枚举与 xtensa 门三处点位，门查门
机制首次自用）；C5 维持批J（本批未触碰禁词执法面）；S-1 sense config 与 power/connect
同口径（env 覆盖优先，缺省 manifest 同目录 config.toml，缺文件显式报错）；N-9 connect
假 AP 表迁 HAL sim 后端真值单点。**clippy 全仓清零复验**：rr-hal-api 契约 Result 体积顶
result_large_err 阈值——契约形状不让步，crate 级带注豁免（不留无解释 allow）；sense
bin 面 unused 导出改 `#[cfg(test)]`、act 死代码删除 + 复杂类型别名 `Rr3CallOutcome`、
interact 未用导入移测试面，修后全绿

2026-10-10 0.0.5 第 III 轮 批J `⊙` 通路 + 描述符七段 + 注入面收官（方案 §14 批J 行，ADR-0018 修订 1 /
ADR-0019 修订 1 ③·修订 2 ②；九步门禁直跑 GATE_EXIT=0，PASS 30 全绿）——**G1/G2 字节流窄缝**：
`rr-ipc/src/transport/bytestream.rs` 落 `Rr3ByteStream` 两方法 trait，`Rr3TransportConn` blanket impl
换底、std 单向 adapter，uds/shm/uart 线上字节零改动（golden + 线级用例为证），transport 层
`cfg(not(target_os = "none"))` 门解除、no_std 板侧 conn 形状用例随 xtensa 门真绿（C6）；**G3+C17 三段
schema**：`contracts/slots.toml` schema_version=2 增 `status`（正常/占位/retired 三态唯一真值点）与
`tier_scope`（档位适用分族）两字段（C17 裁定 (a)，21 格数据同批），`rr-registry` slots 解析与查册门
同步、`profiles/*.toml` 扩 `[platform]`/`[supplies]`/`[slots].present` 三段（含 C15 平台看门狗位）、
G6 档位↔watchdog 一致性校验落描述符装载门（tier=bare 无 watchdog 拒 boot 并点名）；**G4 注入面定形**：
`rr-registry/src/assemble.rs` 落装配校验真值单点（形状级：schema v2/未知字段点名/trait∈七表/
desc_key∈九表/点路径合法；注入级：ops src_key 对模块 config.toml 点路径游走），七 trait × 九键在册表
另立副本 + 契约测试钉死（扩面/扩键走契约级评审两处同批，副本滞后 fail-closed）；rrd 装配段扫描缺项
（缺文件/坏形/注入缺）逐条点名到件 → 模块照常 spawn 但注册入表即 `Rr3Phase::Degraded`、心跳 Kept
不升级、`rr3_reg_undegrade_after` 不捞回（唯一解除路 = 修数据 + 重启 rrd）；17 份 assemble.toml 同批升
v2——battery 真值样板（handles 引 i2c_bus+ina219_addr、ops 三槽引 Rr3BatteryCfg 同源键）、bt 兑现
迁移承诺（ops 声明 `rr3-connect.bt.up`）、其余 14 份如实空声明；`rr3-pack` staging 扩整树
（composite.toml + atoms/ 随包，C1 裁定"随包附可校验件声明"）；mkmod 模板升 v2 并回灌装配校验
（C3，新件直过 ⑥/⑦）；**G5 描述符错误收口**：rr-hal 六选择函数（audio/sink/servo/gpio/power/link）
`.ok()` 吞错改 `eprintln!` 点名（文案纯函数独立可单测，cfg(linux) 门 + test 门两全，R3-D3）；**C14
首批 retired 走通**：`asr/text`/`dialog/turn` 置位 retired（理由与替代者入 topics.toml status/note
数据面）、brain `net/link` 与 interact `intent/result` 订阅撤申报、brain 订阅面只增补
`audio/pcm`+`audio/vad_start`、sim 演练主题换 `rr3-sense/audio/vad_start`（主题计数 28 条不变、128B
线上字节零改动、module.toml 申报面变更有裁定在册）；**C4 落档**：[hardware] 不扩键（九键维持）裁定
记录落 descriptor.rs 头注释。clippy 全仓清零复验（backend.rs 文案函数 cfg(any(linux, test)) 门 +
rr-ipc j4 遗留未用变量清除）；rr-registry 48（lib）+37（contract）、rrd 146（lib）、mkmodule 5、
rr3-pack 8 测试绿

2026-10-10 0.0.5 第 III 轮 批K 判据上门 + 点验矩阵 + 第二形态收官（方案 §14 批K 行，ADR-0018 /
ADR-0019；门禁直跑 GATE_EXIT=0，PASS 50 全绿——⑧ 三门**上线即全绿**，既有步零 FAIL 变 SKIP）——
**H1 件级两腿对拍补齐**：11 件（led/relay/servo/bt/wifi/face/player/battery/charger/capture/vad）各落
腿一确定性（§8 设计行序列同输入两跑轨迹逐拍相等）+ 腿二期望值引自裁定语义与契约（行程/时限/限速门、
转移矩阵、状态机与文案点名，**不由件自产**）；sensor.env / sensor.imu 两占位格 + cognition 四件
（asr/dialog/llm/tts，Python 两件式无 mech.rs）记**不适用**——不适用 ≠ 通过，占位格触发条件登记
status.md。**H2 三门接 check.sh ⑧**（步序与条数登记本节 §2 ⑧ 行）：⑧-a 两腿对拍计数门（slots.toml
status 三态 join atom.toml provides，normal 格腿一+腿二各 ≥1；C10 stub 反例自证内置——恒输出 0 +
只有腿一的门先红，门自证拦得住空件）、⑧-b 件级依赖闭包 grep 门（L0=`use std::`/`extern crate std`、
L1=+serde_json，扫 modules/*/atoms/*/*.rs 非 test 面，39 份源零命中——C7 约束自批K 起由门执法）、
⑧-c 注入面校验门（rr-registry contract injection 用例**显式过滤器**封改名/删除 fail-open）；⑦-a 扫面
同步扩 modules/*/atoms/*/（SURFACE 常量与 println 进禁词扫面）。**H3 点验矩阵出数入口**
`tools/ci/matrix.sh`（architecture §12.6 七列：FAIL exit 1、--strict 未跑列不放行 exit 2、默认如实
出数 exit 0；`--manual <列> --evidence <仓内证据>` 手测回填契约——证据三样齐才收、不覆盖自动 FAIL
格）+ 供给自证表模板落 docs/reference/。**H4 第二形态 + C2**：`rr3-pack --profile` 参数化 + 机型集
pin 同批改（contract.rs），`profiles/esp32-mini.toml` 落描述符（F-B 桥接微型形态三格：
rr3-sense=[audio.capture,audio.vad] + rr3-act=[act.servo]，不启用 net.*/cognition.*，理由随文件落档；
件码/对外主题/slots.toml 零改动）+ rrd 装载正例与 contract 形状 pin 的装配演练真跑。**H6 守护输出
定形**：rr-core::runtime 本地守护"起动态 + 喂拍 + 安全核触发/拒动原因"行格式落档 + 对拍用例钉死
（architecture §12.4 口径同批）。**H5 文档口径收口**：architecture §3 树补画 rr-hal-api + 落点表四行
翻已落盘 + §12.4/§13 口径，ci.yml job 步骤名改"十步"（上一轮唯一未结项结清），tools/ci/README ⑤ 行
撤除不存在的 `legacy/` 排除条款 + ⑧ 三门全文补录，本节 §2 步序登记（七步经 0.0.5 批E 接 ⑥⑦ 成九步、
批K 接 ⑧ 扩**十步**），六份模块 README 目录树对齐重铺实况（atoms/ 五件套 + composite.toml ⊗ 链 +
瘦入口），全仓"六段/六列/七步"口径复扫——活文档全部对齐十步/七列；历史台账（本节登记条目、方案批行
原文、ADR 存档、status 台账行）按 0.0.7 方案"台账原文不改写"纪律保留。clippy 全仓清零复验（两处新
用例错：servo 复杂返回类型抽 `ServoParitySnap` 别名、bt `vec_init_then_push` 改 `vec![]` 宏）；矩阵
首跑回执 PASS=4 / 未跑=3 / FAIL=0（① Docker 缺 3/4 路、⑥ python3 3.9.6<3.11、⑦ E4 真板专属——
无载体如实记，指纹三件套随行落 status.md）；⑤ 逐 crate 对拍 22 条（act 6 / connect 4 / interact 4 /
power 4 / sense 4）+ rr3-pack 19 + rrctl 23 + mkmodule 9 + rr-registry contract 38 + rrd profile 24
测试绿

2026-10-10 0.0.7 两板收口批——批C④⑤ 板级收官落档（0.0.7 方案状态行；十步门禁直跑
GATE_EXIT=0，PASS 50 全绿）——CP2102 到位，ESP32 devkit 按 0.0.7 §9.2 runbook 板级实测：
④ 判据过（128B 头 + payload 完整收发，cmp 逐字节一致）、⑤ 判据过（半帧截断与坏 magic 两类
注入均拒收、注入后不进死锁、正常帧收发不受影响）——**层② 必达全绿、批C 整批收官**；现场实测
由用户口述确认，串口日志摘要未留档如实注记（R2 纪律），boards 回填时补。随批现场补丁 `6d3a1ee`
（esp32-uart-echo 独立条件工程，不在十步门 workspace 面）：espflash 4.x 构镜像硬校验 ELF 内
ESP-IDF app descriptor（IDF 二级引导运行期亦验），esp-bootloader-esp-idf 0.1 宏段名
`.rodata_desc.appdesc` 与 esp-hal 1.1.2 rodata.x KEEP 段名 `.flash.appdesc` 错代、宏产物被
gc-sections 丢弃——按对表段名手挂 `#[used]` 静态量修复（专用门
`tools/ci/esp-uart-echo-check.sh` 直跑绿 + ELF 物证三要素核验齐：节 0x100 字节在册 / magic
0xABCD5432 / project_name 与 Cargo 元数据一致）。0.0.6 §5.4 BOM 第 9 行 CP2102 翻"已到位"；
C12 首条手测行不冒领（⑦ 列证据三样未留档），敞口维持待按供给自证表模板补采

2026-10-10 批K/两板收口批深挖收口（两独立代理对拍 + 人工复核）——P1 一处同批结清：
`base/os/distro/units/rrd.service` ExecStart 硬编码 distro.toml 装机位与批K H4 `--profile`
参数化矛盾（非 distro 机型出包 deb 内无 distro.toml、systemd 拉起即败）——rr3-pack 补
第二处内容改写：ExecStart 装机位随 `--profile` 同步改写（缺省 distro 恒等字节零差异；
模板锚点缺席 fail-closed；反例断言钉死不残留 distro 位）。⑧-b L0 门拓全限定内联与 L1
对称（`std::` 单 token），同批清退三份 atom mech 五处内联活体（face mem::replace ×2 /
player PI ×2 / capture PI ×1 → `core::` 等价替换，语义零变化）；活文档口径同步（§2 ⑧ 行、
architecture §13 表、tools/ci/README）；`.github/README.md` 订正（指路存根 + 八 job + 十步）；
五处 "2026-10-11" 落档日期订正为提交时间戳在案的 2026-10-10。P2 四笔登记 status.md 敞口：
⑧-c 过滤器按用例名子串绑定（空壳可顶替）、matrix.sh 三小项（①列 4/4 硬编码 / --manual
仅标记词 / `{{` 转义剥离失衡风险）、esp-bootloader-esp-idf 默认特性未关、ELF 三要素核验
未自动化 + 上游 idf_ver 误读 MMU 页配置键注记（装饰性，无参宏同源字段一致）

2026-10-10 0.0.7 批D1 层③ 板上实测收官（哨兵 README 实测报告回填；十步门禁直跑
GATE_EXIT=0，PASS 52 全绿 + 专用门 espidf-sentinel-check.sh 绿）——两修正随批：
①哨兵 reader/心跳线程栈 8KB→16KB 根因实锤（serde_json + json! + FrameBuf 峰值溢出
踩堆 → Arc 指针踩成垃圾 → std Mutex 惰性 pthread 初始化崩 EXCVADDR=0xb；
addr2line 定位 + 插桩复跑取证后清理，16KB 钉为运行下限）；②harness parse_upstream
method 门改 Option<String>（Response 帧合法无 method，强制门误杀 round-trip 应答）。
板上对跑 HARNESS_EXIT=0（14 PASS/0 FAIL）+ 三点 RAM（roundtrip min_free=250620B
≈ 启动堆 85% >> 20% 阈值，D1→D2 不触发）+ Down→esp_restart→重注册转移路径附实证。
运维教训两笔：遗留串口采集进程与 espflash 抢同一 USB 串口致同步应答缺字节烧录连败
（lsof 定位——烧录前先清持有者）；zsh 下 `ps | grep foo[g]` 未加引号被 glob 展开成
"no matches found"、进程存在性检查静默失效（方括号模式须引号包裹）。敞口：UART 持续
流开口帧同步（CP2102 缓冲残留 + `Rr3UartConn::open` 不 tcflush，复跑先复位+清缓冲
组合时序）——批E rr3-bridge 同题，登记 status.md

2026-10-11 0.0.7 批E 层④ 收官——0.0.7 四层梯子全收官（0.0.7 方案状态行 + §9.4 ⑥
实装对表（修订版 12）+ status.md 随批；哨兵 dst 修复件 `870cfe6` 十步门禁直跑
GATE_EXIT=0（PASS 54）+ xtensa 专用门绿，本落档提交双远端三端核对一致）——E6 本机
彩排（假哨兵对拍 PASS=18）揪出 harness 盲区：对拍件地址字段恒等真值（src 恒 1）静默
遮蔽 rrd 上行 dst 门缺陷，哨兵应答 dst 改钉 `RR3_TASK_RRD`、harness bus_frame 补
src_task 入参变回归网——**对手件字段须可注入，对拍件恒真值不算覆盖**。E7 真板 e2e
（§9.4 ⑤ macOS 档）PASS=16 FAIL=0：顺序 A/B 双序列、心跳三态、正/负路径点名、
`denied_publish = 0` 全程、断链自愈真拔 USB 双半段实锤。E7 排障四实锤落 §9.4 ⑥：
macOS 串口须用 `cu.*` callout（`tty.*` callin open EBUSY 与占用无关）；stty 配置
close 即丢、串口参数须与读写同 fd 生效（cat 直听见 magic `31 45 52 41` 即物理层通）；
macOS 允许重复 open 同一节点、残留旧桥静默截走数据（清场须 `ps` 核 0 残留，"桥进程
存活"须与 `uart link up` 同验）；桥 open CP2102 的 DTR/RTS 翻转复位 ESP32、哨兵 10s
注册窗口要求 rrd socket 先于任何复位点就绪。批D1 开口帧同步敞口随批E 实证关闭
（E7 全程无 BadMagic 复发——`Rr3UartConn::open` configure 末尾 tcflush(TCIFLUSH) 在案）。
树莓派档 e2e（§9.4 ⑤ 右列）板上线即补，不冒领

2026-10-11 0.0.7 批F 版本收口**纯文档批，零 `.rs` 改动**（ADR-0008/0018）——
status.md 重锚 `95d37ef`→`dc3a1d6`（0.0.7 批G 落档树）：锚点后的 60+ 提交
（0.0.5 增补轮四批W0–W7、第 II 轮批A–E'、第 III 轮批I–K、0.0.6 深挖收口批、
0.0.7 批A–G）按批分组计入在制品段；能力表新增**"非 Linux 主控契约 parity
（ESP32/xtensa，0.0.7 四层梯子）"行**——层① rr-ipc no_std + xtensa 编译期
golden 对拍（实跑 3/4 路）/ 层② UART 载体板级 128B 头+payload / 层③ espidf-
sentinel 上板三点堆 85% / 层④ rr3-bridge e2e 16 PASS + 矩阵行七格（⑤
bit-for-bit + ⑦ 200 拍判据窗），IPC-T-003 行补四路化注记、里程碑 M2 行同步；
**Q5 密钥门注销核对**：ESP-IDF 版本组合（esp-idf-sys 0.35.0 ↔ ESP-IDF v5.2.2 ↔
esp-clang 19.1.2 ↔ ldproxy 0.3.5）与 rsproxy 获取方式已随批W1/批D1 落 0.0.7
方案登记——批A"随批D1 新建 ESP32 笔记"的载体演变为此登记，独立笔记载体欠账
如实注记不阻塞；CI 八 job 首跑回执网页核对敞口维持（status.md 敞口表第 1 条）。
十步门禁直跑 GATE_EXIT=0（PASS 54）全绿；双远端 `ls-remote` 三端核对随本批
推送执行。**0.0.7 收官停等，下一版 0.0.8（立项在案 R17）待用户排期**

2026-10-11 CI 真源迁移小批（R30 裁定，0.0.7 收官后工程基础面，ADR-0008/0018）——
用户裁定"atomgit 没有流水线功能，改用 GitHub 的流水线"：R27 时代的
`.gitcode/workflows/` 单真源路径作废（atomgit 远端永不执行，八 job 首跑回执零取得），
`.gitcode/` 整目录删除，真源 `git mv` 回 `.github/workflows/ci.yml`（八 job 定义零改动，
头注释 R27→R30 口径订正）；`.github/README.md` 改写单真源文档（三远端口径：
atomgit/gitcode 推远端 + GitHub Actions 流水线）；活引用同步（tools/ci/README、
check.sh ⑥ 注释、本节 CI 侧步骤引言、brain 模块设计门禁口径、status.md 敞口表第 1 条
与 IPC-T-003 行链接、.gitignore）；R30 登记 0.0.5 方案 §2.4。八 job 首跑回执改押
GitHub：仓库待建+SSH key 待配，推送后网页核对归 status.md 敞口表第 1 条结项。
十步门禁直跑 GATE_EXIT=0 全绿；双远端 `ls-remote` 三端核对随本批推送执行

2026-10-11 CI 八 job 首跑修复批（H1–H4，0.0.7 收官后工程基础面，ADR-0008/0018）——
GitHub 仓 rockrobot3/rockrobot3 建成转公开 + SSH key 配好后 e0a8bad 触发首跑
5 绿 4 红（lint / conventions / ipct003 / distro-smoke 四红），五轮迭代修复至
run#6（38146782224）八 job 9/9 全绿：批H1 `484a13c` espidf-sentinel fmt 归一 +
rust-cache 工作区 pin；批H2 `f640234` rr-hal linux 单测七败真修复（GPIO context
点名、I2C 13-bit 换算测试正钉、nmcli 超时分支回错在前不读管道、PWM 空文件读零——
四后端确定性缺陷逐条纠正）+ lint 余项清零（rr-hal `result_large_err` 契约级豁免：
错型约 136B 系 Rr3ErrText 128B 定长文案契约面，装箱破 no_std、缩容动契约常量；
rr3-bridge c_char 平台 cast fn 级豁免：aarch64=u8 冗余 / x86_64·macOS=i8 必要，
不能裸删）；批H3 `adf3d16` ci.yml ipct003 job 插 espup 预编译装机步
（export-esp.sh 供 LIBCLANG_PATH）+ ipct003.sh 第④路默认在线直连 crates.io
（本机网络 workaround 收进 `IPCT003_CARGO_OFFLINE=1` / `IPCT003_CARGO_MIRROR=1`
环境开关，bash 3.2 空数组安全展开 `${extra_cfg[@]+…}`）；批H4 `8f33071`
distro-smoke ⑤ 点名断言（must_file/must_exec/must_dir/must_grep，败即报缺席路径 +
父目录 ls + dpkg -L 清单比对 + dpkg -V 异常项，断言集不变）→ `1c3cf41` 根因清除。
**撇号截断教训（run#5 点名回执实锤）**：distro-smoke.sh 的 ⑤-⑧ 整体在 docker
`bash -c '<串>'` 单引号串内——串内 ASCII 撇号后随空白即把 -c 串提前闭合
（`批E' R3` 注释实锤），余下脚本行溢出到宿主机执行（run#1-5 五连败根因：宿主裸
test 退 1 的静默假象，deb 装机内容自始无恙）；`'__pycache__'` 这类两侧无空格相邻
的引号对是"同词内闭合-重开"、文本留在同一 argv 元素内无害。判别实证法：bash
函数 stub 包一层 docker 打印真实 argv（ARGC=13 破口 / 12 修复）。修复后该文件
注释/文案一律禁用 ASCII 撇号（警示注释在案）。status.md 敞口表第 1 条（远端 CI
回执）/ 第 4 条（IPC-T-003 容器真跑，同批移入 envelope.schema.json coverage）/
第 8 条（distro-smoke 真装回执）/ 第 9 条（sim-assemble 六模块巡检回执）四条
同批结项。十步门禁直跑 GATE_EXIT=0 全绿；三远端 `ls-remote` 四端核对随本批推送执行

2026-10-11 批H5 审查修复批（0.0.7 收官后审查循环，ADR-0008/0013）——对六提交
（e0a8bad..e0cb31a）执行 TRAE-code-review 审查出 4 问题（1 major + 3 minor），
用户裁定全修：① `b2c28da` rr-hal linux_link 三测试 temp 目录 pid→pid+纳秒
后缀（`rr3_test_link_dir`）+ state() 套 ETXTBSY 瞬态重试（`rr3_test_txtbsy_retry`，
最多 2 次间隔 100ms，只认 "Text file busy" 字串、其余错误透传）——run#7
attempt1 实锤 ETXTBSY flake（fs::write 假脚本后立刻 spawn，ubuntu runner 偶发
os error 26，同代码 5 连绿 1 红），处置先例同 rrd supervisor 已知边界②（spawn
瞬态 Err）；同提交 nm() 正常退出后读管道改 `rr3_read_to_end_capped`（fd 置
O_NONBLOCK 轮询读到 EOF 或 deadline，各读独立一档 nmcli_timeout）——kill 只收
直子进程，幸存孙进程持写端时阻塞 read_to_end 会拖到孙退出，超时兜底被旁路
架空；② `749f7ba` ci.yml espup 安装 releases/latest → 钉 v0.17.1（latest
漂移破工具链可重现性，与 esp-idf-sys 0.35.0 / ESP-IDF v5.2.2 / ldproxy 0.3.5
钉版纪律对齐）+ distro-smoke ⑧ `grep -q boots` 恢复 JSON 键精确匹配
（`"\"boots\""`，双引号在外层单引号串内安全、无撇号截断，批H1 的子串降级面
回正）；③ 本条目 + status.md 在制品随 docs 落档提交。教训：CI 复跑即绿的
flake 须登记并等裁定，勿当场吞掉——本批即裁定后的加固落地。linux 门内代码
本机 macOS 不编译，用 aarch64-unknown-linux-gnu 交叉 check 做语法核验；十步
门禁直跑 GATE_EXIT=0 全绿 ×3（一提交一跑）。

2026-10-11 R31 裁定 + 0.0.5 增补轮五立项批A（全案落档，ADR-0008/0012/0014/0016/0018）
——用户版本制裁定原文："所有的功能完成都放在0.0.5，0.0.5如果没有提及的话就增加
新一轮次的大改，0.0.6是树莓派跟0.0.7是ESP32是验证功能的可成功运行"——功能归
0.0.5 轮次制（未提及功能→增补新一轮大改），0.0.6/0.0.7 定性为验证版，**0.0.8
不设功能版**（0.0.8 编号维持 brain 迁移既有排期，模块内部分层批0–批6 维持挂账
待排期，内功批不与新功能轮混排）。据此删除已起草的"0.0.8 交互功能实施方案"，
改挂为增补轮五全案（批A 交付：全案文档 + [esp32板卡笔记](../reference/esp32板卡笔记.md)
（批C⑤ R2 证据归档结项：console.log 拒收旁证 + 160B 物证 + 当日 cmp 复验两轮
一致，坏 magic 静默丢弃如实注记脚本 stdout 未留档）+ 0.0.5 方案 §2.6 五行裁定表
注册（唯一落点）+ 0.0.6 §5.4 BOM 九行→十行（第 10 行 USB 声卡+有源喇叭为批E
前置，Pi 5 无 3.5mm）+ status.md/README 索引同步）。全案四裁定：Q1=(a) UART
波特率参数化至 921600 + INMP441 双通道原始音频过桥（带宽账 512kbps < 有效
≈737kbps，超判据切 (b) 板上 DoA+单通道）；Q2=四件外设全挂 ESP32 协处理器
（OLED I2C / 舵机 LEDC / INMP441 I2S / MAX7219 VSPI 预留，宿主经 rr3-bridge
格位化接入）；Q3=本地 vosk（ASR）+ Piper（TTS），失败链 whisper.cpp small，
云后端位留空；Q4=USB 声卡+喇叭进 BOM 第 10 行。纯文档批零 .rs 改动；十步门禁
直跑 GATE_EXIT=0 全绿。

2026-10-12 0.0.5 增补轮五 批B 真板验收修复批（判据①②过，ADR-0018）——esp32-periph-base
真板首跑揪出 `oled_cmd` 8B 缓冲护栏误拒 `oled_init` 25B 流式命令序列：SSD1306 控制字节
Co=0/D/C#=0 表示其后全为命令，单笔 I2C 写合法（数据手册 §8.1.5.1）——扫描 0x3C 通、
400k/100k 双档 init 同败（同段代码）、panic_abort 下 expect 无消息 Guru 复位循环三现象
一并归因。修复：缓冲扩 32B + init 诊断面（Err 明细打印 + 100k 档 5×2s 重试逐次点名 +
终败不 panic 继续跑指令面；OLED 观测面降级放行、指令面各命令仍 fail-closed 回 ERR）。
教训两条：① **panic_abort（build-std 钉死）下 expect/unwrap 无消息直接 Guru**——真板
固件初始化失败处置不得 panic，须诊断面打印 Err 留证据（与哨兵 16KB 栈教训同源：板上
排障靠日志不靠崩）；② **长度护栏按最长合法序列设防**——护栏误拒合法输入即假故障，
I2C 物理层全通也会被自家软护栏拦死。判据①②真板过（扫描 [3C] + boot 画面两行目视与
串口回读一致）；判据③④（舵机五点/堵转 UBEC≥4.7V）待舵机接线。提交 `5280fe0` +
修复 `d98ff5e` 推三远端四端核对一致；专用门直跑 0 + 十步门禁直跑 GATE_EXIT=0 全绿。

2026-10-12 批B 增量批——OLED 4 行扩行（用户裁定"固件扩 4 行"路 B，提交 `81b30f1`）：
`OLED_ROWS` 2→4 = 文本 3 行（`OLED <text>` ≤63 字符按 21 列硬切，**可见 ASCII 过滤
前置**——字节切片前先归一，防 char boundary 劈裂 panic 且与 glyph 行为一致）+ 角度行
移 row3，命令形状不变向后兼容；boot 画面 4 行真板目视确认；配套 `scripts/mac_monitor.py`
Mac 实时数据演示件（CPU 温度/内存/IP/时间 1Hz 上屏，真板连发全 OK 回执 + 目视确认实时
跳动）。开口帧同步教训一条：**UART2 指令面 open 后首行指令会被 boot 期 RX 毛刺残行
污染**（探针实锤回 `ERR unknown cmd` 带噪声前缀）——宿主脚本 open 后先发空行冲残行再
发正式命令（与批E"复位板子+清输入缓冲"组合时序同源，宿主侧轻量替代）。

2026-10-12 0.0.5 第 IV 轮 MCP-0 契约级评审批（MC-0 合并件，开工门兑现）——**MCP-1**
`contracts/topics.toml` 29→30：append `rr3-brain/audio/tts_pcm`（stream L2，brain 侧 TTS
PCM 下行流，F-4 真人声通路的契约半边；载荷体**逐字段复用** `rr3-sense/audio/pcm` 已定形状
`{seq, format, rate, channels, ms, pcm_b64}`，`rr3_pcm_event_body` 为形状事实源；对 TTS
引擎中立——云 CosyVoice2 / 本地 Piper 同一份通路；上行 audio/pcm 与 audio/vad_start 字节
零改动，envelope.schema.json 与 128B 头不动）。与批C/批D **合成一次改表**（§18.4 防重规则
①）：批C 的 display/servo 与批D 的音频上行全部复用在册主题（face/expression、
servo/command|state、sense/audio/pcm），零新增登记——全轮仅此一次 topics.toml 变更。
**MCP-2 作废登记**：`[hardware]` SPI 扩键评审需求随 R31 Q2（SSD1306 由 ESP32 协处理器
I2C 承载）消失，Pi 侧不再新增 SPI 键位——作废留痕，别让"没做"读成"漏做"。**MCP-3**：
`rr3-act` module.toml 申报面变更（`[events].subscribes` 增 `rr3-runtime/system/power`），
低电禁动闸门消费点（MC-5 施工件），走 C14/R-3 同一评审程序。同批同步形状 pin 四处
（rr-registry topics.rs / contract.rs 的 30 条三元组 + kind 12-10-4-4 + merge 33、
rrd topics_load 两处 33=30+3）；评审记录唯一落点 contracts/README.md 演进段；0.0.5 方案
§2.7 尾注记 + status.md 在制品同步。教训复验一条：**形状 pin 是故意的硬断言**——计数
改动必须同批点齐全部 pin 位（含 merge 33 与注释文案），漏一处即红，这正是 pin 的设计
意图；rr-registry 38 条先单跑快验再进全量门禁，反馈环从分钟级缩到秒级。

2026-10-12 0.0.5 增补轮五 批C 收官（能力帧进城 + 宿主格位 + e2e）——编码面 `60a8b81` +
e2e 现场修复批 `bbc60e4` + 胶囊眼增量 `ff9de83`，三提交四端核对一致。路线二裁定落地：板
不进城不注册总线身份，宿主 HAL 桥后端 rr-hal::esp32_uart 长连接直发 `rr3.call`（src=1/dst=2
帧头约定，RR3_PERIPH_TASK=2 两端本地锚定 + 同值锚定测试防分叉），板上 esp32-periph-base
双面分流（首字节 0x31 走帧面 rr3_framebuf_read → rr3.call 分发 → Response 应答；字母首字节
走批B 行面，分流无歧义）。**帧面"失步"正式破案结项**：此前全部失步证据（`ERR unknown cmd`
带噪声前缀、line too long）根因是 pyserial 排障脚本组帧 bug——`hdr[44:44+len(payload)] =
payload` 把 payload 写进 128B 头保留区（44..128 须恒 0），Python bytearray 越界**静默扩容**
不报错；板侧判得完全正确（Header(ReservedNotZero) 拒帧 → payload 尾巴留 RX 环 → 落行面）。
教训钉死：**组帧 payload 必须追加在 128B 头之后**（`bytes(hdr) + payload`）；宿主 Rust
`to_bytes()` 起手 `[0u8;128]` 自动零填充从未有此问题，FrameBuf/UART 驱动/RX 环全部无辜。
真驱动 e2e 两连全绿：判据① display.draw(idle) applied=true、判据② frame() 读回一致、
判据③ servo round-trip 90/45/135/90 逐度一致，console 八帧零丢弃。修复批两件：rr-ipc 新增
`rr3_flush_input`（TCIFLUSH 只冲收不冲发）+ `rr3_ensure` settle 后补冲（开口 DTR/RTS 复位
让板端 banner 在开口 flush 之后才进队，批C e2e 实证 BadMagic）；draw() 成功即置 probed=true
订正 frame() 探活擦屏（冷启动全零位图探活帧会把屏上现帧擦掉——探活语义保留给冷启动先
frame() 后 draw 的路径）。**胶囊眼增量**（用户裁定"有生命感"）：OLED 表情改圆角竖矩形
胶囊眼（圆角半径 = min(半宽,半高)，满高 26px 纯胶囊、眯缝压扁不退化成圆盘——行内实心段
解析求宽逐页写 GDDRAM）；cmd_loop 窥视读改 1-tick（10ms）超时，空口 Ok(0) 驱动 anim_tick
自主微动作（眨眼 130ms / 2.8–5.2s 随机间隔、扫视 dx±6px 500–900ms 回位，esp_random 硬件熵；
指令线程单线程独占 I2C/OLED，动画只能搭窥视空档）；七态表情基态预设；固件 v0.2.0→v0.3.0，
真板 idle/think/idle 三轮 round-trip 应答全对 + OLED 目视确认。教训两条：rr3.call 请求体
args 在 **params 内**（`{"params":{"topic":...,"args":{...}}}`——args 放顶层报"缺
expression/frame 之一"）；专用门 IDF_PATH 无条件 export v5.2.2 盖外部环境——切换 IDF_PATH
必须先清 esp-idf-sys-* 的 CMake cache 目录再重建（cache 源路径不匹配即翻烙饼），且
sentinel 真身在 esp-idf/v5.2.1/ 子目录（传父目录被判 not a git repository）。专用门 +
十步门禁 GATE_EXIT=0 全绿；敞口：判据④（堵转 UBEC≥4.7V/板子不复位）待舵机接线窗口，
读侧阻塞无超时归批E 桥面统一收口。

2026-10-12 0.0.5 增补轮五 批M M-α 收官（MC-9 + MC-8②③：台账诚实化 + 回显型参数与自述
一致）——编码面 `ba90367`（20 files，+132/−43），十步门禁直跑 GATE_EXIT=0（54 PASS）全绿，
推三远端 ls-remote 四端指针核对一致。三件交付：**MC-9 判 (a)**——`contracts/slots.toml`
`net.bt` 格 `normal`→`placeholder`（note 写明触发条件 = 配网备选通道立项 / BlueZ 真后端批，
承诺 ≤ 事实），bt 件 SURFACE 与三处注释同批写明"骨架，不 claim 射频"，`rr-registry` 契约
测试补 status 数据位断言并钉"其余 18 格 normal"——21 格 = 18 normal + 3 placeholder
（sensor.env/sensor.imu/net.bt）；⑧-a 门只读数据位、脚本零改（"门不许 if 件名"的判据天然
成立，sensor.env/imu 先例同构）。**MC-8②（B-10）**——brain `API_KEY_ENV` 改通用键
`RR3_CLOUD_API_KEY`（原 `SILICONFLOW_API_KEY` 把云供应商焊进代码），供应商归 config
`[rr3-brain.cloud]` 的 `base_url`/模型名（换云只改配置不改码），测试三处 + config 头注 +
brain README 两行 + conventions 密钥条目同批换名。**MC-8③（I-9/I-12）**——interact panel
立 `RR3_PANEL_ROUTES: &[(&str, &str)]` 端点总表为唯一真值：启动自述与 serve 行的端点数由
`RR3_PANEL_ROUTES.len()` 推导（手写数字消解），表↔分派漂移由 `route_table_matches_dispatch`
单测钉死（表内逐条路由可分派 + 表外 NotFound 反例 + 计数 pin）；SURFACE 由 `&[&str]` 常量
改 `rr3_panel_surface() -> Vec<String>`（const 上下文不能 `format!`，推导数字必须运行期
组装），composite.rs 启动自述打印拆两段循环保序；静态页头注释撤计数改指向后端表（页面
fetch 端点字面量是功能调用非文案，保留）。教训三条：① `matches!(d, …)` 会 move，后续
`"{d:?}"` 报 borrow of moved value——借引用匹配 `matches!(&d, …)`；② 仓根无 workspace
清单，fmt 需按 crate `cargo fmt --manifest-path` 跑；③ 文档计数 pin 位同批点齐（模块设计
索引 §1 切分表/§8 待裁表、brain/interact 设计册、三份 README），漏一处就被断链门或下轮
对表抓。敞口：MC-8① `cutoff_voltage_mv` 归 M-ζ 与 MC-5 低电禁动同批联动实装（Q2 裁定）。

2026-10-12 0.0.5 增补轮五 批M M-β 收官（MC-11 四件：L-6 / A-6 / S-3 / I-4，判据
"已实装行为有护栏、卡死与静默停摆看得见"）——编码面 `3bc30f1`（8 files，+388/−12），
十步门禁直跑 GATE_EXIT=0（54 PASS）全绿，推三远端 ls-remote 四端指针核对一致。四件
交付：**L-6**——relay/state 同态抑制 RPC 路 e2e 断言补进 `e2e_act` ④b（intent 路 ⑤b
批I 已有；先红后绿走破坏性验证：临时解除 RPC 路抑制→红→恢复→绿，证断言真有牙齿）。
**A-6**——`reflex.rs` 窗结算 `reflex_ticks` 落账（值 = 窗内实跑节拍数），`e2e_act` ⑧
经 `rr3.ctl.counters` 有界轮询点验（先红后绿）。**S-3**——`rr3-sense/composite.rs`
`pcm_frames`（发布成功一帧 +1）/ `pcm_empty_frames`（空帧另账）两键，`e2e_audio_chain`
⑤ 同法点验（先红后绿）。**I-4**——panel spawn 失败改"弃置该连接 + `panel_spawn_fail`
点账，accept 循环继续服务"（旧语义 `?` 上抛使面板静默消失），`rr3_panel_accept_loop`
抽缝 + `Rr3ConnSlot` RAII 守卫管 `live` 计数（Drop 归还，panic 展开不泄漏）。教训五条：
① macOS 对 `thread::Builder::stack_size(1)` **静默 clamp 使 spawn 成功**（线程带极小栈
一跑即溢出 SIGABRT），确定性必败注入须 `stack_size(usize::MAX)`（EINVAL）；② `matches!`
/闭包 move 后 Err 分支再用原值报 E0382——spawn 前另 clone 一份给记账分支；③
`rr3_body_split` 返回 `Result` 非 `Option`，`.map_err(|_| ())` 而非 `.ok_or(())`；④
A-7 守恒核清：`tools/sim/chain.rs` 观察者只收 vad/servo、legacy 面板 relay 按钮只调
`/api/relay`，两处均不按帧数 relay/state——零改，落档注记即可；⑤ I-4 账平断言在门禁
全量并行下撞过一回调度窗（53 PASS 单红，同日另撞 `e2e_spawn_restart` 与 rr-ipc uart
重开两条已知时序 flake，均复跑两连绿定性）——"handler 写完应答"与"线程退出归还槽位"
之间无同步点，读到 200 不等于账已平，断言改 1s 上界有界等待（`91b473c`，产品语义零改）。

2026-10-12 0.0.5 增补轮五 批M M-γ 收官（MC-6 配网凭据持久化 + MC-4 探测真值接线）——
编码面 `084d081`（14 files，+1224/−193），十步门禁直跑 GATE_EXIT=0（54 PASS）全绿，
推三远端 ls-remote 四端指针核对一致。**MC-6**：配网凭据由**后端承载持久化**（件内不写
文件、不出现落盘路径字面量）——`rr-hal` 新增 `nmcli_support`（自 `linux_link` 抽出公共
nmcli 机制：`rr3_nm_unescape`/`rr3_nm_field`/超时守护 `rr3_nm_run`/capped read，全平台
编译）+ `provision_store`（`Rr3WifiStore` trait 三后端 Nm/Sim/Unavailable + 选择函数
`rr3_hal_wifi_store`）；`rr3-connect` 的 connect/forget handler 先过 store 再转状态机
（fail-closed），`Rr3Provision` 只管态不管存，新增 `rr3_provision_restored` 重启恢复
station（psk 不回读——NM 私有面）；主判据 `nm_store_persists_across_reconstruct`（临时
`NMCONF_HOME` 假 nmcli 夹具，主机可测）。**MC-4**：`Rr3LinuxProbe` 五面在位性实装
（sysfs `pwmchip*`/`gpiochip*`、`/dev/i2c-*`、`/dev/spidev*`、`/proc/asound/cards` 非空），
去 cfg 门 + 探测根可注入（生产 `/`、单测假根）；格位→面映射（有硬件面者按面探，软件格
`cognition.*`/运行期格 `net.*`/基础层回"无硬件面可反证 → 在位"）；`rr3_probe_default`
新增 `RR3_PROBE`（`sim`/`linux`/`absent`）**显式优先**（取值不认当场 `eprintln!` 点名后
回落），否则按目标分流（test→sim / linux→真探测 / 其余生产面→恒缺）——**sim 探针不再
当缺省构造**（台-1 清零）；`profile.rs` 加注入缝 `rr3_profile_boot_inputs_with_probe`
+ 2 条"缺格 → 点名件与键"门禁用例。教训两条：① `Rr3AbsentProbe` 须**按面判**——非 linux
生产面无硬件探测面，但有硬件面的格位恒缺（不冒充在位）、**无硬件面可反证的格位（`net.wifi`
等）与真值探测同判在位**（"没有反证"≠"拿不到答案"），否则 e2e_connect 的纯软件格位被
连坐判缺；② e2e 走**真 rrd 子进程**（非 test 编译面），开发机（macOS）与 CI runner 的
硬件格位会被 fail-closed 判缺——`e2e_act`/`e2e_profile`/`e2e_power` 与 `tools/sim` 装配
演练一律**显式** `RR3_PROBE=sim`（与既有 `RR3_HAL_BACKEND=sim` 同一纪律：不赌平台默认）。

2026-10-12 0.0.5 现场处置 方案C（分轴限幅——真板云台撞机械死点/BOD 复位故障的软件面收口，
计划外修正非批M 工单）——编码面 `f6e92d6`（7 files，+150/−55），xtensa 专用门 PASS（真编译
29.42s）+ 十步门禁直跑 GATE_EXIT=0 全绿，推三远端四端核对一致。**故障链（真板实测）**：云台
舵机卡死不动 + 发烫（SG90 堵转 ~700mA/只持续流动），5V 取自板载 USB/5V 排针经拓展坞直供 →
5V 轨跌落 → **BOD 复位循环**（`Brownout detector was triggered`）→ 串口节点与 OLED 反复掉线；
软件加剧因素 = 板侧 WANDER 全行程 0–180 折返 + SCAN 五点 0/45/90/135/180 把舵机推过机械死点
并持续施力。**落点（板侧固件为主 + 宿主 config 同步）**：① 板侧 `main.rs` 立**分轴行程**
（`AXIS_CENTER_DEG=90` / `PAN_LIMIT_DEG=20` / `TILT_LIMIT_DEG` + `Axis::{name,limit_deg,range}`
+ `axis_scan_points` 区间五等分判据点），`Periph::servo_set` 加本轴行程裁决、越程**点名拒收**
（fail-closed，不静默钳位）；WANDER 端点折返与 SCAN 判据点一律取自 `Axis::range`
（pan[70,110]/tilt[85,95]）；INFO 增 `lim=pan[..]/tilt[..]`、HELP 加行程区间。② 宿主
`modules/rr3-act/config.toml` `angle_limit_deg [0,180]→[70,110]`（本面默认轴 pan；改
`RR3_HAL_PERIPH_AXIS=tilt` 须同步）+ composite 仓形状夹具（漂移当场红）。③ 必要连带（不改则
"向左/向右"被新行程拒收）：brain 词表转向三档 45/135/90 → 70/110/90 + `test_intent` 单测；
`--chain` 演练 `RR3_CHAIN_LEFT_ANGLE 45→70` + config 文案/断言；`e2e_act` 夹具 [0,180]→
[70,110] + 左档落角 45→70。**核并改**：板侧既有语义是**「点名拒收」不是「钳位」**——与
rr3-act 设计册 §5 主题面旧文「越界→钳位后成功」对不上（件面 `rr3_servo_check_travel` 实为
拒收，`rr3_travel_clamp` 只是安全核被绕过时的兜底、合法输入恒等），设计册同批核改。教训：
"全行程折返"类演示动作必须与执行器机械死点解耦——行程上下界由**数据**（分轴常量/config）决定，
不许硬编码 0–180。

2026-10-12 0.0.5 现场处置 方案C 修正（tilt 限幅 ±5°→±10°）——编码面 `8f750b2`（3 files，
+11/−10），xtensa 专用门 PASS（真编译 25.78s）+ 十步门禁 `TEN_GATE_EXIT=0`，板上已烧录
（460800），HELP 回读证实「行程 pan 70-110 / tilt 80-100」。**板侧** `TILT_LIMIT_DEG 5→10`，
tilt 行程 [85,95]→[80,100]（保住现场所需俯仰上抬 98°——原值会被 fail-closed 点名拒收），连带
HELP 文案 / SCAN 五等分点注释（tilt 80/85/90/95/100）/ wander 分轴限幅注释。**补方案C 批漏改**
（`8f750b2` 同批）：`modules/rr3-act/config.toml` tilt 注记 [85,95]→[80,100]、
`tools/esp32-periph-base/README.md` 指令表与验收判据（旧文仍写「单轴 0–180」与五点
0/45/90/135/180，与主落点不一致）。裁定：用户放宽 tilt 对称限幅以纳 98°（pan 保持 ±20° 不变）。
**敞口（未闭环）**：板载 USB-UART 在复位/欠压瞬间掉出 USB 总线，只能物理重插恢复；UART2 指令面
（外部 CP2102）双向字节污染（非固件 bug，自查清单已给用户：共地 / DTR-RTS 勿误接 EN-BOOT /
逻辑电平须 3.3V / 短线）。**方案 A**（整板电压监测 + 分轴限幅）另行立项评审，与本处置并行。

2026-10-12 0.0.5 现场处置 板侧验收 + 方案A 裁定——真板验收取证（板载 1410 控制台 / UART0 + 外部
CP2102 0001 UART2，pyserial 持 DTR/RTS 去断言开 1410 硬复位抓 boot）：①控制台状态行 `[periph]
surface=esp-rs-std crate=esp32-periph-base oled=i2c0@21/22 servo=ledc@32/33 cmd=uart2@115200
line+frame` + boot 全档（新镜像 ELF SHA256 `1e330dd12…`、编译 `Oct 12 2026 10:51:03`、`rst:0x1
POWERON_RESET`）；②I2C 扫描 `[3C]`（OLED 在列）、OLED 400k init 失败 → 降 100k 第 2 次重试过 →
`boot 画面已出（4 行）`（硬件通、信号边际）；③LEDC `pan=GPIO32 tilt=GPIO33 中位90° duty
pan=1229 tilt=1229 (max=16384)`；④**UART2 INFO 限幅字段取证 `lim=pan[70,110]/tilt[80,100]`**
（新固件 tilt 80-100 板上落实）+ `wander=off phase=0 oled_last="(boot)"`；⑤UART2 双向字节污染仍在
（我发 `INFO`/`HELP` 有一行到达即被污染 → 板回 `ERR unknown cmd`；非固件 bug）。**用户两症状软件侧
定位（均非固件 bug）**：①OLED 无"胶囊眼" = `oled_last="(boot)"`（自 boot 起未收 face/expression；
胶囊眼是帧面 `rr3-interact/face/expression` 专有，走 `frame_handle_face`→`oled_capsules`），OLED
本体完好；②舵机发烫/未到位 = `pan=tilt=90`、`wander=off`（板侧未驱动），"未转向" = 命令经 UART2
上行被污染 → 板收不到 → 停 90，发烫属物理/供电/装配（头号嫌疑：舵机未走 UBEC 独立 5V 轨，0.0.6
BOM #3 记待采购）。**方案A 裁定**：用户 2026-10-12 授权"详细研究后按最优方案裁决" → Q1–Q6 逐条
采纳建议（Q1 采点 = 5V UBEC 轨·ESP32 ADC1 空闲通道 GPIO34–39·与既有验收判据④「堵转 UBEC≥4.7V」
共用同一测点 / Q2 双阈值 + 迟滞·复用 `Rr3LowEdge` 判定核 / Q3 板侧为主 + 宿主留同语义位 / Q4 config
升 pan/tilt 双区间〔形状变更，须契约级评审前置〕 / Q5 先占位阈值 + 真板两点标定回填 / Q6 并入 0.0.5
现场处置续批并与 M-ζ MC-5 合并为一个动作）。施工未开工（Q4 契约评审前置）。详见
[方案A §8/§9](dev/方案A——整板电压监测与分轴限幅立项评审.md)。

2026-10-13 0.0.5 批M M-ζ 开工（MC-5 低电禁动闸门 + MC-8① cutoff 判 (c) + MC-10 L-1）——**MC-10 L-1**：
`tools/ci/matrix.sh` 此前**无任何调用方**（不在 `check.sh`、不在 `ci.yml`），点验矩阵出数入口形同虚设；
本批新增 CI 第九 job `matrix`（`runs-on: ubuntu-latest` + stable 工具链 + rust-cache，`run: bash
tools/ci/matrix.sh` **非 `--strict`**——⑦ 时序达标无固定载体（E4 真板专属），默认口径如实记"未跑"、
退 0；矩阵出现 FAIL 列才退 1），产出**人工搬进** `docs/status.md` 矩阵行（出数 ≠ 判据，判据真源仍是
`tools/ci/*.sh` 与 `check.sh` 十步）。`.github/README.md` "八 job"改"九 job" + ⑨ 行补录；本文件 CI 侧
步骤引言"八 job"改"九 job"。本机实跑 `matrix.sh` 回执 PASS=4 / 未跑=3 / FAIL=0（① Docker 缺 3/4 路、
⑥ python3 3.9.6<3.11、⑦ E4 真板专属——无载体如实记）。**MC-8① 判 (c)**：`battery.cutoff_voltage_mv`
由"在册 + 解析 + `println` 回显 + 无动作"中间态结清——键保留为策略数据、**明写"本版不关断"**（关断
动作归 M6 关机链），**机器读者 = 注入门**（`assemble.toml` ops 槽 → `rr3_registry_assemble_injection_gaps`
按点路径取值、⑧-c 门仓级强制；`println` 回显不算读者），见方案 §20 第 2 条 ⑤。**MC-5** 低电禁动
闸门：`rr-mech` 加 `Rr3LowGate` 原语 + `Rr3LowEdge` 读出面，`rr3-act` 消费 `rr3-runtime/system/power`
广播置闸——执行面（servo/relay 的 RPC/intent 两路同判）拒动、妆面（led）不禁、拒因点名低电禁动。
十步门禁直跑全绿；推三远端 `ls-remote` 四端核对随本批推送执行。

其余规约（命名、分层、职责边界）靠评审；命名唯一权威是 [naming.md](naming.md)。

**十步之外的 CI 侧步骤**（`.github/workflows/ci.yml` 拆步 job——0.0.5 增补轮四
批W2 落八 job，0.0.5 批M M-ζ MC-10 L-1 增至**九 job**；R30 裁定（2026-10-11：atomgit
无流水线功能，CI 改走 GitHub Actions）
起 CI 单真源迁此；不进十步门，本地可跑）：
`lint`——rustfmt `--check` 与 clippy `-D warnings` 各自独立失败，覆盖 base/runtime 与
全部独立工程（④ 同一 find 口径）；绿色基线是 2026-10-01 的 rustfmt 全仓归一 + clippy
清零提交，此后 fmt/clippy 回退即 CI 红。`brain-unittest`——rr3-brain 26 条 stdlib
unittest（`python3 -m unittest discover -s tests`），ubuntu + macos 双矩阵；brain 不在
Rust workspace 里，①b/②b/④ 覆盖不到它，只有这一步跑。四真进程 e2e 仍属本地实跑
环节（同下节平台条件脚本的理由）。

**十步之外的平台条件脚本**：`bash tools/ci/ipct003.sh [--strict]`（IPC-T-003 跨架构对拍，
ADR-0006 后果项）**不在十步门内**——它依赖平台条件（Docker / aarch64 交叉目标），
塞进门禁等于让无容器环境的机器永远红。它做四件事：①宿主跑 `rr-ipc` 的
`src/bin/ipct003_dump.rs`（golden 头 + 五种 payload 类型各一整帧的 hex，全部编译期字面量），
header 行对 `contracts/golden/event-header-128.hex`；②已装 `aarch64-unknown-linux-gnu`
目标则交叉 `cargo check`，header.rs 的编译期 const 断言（偏移连续 / 头长 128 /
magic LE 字节 31 45 52 41 / payload 枚举值）随之在该目标求值；③有 Docker 则
`--platform linux/arm64` 容器真跑并与宿主输出逐行 diff；④已链 esp 工具链（espup
装机，CI 侧装机步见 ci.yml 的 ipct003 job）则 `cargo +esp check` 在
xtensa-esp32-none-elf 目标编译——golden 头 const 对拍在该目标编译期求值
（IPC-T-003 第④路，0.0.7 批A' 接入；cargo 默认在线直连 crates.io，本机网络 CDN
僵死时 export `IPCT003_CARGO_OFFLINE=1` / `IPCT003_CARGO_MIRROR=1` 恢复离线+镜像
旧行为）。缺环境打 `SKIP` 并明说，
不静默当作已验证；`--strict` 供有容器环境的一方在 SKIP 时拒绝放行（退 2）。
双架构真跑结果何时回填，见 [../status.md](../status.md)。
2026-10-01 T4：`--strict` 模式已接 CI——`.github/workflows/ci.yml` 新增 `ipct003` job
（ubuntu-latest：x86_64 宿主 + `aarch64-unknown-linux-gnu` 交叉目标 + Docker
`rust:1-slim` 容器经 QEMU `--platform linux/arm64` 真跑），四步齐备无 SKIP 才绿；
远端跑绿后把 envelope.schema.json 的 IPC-T-003 移入 coverage 并回填 status.md。
本机（无 Docker）仍按宽容模式跑 ①②，③SKIP 照登。
2026-10-01 F5 本机实况：Intel Mac（macOS 13，无 Docker）跑出 ①PASS（宿主 x86_64，
dump == golden）②PASS（`aarch64-unknown-linux-gnu` 目标已装，交叉 check 通过，const
布局断言已在 aarch64 目标求值）③SKIP（本机无容器环境）。三方对拍的第三路真跑待
换机后补跑：`rustup target add aarch64-unknown-linux-gnu` + Docker 就绪的环境上
`bash tools/ci/ipct003.sh --strict`，全绿后在 [../status.md](../status.md) 回填。
2026-10-14 0.0.5 增补轮六立项（**文档轮，零 `.rs` 改动**）——承
[ADR-0021](adr/ADR-0021-三独立形态与资源供给面-能力原子化补课.md)：增补轮五在 ESP32 实机把四类抽象欠账
以硬件损失付了利息（5V 轨被拉塌致 BOD 复位循环、屏点不亮、舵机堵转发烫、`board.rs` **50 个**（实测计数）
板私有常量无契约归属）。本轮把 16 条坑逐条**升格为五处之一**（供给位 / 能力面 trait / 描述符键 /
格或主题 / 门禁判据）——落点表见 [全案 §1](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)。
本仓口径由此变更四处：① **供给由六位扩七位**（能源域 `[[supplies.domain]]` + 件 `[[atom.demand]]`）；
② **描述符由七段扩八段**（`[pins]` 角色→槽位绑定），且 `[hardware]` 九键**闭集改判**为
"由 `contracts/roles.toml` 角色词表导出、未知键仍拒"（0.0.5 §13.1 的 **C4"不扩键"裁定作废**，
`schema_version` 需 2→3，属破坏性契约变更）；③ **§12.8 三处窄缝之外新立接缝四 资源与语义缝**，
"部署 = 写两件码 + 交七段描述符 + 填一行矩阵"这句改判为"**三件码 + 八段 + domains + 一行矩阵**"，
并写明 `esp-idf-hal` 具名引脚类型那处**不可消除**的不对称；④ **三形态独立**：RK3566 / 树莓派 / ESP32
两两零关系，"外设挂 ESP32 协处理器"的定性作废（施工事实保留）。
拟增 ⑨⑩ 两步见本节上方"拟增两步"段——**建成前不得当作现有强度引用**，且 ⑩ 面板覆盖率门
建成的那天必然是红的（今日 14 端点无一读配置），红项按条登 status.md 敞口表，
**不许靠缩小可配置集让它绿**。
同批回写：architecture 升 **v0.11**（§3/§4.3/§5/§11/§12.2/§12.3/§12.6/§12.7/§12.8/§13/§14）、
模块设计索引（§3 补第四条、§8 新增 D10–D15、§10 两轴读法、§11 新增 L-11/L-12/L-13、§14 新增 14.6）、
六册新增工单 power P-9/P-10、act A-13/A-14、interact I-15/I-16、sense S-9/S-10、connect N-10、
brain B-12；[增补轮五](dev/0.0.5增补轮五——交互功能落地全案.md) 的"与 0.0.7 实测接线零冲突"一句
按 `board.rs` 实锤**订正为已被推翻**。architecture §11 新增风险行自此用 **`RK-` 前缀**（该表原 `R1…R16`
与 0.0.5 裁定 `R1–R37`、索引裁定 `R-1…R-16` 三套编号已在同一份 status.md 里互相指错，
存量不回改、增量先消歧，登记为命名敞口）。
2026-10-14 0.0.5 增补轮六**同日二轮**（主线一致性复查 + 裁定落档，仍零 `.rs`）——用户口径
"以机制和策略分离为主线，尽可能找出阻碍生产、运维的问题"。产物三类：
① **D10–D15 六条全部裁定**（只有 D10 取值是可撤回设计默认），顺带结清 **D3/D5/D6/D7/D8** 五条同族待裁；
结论行落 [模块设计索引](../design/模块设计索引.md) §8、**细则唯一落点 =
[增补轮六全案](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §12**（不复述）。
② 三条**纪律层自指失败**登记为索引 §11 **L-14/L-15/L-16**，全部本轮 `grep` 自核过符号现存性：
器件级探测缺失（`rrd/src/probe.rs` 的 `Rr3LinuxProbe::rr3_face_present` 对 I2C 只判 `/dev/i2c-*` 存在、
从不问从机 ⇒ `power.charger`/`sensor.env`/`sensor.imu` 恒判在位）；**"零策略载荷"有两套禁词表**
（`rr-registry/src/atom.rs` 的 `RR3_BOARD_TOKENS` 含 `gpio/i2c/spi/uart/pwm/bcm`，
`tools/ci/check.sh` ⑦-a 的 grep 模式不含这些词；且 ⑦-a 执法前 `sed 's://.*$::'` 剥注释，
`rr-hal-api` 里 `Rr3FaceFrame` 的"MAX7219 行寄存器直排"对门永不可见）；
**"N 册同批"句式造无主项**（实测 `RR3_POLL_WAIT` 只在 act/connect/interact/power 四份 `composite.rs`，
sense 无此符号，"五册同批"的事实基础即错）。
③ **§15.1 十条无主项逐条指派工单号**：act A-15、interact I-17、sense S-11、connect N-11、brain B-13、
power P-11（细则只在全案里，册内挂单号 + 指针，避免第二真值点）。
门禁面由此从六门扩到**十门**（拟增 h/i/j/k，见本节上方"拟增三步"）——其中 **门 k 最便宜**：
它拦的不是代码，是"看起来有归属"。
**同时核掉两条子代理误报**（落档前自核的产物，登记在全案 §15.4）：一处处数断言未逐字核到故不写入裁定；
`[rr3-brain] log_level` 只核到"零读者"这一半，"未进 §13 清单"半句撤回。
2026-10-15 **全代码逐 crate 复盘 + ADR-0022 落档**（文档轮，零 `.rs`、零契约文件改动）——用户口径
"对目前所有项目代码做一次详细复盘，按已达成的共识与工程思想自主决策修改整体设计方案"。
取证方式：四路并行审查（六单元 / 基座九 crate / 板侧与工具链门禁 / 契约与装机运维面），
**每条要写进设计的断言我都自己 grep 或读码复核**，撤回与收窄项单列在
[全案](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §16.2。
核实成立的主线违例五组（索引 §11 **L-17…L-21**）：机制核携策略窗（`rr-mech` 的 `rr3_reflex_window_plan`
内 `250u64`）、契约层携部署缺省（`rr-ipc` 的 `RR3_UDS_PATH_DEFAULT`/`RR3_SHM_NAME_DEFAULT` 对 none 目标
仍无条件再导出）、跨形态契约携器件缺省（`RR3_AUDIO_CFG_16K_MONO16` 的 `channels: 1` 对上实机双麦）、
基座 crate 钉某块板（`rr-ota` 两处 `include_str!(…radxa-zero3…)`）、**申报面可内联逃逸**
（`tools/espidf-sentinel` 内联 manifest exposes `rr3-sentinel/diag/parity`，该主题**不在** `topics.toml`，
而查册门只作用在 `modules/*/module.toml`；装配扫描亦只遍历 `atoms/*/assemble.toml`，组合件层零消费者）。
另有两组运维结构性空洞（生效绑定 `rr3_hal_bindings_effective` 全仓无外部读者、日志环 512 行不落盘）
与一组产线缺口（打包与回滚只覆盖 host aarch64，**ESP32 形态零烧录封装/零 OTA/零回滚**）。
落档：[ADR-0022](adr/ADR-0022-代码事实复盘-机制核纯净度与申报面逃逸与安全语义身体.md) 裁定 Nn-1…Nn-6；
architecture 升 **v0.12**（第四条增长律、`[hardware]` 条件必填、装配单自洽 + 可达终点表、§13 新增三把待建门）；
待建门由十扩为**十五把**（新增 i/j/k/l/m，见本节上方）；六册新工单 **A-16 / S-12 / I-18 / N-12 / P-12 / B-14**；
施工新增**批N5（生产与运维闭环）**，其首件"按形态分档可达终点表"不依赖新契约、可先行。
同批订正两处**计数与文案**：`board.rs` 板常量数按实测 **50** 订正（ADR-0021 原写"约三十个"）、
`contracts/README.md` 的历史计数行加现值注（不改写历史）；`rr3-act` 的 `move_timeout_ms`
注释与 println 降级为"入参时限裁决"（A-16 ①）。**本批复盘不产任何板上证据**，
所有"已核实"指的是代码文本，不是行为。

2026-10-15（同日第四轮，**证伪审查**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。用户口径
"继续对整体方案做细致检查，对每一个具体设计做证伪审查"。本轮不做完备性复查（§15/§16 已各做过一轮），
只逐条回答一件事：**"什么观测能推翻这条设计"**，并把反例构造到
"不改任何硬件形状、只改装机档或一句话就能让门变绿"的粒度。
**32 条命中，其中 14 条打在 ADR-0021/ADR-0022 与本全案自己写的裁定与编排上**；撤回 7 处、
审过成立 10 条分别单列全案 §18.8 / §18.7。三类新失败形态进索引 §11（**L-22 / L-23 / L-24**）：
**L-22 自愿填写的字段是注释搬了家**（首版十条校验**没有一条写缺失语义** ⇒ 全部可被留空绕过；
`mux_group`/`failsafe`/`rated_a` 不填即不判，而坑 1 那种"不同脚但同外设矩阵"的冲突本就抓不到）；
**L-23 新设计自带第二真值点**（同一个"哪个信号接哪个硬件"事实四套语法并存——件 `handles[].desc_key`、
`[hardware]` 九键、`[pins]` 字段、`sense="adc:gp34"` 串，而门只判新加的那一套；
实读 `rr-hal/src/backend.rs:327` 取 `b.pwm_chip?`、`atoms/servo/assemble.toml:9-11` 申报 `pwm_chip`+`pwm_channel`）；
**L-24 判据恒真是判据缺失**（"并发窗口内"无数据源 ⇒ 恒可满足；门 ⑨-h 落在只有 `atoms` 的空段 ⇒ 建成即绿；
门 ⑩ 分母按角色计 ⇒ 一个端点刷绿一片；"`ps` 看不到环容量常量" ⇒ 编译期常量从不出现在 `ps e`，恒真；
ADR-0022 裁定 3.1 用 `kind != logic` 判必填而 `logic` 从未进过取值域 ⇒ 不可判定）。
**六处改判**：`[hardware]` 六物理键**下移**为 `[pins]` 派生视图（新门 ⑨-n 注入路径唯一）·
`pin_id` 等式定义并让接线档 / 能源域绑定位 / 轴档案一律引它 · 新立契约文件 `contracts/devices.toml`
（新门 ⑨-o；它是"速率取值域按器件在册"、L-14 器件级探测的寄存器 ID、门 ⑨-g 的型号词表三处的共同前件）·
§5 校验补缺失语义与后果三态两列并把口径写成"**十条 / 24 个判定位**" · 门 ⑨-a 由四表改判**六表**
（`roles.toml` 补 `iface`/`requires`，否则"∃trait(role) / ∃key(role)"两条无判据对象；`kind` 补 `logic`；
**撤名 `Rr3PowerDomain`**——§3.1 两处对同一 `rr3_switch` 给了两个 trait 名且它从未定义）·
门 ⑨-f 改"一套词表 + 两张在册豁免域"（撤回上一轮"合一处即可收敛"）。
**待建门由十五把扩为十七把**（⑨-a…⑨-o 十五 + ⑩ + ⑪）。六册新工单六个：
**P-13 / A-17 / I-19 / N-13 / S-13 / B-15**；architecture 升 **v0.13**。
**同轮撤回自己写进去的东西（这一节的存在理由）**：ADR-0022 裁定 2.4 的"同格双件可静默并存"**前提为假**——
`rr-registry/src/atom.rs:530-536` 的仓扫描反查测试当场 `panic!("格 {slot} 被两件抢占")`；
真位点是 `rrd/src/daemon.rs:357-369` 明写的"providers **含被拒模块**"（ADR-0009 有意语义），
缺的是可观测项而非唯一性门。另撤回：§17 烧录判据（把承载档与链路两轴混回一处）、
§17 "`ps` 看不到常量"（恒真）、门 ⑨-f 的收敛方案、D13 的依据句"C 不需要新硬件就能收口"
（C 的自动段与 B 同依赖电流上限，N2 之前只准单步点动 + 人工确认）。
**方法论记一句**：本轮全部命中里近半数打在提出判据的人自己写的判据上——
**尺子必须放进被审对象**；若一轮证伪审查只找出别人的漏洞，它找出的多半不是漏洞，是偏好。

2026-10-15（同日第五轮，**跨文档一致性 + 运行期语义审查**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。
前四轮打完"代码事实 / 判据可判定性"，这一轮打两件从没碰过的事。
**(A) 口径传播**——机械 `grep -c` 数新裁定留下的旧字面量，查出三条真缺陷并已就地修完：
**X-1 "十项"在本仓有两个所指**（装载侧"十项校验" vs 产线侧"可达终点表的十项"）⇒ 统一为
"**装载期十条校验**（24 个判定位）"与"十项"两个名字；**X-2 待建门有两个分母且都没写口径**
（本档设计编号 13 行 a–o vs `check.sh` 拟步 17 把，两套字母**错位一位**：门 h→⑨-f）⇒ §16.4 分两行列明，
纪律 = **引用门必须写全前缀**；**X-4 architecture 正文留着旧分母 9 处**（§1 核心口号"两件码 + 七段"、
§12.1 分档判据"四类供给齐"、**§12.2 标题本身**、§12.8 移植第一步）而速查表与修订记录已是"七位 / 三件码 / 八段"
⇒ **被抄的是正文不是速查表**，9 处全改、4 处历史注记只加指针。
**X-5 由这次修改当场暴露**：把判据写成"七位供给齐不齐"之后才发现 `[supplies]` 今天只有六键
（`rrd/src/profile.rs:45,228`：看门狗三 + 时序三），**七位里只有 2 位可机判** ⇒ 要么批N1 补
`timebase`/`heap` 键，要么明写"5 位靠运行期注册 / 人工自证"，**三态不许被一句话抹平**。
**(B) 运行期语义**——四条设计只写了正常路径，读码核实后各立一条判据（索引 §11 **L-26**、全案 §19.2）：
① **面板写阈值没有通路**（阈值与 `armed` 是 `const`、`SIGHUP`/`reload` 零命中、14 端点无阈值写口）
⇒ 可写量必填 `applies = live｜reboot｜reflash`（**2026-10-19 对表（批N4 件四）：已建**——词表
`RR3_PANEL_APPLY_MODES` 单真值点＋`RR3_PANEL_ROUTES` 第四列申报＋单测/步 ⑩ 双门，详见本文件
§2 同日件四条目）；② **面板唯一写落盘不是原子的**
（`secrets.rs:95-105` truncate 原地覆盖、无 fsync/rename，读侧坏 JSON 回 `Err` 不覆盖 ⇒ 一次掉电从此坏死且不能自愈；
仓内正确范式只有一份 `counters.rs:110-124`）⇒ **写落盘只许一份实现，出现第二条即红**（**2026-10-19 对表（批N4 件四）：已修**——
范式下沉 `rr-ipc/src/persist.rs` 共享单点，secrets 改走共享序，rrd re-export 保调用点）；
③ **闸门复位即开闸**（`Rr3LowGate{blocked:false}` 纯内存；"回阈不自动重放"的真身是"没有重放实现"）
⇒ §6 复位三义**扩为四义**，复位后默认值必须取保守侧；④ **用于判定的读数没有龄期**
（档位是 RAM 快门值；`rr3_now_ns()` 取不到 epoch 折 `0` = 假时间戳）⇒ `age_ms`/`max_age_ms` 入校验⑥-c，
**刻意不新增判定位**（新增位会立刻制造 X-4 那类"21 处要扫"的改名）。
**本轮沉淀两条纪律进本文件**：**（一）改一处口径 = 扫一遍全仓同名字面量**，命中逐处判"历史注记 / 现行文字"，
收口判据是"旧字面量在现行文字中为 0"，**禁止全局替换**；**（二）新增一层必改所有范围表述**
（批N5 立项后"批N1–N4"这类区间写法有 5 处没跟上，已改 N1–N5；引用批次范围优先写全枚举）。
落档：全案 **§19**（X 类 5 条 + Y 类 8 条 + 判据 6 条 + 纪律 2 条）、索引 §11 新增 **L-25 / L-26**、
六册新工单四个 **P-14 / I-20 / N-14 / S-14**、architecture 的 §1/§4.3/§12.1/§12.2/§12.8 分母订正。
待建门数不变（仍 17 把）；**本轮不产任何板上证据**，也不动契约文件。

2026-10-15（同日第六轮，**端到端流与变更爆炸半径**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。
方法：`flow-visualizer` 追两条链 + 自己做一次读者计数（`dependency-impact-analyzer` 的问题：
"同批改 N 处"这句话从来没人数过读者）。取证方式同上：**子代理给的行号我一处一处 `sed` 复核**，
本轮没有一条断言是转述未验的。
**爆炸半径实测**：绑定的物理键有 **22 个文件**在读（含 `rrd/src/profile.rs`、`rr-registry/tests/contract.rs`、
**板侧 4 个文件**、6 份含键的 `assemble.toml`），而 §9 的 N1 改动面清单只列了 5 处 ⇒
新门 **⑨-p 物理键读者集对账**把"清单 = 读者"变成可判红的东西。
顺带查出两个会让门失效的细节：**`i2c_bus` 同名跨命名空间**（描述符键 vs 板侧变量 12 处 ⇒
按标识符数读者必然假阳性）；**`schema_version` 在仓内是四个域、其中两个当前都是 2**
（`RR3_PROFILE_SCHEMA_VERSION` 与 `RR3_ASSEMBLE_SCHEMA_VERSION`），而 5 处文档裸写"`schema_version` 2→3"
⇒ 改错常量当场毁 17 份装配清单；已把表述改成带常量名（**纪律：版本号改动必须指名属于哪个 schema 域**）。
**申报面缺第四个维度**：call 的路由只看被调方 `exposes`（`rrd/src/registry.rs:953-964`），
`module.toml` 里没有"我 call 谁"这一项 ⇒ `rr3-interact` 的 4 次调用（`panel/server.rs:294,332,346,352`）
**在申报面上不存在**，于是"改一条 request 主题谁会断"在仓内不可回答。
`bus.rpc` 是在册 base 格、brain 申报了它、interact 没申报 ⇒ 新门 **⑨-q 调用通道申报一致**。
补 `[rpc] calls` 这一维度**属新增申报面、要契约级评审**，本轮只登记不擅自立。
**最硬的运行期发现**：`rrd/src/supervisor.rs:319` 对子进程 `stdout(Stdio::null())`（`:42-43` 说明这是
测试管道纪律，不是疏漏），而模块生产码 `println!` 实测 **45 处**（act 14 / connect 11 / interact 9 /
power 6 / sense 5）、`eprintln!` 只有 13 处 ⇒ **舵机写入失败、配网成功、拒动点名这些"当场可见"的句子
按设计进 `/dev/null`**，只有 brain（Python，另有 `brain.log`）看得见。新门 **⑨-r 留痕不得进被丢弃的流**。
**两条链的失败传播**（全案 §20.5/§20.6）：P1 面板**根本没有舵机端点**（"经面板点舵机"今天是待建不是现状）；
板侧控制面在物理执行**之前**回成功、回显的是**记账角**、`move_ms` 恒 0 ⇒ 宿主的"回显一致性"门
**结构性地发现不了板侧拒动**；`rr3_low_gate_release` **零调用方** ⇒ 低电闸门一旦置位永不解除，
与上一轮查到的"复位即开闸"合起来是**同一个闸门两端都错**。P2 名义一条链实则两截
（`rr3-audio-rec` 只依赖 `rr-ipc`+`serde_json`、不落总线；`rr3_hal_audio_source()` 无 esp32 分支
⇒ **"板侧双麦 → 宿主 VAD"这一跳结构上不存在**，§2 那句"四件已在板上真跑"说过头，已订正）；
播放点名失败只 print 一行就照常 `chat_rounds+1`（**轮次完成与出声解耦**）；
`rr3_tts_pcm_integrity(0,0)` 判 `Ok` ⇒ 云返回空体时**静默记 `tts_play_ok`**。
待建门 **17→20 把**；索引 §11 新增 **L-27**；六册新工单 **A-19 / I-21 / B-16**；
全案新增 §20（Z 类 7 条 + 流 类 5 条 + 两条逐跳表）。**不产任何板上证据**；
板侧运行面失败是否发生过、gaps/restarts 的真实量级，都归 0.0.7 验证窗。

2026-10-15（同日第七轮，**设计逻辑闭合性审查**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。
本轮对 §2 的改动只有上方"拟增三步"段头的 **同日七轮** 块（注册基座轨工单 `基-1…基-22`、
立"三处计数事实源同批改"纪律），**叙事不在本日志重述**——本日志与
[status.md](../status.md) 的轮次条、[模块设计索引](../design/模块设计索引.md) §14.6 的轮次行
是同一段叙事的三个读者面，第四/五/六轮三面各写一遍已经滞后过（第七轮实测 §16.4 停两轮，T-10）
⇒ **自第七轮起本日志只记"改了 §2 的哪一条规矩"，叙事真值点归
[全案](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §21 与 status.md**（L-23 的执法面）。

2026-10-15 **链④真件档板上批补登**（`e014c89`/`747c586`/`d1d7327`，板上实测回执见
[boards §11](../../base/os/distro/boards/raspberry-pi-64.md)）：①rr-hal MAX7219
**级联锁存纪律**修复——级联下一次 CS 包络每片只锁存移位流最后 16 位，init 序列收
`RR3_MAX7219_INIT_SEQ` 常量表逐条 write、整帧逐行 write（golden 单测随改钉序），
原"一次 write 发完"只对单片成立；②表情白名单 **7→43 态**（七核心态 + char 单字符
档 36 态 charA..charZ/char0..char9）——schema enum / topics.toml note /
`RR3_FACE_EXPRESSIONS` 三处同源同批，帧表 `RR3_FACE_FRAME_TABLE` 同序扩容（36 字模
5×7 列位图：行 0-6 落 bit6..bit2、行 7 恒空），面板下拉 optgroup 同步；**机制零
改动**，点名/挂起/tick 语义不动；③rustfmt 对齐四处（server/secrets/player/composite，
零语义）。rr3-interact 单测 **47→48**（char 档白名单位次 + charH 横杠行字模 +
单帧 tick 恒 0 用例）；十步门禁 GATE_EXIT=0 全绿。

2026-10-15（同日第八轮，**实测取证与分母校正**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。
叙事真值点＝[全案 §22](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) +
[status.md](../status.md) 第八轮条 + 索引 §14.6 第八轮行，本日志按第七轮立的规矩**只记 §2 改了哪五条规矩**：
① 段头 **同日八轮** 块（判定位分母 **24→31**、`基-1…基-25`、**L-31 入口条件纪律**、门 ⑪ 两条机判形状）；
② 段头 "**专属执法位建成 0 把**" 实测注 + **执法位三层判据**（实-14：判据实体 / 反例用例 / 专属执法步，报数必须写清是哪一层）；
③ 门 n 行 六→**七**键（实-9，裁定源 `rrd/src/profile.rs` 的 `RR3_PIN_DERIVED_KEYS: [&str; 7]`）；
④ **实-15 文档纪律**（施工图/方案里凡预写"将来要往台账、契约表或代码某张表加一行"的句子，写的那一刻就回表核一次）
   **及其执法位**——该 Step 自身必须带一次 grep 回执，回执＝"真跑过的那条命令 + 它的实测命中数"，
   **命令选错（范围过宽、或核的是另一张表）与不跑同罪**，没有回执的这类 Step 不许被执行；
⑤ §6 命名空间总表 **11 行→13 行**（+装载期校验号 `①-a…⑩-b`、+批内件号 `n1-*`）。
**本日志的断档如实登记**：第七轮与本轮此前都**没有在本日志留条**（第四/五/六轮各有一条），
两条都是回写对账时撞出来的。**病形＝对账清单按"我这轮改了什么"枚举，于是清单外的面永远不会被改**——
它与 实-14（回写产物把已进 CI 的反例报成零）同族而更隐蔽：实-14 是写错，这条是**根本没列**。
修法落在 全案 §22.8 抬头（清单按"**文件 × 节**"枚举，且收工前跑一次
`git diff --stat <本轮基线> HEAD -- docs/` 与清单逐行对，**清单外出现文件即红**）。
2026-10-16 **增补轮六 批N1 收口**（十小步 n1-1…n1-10，施工图 Task 1–9 + L-14
探测收口；判据真值 = 全案 §5）：契约层新落三份（`roles.toml` 角色词表四角色带
`iface/requires/derives/input_only_ok` / `devices.toml` 器件词表 / `platform/<平台>.toml`
三份 fail-closed 读表器）+ profile schema 3 `[[pin]]` 全链（`[hardware]` 七物理键下移
派生视图、门 n 两稳态实判）+ `[[supplies.domain]]` 第七供给位与校验⑥ + `wiring.toml`
接线档全链与校验⑦ + 装载校验接平台表（⑤-a/⑤-b/⑧-b/③ 三资源空间双占/④-b conflicts
反推互斥，判据实判位 **5→12**，后果一律拒）。rrd lib 单测至 **194**（批N1 全链累计
+约 40：[pins] 解析/查册/六校验/装载门判据用例 + n1-10 L-14 器件级探测 4 条——
探针接口加 `rr3_probe_slot_bound`，I2C 格位在册 `(bus, addr)` 时在位判据升 sysfs
器件节点、总线级降前置条件，"总线在、器件不在"反例落库；rr-registry 63，e2e_profile
3）；十步门禁全绿。批内施工修 1 缺陷：③ 双占 owner 判重原用 pin_id 字符串，
per_instance=false 同角色双 pin 同名吞红，改循环序号判"同一条"。敞口表第 18/19 行
（spi_* 派生键 / L-14 读回比对）随收口登记，批N2–N5 待排。

2026-10-16 **第八轮 × 批N1 对表**（并行设备把 `bbc853d` 四格代码提交推上树之后的基线校正）：文档轮，
**零 `.rs`、零 `contracts/*.toml`**。按上一条立的规矩，本日志**只记 §2 改了什么规矩**：
**"实判位"这类分母宣称必须带口径号**——同一天里 `5→12`（施工图 Task 1/9 预写块的旧口径，本轮已判**作废**，
见全案 §22.4 活体 ⑤）与 `14/31`（全案 §5 口径）**并存于已推树的文档**，而**没有任何一处能机判谁对**。
⇒ 立规矩：引用实判位数一律写成 **`分子/分母（口径＝全案 §5）`** 并附一次逐位 grep 的命中数；
**裸写"5→12""14"这类无口径号的分母宣称视为不合规**（门 ⑪ 的新机判形状，与"裸圈号即歧义"同族）。
对表的六处作废位与四条继续成立位，逐处实测见
[全案 §22.9](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) 与 [status.md](../status.md) 第八轮条末。

2026-10-17（同日第九轮，**模块化尺子：内聚 / 耦合 / 极小性**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。
按本日志第七轮立的规矩，这里**只记 §2 改了什么规矩**（叙事真值点＝[全案 §23](dev/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md)）：
① **集合宣称必须给成员判据，不许只给数**——本轮查出同一个"契约六类"在三处指三个**互不包含**的集合
（architecture §14 含 `README.md` 漏 `face-expressions.toml`；全案 §22.6 把不在 `contracts/` 下的 `wiring.toml` 算作一类；
`contracts/README.md` 自己在册表 9 行）。门 ⑪ 的"三处计数相等"默认已有唯一数法，**而数法本身没有真值点**
⇒ 新机判形状：**凡"N 类 / N 位 / N 表 / N 档"必须同句给成员判据或成员清单**，只给数即红（与"裸圈号即歧义"同族）。
② **一事实一方向律**：任何契约值必须写明"源在谁"，且**源侧改动必须让副本侧红**；
同一对文件出现两个真值方向（codegen 是 contracts→板侧，而 `contracts/platform/esp32.toml:4-6` 自述取值来源是 `board.rs`）
即视为第二真值点成立。**方向冲突比复制更隐蔽**：复制能被 grep 数出来，方向冲突两边各自都"有理由"。
③ **状态词必须带取证基线号**（第八轮立、本轮第二次兑现）：本轮 `rr-bus` 的 Windows 断点复核时发现
**还有第二个断点**（`rr-hal/esp32_uart.rs:34`，因 `rr-ipc` 把 uart 三门钉在 `cfg(any(linux,macos))`，与 unix 无关），
而本仓长期口径是"Windows 编不过 = rr-bus 的 `std::os::unix`"——**一句省略了基线号与范围的状态词，被两端各用了两周**。
④ 挂号三条：发现号新前缀 **`块-*`**（起号前 grep 全仓 0 命中，照 §6 纪律）、教训类 **+L-32 过度抽象没有账**、
基座轨 **+基-27…基-33**（`基-26` 有意空置不给号）、拟步 **+⑨-s/⑨-t/⑨-u** ⇒ **待建门 22 把**。
⑤ **两处自招**（比发现更该记）：(a) architecture `:1662` 是我上一轮亲手改的行，我在同一行写下"契约层 **6/6** 齐"
**却没核这六类的成员集**——上一条刚立的"完成态宣称只准由 grep 说"，第二天就违在它自己的落点上；
(b) 本轮两路子代理各有一条断言我**差点未复跑就收录**（一条说 `DESC_KEYS` 机判是"自指、无实体"，实测
`contract.rs:1817-1838` 确有双向机判；一条说面板 option 42 个，我复跑得 2，两侧口径都对不上）
⇒ 立流程动作而不立态度：**凡转述来的数字，进文档前必须由我自己跑同一条命令**，且**报命令不报结论**。
⑥ **同日第二次自招，且是违在自己刚立的规矩①上**：本档 ⑤ 之后把第九轮写回的"代码位号锚点 **19** 个"复跑了一遍——
19 是把**双向集合差折成一个标量**的结果（清点法写死后正确口径＝**宣布 31、有锚 16、缺锚 15、代码反向多 5**），
成员列举还漏 `⑧-c`/`⑨-d`、多算 `⑨-n` ⇒ 数与成员**双双不对**，正是 块-1"各自分类各自数"在写回侧复发。
⇒ **规矩① 的机判形状补一句：集合宣称除给成员判据外，还不许把两个方向的差折成单个数**；
逐家族对账表落 **全案 §23.6**（含清点法），双向清点归 **基-28**。反向多出的 **`⑧-c`** 被 §8 门 j、§15.2 G-3、§19 F-22
三处当校验位引用而 §5 行 ⑧ 只列 a/b ⇒ **被引用的位号集不等于宣布的位号集**；补位即 31→32，属分母变更，**本轮不自改**。

⑦ **同日第十轮加两条规矩**（基线 `ced8946`，全案 §24；尺子＝"**待裁这件事本身闭合没有**"——前九轮审门、审对象、
审口径、审运行期、审流与半径、审切分，**没审过"决定"这个物件**）：
① **待裁清单每轮重取、不许继承，且每条必须带"落点＋缺省态"**——实测 `待裁` 散在 **18 文件 / 70 处**而无处能答"悬着几个决定"，
施工图 §6 第三问（host 档空 `adc_pins`）**四轮前就已在代码与正文落定**（`platform.rs:259-262` 那条错误串原文自带"裁定②"、
两份平台表已写 `adc_pins = []`、四处 `is_empty()` 断言、`施工图:458` 正文），今天仍挂在待裁清单上占用裁定预算；
70 处里**明写"不裁会怎样"的不足 6 处** ⇒ 新立 **基-34 未决对象唯一登记处**（六列必填），三处现有登记处改为引用不复制。
② **"共 N 条"与成员串必须同批产出**（规矩①的第二形态）：D 表在**同一个文件**里有三个口径——
`:124` D1 已结、`:151`"没裁的…这八条"实列七名且含 D1、`:481`"剩六条未裁" ⇒ 不需要跨文件传播就已经不可判（本轮就地订正 `:151`）。
⇒ 本轮新类 **L-33 决策没有唯一登记处**（L-30 的决策版：那处是撞名，这处是**状态腐烂**）；新前缀 **`裁-*`**（起号前 grep `裁-` 全仓 0 命中）；
基座轨 **+基-34…基-39**（未决登记处 / 依赖面切 `rr-hal-api` / 件 demands 最小集 / 文档锚点符号化 / 装载门同形块计数 / 角色覆盖扩表）；
拟步 **+⑨-w 依赖面禁令** ⇒ **待建门 23 把**。
**两处自招**：(a) 第八轮我写"全仓文档对 批N6/批N7 零命中"，今天实测**文档侧 9＋10 行在用这两个批次号、代码侧只剩 3 行注释**
（`profile.rs:213,:462,:724`）⇒ 这是 实-16"抄侧必须自己再跑"的第三次兑现，旧状态词用前必须重取；
(b) 第九轮 块-20 写"仓根爬路 44 处"，实测 ≥3 级 **54 行**、剔 `cfg(test)`/`tests/` 后真爬路 **14 行**、其中 5 处已收口 ⇒
**未收口只有 4 处**，而"44"恰是 **Cargo 边数**（实测 44 条）＝把两个不同集合的数写串，**又是一例 块-1**。
另拦下取证代理一处误报（它称"六册 §10 的待裁是已裁未回写"，经核 D3/D5/D6/D7/D9 确实未裁，与 `:481` 一致）⇒ **没写进文档**。

2026-10-17（**拍板轮：全案 §24.4 十六项决策全裁 ＋ 批N2–N5 开发计划落档**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。
用户裁定全案 §24.4 的 A 组四项（施工图 §6 四问）＋ B 组六项（D3/D5/D6/D7/D8/D9）＋ C 组六项（结构决定）**共十六项全部按推荐**——
裁定值与施工落点的唯一真值点 =
[批N2至批N5开发计划与第十轮决策拍板.md §2](dev/0.0.5增补轮六——批N2至批N5开发计划与第十轮决策拍板.md)；
全案 §24.4 只加拍板指针、正文按历史保留不改写。同批落档批N2–N5 开发计划（第 0 步 基-34 未决登记处建表 →
第 0.5 步 结构小件批：基-35 依赖面切换＋⑨-w 依赖面禁令、基-32 爬路 4 处、基-37 文档锚点符号化 →
批N2 能源 → 批N3 执行件 → 批N4 面板 → 批N5 生产运维）。**裁定 1（批N6/批N7 正式登记）的四处全枚举与
D3/D5/D6/D7/D9 状态词改"已裁"随第 0 步施工同批做，本行先记账**；敞口表无增减。

2026-10-17（**第 0 步施工：决策拍板落档，兑现上一行预约**）：文档轮，**零 `.rs`、零 `contracts/*.toml`**。
新建 [decisions.md](decisions.md)（基-34 未决对象唯一登记处，六列必填；首批 = 十六项已裁 + 1 条在册未决）；
施工图 §6 / status 敞口表 / 索引 §8 三处"待裁"改**引用不复制**（各自加指针指向 decisions.md §2）；
**裁定 1 落地**：批N6/批N7 正式登记（本 §6 批次行 / 全案 §9 / 索引 §14.6 / docs/README 四处全枚举），
"批N1–N5"区间写法（**现行范围表述类**）改全枚举（历史记录/台账/判据自指按 X-3 先例不改）；
D3/D5/D6/D7/D9 状态词改"已裁"（D3 值更新、D6 值反转、D9 转自动可判）。敞口表无增减。

2026-10-18（**第 0.5 步施工·件一：基-35 依赖面切换＋⑨-w 依赖面禁令建门**）：代码＋门＋文档三面同批。
5 份单元 `Cargo.toml` 直依 `rr-hal-api`（**半切诚实登记**：rr-hal 边保留装配用——装配选择函数
`rr3_hal_*()` 住 `rr-hal/src/backend.rs`、装配位今日在五单元 composite.rs 内，Cargo 级拆边需装配上移
＝超范围，两敞口随收口行入 status 敞口表）；契约符号 use 全切 `rr_hal_api::`（30 处，含 atoms 层 7 处，
`Rr3PcmStream` 留白名单）；`check.sh` 新增 `step "⑨-w 依赖面禁令…"`（三判据全 fail-closed：单元 Cargo
必依 `rr-hal-api`／modules use 面白名单四类冻结 `backend::`/`sim`/`provision_store`/`Rr3PcmStream`／
板侧禁 `rr-hal`/`rr-bus`/`rrd`，枚举打空即门禁失效）。计数对表三事实源同批改（T-10）：可数 step
**11→12 行**、专属执法位 **1→2 把**、待建门 **23→22 把**（本文件 §2 对表段／architecture §14／全案
§16.4）。敞口表本件不动（＋2 随收口行）。

2026-10-18（**第 0.5 步施工·件二：基-32 爬路收口，非 test 4 处清零**）：`header.rs` golden include 与
`rr3-pack` 的 rrd.service include 改 `concat!(env!("CARGO_MANIFEST_DIR"), …)`——契约根构建期注入，
手写跨仓层数消灭（目录动一层不再炸机制核）；`rr-core`/`rr-mech` lib.rs 头注的 ADR-0018 相对链接
（爬路）改纯文本编号，对齐仓内"ADR-0018 裁定 N"无链接通行风格；"路径无关"宣称按 裁-12 改注
（**契约根由构建期注入后才路径无关**，未注入前不成立——注记随两处 include 落码）。
test 面 `header.rs:356`/`:440` 本就是 concat 形，不在本件；其余"44 处"系 Cargo 边数（裁-11 缩围口径）。

2026-10-18（**第 0.5 步施工·件三：基-37 文档锚点符号化**）：门 ⑪ 扩一条"现行文字里的**裸行号**须带基线号"
（判据落点一律写函数名／段名／符号名，行号只作取证时点注记——行号随代码漂移，符号名不漂）。
四处存量漂移同批修：门 n 判定位（`rrd/src/profile.rs` 门 n 段真位 `:1089-1100`，测试用例真位 `:2347`/`:2360`；
文档散写 `:1042-1056`/`:1986`/`:1999` 系 `b11dd2f` 时点）、`RR3_PIN_DERIVED_KEYS`（真位 `:92`，散写
`:91-99`/`:88-99`）、`board.rs` 引脚三常量（真位 `:148/:150/:174`，散写 `:156/:158/:182`）、ADR-0021
"待裁 D12"（真位 `:235`，T-9 与修订记录引 `:220/:280`——按 T-9 只补注记不改写）。**零 `.rs`**；
修订记录等历史日志按 X-3 不改，本条即其现行文位的替代锚点。

2026-10-18（**第 0.5 步收口件**）：status 敞口表 **+2 行**——第 22 行 装配位上移／第 23 行 单元总线承载面契约化
（基-35 件一窄化两敞口，皆**缺裁定类**，首条先例＝第 18 行；行内明写分档并给裁定归属＝decisions.md §3 在册未决，
§3 计数 1→3、"裁定类预算为零"句同批订正）；decisions.md 基-35 行回填施工提交号 `be69c09`；
第 0.5 步计划档落 DoD 3 对账（三件提交 `be69c09`/`e9a764f`/`8f8bbce` 各自门禁全绿回执，⑨-w 待建门 23→22 已计）。
**件三余量同批补**：status 敞口表第 21 行的 board.rs 引脚散写行号系同一漂移事实的漏网点，补取证时点注记
（真位 `:148/150/174`，原记 `:156/158/182` 系 `b11dd2f` 时点）——兑现"存量漂移随触碰即补"的自查。

2026-10-18（**n1-11，批N1 收尾小件**）：`check.sh` 建 **`step "⑨-n 注入路径唯一"`**（门 n 第 ③ 层专属执法步，
全案 §8 门 n 行 / 施工图 Task 8 Step 2）——键名单自代码常量 `RR3_PIN_DERIVED_KEYS` awk 导出（单真值点，批N3 扫面 7→8 自动跟随）
＋扫面前件 fail-closed 四查（声明数非空 / 实解析数非零 / 两数判等 / profiles 份数非零，任一不成立即"门禁失效，不放行"）
＋逐 `profiles/*.toml` 扫 `[hardware]` 段（行注释先剥，命中逐行点名 file:line）；三事实源同批对表：可数 step **12→13 行、
专属执法位 2→3 把、待建门 22→21 把**（§2 对表段 / architecture §14 / 全案 §16.4）。**零 `.rs` 零 `contracts/*.toml`**；
`spi_device` 收口仍随批N3（status.md 敞口第 18 行）。

2026-10-18（**批N2 首件：基-36 demands 最小集**，裁定 13"只交一件"）：申报面＋判据实体＋反例三层同批。
①申报面定形 `[assemble].demands = [{ rail, peak_ma, critical }]`（rr-registry `assemble.rs`：条目级解析
`rr3_demands_from_array` 件级专用——组合件级 [assemble] 不认 demands，电流需求是原子件物理属性，未知键点名即拦；
rail 非空 / peak_ma 正整数 / critical 显式布尔不给缺省；**缺键 = 空申报合法**，执法面"kind=power 未申报 → 红"归门 c
不在形状层预支；增键不 bump，demands 缺省语义 = 未申报，已激活 17 份数据语义不变——composite.rs 头注同构先例）；
②判据实体 `rr3_demand_budget_check` 落 `rrd/src/profile.rs`（`Σ demands.peak_ma (critical=true) ≤
domains[rail].current_cap_ma`，fail-closed 逐条点名：rail 不在 domains ⇒ 域缺即 cap 缺红；per rail 超额 ⇒ 红点名
申报者；critical=false 不进 Σ；并发缺省 = 全部同时，`concurrency_group` 收窄随门 c 不预支）；③反例用例
`demand_budget_check_nine` 进 rrd ①b（正例 Σ700≤2000 绿 / 双件超 cap 红 / ghost rail 红 / critical=false 不进 Σ）；
首条真实声明落 `modules/rr3-act/atoms/servo/assemble.toml`（`rail5v peak_ma=700 critical=true`——act.axis.pwm 是
全仓唯一 kind=power 角色，未来门 c 执法面恰闭合于一件；三份 profile rail5v cap=2000 ⇒ 绿）。
**三层诚实登记（实-14）**：①判据实体落、②反例用例落、**③专属执法步 = ⑨-c / 门 c 仍待建**——判据不接装载拒链，
可数 step 13 行、待建门 21 把**计数均不变**；裁-9 推翻条件（"写了 demands 且 ⑨ 在跑"）**未满足**（⑨ 尚不在跑）。
`contracts/*.toml` 零改动、`check.sh` 零改动。

2026-10-18（**批N2 件一：⑨-c 门 c 接链**，校验⑨ 装载拒链）：判据接 boot 链致命汇合点。
①申报面五键定形（ADR-0021 裁定 2）＝`{ rail, peak_ma, sustained_ma, critical, concurrency_group }`
（`rr3_demands_from_array` 重写：sustained_ma 正整数且 ≤ peak_ma——连续分量超过峰值是物理矛盾 fail-closed；
concurrency_group **缺键即红**（T-5 同 no_injection 范式——缺省与漏填不可区分），显式 `[]` = 不分组 =
全体并发最坏口径合法；servo 申报升五键 `rail5v 700/120/true/[]`）；②判据实体接链（`rrd/src/profile.rs`）：
`rr3_demand_declared_expand` per-instance 扩算（**每实例语义**：条目 = 该件服务的每个角色实例的需求，
实例数 = 角色 iface ∈ 件 handles.trait 且 slot 绑本模块的 pin 数，归因打空地板 1 宁超算不漏算，
同模块多件声明同一 trait 各按全额计）＋`rr3_demand_budget_check` 并发口径（恰一组归属 = 组内互斥取 max，
空组/多组归属按不收窄计——F-12 收窄必须是数据，判据只认数据不验物理）＋`rr3_demand_kind_power_gaps`
（§5 ⑨ 缺失语义：角色 kind=power 的 pin 挂轨 X 而绑定模块申报面无 X 需求 ⇒ 红；rail 空/none 由⑥-b
点名不双报）；③执法位 `check.sh` 建 `step "⑨-c 电流预算装载拒链"`。反例组扩：`demand_budget_check_nine`
并发三态（同组互斥 3×700→max 绿 / 多组归属不收窄 2100 红）＋装载拒链三用例 `demand_budget_chain_*`
（三件同域叠加 2100 红 / kind=power 未申报红 / 同布局组收窄 1400 绿）；真实 drill 兼容
（default/distro/esp32-mini 三份 profile 各单轴 700 ≤ cap 2000）。**三事实源同批对表：可数 step 13→14 行、
专属执法位 3→4 把、待建门 21→20 把**（§2 对表段 / architecture §14 / 全案 §16.4）；
decisions.md 基-36 行回填首件提交号 `bcc13fd`，裁-9 推翻条件（"写了 demands 且 ⑨ 在跑"）**满足**——
申报面定形 `[assemble].demands`（形状以 ADR-0021 裁定 2 / 基-36 为准，非裁-9 行原文的 `[[demand]]`）。
`contracts/*.toml` 零改动。

2026-10-18（**批N2 件二：能源能力面 Rr3Rail ＋ 两格一主题**，ADR-0021 裁定 2）：判据实体落 hal
契约面，契约三册同批扩格。①`rr-hal-api` 立 `Rr3Rail` trait 三方法（`rr3_read(domain) ->
Rr3RailState` / `rr3_current_ma(domain)` / `rr3_switch(domain, on)`——F-32 撤名收口：
`Rr3PowerDomain` 是无定义名不进文档，一次定形归 `Rr3Rail`；`Rr3Power` 保留不动＝增长律
"新立不改既有"）＋ `Rr3RailState{mv, tier, age_ms}`（**T-6/P-15**：`age_ms` 与 `Rr3RailTier::Unknown`
同批落——判据一条写完：`age_ms > max_age_ms` 或时基取不到 ⇒ `Unknown`（max_age_ms 住 profile
域声明 ⑥-c1）；消费纪律＝执行面按 Severe 处置、展示面原样显示不许折算 Ok，执法归⑥-c2 件三，
契约层只供数）＋ `rr3_rail_tier_name` 四态名（ok/mild/severe/unknown，与板侧 `RailTier::name`
同串）；占位后端 `Rr3UnavailableSource` 三方法显式回 `Unsupported` 不静默退化；rr-hal re-export
三名（`Rr3Rail`/`Rr3RailState`/`Rr3RailTier`）路径原样保住。②契约三册 append：`slots.toml`
schema_version 2→**3** 基础层两格 `power.rail`（域电平观测；bare 档经守护输出行不发主题）/
`power.budget`（能源预算仲裁），tier_scope 三档全在，`power.battery` 保留不动（单域电池与新格
多域不是同一对象）；`topics.toml` 30→**31** 条 append `rr3-power/rail/state` event L2（域压
消费走 Rr3Rail trait 本地决策不经总线，同 battery 语义，主题供次级观测面）；`module.schema.toml`
base_capabilities 5→**7**（与 slots 基础层钉死等价，`slots_base_layer_is_pinned_to_base_capabilities`
自动跟随）。③五处形状 pin 同批：rr-registry `src/slots.rs`（23 格 / base 7＋新格 tier_scope 断言）、
`tests/contract.rs`（三元组 31 条、kind 12-11-4-4、格位 23 / 层分布 7-12-4）、`src/topics.rs`
（merge 34 = 31 + 3）、rrd `src/topics_load.rs`（31 / 34 三处）；atom 承担账两格进
`BUILTIN_NO_ATOM` 豁免（承载是 rr3-power 进程内机制切片 RailMonitor/RailJudge——件五归属搬家——
与板侧守护，无独立原子目录，立无源码空原子件即 D15 空壳）。**本件不建执法步**（check.sh 零改动，
可数 step 14 行、待建门 20 把计数均不变）；②-a0 元判据尚未建，新格 normal 无在册角色属诚实红账，
角色覆盖扩表 4→18 归件六基-39；contracts/README 演进登记与裁定段计数、status.md 台账同批。

2026-10-18（**批N2 件三：校验⑥-c2 运行期判据实体 ＋ D3 无器件合法态**，decisions.md 裁定 5）：
①判据实体落 rr-hal-api 契约面（单真值点——产出侧与消费侧共用，不许各自手抄 `>` 判据）：
`rr3_rail_tier_effective(tier, age_ms: Option<u32>, max_age_ms: u32) -> Rr3RailTier`——判据一条写完：
`age_ms > max_age_ms` **或** 时基取不到基准（`Option::None`，零新类型表达"取不到 epoch 就不编造
龄期"，Y-4 同线）⇒ 折 `Unknown`；边界 `age == max_age_ms` 不折（新鲜度上界闭区间内）；红例五断言
（新鲜原样 / 边界不折 / 超龄 Ok ⇒ Unknown / 超龄 Severe ⇒ Unknown——陈旧 Severe 不许冒充新鲜，
T-6 立项理由 / `None` ⇒ Unknown），rr-hal re-export 同批补函数名。②D3 落地 rrd `profile.rs`：
`Rr3Profile` 加 `slots_device_absent: BTreeMap<String, String>` 字段（[slots].present 附近，
doc 引 D3/L-28），[slots] 段解析 `device_absent` 表（格名 → 非空理由；留空即红——no_injection
范式，缺席不写理由＝未申报的静默退化）＋未知键白名单 `present`→`present / device_absent` 放行；
查册门三查 fail-closed：格名在册（查册门）、必须是 base 层格（**非 base 格的缺席表达＝不进
present，写进本表属第二套语法即红，F-2 同型**）、理由非空（解析段已判）。`present` 语义不动
（本板物理在位格位声明，base 格不许进 present 的既有门照跑）——device_absent 是 base 格专属的
器件缺席登记，与 present 正交；schema_version = 3 不动（[slots] 段新可选键 append，非破坏性）。
③测试：rrd `device_absent_declares_base_slot_absence_legally`（正例：power.battery 带理由登记
boot 过、不进缺格判定）＋ `device_absent_gate_rejects_unknown_nonbase_and_empty_reason`（三红）
＋ `good_profile_parses` 补空 map 断言；rr-hal-api 红例用例进 lib 测试（判据实体＋红例全在
cargo test，**本件不建 check.sh 执法步**——可数 step 14 行、待建门 20 把计数均不变）。
④decisions.md D3 行施工注记同批（提交 `f47d6c7`，批N2 收口对账 2026-10-18）；运行期对照（battery 回
Unsupported 时"无器件 vs 坏了"的分辨）归件五/批N4——本件只落登记面与判据实体，不消费。

2026-10-18（**批N2 件四：校验⑩ 内存预算——[supplies.budget] 分母 ＋ [assemble] 分子 ＋ schema 4**，
decisions.md §4 变更记录同批）：
①分母落 `[supplies.budget]` 四键 `{ not_applicable, task_stack_budget_bytes, heap_total_bytes,
heap_reserved_bytes }`（ADR-0021 修订 3"栈/堆预算不住 platform 表——三份数据三个主人"）＋**分档自洽
parse 层裁**：host ⇒ `not_applicable` 必须真且三数键不得并存（自相矛盾红）；rtos/bare ⇒ 必假且三数键
必填（`task_stack_budget_bytes > 0` / `heap_total_bytes > 0` / `heap_reserved_bytes ≥ 0` /
`total ≤ reserved` 红文案点名"可用堆 = total − reserved ≤ 0"）；tier 非法（空）跳过分档判不级联；
段缺红文案点名"host 档显式写 not_applicable = true，不许留空"（F-6）；`schema_version` 3→4
（新必填段 = 破坏性变更，批N1 `[[supplies.domain]]` 先例；`[supplies]` 段缺时 budget 点名不级联）。
②分子落 `[assemble]` 新增 `task_stack_bytes`/`heap_peak_bytes` 两键（rr-registry `assemble.rs`）：
**件级语义非每实例**（双轴共享一个任务栈直加不扩算，与 ⑨ demands 的每实例扩算相反）；负数红
（"负数是物理矛盾"）；缺省 0 = 未申报不进 Σ——申报义务未触发（板侧任务映射未建模归 0.0.7
批G/批D1），红例用合成申报＋诚实注记防 L-24 假绿；组合件级 [assemble] 点名两键即红（件级专用）。
③判据实体 `rr3_memory_budget_check(stack_sum, heap_sum, budget, tier)`（rrd `profile.rs`）：
`not_applicable` ⇒ 绿；⑩-a 红文案含"分子为件级直加非每实例扩算"；⑩-b 红文案点名
"heap_total_bytes − 常驻 heap_reserved_bytes"并引 F-13（渲染缓冲/队列/板侧配置全是堆账，
只加总栈会击穿半边）；装载拒链接入（`rr3_demand_kind_power_gaps` 之后致命汇合点前）——
坏形件不进加总（可见面 = `assemble_broken` 点名，不 claim 全覆盖，同 ⑨ 诚实注记）。
④数据三份：default/distro（host）显式 `not_applicable = true`；esp32-mini（rtos）落**声明预算位**
65536/327680/16384（= 2× 现役 16KB 任务栈＋余量，board.rs THREAD_STACK/MIC_THREAD_STACK 同源；
堆上限按 S3 数据手册内部 SRAM 推算待 0.0.7 真板对账；首位落账 `UART2_TX_RING`——F-13 常驻
缓冲是堆账，audio 队列/渲染缓冲/批N4 配置服务随 N-13 补账）；四份 e2e 夹具 + tools/sim 夹具 +
wiring.rs 路径推导夹具同批 schema 4。
⑤测试：rrd 六条新用例（判据实体绿/红含等号边界、分档自洽七红、两超限拒 boot、只超堆不超栈
断言 `!contains("校验⑩-a")`、边界和过 boot、双件直加非每实例扩算）＋ rr-registry 两新
（parse 正/负/非整数/未知键、组合件级点名红）＋ contract.rs 三处形状 pin 同批 3→4（host 两处
`not_applicable = true`＋数字键不得并存、rtos 三数键逐值 pin）；⑩ 经步①b `cargo test --workspace`
进 CI，**本件不建 check.sh 新步**（可数 step 14 行、待建门 20 把计数均不变）。
⑥decisions.md §4 变更记录同批（提交 `954ea75`，批N2 收口对账 2026-10-18）；decisions.md §2/§3 无增减。

2026-10-18（**批N2 件五：RailMonitor/RailJudge 接能力面——机制不动、归属搬家**，
decisions.md §4 变更记录同批）：
①选择入口 `rr3_hal_rail()`（rr-hal backend.rs，与 `rr3_hal_power` 同族）：今日全后端
回占位（`Rr3UnavailableSource`，使用时 Unsupported 显式回错 fail-closed）——板侧
RailMonitor/RailJudge 机制本体在 tools/esp32-periph-base 真跑（rtos 档自持，板侧文件
不改），宿主真件（INA219 电压/电流面）归 0.0.7 真件批（status.md 敞口表 12 行），届时
照 `rr3_hal_power` 模板接 linux 分支与生效绑定。
②rr3-power 接面：config 新 `[rr3-power.rail]` 段——段必填＋全部键必填（up=false 时
其余键惰性但显式写，F-6 精神：统一解析路径无分支）；`severe_mv < mild_mv` 严格校验
（相等也红：Mild 档不可达）；`max_age_ms = 0` 合法（龄期恒超界 ⇒ 恒 Unknown，诚实
语义）。主循环第 4 拍（`rail.up = true` 才跑）：读域压 → ⑥-c2 按龄期折有效档位 →
判定核 → 发 `rail/state`（四键 `{seq, mv, age_ms, tier}`，tier 串走
`rr3_rail_tier_name` 同源词表，Unknown 拍 "unknown" 原样）＋越阈首拍边沿守护行；
`up = true` 开机探活占位回错即 exit(1)（fail-closed 同电源面口径）；周期读数失败
counter（`rail_read_fail`）先行再 exit(1)（同 battery_read_fail 纪律）。
③机制切片归属搬家（逐字对照板侧 power.rs，机制不动）：`rr3_rail_tier_of`（纯 mV
比较：mv≤severe⇒Severe、mv≤mild⇒Mild）＋ `Rr3RailJudge`（一核两条线，mild/severe
各由 Rr3LowEdge 一实例承载，event = 两线任一低电段首拍，逐拍等价于向下档变——
"严格回阈才再武装"语义钉在 Rr3LowEdge 内，4500 回阈后 4300 是 severe 新段首发再报）
＋ `rr3_rail_beat`（⑥-c2 消费执法点：有效档 Unknown 不喂核原样透传——mv 再好不许
从 mv 重分类出 Ok；fresh 拍喂核取判定核分类值）。
④module.toml publishes 加 `rr3-power/rail/state`（topics.toml 件二已在册）；仓
config.toml 与 rrd e2e_power CONFIG 夹具同形状 up=false 段（取值与 host 档
profiles/default.toml rail5v 域一致：mild=4600/severe=4400/max_age=0）。
⑤测试（composite.rs 追加，power 非 atom 名不占 ⑧-a 分母）：config 逐键覆盖/缺段/
缺 up/倒挂（含相等红）/0 周期钳制同拍、`rail_state_body` 四键无加戏、判定核序列
对照板语义、两腿对拍（腿一确定性＋腿二"事件 ⇔ 向下档变"）、⑥-c2 执法两条各一
（`rail_beat_unknown_not_refed_and_passthrough`：Unknown 不折 Ok 不喂核＋陈旧折
Unknown 不喂核＋fresh 拍喂核）；rr-hal/rrd/rr3-power cargo test 全绿，**本件不建
check.sh 新步**（可数 step 14 行、待建门 20 把计数均不变）。
⑥decisions.md §4 变更记录同批（提交 `53e243a`，批N2 收口对账 2026-10-18）；rr3-power 模块设计
P-9 行 X-4 加注（归属搬家落点与执法用例落点）；板侧 power.rs 零改动。

2026-10-18（**批N2 件六：⑨-a 门 a 报告模式 ＋ 基-39 N2 那一刀角色扩表**，decisions.md §2
施工图§6-④ 裁定）：门 a 首建采报告模式＋三件配套，缺一件即等同永久红灯。①契约扩表：
`contracts/roles.toml` append 两角色 `power.rail.sense`/`power.current.sense`（slot=power.rail，
kind=signal，iface=Rr3Power 设备类面——域级 Rr3Rail 由 rr3-power 组合件经 RailMonitor/RailJudge
承载（件五），不经 [pins] 句柄，本件不做 trait 扩面；requires=controller+addr，
derives=i2c_bus+ina219_addr，direction=in，droppable=false 测量面须活着才知欠压，
input_only_ok=true，per_instance=true，instance=域名 → pin_id=power.rail.sense.rail5v，
domains[] 绑定位一律写 pin_id 校验⑥-d）；covered normal 格 4→5（power.rail 进账）。
②执法位 `check.sh` 建 `step "⑨-a 基-39 角色覆盖六表对账·报告模式"`：报告三段（A 未覆盖
normal 格／B roles 四外键悬空——应恒 0，悬空是错不是账、硬红不进基线／C TRAITS·PIN_KEYS
差集），红账＝A+B+C 对低水位基线文件 `tools/ci/gate-a-baseline.txt`（初值 23＝15+4+4：
20 normal−5 covered＝15、TRAITS 差 4＝Rr3AudioSource/Rr3AudioSink/Rr3Link/Rr3Display、
PIN_KEYS 差 4＝role/instance/direction/mux_group）——count>基线即红（"红条数单调不增"，
裁定配套 c）、文件缺/坏/非整数即红 fail-closed、count 下降打印"须同批下调"提醒；报告走
stdout 可见面（裁定配套 a，不走被 `Stdio::null()` 吞掉的面）。键集四腿全部自代码常量导出
（assemble.rs 两在册表＋profile.rs `RR3_PIN_KEYS` 声明数/实解析数判等＋slots/roles 两契约表，
单真值点不抄第二份，⑨-n 先例）；六表之第 5 表（件 handles[].desc_key）已由 ② parse 层执法、
第 6 表（平台 role→具名构造）归门 a 转红收口件，本步如实不判。③归批N3 的两笔如实注记在
roles.toml：power.switch 不落本表（derives 需新派生键——gpio_relay_line/gpio_led_line 是格位
专名，语义不可混用，扩键属契约级 C4 裁定＋descriptor.rs＋backend＋门 n 联动）；power.charger
随批N3 器件角色进场（计划 §3）；bus.*/net.online/power.budget＝logic 内建格无引脚绑定
（roles.rs 判 logic 角色不进本表），其覆盖口径归转红收口件另裁（豁免或 logic 词形）。
④rr-registry roles.rs 契约测试同批 4→6（len＋names＋新角色 per_instance/requires/droppable/
names_for_slot 断言）。**三事实源同批对表：可数 step 14→15 行、专属执法位 4 把不变＋报告位
1 把、待建门 20→19 把**（§2 对表段 / architecture §14 / 全案 §16.4）；decisions.md §2
施工图§6-④ 行施工注记＋§4 变更记录同批（提交 `e918b76`，批N2 收口对账 2026-10-18）；contracts/README
演进登记同批；基线文件初值随本件落 `tools/ci/gate-a-baseline.txt`。

2026-10-19（**批N3 件六～件九收口：SPI 词形拆格＋D5 up 语义落码＋计量格补课**）：代码＋门＋
文档三面同批。⑥件六 `6953222`：`profile.rs` SPI 派生臂（`spidev<N>` 词形＋channel 片选号 →
`/dev/spidev<N>.<M>`，与旧 `[hardware]` 直给值逐字节对齐）＋门 n 扫面 `RR3_PIN_DERIVED_KEYS`
7→8（直给与派生并存即红，装载红例落库）＋roles 新角色 `ui.face.spi`（iface=Rr3Display、
requires=[controller,channel]、derives=[spi_device]）＋profiles 两份 `[hardware].spi_device`
撤除迁移 `[[pin]]`＋gate-a 基线 23→21 同批下调——**敞口 18 行结项**（decisions §2 行 2 施工
注记同批）。⑦件七 `4e77e1f`：`contracts/events/connect-net-link.schema.json` 补齐（net/link
载荷 schema 此前缺位；up 只表链路层已关联不表可出网——D5，出网性由 `net.online` 通道另证、
`Rr3LinkState` 不加字段；AP 态恒 `up=false`；`up=false ⇒ ssid/rssi_dbm` 必 null 的 §9 红线
机器化进 allOf）＋`cloud/session` 未就绪文案按配网态分流（AP 态点名 `mode=ap`、不折叠成
`up=false`；station 原词 append 保留，判据词"链路"两分支都在）＋`rr3_link_body` doc 注记
（载荷字节不变）＋topics.toml net/link 行 note＋connect 册五处对表（N-4 随结；decisions D5
行施工注记同批）。⑧件八 `3025205`＋`5018e20`：rr-hal-api 五型计量契约面（Rr3SampleKind/
Rr3SampleQuality/Rr3Sample/Rr3Sensor/Rr3DigitalIn——Rr3Sensor 随三计量角色进 roles.iface 引用、
Rr3DigitalIn 在册而 iface 未引用（数字输入面先行，差集第三段 3→4 如实进红账））＋
assemble traits 7→9＋roles 三计量角色 charger/env/imu（derives=[] 显式空语义——roles.rs 撤
"空 derives=红"判据、词形漂移判据跳过空 derives）＋profile `device` 键（RR3_PIN_KEYS 10→11）
＋装载校验 ③-c 共址精化（同 (bus,addr) 共址须同 device 非空且型号在册，四反例矩阵）＋
devices.toml SHTC3/ICM-42688 进场（现役六器件）＋profiles 两份 charger/env/imu 三 pin 补
device＋architecture 四处对表（不 bump 版本）。附加评估 `e07bbe7`：B-14② brain 档位角
70/90/110 档案派生改判批N4（语义位进 payload 属契约级变更／通道与 A-13③ 同链未立／语义位→
档案键映射待裁定；三理由落 brain 册 B-14 行，intent.py 三常量维持）。⑨件九收口对账（本条目）：
decisions §2 行 2＋D5 行施工注记、status 敞口 18 行结项＋19 行对表注记（id_reg 判据源 4→6，
不结项）＋缺裁定类清零注记＋status 批N3 件六～件九行回填（本提交）。**四项推荐代行待追认**
（AskUserQuestion 四问用户跳过、按全案推荐项代行施工，追认前不改裁决力）：charger 形状＝
meter 采样面／env-imu 深度＝器件在册＋契约面先行、真件探测归器件批／家族E 立格＝三计量角色
进场／D5＝up 只表链路关联。**三事实源同批对表**：红账 21＝基线（A=13＋B=0＋C=8＝trait 差
4＋pin 键差 4），roles 现役 10 角色、devices 现役 6 器件、RR3_ASSEMBLE_HANDLE_TRAITS=9、
RR3_PIN_KEYS=11。

2026-10-19（**批N4 件一～件三：证据号命名空间＋板侧配置腿选型＋门 d 覆盖率门建成**）：三件分笔
落库，日期台账同批补登。①件一 `947af3d`（brain B-15）：证据号规则落 §6.1 新段（形制 `<形态>/<编号>`、
frontmatter 六键；校验器唯一实现住 `rr-ipc::evidence`——形制＋存在＋frontmatter＋form 与目录一致
四道 fail-closed，单测钉死）＋容器 `docs/evidence/`（README 模板）建成＋敞口表 15 行结构面注记；
证据产出归 0.0.6/0.0.7 验证窗，写面挂接随批N4 件九端点。②件二 `1c30b9b`（connect N-13）：板侧
配置腿载体选型 esp_http_server（TLS 不做四理由落 connect 册 N-13 行注记、凭据面不接 N-11）＋
堆核算全账进装载校验⑩-b（`profiles/esp32-mini.toml` heap_reserved 16384→98816 逐项留痕：
UART2_TX_RING＋audio_q 峰值＋渲染＋配置服务预留）＋architecture 对表；板上 HTTP 实证与真板堆
对账归真板批。③件三（本提交，interact I-19）：`panel/server.rs` `RR3_PANEL_ROUTES` 升三元组带
`covers` 数据化申报（14 端点全 `&[]` 如实——读配置者 0）＋词表常量 `RR3_PANEL_COVER_SETS` 五名
码内单真值点＋形制单测；`check.sh` 建 `step "⑩ 门 d 面板覆盖率对账·报告模式…"`（判据三段与
fail-closed 细目见本节对表段本日条目）；基线文件 `tools/ci/gate-d-baseline.txt` 初值 70/70
（建即红如实在账，红项随件九扩端点转绿）；interact 册 I-19 行对表注记＋status 批N4 行回填。
**三事实源同批对表：可数 step 16→17 行、专属执法位 5 把不变＋报告位 1→2 把、待建门 18→17 把**
（本节对表段 / architecture §14——计数行补登批N3 件五 ⑨-e 漏更后随本件续记 / 全案 §16.4）。
**随批补修件一/件二两处漏更**（独立提交，全量门禁回执抓出——两件当时未跑全量门即落库，属违尺，
本批如实登记）：①件一漏把 `docs/evidence/README.md` 写进 docs/README.md 库地图（⑤ 文档门
"导航漏链"红）；②件二漏把 contract 形状 pin `heap_reserved_bytes` 16384→98816 同批
（`rr-registry/tests/contract.rs` `esp32_mini_profile_binds_bridged_minimal_shape` 红）。

## 3 结构纪律

- **主语言 Rust（2026-10-04 立规）**：仓内一切新实现一律 Rust；存量非 Rust 面按
  **功能语义不变**原则渐进迁移为 Rust，不设豁免通道——当前非 Rust 面为
  `rr3-brain`（Python，ADR-0014）与 CI 胶水脚本（bash，可最后迁）。brain 的迁移
  路线已由 [ADR-0016](adr/ADR-0016-brain迁移路线-python转Rust渐进首裁定.md)
  落设计裁定（首件 bus.py → rr-bus 复用、实施归 0.0.6 专项）；迁移本身走正常批次
  （生成器产出、契约校验、十步门禁一样不少），`module.toml` 的 `lang` 字段随迁移改为
  `rust`，原"Python 语法检查属人工检查点"的面随之收敛为 cargo 检查
- 新模块一律由生成器产出：`cargo run --manifest-path tools/mkmodule/Cargo.toml --bin rr3-mkmod -- rr3-<名>`
  （模块名形如 `rr3-arm`，第二参数是输出根，默认 `./modules`）。生成器落盘**五个文件**：
  `module.toml`、`config.toml`、`Cargo.toml`、`src/main.rs`、`README.md`——[naming.md](naming.md) §4
  所称"四件套"指四类结构角色（manifest / 运行参数 / 源码 / 说明），`Cargo.toml` 属于源码那件的
  crate 半边。五件齐备且 manifest 过契约校验才可合入 `modules/`；校验不过则整体不落盘，
  不留半成品模块
- 新增/变更主题必须先改 `contracts/topics.toml`（改契约 = 走 review），再在 manifest 里引用
- `contracts/` 只增不改；破坏性变更升 `contracts/envelope.schema.json` 的顶层元数据键
  `x-schema-version`（Rust 侧同名常量 `RR3_CONTRACT_SCHEMA_VERSION`，由 `cargo test -p rr-ipc
  --test envelope_meta` 逐值钉住）并在 `contracts/README.md` 的"演进登记"留痕；
  `contracts/topics.toml` 与 `module.schema.toml` 用的是另一个键名 `schema_version`，别混写
- **版本按交付物类型分窗（2026-10-12 立规，[ADR-0020](adr/ADR-0020-版本边界-开发收口与实机验证与验证后整理.md)
  裁定 1/6 = 0.0.5 §2.7 R32/R37）**：一个版本只产**一类**交付物——**开发窗**（0.0.5）产代码 + 用例 +
  契约 + 门，**验证窗**（0.0.6 全承载档 / 0.0.7 轻承载档）产点验矩阵行与板上实录，**整理窗**（0.0.8）
  产重构与返工。三条执法口径：① **验证窗禁新增功能码**——本版内改码会让本版先前取的证据带失效日期
  （与 §2 的"换代即重跑"同因）；验证暴露的功能缺陷登记进 0.0.8，唯一例外是取证链路自身（门脚本 /
  矩阵 runner / runbook 命令断点 / 夹具）；② **两种败法分开登记**——"无载体 + 触发条件"（器件未到 /
  板离线）与"**缺码 + 工单号**"（对应开发项未交付）不许互相顶，把缺码写成无载体等于把开发欠账藏进
  采购欠账；③ **"代码可达"与"板上跑过"是两个证据对象**，任一格不许用另一格的证据充当（同
  architecture §12.6）。批次归属的唯一落点表 = [模块设计索引](../design/模块设计索引.md) §14，
  一个余数只挂一个版本
- 模块 crate 之间**不得**互相依赖，只能经 `rr-ipc`/`rr-bus`（一票否决项）
- 密钥、token 不进仓库：构建期环境变量注入（通用键名 `RR3_CLOUD_API_KEY`，
  0.0.5 批M MC-8②：原 `SILICONFLOW_API_KEY` 把云供应商焊进代码，供应商由
  brain `config.toml` `[rr3-brain.cloud]` 承载；源起 `legacy/core/README.md`）
- 反射环（1k–10kHz 闭环）留在模块进程内，永不进总线（ADR-0007 裁定 1）
- `contracts/slots.toml` 能力格总表（ADR-0012）：与 topics.toml
  同级同纪律——只增不改，破坏性变更升 `schema_version`；manifest 的
  `provides`/`depends` 只许引用表内格位，查册门接入 CI 全集层（与仲裁共用真值）
- 装配单（ADR-0012）：`profiles/<机型>.toml` 是纯数据（`[os]` / `[bindings]` /
  `[enabled_slots]`，2026-10-08 批A② 由 `[faces]` 消歧改名，避免与职责面 Facade 撞名），
  不得包含逻辑；新增机型 = 新增文件，不改代码；F⊗F 非法（brain 不许运行期
  决定能力归属，rrd 不许运行期重排装配）

## 4 Definition of Done

- [ ] `bash tools/ci/check.sh` 本地 exit=0
- [ ] 涉及 `contracts/` 的变更附带演进说明（只增不改 / major 升级）
- [ ] 涉及架构语义的变更已更新或新增 ADR
- [ ] 新模块由 rr3-mkmod 生成并通过 schema 校验
- [ ] 无新增 warning（`cargo check` 干净，不留 `#[allow]` 兜底）

## 5 文档纪律

- **legacy/ 处置三阶段（2026-10-05 用户裁定落规约，替代旧"冻结区不动它"口径）**：
  ①**只读消化期**——从 legacy 提语义进新架构（迁移映射见 architecture §8、逐组件盘点与
  排期唯一落点 [legacy-digest.md](legacy-digest.md)），**不回写
  legacy/ 内任何文件**（含其 README 已知断链——退役批才注销）；②**融合完成期**——盘点
  三分类中"待融合"清零，全部收敛为已融合/退役两类；③**退役期**——整树删除 + ⑤ 门
  legacy 排除条款撤除 + architecture §8 标注消化完成 + status 已知偏差注销，删除动作由
  退役批单独一提交执行。退役前的中间批次一律维持只读，不因"顺手"在 legacy 内改文件。
  **退役期收口注记（0.0.5 §2.4 R21，2026-10-06）**：三阶段就此闭环——git 历史
  整树删除在案（0.0.4 批F）+ 工作树残留 `legacy/firmware-c/sdkconfig` 清盘 +
  `!legacy/` 白名单与 ESP-IDF 段忽略规则摘除；本条目转为历史口径存档，原文不改。
- 状态与进度只写 `docs/status.md`；ADR 只在现状注记里记"决策落地到哪一步"的一句话指针；不写散落的周报式文档
- `docs/architecture/architecture.md` 是唯一架构图；目录树变更必须同步其 §3
- `docs/reference/` 三件的代码定位一律用**符号名锚定**（如 `registry.rs 的 rr3_reg_beat`、
  `Rr3Envelope 的 seq 字段`），**不写行号**（行号随代码漂移，是系统性风险源）；函数体内某段用
  "外层函数名 + 段描述"。`conventions.md` 正文里的历史叙述（如某轮评审点名的 `conn.rs:NNN`）
  属"当时发生了什么"的实录，不回改。

## 6 标识符命名空间总表（**唯一登记处**；新序列必须先来这里挂号）

第七轮 T-1 的直接产物：仓里跑了 **11 个编号序列**，但从来没有一张总表。
**第八轮 实-5 复数：实为 13 个**——漏登的两条是**装载期校验号 `①-a…⑩-b`**（与 `check.sh` 步号共用圈号字形，
已实际双义）与**批内件号 `n1-*`**（`813a5d0…b11dd2f` 五个提交信息里在用，仓内无登记处）。两行补在表尾。
同批在**发现号**行补一个新前缀 `实-*`（第八轮"实测分母"轮；起号前已 grep 全仓确认零命中，
候选 `U-`/`W-` 实测已被占用、`装-` 虽空但语义只覆盖一半，故取 `实-`）。
后果实测到两次——① 基座轨（改机制核 / 契约 / 门禁 / 装配单本身）根本不知道自己该用哪个前缀，
于是二十把待建门一条工单都没有；② 本轮差点把新序列起成 `G-n`，而 `G-1…G-5` 已被全案 §15.2
的"门禁自指项"占用并被 §8 六行引用——**没有总表时，撞名只能在引用侧发现，而那时已经写进去了**
（历史先例：批次前缀撞单字母造成过一次真实误挂，自此用 `N` 前缀）。

| 序列 | 含义 | 唯一登记处 | 数量与状态 |
|---|---|---|---|
| `批A…批N7` | 施工批次 | 各版本方案 + [status.md](../status.md) | 增补轮六用 `N` 前缀（**不复用单字母**，撞名曾造成真实误挂）；**2026-10-17 批N6/批N7 正式登记**（拍板轮裁定，记录见 [decisions.md §2 行 1](decisions.md)）：N6＝domains 立册（校验⑨读者与 `[[demand]]` 申报面随批N2）、N7＝`mux_group`/平台 `[[conflicts]]` 表（表本体已随批N1 n1-7 三份平台表落树、④ 反推判据半边随批N2 键集施工）；登记后"批N1–N5"区间写法（**现行范围表述类**）一律改全枚举 |
| `P/A/I/N/S/B-*` | 六单元工单（power/act/interact/connect/sense/brain） | 各册 §12/§13 + [索引 §14](../design/模块设计索引.md) | 单元内递增，**跨单元可同数不同义**（P-13 ≠ N-13）⇒ 引用必须带字母 |
| `MC-*` | 0.0.5 第 IV 轮（批M）条目 | [0.0.5 第 IV 轮](dev/0.0.5版本洛克三号对齐与裁定方案.md) 与索引 §14 | MC-0…MC-11，除 MC-7 外全部兑现或判据落地 |
| `台-*` | 站替台账（"用替身当证据"的清偿项） | 索引 §14 | 台-1…台-5，**五条全清零** |
| `R-*` | 索引整改项与裁定指针 | [索引](../design/模块设计索引.md) | R-1…R-16+（R-15 就地结案的那条即例） |
| `D*` | 待用户拍板的裁定 | 索引 §8（结论行）+ 全案 §12（细则） | D1…D15，**无连字符**（写 `D-12` 即第二个真值点） |
| `L-*` | 审计教训类（可复用的病形） | 索引 §11 | L-1…L-33（第七轮 +L-28 判据前件未立 / +L-29 基座轨无工单序列 / +L-30 标识符序列没有唯一登记处；**第八轮 +L-31 判据的入口条件可被省略参数绕过**；**第九轮 +L-32 过度抽象没有账**——全仓只有"缺抽象"的账，没有"多抽象"的账（845 行 shm 传输在 rr-ipc 之外零消费者、transport 两 trait 各 2 实现却**零多态调用点**是首批两例；见 全案 §23.5 块-11；**第十轮 +L-33 决策没有唯一登记处**——L-30 的决策版：`待裁` 散在 **18 文件 / 70 处**而无处能答"悬着几个决定"，ADR 侧对"待评审／待定／未决"三词零命中，后果不是撞名而是**状态腐烂**（施工图 §6 第三问四轮前已裁完并落了数据却仍挂账、同一文件 D 表三个分母），收口件 **基-34**，见 全案 §24.2 裁-1/裁-2/裁-4） |
| `F-*` / `X-*` / `Y-*` / `Z-*` / `流-*` / `T-*` / `实-*` / **`块-*`** / **`裁-*`** | 各轮**发现号**：F 证伪、X 跨文档、Y 运行期、Z 爆炸半径、流 流与失败传播、T 闭合性、实 实测分母与追溯闭环（第八轮）、**块 模块化尺子：内聚 / 耦合 / 极小性（第九轮）**、**裁 决策闭合与依赖面极小性（第十轮：待裁这件事本身闭合没有）** | 全案 §18 / §19 / §20 / §21 / §22 / §23 / **§24** | 是**记录**不是工单：发现号必须要么升格为某单元/基座工单，要么升格为门禁判据，否则算未收口。**`流-` 是第七轮 T-11 从 `S-` 改过来的**——`S-` 是 sense 的工单序列（S-1…S-14，早进批M 台账），第六轮给 §20.7 起号时撞上了它；本表建起来之前，这类撞名只能在引用侧发现。**`实-` 起号时按同一条纪律先 grep**：`装-` 亦为空但语义只覆盖"装载"一半——本轮十六条里打在**命名空间撞号**（实-5）、**索引与台账说谎**（实-8）、**打包轨真值集**（实-12）、**施工图预写的台账文本**（实-15）上的那几条都不属"装载"，`U-`/`W-` 实测已被占用 |
| 门 `a…r` | 全案 §8 的**设计编号**（16 行，**无 m 位**） | 全案 §8 | 与拟步号错位一位（门 h→⑨-f、门 i→⑨-g、门 j→⑨-h），映射表在全案 §8 表前 |
| 拟步 `⑨-a…⑨-r` / `⑨-s` / `⑨-t` / `⑨-u` / **`⑨-w`** / `⑩` / `⑪` | `check.sh` 的**执法编号**（**现 23 把待建**（第十轮，取证基线 `ced8946`）；立表时 20 把、**2026-10-16 对表后 19 把待建、1 把已建**＝步 ⑨-d＝门 f；**第九轮 +⑨-s 枚举单源对账 / ⑨-t 契约版本位必读 / ⑨-u 运行期绑定键表单源 ⇒ 22 把**；**第十轮 +⑨-w 依赖面禁令**（单元与板侧的 Cargo 依赖里不得出现实现 crate `rr-hal`/`rr-bus`/`rrd`；实测 5/5 单元今天直依 `rr-hal`、`modules/**` 内 `use rr_hal_api` 零行）⇒ **23 把**，清单见 全案 §23.4 与 §24.5） | 本 §2 | 引用门必须写全前缀，裸写"第 8 把门"一律视为不合规。**第八轮实测（基线 `b11dd2f`）：专属执法位建成 0 把**（脚本里可数的 step 只有 ①…⑧ 共 10 行，⑨/⑩/⑪ 零命中）；
**2026-10-16 对表（基线 `bbc853d`）：`check.sh` 多出一行 `step "⑨-d 生成物一致性…"`（门 f，n1-8 的 `rr3-codegen --check`）⇒ 可数 step **11** 行、专属执法位建成 **1** 把、待建门 **19** 把，`⑩`/`⑪` 仍零命中**；**"执法位"三层判据见本 §2 拟增三步段头（实-14）**——已有装载期实体与反例用例的门，其反例经步 ①b 进 CI，不得报成"零执法" |
| `基-*` | **基座轨工单（第七轮新立）**：建门本身 + 门禁自指对象的施工件 | 全案 §21.4 + 索引 §14.7 | 基-1…基-39（第八轮 22→25；**第九轮 25→33：+基-27 契约类成员判等 · +基-28 判定位位号锚点集合差 · +基-29 档位 `tier↔[os].kernel` 交叉判据 · +基-30 基座单元体量 · +基-31 代码注释纳入文档门扫面 · +基-32 契约根目录单点（第十轮订正范围：收**未收口的 4 处**非 test 爬路——原写"44 处"实为 Cargo 边数，实测 ≥3 级 54 行、剔 test 后 14 行、其中 5 处已收口常量）· +基-33 Windows 可编面（两枚 cfg 门 + 7 个消费点）**；**第十轮 33→39：+基-34 未决对象唯一登记处（L-33 的收口件）· +基-35 单元与板侧依赖面切 `rr-hal-api`（配 ⑨-w）· +基-36 件 `[[atom.demand]]` 最小集（第七位供给今天 0 份数据 0 个读者）· +基-37 文档锚点符号化（行号换符号名＋基线号，四处漂移实证）· +基-38 装载门同形块计数（替换 基-30 的行数上限）· +基-39 `roles.toml` 覆盖面扩表（4 角色→18 normal 格，门 ⑨-a 那 22 条红的唯一终点）**；**`基-26` 有意空置不给号**——不占号就不是空洞），基-1…基-20 与拟步号一一对应（基-1↔⑨-a … 基-18↔⑨-r，基-19↔⑩，基-20↔⑪）；基-21 门 e、基-22 平台登记表对象件、**基-23 出厂轨装载面齐装、基-24 桥上配置腿、基-25 装载消费真值集清单**（后三件第八轮新立，**均不占拟步号**）。**候选 `G-` 已核为 §15.2 占用，弃用** |
| **装载期校验号 `①-a…⑩-b`**（第八轮补登） | 全案 §5 的**十条装载期校验**逐判定位编号，**31 位**（今天实判 14，口径＝全案 §5）；**代码侧锚点双向对账＝全案 §23.6**（有锚 **16** ／ 缺锚 **15** ／ 代码反向多出 **5**，其中 `⑧-c` 是三处文档在当校验位引用而 §5 行 ⑧ 未列的位号 ⇒ **分母与实际引用集不闭合**，清点归 **基-28**，补位属分母变更、本轮不自改） | 全案 §5（分母表）+ **全案 §23.6**（锚点对账）+ architecture §13/§14 | **与 `check.sh` 步号共用圈号字形 ⇒ 已实际双义**：`check.sh:196` 的步 ⑦ 是"§13 静态断言五件"，全案 §5 的校验 ⑦ 是"接线闭合与额定加总"，两者毫无关系。纪律：**引用一律写全名**——"装载期 ⑦-c1" 或 "`check.sh` 步 ⑦"，裸写 `⑦-c` 视为不合规（门 ⑪ 的机判形状之一） |
| **批内件号 `n1-*`**（第八轮补登） | 批N1 内部的**施工件号**，号段 `n1-1…n1-10`，只活在提交信息、契约头注与施工图任务清单里 | 施工图任务表（真值点，[批N1 收口与装载门建成](dev/0.0.5增补轮六——批N1收口与装载门建成施工图.md)）；契约侧唯一出现处＝`contracts/roles.toml:20` 头注（"随 n1-10 文档回写"）；施工图 **D-3 行**已把"批内件号没有登记处"记为分叉，本行是它的收口位 | **实测号段有空洞**：全仓 docs 只出现 **6** 个号（`n1-1`/`n1-4`/`n1-5`/`n1-6a`/`n1-6b`/`n1-7`），`n1-2`/`n1-3`/`n1-8`/`n1-9` **零命中**，`n1-10` 只在契约头注里出现一次 ⇒ 它是施工期临时编号，不是可追溯的工单序列。纪律：**批内件号不得充当归属**——归属只写单元工单或 `基-n`，件号只回答"这件事在批内的第几刀"；与 `基-*` 同族病（L-29 的镜像：那处是"建门没有单号"，这处是"有单号没有登记处"） |
| **证据号 `<形态>/<编号>`**（如 `对拍/0001`，批N4 件一补登） | 真板对拍/实测证据文件的编号（D11 `armed=true` 界面门与 `severe_mv`/`current_ceil_ma` 安全写引用的真值点容器，brain B-15） | `docs/evidence/`（容器，自带 [README 模板](../evidence/README.md)）；规则唯一落点＝本节 §6.1 | 0.0.5 内**零证据文件**（结构面已立：规则＋容器＋校验器 `rr-ipc::evidence`；证据随 0.0.6/0.0.7 验证窗产出） |

**三条用法纪律**：
- **一个新序列只在本文登记一次**，其余位置一律引用不复制；起新名前必须先 grep 全仓确认该前缀为空
  （第七轮的 `G-` 就是这样拦下来的，`基-` 实测零命中后才用）。
- **发现号 ≠ 工单号**：`F-12` 不能作为归属，它只能作为"这件事从哪查出来"的出处；归属必须写单元工单或 `基-n`。
- **中文前缀（台-/基-）与拉丁前缀混用是有意的**：中文前缀专门留给**不落在任何单一模块里**的轨
  （台账、基座），这样"这条工单属于哪个单元"这个问题在命名上就回答不了——它本来就不属于任何单元。

### 6.1 证据号命名空间（brain B-15，批N4 件一立）

D11 的 `armed=true` 界面门要求"必须引用一条真板对拍证据的编号"，`severe_mv`/`current_ceil_ma`
两个安全写同规则（B-15 ③）——这条门要可执法，必须先有"一条证据长什么样、住哪儿、怎么被引用"
的机制事实（全案 §18 F-24：没有容器，门只能靠人肉判）。规则如下，**唯一落点本节**：

- **文件形制**：`docs/evidence/<形态>/<编号>.md`（容器自带 [README 模板](../evidence/README.md)）。
  形态目录名非空且不含空白与斜杠（首两例：`对拍`＝真板两腿对拍、`实测`＝板上实测；形态是文件
  自声明的分类词、非编号序列，不设挂号，新增形态在本条追加例词即可）。
- **编号**：形态目录内四位零填充十进制递增（0001 起）。**一次对拍一份文件，编号不复用**——
  作废文件保留原文加作废注记，不删除、不回收号。
- **frontmatter 六必填键**（TOML 子集、`+++` 定界）：`form` / `slot_or_role` / `method` /
  `conclusion` / `date` / `recorder`（形态/格或角色/观测方法/结论/日期/取证人）。值为一行
  双引号文本（内部不含双引号）；`form` 必须与所在形态目录名一致（搬家不改 `form` 即红）。
- **引用形制**：`<形态>/<编号>`（如 `对拍/0001`）。
- **校验纪律（fail-closed）**：写面与装载期对**出现的引用**四道全核——形制、文件存在、
  frontmatter 六键齐、`form` 与目录一致——任一道不过即拒，无豁免通道（与"缺键即红"同列
  语义）。校验器**唯一实现**住 `rr-ipc::evidence`（面板写面与装载期两个消费面经依赖面纪律
  只剩 rr-ipc 共同可达；跨文件同形函数须指向共享件，基-30 判据）。机制与策略分离：本节与
  校验器只持"一条引用怎么算合法"；"哪些写必须带引用"归各写面（面板写面随批N4 件九端点
  挂接：`armed=true` 置位、`severe_mv`、`current_ceil_ma`；装载期随落盘数据的 evidence
  引用字段核验）。
- **本命名空间不产出板上结论**：它只规定结论的容器形制与被引用方式；证据本身随
  0.0.6/0.0.7 验证窗产出，"mock 不得充当在位证据"（brain 册 G-4）的正面答案即此节。
