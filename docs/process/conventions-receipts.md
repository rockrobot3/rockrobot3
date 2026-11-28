# rock3 开发规约·历史回执存档

本文件是 [conventions.md](conventions.md) §2 的日期回执原文存档（纯移位迁出，
逐段逐字照搬、未改写）：正文只留可机械检查的判据与指针，逐条"事实—处置—判据"
留在这里供追溯。迁出基线：2026-11-18（续轮二十四），87 段 / 1138 行；
§3 结构纪律里的一条日期注记与号段表保持原位（表体属结构内容，不迁）。

## 回执

**2026-11-02 批N43 对表**：新增独立 std 工程 `tools/pi-periph-base`（W4/P0-C Pi 板侧
五端口后端）**进 ④/⑥ 扫面**（非 esp 条件工程，不 SKIP；⑥ 跑 24 测）；前置条件 =
`rg esp_idf` 本 crate 零命中、`cargo tree` 无 esp-*。同批 `profiles/` 机型集新增
`pi4b-board.toml`（板端点 rtos 档；host 单轴描述符不能容纳双轴，审查裁定记
批N43 计划册 §1.1），由 `rr-registry/tests/contract.rs` 机型集 pin、形状 pin
与 rrd `pi4b_board_profile_boot_inputs_drill` 双向执法。

**2026-11-06 N44 对表**：`profiles/` 机型集新增 `pi4b-host.toml`（Pi 4B 独立
host-only；本机末梢只有 MAX7219/USB 声卡回放，act/sense 不启用），由
`rr-registry/tests/contract.rs` 机型集 pin、形状 pin
`pi4b_host_profile_shape_n44_harness` 与 rrd `pi4b_host_profile_boot_inputs_drill`
双向执法。

**2026-11-08 B11 质量度量步（⑫，已建成）**：`tools/ci/check.sh` 末尾新增
`step "⑫ 质量度量门"`，度量核为纯标准库脚本 `tools/ci/audit/metrics.py`（无外网依赖；
python3 缺位按 SKIP 纪律明说）。四条判据：**重复率硬门槛 3%**（W=10 有效行滑窗，
去注释/字符串、剔纯标点行，生成物与测试面不参与）；**圈复杂度均值/单函数最大值**、
**生产面 panic 落点数**（`.unwrap()`/`.expect(`，测试面同口径截除）与**超长生产
函数名数**按 `tools/ci/baseline/metrics-baseline.json` 钉基线、只降不升（与红账基线门同款
纪律）。**数值真值 = `tools/ci/baseline/metrics-baseline.json`，本文只放与它同批核对的快照**
（防"文档写一套、门禁跑一套"）：重复率 0.57%、CC 均值 2.46 / 最大 13（目标均值
2.5 已达）、生产 panic 落点 73、>20B 生产函数名 0 个（FFI `AudioQueue*` 豁免）。
**2026-11-12 对表（P1 审计件三）**：各批拆件已把峰值 21→14、均值 3.93→3.65，
基线同批收到当前低水位（`cc_max_fn` 21→14、`cc_mean_max` 3.93→3.65）——只降
不升的纪律要求基线跟到低水位，否则"低水位"落不了地。
**2026-11-13 对表（T10 panic 落点进门）**：新增第四项判据并把基线钉到实测 71——
此前"生产 unwrap 181 处"的审计数口径有误（未截掉 `#[cfg(all(test, …))]` 形态的
测试面），按本文件同一截除口径复算为 71，其中 61 处在工具/测试支撑件、10 处在
核心运行时且全部是"查表后自证"或启动期 fail-fast 形态。**此判据的意义不是逼数字
下降，而是让新增 panic 必须被看见**（新增即红，改 Result 或同批给理由后再下调）。
基线只能随重构下调，上调须评审说明理由；快照与基线文件漂移即本文失真，同批改回。
**2026-11-14 对表（panic 71→73，口径归真而非新增债）**：`production_part` 原语义是
"遇 `#[cfg(test)] mod` 即截尾"，实测把 `esp32-periph-base` httpd 测试模块**之后**的
生产段（线程起子与白名单 `CString` 自证）整段藏出度量；本轮结构拆分把测试模块移出
生产文件后，同一代码被看见＝+2 落点（均为启动期 fail-fast 自证形态，非新增）。根因
已修：测试 mod 改为"只摘块、不截尾"，生产项排在测试模块之后不再隐身；同批把
`cloud/testkit.rs`（test-only 支撑件）移入 `tests/` 路径，度量按路径排除，避免把
测试辅助件当生产代码记账。

**2026-11-10 B6 仓面卫生步（⑬，已建成）**：`step "⑬ 仓面卫生门"` 两条 fail-closed
判据——追踪文件不得含本机开发绝对路径（`/Users/<名>/`、`trae_projects`、
`C:\Users\` 三式，脚本自身与文档反例除外）、追踪面不得出现 `target/`/`build/`/
`node_modules/` 目录。`.gitignore` 的"默认忽略所有 + 白名单"防的是**无意收进**，
本门防的是**已被收进**；两者一前一后，缺一留漏面（规则 7-b 的机制化）。
**2026-11-10 B8 补第三条（未入册源码门）**：`git status` 报 `??` 的源码类新件
（`*.rs`/`*.toml`/`*.py`/`*.html`/`*.sh`）即红——⑫ 度量只读 `git ls-files`，
新源码没 `git add` 就不进度量也不进构建，而白名单策略下它连 `git status` 的
常规输出里都不显眼（静默漏面）；扫描器自身失败同红（fail-closed）。
**2026-11-15 补第四条（shell 展开截断门）**：追踪的 `*.sh` 里，可执行行出现
`$name` 紧跟非 ASCII 字节即红（整行注释跳过）。根因实测：bash 3.2（macOS 默认
`/bin/bash`）在 C locale 下把多字节首字节并进变量名，`set -u` 当场 unbound——
`tools/deploy/stage-image.sh` 末行 `$out（arch=…` 在开发机退 1，而 CI 的 bash 5
不复现（本地红、CI 绿最难查）。写法统一 `${name}`，语义不变；同批把全仓 25 处
同款写法一次收口。

**2026-11-10 lint 并门步（①c/①d/④b，已建成）**：CI 的 lint job（rustfmt
--check + clippy -D warnings，仓内全工程）此前不在本地门禁里，手工折行与
clippy 告警一路入库（实证：`e1a93eb` 收 ⑱ 文件漂移、`eae280e` 收
too_many_arguments）。本批把 CI lint 两段拆为 ①c（fmt：workspace + 全部独立
工程，esp 条件工程不编译照跑）、①d（clippy：workspace）、④b（clippy：独立
工程，排除口径同 ④）三步并入 `check.sh`，本地与 CI 从此同一强度。

**2026-11-10 status.md 实况锚点重锚（`8985677`→`e22f20a`）**：旧锚哈希不在对象库
（批N33 件一 amend 后无引用可循；同批失效的还有 `9c07b3e`/`dc3a1d6` 等改写前
命名空间哈希，现行哈希自批N30件三 `e133217` 起在册）。主锚点、里程碑表头、
在制品段三处重锚，死哈希以历史注记统一登记（不改写历史清单原文）；
e2e 并发确定性同批结案——真 flaky 是 uart flock spawn 瞬态（批N21 件一已
根因拆修），撞 tag 假设被枚举证伪（各测试文件内 tag 互异、跨文件靠 pid 隔离）。

**2026-11-10 architecture.md brain Python 化残留清理（10 处）**：分层图两处、CI job 描述、
contracts 定位语、rr-ipc schema 对齐语、模块树 rr3-brain 布局、legacy 迁移映射两处、风险表
R3（标消除）R4（改宿主/模块/板侧 Rust↔Rust）。保留不动的合法提及：schema `lang` 域
（rr3-pack python 分支仍在册）、ADR-0014 文件名、带日期的审计史录（基-33 五处）。

**2026-11-10 生产面 unwrap/expect 收敛普查 + metrics 双定义遮蔽修正**：正确口径普查 74 处
——绝大多数历代批次已带不变量注记（expect 消息点名判据），16 处测试辅助面
（test_support/test_fixtures）按 fail-loud 语义豁免，4 处真 UNWR 补不变量注记
（rr-ipc header 长度守卫直读 3 处、rr3-pack entry 行扫描直读 1 处）。同批修正
metrics.py **双 `production_part` 定义遮蔽**（旧版"只截 mod"定义在后遮蔽新版
"散点剔除"，t15 修复实际半失效）：旧版删除后散点 cfg(test) 项（CC≈1）不再
稀释均值，基线按真口径重校 3.92→3.93（红账方向不变，只升 0.01 属口径归真）。

**2026-11-10 CC 热点 ≥24 档全拆（max 26→21）**：rr3-audio-rec main（26→5，起子
`rec_spawn_threads`/解码 `decode_loop`/收兵 `finish_and_report`，`err_streak`/`resyncs`
归并进 `DecodeState`，三线程面收口为 `RecThreads`）、esp32-uart-echo main（26→14，
`drain_oversize`/`recv_payload`/`echo_frame` 三臂）、esp32-periph-base `spawn_mic_smoke`
（25→11，统计窗收口 `MicStats` 结构体 + `chunk_flush`/`dump_watch`/`host_watchdog`）、
pi-periph-base `line_round`（24→16，OLED/FACE/SERVO 三臂抽 `oled_text`/`face_expr`/
`servo_pair`）。宿主面全量 clippy+测试绿；esp 面按 ④b 口径 fmt 过（本地不编译，
行为等价逐分支核对）。基线 `cc_max_fn` 26→21（真降，红账只降不升兑现）。

**2026-11-10 远端 CI 三红根因收口（P0-3 连带，夹具/CI 层零生产码改动）**：
远端回执考古红至至少 12 run 连败、早于本会话——P0-3（6954b5d）消除"漏 env 静默
落 sim"后，部署真值 hal_backend=linux 直透无硬件环境，三个演练载体全部翻红：
① sim-assemble——rr3-sim 只给 rrd 设了 sim env，supervisor 按装配单注入覆盖之；
修 = 沙箱装配单副本 sim 变换（`rr3_asm_sim_text`：hal_backend→sim +
audio/servo/display 专面摘除，改写后重解析复核，仓源装配单零改，与 --chain 同语义）。
② distro-smoke——双根因：仍按批N32 已删除的 Python brain pkg 树打包（③ 即爆）+
容器内 linux 后端不可兑现；修 = brain 装机面改 Rust 形（bin+manifest+config，Python
shim 断言与 python3 apt 安装撤）+ ⑥ 前装机 distro.toml 副本 sed 变换 sim（变换前后
双断言）+ RR3_BRAIN_CLOUD=mock。③ matrix——job 缺 `docker/setup-qemu-action`，
① 的 ipct003.sh arm64 容器真跑（--platform linux/arm64）无 binfmt 时失败非 SKIP
被误判"对拍破裂"；修 = 补一步与 ipct003 job 同源。本地复现-修复-复绿闭环：
sim-assemble 六模块 running + denied_publish=0、sim 27 单测绿、matrix FAIL=0。
教训入册：**改"隐式缺省→显式数据"类真值收紧时，必须全量清点每个无硬件消费
载体（演练夹具/容器冒烟/CI 矩阵），否则门禁绿、真装配红**。

**2026-11-10 第六轮跨平台静态审计（四平台三方对账，[审计文档](dev/0.0.8/静态生产审计/0.0.8版本静态生产审计-第六轮-跨平台.md)）**：
真值漂移一处——esp32s3 平台表 UART2 conflict 集仍钉 17/18，真板订正（6c0169e 迁
41/42）只同步了 profile 键值漏了契约表，已随真值修（ed82016）；mkmodule fmt 漂移
系 e22f20a 遗留、新并入 fmt 门首跑实爆（3be6ad5）。核过无损六条：Pi 绑定三方一致、
linux_pwm/gpio 无平台名分支（chip/channel 全走绑定）、nmcli 假设显式、esp32audio↔
mic.rs 四道门同形（线上恒 s16le，32bit 截定在板侧）、rk3566 演练消费面闭环。
遗留观察（**2026-11-12 P1 件一已裁**）：I2S 未进 esp32/esp32s3 平台表——
音频进装配单角色域前须先补登记，否则 ③-b/⑧ 对音频脚失明（审计文档 §四存照）。
**裁定：不登记**——`kind` 封闭七值无 `i2s`，扩词域属契约级评审；I2S 三信号脚由
board 具名装配直给、无 `[[pin]]` 角色消费者，平台表的 ③-b/④-b/⑧ 判据面不适用。
两表内已立明文注记（事实/判据/何时再进表三件），事实真值点维持 board 引脚真值段
单点。存照见 [第七轮 P1 落地审计](dev/0.0.8/静态生产审计/0.0.8版本静态生产审计-第七轮-P1落地.md)。

**2026-10-16 对表**：`bbc853d` 起脚本多了 **`step "⑨-d 生成物一致性…"`**（门 f，n1-8 的 `rr3-codegen --check`）⇒ 现在可数的 step 是 **11 行**、
**专属执法位建成 1 把、待建门 19 把**（`⑩`/`⑪` 仍零命中）；**第九轮再 +⑨-s 枚举单源对账 / ⑨-t 契约版本位必读 / ⑨-u 运行期绑定键表单源 ⇒ 待建门 22 把**（全案 §23.4）；**第十轮再 +⑨-w 依赖面禁令 ⇒ 待建门 23 把**（全案 §24.5，取证基线 `ced8946`）。这正好**验证了 实-14 的三层判据**：
门 f 的三层里第 ③ 层（专属执法步）今天落了，前两层早就有——**报数必须说清是哪一层，否则同一把门会被报成"没建"和"建了"两种相反的话**。
**2026-10-18 对表（基线 `f0a1c75`）**：第 0.5 步件一建 **`step "⑨-w 依赖面禁令…"`**（基-35）⇒ 可数 step **12 行**、
**专属执法位建成 2 把、待建门 22 把**；判据＝单元 Cargo 直依 `rr-hal-api` ＋ use 面白名单四类冻结（backend 装配/sim/provision_store/Rr3PcmStream）＋ 板侧禁 `rr-hal`/`rr-bus`/`rrd`；
单元 Cargo 级 `rr-hal` 拆边（装配上移）与 `rr-bus` 契约面化两条窄化敞口随第 0.5 步收口行入 status.md（decisions.md 基-35 行登记）。
**2026-10-21 第十一轮实测（基线 `14e7dcc`，本人跑 `grep -c '^step '` 得 **18**）**：可数 step **11 → 18 行**，
**专属执法位建成 8 把**（`⑨-a ⑨-c ⑨-d ⑨-e ⑨-n ⑨-r ⑨-w` ＋ **`⑩` 门 d 面板覆盖率**），`⑪` 仍零命中（文档门是 ⑤）。
⇒ **待建门重取为 16 把**（`⑨-b ⑨-f ⑨-g ⑨-h ⑨-i ⑨-j ⑨-k ⑨-l ⑨-m ⑨-o ⑨-p ⑨-q` 十二把 ＋ `⑨-s ⑨-t ⑨-u` ＋ `⑪`）；
**本轮再新立两把**：`⑨-y 跨平台可编面`（Windows 红 crate 数用基线钉数法，归 `基-33`）与 `⑨-z 真值集清单成员判等`（归 `基-40`，裁-15）
⇒ **现口径＝待建 18 把、已建 8 把、全集 26 名**。**上面几段里的 20/19/22/23 全是时点值，一律以本行为准**；
本行也只是一次快照——按 §25.6 的纪律，这类数今后应由 `基-28`/门禁步产出而不是由本档持有（`基-37`/`基-28` 未落地前，每次引用仍需重跑）。
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
基线／C TRAITS·PIN_KEYS 差集）对低水位基线文件 `tools/ci/baseline/gate-a-baseline.txt`（count>基线即红防回涨、
文件缺/坏即红 fail-closed、count 下降打印须同批下调）；配套 (a) 报告走 stdout 可见面，配套 (b) 转红
终点＝基-39 角色覆盖扩表（N2 那一刀同件落 roles.toml 两角色 power.rail.sense/power.current.sense，
covered normal 4→5、红账 23＝基线初值；switch/charger 归批N3——switch 需新派生键属契约级裁定、
charger 随器件角色进场）；键集四腿全部自代码常量导出（`RR3_ASSEMBLE_HANDLE_TRAITS`/
`RR3_ASSEMBLE_DESC_KEYS`/`RR3_PIN_KEYS` ＋ slots/roles 两契约表，单真值点不抄第二份，⑨-n 先例）；
六表第 5 表已由 ② parse 层执法、第 6 表（平台 role→具名构造）归转红收口件——本步如实不判。
**2026-11-12 对表（P1 审计件四，⑨-a 转执法）**：红账 **21→0**、`gate-a-baseline.txt`
归零，本门由报告模式**转执法**（count>0 即 FAIL，文件保留为低水位锚 0）。两件事同批：
① **真覆盖**——`roles.toml` 10→14（`net.wifi.link`/`audio.capture.mic`/`audio.playback.spk`/
`audio.vad.gate`，`kind = logic` 无 `[pins]` 绑定、`requires`/`derives` 双显式空，iface 取
`Rr3Link`/`Rr3AudioSource`/`Rr3AudioSink` 真实承载面），`roles.rs` 形状放宽 logic 角色的
空 requires（signal/power 仍非空，反例用例在册），`rr3_role_gaps` 同批把 logic 角色移出
"缺角色"点名面（否则能力面角色化反把 boot 判降——`surgery_gates` 两用例实测拦下过
未配套的这一半）；② **口径裁定**——A 段分母收为 `layer = body` 的 normal 格（base
内建四族与 cognition 软件四格无 `[pins]` 角色域，设计原文"logic 内建格覆盖口径归转红
收口件另裁"即本裁），C 段豁免 `Rr3DigitalIn`（S-10 数字输入面未立项、零格位消费者，
不是漏覆盖而是还没有格）与 `role`/`instance`/`direction`/`mux_group` 四枚永不进
`requires` 的键（自指键/条件键/独立方向判据/互斥组归平台 conflicts），逐条理由写在
`check.sh` ⑨-a 段内。验证：`check.sh` ⑨-a `红账 0 = 基线 0`、⑩ 仍 `0/83` 无连带。
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
（I-19 判据字面）机器化＝双行基线 `tools/ci/baseline/gate-d-baseline.txt`（行 1 红账上限：回涨 FAIL、
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
三处假通过口同笔闭合——interact `rr3_pcm_integrity` 加零字节下界（(0,0) 不再是合法通过态，
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
**2026-10-19 对表（批N4 件九首笔，裁定 6.3 开放项数据结构落地）**：`docs/openitems.toml`
新立（结构源，机读 TOML）——裁定 6.3 字段五项 `{条目/entry, 分类/kind（缺码|无载体）,
工单号/owner, 证据路径/evidence, 状态/status}`；与两既有面的分工写进文件头注（单真值点
纪律）：门 d 差集**机器真值**仍＝步 ⑩ " D " 行打印（本表 `fields` 是登记时点归因快照，
成员增减以步 ⑩ 为准、随转绿同批改状态），status.md 敞口表**生成关系**随本表建立
（现存敞口行迁移不做本批，历史行不改写）；登记内容 = 门 d 差集 73 项按读面分五组
（ops 五模块 30 槽／pin 11／role 7／domain 11／axis 14，各组 kind=无载体、owner=件九
观测面扩端点工单、evidence=步 ⑩＋基线行 1、status=open）——brain ops 六槽已随件六二
转绿不在差集。本机 python3 3.9 无 tomllib ⇒ 形制自验走一次性脚本（5 组/73 引用/零错，
仓外即弃），机判校验归 CI（python3.12）后续批随门位扩展再议。docs/README 库地图
同批登记；status.md 敞口表对账归批N4 收口件。
**2026-10-19 对表（批N4 件九笔2，观测面收官——ops 组 30 槽全转绿）**：契约 32→37
（topics.toml append 五模块 `rr3-<mod>/ops/list` request 主题，照 brain 先例 stable=L2；
八处计数 pin 同批：rr-registry topics.rs ×3、tests/contract.rs 32→37＋request 13→18、
rrd topics_load.rs ×3，35→40 两处随契约计数联动）；五 module.toml exposes 双写（power
自此首个事务面主题）；五模块 composite 快照应答臂＝**白名单组体**（槽名与 assemble/
composite.toml 槽表申报同源——act led/servo 四槽、connect bt/wifi 四槽、interact
face/player/panel/selftest 十一槽、power battery/charger 四槽、sense audio/doa/vad
七槽；密钥/凭据/行程/瞬态不在快照面）；面板 `GET /api/{act,connect,interact,power,
sense}/ops` 五端点同一 BusCall 透传范式（interact 自身也走 rrd 归属门，不因同进程
豁免），covers 申报 30 槽 ⇒ ops 路 36 槽全转绿，端点 pin 15→20，基线
`gate-d-baseline.txt` 行 1 **73→43**（余红 = pin 11＋role 7＋domain 11＋axis 14，
读口归件九后续笔；分母 79 不动）；openitems.toml ops 组 status=done、fields 清空
（头注对表行同批）；前端 tab-sys 加「模块配置面」只读卡（模块名入列四栏渲染）。
门 d 差集机器真值仍＝步 ⑩ " D " 行打印，openitems 归因表随转绿同批改状态——
两口径本批实跑一致。
**2026-10-19 对表（批N4 件九笔3，配置面——brain ops 槽写口＋applies 第二条申报）**：
契约 37→38（topics.toml append `rr3-brain/ops/set` request L2；计数 pin 同批：
rr-registry topics.rs ×3、tests/contract.rs 37→38＋request 18→19、rrd
topics_load.rs ×3，40→41 两处随契约计数联动；contracts/README.md 现值窗行
31→38 补齐——批N2 时点的现值句笔2 漏更，本笔对表时补）；**写面三问拍板**：
① 寻址 = 组.槽与 `ops/list` 快照读源对偶（llm 组写实效源——模型行 active 在则
写行、fallback 写 cloud 段；asr/tts/dialog 组与 llm.timeout_s 写 cloud/dialog 段）；
② 生效 = live 内存生效（写臂重建 asr/llm/tts 客户端，llm 重建即换会话 history
归零；dialog 就地改参——`rr3_utt_expired` 现读 `max_utterance_ms` 不丢开轮
状态），**不落盘**——应答 `persisted:false` 如实申报，重启回 config.toml 真值
（不申报 reboot：内存写不落盘，重启是丢失不是生效）；③ 落盘归后续批（Python 无
TOML writer、重写丢 config 注释、Y-2 单写路径是 Rust persist.rs——三条协商点
随本行在册）。brain 写臂 = 纯函数 `rr3_ops_write`（白名单/类型域值 fail-closed
点名拒，密钥与 key_ref 不在写面）＋分发臂重建段；module.toml exposes 双写 +1；
单测 TestOpsWrite 六用例（brain tests 归 CI brain-unittest job，本机 python3.9
无 tomllib 以仓外一次性 runner 验证 19/19；同批给 cloud/__init__.py、cloud/llm.py
补 `from __future__ import annotations`——包内其余四文件已有，风格补齐，PEP 604
注解 3.9 import 安全）。面板 `POST /api/brain/ops`（BusCall 透传，body 原样交
owner 裁决），端点 pin 20→21，covers 六槽申报 = 与 GET 读口同集合（∪ 不增，
基线 43 不动），applies 申报实况 pin 一条→两条（model/key live ＋ brain/ops
live——写口动了可配置集必填生效路径，I-20 双门同批过）；前端 brain 模型面卡加
写槽行（数值槽前端转 Number，owner 仍 fail-closed 兜底）。
**2026-10-20 对表（批N4 件九笔4，接线图——门 d 红账 43 全转绿收官刀）**：
rrd 第七 ctl 方法 `rr3.ctl.wiring`（ADR-0010 白名单六 ctl→七 ctl 同批扩，rpc.rs 解析/白名单/计数
pin 同批）：assembly.rs 建 OnceLock 全局单例快照（boot 成功臂 set 首份胜出）＋纯渲染
`rr3_assembly_render`（四路底账 profile/pins 11 键/domains 11 键/wires 9 键/axis 14 键/roles/checks
逐条——单测用门 d 同源词表逐键钉死，全局壳 None 分支由 e2e --manifest 路径覆盖）＋`available`
位（--manifest 调试路径如实 `available:false` 点名 --profile 解锁路径，不造假数据）；profile.rs
boot 链交出快照（Rr3ProfileBoot 加 `snapshot` 字段、Eq 随 f64 摘除；axis 档案装载拆
`rr3_axis_files_load` 单次装载——投影与快照共用同一清单，不重读目录造第二真值点）；conn.rs
serve_ctl 加臂＋已注册连接显式 OR 臂；daemon boot 成功臂 set 快照。**[pins]×wiring 全交叉判据**
三族三态（pass / not-applicable / fail；未知值 fail-closed 折 fail——恒败腿不得折算通过）单点在
assembly.rs 纯函数；面板 selftest 加**腿四 wiring**（判果是 rrd 的产出只转述不重判，第二真值点
禁令——`leg = "wiring/<item>"` 逐条摊进 legs＝全案 §7 ⑤"出厂自检必测全交叉"原文；取数失败/
快照不可得＝单腿如实 fail；**verdict 语义改判** `有 fail 即 fail、无 fail 且至少一腿真判 pass 才
pass`——n-a 不拖累不救场，全 n-a＝无一真判如实 fail；旧 all_pass 在无 n-a 时行为不变）。面板
`GET /api/wiring`（ctl.wiring 一问一答 200 原样透传，端点 pin 21→22），covers 申报四族全集 43 项
（pin 11＋role 7＋domain 11＋axis 14，与分母五路并集逐项同源；计数 pin 测试同批钉 43＝
11＋7＋11＋14）⇒ **红账 43 全转绿**，基线 `gate-d-baseline.txt` 行 1 **43→0**、差集空、门 d 转
红（差集 > 0 即 FAIL，转绿终点达成）；openitems 四组 status=done、fields 清空（头注对表行同
批）；前端 tab-hw 加「接线图」卡（判果表＋四族底账表渲染、available:false 如实展示）＋自检卡
文案同批点接线腿与 verdict 新语义。
**2026-10-20 对表（批N5 首件，按形态分档可达终点表落档）**：交付物
[批N5首件——按形态分档可达终点表.md](dev/0.0.5/增补轮六/0.0.5增补轮六——批N5首件——按形态分档可达终点表.md)
（判据原文＝全案 §17 批N5 行；改判口径＝首版手写＋逐格标 `文件:符号` 或敞口号——原"由结构化开放项
数据生成、禁手写"自相矛盾，证伪审查轮订正作废）。三形态（RK3566／树莓派／ESP32）× 十项 30 格逐格
台账，三态判定口径：**能做到**＝机制在树且载体在位带 `文件:符号`；**不能（无载体）**＝机制在树载体
不在，缺什么写"无载体＋触发条件"；**不能（缺码）**＝机制不在树带工单号。取证时点 2026-10-20 基线
`b6d5902`；判定分布＝能做到 8 格、不能 22 格（无载体／缺码／语义未裁，逐格带触发条件或工单号——
"不能"不带触发条件即 FAIL 是判据原文第 3 条）。零 `.rs` 零 `contracts/*.toml`；docs/README 主表、
开发计划 §3 批N5 段、status.md 轮次条三处登记随批N5 首件同批（§5 文档纪律的本行即对表动作）。
**2026-10-20 对表（批N5 件二运维留痕三笔收官＋波及订正，`low_power`"只升不降"敞口关闭）**：
判据原文＝全案 §17 运维留痕行三半句。笔1 环容量键（`f8329f0`）——profile schema_version 4→5，
`[runtime]` 段 `log_ring_cap` 键装载缺红逐条点名；**载体订正**：环容量进装配单 `[runtime]`
顶层段而非 HAL 描述符键集（`RR3_ASSEMBLE_DESC_KEYS` 有契约测试逐名钉死，且环容量是 rrd 运维
参数非外设绑定），F-30 改判判据"环容量∈描述符键集"的落点随载体订正为"`[runtime]` 段键集，
装载期缺即红"（判据精神不变）。笔2 rrd 自身日志有界落盘（`8e02be8`）——stdout/内存环/落盘
文件三面同源、sink OnceLock 首份胜出、启动回填尾 64KB 置环头，"一次重启后仍能取到崩溃前的
最后 N 行"另一半成立（模块侧批N4 件八已成立，rrd 自此同权）。笔3 解除路径（`8a336e5`）——
契约 append `rr3-power/battery/ok`（event L1，主题 38→39、event 12，schema 三必填
additionalProperties=false；RR3_PINNED_TOPICS 数组随本笔补齐批N4 件六二/九二/九三漏掉的
7 条 ops 主题 31→39，既有缺口顺带修复留痕）；`Rr3LowEdge` 补 `rr3_low_edge_recover`
（回阈首拍恰一次；先 recover 后 cross、同一采样值的喂入次序是契约——先喂 cross 会被再武装
分支吞掉恢复沿）；rrd `rr3_reg_lp_lower()`（返回是否真下降沿）＋conn battery/ok 臂
（falling 才广播 `system/power(reason=battery-ok)`，重复 ok 不重复广播）＋新
`e2e_power_ok` 全链线级证据；"low_power 旗补解除路径（今天只升不降）"敞口自此关闭——
两个沿一个对：battery/low 上升沿置旗拒新 spawn／battery/ok 回阈下降沿清旗恢复新 spawn。
波及订正（`69b2b47`）——rr3-act 订阅臂按 reason 分流：battery-ok 解除广播不闭锁不解禁
（低电段中 act 重启会被解除广播误闭锁至重启；解禁只认轨压样本复位语义④，喂入面挂账批N3
件七不变），缺 reason/未知值 fail-closed 闭锁；status.md 能力行"只升不降"表述同批补正。
**2026-10-20 对表（批N5 件三：装载消费真值集清单＋三处消费方判等，host 半边收官）**：
判据原文＝全案 §17 基-25 行＋实-12 修法；范围裁定＝烧录与装机 **host 半边**归基-25，rtos espflash
烧录半边等真板另登（余量落索引 §14.7 基-25 行，缺载体按敞口表章程不混装）。笔1（`280da7b`）——
rrd 立 `rr3_runtime_truth_set` 唯一清单（装载面 files 七条＋dirs 一条，单测真仓双装配单逐成员钉死；
清单是数据不抄第二份）；rr3-pack 照单扩包：装配单先按装载同判据预验（坏单拒出包）→ truth set
缺册检查（缺册拒出包）→ 按清单循环落位。笔2（`497e1a6`）——装机面消费方判等：distro-smoke ⑤
补平台册装机断言（块-15 修法 b）；post-build.sh 新立第 5 件 profiles/contracts/hardware 整目录
随装（块-15 修法 c＝基-23 第二半提前归基-25，S99rrd 传参启用消费仍归基-23）；OTA 判据占位落
docs/design/分区布局与OTA槽位.md（staging 真值集判等＋manifest 契约 schema 版本列不等即拒装，载体随
M6）。块-15 施工顺序约束"基-25 必须先于基-23"host 半边兑现；status.md 轮次条、开发计划 §3、
索引 §14.7、architecture §13 顺序约束注记同批对表（本笔对表）。
**2026-10-20 对表（批N5 件四：基-23 出厂轨装载面齐装＋`--profile` 升必选，三笔收官）**：
判据原文＝索引 §14.7 基-23 行＋L-31 实-1。笔1（`ff2f318`）——post-build.sh 安装根全迁
`/usr/lib/rockrobot3`（与 deb 轨同名；modules-root 由装配单路径上两级推导是物理前件，
`/var/lib/rr3/staging` 是分区挂载点不属安装位不随根动）＋S99rrd 改传 `--socket`/`--contracts`/
`--profile` 三参数（与 rrd.service ExecStart 逐字同源，`--manifest` 循环删除）＋机型档
distro.toml 随装＋真值集三目录落盘断言进场（装配冒烟 22 项断言，镜像级）。笔2（`8b1390c`）——
rrd cli 双缺拒门（装配面必选：`--profile` 与 `--manifest` 至少给其一，两者同给也拒〔Q3〕；
拒门在 socket 空检查之后保住既有拒因序）＋单测改判 10 处＋监听面缺格跳过夹具适配（stub
manifest 真实落盘＋夹具树 Drop 清理）＋`rr3_start_manifest_stub` 后门夹具＋裸"拒绝"needle
收窄＋daemon None 臂注释收窄＋rrd-cli.md。笔3 对表：status.md 敞口 20 与轮次条、rrd.service:4、
终点表 RK3566 观测格、索引 §14.7、architecture v0.16 段④与 §13 装载门行、开发计划 §3、本节。
敞口表无增减；敞口 20 修法半边已落、证据半边（出厂机真跑启动日志＋坏接线档拒起对照）归真板批
不结项。
**2026-10-21 对表（批N5 件五：出厂自检判据四缺口四笔收官）**：
判据原文＝全案 §17 出厂自检行＋§20.1 Z-4 行（"包内四份契约文件齐全且 sha 对"）。
笔1 F-31 预期表进判（`6f32e94`）——`[rr3-interact.selftest.expect]` 预期表落装（固定腿
speaker/relay/face 逐名三态值域闭集＋组预期 zero-fail），verdict 纯函数化改判**逐腿结果集与
预期集等值**（F-31：整体红绿是派生量非判据），段/键缺省回落全 pass＋组零 fail，未知腿名/
在册外值 fail-closed 拒 boot；笔2 speaker 内置测试音（`dcdb28f`）——beep 臂合成 440Hz/200ms
短正弦直喂播放面（同径 volume_pct 增益），波形源进程内自持不再依赖 tts_pcm 流（I-21① 由构造
保证），speaker 恒败腿自此消失（D15"测试音内置策略件补回"兑现，落批N5 件五非裁定原文的
批N4）；笔3 wiring ⑦-c1 额定加总族（`b5016eb`）——wiring 带 feeds 后按 net 拓扑加总判
（F-10 峰值直加 Σpeak≤rated_a×1000，三色 DFS 环/悬空即漂移 fail，菱形分叉只收账一次），判据
真值单点仍在 `rr3_assembly_cross_checks`，装载期不预支（裕量取值是策略位须数据承载，⑦-c2
件侧归批N2）；笔4 契约 sha 链（本笔）——rrd 新 `contracts.rs`：手写 FIPS 180-4 sha256
（rrd 零密码学依赖纪律，NIST 五向量钉正确性）＋boot 装载链验收后契约四册
（topics/slots/roles/devices）sha256 记账快照（OnceLock，定格点 fail-closed 拒 boot）＋第八
ctl 方法 `rr3.ctl.contracts`（白名单七 ctl→八 ctl，`--manifest` 调试路径 `available:false`
如实）＋自检腿五 contracts"齐全且 sha 对"（缺册＝齐全破、sha 漂＝sha 对破，判据同质无 n-a
豁免面）＋面板 selftest `contracts/*` 组 verdict 加 zero-fail 臂（与 wiring/* 同规则）＋预期表
contracts 键。门 d 分母 79 不动（无新 HTTP 端点，covers 零波及）。单测：rrd lib 250（＋sha256
向量＋判果渲染两用例）、e2e_profile 5（＋契约问询四册点名）、interact 62（＋五腿/预期表/
正弦确定性对拍）。**批N5 件五全件收官**（笔0–笔4）。对表十处：status.md 能力行十二方法与本
轮次条、开发计划 §3、interact README beep/selftest 行、索引 D15、全案 §17/§20.1、ADR-0010、
rrd-rpc.md ctl 表（补 wiring 行——件九笔4 漏登，本批同批补齐）、architecture devices 行、本节。
敞口表无增减。
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
[0.0.5增补轮六全案](dev/0.0.5/增补轮六/0.0.5增补轮六——底座资源模型与能力面抽象补课全案.md) §5/§8/§15 与
[architecture](../architecture/architecture.md) §13 的"待建"门行；
同批还须把 `tools/esp32-periph-base` 这类独立固件 crate 从 ④/⑥ 的排除表里放进来（否则"格格双达标"
对板侧永远是空话）。本仓规矩不变：**门禁的强度只等于脚本真做的事**。

**2026-11-13 CI 覆盖面重构（"本地只写代码，构建与测试在远端"）**：本地资源有限且
环境不标准（macOS + 非标工具链），编译/测试/集成一律以远端回执为准。原 `ci.yml` 单文件（八 job）先扩为 12 job、再按职责拆成 **四条流水线**
（`gate.yml` / `platforms.yml` / `integration.yml` / `publish.yml`），门禁唯一入口固定在 CI：

**2026-11-16 冒烟判据单点化**：两段冒烟的内联 shell 抽到
[`tools/deploy/image-smoke.sh`](../../tools/deploy/image-smoke.sh)（`file` / `run` 两个
子命令，CI 与本地同一份判据）。动机：判据只住在 workflow 里时，改一次只能"打标签→等
CI"往返，本地无法复跑。SKIP 纪律：仅 `RR3_SMOKE_ALLOW_SKIP=1` 时无 docker 打 SKIP 并
退 0（本机开发），CI 不设该变量——缺 docker 即红，不以 SKIP 充绿。两段各自独立退出，
容器起不来 / 装机树缺件 / 二进制不可执行 / 六模块未起齐 / 拒绝 boot 归因不混。

**2026-11-16 平台矩阵重构回执**（commit `6438c2c`，run 43803306704 三 workflow 全绿）：

**2026-11-16 追加：Windows（登记平台 L1）腿建成**（commit `444ea21`）：`host-windows`
在 `windows-latest` 上跑 workspace 编译面 + `cargo test --no-fail-fast`，敞口 #5 的
Windows 半边结项。首跑 4 处红全部是**跨平台假设**而非产品缺陷：3 处 CRLF（checkout 的
`autocrlf` 把文本转成 CRLF，而装载面/测试用 `include_str!` + `"\n"` 的 replace/断言 →
根因修 `.gitattributes` `* text=auto eol=lf`）＋1 处路径分隔符（期望值改由 `Path::join`
生成）。注解通道自身也修了三处：`cargo test` 不支持 `--keep-going`（改 `--no-fail-fast`）、
`Tee-Object` 默认 UTF-16 导致零匹配（统一 `-Encoding utf8`）、`failed` 大小写不敏感误配
（改 `-CaseSensitive`）。

**2026-11-16 供应链门建成（`supply`）**：`gate.yml` 新增 `supply` job，
[`.cargo/deny.toml`](../../.cargo/deny.toml) 是策略唯一真源，[`tools/ci/platform/supply-audit.sh`](../../tools/ci/platform/supply-audit.sh)
枚举 **22** 个工程根（`base/runtime` 工作区 + `modules/`/`tools/`/`base/ota/` 独立工程）
逐个 `cargo deny --locked check`。四类判据：①许可证**显式白名单**（2026-11-16 全根
实测只有 MIT / Apache-2.0 / ISC / Unicode-3.0 / BSD-3-Clause / Unlicense /
CDLA-Permissive-2.0 七类，无 copyleft；私有件 `publish = false` 显式忽略）；②RustSec
公告（未维护项只对直接依赖执法，避免上游陈账把每次推送染红）；③来源白名单（仅
crates.io + 仓内 path）；④通配版本依赖拒绝。本地无 cargo-deny 时 SKIP 明说，CI
`--strict` 工具缺位即红。`.cargo/deny.toml` 由 `.gitignore` 的 `.cargo/**` 白名单
覆盖（默认忽略所有策略下必须显式登记）。**首跑红与根因**（如实留痕）：`--locked` 写在子命令 `check` 之后，
cargo-deny 的公共选项只认子命令之前（`cli/common.html`），22 个工程根全部瞬时失败；
注解侧同时暴露第二处缺陷——只取 head 时逐工程 FAIL 行把唯一的命令级错误挤出上限，
已改为"诊断行优先 → FAIL 行 → 日志尾部"三级回退，脚本失败时也回显日志首行。**次跑红**：
`wildcards=deny` 把私有件的 path 依赖（`rr-ipc = { path = … }`）判成通配——这是工具
语义与仓面形态的差异，不是缺陷代码；按 `allow-wildcard-paths = true` 收口（私有件豁免、
公开件继续执法，不加逐 crate skip 名单）。**同批时间维度**：`gate` 加周程（UTC 周日
18:30 = 北京周一 02:30，与 platforms 的 18:00 错开）——RustSec 库随时间漂移，没有推送
也必须复核；job 表的触发列同批订正为实际形态（integration 无周程，platforms 有）。

**2026-11-16 ESP 门按芯片拆腿（commit `21797ee`，run 43833170142 三工作流全绿）**：
`esp32-gates` 由"六门串行单腿"改为 `esp32` / `esp32s3` 矩阵两腿，六门判据与覆盖面
零缩水（classic 腿 5 门、S3 腿 2 门，见上文 job 表与芯片平台表）。**实测回执**：
classic 腿 17.6 分钟（08:40:41→08:58:18，5 门全 PASS）、S3 腿 12.1 分钟
（→08:52:54，2 门全 PASS），两腿并行故墙钟 = 17.6 分钟，单腿时代的 18–20 分钟
被压到一条腿的长度；本轮两腿都是**新缓存键冷跑**，toolchain 步各约 5 分钟，热缓存
只会更短。**归因收益**：S3 档失败不再连带掩盖 classic 结论，注解 title 一律带芯片
前缀（`esp32-periph-base` / `esp32s3-act-sense` 等），一眼看出是哪块板的问题。
双档模块门 `rr3-act-sense-espidf-check.sh` 增芯片参数（`esp32` / `esp32s3` / `all`，
缺省 `all` = 本地整门口径不变），参数笔误在查工具链之前就退 2。
**同批根因修复**：`esp-toolchain` 的缓存键原先不含 targets——GitHub 缓存按 key
不可变，同一 key 只留先存者，另一芯片腿每次都要重装自己的 Xtensa/IDF 工具集；
键改为 `esp-<os>-<targets>-<espup>-<idf>` 后各腿各存一份，同 targets 的 `ipct003`
继续共享。

**2026-11-13 源文件行数上限（⑭，新立）**：每个 `.rs` 源文件必须 **<600 行**
（含测试文件）——超过即按职责结构化拆分，不是加白名单。`check.sh` 新增
`step "⑭ 源文件行数门"`：`git ls-files '*.rs'` 逐文件计行，≥600 的文件数
与低水位基线 `tools/ci/baseline/gate-lines-baseline.txt` 比对（**只降不升**：新增超标
即红；拆完同批下调，不许直接上调）；基线文件缺/无整数即红（fail-closed）。
立规时存量 **58** 个（最大三件：`rr-registry/tests/contract.rs` 2632、
`rr3-interact/src/panel/server.rs` 2079、`rrd/src/registry/tests.rs` 1917），
按模块分批拆分并逐批下调基线。可数 step 17→**18** 行。

**2026-11-13 首批三拆（基线 58→55）**：rrd registry 测试 1917→`tests/` 八件
（最大 421）；rr-registry 契约对拍 2632→`tests/contract.rs` 夹具根 435 +
`tests/contract/` 十一段（最大 326）；rr3-interact 面板服务端 2079→`server.rs` 550 +
`server/{ctl,selftest,serve}.rs` + `server/tests/` 四段（最大 424）。两条拆分纪律：
①集成测试目标（`tests/*.rs`）自身是 crate root，子模块按 `tests/` 解析而非
`tests/<目标名>/`，必须 `#[path = "..."]` 显式指路；②跨模块可达的夹具按最小面
放开到 `pub(super)`，不放宽到 `pub`。

**2026-11-14 第二批（基线 55→35）**：19 件测试体抽取（rr-hal esp32_uart、rr-ipc
protocol/header、rr-mech control、rr-registry atom/depends、rrd assembly/cli/manifest/
topics_load/wiring、rr3-act intent、rr3-interact boot、rr3-power dispatch、rr3-sense boot、
pi-periph-base ports、rrctl commands、sim main、rr3-brain boot 夹具/用例）+ 生产面三件
（sim drill 定位段、rr3-brain boot 夹具、rrd domain 写口四分 1412→82）。第三条纪律：
③测试体搬目录后 `include_str!`/`include_bytes!` 的相对基准跟着文件走——仓内夹具一律
`concat!(env!("CARGO_MANIFEST_DIR"), ...)` 定位，不再写 `../` 层级。

**2026-11-14 第三批（基线 35→28）**：rr-registry platform 1157→193（head/ctl/caps/
conf/cons/view）、assemble 1133→288（item/empty/asm+tests）、rr-ipc UART 1192→528
（excl/term+tests）、rrd 测试支撑 1193→29（inst/frames/client/fixture）、rr3-brain
dispatch 1714→546（wake/turn/arms/tts + tests 四段）、rr3-pack 1513→158
（args/copy/stage/deb + tests）、esp32-periph-base httpd 1112→386（serve+tests）。
另两条拆分纪律：④结构体被兄弟子模块读字段时，字段同样要 `pub(super)`（只开结构体
不开字段 = E0616）；⑤搬走测试后，"按文件名扫机制实体"的门（⑧-a 两腿计数、⑨-z
打包面共读）扫描面必须同步按内容扩到 `<实体>/tests/` 与 `src/` 全树，否则静默漏扫。

**2026-11-16 对表（⑥b 漂移门扩面到部署面三类，真板红绿对照）**：`verify-board-config.sh`
的扫描面此前只有 `modules/*/config*.toml` 七件，而当天两次真实事故都落在漏面上——
① 装配单 `profiles/*.toml` 的 `alsa_device` 指向未接入的 USB 声卡，`rr3-interact`
连崩 5 次后运行时降级；② systemd unit 的 `ExecStartPre`（#24 音量下限）从未铺到
板上，电平仍 38%。两条都不是"代码写错"，是**部署面真值与仓内真值分了叉而无人看得见**。
本批把比对面数据化为 `rr3_deploy_manifest`（模块 config＋装配单＋unit 三类），口径分
`full` 与 `unit`（后者忽略 `ExecStart=` 行——rr3-pack 出包按 `--profile` 改写该行，
逐字节比必然假红）。self-test 三例扩到六例（三类齐全／unit 口径／config 改字节／
装配单漂移／unit 非 ExecStart 行漂移／unit 缺件）。真板当场取到对照：比对 16 件 →
`esp32s3-mini.toml`（UART2 41/42 真板订正注释只进仓）与 `rrd.service`（批N38 注释未铺）
两件 DRIFT → 按仓内真值铺板 → 16/16 PASS。另修一处门自身的静默漏面：逐行清单经
here-doc 喂给 `while read` 时，`ssh` 默认吃 stdin，实测"比对 16 件"变成"比对 2 件"
——统一 `ssh -n`。

**2026-11-17 A1 首片（ADR-0026：OTA 制品签名/验签）**：`rr-ota::sign`
新增 9 条（crate 26→35）：严格行式 manifest 渲染/解析、Ed25519 签名/严格验签、
key_id 派生、镜像摘要与字节数核对、目标/降级策略；覆盖篡改 manifest/image、
错公钥/错 key_id、坏十六进制、字段缺失/重复/未知/顺序/CRLF/非 ASCII、路径穿越、
超大 manifest、版本文法与降级开关。`rr3-ota-ctl` 独立工程 6→11 条，新增
keypub/manifest/sign/verify 端到端、错私钥 key_id、0600 权限、超大输入与
降级/目标策略；`--allow-downgrade` 缺 `--current-version` 归用法错。
①b 本轮实测 **767 passed**（macOS x86_64，2026-11-17；本行是唯一计数处）。
度量：重复率 0.56%、CC 均值 2.49 / 最大 13、生产 panic 73、长名 0；
`check.sh` 全绿。结构：`sign.rs` 432 + `sign/tests.rs` 262；CLI `args` 64 /
`bootconf_cmd` 192 / `main` 518 / `sign_cmd` 305（均 <600）。依赖：
`ed25519-dalek 2.2` + `sha2 0.10`（纯 Rust，锁文件同批；供应链门由 CI
`supply-audit --strict` 执法）。首跑红一处：`rr3_manifest_parse` CC 27
（提交 b75a031），已按"字节形制 / 字段值域 / 数值摘要"三段拆分修复（b5056d8）；
远端最新 run 回执以 CI 结论为准。

**2026-11-17 A1b/A1c（OTA CLI 进发布面 + 签名演练）**：`rr3-ota-ctl` 进
`cross-artifacts`（八件产物）、`distro-smoke`、`pack-arm64`、`pack-bundle`、
`stage-image`、Buildroot `BINARIES_DIR` 注入与 ghcr `publish` 构建；
`rr3-pack` 入口清单收敛为 `RR3_PAYLOAD_BINS` 单常量，缺件逐件点名，
测试 13 条。新增 `tools/ci/platform/ota-sign-smoke.sh` 与 `integration.yml` 的
`ota-sign` job：临时测试种子走 keypub→manifest→sign→verify，正例 + 镜像篡改/
清单篡改/错公钥/未允许降级四负例，真实私钥不进 CI。aarch64-unknown-linux-musl
静态交叉实测：`ELF 64-bit ARM aarch64, statically linked`。`check.sh` 全绿；
度量维持 0.56% / 2.49 / 13 / 73 / 0。

**2026-11-17 A1d（验签后 staging 安装原语）**：`rr3-ota-ctl install` 在
`verify` 通过后才复制到目标同目录临时件，摘要复核 + `fsync` + `rename`；
目标必须是普通文件（块设备/槽位写入仍归 M6），缺 `--confirm` 用法拒，
源/目标同路径与目录目标拒，篡改镜像在落目标前拒。`rr3-ota-ctl` 测试
11→12；度量重复率 0.62%、CC 2.49/13、panic 73、长名 0。

**2026-11-17 A1e（公钥目录按 key_id 查键）**：`verify/install` 支持
`--pub-dir`，按 manifest 的 `key_id` 查 `<key_id>.pub`；`--pub` 与 `--pub-dir`
二选一，缺键点名实际路径。测试 12→13；度量重复率 0.64%、CC 2.49/13、
panic 73、长名 0。

**2026-11-17 P1 连接鲁棒性（满员拒连退避与日志节流）**：事实——
`rr3_daemon_accept` 在并发槽满时 `rr3_conn_acquire` 拒连后直接 `continue`，
下一拍立刻再 accept，且每次拒连同步 `rr3_log` 一行；重连风暴下这是
"accept→drop→写日志"的忙循环（尝试率无上界，rrd 日志同步 append 会被写穿）。
判据——并发上限 64 是保护而非错误路径：满员时本就没有可服务的连接，退避
不损失可用性；拒连是运维事实，首条必须可见、规模不能报小。处置——accept
结果分 `Conn/Full/Retry` 三态；`Full` 走 50ms 退避（尝试率封顶 20 次/秒），
日志用 `Rr3RefuseGate` 时间窗节流：首条即时、5s 窗口内只计数、窗口到点汇总
"另有 N 次"，窗口状态机有单测；拒连同时累加 rrd 自有计数键 `conn_refused`
（`ctl.counters` 可见）——节流只压打印，不压真实次数。验证——
`cargo test -p rrd --lib` 308 passed
（新增 `refuse_gate_logs_first_then_summarizes`）；①b **768 passed**；度量
重复率 0.64%、CC 2.49/13、panic 73、长名 0；`check.sh` 全绿。风险边界——
退避期间释放的连接最多多等 50ms（控制面重连窗口以秒计，不敏感）；本件不
承诺"满员时优先淘汰旧连接"。

**2026-11-17 A2（低电关断语义裁定 + 装载门）**：[ADR-0027](adr/ADR-0027-低电关断语义与断电轨权限.md)
裁定两条电压线的边界（cutoff 只在 low 段内评估，且必须严格 < low）、去抖
（连续 3 拍且满 3×sample_period，任一拍回升即清零）、闭锁（一次生效，仅冷
启动解除）、执行相序（禁动 → 有序停机 → 切 T1/T2，切轨失败 1Hz 最多 3 次）、
权限（判定与执行都只归 `rr3-power`；rrd/面板/任何 `rr3.ctl.*` 写口不得发起）
与执行面开启四前置（真实 `rr3_switch` 实现 / 装配 `switchable·armed` 对账 /
台架 20 次关断循环 / 串口与 rrd 日志双路留痕对齐）。本版执行面保持关闭，
MC-8① 判 (c) 的"声明未生效"文案逐字不动。同批落装载门 `cutoff < low`
（相等/倒挂拒 boot，文案指向 ADR）；去抖状态机等真实执行器到位再写——今天
没有读者，先写即死代码。验证：`cargo test -p rr3-power` **21 passed**
（新增 `battery_cutoff_must_stay_below_low`）；装载门文案含"阈值倒挂"与
"ADR-0027"；度量重复率 0.64%、CC 2.49/13、panic 73、长名 0；`check.sh` 全绿。

**2026-11-17 A1d 补（OTA 落盘目录项耐久 + 写路径归一）**：事实——`install` 与
`rr3_write_atomic` 只 fsync 临时文件再 rename；文件内容耐久、目录项不耐久，
断电窗口里可能丢名字或留半状态。判据——ADR-0026 的"fsync → rename"只覆盖
内容半边；且 `rr_ipc::persist` 头注明令"出现第二条写路径即红"，OTA CLI 自持
tmp+rename 正是第二条。处置——`rr_ipc::persist` 把目录 fsync 提为公开机制
`rr3_sync_dir -> io::Result`（策略归调用方：通用落盘忽略错误，OTA staging 严格
报错）；`rr3_write_atomic` 改走 `rr3_atomic_store` 单一写序，install 尾部用
`rr3_sync_dir` 选严格策略。验证——`rr-ipc` 测试 90→**91**（目录 fsync 正/负
两例），`rr3-ota-ctl` 测试 13→**14**（原子写回读 + 临时件零残留；用例独立成
`sign_cmd/tests.rs`，避免顶穿 `main.rs` 599 行）；度量重复率 0.64%、CC
2.49/13、panic 73、长名 0；`check.sh` 全绿。

**2026-11-17 文档对表（status.md 方法数与 OTA 行订正）**：事实——status.md
是唯一实况快照，但三处与 HEAD 不一致：①RPC 行写"十二方法（模块四 + 控制八）"，
[rpc.rs](../../base/runtime/rrd/src/rpc.rs) 实为 **13/9**（漏 `rr3.ctl.domain.set`）；
②`rr3-ctl` 行写"rrd ctl 六方法 / rrctl 逐一同名对齐"，实际 ctl 9 方法、
rrctl 仅覆盖其中 6 个；③A/B OTA 行写"CLI 三子命令 / 落盘 fsync 加固未做"，
实际 9 子命令且 bootconf 已走 `rr_ipc::persist`（文件 + 父目录 fsync）。处置——
逐行订正并注明判据来源；不重写快照锚点（锚点是"逐条读代码"的起点，增量按行
维护）。验证——`check.sh` ⑤ 文档门通过；方法数以 `RR3_RPC_NAMES`（13 项）与
`RR3_CTL_METHODS`（9 项）为唯一真值，行内计数与它逐项对齐。

**2026-11-17 文档导航（docs/README 的 ADR 范围）**：事实——[docs/README](../README.md)
的 `process/adr/` 行写"ADR-0001 ~ ADR-0022"，实际目录内 27 份（ADR-0023…0027
已落树且被代码/设计文档引用）。处置——范围改到 **ADR-0027** 并补五份标题/日期；
**不新增 ADR 总表文件**——README 已是导航真值，另立总表等于造第二真值点。
验证——`check.sh` ⑤ 文档门通过；五份文件名与目录逐字一致。

**2026-11-17 复杂度回归（`rr3_cmd_manifest` 13 → 拆分）**：事实——本轮收口
时 CC 均值 2.49，峰值 13 在 rr-hal，但 OTA CLI 的 `rr3_cmd_manifest` 已到 13
（与峰值并列），其判定点大半来自 8 个重复的 `rr3_val_arg + map_err` 参数样板。
处置——参数面抽成 `Rr3ManifestArgs` / `rr3_manifest_args`，镜像文件名抽
`rr3_image_name`，主流程只留生成 / 复核 / 落盘；CLI 行为与文案逐字不变。
验证——`rr3-ota-ctl` 14 tests、clippy/fmt 全绿；CC 峰值仍 13（仅 rr-hal），
均值 2.49、重复率 0.64%、panic 73、长名 0；`check.sh` 全绿。

**2026-11-17 流水线覆盖（rr-hal alsa 特性编译门）**：事实——`alsa` 是 rr-hal
的**非默认特性**；`check.sh`（默认特性）、host-macos、cross-artifacts 三腿都
不编译 `linux.rs` 与 backend 的 alsa 分支——这半边代码只在板上第一次过编译器，
"编得过、跑不了"之前还有更早的一关没守。处置——[gate.yml](../../.github/workflows/gate.yml)
增 `alsa` job：apt 装 `libasound2-dev` + `pkg-config`，
`cargo check --all-targets -p rr-hal --features alsa`；**只编译不打开设备**
（运行期行为归板上批），失败压成单行注解。验证——job 与 conventions/lint 并行，
不改 check.sh 的本地口径。**首跑红一处**：job 在仓根直接 `cargo check`，报
`could not find Cargo.toml`——补 `--manifest-path base/runtime/Cargo.toml` 后
复跑；根因是工作目录与 workspace 布局，不是特性代码。**同日追加**：同 job 增
`cargo test -p rr-hal --features alsa --lib`（选择器构造不触设备，CI 无设备可跑）
——编译门升级为"编译 + 单测"门，alsa 分支不再只过编译器。

**2026-11-17 复杂度峰值收口（`rr3_audio_source_cmd` 13 → 12）**：事实——全仓
CC 峰值 13 只剩 rr-hal 的 `rr3_audio_source_cmd`（六个后端臂 + 两处重复的
cfg 守卫挤在一个函数里）。处置——按后端拆成 `rr3_src_linux` / `rr3_src_mac` /
`rr3_src_esp32` / `rr3_src_sim` / `rr3_src_na`，主函数只留一次 match；linux/mac
的"特性/平台在不在"从双守卫臂收敛为 `Option` + 占位回退（行为等价：缺 alsa /
缺生效绑定 / 非 macOS → `Unsupported` 占位）。验证——rr-hal lib **73 passed
/ 1 ignored**；`alsa` job 在基线树首跑 green 证明该面可编，重构后同 job 再复核
（本推送的 gate 运行）；CC 均值 2.49 / 峰值 **12**，基线 `cc_max_fn` 13→**12**
（只降不升）；`check.sh` 全绿。

**2026-11-17 长阻塞静态门（生产长睡字面量基线 0）**：事实——"长阻塞主循环"
此前只有评审、没有机器判据；模块/守护面一旦有人写 `sleep(2s)`，检查全绿而
看门狗窗口可能小于它。处置——[metrics.py](../../tools/ci/audit/metrics.py) 增"生产长睡字面量"
项：只扫 `modules/rr3-*` 与 `base/runtime/rrd/src`（排除 bin），认
`sleep(Duration::from_millis|from_secs(数字))`（含 `std::time::Duration` 全限定）
且 ≥1000ms，基线 `long_sleep_max = 0`；`check.sh` ⑫ 标题同步。**负例实测**——
临时向 rr3-power 生产面注入 `sleep(from_secs(2))`，metrics 退 1 并点名
`('modules/rr3-power/src/boot.rs', 2000)`；撤回后复绿。局限如实写：动态等待、
阻塞 IO、锁内长临界区不在扫面，归评审与运行时观测。

**2026-11-17 验签链单点化 + CC 峰值 11**：事实——`verify` 与 `install` 各写
一份"解析 → 取公钥 → 验签 → 目标/降级 → 镜像摘要"的五步链（ADR-0026 要单点），
`rr3_verify_args` / `rr3_cmd_install` / `rr3_manifest_parse` 三处 CC 12。处置——
抽 `rr3_check_pkg`（verify/install 共用**唯一验签链**；install 只在写前复解析
原文做摘要判据，不二次验签）、`rr3_key_src` / `rr3_downgrade_ok`、`rr3_meta_fields`。
验证——rr-ota **35**、rr3-ota-ctl **14** tests；重复率 **0.56%**、CC 均值 2.49 /
峰值 **11**，基线 `cc_max_fn` 12→**11**（只降不升）；`check.sh` 全绿。

**2026-11-17 buildroot U-Boot 根因（FORMAT_ITB ↔ binman 无该目标）**：证据——
U-Boot v2024.10 的 `u-boot.itb` 目标包在 `ifdef U_BOOT_ITS` 里，只有
`CONFIG_SPL_FIT_SOURCE` / `CONFIG_USE_SPL_FIT_GENERATOR` 才定义；rk3566 的
defconfig 走 binman（`CONFIG_SPL_LOAD_FIT`/`CONFIG_FIT`），该目标不存在，而
Buildroot 的 `FORMAT_ITB` 把它加进 `UBOOT_MAKE_TARGET` → 精确复现
`No rule to make target 'u-boot.itb'`。binman 的
`arch/arm/dts/rockchip-u-boot.dtsi` 把 `idbloader.img` 与 `u-boot.itb` 写成
子产物 `filename`，`make all` 会生成这两个文件。处置——defconfig 去掉
`FORMAT_ITB`，改 `FORMAT_CUSTOM_NAME="idbloader.img u-boot.itb"`（只加拷贝
清单、不加 make 目标），补 `NEEDS_PYLIBFDT`/`NEEDS_PYELFTOOLS`；失败注解补
dtc 版本（scripts_dtc 第一现场）。验证——YAML/`check.sh` 绿；全量回执由本次
`[buildroot-full]` 触发。**同日第三拍回执**：`FORMAT_ITB` 错配已消除
（`dtc=Version: DTC 1.7.0`，scripts_dtc 不再是失败点），新失败是
`tools/mkeficapsule.c:21: gnutls/gnutls.h: No such file or directory`——defconfig
打开了 `CONFIG_TOOLS_MKEFICAPSULE` 但宿主缺 gnutls。处置——补
`BR2_TARGET_UBOOT_NEEDS_GNUTLS=y`（Buildroot 该选项正是为 MKEFICAPSULE 设）。
Makeinfo 的 too old 警告只影响文档，不是失败点。

**2026-11-17 status 里程碑对表（M1 Buildroot 行）**：事实——M1 行只写"全量
make 构建缺"，未记 2026-11-17 两次 CI 尝试与失败阶段，读者会误以为从未试过。
处置——追加对表段：custom git 源、两次红在 uboot 阶段、诊断拍/失败日志
artifact/镜像名解析/六分区断言与产物上传已就位、**全量绿回执仍未取得**。
验证——`check.sh` ⑤ 文档门通过。

**2026-11-17 publish 合并标签冒烟**：事实——`merge` job 只做
`imagetools inspect` 的架构集合判等，没有拉取合并标签本体；两只 per-arch 腿的
冒烟证明不了"合并清单指向的层都在"。处置——merge 末尾调
`image-smoke.sh file "${IMAGE}:${tag}"`（拉取发布标签，验装机树三件 + rrd 用法），
与 build 腿同一脚本单点。验证——脚本与 build 腿同源；**远端回执等下次
tag/main 的 publish**，本机不冒充。

**2026-11-17 status 口径补充（锚点与逐行增量）**：事实——表头"当前 HEAD =
`e22f20a`"与后来按行补的 A1/A2/alsa 等行并存，读者可能把"某行补过"误读成
"整表已按最新 HEAD 实测"。处置——表头加口径段：锚点之后按行增量维护、**未注记
的行**仍只指锚点、全表重读只在收口做。验证——`check.sh` ⑤ 文档门通过。

**2026-11-17 CI 漂红审计（e2e_profile 回填用例偶发红）**：事实——
platforms/host-macos 在 `f974e4d` 红一次（`rrd_reboot_recovers_previous_run_lines_from_disk`），
本地重复 5/5 通过，且本次改动不涉 rrd。可疑点是环容量 64 与慢 runner 的挤占
竞态（用例靠"回填行仍在环里"取证），加上 10s/20s 固定等待。处置——环容量
64→**256**（断言一条不改）、两处等待 20/10s→**30s**；这是去竞态，不是放宽判据。
验证——本地重复 3/3 通过；远端以 platforms/host-macos 复跑回执为准。

**2026-11-17 实时性观测（sense 节拍抖动 `max_jitter_ms`）**：事实——"实时性"
此前只有静态长睡门（字面量 sleep），没有运行期节拍指标；采集超时只能靠
`pcm_frames` 零增长间接猜。处置——sense 每拍记"上一拍开始 → 本拍开始"的实际
周期，超出标称 `frame_ms` 的毫秒数取历史最大；键 `max_jitter_ms` 用**增量**
上报（首达 7 加 7、再到 12 加 5），rrd 聚合值即全局峰值；纯函数
`rr3_jitter_step` 有单测，README 计数表同步。验证——rr3-sense **41 tests**；
`check.sh` 全绿（度量按当日基线）；板上真实抖动值随证据批，不在本机造数。

**2026-11-17 buildroot 全量镜像：源修正 + 产物化 + 分区断言**：事实——①image
job 从未成功：`8ae6fd1` 在全量 make 的 uboot 阶段因 ftp.denx.de 443 超时失败，
而 `cac2d8b` 的 image job 是 **skipped**（此前"全量构建已成功"是误读）；②断言
步硬写 `sdcard.img`，genimage 实际产出 `rock3-zero3.img`，即使 make 成功也会
假红；③产物不上传，OTA 签名链没有真实载体。处置——defconfig 的 U-Boot 源改
**custom git**（GitHub tag `v2024.10`，去掉 denx tarball 依赖），删 CI 的预塞
tarball 步；CI 从 `genimage.cfg` 解析镜像名（不写第二份字面量）、加 GPT 分区
断言（`boot/rr_a/rr_b/rr_factory/rr_staging/data`）、生成 sha256 清单并上传
artifact（14 天）。验证——YAML/脚本语法门通过、镜像名解析实测
`rock3-zero3.img`；**全量构建回执由本次 `[buildroot-full]` 提交触发**（小时级，
结论以 buildroot workflow 为准）。**同日第二次全量（`0878f07`）仍红**：custom git
已生效（阶段名 `uboot v2024.10`），失败为 `No rule to make target 'u-boot.itb'`
＋ `scripts_dtc Error 1`；旧注解被 `pyerrors.h` 这类含 error 的路径污染，根因
读不出来。追加处置——defconfig 与全量 make 拆步，新增 `make uboot` 预构建拍
（失败打印干净末段），失败时上传 `br-uboot.log`/`br-build.log` artifact；下一拍
据此定位 U-Boot 根因。

**2026-11-17 status 里程碑对表（M6 OTA 行）**：事实——M6 行仍写"U-Boot env
写侧未实装 / `rr3-ota-ctl` 三子命令"，与 HEAD 不符：`rr-ota/src/bootenv.rs`
已实装（0.0.8 P1 审计件五），CLI 已九子命令，ADR-0026 签名/验签链已落地。
处置——在 M6 行追加"2026-11-17 对表"段，历史文字不改；里程碑状态仍为
**未收口**（块设备探测、槽位写入、板上回滚实录未做）。验证——`check.sh` ⑤
文档门通过（新增两条 ADR 相对链接可解析）。

**2026-11-17 发布制品签名与托管裁定（ADR-0028）**：事实——Buildroot 全量产物的唯一载体
是 Actions artifact（保留 14 天、需登录）；`rr_a.ext4` 无签字，M6 写槽链没有可验的真实
制品；签名若在分支触发，等于把信任根暴露给任何能改分支的人。处置——立
[ADR-0028](adr/ADR-0028-OTA发布制品签名与ghcr托管.md)：仅 `v*` 标签签名、签名对象是
`rr_a.ext4` 的规范副本、Secret `RR3_OTA_SIGNING_KEY` 缺失即失败、先验后发、ghcr 两个
不可变 OCI 制品（`rockrobot3-ota` / `rockrobot3-buildroot`），并给施工清单与负例口径。
验证——⑤ 文档门通过（新增 ADR 链接可解析）；接线回执待 Buildroot 全量绿后同批取得。
同批修正 `docs/README.md` 库地图的 ADR 行：它仍写 ADR-0001~0022（首表已到 0027），是同一
事实的第二份副本；改为指向首表的稳定描述，不再抄编号清单。

**2026-11-17 status 里程碑对表（M3/M5）**：事实——M3 行未记 `rr-hal` 的 `alsa` 非默认特性
已进 CI 专属门（`gate.yml` 的 `alsa` job：全目标编译 + 单测），也未记 sense 的节拍抖动峰值
`max_jitter_ms`；M5 行未指向 ADR-0027 的低电关断裁定。处置——两行各追加"2026-11-17 对表"
段，历史文字不改；M3 的"真麦/板上 ALSA 实录"与 M5 的"真硬件动作证据"敞口维持，不因
ADR 或 CI 门存在而改判。验证——`check.sh` ⑤ 文档门通过（ADR-0027 相对链接可解析）。

**2026-11-17 Buildroot U-Boot 根因二（binman Error 103）**：证据——第五拍（run
`43979037538`）的 U-Boot 预构建失败，注解 `br-uboot` 给出 `dtc=1.7.0` 与
`.binman_stamp Error 103`；其余行是 host-gnutls 构建噪音（`libhogweed` 的 ignored
Error 与 makeinfo 版本提示），不是失败点。根因——RK3566 的 DDR 初始化 TPL 与 BL31
不在 U-Boot 源码内，defconfig 未启用 `rockchip-rkbin`，binman 组 `idbloader.img`
与 `u-boot.itb` 时找不到必需 blob。处置——补
`BR2_PACKAGE_ROCKCHIP_RKBIN`（TPL=`bin/rk35/rk3566_ddr_1056MHz_v1.18.bin`、
BL31=`bin/rk35/rk3568_bl31_v1.43.elf`，文件名按 Buildroot 2024.02.11 钉住的 rkbin
提交 `b4558da` 实查）与 `BR2_TARGET_UBOOT_NEEDS_ROCKCHIP_RKBIN=y`；不走源码编译
ATF 的第二条路（rkbin 已含 BL31，双来源会造第二个真值）。验证——下一拍全量构建的
U-Boot 预构建步与 `.binman_stamp` 回执。

**2026-11-17 出厂轨 fstab 补 sysfs/devtmpfs**：事实——post-build.sh 覆写的 `/etc/fstab`
只有 proc/devpts/shm/tmp/var-run/var-log 与 data/staging，没有 sysfs 与 devtmpfs；
本轨是 BusyBox init，唯一的伪文件系统挂载器是 inittab 的 `mount -a`（读本文件）——
缺 sysfs 时 `/sys` 为空（模块拿不到设备树/class 节点），devtmpfs 若内核未自动挂载则
`/dev` 连基础节点都没有。处置——补 `sysfs /sys` 与 `devtmpfs /dev mode=0755,nosuid`
两行，与 Buildroot 默认骨架同口径。验证——`sh -n` 通过；镜像内 `/etc/fstab` 随下一拍
全量构建回执复核（板上启动实录仍归 M1 硬件到位后）。

**2026-11-17 OTA bundle 机制单点 + CI 门（ADR-0028 施工前半）**：事实——ADR-0028 的
组包/签名/复验顺序若写在 workflow 内联 shell，只能在打标签那天第一次真跑；
`rr3-ota-ctl` 的签名链已有单测，但"规范名 → manifest → 签名 → 公钥复验 → 摘要"这条
发布顺序没有可本地复跑的载体。处置——新增
[`tools/deploy/ota-bundle.sh`](../../tools/deploy/ota-bundle.sh)（输出目录非空即拒、
版本只认 `major.minor.patch`、私钥不复制不打印；产物含四件 + `checksums.txt` +
`bundle.txt`），`gate.yml` 增 `ota-bundle` job：1MiB 随机件走正例，并断三条负例
（篡改镜像 / 非空输出目录 / 坏版本）。验证——本地正例六件齐、摘要四行；
篡改镜像、非空输出、坏版本、0644 私钥四类负例均按预期非 0；`bash -n`、YAML 解析、
`check.sh` 全量通过；CI 回执待推送后核对。

**2026-11-17 Buildroot 全量 make 根因三（rootfs.ext2 容量）**：证据——run
`43984190313` 的 `br-build` 注解：阶段 `post-build → rootfs.ext2`，
`make: *** [fs/ext2/ext2.mk:66: /tmp/br-out/images/rootfs.ext2] Error 1`。
Buildroot 2024.02 的 `BR2_TARGET_ROOTFS_EXT2_SIZE` 缺省 `60M`，而本档 target 树含
glibc + Python3 + ALSA utils，60M 放不下。处置——defconfig 显式
`BR2_TARGET_ROOTFS_EXT2_SIZE="512M"`；该值只约束中间产物 `rootfs.ext2`，
最终 `rr_a`/`rr_b`/`rr_factory` 由 genimage 从同一 target 目录按各自
`@XXX_SIZE@` 生成，不受此值影响。同批改失败诊断：U-Boot 预构建与全量 make 的
注解过滤 `Error 1 (ignored)` 与 makeinfo 噪音，全量 make 注解收
"Maybe you need to increase…"，失败时打印末 40 行。验证——下一拍全量构建回执。

**2026-11-17 Buildroot 下载缓存（DL_DIR）**：事实——全量构建每拍都从零下载包源码与
custom git（U-Boot/rkbin），是小时级耗时与网络抖动的主要来源；缓存 `O=` 输出树则会把
上一拍失败的残骸带进下一拍（假绿来源）。处置——`buildroot.yml` 增 `actions/cache@v4`，
**只缓存 `/tmp/buildroot/dl`**，key = Buildroot 版本 + defconfig 哈希，
`restore-keys` 允许同版本配置变更后复用仍有效的包源码（Buildroot 自带包 hash/revision
校验）。验证——下一拍全量构建的缓存 step：首拍 miss 属预期并在末尾保存，后续拍命中。

**2026-11-17 Buildroot 全量 make 根因四（genimage 缺宿主 vfat 工具）**：证据——第 6 拍
（run `43995585851`）全量 make 推进到 `post-image → genimage`，失败行
`make: *** [Makefile:820: target-post-image] Error 1`；注解里 rootfs.ext2 已按 512M
生成，说明上一处容量修复已生效，失败点在 genimage。根因——Buildroot 的
host-genimage 只依赖 host-libconfuse，不带 `mkfs.vfat`（dosfstools）与 `mcopy`
（mtools）；defconfig 未显式选这两个 host 包，genimage 在 boot.vfat 分区上失败且
不点名缺哪个工具。处置——补 `BR2_PACKAGE_HOST_DOSFSTOOLS=y` 与
`BR2_PACKAGE_HOST_MTOOLS=y`（e2fsprogs 已由 ext2 根文件系统自动 select）；同批把
`br-build` 注解提取改为大小写不敏感、收 genimage/mkfs/mcopy/no space/not found，
并把末 12 行塞进注解（`target-post-image` 的真因只在末几行）。验证——下一拍全量
构建回执。

**2026-11-17 Buildroot 全量 make 根因五（post-image 参数符号名）**：证据——第 7 拍
（run `44001671811`）post-image 阶段 `genimage.sh` 报 `Error: Missing argument` 并打印
usage；失败注解的末 12 行点明了 `post-image.sh` 已注入配置、`genimage.sh` 却没收到 `-c`。
根因——defconfig 写的是 `BR2_ROOTFS_POST_IMAGE_SCRIPT_ARGS`，但 Buildroot 2024.02.11 的
`system/Config.in` 只定义 `BR2_ROOTFS_POST_SCRIPT_ARGS`，`Makefile` 的
`target-post-image` 规则也只读后者；**kconfig 对未知符号静默忽略**，拼错的符号不会
红，只会让参数凭空消失。处置——改用真符号 `BR2_ROOTFS_POST_SCRIPT_ARGS`，并在
defconfig 就地注明"改名先 grep Buildroot 的 system/Config.in"。验证——下一拍全量构建。

**2026-11-17 Buildroot 全量 make 根因六（genimage `label` 位置）**：证据——第 8 拍
（run `44012093158`）genimage 报 `genimage-rock3.cfg:26: no such option 'label'`。
根因——`label` 是 genimage **文件系统子段**的选项（`vfat_opts` / `ext_opts`），
`image` 公共段（`image_common_opts`）不认；`boot.vfat` 的 `label = "boot"` 原写在
`vfat {}` 块外。处置——把 label 移入 `vfat {}`；四个 ext4 段的 label 本就在
`ext4 {}` 内，不动。验证——下一拍全量构建。

**2026-11-17 genimage 空分区与 mke2fs 审计（第 8 拍连带）**：事实——`rr_staging` /
`data` 两个 ext4 只给了 `label` 与 `size`，没有 `mountpoint` 也没有 `empty`；
genimage 的 `mountpath()` 在无 mountpoint 且未置 empty 时回落到根挂载点，会把整个
rootpath 灌进 256M 暂存区（必失败）与 1G 数据区（内容语义错误）。另，genimage 的
ext 生成默认走 `genext2fs`，而 Buildroot 的 host-genimage 不带它；`mke2fs` 由
`BR2_TARGET_ROOTFS_EXT2` 自动 select 的 host-e2fsprogs 提供。处置——两区加
`empty = true`；五个 ext4 段加 `use-mke2fs = true`（走已选中的 mke2fs，不引第二套
文件系统生成器）。验证——下一拍全量构建。

**2026-11-17 Buildroot 全量镜像绿回执（第 10 拍）**：证据——run `44018120685`
（HEAD `66bbec1`）的 image job **全步骤 success**：释放磁盘、宿主依赖、取 Buildroot
2024.02.11、DL 缓存、交叉产物注入、BINARIES_DIR 注入、defconfig、U-Boot 预构建、
全量 make、镜像名解析（`rock3-zero3.img`）、六分区断言
（`boot/rr_a/rr_b/rr_factory/rr_staging/data`）、摘要清单、artifact 上传
（`rock3-zero3-image`，58,128,908 B）。这是 M1 构建半边的**首个全量绿回执**；
串口出 shell 与 maskrom 实烧仍属板上收口。修复链（按拍）：rkbin TPL/BL31 →
rootfs.ext2 512M → 宿主 mkfs.vfat/mcopy → `BR2_ROOTFS_POST_SCRIPT_ARGS` →
genimage `label` 位置 → 空分区 `empty` / `use-mke2fs`。

**2026-11-17 镜像内容只读断言（M1 构建半边补真值）**：事实——第 10 拍绿回执只证明
"genimage 跑通 + 六分区名在 GPT"，不证明分区内容：ext4 可能坏、标签可能错槽、
`/etc/fstab` 可能缺 sysfs/devtmpfs、`u-boot.itb` 可能没有 BL31（能烧不能启）。
处置——image job 新增只读断言步：五个 ext4 `e2fsck -fn`、`dumpe2fs` 标签逐槽对表
（`data` 期望 `rr_data`）、`debugfs` 读 `rr_a.ext4` 的 `/etc/fstab` 断言
sysfs/devtmpfs/rr_data/rr_staging、`fdtdump` 断言 `atf-bl31`、idbloader 体量下限。
验证——下一次全量构建回执；失败即红，不以"genimage 成功"代替内容真值。

**2026-11-17 空 rootfs 根因（genimage `--rootpath` 空旷）**：证据——第 11 拍
（run `44026993488`）内容断言 `br-fstab` 报 `rr_a.ext4` 的 `/etc/fstab` 缺 sysfs；
同拍 e2fsck 与 dumpe2fs 标签**全过**。根因——Buildroot 的 `genimage.sh` 给 genimage
传的 `--rootpath` 是**空临时目录**（避免整树复制），ext4 段只写
`mountpoint = "/"` 时 genimage 从空 rootpath 生成**空文件系统**：标签对、e2fsck 过、
`/etc/fstab` 与 rrd 全都不在。这就是"绿但不对"的典型形态——genimage 成功 + 标签过
不等于 rootfs 装进去了。处置——`rr_a`/`rr_b`/`rr_factory` 改用
`srcpath = "@TARGET_DIR@"`，由 `post-image.sh` 注入真实 target 目录（路径白名单校验，
残留占位符全量点名）；`rr_staging`/`data` 保持 `empty = true`。内容断言补
`/usr/bin/rrd` 与 `/etc/init.d/S99rrd` 存在性（`debugfs stat`）与 debugfs 工具在位检查。
验证——下一拍内容断言全绿。

**2026-11-17 Buildroot 内容回执（第 12 拍）**：证据——run `44035094772`
（HEAD `621c26f`）的 image job 全步骤 success；内容断言全过：五个 ext4 `e2fsck`、
`dumpe2fs` 标签逐槽（`data`→`rr_data`）、`/usr/bin/rrd` 与 `/etc/init.d/S99rrd`
在册、`/etc/fstab` 四键（sysfs/devtmpfs/`LABEL=rr_data`/`LABEL=rr_staging`）、
`u-boot.itb` 含 `atf-bl31`、idbloader 体量下限；artifact `rock3-zero3-image`
265,842,537 B。第 10 拍的 58,128,908 B 实为**空 rootfs**（e2fsck 与标签能过、内容
为空），第 11 拍内容断言将其拦下——这是"绿但不对"的实证；内容断言自此是 M1
构建半边的必过项，不再以"genimage 成功 + 分区名在 GPT"作为收口证据。

**2026-11-17 ADR-0028 施工后半（release job）**：事实——签名机制与 CI 门已落
（`ota-bundle.sh` + gate `ota-bundle`），但发布面从未接线：tag 不触发 buildroot，
镜像不签、不推 ghcr。处置——`buildroot.yml` 的 `on.push` 增 `tags: ["v*"]`；
image job 的 `if` 纳入 tag 且对 `head_commit` 做空值兜底；新增 `release` job
（`needs: image`）：download-artifact → `checksums.txt` 复核 → 宿主编译
`rr3-ota-ctl` → Secret 落 0600 临时件（缺/坏即失败）→ `ota-bundle.sh` →
`oras` 不可变标签检查（只认 404）→ 推 `rockrobot3-ota` / `rockrobot3-buildroot`
两个 OCI 制品 → 回读 → job summary；私钥在 `always` 步删除。验证——YAML 静态通过；
机制由 gate `ota-bundle` job 覆盖（正例 + 三负例）；tag + Secret 的正回执是外部
依赖，未取得前不冒领。

**2026-11-17 rrd 冷启动观测与健康读面 append（续轮三）**：事实——注册行原先没有
耗时可读，`ctl.health` 也没有注册延迟列，"卡在哪个模块"只能靠掐时间。处置——注册行
追加 `boot_ms`（daemon 起点相对毫秒）；`Rr3HealthRow` append `boot_ms`（基准未定格
为 `null`，不拿 0 冒充）；`rr3-sim --chain` 报告输出"启动基线"行（排除观察者
rr3-obs，模块注册耗时全列可见），CI sim-chain job 每拍打印同一行。测试面净增
**4** 条：`registry::tests` +1（health 的 boot_ms 偏移 / 未定格 / 倒退夹 0）、
`tools/sim` 的 `chain::tests` +3（解析顺序、断供与坏值负例、跳过观察者）。
验证——`cargo test -p rrd --lib` 309 条全绿、`e2e_ctl` 10 条全绿、`tools/sim` 30 条
全绿；本机冷/热两档基线已入 status"运行流畅度基线"段，CI 数值待 sim-chain 回执补。

**2026-11-17 失败日志归因单点与 action 运行时（续轮二）**：事实——buildroot 轨五处
失败注解各写一份 grep、gate 轨九处各写一份，噪音口径不一（`pyerrors.h` 这类命中会
把真因挤掉）；Node 20 档 action 已告弃用。处置——`tools/ci/lib/log-digest.sh` 成单点
（信号提取 → 噪音过滤 → 单行注解 → 末段回显，`--head` 用于"命令级错误在前"的日志），
13 处调用收敛；`--self-test` 正例 + 四负例进 gate `scripts` job；
checkout/upload/download 升 v7/v7/v8，setup-qemu 升 v4（只换 Node 24 运行时，
输入面未变；login/buildx 留发布面正回执后升）。验证——`9e1022f` / `b6dac2b` /
`1d9510d` / `096a4b6` 的 gate/integration/platforms 回执。

**2026-11-17 unsafe/FFI 注释审计门（续轮四）**：事实——生产面 122 处 `unsafe`
关键字中 78 处没有 SAFETY 前言，集中在 esp32 板侧、macaudio 开发面与少数 Linux
目标面；"有没有注释"此前没有任何门在管。处置——新增
`tools/ci/audit/unsafe_audit.py`（静态扫描，关键字匹配 + 到空行的紧邻注释段，跳过测试
路径），允许条数住 `tools/ci/baseline/unsafe-baseline.txt`（**只降不升**，与 gate-a/gate-d/
行数/体积门同款）；gate `scripts` job 执法。首批把 Linux 目标面 11 处补全
（rr-ipc uart/excl、rrd daemon/ctl 的 signal/write/chmod/getgrnam/chown、
rr-hal linux_gpio/linux_power 的 close、nmcli 的 fcntl），基线自 78 降至 **67**。
**同轮批次二/三**：esp32 板侧 `httpd/serve.rs`/`cred.rs`/`main.rs`/`oled.rs`/
`commands.rs`/`state.rs`/`link.rs` 与 `espidf-sentinel`/`espidf-std-hello` 共 49 处、
macaudio 18 处补齐 `// SAFETY:` / `/// # Safety`（unsafe fn 用调用方契约形式），
**基线降至 0**；`commands.rs` 与两处状态行的内联 unsafe 顺带提为带前言的局部变量
（行为不变）。此后任何新增未注释 unsafe 即红。**续轮六扩面**：加 `--self-test`
（生产/测试/extern 三类扫描面夹具）进 gate `scripts` job；`--report` 只读单列测试
路径未注释 unsafe 与生产面 `extern "C"` 缺 SAFETY 的清单，不计基线、不改退出码
（首份报告：测试路径 0、`extern "C"` 声明块 6 处待下批评审——`extern "C" fn`
实现/类型别名不计，契约住注册点与调用点）。

**2026-11-17 流水线结构审计门（续轮七）**：事实——五条 workflow 的 21 个 job 均有
`timeout-minutes` 与顶层 `concurrency`，但 action 版本靠人记：platforms 的 ipct003
仍用 `setup-qemu@v3`，而 cross 腿已升 v4（同一工作流两版并存），"升级漏一拍"
没有任何门在管。处置——新增 `tools/ci/audit/workflows_audit.py`（文本扫描：并发组 /
job 超时 / `ACTION_MATRIX` 版本矩阵；升级走显式提交改表），ipct003 的 qemu 升 v4；
gate `scripts` job 先 `--self-test`（好例 + 缺并发/缺超时/版本漂移三负例）后执法。

**2026-11-17 流水线审计扩面（续轮七）**：判据扩到**触发面与依赖图**——`push:` 必须带
`branches`/`tags`（全分支推送会白烧 CI）、`publish.yml` 必须由 tag 触发；job `needs`
引用必须存在且无环。自检负例由三条扩到六条（新增 needs 悬空 / needs 成环 / push
无范围），全仓五条 workflow 通过。**同轮覆盖矩阵**：`tools/ci/README.md` 首立
"平台 × 编译腿 × 执行腿 × 契约面 × 发布制品 × 缺口"对照表，并如实登记四条缺口
（riscv64 无发布镜像、S3 无固件 release 资产、三块真板运行证据不属 CI、Windows
独立工程门未跑）；补腿按缺口清单立项，不凭印象宣称"覆盖面全"。

**2026-11-17 sim 稳态段与日志判据修正（续轮七）**：事实——① `--chain` 取证后立即收尾，
"实时性"只有 sense 自报的 `max_jitter_ms`，没有宿主侧、跨模块的操作口径；② 本机首跑
暴露演练假红：brain 分次写「对话轮完成」行，`wait_log_contains` 在 `tts_pcm_ms=` 处
命中半行，`actions=1` 尚未落盘 → 误判"动作数不是 1"。处置——`--chain` 增 `--hold-ms`
（仅该模式，1000–600000）：取证后每 500ms 采 `rr3-ctl health`，`steady.rs` 纯逻辑聚合
各产品模块 `since_beat_ms` 峰值与末拍相位，判据＝四模块末拍 `running` 且峰值不触各自
看门狗（观察者不计）；CI `sim-chain` 缺省 hold 60s（`RR3_SIM_HOLD_MS` 覆写）。日志判据
改为**只认以换行收尾的完整行**（`rr3_log_find`），半行不再提前返回。本机回执：60s /
117 拍，峰值 201–206ms（看门狗 1000ms）；`tools/sim` 35 测试全绿。

**2026-11-17 rr3-ctl 健康表 BOOT_MS（续轮七）**：事实——`ctl.health` RPC 自 `38089c1`
起回 `boot_ms`，但 CLI 健康表只有六列，冷启动列在实际运维面不可见（RPC 有、CLI 无）。
处置——表头加 `BOOT_MS` 第七列，JSON null（基准未定格）渲染 `?`（沿用"缺失字段不得
显示 null"口径）；单测钉数值/表头/null 三态。

**2026-11-17 日志落盘序（macOS 腿红根因）**：事实——`3e00a37` platforms 的 host-macos
红在 `rrd_reboot_recovers_previous_run_lines_from_disk`；`rr3_log` 先写 stdout 再落盘，
用例等 stdout 的收尸行后立刻 SIGTERM，而关停 flush 线程落盘后直接 `process::exit(0)`
不等其他线程 ⇒ 文件里可能缺这行，第二世回填判据偶发失败（时间窗窄，本机 15 次未复现，
macOS runner 时序更紧）。处置——`rr3_log` 改**落盘先于 stdout**（"可观察即已持久"；
落盘失败仍只降级自身）；host-macos 失败注解补 `panicked at` 与末 12 行（只抓 test FAILED
等于只报门牌号）。本机 e2e_profile 5/5、该用例重复 20 次全绿。同批把日志面自
`lib.rs` 结构化拆到 `src/log.rs`（修序后 lib 触到 600 行门；纯移位，`lib.rs` re-export
保持 `crate::rr3_log*` 路径零改动——lib 86 行 / log 532 行）。

**2026-11-18 流水线审计对齐新结构（续轮九）**：事实——远端结构治理把 CI 政策写成
[ci-workflows.md](ci-workflows.md)（工具链钉 1.100.0、缓存 `workspaces` 必填与
`cache-all-crates`、触发 `paths-ignore`、job ≤10、publish 不可中断），但
`tools/ci/audit/workflows_audit.py` 仍只查版本矩阵/并发/超时/needs——政策没有执法位。
处置——审计扩到新政策六类（工具链钉版、缓存参数、触发过滤、job 上限、publish
cancel 语义、needs 图沿用），自检负例由六条扩到十一条；全仓五条 workflow 通过。

**2026-11-18 广播丢帧读面（续轮十一）**：事实——fanout 满队列策略是"丢当前帧、
保留连接"，但只有逐 64 帧一次的日志，`ctl.status` 看不到累计；慢消费者期间"丢了多少"
无法从运维面读出（连接鲁棒性/任务级可观测性缺口）。处置——registry 增
`rr3_reg_fan_dropped()` 聚合，`ctl.status` 回 `fan_dropped`，`rr3-ctl status` 渲染同名行；
单测钉"两次丢帧后聚合 = 2、续投后不回退"与 CLI 渲染；rrd-rpc/status 对表。

**2026-11-18 慢消费者点名与 ctl.logs 口径订正（续轮十二）**：事实——全局
`fan_dropped` 只能回答"丢了多少"，不能回答"谁慢"；`rrd-rpc.md` 的 `ctl.logs` 还写着
"不落盘"，与 sink 已实装矛盾。处置——`ctl.health` 行增 `fan_dropped`（`rr3-ctl` 健康表
第八列 `FAN_DROP`），单测钉"health 行 = 2"；rrd-rpc 的 health/logs 两行按现状订正。

**2026-11-18 publish 合并腿缺 checkout（main 首跑红）**：事实——远端结构治理后
publish 吃 main 推送（edge 标签）；main 首跑 build 两腿绿、merge 腿 `exit 127`：
合并腿直接 `bash tools/deploy/image-smoke.sh`，但该 job 没有 `actions/checkout`，
脚本根本不在工作区。处置——merge job 首步补 checkout（注释记根因）。教训——
**只在 tag 触发的作业腿，main/PR 面从未执行**，"流水线里有这条腿"不等于跑过；
新结构把 publish 扩到 main 后，这类零执行死角才暴露。

**2026-11-18 计数器落盘执行面分离（续轮十三）**：事实——R26 合并写在窗口到点时由
心跳 reader 线程持全连接共享的 gate 锁做 tmp+fsync+rename；一次 eMMC/SD 抖动就把
其余模块的心跳判活（`rr3_reg_beat`）排在磁盘后面，慢过看门狗即误判死。处置——新增
`rrd-ctr-store` 单线程（`counters.rs` 的 `rr3_ctr_store_spawn`），窗口到点只投
`try_send` 非阻塞信号（快照在落盘时取当值），`Rr3CountersGate` 增 `in_flight`
合并一轮，`conn/beat.rs` 改为判活记账先于计数归并。判据——心跳路径不再出现
`rr3_counters_store` 调用；单测钉在飞合并 / 失败回执 / 当下快照；`e2e_ctl` 三条
counters 用例保持绿。

**2026-11-18 watchdog 拍漂移留痕（续轮十三）**：事实——watchdog 是判活唯一线程，
自身被长动作或锁竞争拖慢时判死时刻整体后移，而 health 的 `since_beat_ms` 只在扫描
时刻采样，看不出一条"判死晚了"。处置——每拍起点量距上一拍完成的间隔，>2×tick
点名（连续漂移每 60 拍再点一次），判死基准不补跳；判据 `rr3_tick_late` 单测钉
2×tick 边界不误报。

**2026-11-18 日志落盘常持句柄（续轮十四）**：事实——`rr3_log` 每行
`stat+open+write+close`（轮转检查逐行 metadata），且 fanout 满队列点名发生在
registry 锁内、watchdog 判死也逐行落盘——锁内系统调用越少越好。处置——落盘臂拆
`log/sink.rs`：定格时打开一次句柄，每行一次 `write_all`（仍无用户态缓冲，"打过
的行当场就在文件里"不变），轮转按进程内累计字节（打开时既有大小计入，跨代超界
文件首行即轮转）；`rr3_log_rotate` 保留给 supervisor 的 per-unit 文件。判据——
行内系统调用 4→1；单测钉按字节预算轮转与既有大小计入；重启回填 e2e 同批复核。

**2026-11-18 rr3-ctl 补只读子命令（续轮十五）**：事实——`rr3.ctl.wiring` /
`rr3.ctl.contracts` 早已实装，但 rr3-ctl 只有六子命令，运维面读不到装配快照与
契约审计（功能欠账）。处置——命令面 6→8，wiring 六段表、contracts 三段表；空段
"（无）"、`available:false` 报不可得、判果 `ok/FAIL/?`；渲染臂拆
`commands/surface.rs`（mod 398 / surface 305 行，原单文件 697 行触 600 门）。
判据——rr3-ctl 单测 21→25（wiring 渲染/不可得/contracts 渲染/fake-rrd 往返），
clippy 干净；文档门抓出 `0.0.5 方案` 指向旧 `commands.rs` 的断链并同批修复。

**2026-11-18 Windows 独立工程门补腿（续轮十六）**：事实——`host-windows` 只跑
workspace，独立工程（`modules/*`、`tools/*`、`base/ota/*`）在 Windows 面零覆盖
（覆盖矩阵缺口④）。处置——先收平台中性子集编译步（`tools/mkmodule` /
`tools/rr3-codegen`）；**首跑红**：`rr3-sentinel-harness` 依赖
`rr_ipc::transport::uart`（模块按 `linux/macos` 门控，raw termios），Windows 无
该模块——子集移除并在矩阵登记理由，workflow 注释记根因。判据——host-windows
新步 success；矩阵缺口④由"未覆盖"改"部分收口（2 个中性工程）"。

**2026-11-18 审计增权限面与失败显形（续轮十六）**：事实——五条 workflow 都有
顶层 `permissions:` 且无 `continue-on-error`，但这两条**没有门在管**，删除或加
掩码不会有人发现（掩码会把漂红巡检变成假绿）。处置——
`workflows_audit.py` 增两条判据：顶层 `permissions:` 必须存在、含
`contents: read`、不得 `write-all`；`continue-on-error` 非 false 即红。自检负例
11→13，政策真源 [ci-workflows.md](ci-workflows.md) 与 `tools/ci/README` 同批登记。

**2026-11-18 落盘状态可观测与锁内 I/O 清零（续轮十七）**：事实——① 日志落盘
写失败只有 stderr 一行，`rr3-ctl status/logs` 看不到"崩溃取证面已经没了"；
② fanout 满队列点名、影子统计首见、计数键上界三处都在持 registry 锁时直接
`rr3_log`（每行一次 write，慢盘下是锁内阻塞）。处置——① `ctl.status` 增
`log_sink`（on/off/dead），失败分支先置降级位再走 `rr3_log`（ring/stdout 可见、
不重入落盘），CLI 同名行；② registry 增 `pending_logs`，锁内只收集，新增
`rr3_reg_with` 放锁后统一落盘，凡可能产生日志的调用（fanout/shadow/ctr_merge）
全部改走该入口。判据——单测钉"收集一行、取走即空"与状态读口，e2e 钉字段可读；
rrd 315 单测 / rr3-ctl 25 测试 / clippy / check.sh / metrics 全绿。

**2026-11-18 Windows 独立工程覆盖扩面（续轮十八）**：事实——`host-windows` 的
独立工程子集只有 mkmodule / codegen，`rrctl`（UDS 查询已有非 Unix 返回路径）与
`rr3-pack`（权限位已 cfg 门控）实际可跨平台编译却零覆盖。处置——子集扩到 4 个；
**首跑红**：`rr3-ota-ctl` 依赖 `rr-ota::bootenv`（实现按 `cfg(unix)` 收口、
CLI 子命令无非 Unix 分支）——"源码里 unix 引用都被 cfg 包着"不等于模块面在非
Unix 可用；移除并把 root cause 与"不要凭印象加回"写进 workflow 注释与覆盖矩阵
缺口④。判据——host-windows success（4 个工程逐个 `cargo check`，失败点名工程根）。

**2026-11-18 锁内 I/O 收口与排拍栅格化（续轮十九）**：事实——① `rr3_write` 的
`if let Err(..) = rr3_frame_write(&mut *rr3_lock(..))` 中临时 guard 活到整条语句
结束，失败点名成了持出向锁写磁盘；② `rr3-ctl status` 丢弃 rrd 已回的 `boot`
装配摘要；③ watchdog 用"睡眠 tick 后扫描"，实际周期 = tick + 扫描耗时，负载下
逐拍累积后移。处置——① 结果先落局部变量、放锁后点名；② CLI 渲染 `boot = …`
（null 显式 `?`）；③ 改固定栅格 `deadline + tick`，超拍从收尾时刻重排不补跳，
单拍扫描 > tick 才点名。判据——rrd 316 单测 / rr3-ctl 26 测试 / clippy /
check.sh / metrics 全绿，九腿 CI 覆盖。

**2026-11-18 模块间热路径锁内 I/O 收口（续轮二十）**：事实——`rr3.call` 转发
`rr3_call_fwd` 在持 registry 锁时打点名日志（每次模块间调用一次磁盘 write），
心跳记账、watchdog 扫描与扇出都在等这把锁；应答回路 `rr3_resp_send` 的失败
分支也因 if 条件临时 guard 延寿而在锁内回错误帧。处置——锁内只做查表/构帧/
`try_send`，点名与错误应答放锁后；投递结果先落局部变量再判。判据——316 单测 +
`module_call` 4 用例（路由/三种拒法）全绿；首次 check.sh 的 fmt 步抓出格式漂移，
同批 `cargo fmt --all` 收口。

**2026-11-18 计数落盘执行面存亡可读（续轮二十一）**：事实——`rrd-ctr-store`
线程 panic 展开后只有 panic hook 一行 stderr：gate 的 `in_flight` 永不回执，此后
所有窗口都不再排落盘，而 `ctl.status` 与"正常"没有差别。处置——执行面存亡定格
三态（`off` 未启用或已随关停收场 / `on` 线程在位 / `dead` 启用后异常消失），
收场守卫在展开中只置位、不做 I/O（避免二次 panic），线程起不来同样记 `dead`；
`ctl.status` 与 `rr3-ctl status` 同名可读。同批把 counters 单测移到
`counters/tests.rs`（纯移位，生产文件 550→347 行，先腾出 600 行门余量）。判据——
counters:: 11 测（含 `on`→`off` 收场断言）、e2e `ctl-status`、rrctl 26 测全绿；
`check.sh` + `metrics.py` 通过。

**2026-11-18 执行面自愈与锁内 I/O 复核收口（续轮二十二）**：事实——一轮落盘
panic 会带走整条 `rrd-ctr-store`：在飞位留在 gate 上，reader 侧此后不再排新一轮，
账本静默停写；"锁内 I/O 已清零"此前只有人工巡检，没有机器判据。处置——① 每轮
`catch_unwind` 隔离，panic 时释放在飞位并续跑（panic 文案仍由 daemon 的 panic hook
落日志）；② 复核 `daemon/boot.rs`、`supervisor.rs`、`supervisor/spawn.rs` 低电旗门
（条件里的临时 guard 在 `if` 语句结束即释放，日志在语句之后）与全仓 `rr3_reg_fanout`/
`rr3_reg_ctr_merge`/`rr3_reg_shadow` 生产调用点（逐点确认在 `rr3_reg_with` 闭包内），
无锁内 I/O 残留；机器判据列为下一轮 P1（人工巡检不再重复）。判据——counters:: 12 测
（新增"panic 后在飞位释放"）、`check.sh` + `metrics.py` 通过。

**2026-11-18 日志落盘自愈（续轮二十三）**：事实——日志落盘写失败一次即置
`dead` 且"本进程不再重试"：一次写抖动 / 暂时 ENOSPC / 日志目录被短暂挪走，就把
崩溃取证面永久关掉，只能重启 daemon 恢复；`ctl.status.log_sink` 只能读到终态。
处置——失败后进 10s 冷却（冷却期内每行只做间隔比较、不撞盘），到点先重开句柄
再试一行，成功即回 `on`；点名只在"首次转入降级"与"恢复"各一次；`dead` 语义改为
"此刻不可用"。判据——log 面 12 测（新增冷却闸：窗内不重试 / 到点到期 / 恢复清位）、
`check.sh` + `metrics.py` 通过。

**2026-11-18 §2 回执迁出（续轮二十四）**：事实——`conventions.md` 已 4134 行，§2 的
日期回执 87 段 / 1138 行把判据表挤在开头、检索靠翻；判据与沿革混在一处，改一处判据
要在史料里找落点。处置——按纯移位迁出到本文件（逐段逐字照搬、未改写），正文只留
判据表、步号口径与指针；§3 里那条带号段表的日期注记保持原位（表体属结构内容，
不迁）。判据——迁出前后非空行多重集守恒（脚本核对）、§2 零残留日期段、
`check.sh` 全量门禁通过；`conventions.md` 4134→2921 行。
同批红收口：⑬ 仓面卫生门原先只豁免 `conventions.md`，回执里"仓面卫生步"这条规则
原文引用被禁模式（`trae_projects` 等）即触发；排除面同步到本存档（`5391bf0`），
本地复现与修复后均全绿。

**2026-11-25 ⑬ 提交面补盲（续轮二十五）**：事实——历史全量治理后，提交信息与
提交身份已中性；但 ⑬ 的扫面只有 `git grep`（追踪文件）与 `git status`（未入册），
**提交对象根本不在面内**——增量里再写进本机路径不会有人拦。全量核查还发现两条
早期提交正文在"描述规则本身"时引用了被禁模式串（规则说明，不含用户名与绝对路径），
说明该盲区已实际发生过一次。处置——判据收进 `audit/commit_msg_audit.py`，接进既有
⑬（不新增门号）：命中集合须与 `baseline/commit-msg-allow.txt` 逐条相等，新增命中即红、
表内失效（历史重写）亦红，豁免表不许追加；身份面只拒本机域（`@localhost` / `*.local`），
不对贡献者邮箱设白名单。自检五例（干净 / 泄漏 / 豁免内 / 豁免失效 / 本机域）接入 gate
scripts job。判据——`commit_msg_audit.py` 自检与主体通过（2 条豁免命中）、⑬ 五项全绿。
同批订正：⑫ 行面数字与 `metrics.py` 实况对齐（>50 行档 =0、panic ≤2、CC 最大 ≤8）。

**2026-11-25 ⑬ 两处失效根因（续轮五十一续）**：① 提交面纳入后 CI 首跑红——
`actions/checkout` 缺省浅克隆只含一条提交，豁免表整体判"失效"；处置：conventions
作业全历史检出（其余作业保持缺省），审计对浅克隆显式报错（fail-closed，不静默放行）。
② 本地恒绿——`hyg_paths` 用 `BASH_SOURCE[0]` 定位步骤体做排除，zsh `source` 时该变量
为空，`:(exclude)` 空路径把**全部文件**排除，本机路径检查长期是空跑；处置：空值回落到
固定路径，且新增检测器自身列入排除面（它必须持有模式串）。判据——zsh / bash 两环境
五项全绿；负例（临时追踪文件含真实本机路径）当场判红并点名文件行。

**2026-11-26 批52 门禁负例体检（六条，均判红）**：在 detached worktree 注入违规、
验后回滚，避免污染主工作区。① ⑤：`base/runtime/rr-core/NOTES.md`（docs/ 外散落）
→ 退 1 并点名；② ⑯：505 行 `.rs` → `fail=1` 并点行数；③ ⑫：71 行生产函数 →
退 1 并给文件/函数/行数；④ ⑬ 文件面：追踪文件含本机绝对路径 → `fail=1` 点名行；
⑤ ⑬ 提交面：提交标题含本机路径 → `fail=1`；⑥ `release-tag`：`v0.0.9`（与
`VERSION 0.0.8` 不同源）退 1、`v0.0.8` 指向非 release 提交退 1、指向 release
提交输出 `tag=v0.0.8 version=0.0.8`。
**契约记录**：⑤/⑫ 以函数/进程退出码报红，⑬/⑯ 以全局 `fail` 报红——体检与复算
必须按各自契约取值，只读退出码会把 ⑯ 的判红误读成绿。

**2026-11-26 批53 烧录面收口（静态一致性）**：事实——`flash-esp32.sh` 直接
`espflash flash "$ELF"`，分区表靠 cwd 猜；从仓根跑猜不到 `tools/esp32-periph-base/
partitions.csv`，内置缺省表没有 `spool`，语音段会静默降级；`image-esp32.sh`
未给 `--partitions` 时同样吃内置表。处置：两条路径都把分区表钉到工程真值并在烧录/
装配前 `fail-closed` 校验 `spool` 条目；`flash-artifact.sh` 只改示例路径（`~/Downloads`
→ `./`），语义不变。判据——三脚本 `bash -n` 通过、⑬ 全绿；未实板执行（硬件未到位），
实板首烧按 `tools/deploy/README.md` §验收回填。RK3566 侧核对：maskrom 手册的
32KiB/6MiB 裸区与 `genimage.cfg` 的 `offset = 32K/6M` 一致，镜像名与 `post-image.sh`
注入链一致，无需改动。

**2026-11-26 批54 运行流畅度静态审计（不改代码）**：逐点核对三个热路径——
① 广播扇出在注册表锁内只做 `try_send` 到有界队列，满队列丢当前帧并计数、不阻塞
（`registry/view.rs` `rr3_reg_fanout`）；② 看门狗按固定栅格排拍，扫描耗时不计入
周期，超时只点名不补跳（`daemon/tick.rs` `rr3_watchdog_loop`）；③ sim 采集在
`open` 时预解波形函数指针与每通道增益/相位，`read` 不走逐样本查找
（`sim/source.rs` `Rr3SimOpen`）。三处均已是目标形态，未发现可被现有测试/指标
证明的退化项，故本轮不动生产代码。

**2026-11-26 批56 apt 安装收拢（`apt-retry` 复合 action）**：事实——gate 有
三次有界重试，platforms/publish/buildroot 共五处各自内联单次安装或复制重试骨架，
口径漂移且失败归因不一致。处置——新增 `.github/actions/apt-retry`（每轮先刷新索引
再装、三次有界重试、末次经 `log-digest` 压注解），六处调用统一为"包名 + 标题 +
日志名"；`tools/deploy` 与工作流文档同步。判据——`workflows_audit.py` 通过；head
`8ef6a51` 四工作流全绿，其中 gate（alsa）、platforms（riscv64 ×2）、buildroot
（dts + 宿主依赖 ×2）实际执行了本 action。

**2026-11-26 批55 文件头收敛收口（33 → 18 行）**：事实——`.rs` 头注释最长 33 行，
>24 行 33 个；多数是批次沿革、历史裁定与重复解释，职责/边界/纪律反而被埋。
处置——按"只留职责一句、边界与不变量、违反即出错的纪律"逐批改写，保留 SPDX 与
intra-doc 链接；累计 20 批（部分文件跨批复压），最长 33→18 行，>24 行清零。剩余
15–18 行档 82 个（SPDX + 13–16 行），内容已是职责/纪律，无批次沿革；再压会删
不变量，故停在此档。判据——每批提交前 ⑤/⑯/⑬ 全绿；头行数脚本复算
`max=18, >24=0`；代码零改动（diff 仅注释行）。

**2026-11-27 批57 文档面收口（docs 根散落件归位）**：事实——`docs/洛克三号-开源智能
硬件赛道-参赛介绍.md` 住在 docs 根、四档之外，`git grep` 全仓零引用；⑤ 门第三判据只
覆盖 README，非 README 的散落件是它的盲区。文内两处断言已失真："24 份 ADR" 实为 31
份；"语义基线 `f64451e`" 在批47 重锚后已不在对象库。处置——归 `docs/reference/`
（与板卡笔记、自证表同类），`docs/README.md` 库地图登记并注明"不属四档、不持有事实
真值"，`.gitignore` 删去旧白名单行（`!docs/reference/**` 已覆盖）；文内 ADR 计数改
31、失效哈希改为 `VERSION = 0.0.8` 发布点并链接 `status.md`。判据——⑤ 文档门（空文档/
断链/导航漏链/docs 外散落）全绿；`git grep 参赛介绍` 命中索引行；diff 仅位置与三处
文字变更。

**2026-11-27 批58 tag 命名空间收口（本地直名清零）**：事实——本地存在直名 tag
`pre-scrub-backup`（指向 `8ac19dfa`，批47 重锚前的备份点，非 HEAD 祖先），与"直名只留
发布点"的 §7 口径不符；三远端均无此 tag，故"远端干净"掩盖了本地未治理。处置——改打
`archive/pre-scrub-backup` 后删直名 tag（仅本地，不改远端）；§7 补一条**直名 tag 只留给
发布点**（本地与远端同一条尺子，备份/取证/试验点一律 `archive/<用途>`），把这次靠人发现
的盲区变成明文口径。判据——本地直名集合 = `v0.0.8`、`archive/*` = 19；三远端
`ls-remote --tags` 仍只有 `v0.0.8`。

**2026-11-27 批59 ⑤ 门盲区收口（docs/ 根白名单）**：事实——第四判据只扫 docs/ 之外，
docs/ 根上的非 README 散落件没有判据（批57 的参赛介绍正是人工发现后才归位），自检里也
没有对应负例。处置——`steps/5.sh` 增第五判据（docs/ 根只允许 `README.md` / `status.md` /
`status-history.md`，其余一律打印 `散落文档`），并补两只夹具（docs 根散落即红、
docs/status.md 豁免通过）；`conventions §2` ⑤ 行、`tools/ci/README` ⑤ 行、`docs/README`
目录约定三处同步。判据——自检 9/9 PASS；真仓 `check_docs` 退出码 0；负例只在夹具里红，
不在真仓静默放行。

**2026-11-27 批61 部署面"到货即执行"对表**：事实——`tools/deploy/README.md` 只覆盖 ESP32
烧录（资产/整片/裸 espflash/验收），RK3566 的 Buildroot 保底轨与 distro 快路轨、x86_64 /
riscv64 的运行镜像、树莓派参考真板都不在这张入口表里，到货时得跨四份文档拼流程。处置——
新增"平台 × 入口 × 判据"表（七个平台行：ESP32 classic、ESP32-S3、RK3566×2 轨、
英特尔/AMD x86_64、进迭时空 riscv64、树莓派 4B），每行给制品、入口命令与验收判据；表下
按平台列"各自第一个失败点"（DTR/RTS、maskrom 认设备、镜像两段冒烟），实板状态一律指
`docs/status.md` 敞口 6/10/11 不在此重述。判据——⑤ 门绿（新增链接全部可解析）；未改任何
脚本行为；`git diff --stat` 仅本 README 与回执。

**2026-11-27 批60 对外介绍稿计数对表（12 处订正）**：事实——参赛稿自述"每个数都附产出
它的命令或仓内文件"，逐条对账后 12 处已漂移：物理键八→九（`RR3_PIN_DERIVED_KEYS:
[&str; 9]` 含 `oled_addr`）；主题"42 含 2 retired"实为 44 条（在役 42）；31 个判定位
宣称"逐位有反例"而实判 14、余 17 待建；S3 UART2 仍是订正前的 17/18@921600（真值
GPIO41/42 @1.5M）；`dc7a40c` 在重锚后不可解析；四份文件行数 75/110/36/32 → 85/114/38/33；
门禁 20 步/9 job → 25 步/29 job（六条 workflow）；基线 21/79 → 0/83；`#[test]` 裸数
1122 → 1467；transport trait 2 → 3（shm 已摘面）；轮次"十一轮"与 §18–§25 只有 8 节不符；
对拍路数增了 riscv64 第五路。处置——逐条按仓内真值订正；会随基线漂移的测试计数改
"产出者 + 基线号 + 当时值"写法；失效哈希改"基线号随 `status.md` 归档"。判据——每条
订正带可复跑命令：`grep -c '^\[\[topic\]\]' contracts/topics.toml`=44、`grep -c '^step '
tools/ci/check.sh`=25、六 workflow 的 `jobs:` 二级键=29、`grep -rn '#\[test\]'
--include='*.rs' | wc -l`=1467、两份 baseline 尾行=0/83；⑤ 门绿。

**2026-11-27 批62 流水线时长基线归档（只作评审基线，不成门）**：事实——"流水线变慢"没有
可比基线：publish/buildroot 的 release 腿很少跑，gate/integration/platforms 的墙钟从未记录；
job 日志匿名不可读（403 已实证），缓存命中率拿不到。处置——`docs/process/ci-workflows.md`
增"时长基线"节：六行（gate 3.9–4.0min、integration 4.4–4.5min、platforms 12.9–15.1min、
buildroot 日常 dtb 0.6–1.1min、buildroot 全量镜像 v0.0.8=86.9min、publish v0.0.8=9.9min），
每行带取证 head；附一段匿名只读取数脚本与读法（越上沿 50% 先问腿数/缓存，先不判据）；
明确"日常 1 分钟是 dtb 腿、全量镜像腿受 `if:` 门控，两者不可互相冒充"。判据——⑤ 门绿；
未新增任何门禁脚本；表内每个数可由随附脚本复跑。

**2026-11-27 批63 文件头收敛（15–18 行档 75 件，5 批提交）**：事实——`git ls-files '*.rs'`
按"首段连续 `//` 行数"分档统计，15–18 行档 75 个；内容普遍夹带批次沿革（"批N23 件三"、
"Task 11b"、"方案快照漏记"）与真板订正过程叙述，与"文件头精简"直接冲突。处置——按
base/runtime（21 件）、rrd（13 件）、modules（12 件）、tools/esp32*（15 件）、tools 其余
（13 件）五批收敛：**保留** SPDX 首行、职责一句、边界与不变量、违反即出错的纪律、失败
语义分档、接线与平台事实；**删除**批号/轮次/任务号沿革、"此前如何/为什么改"的过程叙述与
重复解释；段落空行合并压行，代码零改动。第 4 批清单原 16 件，其中 `mic.rs` 本就在 15 行
档，未动即达标。判据——复核脚本按"首段连续 `//` 行数"逐件重算，五个提交后全 75 件 ≤15；
⑤ 文档门（空文档/断链/导航漏链/散落面）绿、⑬ 仓面卫生门（无本机绝对路径、无未入册
源码件、提交信息无本机标识）绿、⑯ 源文件行数门绿；移除的 intra-doc 链接全部改为普通
反引号，保留的链接（`SETTLE`/`rr3_store_perm`/`rr3_assemble_parse`/`rr3_role_group` 等）
逐个人工核过符号仍在册。

**2026-11-27 批65 卡住可查性复核（先查已有机制，结论：不改代码）**：事实——"tick 超时与
fanout 丢帧读不到"的前提不成立：`rr3.ctl.status` 早带 `denied_publish`/`fan_dropped`/
`fan_write_timeout`/`log_sink`/`log_sink_max_us`/`ctr_store`，`rr3.ctl.health` 带逐模块
`since_beat_ms`/`beat_peak_ms`/`fan_dropped`/`crash_streak`/`restart_exhausted` 与运行级
`watchdog_overruns`，`rr3-ctl status|health` 两侧渲染面也都在（`format_status`/
`format_health`）。处置——不新增字段、不加第二个读面（同一真值立第二供给路径即债）。
判据——`grep -n` 对齐三段实现（`rrd/src/conn/ctl.rs` 状态应答、`rrd/src/conn/ctl.rs`
健康应答、`tools/rrctl/src/commands/mod.rs` 两渲染函数）；本批零代码改动，宁缺勿滥。

**2026-11-27 批66 ESP 腿快照补缓存命中信号**：事实——两腿时长差（classic 五门 vs S3 两门，
8.3 / 15.0 min 量级）缺少"是门数还是缓存未命中"的可读归因；快照注解只有 target 与
`.embuild` 体积，且该段注释在历次编辑后出现整段重复。处置——注册表缓存步骤补 `id`，
快照步骤读 `cache-hit` 三态（`hit`/`partial`/`miss`）并入同一行与注解 title；重复注释收敛
为三行（体积口径与"只读数不改判据"保留）。判据——`python3 tools/ci/audit/workflows_audit.py`
绿（并发组/超时/action 版本矩阵/权限/五芯片覆盖/发布面/制品托管七项）；本步只读输出，
不进入任何 `if:` 判据，不改变门数与覆盖面。

**2026-11-27 批63 收尾 推送后漂红审计与根因修复**：事实——收敛后首推 gate 红，注解定位到
`cargo clippy -D warnings`（workspace 与 `tools/sim` 两处），根因是**头注释内的 markdown
列表后直接接段落**：`rr-registry/src/assemble.rs` 两条 `- ` 项后紧接 `demands` 段落、
`tools/sim/src/main.rs` 编号列表第 5 项后紧接"另有三个互斥模式"段——两者都触发文档续行
类告警，注释内容本身没错、错在分隔。处置——恢复列表后空行；`assemble.rs` 的 `demands` 段
压到三行、`main.rs` 尾两段合并，两件仍 ≤15 行。判据——全仓扫描器（列表项后紧接不缩进
文本行）命中数 0；远端 gate / integration / platforms 三条流水线在 `258778d` 全绿；
`platforms` 绿同时验证批66 的快照步骤（`cargo_cache=hit|partial|miss` 一行）可正常执行。
**教训入册**：压缩行数时不许拆掉列表与段落之间的空行——那是 markdown 结构，不是排版余量。

**2026-11-27 批67 对外稿取证复核**：事实——参赛介绍里三类数字不可静态核：板上证据段的
心跳/直方图/组帧延迟/增益、S3 段 App 字节数与占用率、改动清单表的行数列。处置——板上
证据段改为"结论 + 判据形状"，读数回指证据册与 status 历史条目（条目内自带产出者与当时
基线）；S3 段改"约占 16 MB 的 7%、boot 堆约 8.6 MB"并点名产出者（`espflash` 输出）与
原始条目（三平台验证计划 §6.6）；改动清单表删行数列，改"取自那次 `git diff --numstat`、
原提交在历史重锚后不可达，故不追哈希不抄行数"，并给出今天可复核的命令。判据——文档门 ⑤
绿；全文再无"无产出者、无命令"的实测计数（人工逐条过一遍）。

**2026-11-27 批68 板档目录同形**：事实——清点六个 `modules/*` 与十八个 `tools/*` 根文件：
模块侧六份固定件（`module.toml`/`composite.toml`/`config.toml`/`Cargo.toml`/`Cargo.lock`/
`README.md` + `src/`/`atoms/`）齐、可选件（`build.rs`+`sdkconfig.defaults`、`config-<板>.toml`）
已在 `modules/README.md` 登记，无缺口；tools 侧唯一缺口是**承载平台底座** `pi-periph-base`
没有根 README（`esp32-periph-base` 有），属平台间结构不对称。处置——补
`tools/pi-periph-base/README.md`（端口表、两个控制面、策略面单点、接线表、未收口五项，数值
逐条按 `board.rs` 常数与 `dispatch`/`info_of` 源码核对）；`tools/README.md` 落"板侧后端目录
同形"裁定（四件事：端口表/策略面/接线/未收口，其余小工具一支一行不另立 README）；
`docs/README.md` 补导航一行。判据——⑤ 文档门绿（新 README 在导航内、链接可达）、⑬ 仓面
卫生绿。

**2026-11-27 批69 广播帧共享体（运行流畅度）**：事实——`rr3_reg_fanout` 对每个收件连接
`tx.try_send(frame.clone())`，即一次广播深拷 N 份 payload（N = 收件连接数；128 ms 音频片
约 11 KB，六连接一帧约 66 KB），且这段复制发生在 registry 锁内。处置——收件队列元素改
`Rr3FanFrame = Arc<Rr3Frame>`：`rr3_reg_fanout` / `rr3_reg_send_to` 改按值收帧、构造即入
共享体，每连接只加一次引用计数；写线程按 `&Rr3Frame` 写，线上字节、投递语义、满侧丢帧
策略（`RR3_FAN_CAP` 满即丢当前帧并计数）全部不变。前后计数：每帧深拷由 **N 份 → 0 份**
（构造期那份保留），堆分配由 1+N 次 → 1 次，锁内复制量由 `N × (body+2)` 字节 → 0。
判据——`rrd` 325 项 lib 测试全过、clippy `-D warnings` 与 `cargo fmt --check` 干净；
gate / integration 在 `bed8eb4` 全绿。
**漂红与根因（本轮实证）**：首推即红，注解把失败点指到 `rrd (lib test)` 与
`rrd (lib)`——三类编译面漏项：① `registry.rs` 去掉 `Rr3Frame` 引入后，测试模块
（`use super::*`）失去该类型；② `beat.rs`（恢复沿）与 `supervisor/schedule.rs`（降级沿）
两处广播仍传借用；③ 字符串替换误伤 `&frame_to(b)`，生成 `frame.clone()_to(b)`。
**教训入册**：批量改签名时，测试模块的 glob 引入不算"跟着改"；替换模式要带词边界
（`&frame,` / `&frame)`），否则 `&frame_to` 这类同前缀标识符会被切坏。

**2026-11-27 批70 两条烧录链现状与制品对表**：事实——部署册有"平台 × 入口 × 判据"矩阵，
但缺"这条链跑到哪一步"与"制品 tag 与 `VERSION` 怎么对"两栏；ESP32 基本功能档的真实进度
（S3 首烧、classic 复烧）散在证据册与验证计划里，RK3566 maskrom 只有文档没有实烧。处置——
`tools/deploy/README.md` 增两节：①两条链现状表（已跑证据编号 / 待实板项 / 命令来源），
②制品 tag 对表（`VERSION=0.0.8`、仓内唯一发布 tag `v0.0.8`、旧号归档
`archive/v0.0.x/*`、五类制品的 tag 形状与触发条件、开发分支不产出制品）。判据——⑤ 文档门
绿；表中每条"已跑"都能点回证据册条目或流水线 run 号，每个制品名都能在
`publish.yml` / `buildroot.yml` 里逐字找到。

**2026-11-27 批71–72 写侧零拷贝（广播链第二刀）**：事实——扇出已改共享体，但
`rr3_write_bytes` 仍走 `rr3_frame_to_bytes`，**每个收件连接每帧再拼一份 头+payload**
（音频片 ~11KB，六连接就是 66KB/帧 的分配与复制，且在出向锁内）。处置——头用
`rr3_header_encode` 出 128B 栈缓冲、payload 用帧内切片，两段 iovec 一次
`write_vectored` 提交，部分写在锁内续投；每帧复制由 **1 次/连接 → 0**，系统调用
**1 次/帧**，`RR3_FAN_WRITE_TIMEOUT` 仍是"一帧 5s"。**审计修正（重要）**：批71 首版
写成两次 `write_all`，审计发现它把 5s 预算拆成两段（最坏 10s），与
`rrd-rpc.md` 的"对端 5s 不收"口径不符——批72 改单次 `writev` 收回口径，两次提交
在同一条历史里，不掩盖。判据——`rrd` 325 项 lib 测试全过、clippy `-D warnings` 与
`cargo fmt --check` 干净；gate / integration / platforms 在 `3c1c22d` 前后均绿。

**2026-11-27 批73 六份模块 README 同形**：事实——六模块 README 骨架不一致：brain 的
"与其它模块的边界"实为一张主题表（就是总线面），且把记账落点写成已不存在的
`dispatch.rs`（实为 `dispatch/turn.rs`）、还抄了"77 条"这种会漂的单测计数；
connect 缺"心跳自报计数"节。处置——brain 该节改名"总线面"并补一句"边界就是下表"、
落点改 `dispatch/turn.rs`、删硬编码条数；connect 补"不上报自报计数"一节并给理由；
`modules/README.md` 记下同形裁定（目录结构 → 总线面 → 心跳自报计数 → 本地自检，
无计数也要写这节，会漂的数字不抄）。判据——⑤ 文档门绿；六份 README 的二级标题
逐份对照只差模块专属小节（interact 的 Web 面板、brain 的意图规则面）。

**2026-11-27 批74 五芯片覆盖矩阵**：事实——`platforms.yml` 的五芯片约束只有散文
（哪些腿必须有），没有"某芯片在哪条腿、拿什么判据、发布面走哪条"的一页对表，
评审时要翻三份 workflow。处置——`docs/process/ci-workflows.md` 在"五芯片覆盖"下
落矩阵表（芯片 × 环境 target × 每次 push 的腿 × 发布面腿 × 判据），并写明两条口径：
英特尔与 AMD 同属 `x86_64`，一条腿覆盖两家、不造重复腿；Windows/macOS 是宿主等价面
不计芯片覆盖；另注明 `ipct003` 与 `autonomy-face` 两腿的证据归属。判据——⑤ 文档门绿；
`workflows_audit.py` 七项全过（矩阵里每个 job 名都能在 workflow 中找到）。

**2026-11-27 批74 附 漂红处置（distro-smoke）**：事实——`87de7c5` 的 integration 红在
`distro-smoke`，注解只有"exit code 101"，job 日志匿名不可读（403），且该腿 96 秒即退出
（不足以完成容器内全量构建）——根因不可见。处置——不猜、不改判据：把该步套上仓库既有
的 `.github/actions/guarded-run`（失败时把 error 行连同后 3 行上下文压成注解，并对
`spurious network error/502/504` 退避重试两次），命令与判据不变。判据——重推后
gate / integration 在 `3c1c22d` 全绿（distro-smoke 通过），落定为**容器侧瞬态**而非
代码缺陷；此后该腿失败会自带根因行，不再只剩退出码。

**2026-11-27 批75–76 写面收口审计与"每帧复制账"**：事实——全仓还有三处整帧拼装：
① ESP32 语音上行的 `audio_chunk_to_frame`（整帧字节**就是**有界队列的单元，不可拆）；
② `rr3_frame_write` 的三处生产使用者（rrctl、sim、sim_peer）全是低频单写者；
③ 测试夹具。热路径（rrd 广播）没有第二处。**每帧复制账**（一条 11KB 音频 Event）：
JSON 解析 1 次整树复制（要 topic 必须解析，换借用式需引入 serde 依赖或第二套解析器，
两者都比它省下的 44KB/s 更贵，**不做**）+ `rr3_body_encode` 1 次（2 字节类型前缀
必须拼入）+ 扇出 0 次（批69）+ 写面 0 次（批72）；余下的可削项只有"每连接重算一次
载荷 CRC"，实测量级 <1ms/s，收益不抵破坏 `rr3_header_encode` 这层安全网的风险，保留。
处置——把三档写法的选择依据钉进 `rr3_frame_write` 的文档（整帧单写 / 广播 writev /
所有权移交 `rr3_send_buf`），避免下次又在热路径复用它。判据——rr-ipc 测试绿、
`cargo fmt --check` 绿；gate / integration 在 `6c2230a` 绿。

**2026-11-27 批77 三篇 reference 与代码对表**：事实——按"引用须与代码逐字一致"抽检：
`USAGE` 全文、八个开关的默认值（1000/3000/200 等）、横幅格式串、§3 十一条错误文案、
`Rr3RpcMethod` 13 个方法与文档 13 行、`Rr3Phase` 四态、`RR3_RESTART_BACKOFF` 七档与
"连续 5 次"上限、counters 两个环境旋钮（10_000 / 64）**全部一致**；发现一处漂移：
`rr3.ctl.logs` 的"512 行环形"——自批N5 件二 F-30 起容量由装配单
`[runtime].log_ring_cap` 定格，512 只是兜底（`rr3_log_ring_cap_set`）。处置——
`rrd-rpc.md` 与 `status.md` 改口径；ADR-0010 裁定 5 正文保留、加修订注（ADR 不改写
历史），后果段同步。判据——⑤ 文档门绿；抽检的每一条都能在 `cli.rs` / `rpc.rs` /
`registry/model.rs` / `counters.rs` / `log/mod.rs` 的字面量里逐字对上。

**2026-11-27 批78 清理面巡检（不删有效物）**：事实——生产代码 `allow(dead_code)`
共 11 处，逐处查过：sense 三处是 `cfg` 条件面上的 Windows/macOS 分支、interact 两处是
门禁 awk 消费的词表常量（Rust 侧确实无调用点、注释已写明）、connect `bt` 五处是
"事件面登记后接线"的占位件（带触发条件）、brain 一处是"只记不读"的对话历史载体
（面板回看待接线）。**没有未登记的遗留**；`rr3_frame_to_bytes` 仍有真实消费者
（rrctl/sim/夹具），不退役。处置——本批零代码改动，结论入册（宁缺勿滥）。判据——
`grep -rn 'allow(dead_code)'` 逐条有理由；`cargo check` 无新增告警。

**2026-11-27 批79 rock3 双轨目录核对与"换板六件"**：事实——`base/os/` 两个子轨的落点
散在 README 的散文里，没人能一页回答"接一块新板要交什么、现役长什么样、谁接住"；
核对两轨现役文件（保底轨 8 件全在 `board/radxa-zero3/` + `configs/` + `patches/`，
快路轨 2 页 board 笔记 + 1 个 unit）无孤儿件；两轨形状不同（`board/<板>/` 多件 vs
单页 md）也不是缺口。处置——`base/os/README.md` 增"接入一块新板：要交的六件"表
（芯片事实 / 装机描述符 / 引脚构造 / 保底轨 / 快路轨 / 制品与烧录入口），每行给现役
实例与接手它的判据；写明形状差异的理由（构建系统按目录取板级件，发行版差异是运维笔记）。
判据——⑤ 文档门绿；表中每个路径都在仓内存在（逐条 `test -e` 过），每条判据都能点回
workflow 或门禁步。

**2026-11-27 批80 慢消费者闭环补测**：事实——慢消费者链三段（队列满丢帧 + 写超时计数）
有单测，第三段"收件端消失 → 扇出摘连接（`Disconnected` → `tx=None`）"**没有**：缺了它，
扇出会一直往没人读的通道塞帧、只记账不摘链。处置——`registry/tests/death.rs` 增
`fanout_drops_dead_writer`：drop 接收端后，投递数不计死连接、`rr3_reg_live` 转假、
活连接不受牵连、且**不计入 `fan_dropped`**（两种故障分开，不混账）。判据——
rrd lib 测试 326 项全过；`write_counted`（写超时分类）在批72 的 writev 后照常通过，
说明写路径换实现没有改变超时语义。

**2026-11-27 批81 发布被拒点名留痕**：事实——归属拒只在 `denied_publish` 加一，
rrd 日志里没有"谁、什么主题、被哪一层拒"；运维看得到在越权却查不出源头。
处置——注册表新增 `rr3_reg_note_deny`（总数照记 + 首见必落一行、同一 (模块,主题)
此后每 64 次再报），行只进 `pending_logs`、由 `rr3_reg_with` 放锁后落盘（registry 锁内
零文件 I/O 的纪律不变）；`conn/publish.rs` 的 `rr3_pub_check` 改走 `rr3_reg_with`。
判据——rrd lib 326 项 + `register_publish` 19 项集成用例全过、clippy `-D warnings` 干净。

**2026-11-27 批82 流水线计数去手抄**：事实——`ci-workflows.md` 正文写"gate 7 /
integration 7 / platforms 6（实例 9）/ publish 3 / buildroot 3 / coverage 1"，同文件
表格写 platforms 8，实测是 6/7/7/4/3/2——同一个文件里三套数。根因是 job 清点被手抄
进正文，而唯一真源本是 `workflows_audit.py` 的 `REQUIRED_JOBS`。处置——正文改为
"条数不在这里抄，真源指过去"，并把这次实测值作为反面教材写进同一段；表格 platforms
修正为 7。判据——⑤ 文档门绿；`workflows_audit.py` 七项全过。

**2026-11-27 批83 平台表五段与段序对齐**：事实——四份 `contracts/platform/*.toml` 段集合
其实齐全（esp32/S3 的 `[controller_wordform]` 是**空表显式声明**，不是缺段），但发现两处
形状漂移：① 树莓派表把 `[construction]` 排在 `[capabilities]` 之前（另三份都在其后）；
② `contracts/README.md` 仍写"每平台一份**四段**"，漏了 `[controller_wordform]`。
另有一条容易误读的差异：无冲突平台的 `conflict = []` 出现在 `[[controller]]` 之前——
那是 TOML"顶层键必须在任何段头之前"的硬约束，不是形状不同。处置——树莓派表按
controller → capabilities → construction → wordform 重排（内容逐字未动）；README 改为
五段并写明段序与顶层键规则。判据——⑤ 文档门绿；rrd lib 326 项全过（平台表由装载链
逐段解析，重排后照常通过）。

**2026-11-27 批84 rrd 判活时钟面复核（无改动）**：事实——逐条查过：判活读数
`rr3_now_ms(t0: Instant)` 走 `t0.elapsed()`（单调，不回拨）；`since_beat_ms` 与
`boot_ms` 全部 `saturating_sub`；注册时刻与判死时刻同一单调轴；头里的
`timestamp_ns` 按契约用 CLOCK_REALTIME（`SystemTime`），早于 Unix epoch 的退化为 0
（不可达，且只影响时间戳字段、不参与判活）。处置——本批零代码改动；结论入册，
避免下一轮重复排查。

**2026-11-27 批85 装配单段形状入册**：事实——十份 `profiles/*.toml` 逐份对表：`[runtime]`
→ `[platform]` → `[supplies]` → `[bindings]` → `[hardware]` → `[enabled_slots]` →
`[slots]` 七段齐全；**唯一缺段是两份 MCU 档的 `[os]`**，classic 档在文件里写了理由，
S3 档没写。处置——`profiles/README.md` 增"段形状"规则（唯一可缺段 = `[os]`，缺省须
写明理由，其余任一段缺即 fail-closed）；`esp32s3-mini.toml` 补上与 classic 同口径的缺省
注记。判据——⑤ 文档门绿；两份 MCU 装配单在 `integration` 的装配演练里照常装载。

**2026-11-27 批86 发布资产集复核**：事实——`publish.yml` 的 `firmware` 腿资产集 =
双 ELF + 双整片 + 双 bootloader + `rr3-partitions.csv` + `FLASHING.md` + `firmware-sha256.txt`
（整片在大步骤后用 `tee -a` 追加进 sha256，oras 推送清单也含它们），与 `tools/deploy/README.md`
的资产表逐项一致；唯一不一致是**推送回执行文**只写了"双 ELF + 双 bootloader + 分区表 +
烧录说明 + sha256"，漏掉双整片——回执与资产集不符会让审阅者以为整片没发。处置——回执行文
补齐"双整片镜像"。判据——`workflows_audit.py` 七项全过；gate / integration 在 `34a6883` 绿。

**2026-11-27 批87 十七个原子件五件套对表**：事实——六模块 `atoms/` 共 17 个原子件逐件
清点：`atom.toml` + 入口 + `mech.rs` + `assemble.toml` + `private.rs` **17/17 齐全**；
唯一形状差异是 `rr3-interact/atoms/face` 的机制件按 500 行线拆成 `mech.rs` + `mech/`
五个子域文件（入口仍单文件）。处置——`modules/README.md` 把这条先例写成规则（子目录只放
机制子域、不放件入口与装配清单），并记录本次对表结论。判据——⑤ 文档门绿；⑯ 行数门绿
（17 件无超线）。

**2026-11-27 批88 `ctl.logs` 应答的单帧上限**：事实——读侧有 1MiB 载荷帽，写侧原先不检查；
环容量是装配单数据（`[runtime].log_ring_cap` 可调大），`tail=0`（不限）时一次查询可能拼出
超帽应答——客户端只看到"帧太长"，查不出该改哪个参数。处置——`rr3_ctl_logs` 在产侧按帽判，
超限即回 `Invalid` 并点名两条缩小手段（`tail=<n>` / `prefix=<子串>`），口径与参数坏形一致；
`rrd-rpc.md` 的 `ctl.logs` 行同步。判据——rrd lib 326 项全过；clippy 干净；
⑨ 元器件的"帧长上限"在产侧与读侧同一常量（`RR3_FRAMING_PLEN_CAP_DEFAULT`）。

**2026-11-27 批89 零拷贝写帧收进协议层**：事实——模块上行（sense 音频每 128ms 一帧）走
`rr3_publish → rr3_body_encode（复制）→ rr3_frame_write → rr3_frame_to_bytes（再复制一整帧）`：
第三个整帧复制在协议层，且 rrd 与模块客户端各有一份零拷贝写法的雏形。处置——rr-ipc 新增
std 面的 `rr3_frame_write_io`（头 128B 栈缓冲 + payload 两段 iovec 一次 `writev`，部分写内部
续投，字节与 `rr3_frame_write` 逐字相同），**rrd 的出向写与模块客户端都改走它**，rrd 侧只留
"出向锁 + 超时分类"这两件策略；`rr3_frame_write` 保留给低频单写者面并在文档写明分工与
原子性前提。判据——rr-ipc / rr-bus-api（11 项）/ rrd（326 项）测试全过、clippy 干净；
integration（含 sim-chain 真 socket 全链）在 `3d2ae04` 绿——线上字节未变。

**2026-11-27 批90 coverage 收进覆盖契约**：事实——`REQUIRED_JOBS` 只钉了五条流水线的 job 名，
`coverage.yml` 两块 job（`workspace` / `independent`）不在表内：它们是**非阻断**观测面，
但两块一起消失时没有任何判据会红——"非阻断"被误读成"可以被悄悄摘掉"。处置——契约表补
`coverage.yml` 两条；`ci-workflows.md` 同口径说明。判据——`workflows_audit.py` 七项全过
（含 job 覆盖契约一项），gate 绿。

**2026-11-27 批91 写出口统一拒超帽帧**：事实——单帧 1MiB 上界此前只有 `ctl.logs` 一处自查，
写出口不判；超帽帧写出去只会换来对端读侧拒收 + 断链，模块只看到"总线断了"。处置——
`rr3_write_bytes` 在写前按同一常量（`RR3_FRAMING_PLEN_CAP_DEFAULT`）判，超帽即回带名字的
错误（"查该应答的产侧是否该分页/限行"），由 `rr3_write` 统一落日志；批88 的点防护收成面防护。
**审计连带修**：既有 `write_counted` 用例拿 4MiB 帧"赌缓冲满"——那种帧生产里根本发不出去，
测的是"超帽被拒"而不是"对端不收"；改成 64KiB 帧写满 socket，仍走慢消费者那条路。
判据——rrd lib 326 项全过、clippy 干净。

**2026-11-27 批92 版本读法收成一处**：事实——`stage-image.sh` 与 `build-riscv-image.sh` 各自
`cat VERSION`，`release-tag.sh` 又内嵌一份 X.Y.Z 校验：VERSION 一旦带后缀（rc/日期），
拼名与 tag 判定就会各说一套。处置——`release-tag.sh` 抽出 `rr3_version_value`（trim + 三段
校验 + 点名），发布执法与两个部署脚本共用；两个脚本改为 source 该库（路径相对仓根，
脚本本来就 `cd` 到仓根）。判据——`release-tag.sh --self-test` 六项全绿；
`bash -n` 两个脚本通过；⑬ 仓面卫生绿。

**2026-11-27 批93 bridge 背压口径复核（无改动）**：事实——`rr3-bridge` 是零解析字节中继，
UDS 未就绪/断链期间**不缓冲、直接丢弃并计数**（`link/stat.rs` 只在进入丢弃期打一行、恢复时
报累计），与 rrd "满即丢帧、保连接"同一口径；`link/tests.rs` 已覆盖"断连窗口丢弃 + 恢复后
续传"。处置——零改动，结论入册。

**2026-11-27 批94 SKIP 面收口**：事实——七把平台门都走 `tools/ci/lib/skip.sh`（CI 内 SKIP
即红），只有 `ipct003.sh` 因为要跑完四路再汇总而自己判 SKIP，靠调用方记得加 `--strict`：
漏加一次就是"零覆盖但流水线绿"。处置——汇总段补 `CI=true` 条件（与 `--strict` 同判同码），
`--strict` 语义不变；`ci-workflows.md` 同口径注明。判据——`bash -n` 通过；
`skip.sh --self-test` 与 `release-tag.sh --self-test` 在 gate `scripts` job 里照常执行；
gate / integration 在 `2152fcd` 绿。

**2026-11-27 批95 日志查询按尾取**：事实——`rr3_log_filter` 在 `tail>0` 时先全量
`cloned()` 再 `drain` 头部：环容量是装配单数据（可调大），`tail=10` 配 8192 行环会白拷
整个环。处置——改为从环尾 `rev().take(tail)` 收再反转，只复制 tail 条；`tail=0`（不限）
路径不变。判据——rrd lib 326 项全过（含日志环查询用例），行为逐字不变。

**2026-11-27 批96 版本化部署包 A10 契约立节**：事实——`pack-bundle.sh` 与
`install-bundle.sh` 互相引用"A10 打包契约"，而 `grep -rn A10 docs` 零命中：契约只活在
两个脚本的注释里。处置——`tools/deploy/README.md` 立"版本化部署包契约"节，三条对齐项
（顶层目录名 `rockrobot3-<commit>` / `BUNDLE_VERSION` 五个键必在顶层 / 安装失败回树**且**
回二进制），并点明两侧实现单点。判据——⑤ 文档门绿；三条逐条能对上脚本行（顶层校验、
`grep -qx .../BUNDLE_VERSION`、`rollback()` 的 binbk 段）。

**2026-11-27 批97 sense 上行失败语义复核（无改动）**：事实——`rr3_pcm_pub` 的发布失败
经 `?` 上抛到 `serve`，模块打印 `发布 <topic> 失败: …` 后 exit(1)，由 supervisor 按
restart_policy 重启——这是 rr3mod 写明的"模块不装活着"语义，不是静默；rrd 重启窗口里
桥侧丢帧、模块侧重连注册，两条路径各自有名点。处置——零改动，结论入册。

**2026-11-27 批98 gate 自检面复核（无改动）**：事实——`tools/ci/audit/*.py` 里带
`--self-test` 的七支（unsafe / baselines / commit_msg / gate_steps / workflows /
metrics / lockio）与 `lib/{log-digest,skip,release-tag}.sh` 三支自检，在 `gate.yml` 的
`scripts` job 里**全部有调用点**，无"有自检没人跑"的死件；`bash -n` 全量语法检查与
actionlint 也在同 job。处置——零改动，结论入册（避免下轮重复排查）。

**2026-11-27 批99 广播过滤待裁项入册**：事实——`fanout_shadow*` 三键（0.0.8c 起）一直在
记账"按 manifest `subscribes` 本应过滤多少目标"，但它要回答的那个决定（扇出要不要改成
过滤投递）在 `docs/process/decisions.md` 里**没有行**——机制在跑、决定无主，正是 L-33 的病形。
处置——§3 补一行：缺**裁定**、缺省态=维持全量扇出（模块侧自行丢弃）、触发=某主题
`fanout_shadow/<topic>` ÷ 该主题发布帧数 ≥ 在册连接数一半，或链路预算被广播面吃掉；
标题计数与预算段同步（§3 余 2 条：评审 1 + 裁定 1）。判据——⑤ 文档门绿；六列齐全。

**2026-11-27 批100 rr3-ctl 错误面复核（无改动）**：事实——错误信封渲染单点在
`rr3_ctl_reject`：`cmd 被拒（code N）：msg`，缺字段以 `?` 占位（"被拒"这一事实比字段完整度
重要），码与文案原样透传、不改写不吞；ota/reboot 两条恒拒回显有 fake-rrd 真 UDS 往返用例。
处置——零改动，结论入册。

**2026-11-27 批101 RK3566 派生链对拍（无改动）**：事实——用脚本把
`profiles/default.toml` 的 6 条 `[[pin]]` 与 `contracts/platform/rk3566.toml` 对拍：
控制器引用（`pwm0`/`gpiochip0`/`i2c1`/`spidev0`）**全部在册**，零缺失；四条 `[controller_wordform]`
前缀与各自控制器名逐一对齐（`pwm`/`gpiochip`/`i2c`/`spidev`）。处置——零改动，结论入册。

**2026-11-27 批102 判死边界用例**：事实——`rr3_reg_kill` 的判据是 `elapsed >= watchdog`，
但既有用例只覆盖"超窗"（600 vs 500）与"喂狗后 1ms 不判死"，**恰好相等那一毫秒没有钉住**：
`>` 与 `>=` 的差别决定"准点判死"还是"多等一拍"。处置——新增 `expire_boundary_inclusive`：
`499 放行 / 500 判死`，并断言回执的 `(elapsed, watchdog) = (500, 500)`。判据——rrd lib
327 项全过；gate / integration 在 `1ce8432` 绿。

**2026-11-27 批103 覆盖矩阵 ESP 判据列补全**：事实——矩阵里 ESP 两行的判据只写"五门构建/对拍"，
审阅者无法知道是哪五门，也看不出"SKIP 在 CI 内即红"这条纪律在这里怎么落地。处置——两行判据
逐条点名：`{chip}-periph-base`（外设底座编译）/ `esp-uart-echo`（板级回环夹具）/ `espidf-std`
（构建链冒烟）/ `espidf-sentinel`（机制核真板消费者）/ `{chip}-act-sense`（执行+感知编译），
均经 `guarded-run` 且按 `skip.sh` 口径 CI 内 SKIP 即红；整片装配的三点 magic 与尺寸断言另列；
S3 行补 release/debug 两份固件产物。判据——⑤ 文档门绿；五门名与 `platforms.yml` 的五个
`guarded-run` title 逐字对上。

**2026-11-27 批104 `RR3_FAN_CAP` 内存账复核（无改动）**：事实——常量注释的算账
（128 帧 × 音频片 ~11KB ≈ 1.4MB/连接；× 16 连接上限 ≈ 22MB）与代码常量一致：
`RR3_FAN_CAP = 128`、`RR3_REG_MAX_MODULES = 16`；11KB 也站得住（2048 帧 × 2ch × 2B = 8KB
PCM → b64 ≈ 10.9KB + 信封）。处置——零改动，结论入册（避免下轮重复核）。

**2026-11-27 批105 rock3 容量注入链复核（无改动）**：事实——`genimage.cfg` 六个容量位
（boot/rr_a/rr_b/rr_factory/rr_staging/rr_data）+ `@TARGET_DIR@` 全为占位符；`post-image.sh`
逐个注入（白名单 `<数字>[KMG]`，非法即拒不在 sed 里揉坏值），并在产出文件上做
`@[A-Z_]*@` 残留检查——"容量不在本层冻结"这条设计真的落地了。处置——零改动，结论入册。

**2026-11-27 批106 buildroot 发布面判据复核（无改动）**：事实——`image` 腿六分区断言
（`boot/rr_a/rr_b/rr_factory/rr_staging/data`）、debugfs 查 `/usr/bin/rrd` 与 `S99rrd`、
`/etc/fstab` 四键、`sha256sum` 清单、`release` 腿 `refs/tags/v` 门 + `packages: write` +
oras 推 `rockrobot3-ota` / `rockrobot3-buildroot`——与 `ci-workflows.md` 的发布面判据逐条一致。
处置——零改动（同批另补 AMD 覆盖边界一句，见下条）。

**2026-11-27 批106 附 AMD 覆盖边界如实登记**：事实——矩阵里"英特尔/AMD 同属 x86_64、一条腿
覆盖两家"容易被读成"两家都验证过"。处置——`ci-workflows.md` 补一句边界：该腿跑在
`ubuntu-latest`（x86_64 宿主）上，**AMD 没有独立 runner 回执**，"AMD 上能跑"今天的证据只到
"指令集与内核 ABI 相同"这一档。判据——⑤ 文档门绿；不新增腿（同一 target 造两条重复腿没有
判据意义）。

**2026-11-27 批107 补丁台账与实际位置订正**：事实——`buildroot/patches/README.md` 写
"本目录承载补丁…现存一件"，而 `ls` 该目录只有 README：补丁体按 Buildroot 惯例在
`board/radxa-zero3/patches/`（defconfig 的 `BR2_LINUX_KERNEL_PATCH` 指过去）。读者按台账
去找补丁会扑空。处置——台账开头改成"**只放台账，不放本体**"并点明本体路径与 defconfig
指向；核对清单同一句同步。判据——⑤ 文档门绿；`find -name '*.patch'` 与台账"现存一件"
逐字对上。

**2026-11-27 批108 主题契约静态对账（无改动）**：事实——把六模块 `module.toml` 的
`[events].publishes/subscribes` 与 `contracts/topics.toml` 做全表交叉（一次性脚本，
不新增门禁）：**零缺失**——没有任何模块申报了契约里没登记的主题。处置——零改动，
结论入册（运行期越权发布本来就由归属门拒并计数，静态面这次先查过一遍）。

**2026-11-27 批109 OTA 设计档过期说法订正**：事实——`分区布局与OTA槽位.md` 写
"rr3-ota-ctl 已不在仓"，而 `base/ota/rr3-ota-ctl` 在仓、`publish`/`buildroot` 两条流水线
都在构建它——同一句同时与仓内实况和流水线矛盾。处置——改为"**在仓**，缺的是板上消费
运行态"（与 `rr3.ctl.ota` 恒拒的实况一致）。判据——⑤ 文档门绿；`ls base/ota/rr3-ota-ctl`
可见。

**2026-11-27 批110 探针集清点（无改动）**：事实——`tools/pi-periph-probe` 17 支 `.py`，
README 逐个点名 17 支；`grep -rln 'TODO|deprecated|legacy|旧路径'` 零命中。处置——零改动
（没有证据支持"某支已退役"就不删，删了等于毁证据链）。判据——README 与文件名逐一对上。

**2026-11-27 批111 影子统计派生比值（不落代码，理由入册）**：事实——§3 那条待裁项要的
"本应过滤连接数 ÷ 发布帧数"是**读数**不是**计数**：`fanout_shadow_conns`（帧×连接）与
`rr3-sense/pcm_frames`（发布帧数）都已在同一张 counters 表里，运维一次相除即得。处置——
**不新增派生计数器**：加一个"分母随主题而变"的比值键会把同一条判据变成两套口径（谁算、
什么时候算、除零怎么表示），正是"第二真值"病。§3 行内的算式已足够可判，评审时直接取数。
判据——零代码改动，读数路径（`rr3.ctl.counters`）与 §3 行逐键对上。

**2026-11-27 批112 装机漂移门扫描面复核（无改动）**：事实——`verify-board-config.sh`
不写死件数：模块 config、`profiles/*.toml`、`units/*.service`、板级资产四类全是**运行时
glob 枚举**，并自带"16 件静默缩成 2 件"的守卫（注释与实现同处）；`--profile` 只比该机型
装配单与其板级资产。新加一份装配单/unit 自动进扫描面，不存在"漏进扫描面"的洞。处置——
零改动，结论入册。

**2026-11-27 批113 可达终点表 30 格对表（6 格改判）**：事实——该表自批N20（2026-10-23）
后未按其 §4 维护纪律逐批更新；2026-11-16/22 真板会话（[0012](../evidence/实测/0012.md)、
[0016](../evidence/实测/0016.md)）已使树莓派行五格"不能（无载体）"的缺载体理由失效；格内
所引 `rr3_manifest_install_rewrite` 已迁入 rrd 并改名。处置——改判树莓派 出字/转动/拾音/
播放/标定 五格为"能做到"并逐格挂证据；表头"板当前离线"订正为板在线；RK3566 烧录格依据句
订正为委托 `rr3-pack --install-root`；符号改名订正；30 格逐格复核（6 格改判、24 格维持）。
覆盖盲区如实登记：点验矩阵七列与十项不同分母，30 格无自动化腿直接判态，生成器未建前靠
改判纪律兜底。判据——⑤ 文档门绿；⑯ 硬线 0；全表路径解析无缺。

**2026-11-27 批114 语义漂移巡检（退役路径与符号名）**：事实——W-A 分层后六份模块设计册与
7 处原子件头注仍引 `src/composite.rs`；3 份事件 schema 描述同；`rr-periph-proto` 两处 schema
名用下划线而仓内实为连字符（原引用不存在）；`status.md` 事件 schema 行写 3 个、实为 8 个；
`ci.yml` 已按职责拆六条流水线而多处仍引旧路径。处置——设计册加结构订正注（boot/rpc/
dispatch 对应关系）；原子件头注改指当前文件，`rr3_relay_apply_error_text` 改名
`rr3_relay_err_text` 同批订正；schema 描述改当前实现；schema 名订正为实名；status 行补
5 份；`openitems.toml` 与 `check.sh` 的 ci.yml 引用改指 platforms.yml / gate.yml；
conventions 加订正段。判据——⑤ 绿；⑬ 五条 PASS；⑫ 各项 PASS；8 份 schema JSON 可解析。

**2026-11-27 批115 entry 装机改写单点对表（无第二套语义）**：事实——post-build.sh 已无
第二份 entry 改写，统一委托 `rr3-pack --install-root`；全仓装机改写实现只有
`rrd::manifest::rr3_manifest_rewrite` 一处（rr3-pack 与 sim 演练共用）；运行时的
`rr3_entry_absolutize` 只造注册帧 wire manifest、不写文件，rrd spawn 取装机 manifest 原文。
处置——澄清运行时/装机两条路的分工（不是第二实现）；sim 两处"三道门"统一为"四道门"
（形制/恰一条/旧值对账/重解析复核），与实现阶段、测试面一致。判据——全仓 rg 仅一处
装机改写实现；三调用点（pack stage、sim layout）同源；⑤ 绿。

**2026-11-27 批116 rr3-ctl 九方法入口覆盖表**：事实——rrd ctl 九方法、CLI 八子命令；
`domain.set` 无 CLI 入口（唯一入口＝面板 `POST /api/profile/domain`）；`ota` 只有拒因渲染、
无成功渲染器（rrd 恒拒，成功分支不可达）；面板入口实测为 status/health/data/logs/wiring/
selftest/reboot/domain 七路。处置——[rrd-rpc §3](../reference/rrd-rpc.md) 增逐方法入口表，
登记 ota 成功渲染器缺口（触发条件＝M6 接入下载链）；status.md rr3-ctl 行同步指向。
判据——⑤ 绿；`command_whitelist`（8）与 `RR3_RPC_NAMES`（9）两张 pin 表逐条对上，
`domain.set` 差额有理由。

**2026-11-27 批117 S3 装配单补接线档（可装载）**：事实——`profiles/esp32s3-mini.toml`
自 W1/T2 落树后一直没有 `hardware/esp32s3-mini/wiring.toml`：按 `rr3_truth_paths` 推导，
该档装配必缺件，CI 从不装配该档故长期未暴露（0.0.8 单板全链计划 §批45 盲区①）。处置——
补接线档（J0 电源入 feeds J1/J2 + 两条 SG90 线组，信号写 `act.axis.pwm.pan/tilt`）与双轴
档案（pan [60,120] / tilt [80,100]，0009 号 S3 全行程实测覆盖同区间）；引脚真值与
`board/pins_s3.rs`、装配单 `[hardware]`/`[[pin]]` 三处同源。判据——新增
`esp32s3_boot_drill` 过完整装载门、boot 队列两模块、板侧十一键进注入文本、双轴档案按
缺省轴投影；drills 8 条全绿（`/tmp` 产物，源码区零残留）。

**2026-11-27 批118 S3 描述符实名统一**：事实——同一块 S3 板在文档/证据里有两个名字：
`esp32s3-solo`（CI 产物名、证据 frontmatter、计划 W6 原写 `esp32s3-solo.toml`）与
`esp32s3-mini`（真实落树的装配单与 hardware 目录、architecture 机型集、contract pin）。
处置——**描述符唯一实名 = `esp32s3-mini`**；"solo" 降为形态标签（单板全链运行方式），
不另立 `esp32s3-solo.toml`（两个名字会在装载面变成第二真值）。计划 §3.2/W6 加订正注，
装配单头注登记命名口径。判据——⑤ 文档门绿；`ls profiles/esp32s3-solo.toml` 仍缺席且
不再被任何步骤引用为待建文件。

**2026-11-27 批119 通用底座计划现状盘点订正**：事实——该计划 §2 的"现状盘点"四项已
过期：五端口 trait 已迁 `rr-periph-proto::ports`（`pub`，P0-A1）、本地 `ports.rs` 已删；
S3 已装配并出全链证据；Pi 宿主与 Pi 当板均已兑现。处置——逐条加"2026-11-27 订正"注，
原计划行保留；§8.2 A1 标"已完成"。判据——⑤ 绿；`base/runtime/rr-periph-proto/src/ports.rs`
五 trait 均 `pub`，`tools/esp32-periph-base/src/ports.rs` 零命中。

**2026-11-27 批120 分压比打印口径根修**：事实——`board.rs` 的
`ADC_RAIL_DIVIDER_NUM/DEN` 已是 2:1，而 `boot.rs` 启动横幅写死"占位分压 1:1"、
README 两处仍写 1:1——运维读到的分压比是假数，标定时会按错比例换算（假读数比缺读数更坏，
计划 §批45 盲区③）。处置——横幅改为从常量渲染 `分压 N:D，接闸 armed=<bool>`（打印不再
有第二真值可漂）；README 两处同步订正为 2:1 已回填、阈值仍占位。判据——`cargo fmt --check`
过；⑯ 硬线 0；常量与打印同源（rg 全仓不再有写死的"1:1"分压文案）。

**2026-11-27 批121 default↔distro 接线镜像机械对账**：事实——两档接线档声称"同板同接线
镜像"，但一致性只靠人工纪律（计划 §批45 盲区②）；实测两者差异只在注释。处置——新增
`wiring_mirror_drill`：两档 TOML 解析值必须相等，注释外任何差异即红。判据——drills 8 条
全绿；`profile::tests::drills::wiring_mirror_drill` 在 CI gate ①b 内真跑。

**2026-11-27 批122 分压比与接线档事实订正（含 gate 漂红根修）**：事实——①敞口 12 仍写
`ADC_RAIL_DIVIDER_NUM/DEN` = 占位 1:1、三形态"没有任何机器可查的接线表"，而常量已回填
2:1（S3 10k/10k 实测档）、三形态 wiring.toml 均已落树；②`periph-mech` 的 rail 换算注仍
称"占位 1:1"；③批121 推送后远端 gate 红：`git push` 前 `cargo fmt --check` 的退出码被管道
吞掉，`cargo clippy -D warnings` 抓出 `esp32s3_boot_drill` 头注第二行以 `+` 开头被 Markdown
当列表项（doc_lazy_continuation）。处置——①②按现状订正（未做的仍是万用表 ≤2% 对拍与接闸，
判据不变）；③改写头注去列表前缀，本地复跑 `cargo clippy --workspace -D warnings` 零告警后
推送。判据——`gate`/`integration`/`platforms` 在 `67fe2f0` 全 success（platforms 含
esp32/esp32s3 双芯片固件编译，验证 boot.rs 打印改动在两目标可编）；⑤ 文档门绿。

**2026-11-27 批123 esp 门前置收敛 + S3 补 clippy 面**：事实——classic 与 S3 两条 esp 门
各有约 65 行前置复制体（工具链/目标/IDF/esp-clang/镜像/venv），且复制造成真实漏面：
S3 门没有 classic 自 2026-11-21 就有的 clippy `-D warnings` 腿——同一 crate 的
`cfg(feature = "esp32s3")` 分支此前零告警覆盖。处置——新增 `tools/ci/lib/esp-env.sh`
（`rr3_esp_ready` / `rr3_esp_env` / `rr3_esp_preflight` / `rr3_esp_clippy` 四个函数，
各 ≤50 行），两门只保留各自构建腿；S3 门补 clippy（`esp32s3,autonomy,tts` all-targets，
同一门体不是新门）。判据——`platforms` 在 `361118d` 上 `esp32-gates / ESP32（classic）` 与
`esp32-gates / ESP32-S3` 双 success（S3 clippy 首跑即过）；`bash -n` 三门全过。

**2026-11-27 批124 流水线 job 表对账（无改动）**：事实——`workflow_release.py` 的
`REQUIRED_JOBS`（gate 6 / integration 7 / platforms 7 / publish 4 / buildroot 3 /
coverage 2，共 29 名）与六份 YAML 的实际 job 名逐名相等，无漏登记、无死条目。
处置——零改动。判据——`python3 tools/ci/audit/workflows_audit.py` PASS。

**2026-11-27 批125 ghcr 四包匿名复核**：事实——运行镜像 10 个 tag（`latest`、`edge` 四枚、
`v0.0.8` 三架构、`v0.0.8`、一条 `.sig`）与 `VERSION=0.0.8` 同源；firmware 包 `v0.0.8` 在册，
但仍留 v0.0.17–v0.0.23 共 11 个治理前旧 tag；OTA/buildroot 各只 1 个 `0.0.8`（首发命名，
status 正文已述）。处置——事实写进 status 发布段；旧 firmware tag 清理需 ghcr 写权限
（PAT/包设置），登记为凭证窗口项，不冒充已清。判据——匿名 token + tags/list 200，四包
响应落回执。

**2026-11-27 批126 五芯片覆盖复核（无改动）**：事实——`cross-artifacts` 三腿（x86_64 原生
覆盖英特尔/AMD、aarch64-musl 覆盖瑞芯微、riscv64-gnu+qemu 覆盖进迭时空）＋`esp32-gates`
双芯片腿；每腿三件事同口径（产物构建 + 目标 ISA 真执行 + 产物上传），`CHIP_MATRIX` 由
流水线审计钉住删除/改名。处置——零改动；Intel/AMD 共用 x86_64 ISA 腿的边界已在
ci-workflows 明写（厂商级差异不在 CI 可判面）。判据——`platforms` 三腿 + 双芯片腿在
`361118d` 全 success。

**2026-11-27 批127 macOS 腿依赖预取有界重试**：事实——`361118d` 的 platforms 唯一红是
`host-macos`：注解原文 `Could not resolve host: index.crates.io`（cargo 自带 retry 不覆盖
解析失败），同拍 esp32 双芯片与其余八腿全绿——网络瞬态而非代码失败。处置——新增
`tools/ci/lib/fetch.sh`（`cargo fetch --locked` 三次退避重试，末次失败才点名退出），
workspace 与独立工程两段都先预取；ci-workflows 缓存面口径补记。判据——`platforms` 在
`10bc4e7` 全 success（macOS 腿转绿），gate/integration 同头亦 success。

**2026-11-27 批128 三条 espidf 门前置收敛**：事实——`espidf-std` / `espidf-sentinel` /
`rr3-act-sense-espidf` 三门各带一份 40+ 行前置复制体（工具链/rust-src/ldproxy/IDF/
esp-clang/镜像/venv），与批123 收敛的两条 periph 门同源。处置——`tools/ci/lib/esp-env.sh`
拆出 `rr3_esp_toolchain`（工具链四查）与 `rr3_esp_idf`（IDF 克隆+esp-clang），
`rr3_esp_ready` 保留兼容；三门改调 `rr3_esp_preflight`，act-sense 的目标按芯片档选、
模块内 venv 兼容改调 `rr3_esp_env`。裸金属门 `esp-uart-echo` **不套**公共前置并在头注
写明原因：它不需要 rust-src/IDF/esp-clang，链接面是 Xtensa esp-gcc，硬套会把缺件误判成
整门 SKIP。判据——`platforms` 在 `9b582b8` 全 success（含 esp32/esp32s3 双芯片腿）；
四门 `bash -n` 全过，SKIP 文案逐门核对无「门 门」重复；行数 93→52 / 93→58 / 132→106。

**2026-11-27 批129 edge 与发布点 digest 复核（无改动）**：事实——匿名 manifests 实测：
`latest` 与 `v0.0.8` 同指 `sha256:07d189…`（OCI index，三架构 + 3 条 attestation
`unknown/unknown`）；`edge` 指 `sha256:39d8ee55…`（另一 index，同三架构）——edge 未与
发布点混指，`latest` 只随正式 semver 更新。处置——零改动（status 发布段已述，本次为
复核回执）。判据——三 tag 的 Docker-Content-Digest 与平台集合逐条对上。

**2026-11-27 批130 buildroot 镜像内容断言收为脚本**：事实——`buildroot.yml` 的
"镜像内容只读断言"是 57 行内联体（e2fsck/卷标/rootfs 内容/FIT+TPL 四组混在一段），
超过 50 行审查阈值且只存在于 YAML 里。处置——抽 `tools/ci/platform/
buildroot-image-check.sh`：`rr3_br_fsck` / `rr3_br_rootfs` / `rr3_br_fit` 三函数各 ≤35 行，
错误标题（br-fs/br-fsck/br-label/br-tool/br-rootfs/br-fstab/br-fit/br-tpl）与文案逐字保留，
workflow 收为一行调用。判据——`buildroot` 在 `9b582b8` success（真镜像上执行抽取后的
脚本）；缺镜像路径本地实测按 `br-fs` 退 1。

**2026-11-27 批131 五芯片腿版本钉对表（无改动）**：事实——六条流水线共 24 处 Rust
工具链全部钉 `1.100.0`；esp 面 espup `v0.17.1`（含 sha 下载与三次重试）、ESP-IDF
`v5.2.2`、espflash `4.6.0` + sha256 单点；缓存键分别按 `Cargo.lock` 与工具链版本。
处置——零改动。已知边界：riscv64 apt 交叉包（`gcc-riscv64-linux-gnu` 等）未钉版本，
属 runner 镜像面而非仓内配置，登记为诚实边界不冒充已钉。判据——`grep 'toolchain: "1.100.0"'`
六表计数一致；esp 三件钉版在 action 内单点。

**2026-11-27 批132 合并清单平台判据抽为可自检脚本**：事实——`publish.yml` 的 merge 步
内嵌 21 行 python heredoc（经 `docker buildx imagetools inspect --raw` 取清单、数平台集、
排除 attestation），判据只在 tag 发布时才跑到，仓内无法单测。处置——抽
`tools/ci/platform/manifest-platforms.py`：`platforms()` / `check()` 纯函数、
`--self-test` 三例（三架构+attestation 放行 / 缺架构拒 / 非 index 拒）；workflow 改为
raw dump + 脚本调用，判据语义与错误标题（`manifest-platforms`）逐字不变。判据——
自检三例全 PASS；实网 `v0.0.8` index（3 架构 + 3 条 attestation）判 PASS；
`workflows_audit` PASS；gate/integration/platforms 在 `5582681` 全 success。

**2026-11-27 批133 三架构 attestation 计数复核（无改动）**：事实——匿名逐条取
`v0.0.8` 与 `edge` 的 index：各 3 条 attestation manifest；逐条取其 manifest，
**每腿 2 层 in-toto**（provenance + SBOM，amd64/arm64/riscv64 三腿均 2）。处置——零改动，
满足发布面"每腿 attestation ≥2"的既有判据。判据——逐腿层数与 `vnd.docker.reference.digest`
反查的架构名对表。

**2026-11-27 批134 文档门按判据拆组（函数 62→15 / 52→9）**：事实——`tools/ci/steps/5.sh`
的 `check_docs` 62 行、`check_docs_selftest` 52 行，均超 50 行审查阈值，且一段里混装
链接/导航/散落三组判据。处置——拆 `docs_links` / `docs_nav` / `docs_scatter` 三组
（各 12/21/21 行）+ 15 行装配；自检拆夹具 `rr3_docs_case`、断言 `rr3_docs_expect`、
核心六例、豁免三例、总入口五函数（最长 22 行），夹具与断言提到文件作用域共用。
判据——真仓 `check_docs` 绿、自检九例全过、`gate_steps_audit` 29 步清单逐字一致；
gate/integration/platforms 在 `5582681` 全 success。

**2026-11-27 批135 固件发布集装配收为脚本（可本地干跑）**：事实——`publish.yml` 的
firmware 块内联 25 行做"四件拷贝 + 分区 spool 断言 + bootloader magic 断言 + sha 清单"，
只在 tag/main 触发时才执行，仓内无法验证。处置——抽 `tools/deploy/fw-set.sh`：
`--from <c-elf> <s-elf> <c-boot> <s-boot> [--out <dir>]` 装配七件集 + 三道判据
（ELF 齐 / 分区表含 spool / bootloader 首字节 e9）+ sha 清单；`--out` 供本地干跑不污染
工作区，sha 工具 Linux `sha256sum` / macOS `shasum -a 256` 双档；workflow 只留构建与
bootloader 定位，错误标题（fw-build/fw-elf/fw-boot/fw-partitions）不变。判据——本地干跑：
正例七件齐且 `firmware-sha256.txt` 生成（rc=0），bootloader 非 e9 反例按 `fw-boot` 退 1；
`workflows_audit` PASS；gate/integration/platforms 在 `d545adaf` 全 success。
**证据边界**：publish 流水线只在 tag/main 触发，本仓暂无发布窗口——脚本逻辑由本地干跑
验证，发布面真跑归下次正式发布。

**2026-11-27 批136 集成腿脚本体量审计（无改动）**：事实——`distro-smoke.sh` 160 行、
`sim-chain.sh` 60 行、`sim-assemble.sh` 24 行、`rk3566-onboard-check.sh` 86 行；均为线性
编排或单函数（rk3566 仅 `check_file` 3 行），无 >50 行函数、无 >500 行脚本。处置——零改动。

**2026-11-27 批137 门禁步骤面函数长度复核（无改动）**：事实——对 `tools/**/*.sh` 全量
函数扫描（列 0 收尾口径）：仅 `tools/ci/steps/5.sh` 曾有两函数超 50 行（批134 已拆，
现最长 22 行）；其余步骤体与平台脚本均 ≤50。处置——零改动。

**2026-11-27 批138 下次发布 tag 规范确认（无改动）**：事实——`buildroot.yml` 的 OTA 与
buildroot 包推送标签已是 `v${VER}`（行 425/426），`VER` 来自 `release-tag.sh` 与 `VERSION`
同源；firmware 包用 `${{ github.ref_name }}`（正式 tag = `vX.Y.Z`）；运行镜像 tag 由
release meta 生成。下次发布不会重演首发无 `v` 前缀的疏漏。处置——零改动，登记为发布前
复核结论。判据——三处 tag 生成式逐行对上 `VERSION` 与 semver 正则。

**2026-11-27 批140 S3 真值集消费验证（无改动）**：事实——`rr3_truth_names` 按
`profiles/<stem>.toml` + 契约四册 + `contracts/platform/<chip>.toml` +
`hardware/<stem>/wiring.toml` 生成成员，`dirs` 为 `hardware/<stem>/axis`；对
`esp32s3-mini` 逐个实测八名成员全在树（批117 补的 wiring 与双轴档正是被它消费）；
`rr3-pack` 的 profile 测试（esp32-mini 夹具：files 直落 + dirs 整目录随装 + 缺册拒包）
3 条全绿，证明同一机制按 stem 泛化。处置——零改动。**结论**：批117 之前该档打包必
拒（缺 wiring），补齐后真值集与打包面自动吃进，无需第二份文件清单。

**2026-11-27 批141 ci-workflows 文档与流水线逐项对表（无改动）**：事实——job 数
（gate 6 / integration 7 / platforms 7 / publish 4 / buildroot 3 / coverage 2）、
`paths-ignore: docs/**,**.md`（integration/platforms）、publish `tags v* + branches main`、
buildroot `branches main/dev** + paths 白名单 + tags v*`、五条流水线并发组、
工具链统一 1.100.0、distro-smoke 无宿主工具链/无 rust-cache——逐条与文档一致。
处置——零改动。判据——六份 YAML 逐段读回与 ci-workflows 对应段对表。

**2026-11-27 批142 firmware 出口补版本门（真缺口修复）**：事实——`release-tag.sh` 已被
publish 的 build / build-riscv64 / merge 与 buildroot release 调用，但 `firmware` job
与 build 腿**并行**且只判 `refs/tags/v`：一个与 `VERSION` 不同源或未指向 release 提交的
`v*` 标签会让 build 腿红、firmware 腿照推——发布面版本纪律存在绕过路径。处置——在
firmware job 的 checkout 之后补一步调同一 `release-tag.sh`（不引第二套判据），四个出
制品出口全覆盖；ci-workflows 执法点描述同步。判据——`release-tag.sh --self-test` 六例
全过；`grep -c 'release-tag.sh "${...event_name}..."' publish.yml` = 4（build /
build-riscv64 / merge / firmware）；`workflows_audit` PASS。发布面真跑归下次正式 tag。

**2026-11-27 批143 S3 真值集成员机械钉**：事实——批140 只做了静态对表（成员名单由
`rr3_truth_names` 生成、八名文件在树），没有用例保证"真仓 S3 档打包时这些成员不会
缺"。处置——`tools/rr3-pack/src/tests/profile.rs` 新增 `s3_truth_members_exist`：
读真仓 `profiles/esp32s3-mini.toml` → `rrd::profile::rr3_profile_parse` →
`rr3_truth_set` → 逐成员断言 `is_file` / `is_dir`（不引第二份清单）。判据——本地
`cargo test -p rr3-pack profile` 4 条全绿；gate ④ 同测试集在 CI 真跑。

**2026-11-27 批144 dispatch 手输标签旁路封堵**：事实——`release-tag.sh` 对
`workflow_dispatch` 按非发布事件处理（手输标签原样给，不判 VERSION 同源），而
firmware job 的 `if` 只判 `refs/tags/v`：在 tag ref 上手触 dispatch 时，版本门
（批142 补的那步）会原样放行输入标签，firmware 仍可推——残留旁路。处置——
`firmware.if` 增 `github.event_name == 'push'`：发布 artifact 只从 tag 推送出，
手动窗口只服务 edge 镜像。判据——`workflows_audit` PASS；`release-tag.sh --self-test`
六例仍全过（dispatch 语义不变，改的是 job 入口条件）。

**2026-11-27 批145 deploy README 与发布集产物名对表（无改动）**：事实——
`tools/deploy/README.md`（即随包 FLASHING.md）列出的七件与 `fw-set.sh` 产物名逐字一致
（两只 ELF / 两只 bootloader / `rr3-partitions.csv` / `firmware-sha256.txt` / FLASHING.md），
烧录命令与 `flash-artifact.sh` 参数对得上。处置——零改动。

**2026-11-27 批146 S3 线束文档与真值对表（无改动）**：事实——`tools/deploy/README.md`
是**制品级**入口（镜像/ELF/烧录命令），不承载引脚表；S3 引脚真值三处同源：`board/pins_s3.rs`、
`profiles/esp32s3-mini.toml [hardware]/[[pin]]`、参赛介绍 §S3（41/42、8/9、4/5、6/15/7、
ADC1 GPIO1）——逐组相等。处置——零改动（README 不抄引脚是避免第二真值的正确分工）。
判据——三处逐键对读。

**2026-11-27 批147 五芯片部署文档覆盖复核（无改动）**：事实——`tools/deploy/README.md`
平台表五档齐（ESP32 classic / ESP32-S3 / RK3566 buildroot / RK3566 distro / 英特尔-AMD x86_64 /
进迭时空 riscv64），每档有制品、入口、验收判据；`base/os/distro/boards/` 只收**物理板**
笔记（Pi 4B / RK3566），x86_64 与 riscv64 是镜像/容器部署面，不属板卡笔记。处置——零改动。
判据——平台表逐行与 `platforms.yml` 五芯片矩阵对照。

**2026-11-27 批148 OTA 包内容与分区文档对表（无改动）**：事实——匿名拉
`rockrobot3-ota:0.0.8` manifest：6 层 = 镜像 / `.manifest` / `.sig` / `keys/8570b80b2a0f82c3.pub` /
`checksums.txt` / `bundle.txt`，与 `ota-bundle.sh` 头注"四件 + 两清单"逐名一致；公钥包内路径
`keys/<key_id>.pub` 与文档"安装树 `ota/keys/<key_id>.pub`"是同一物的打包/安装两态。处置——
零改动。判据——层名与脚本产物清单逐条对上。

**2026-11-27 批149 漂移门补 hardware 真值面（真缺口修复）**：事实——
`verify-board-config.sh` 的比对清单只扫模块 config、`profiles/<name>.toml`、unit 与
`boards/<profile>/etc/**`，**漏掉 rr3-pack 随包的 `hardware/<stem>/wiring.toml` 与
`axis/*.toml`**——这两类文件 rrd 装载期直接消费，漂了即拒 boot，而漂移门看不见
（"门绿而 wiring 可漂"）。处置——`rr3_deploy_manifest` 增 hardware 两条目（指定
`--profile` 只扫该档；未指定扫全部），并按 50 行阈值把 55 行函数拆为六函数；
自检夹具补 wiring/axis，新增两例（wiring 漂移红 / 轴档漂移红）。判据——自检 9 例全过、
函数全 ≤50、文件 369 行；该自检在 gate ⑥b 真跑。

**2026-11-27 批150 两档 S3 引脚口径同源（无改动）**：事实——`pi4b-host-esp32s3` 与
`esp32s3-mini` 的 `[hardware]` 逐键比对：11 个 S3 引脚键值完全相同，host 档只多三枚
宿主键（`alsa_device`/`link_ifname`/`periph_uart`）；11 值与 `board/pins_s3.rs` 锚点
逐一对上。处置——零改动（两档共存但只有一套引脚真值，差异是"宿主面 vs 单板面"）。
判据——tomllib 逐键 diff + 锚点常量对读。

**2026-11-27 批151 部署产物名单与实际输出对表（无改动）**：事实——README 的整片名
`rr3-esp32-safe-merged.bin` / `rr3-esp32s3-full-merged.bin` 与 `publish.yml` 的
`--out` 名逐字一致；`image-esp32.sh` 的缺省名只在未给 `--out` 时生效，发布面显式传名；
`fw-set.sh` 七件名与 README 一致。处置——零改动。

**2026-11-27 批152 漂移门真仓 S3 树干跑（结论行补 hardware）**：事实——批149 只在夹具里
证明 hardware 面进清单；结论行仍只列"模块 config/装配单/unit/板级资产"，运维看不出
wiring/axis 也在比对面。处置——结论行补"hardware 真值集"；用 `cp` 搭 /tmp 装机树
（13 件：模块 config 6 + 装配单 + unit + wiring + 双轴档）跑 `--local --profile
esp32s3-mini`：**13 件 0 漂移**；篡改 `axis/pan.toml` 后按 `DRIFT` 退 1 并点名两枚 sha。
判据——正例 rc=0、反例 rc=1；`--self-test` 9 例仍全过；gate ⑥b 真跑。

**2026-11-27 批153 烧录/镜像脚本参数与 README 对表（无改动）**：事实——
`flash-artifact.sh` 支持 `<elf> [--chip esp32|esp32s3] [--bootloader] [--port] [--monitor]`，
README 三条命令用的正是这些；`image-esp32.sh` 支持 `[--release] [--elf] [--chip]
[--flash-size] [--out] [--bootloader] [--partitions]`，publish 的 `--out/--chip/
--flash-size/--partitions/--bootloader` 全在表内；README 的裸 espflash 段用
`--partition-table` 属 espflash 自身参数，不是本仓脚本。处置——零改动。

**2026-11-27 批154 真值集两处测试分工复核（无改动）**：事实——rrd `runtime_set_pinned`
钉**成员名单形状**（distro 精确向量、esp32-mini 的 chip 选册成员与 dirs），
rr3-pack `s3_truth_members_exist` 钉**真仓 S3 成员存在**（八文件 + axis 目录）；两者
一个管"名单长什么样"、一个管"名单指的件在不在"，无重复口径也不冲突。处置——零改动。

**2026-11-27 批155 敞口 25 扫描面订正**：事实——敞口行仍写"门扫描面 16 件（模块 config 7
＋装配单 8 ＋ unit 1）"，是 2026-11-16 快照；批149/152 后已含 hardware 面。处置——按现树
计数订正：全档 = 模块 config 8 ＋装配单 9 ＋ unit 1 ＋板级资产 2 ＋ wiring 9 ＋ axis 5
＝ **34 件**；`--profile` 只取该档。判据——各类文件逐目录计数，与工具跑出的"比对 N 件"
（S3 档 18 件）对上。

**2026-11-27 批156 deploy README 补漂移门使用节**：事实——README 只在平台表一行提到
`verify-board-config.sh`，没有用法/覆盖/退出码，运维需读脚本头注。处置——在"版本化
部署包契约"后补一节：覆盖六类、`--local`/`--remote`/`--self-test` 三条命令、退出码
0/1/2/3。判据——三条命令与脚本 usage 逐字对照。

**2026-11-27 批157 漂移门补契约册（真缺口修复）**：事实——交叉核对"工具扫描面 vs
rr3-pack 随装面"：扫描面仍少 `contracts/{topics,slots,roles,devices}.toml` 与
`contracts/platform/<chip>.toml`，而这五册装机后 rrd 装载直接读，漂了即拒 boot。
处置——新增 `rr3_manifest_contracts`（四册恒比；平台册按 `--profile` 的
`[platform].chip` 选一，legacy 模式不猜芯片只比四册）；自检加"契约册漂移红"一例；
结论行补「契约册」。判据——自检 10 例全过；真仓 S3 干跑 **18 件 0 漂移**（原 13 件 +
四册 + 平台册）；gate ⑥b 真跑。

**2026-11-27 批158 bundle 随装面与漂移门扫描面交叉核对**：事实——`pack-bundle.sh` 打包
`profiles/`、`contracts/`、`hardware/` 三个整目录 + `modules/<m>/{bin,module.toml,config*.toml}`
+ `bin/`，是漂移门扫描面的**超集**（扫描面 ⊆ 随装面）；但 bundle **不含也不装 systemd
unit**——`install-bundle.sh` 从已装 unit 读 `--socket`/`--profile`。处置——README 的
bundle 契约补"base 层前置"：干净机首装必须先走 deb；bundle-only 干净机上漂移门报
unit `MISS` 是如实提示。判据——两脚本文件清单与扫描面逐类对照。

**2026-11-27 批159 build-onboard 漂移报告支持 --profile**：事实——第七段固定跑全档
模式；单档板上会因"9 装配单 + 14 hardware 成员 + 契约面"刷出成片 MISS 噪音，报告
失去可读性。处置——`build-onboard.sh` 增可选第 4 参 `profile`，给了就只比该档，未给
保持全档（多档镜像自测口径）；用法行同步。判据——`bash -n` 过；两条分支参数与
`verify-board-config.sh` usage 逐字对上。

**2026-11-27 批160 --remote 路径对新成员的安全性复核（无改动）**：事实——所有清单行
（含新增 contracts / hardware 成员）都经 `verify()` → `rr3_hash_remote()`，该函数两个
口径分支**都带 `ssh -n`**（2026-11-16 的 here-doc 吃 stdin 修复对新增成员自动生效）；
安装侧路径由 `rr3_inst_path` 默认分支拼 `$root/hardware|contracts/...`，与安装根一致。
处置——零改动。判据——函数调用链静态走读。

**2026-11-27 批161 install-bundle 缺 unit 时点名**：事实——bundle 不含也不装 unit；
`unit_arg` 吞掉 `systemctl cat` 非零后，`SOCK` 走缺省、`profile` 为空，脚本会**静默**
换树——干净机上得到"装完没有服务"的结果。处置——在 SOCK 缺省后补一条 WARN：点名
「未找到已装 rrd.service、bundle 不装 unit、干净机先装 deb、本次按缺省 socket 继续」；
不中止（staging 树/chroot 下 systemctl 不可用属合法场景）。判据——`bash -n` 过；
分支条件为 `--socket` 与 `--profile` 双空，避免误报。

**2026-11-27 批162 boards §10 漂移核验命令复核（无改动）**：事实——
`base/os/distro/boards/raspberry-pi-64.md` §10 步④ 已写
`--repo . --remote pi@<板地址> --profile pi4b-solo`，与批159 的 profile 口径一致。
处置——零改动。

**2026-11-27 批163 build-onboard 用法在文档中的引用复核（无改动）**：事实——README 与
boards 只按名引用 `build-onboard.sh`（作为"板上原生构建音频件"的入口），未抄参数表；
批159 新增的第 4 参不产生文档漂移。处置——零改动。

**2026-11-27 批164 install-bundle 缺 unit 的 WARN 与后续控制流对齐**：事实——批161 的
WARN 承诺"按缺省 socket 继续"，但换树流程第一行 `systemctl stop rrd` 在 unit 缺失时
返回非零，`set -e` 当场中止且不自述，承诺与行为相反；同时 `expected` 取不到时 PASS 段
打印"模块："空白，读起来像"零模块也验过"。处置——unit 缺失改为**动树之前** fail-closed
die（点名 bundle 不装 unit、先装 deb 铺 base 层）；PASS 段按两种成因诚实降级（unit 的
`--profile` 指向不存在/不可读、或 ExecStart 未声明 --profile ⇒ 只验到 rrd 应答）。
判据——`bash -n` 过；die 位于 tar 校验、换树与 `systemctl stop` 之前；README 装机节
已写 deb 前置，文案与之一致。

**2026-11-27 批165 bundle 自足面入册 §3**：事实——`pack-bundle.sh` 的随装面一直不含
systemd unit，"bundle 能不能单独交付"这个决定从未登记（L-33 病形：事实在跑、决定无主）；
批164 把缺 unit 改为 fail-closed 后，缺省态的代价才显式可见。处置——[decisions.md §3](decisions.md)
补一行"bundle 自足面"（裁定类）：不裁缺省态＝先 deb 后 bundle，两选项代价（随包自足 vs
覆盖用户 unit）与触发条件（bundle-only 首次上线）写入行内；计数 2→3。判据——六列齐全；
取证基线号填批164 提交 `33c0af76`；§3 头行与末段计数同步。

**2026-11-27 批166 漂移门档位面自洽与平台册覆盖**：事实——`--profile` 拼错、档位缺
`[platform].chip`、或该芯片的平台册不在仓内时，`rr3_deploy_manifest` 会静默少列成员，
漂移门照样打绿，而漏比的恰是该档最要紧的平台册/装配单/板级资产；另 `check.sh` 注释
"六例"、deploy README"九例"与脚本实际 10 例早已不符。处置——抽出 `rr3_chip_of` 单点
派生，新增 `rr3_profile_guard` 三前置（档位装配单存在／chip 非空／平台册在仓），
不自洽即 rc 2 并点名缺哪一件；自检补四例（profile 模式全等过、档位不存在、无 chip、
平台册缺件），装配单还原改为 `cp` 仓侧原件（手抄字面量在字段变长后必漏，本次实测）；
两处文档例数改为"以 `--self-test` 输出为准"，退出码 2 的口径补档位面不自洽。判据——
self-test 14/14 PASS；真仓 9 份装配单逐个干跑（空根）均 rc=1 且各恰 1 行平台册，
把 `README.md` 当档名传入得 rc=2；⑤ 文档门、⑬ 仓面卫生、⑯ 行数门与 ⑫ 指标全绿。

**2026-11-27 批167 本轮收口·三线全绿**：事实——本轮三批（164 装机器 fail-closed／165 bundle
自足面入册／166 漂移门档位面自洽）全部推齐三远端后，`tools/ci/ci-status.sh` 于本地头
`40979b8` 得 gate／integration／platforms 三线 success（platforms 五芯片矩阵照旧）。
处置——本轮无遗留红条；下轮任务见轮末清单（bundle 契约进流水线真跑为第一件）。
判据——ci-status 退出码 0；⑫ 指标（重复率 0.41%、CC 均值 2.47）与 ⑤⑬⑯ 门同拍全过。

**2026-11-27 批168 A10 部署包契约进流水线真跑**：事实——`pack-bundle.sh` 是 rk3566 快路轨
的唯一交付件，A10 契约只活在脚本/文档里，六条流水线从不出包、从不验包（2026-11-08 真板
事故正是包内成员版本偏斜）。处置——新增 `tools/ci/platform/bundle-contract.sh`（顶层名／
BUNDLE_VERSION 与 HEAD·target·八件组件／三件 bin 执行位／五模块 binary+module.toml+config／
装配单·契约四册·各平台册·各机型 wiring 按仓内真值全量对／八件 ELF64 与架构串），带合成包
自检（合规过＋模块缺 config／非 ELF 件／commit 不符三红例）；platforms 的 aarch64-musl 腿在
构建后真跑 `pack-bundle.sh` 再断言。判据——self-test 4/4 PASS；YAML 解析过；workflow 结构
审计通；⑤⑬⑯ 与 ⑫ 指标全绿；CI 结果以 platforms 腿实测为准（后续观察）。

**2026-11-27 批169 装机器干净机拒绝路径上流水线实证**：事实——批164 的 fail-closed 只有
静态判据（代码走读），没有一次真执行；而"干净机上装 bundle"正是 2026-11-08 事故的同型
入口。处置——platforms 的 aarch64-musl 腿在出包后以 `sudo` 跑 `install-bundle.sh` 指临时
安装根，断言 rc≠0 且输出含"未找到已装 rrd.service"；正例（真 systemd + rrd 健康窗口）留
真板，不在流水线冒充。判据——YAML 解析过；workflow 结构审计与 ⑬ 门全绿；实跑结论以
platforms 腿日志为准。

**2026-11-27 批170 漂移门远端面真跑与自检册拆分**：事实——`--remote` 从建成起零执行，
自检只走 `--local`：`ssh -n` 语义、unit 口径的远端 sed 分支、远端缺件归因都没机器判据；
且主文件加到 476 行，逼近 500 硬线。处置——自检册按职责拆出
`verify-board-config.selftest.sh`（主文件只留判据机制，334 + 154 行）；自检加 ssh 桩
（`ssh -n <host> "<cmd>"` 的 cmd 在本机跑）真跑远端四例：全等过／改一字节红／unit 口径
仍过／缺件红；远端用例走档位模式，避开档位负例夹具的全档噪音。判据——self-test 18/18
PASS；⑤⑬⑯ 与 ⑫（重复率 0.41%、CC 2.47）全绿。

**2026-11-27 批171 tools 清单订正**：事实——`tools/README.md` 的 deploy 行写"12 支"并漏列
`build-riscv-image`／`flash-artifact`／`fw-set`／`console-log.py`，与实际 14 支 .sh 不符
（清单是唯一说明面，漏列即"有件无据"）。处置——按真实成员改写为"14 支 .sh 与
`Containerfile`、`console-log.py`"并补全名单。判据——`ls tools/deploy/*.sh` 14 支与行内
名单逐名对上；⑤ 文档门绿。

**2026-11-27 批172 ci-status 未定判定补盲（上一头在跑曾被读成绿）**：事实——判据只把
**本地头**的 run 计入 pending；纯文档提交被 paths-ignore 跳过时本地头没有 run，而上一头
的平台腿仍在跑，旧实现照样退出 0（当天实测：`251d8387` gate success + `cfd51ebb`
platforms in_progress ⇒ 0，同一时刻平台上并无当前代码的结论）；README 又写"最近 40 次"
而脚本 per_page=10。处置——判据抽成 `tools/ci/lib/ci_verdict.py`（机制单点），pending
改为"任一 workflow 最新一条在跑即 2"，并加 `--self-test` 四例（全绿过／旧头仍在跑报
未定／任一红／本地头无 run，各钉一条退出码）；README 的条数与口径同步。判据——
self-test 4/4 PASS；同一时刻真实调用得 rc=2（旧实现 0）；⑤⑬⑯ 全绿。

**2026-11-27 批173 本轮收口·代码头三线全绿**：事实——代码头 `3e154a8e` 上 gate success、
integration success、platforms success（run #825，HTML 面核对 `WORKFLOW_RUN_CONCLUSION_SUCCESS`；
同一 job 在 `cfd51ebb` 运行时"版本化部署包契约"与"装机器干净机拒绝"两步均 success，批168/169
实跑成立）；期间匿名 API 撞 60/h 限额，ci-status 如实报"未定"（不冒充绿），改用 actions
页面核对。处置——本轮收口；下轮第一件＝rk3566 bundle 进发布面（publish 现只出镜像/固件/
release 资产，bundle 不在列）。判据——`ci-status --self-test` 4/4；三线结论与 run 页一致。

**2026-11-27 批174 rk3566 distro bundle 进发布面**：事实——发布面此前覆盖 x86_64/arm64/
riscv64 镜像与 esp32 双芯片固件，唯独 RK3566 distro 快路轨的版本化 bundle 只在板上/本机
手工出包：`publish.yml` 无此腿，正式发布集缺一块。处置——新增 `distro-bundle` 腿（只认
tag 推送，与 firmware 同纪律）：`release-tag.sh` 版本门 → `cross-artifacts.sh` aarch64-musl
八件全静态 → `pack-bundle.sh` 出 `rockrobot3-distro-<ver>-aarch64.tar.gz` → `bundle-contract.sh`
A10 断言 → release 资产（含 sha256）→ oras 推 `rockrobot3-distro:<tag>`（不可变标签）→
cosign keyless 正/负验证。判据——YAML 解析过；workflow 结构审计通过；构建/断言两步与
platforms aarch64 腿逐字同源（该腿 2026-11-27 实跑 success）；tag 窗口实推待下次发版。

**2026-11-27 批175 发布面审计纳入 distro 腿**：事实——批174 新增第五条发布腿，而
`workflows_audit` 的发布面契约仍按四腿（build/build-riscv64/merge/firmware）钉名，
新腿被删或托管面被削都不会红；"发布面完整性"的覆盖与实际发布集不再同集。处置——
`workflow_release.py` 的必列 job 与断言表加入 `distro-bundle`，并新增
`distro_problems`（pack-bundle/bundle-contract 同源、rockrobot3-distro 与 oras 双载体、
sha256 清单、tag 门、contents/packages 权限）；自检夹具补一条"只有出包一步"的 distro
腿作靶，断言表 25→26；`ci-workflows.md` 与 conventions 的发布面表同步。判据——
`workflows_audit --self-test` 26/26 PASS；结构审计绿；⑤⑬⑯ 全绿。

**2026-11-27 批176 漂移门远端一趟往返（装机核验流畅度）**：事实——远端模式逐件起 ssh，
单档 16~19 次握手；Wi-Fi 板上每次都要重新认证，核验要等半分钟，且 ssh 失败会把整份
清单打成 MISS（"网不通"读成"板上全漂"）。处置——`rr3_remote_script` 把整份清单编成
远端脚本、一条 `ssh <host> bash -s` 取回 `<路径>\t<sha|MISS>`，摘要在远端按同一 sha
口径计算；`rr3_remote_fetch` 把 ssh 255 区分为"不可达"→ 退出 2（未定），不冒充漂移；
自检按职责拆成 local/profile/remote 三组（单函数回到 50 行内），并补两例回归钉：
单趟往返（ssh 恰 1 次）与不可达报 2。判据——self-test 20/20 PASS（单趟用例实测 ssh=1）；
⑤⑬⑯ 全绿；主/自检册 354/204 行。

**2026-11-27 批177 本轮收口·代码头三线全绿**：事实——代码头 `9cac785c` 上 gate #1119、
integration #826、platforms #828 均 success；本轮四件（174 发布面加 distro 腿／175 审计
纳入该腿／176 远端核验收成一条 ssh）全部推齐三远端。处置——本轮收口；distro 腿的 tag
窗口实推未发生（无新 release 提交），按"结构已钉、实推待发版"记，不冒充验证。下轮首件＝
发布面齐套巡检固化成一条只读命令（镜像/固件/distro/OTA/buildroot 五类按 tag 核对）。
判据——三线结论与 run 页一致；audit self-test 26/26、漂移门 self-test 20/20。

**2026-11-27 批178 发布面齐套巡检器建成**：事实——发版后"五类制品齐不齐"只活在
runbook B 的手工命令里：镜像三架构/固件/distro/OTA/buildroot 逐条 curl，成员清单靠
记忆；核对时已发现一处标签写法分歧（status 记 `rockrobot3-ota:0.0.8`，而 buildroot.yml
实推 `v${VER}`）。处置——新增只读巡检器 `tools/ci/lib/release-verify.sh`：一张数据表
钉五类 ref 与成员（固件 9 件、distro 2 件+artifactType、OTA 6 件含 `keys/*.pub`、
buildroot 4 件），镜像索引判三架构、单架构判 attestation≥2；任何缺件/不合规逐项点名并
打 digest，不发写请求。自检以假 oras + manifest 夹具跑六例（全齐/缺层/缺架构/缺引导/
证明面不足/非 semver），纳入 gate 的 scripts 步防判据腐化。判据——self-test 6/6 PASS；
YAML 解析过；⑤⑬⑯ 全绿；实仓取数待凭据窗口（本机无 ghcr 凭据，不冒充）。

**2026-11-27 批179 齐套巡检接进两条发布流水线**：事实——各发布腿只验自己那一件
（推完 `oras manifest fetch`），"本流水线三类都在、成员齐不齐"没有总账；而 publish 与
buildroot 同为 tag 触发、后者是小时级，不能用"五类全齐"互相卡住。处置——巡检器加
`--classes`（只认五值，拼错即退 2，防"跳过全部判据仍打绿"），publish 增 `release-verify`
作业（needs merge/firmware/distro-bundle，核 image+firmware+distro），buildroot 的
release 作业在推完 OTA/buildroot 后核 ota+buildroot；两处同用 `--repo` 取当前仓、
oras 走本作业登录。判据——self-test 7/7（含类别过滤例）；YAML 解析过；结构审计绿；
⑬⑯⑤ 全绿；实仓回读待 tag 窗口。

**2026-11-27 批180 齐套总账进审计与文档**：事实——批179 新增的 `release-verify` 作业
不在发布面契约的必列表里，删掉或把 `--classes` 改窄都不红；buildroot 的 `release` 作业
推完不接总账也无判据；runbook B 仍写"cosign 未启用"（与 B.1 同页"已接入 2026-11-25"
自相矛盾），执行段也只有手工 curl。处置——`workflow_release.py` 必列 job 加
`release-verify` 并新增 `verify_problems`（复用脚本、三类在账、needs 三产出腿、packages
权限）；`release_artifacts_problems` 对 buildroot 增"推完必须接 release-verify.sh"；
自检夹具补空壳总账腿、断言 26→28；runbook B 执行段改为总账命令 + v0.0.8 历史例外口径，
判据与 B.1 的签名状态订正；ci-workflows/conventions/tools-ci README 同步。判据——
audit self-test 28/28；结构审计绿；⑤⑬⑯ 全绿。

**2026-11-27 批181 本轮收口·四线全绿**：事实——代码头 `221a2b83` 上 gate/integration/
platforms 均 success，改过 YAML 的 buildroot 在 `0ef9c4df` 也 success（其 release 作业
只认 tag，未被分支推送误触发）。本轮三件：齐套巡检器建成（178）、接进两条发布流水线
（179）、进发布面契约与文档（180）。处置——本轮收口；下轮首件＝把巡检器放进周程做
匿名回读（已发布包公开，防包被删/权限回退），其次补 distro-smoke 的 boot 时延上限判据
（流畅度基线；本机无板、匿名读不到日志，故用"步骤内计时+保守上限"而非事后取数）。
判据——四线结论与 run 页一致；audit self-test 28/28、巡检自检 7/7。

**2026-11-27 批182 发布面匿名回读进周程**：事实——ghcr 正式制品是公开交付面（status 与
runbook 都按"无凭据可下载"记账），但该承诺没有任何周期判据：包被转私有、tag 误删、成员
被改，三条流水线都不会红。处置——platforms 增 `release-anon` 作业（只在 schedule/
workflow_dispatch 跑，不登录、不推送、不回滚）：tag 取仓根 VERSION 的单一真值，按
`release-verify --classes image,firmware` 匿名回读三架构与固件九件；OTA/buildroot 在
v0.0.8 是无 v 前缀的历史例外，暂不纳入，补齐后按默认五类扩。审计必列 job 加该腿，
自检断言 28→29。判据——YAML 解析过；结构审计绿；定向自检 29/29；⑤⑬⑯ 全绿；实跑
在下一次周程（或手动 dispatch），不冒充已验。

**2026-11-27 批183 齐套巡检的"看不到 vs 没有"分账**：事实——取数失败（401/网络/包不可见）
此前只打一行 FAIL 并计入不合规，结论仍是"发布面不齐"：会把凭据问题引向查制品；且无夹具
钉住该路径。处置——单列 `FETCH_FAILS`：只要有取数失败，整份结论为**未定**（退出 2，与
用法错误同权），与"缺件/不合规"（退出 1）分开；自检加 401 夹具一例（假 oras 受
`RR3_ORAS_FAIL` 控制），八例；`tools/ci/README` 与 runbook B 补退出码口径（未定不得记
成缺件、不得跳过宣称发布成功）。判据——self-test 8/8；⑤⑬⑯ 全绿。

**2026-11-27 批184 distro-smoke 的 boot/status 时延上限**：事实——⑧ 只有"30s 内六模块
spawn 齐"的等待上限：慢到 25s 也算绿，流畅度回归无判据，也没有任何数字可对表；status
回读只验文本，卡顿不可见。处置——在容器内用毫秒时钟记六模块 spawn 齐的时刻，打印
`timing boot_spawn6_ms=…` 并按保守上限 20s 判红；`rr3-ctl status` 同样记往返并打
`timing status_rtt_ms=…`，上限 5s。上限取保守值（远低于原等待窗），只拦病态回归，
不把 CI 抖动写进失败面。判据——`bash -n` 过、外层单引号串仍只有 2 个（不触发 2026-11-08
撇号截断旧病）；⑬⑯ 全绿；真实 timing 数字以 integration 腿日志为准（随后观察）。

**2026-11-27 批185 证明面判据按真实形状修正（匿名实测）**：事实——用 ghcr 匿名 token
逐条回读 v0.0.8：运行镜像 `200`（合并 index 6 入口 = 3 镜像 + 3 attestation）、固件
`200`（9 层）；OTA `v0.0.8` 为 `404`（与 status 记的"该版以 0.0.8 名留存"一致）、distro
尚无包（403/未发布）。**关键发现**：单架构 tag 的索引里只有 **1 条** attestation
manifest，其内部两枚 in-toto 层分别声明 SLSA provenance 与 SPDX SBOM——批178 的
"attestation≥2"与 runbook 旧文案都是错的，周程匿名回读（批182）一上线就会假红。
处置——判据改为按谓词层判（`attest_digest` + `predicates_of`，缺任一枚即点名），
夹具同步为真实形状（attest.json 两枚谓词；负例删 SPDX）；runbook B 与 ci-workflows
订正为"1 条 manifest、两枚 in-toto 谓词"。判据——自检 8/8 PASS；文档门绿；匿名回读
证据即上文实测（不冒充已跑周程）。

**2026-11-27 批186 巡检器实跑首证与缺件/未定分账**：事实——把 oras 1.2.3 装到 /tmp 后
**匿名实跑**：`--tag v0.0.8 --classes image,firmware` 全 PASS（三架构 + 三腿证明面
provenance+SBOM + 固件 9 件，与批185 的判据形状一致）；`--classes ota,buildroot` 实得
`not found`，旧实现却报"取数失败→未定"——远端明确不存在与看不到混成一种。处置——
`fetch` 按 stderr 再分账（not found/manifest unknown/404 ⇒ 缺件退 1；401/403/网络 ⇒ 未定
退 2）；自检加远端明确不存在一例（九例）；runbook 的 v0.0.8 示例订正为只核两类
（distro 当时尚未发布），README 口径同步。判据——自检 9/9；实跑 image+firmware 退 0、
ota+buildroot 退 1 并逐件点名缺件；⑤⑬⑯ 全绿。

**2026-11-27 批187 本轮收口·三线全绿**：事实——代码头 `1bfaecfb` 上 gate/integration/
platforms 均 success（distro-smoke 的 boot/status 时延上限在 `cf28e2cd` 的 integration
腿已实跑通过）。本轮五件：匿名回读进周程（182）、缺件/未定分账（183）、distro-smoke
时延上限（184）、证明面判据按真实形状改判（185）、巡检器匿名实跑首证并再分账（186）。
处置——本轮收口；顺手清掉 `check_image` 的死变量 `n`。下轮三件：① cosign 签名纳入
齐套总账（`--with-signature`，身份约束与 runbook B.1 同源）；② VERSION 已 bump 而未
tag 时 `release-anon` 该红还是该 SKIP——入 decisions §3 待裁（避免周程误红或静默放行）；
③ 发版回执模板把巡检结论列为必填项。判据——三线结论与 run 页一致；自检 9/9；⑤⑬⑯ 绿。

**2026-11-27 批188 cosign 签名纳入齐套总账**：事实——runbook B.1 的验签命令只有人工
形态；总账只看"制品在不在、成员齐不齐"，签名缺失/身份被换不会红。匿名实测确认签名面
确实存在：运行镜像合并 digest `sha256:07d189…` 的 `sha256-07d189….sig` tag 可匿名读
（1 层 `simplesigning.v1+json`），固件 digest 同理——但 cosign 二进制体积过大，本轮未在
本机跑端到端 keyless 验证，不冒充。处置——巡检器增 `--with-signature`：镜像总账/固件/
distro 按 `oras` descriptor 取 digest，调 cosign verify 并**固定**身份正则与 issuer
（与 B.1 同源，不做可配置放宽）；缺 cosign 退 2（未定），无匹配签名退 1。自检加
cosign 桩与三例（通过/无匹配/缺工具），共十二例。判据——self-test 12/12；签名 tag
匿名可读的证据如上；端到端 cosign 验证待有 cosign 的环境（tag 窗口或本机安装）。

**2026-11-27 批189 周程按"已发布最新 tag"回读**：事实——批182 的周程腿用
`v$(cat VERSION)` 拼 tag；VERSION 在 release 提交里先 bump、tag 随后才推，这个正常窗口
会让周程常红（假红久了就没人看），而"公开交付面现在是什么"的正确问题对象是**已发布
的最新正式 tag**。处置——巡检器增 `--tag auto`：`oras repo tags` 取镜像仓 tag，筛
`vX.Y.Z` 后 `sort -V` 取最大；一个正式 tag 都没有时报未定（2），不退化成拿 edge/latest
去核；platforms 周程腿改 `--tag auto`。自检加两例（auto 选中 v0.0.8 / 无正式 tag 报
未定），共十四例。判据——self-test 14/14；本机匿名实跑 `--tag auto` 选中 v0.0.8 并
image+firmware 全 PASS；⑤⑬⑯ 绿。

**2026-11-27 批190 发版回执必填项固化**：事实——"发布回执"此前只有散在 status/
conventions 里的历史样本，没有形状约束：巡检结论、五类 digest、签名结论是否必须出现，
全凭记性；而本轮刚建成的总账与验签能力没有回执入口。处置——runbook 增 B.2
"发版回执必填项"四条（同提交三线全绿＋`release-verify --with-signature` 结论行＋五类
digest 或未产出原因＋keyless 签名结论），并点名唯一历史例外（v0.0.8 的 OTA/buildroot
无 v 前缀、distro 未产出）；取数失败不得改写为缺件、不得跳过。判据——⑤ 文档门绿；
四项与本轮工具的实际输出逐条对得上（186/188 的实跑输出即样板）。

**2026-11-27 批191 本轮收口·三线全绿**：事实——代码头 `92b9a48f` 上 integration 与
platforms 均 success（gate 在前序头 success），文档头 `5f339737` 的 gate success；190 只动
md，按 paths-ignore 未触发另两条腿，属预期。本轮三件：cosign 签名纳入总账（188）、周程
改用已发布最新 tag（189）、发版回执必填项固化（190）。处置——本轮收口；下轮三件：
① 巡检器输出"五类 digest 汇总段"供 B.2 回执直接引用；② 周程腿加 cosign 并开
`--with-signature`（公开签名可匿名验，把"签了没验"补上）；③ 收口前复核两条流水线的
分账 classes 与 B.2 四项逐条对齐。判据——三线结论与 run 页一致；巡检自检 14/14、
审计自检 29/29；⑤⑬⑯ 绿。

**2026-11-27 批192 巡检器输出 digest 汇总表**：事实——B.2 要求回执列五类 digest，而巡检
只在逐项 PASS 行里零散打印；人工抄五条长 digest 易错，且"抄哪一条"无判据。处置——
各类取回成功后记一条 `类别 ref digest`（镜像总账/固件/distro/OTA/buildroot），结尾统一
打印"制品 digest 汇总表"，回执直接引用；自检加一例钉住表头与 digest 出现，共十五例。
顺手订正 README 里残留的"attestation≥2"旧口径（批185 只改了 runbook/ci-workflows）。
判据——self-test 15/15；匿名实跑 `--tag auto` 末段正确列出镜像 `sha256:07d189…` 与固件
`sha256:39cc0f…`；⑤⑬⑯ 绿。

**2026-11-27 批193 周程腿启用验签**：事实——批188 把 cosign 验签做进巡检器，但周程
`release-anon` 没开 `--with-signature`："签了没验"在流水线里仍然成立；签名 tag 本身是
公开仓的普通 tag（批188 已实测合并镜像与固件的 `.sig` 可匿名读），验签无需凭据。处置——
周程腿加 `sigstore/cosign-installer@v3`（固定 v2.6.5，与 publish 同版本）并开
`--with-signature`；身份约束仍由脚本内固定正则把关，不在工作流里放宽。判据——YAML 解析
过；结构审计绿（ACTIONS 版本矩阵含该动作）；真实 keyless 验证结果以下一次周程/手动
dispatch 为准，本机未装 cosign，不冒充已验。

**2026-11-27 批194 回执签名面边界对齐（无代码改动）**：事实——B.2 第 2 项读起来像
"五类都该有 cosign 签名"，而 OTA/buildroot 是 ADR-0028 的文件级 `.sig`，不经 cosign；
巡检器本身只对 image/firmware/distro 调 verify_sig（代码为准），文档未写清边界，回执
容易被误读成"OTA 未签名 = 发布不合规"。处置——B.2 明确签名面只覆盖三类、OTA/buildroot
的 `.sig` 不在 cosign 面内；工具代码零改（边界本已正确）。判据——⑤ 文档门绿；
`check_ota`/`check_buildroot` 无 verify_sig 调用（静态走读）。

**2026-11-27 批195 本轮收口·三线全绿**：事实——代码头 `d72e026d` 上 integration 与
platforms success（含 workflow 改动），文档头 `1440e165` 的 gate success。本轮三件：
digest 汇总表（192）、周程启用验签（193）、回执签名面边界写清（194）。处置——本轮
收口；下轮三件：① 把 `release-anon` 从 platforms 拆成独立轻量 workflow（现在手动验证
要拉全平台矩阵，成本与收益不匹配），审计必列文件/job 同批更新；② 全仓"例数/件数"类
陈旧计数清扫（README/runbook 为准，历史回执不改写）；③ distro 首发后的 B.2 五项实测与
`--classes` 默认口径复核（待 release 触发）。判据——三线结论与 run 页一致；巡检自检
15/15、审计自检 29/29；⑤⑬⑯ 绿。

**2026-11-27 批196 匿名回读拆成独立 workflow**：事实——`release-anon` 挂在 platforms 里，
手动验证要拉五芯片全矩阵（本腿只读 15 分钟，成本与收益不匹配）；且 `REQUIRED_JOBS`
只校验"文件存在时的 job 名"，整文件被删不会红（覆盖契约的文件级洞）。处置——新建
`release-anon.yml`（周程 UTC 周一 03:00 + 手动，independent 并发组，只读、不登录，
`--tag auto --with-signature --classes image,firmware`），platforms 删该腿；审计加
`missing_workflows`（必列 workflow 文件缺即红）并把 `release-anon.yml`/`anon` 入表；
ci-workflows 与 conventions 同步。判据——两文件 YAML 解析过；结构审计绿；自检 30/30
（夹具缺该文件被抓）；⑤⑬⑯ 全绿；真实周程/手动结果待触达。

**2026-11-27 批197 发布面腿数陈旧计数清扫**：事实——批174/179 把发布腿扩到五条产出腿
＋总账腿后，三处现行文档仍写旧数：publish.yml 的 version-gate 注释"四个出口"、
ci-workflows 的"build/build-riscv64/merge/firmware 四个出口"、tools/ci README 的
"四腿"；读者按文档核对会漏掉 distro 与总账。处置——三处按现状改写（五条产出腿＋
release-verify；覆盖契约补 release-anon 文件级存在判据）；历史回执/证据档不改写
（记录当时事实）。判据——⑤ 文档门绿；结构审计绿；三处数字与 `REQUIRED_JOBS`、
publish.yml 的 job 列表逐名对上。

**2026-11-27 批198 本轮收口·三线全绿**：事实——代码头 `465c7cb9` 上 gate/integration/
platforms 均 success（含 platforms 全矩阵复跑；新增 `release-anon.yml` 仅 schedule/
dispatch，不参与推送触发）。本轮两件：匿名回读拆独立 workflow（196）、发布面腿数陈旧
计数清扫（197）。处置——本轮收口；下轮三件：① distro 首发窗口的 B.2 五项实测与默认
五类复核（无 release 触发时先做 `--deep` 的夹具与无包路径，真包留首发，不造演示件）；
② 周程首次自动跑（UTC 周一 03:00）结果归因，失败按注解定位；③ distro-smoke 的
boot/status 上限按可读数据收紧（无板/无日志则如实记敞口）。判据——三线结论与 run 页
一致；巡检自检 15/15、审计自检 30/30；⑤⑬⑯ 绿。

**2026-11-27 批199 distro-smoke 时延上公开面**：事实——批184 加了 boot/status 上限并打印
timing，但数字只进 job 日志；本仓 job 日志匿名不可读（403 已实证），上限是否接近实际
无从判断，也无法在回执里引用。处置——宿主编排一个临时 timing 文件挂进容器，容器把
`boot_spawn6_ms` / `status_rtt_ms` 写入，退出后主机回显并追加到 `GITHUB_STEP_SUMMARY`
（公开可读；未设置摘要面时只回显），文件用 trap 清理。判据——`bash -n` 过；外层单引号
仍平衡（2 + 摘要围栏 4 + trap 2 = 8）；integration 腿下一次实跑给出真实数字与公开摘要，
本轮不预填数值。

**2026-11-27 批200 本轮收口·三线全绿**：事实——代码头 `96fe88e9` 上 gate/integration/
platforms 均 success；integration 的 distro-smoke 作业 success，说明 timing 采集、
临时文件挂载与 summary 追加三条路径没有运行时错误；但本机匿名抓取 job 页面两次超时、
job summary API 需鉴权，**数字本身本轮无法取回**，不预填也不据此收紧上限。本轮一件：
时延数字上 step summary（199）。处置——本轮收口；下轮三件：① distro 深检（从 registry
拉 tar 过 A10 契约；bundle-contract 需增"按 tag 提交校验/跳过 HEAD 锚"的模式，先在夹具
与无包路径验证，真包留首发）；② 从可读面（人工登录 / 后续真板）取一次 timing 数值，
据此把 20s/5s 上限收紧到实测余量；③ 收口。判据——三线结论与 run 页一致；巡检自检
15/15、审计自检 30/30；⑤⑬⑯ 绿。

**2026-11-27 批201 distro 深检：发布包内容过 A10 契约**：事实——齐套巡检只核 distro 的
层名与 artifactType："有这个名字"不等于"拉下来还能装"；而 `bundle-contract` 的 commit
锚是仓 HEAD，对已发布包（提交是 tag 当时的值）会假红，无法直接复用。处置——
`bundle-contract.sh` 增第 4 参 `期望提交|any`（缺省 HEAD；`any` 供已发布包，tag 真伪由
cosign 签名背书），自检加"发布包 any 跳过 commit 过"；`release-verify --deep` 对 distro
按层标题取 digest、`oras pull` 拉回、以 `any` 跑契约：坏包退 1、拉层失败按未定退 2。
夹具生成器按仓内真值树 + ELF 头占位件合成 A10 合规 tar（commit 用 000… 专门覆盖 any），
假 oras 增 pull 分支，自检 18 例（含通过/坏包/拉失败）。判据——契约自检 5/5、巡检自检
18/18；⑤⑬⑯ 绿；真包深检留 distro 首发（当前 registry 无包，不冒充）。

**2026-11-27 批202 时延上限校准敞口入册**：事实——批184 的 20s/5s 上限是保守值（只拦
病态回归），批199 把数字放上 step summary，但本机匿名读不到（job 页超时、summary API
需鉴权），"上限贴合实测"没有证据。处置——status 敞口表补第 29 行：判据＝一次可读的
`boot_spawn6_ms`/`status_rtt_ms`（人工登录读摘要或真板启动日志），触发条件与"不得把
20s 读成实际值"的红线一并写清；不结项，也不拍脑袋收紧。判据——⑤ 文档门绿；工具体侧
上限仍由 distro-smoke 实跑守住（integration 绿）。

**2026-11-27 批203 周程窗口与分账复核（无代码改动）**：事实——`release-anon.yml` 的 cron
为 `0 3 * * 1`（UTC 周一 03:00 = 北京周一 11:00），今天 2026-11-27 周五，首次执行尚未
到期，无结果可归因；B.2 与两条流水线分账逐条对表：publish 的 `--classes
image,firmware,distro` 恰为 cosign 签名的三类，buildroot 的 `ota,buildroot` 覆盖文件级
`.sig` 两类，digest 汇总表五类齐。处置——零改动；窗口与"到期后按 run 结论归因"写入
本回执，不预判绿红。判据——cron 与窗口时区换算（UTC+8）；分账字符串与工作流逐字对上。

**2026-11-27 批204 本轮收口·三线全绿**：事实——代码头 `528edcad` 上 integration 与
platforms success，文档头 `9b1083b8` 的 gate success。本轮四件：distro 深检与契约
`any` 模式（201）、时延校准敞口入册（202）、周程窗口/分账复核（203）、收口（204）。
处置——下轮三件：① `release-anon` 首次执行（2026-11-30 03:00 UTC）后按 runs 结论归因，
失败凭注解定位；② distro 首发窗口的深检真包验证（B.2 与 `--deep` 同批实测），未触发
则登记；③ rock3 运行面一项可量化改进（先取证据，再改码）。判据——三线结论与 run 页
一致；巡检自检 18/18、契约自检 5/5、审计自检 30/30；⑤⑬⑯ 绿。

**2026-11-27 批207 boot 投递摘要带规模与用时**：事实——`rrd` 在 boot 段与 accept 段边界
只有一句"boot 队列已投递，进入 accept"，没有规模与用时：慢在装配、慢在投递还是慢在等
连接，只能靠前后 spawn 行人工数；`rr3_daemon_queue` 也丢弃了投递计数。处置——队列返回
`(已投递, 缺格跳过, 拒启)`，边界日志改为
`boot 队列已投递: 已投递=N 缺格跳过=K 拒启=R 用时=Xms，进入 accept`；`rr3_queue_dispatch`
/`rr3_dispatch_one` 返回是否真投递；distro-smoke 增一行断言（`已投递=6` 必须在场，缺行
即红，防日志被删或 boot 没走到边界）。批205/206 的窗口如实登记：`release-anon` 首跑
（2026-11-30 03:00 UTC）未到期、distro 无 release 未触发深检，均不冒充。判据——
rustfmt `--check` 过（本机不编译，Rust 编译与测试由 gate 流水线给回执）；⑤⑬⑯ 绿。

**2026-11-27 批208 本轮收口·三线全绿（含 Rust 改动实跑）**：事实——代码头 `ea2c1b1b`
上 gate/integration/platforms 均 success：gate 完成 Rust 编译与测试，integration 的
distro-smoke 真起 rrd 并命中了新增的 `boot 队列已投递: 已投递=6 …` 断言（缺行会红）。
本轮：批205/206 窗口未到（release-anon 首跑 2026-11-30 03:00 UTC；distro 无 release），
按登记不冒充；批207 落地 boot 投递摘要（规模+用时）。处置——下轮三件：① 周程首跑后按
runs 结论归因；② distro 首发深检与 B.2 实测；③ 文档/脚本一致性巡检（数量与措辞类，
历史回执不改写）。判据——三线结论与 run 页一致；rustfmt --check、⑤⑬⑯、巡检自检
18/18、契约自检 5/5、审计自检 30/30 全绿。

**2026-11-27 批209-211 窗口登记与文档一致性**：事实——`release-anon` 已在 GitHub 注册
（workflows 列表可见），但尚无任何 run（首跑窗口 2026-11-30 03:00 UTC 未到）；distro
仍无 release，深检无真包可验——两项均无结果可归因。文档面巡检：现行文档无残留"四腿/
十五例"类陈旧计数；status 的 spawn 行未记批207 新增的投递摘要。处置——status 装配行
补一句边界日志口径（顺序—规模—时刻三段定位）；窗口两项按登记处理，不预判绿红。判据——
⑤ 文档门绿；workflows 列表与 cron 表达式逐字核对；spawn 行文本与 run.rs 当前格式对上。

**2026-11-27 批212 本轮收口·三线全绿**：事实——代码头 `ea2c1b1b` 上三线 success（沿用
批208 的实跑回执），文档头 `9a2b87e3` 的 gate success。本轮：209/210 两项窗口未到
（release-anon 首跑 11-30、distro 无 release），按登记不冒充；211 文档一致性——status
补边界日志口径，未发现残留陈旧计数。处置——下轮三件：① 11-30 后按 runs 结论归因周程；
② distro 首发深检与 B.2 实测（未触发继续登记）；③ 运行面再取一处可量化证据（优先
从新版投递摘要与 distro-smoke 时延数字入手，读不到就记敞口）。判据——三线结论与 run 页
一致；⑤⑬⑯、巡检 18/18、契约 5/5、审计 30/30 全绿。

**2026-11-27 批213-216 本轮收口·窗口复核与公开面再证**：事实——`release-anon` 首跑窗口
（2026-11-30 03:00 UTC）未到，各 workflow 最新一条中无 release-anon run；distro 包仍
不可读（匿名 token 拒绝，403＝不存在/未发布），`--deep` 无真包；公开面匿名复核
`--tag auto --classes image,firmware` 全 PASS，digest 与批186 一致（镜像 `07d189…`、
固件 `39cc0f…`），说明本周改动未触碰已发布制品。处置——本轮收口；下轮首件＝**装机器
正例进流水线**（tag 时原生 x86_64 出 deb → runner 上 `dpkg -i` + systemd 起 rrd（sim 档）
→ 以真 unit 跑 `install-bundle` 正例并验六模块 running），把唯一未测的"装机成功路径"
补上；失败即带回滚证据。判据——匿名复核退出 0；③ 三线沿用 `ea2c1b1b` 实跑回执；
⑤⑬⑯ 与巡检自检 18/18 绿。

**2026-11-27 批217 装机器正例进流水线**：事实——distro-smoke 在容器里直跑 rrd（容器无
systemd），platforms 的装机器只验"干净机拒绝"；install-bundle.sh 的成功换树与失败回滚
两条路径此前没有任何流水线执行过。处置——新增 integration 作业 install-smoke（判据体
tools/ci/platform/install-smoke.sh）：原生构建十件 → rr3-pack 出 deb → dpkg -i →
systemd drop-in 注入 RR3_PROBE=sim / RR3_BRAIN_CLOUD=mock → unit 拉起并验六模块 running
→ 真出 bundle（解包副本 profile 改 sim 后重打包，BUNDLE_VERSION/A10 原样）→
install-bundle 正例换树 → 坏 rrd（非 ELF）包回滚例；失败时 ERR trap 打 journal 与装机树。
审计发现并修两处：① `is-system-running=starting` 是 hosted runner 上 cloud-init 未收敛的
常态（同一提交三次实跑 running 1 / starting 2，两次把可用宿主判死）——按全局收敛态判是
错的对象，改为只判"无 system manager"（offline/unknown），功能判据交给 systemctl start
本身；② 结果只留在匿名不可读的日志里——正例/换树/回滚各上一条 notice 注解，公开可回读。
覆盖契约与缓存纪律同批更新（REQUIRED_JOBS / CACHE_ALL_JOBS 增 install-smoke）。判据——
integration run `46017650574` / job `140568786079` success，注解三条逐字："deb 装机 +
systemd unit 拉起成立，六模块 running"、"换树成功 commit=b88542d88256，六模块 running"、
"坏包回滚成立 rc=1（树与二进制已回上一版，六模块 running）"；同提交 gate 六 job success；
⑤⑬⑯、度量与流水线审计自检全绿；仓源零改（sim 变换只在装机副本与 bundle 解包副本）。

**2026-11-27 批218 装机正例保真化与漂移门正例**：事实——批217 的换树载荷是解包副本（profile
改 sim 后重打包），验的不是发布面那一份；且 `verify-board-config` 只有自检与真板远端，从无
"刚装完的机器"本地正例。处置——sim 环境整体移到部署面：`/etc/rockrobot3` 下铺同形副本
（profiles/distro.toml 改 sim、hardware/distro/wiring.toml 拷贝、modules/contracts 链回装机树），
unit 用 drop-in 重写 ExecStart 的 `--profile` 指向副本并注入 RR3_PROBE=sim / RR3_BRAIN_CLOUD=mock；
bundle 走 pack-bundle 原样出包、原样换树，换树后跑 `verify-board-config.sh --local` 判一致。
审计修两处：① 部署副本一度只拷 profile——rrd 由装配单两级父目录推导 modules-root，缺
modules/contracts 直接拒 boot（首跑 `rrd 未 active`），补链并把 journal 根因收进 die 文案
（`unit_reason`），故障不再只剩一行"未 active"；② `die` 走 exit 不经 ERR trap，现场没被打出来
——die 现在先 dump 再退。判据——integration run `46022059197` / job `140583907566` success
（2m14s），四条 notice 逐字："deb 装机 + systemd unit 拉起成立，六模块 running"、"换树成功
commit=b964890b16d6，六模块 running"、"真包换树后装机面漂移门判一致（仓/装机逐字节）"、"坏包
回滚成立 rc=1（树与二进制已回上一版，六模块 running）"；同提交 gate success；bash -n、⑤⑬⑯、
度量与流水线审计自检绿。

**2026-11-27 批219-220 窗口复核与本轮收口**：事实——`release-anon` 工作流 active 但 runs 总数 0
（cron 首跑 2026-11-30 03:00 UTC 未到，今日 2026-11-27）；GitHub releases 最新仍 v0.0.8，资产仅
ESP32 固件七件，无 distro bundle——`--deep` 深检与 B.2 签名实测均无真包可验，按登记不冒充。
处置——本轮交付 install-smoke 两批（217 正例/回滚 + 218 真包/漂移门）收口；下轮四件：①
install-smoke 时延回传（systemd 拉起与换树窗口的 boot→六模块用时、回滚恢复用时，给敞口 29
补第二载体）；② 11-30 后 release-anon 首跑归因；③ distro 首发后 `--deep` 与 B.2 实测；④
装机面漂移门负例（真装机树上改一件 config 期望 DRIFT，验完还原）与三线收口。判据——三线在
`b964890b` 上 gate / integration / platforms 三线 success；⑤⑬⑯、度量与流水线审计自检绿。

**2026-11-27 批221 install-smoke 三段时延公开回传**：事实——unit 拉起/换树/回滚耗时只活在
匿名不可读的 job 日志里，发版回执与敞口 29 都取不到数。处置——脚本在三个边界记毫秒：
`boot_systemd_ms`（systemctl start→六模块 running）、`bundle_swap_ms`（含停 rrd、换树、起、
六模块）、`rollback_ms`（坏包安装到恢复 running），汇总一条 notice + 各阶段 notice 带数 +
`GITHUB_STEP_SUMMARY`；**只记录不判红**（阈值校准归敞口 29，先积累实测）。判据——
integration run `46026067849` / job `140597668459` success，notice 逐字
`boot_systemd_ms=2115 bundle_swap_ms=2333 rollback_ms=32550`；同提交 gate success。首组实测
显示回滚恢复被 30s 健康探针窗口主导（坏包起不来要走满探针），登记为后续优化候选，本批
不改产品行为。

**2026-11-27 批222 漂移门负例（真装机面）**：事实——漂移门在真装机树上只有正例（没漂时
判一致），"漂了门看不看得见"仍只有自检夹具证据。处置——正例之后改一件装机 config
（`modules/rr3-sense/config.toml` 追加一行注释），要求门非 0 退出并点名
`DRIFT modules/rr3-sense/config.toml`；随即还原并重新判一致。抽 `drift_probe` 单点命令，
正负例共用同一参数，不写第二套。判据——integration run `46026798086` / job
`140600166786` success，notice 逐字："改一件 config 后漂移门判 DRIFT 并点名，还原后重回一致"；
同批时延 notice 两拍稳定（boot≈2.1s、换树≈2.4s、回滚≈33s）；同提交 gate success。

**2026-11-27 批223-224 窗口复核与本轮收口**：事实——`release-anon` runs 仍为 0（cron 首跑
2026-11-30 03:00 UTC 未到）；GitHub releases 最新 v0.0.8 资产仍无 distro bundle，`--deep`
深检与 B.2 签名实测无真包，按登记不冒充。处置——本轮 install-smoke 三批（217 正例/回滚、
218 真包/漂移门正例、221 时延、222 漂移门负例）收口；下轮四件：① 11-30 后 release-anon
首跑归因；② distro 首发后 `--deep` 与 B.2 实测；③ install-smoke 回滚检测时延优化评估
（33s 由 30s 探针窗口主导，先取证再定是否收窄，禁止无据调参）；④ 三线收口与再拟下轮。
判据——`0a3ffd75` 上 gate（run `46026797980`）/ integration（run `46026798086`）/ platforms（run `46026797816`）三线 success；⑤⑬⑯、度量与流水线审计自检绿。

**2026-11-27 批223 gate 漂红的根因修复（测试夹具静默上界）**：事实——docs 提交 `3b4af7ae`
的 gate conventions 在 `killed_eof_skipped` 偶发红（同一代码 `0a3ffd75` 为绿；panic 落在
`rrd/tests/support/client.rs:118`）。根因——测试端注册为 `rr3-simb`/`rr3-sima` 后不再发帧，
而服务端注册后的读静默上界 = 2 × watchdog（`conn/read.rs`）：100/500ms 的窗口会在 1s 后
回收整条连接；用例要等外部 peer 启动/abort，CI 负载下超过 1s 就在 `rr3_read_state` 读到
EOF。处置——"旁观连接"收成单点 `rr3_connect_as`（3_600_000/3_600_001, never，注释写明
2×watchdog 上界），`connect_simb` 与 `sim_exit_restart` 的观察端改用它，重复注册代码一并
收掉；断言一条未放宽（仍要求 unregistered / 干净收尾）。判据——gate run `46031019507`
success（conventions 含 `cargo test --workspace`）；同提交 integration run `46031019241`
success；本地 rustfmt --check、⑤⑬⑯、度量与流水线审计自检全绿。

**2026-11-27 批224 本轮收口（三线回执与窗口登记）**：事实——`971dc2c5` 上 gate /
integration / platforms 三线 success；`release-anon` runs 仍为 0（cron 首跑 2026-11-30
03:00 UTC 未到）；GitHub releases 最新 v0.0.8 仍无 distro bundle，`--deep` 与 B.2 无真包，
均按登记不冒充。处置——本轮四批（221 时延回传、222 漂移门负例、223 夹具静默上界根因修复、
224 收口）结束；下轮五件：① 11-30 后 release-anon 首跑归因；② distro 首发后 `--deep`
深检与 B.2 验签实测；③ 回滚检测时延优化评估（33s 由 30s 探针窗口主导，先取证再定，禁止
无据调参）；④ 测试夹具静默上界巡检（`e2e_ctl`/`register_publish` 等短窗口被动端同病灶
排查，能收口到 `rr3_connect_as` 的一并收口）；⑤ 三线收口与再拟下轮。判据——三线 run
`46031019507`（gate）/ `46031019241`（integration）/ `46031019318`（platforms）success；
⑤⑬⑯、度量与流水线审计自检绿。

**2026-11-28 批225 被动注册统一长窗口（夹具盲区收口）**：事实——批223 只修了
`e2e_sim_peer` 两处；"服务端注册后读静默上界 = 2×watchdog（`conn/read.rs`）"是全局
机制，其余共享夹具仍以 100/500 注册（`register_publish::register_sima`、
`e2e_ctl::ctl_status_setup`、`support/ctl::register_sima_task`），跨秒用例慢一拍同样
会读到 EOF。处置——新增 `rr3_register_passive`（3_600_000/3_600_001, never）作为
"被动注册"单点，四个共享夹具收口，`rr3_connect_as`/`rr3_connect_observer` 复用它；
support 头注写明"窗口本身是被测对象才写短窗口"，`ctl_echoes_numbers` 因断言
hb/wd 数值保留 200/900。断言一条未改。判据——gate run `46034271402` success
（conventions 含 workspace 全测）；integration run `46034271535` success；本地
rustfmt --check、⑤⑬⑯、度量与审计自检全绿。

**2026-11-28 批226 装机四段用时上成功/回滚行**：事实——install-smoke 的 32~33s 回滚
只有 CI 侧阶段数字，真板操作者看不到 install-bundle 内部慢在哪。处置——install-bundle
记停机 / 换树 / 二进制 / 启动探针四段毫秒，PASS 行与回滚、首装失败行逐字带出；
deploy README 的 A10 节注明"排障先读这一行"，status 敞口 29 登记为板侧载体。判据——
integration run `46035110220` / job `140628421670` success（install-smoke 六条 notice：
换树 3083ms、回滚 32077ms 等，说明新行格式与断言相容）；gate run `46035110266`
success；bundle 正例/回滚断言未改仍绿。

**2026-11-28 批227-228 窗口复核与本轮收口**：事实——`release-anon` runs 仍为 0（cron 首跑
2026-11-30 03:00 UTC 未到，今日 11-28）；GitHub releases 最新 v0.0.8 仍无 distro bundle，
`--deep` 与 B.2 无真包，按登记不冒充。处置——本轮三批（225 夹具长窗口、226 装机四段
用时、227/228 收口）结束；下轮五件：① 11-30 后 release-anon 首跑归因；② distro 首发后
`--deep` 深检与 B.2 验签实测；③ 回滚 32s 的取证与优化评估（install-bundle 四段用时已
就位，先取证再定，禁止无据调参）；④ 瑞芯微 aarch64 deb 结构门（apt 交叉 gnu + brain +
rr3-pack 出 deb，断言 control/unit/六模块/契约；不冒充安装——install-smoke 仅 x86_64、
bundle 为 musl 五模块，aarch64 deb base 层目前只有真板证据）；⑤ 三线收口与再拟下轮。
判据——`80cb04ee` 上 gate（run `46035110266`）/ integration（run `46035110220`）/
platforms（run `46035110210`）三线 success；⑤⑬⑯、度量与流水线审计自检绿。

**2026-11-28 批229 四段用时并进公开 notice（回滚 32s 归因取证）**：事实——install-smoke 的
32.5s 回滚此前只有总时长；install-bundle 批226 已自报四段，但 job 日志匿名不可读。处置——
install-smoke 解析 install-bundle 成功行的四段与回滚行的"本次尝试"，并进公开 notice。
判据——integration run `46039090742` / job `140641911825` success，notice 逐字：正例
"停机 22ms / 换树 6ms / 二进制 276ms / 启动探针 1042ms"，坏包"本次尝试 30278ms"；同提交
gate run `46039090687` success。结论：回滚 32.5s 由 30s 健康探针窗口主导，停机/换树/二进制
合计 <1s；据此不做无据调参，优化决策留待真板坏包场景取证。

**2026-11-28 批230 瑞芯微 aarch64 deb 结构门**：事实——install-smoke 只覆盖 x86_64 真装；
distro bundle 是 aarch64-musl 五模块（brain 不在 musl 交叉面）；aarch64 glibc deb（六模块含
brain）此前只有真板证据。处置——新增 platforms `arm64-deb` 作业与
`tools/ci/platform/arm64-deb-check.sh`：复用 `cross-artifacts`（gnu + with-brain）交叉构建
十件，`rr3-pack` 出 arm64 deb，断言 control 三字段、包内三入口/unit/四册/平台册/接线/
六模块各三件、unit 的 `--profile` 装机位、postinst 可执行位、九件 ELF 全 aarch64；
**不冒充安装**（systemd 不能跑在 qemu-user 里）。覆盖契约与缓存纪律同批更新
（REQUIRED_JOBS / JOB_STEPS / CACHE_ALL_JOBS）。判据——platforms run `46039828675` 的
`arm64-deb` job `140644548064` success（该 run 的 platforms 整体随后被批231 取代，作业本身
已结）；同提交 gate run `46039828646`、integration run `46039828659` success。

**2026-11-28 批231 结构门结果公开回传**：事实——门绿只在不可读日志里，公开面无法复核
aarch64 deb 的成员与架构。处置——`arm64-deb-check.sh` 末尾发一条 notice（包名/版本/架构 +
成员面 + 九件 ELF 结论）。判据——platforms run `46040658922` / job `140647369395` success，
notice 逐字："rockrobot3 0.0.8 arm64；六模块/四册/平台册/接线/unit/postinst 齐，九件 ELF
全 aarch64（结构门，不冒充安装）"；同提交 gate run `46040659819`、integration run
`46040658896` success，platforms run `46040658922` 整体 success。

**2026-11-28 批232 窗口复核与本轮收口**：事实——`release-anon` runs 仍为 0（cron 首跑
2026-11-30 03:00 UTC 未到，今日 11-28）；GitHub releases 最新 v0.0.8 仍无 distro bundle，
`--deep` 与 B.2 无真包，按登记不冒充。处置——本轮四批（229 回滚归因回传、230 aarch64 deb
结构门、231 公开回传、232 收口）结束；下轮五件：① 11-30 后 release-anon 首跑归因；
② distro 首发后 `--deep` 深检与 B.2 验签实测；③ 真板坏包场景取证与 30s 探针窗口决策
（公开数据已指向探针，改前要真板证据）；④ arm64 deb 结构门负例（改坏 control/模块成员，
期望门红并点名）；⑤ 三线收口与再拟下轮。判据——`4d4e3765` 上 gate / integration /
platforms 三线 success；⑤⑬⑯、度量与流水线审计自检绿。

**2026-11-28 批233 arm64 deb 结构门三条负例**：事实——正例只证明"好包能过"，门能不能看见
坏包没有证据。处置——以正例 deb 为底 `dpkg-deb -R` 解包、改坏一处、重打包，三条负例：
control 架构改 amd64、删 `modules/rr3-brain/rr3-brain`、把 rrd 换成 x86-64 ELF；每条要求
断言非 0 **且点名根因**（只判红不判原因等于没测）。判据——platforms run `46043732878` /
job `140657649494` success，notice 逐字："负例 3/3 被抓：control 架构 / 模块缺件 /
ELF 架构（门看得见）"；同提交 gate run `46043732980`、integration run `46043732987`
success。

**2026-11-28 批234 窗口复核**：事实——`release-anon` runs 仍为 0（cron 首跑 2026-11-30
03:00 UTC 未到，今日 11-28）；GitHub releases 最新 v0.0.8 只有 9 个固件资产，仍无 distro
bundle——`--deep` 与 B.2 无真包。处置——两项按登记不冒充，触发条件不变。

**2026-11-28 批235 本轮收口**：事实——`3a400e52` 上 gate / integration / platforms 三线
success；本轮 233 把 aarch64 deb 结构门从"正例绿"补到"正例 + 3 条负例"，234 窗口未到。
处置——下轮五件：① 11-30 后 release-anon 首跑归因；② distro 首发后 `--deep` 与 B.2 实测；
③ 真板坏包场景取证与 30s 探针窗口决策；④ 运行面成员清单单点化（六模块/三入口在
install-smoke、distro-smoke、arm64-deb、publish 多处字面量，改为从 `modules/*/module.toml`
派生，musl 五模块例外显式声明）；⑤ 三线收口与再拟下轮。判据——三线 run `46043732980`
（gate）/ `46043732987`（integration）/ `46043732878`（platforms）success；⑤⑬⑯、度量与
流水线审计自检绿。

**2026-11-28 批236-238 窗口与外部证据登记**：事实——`release-anon` runs 仍为 0（cron 首跑
2026-11-30 03:00 UTC 未到，今日 11-28）；GitHub releases 最新 v0.0.8 只有 9 个固件资产，
无 distro bundle（`--deep` 与 B.2 无真包）；真板坏包场景无载具。处置——三项按登记不冒充，
触发条件不变（11-30 / distro 首发 / 真板）；本轮把可做的清单单点化提前（批239）。

**2026-11-28 批239 运行面模块清单单点化**：事实——六模块/三入口在 cross-artifacts、
distro-smoke、install-smoke、arm64-deb、pack-bundle、bundle-contract 各抄一份字面量；
新增模块漏改任一处，流水线仍绿而某个载体静默缺件。处置——新增
`tools/ci/lib/runtime-members.sh`（真值 = `modules/rr3-*/module.toml`，空清单即失败），
六个消费方改为派生；musl bundle 的 brain 例外在两处同源显式声明（ring 需目标 C 工具链，
归板上原生构建）；发布镜像/发布清单仍显式列六件（发布决策，由
`workflow_release.PUBLISH_MODULES` 钉住）。审计修一处：distro-smoke 的 `$MODN` 紧跟
全角括号被 ⑬ 抓（bash 3.2 下 unbound 风险），改 `${MODN}`。判据——gate run
`46046977868`（含 bundle-contract 自检 5/5）、integration run `46046977876`（distro-smoke
与 install-smoke 派生后全绿，notice "全模块 running"）、platforms run `46046977799`
（cross ×3 与 arm64-deb 全绿，notice "全模块 … 负例 3/3"）；本地
bundle-contract --self-test 5/5、⑤⑬⑯、度量与审计自检绿。

**2026-11-28 批240 本轮收口**：事实——`332752a8` 上 gate / integration / platforms 三线
success。处置——下轮五件：① 11-30 后 release-anon 首跑归因；② distro 首发后 `--deep`
与 B.2 实测；③ 真板坏包场景取证与 30s 探针窗口决策；④ 清单单点化收尾——stage-image /
image-smoke / pack-arm64 / publish 仍显式，逐项裁定"发布策略（保留显式）"还是"运行面
真值（改派生）"，先出清单再动；⑤ 三线收口与再拟下轮。判据——三线 run
`46046977868`（gate）/ `46046977876`（integration）/ `46046977799`（platforms）success；
⑤⑬⑯、度量与流水线审计自检绿。

**2026-11-28 批241-243 窗口与外部证据登记**：事实——`release-anon` runs 仍为 0（cron 首跑
2026-11-30 03:00 UTC 未到，今日 11-28）；v0.0.8 release 仅 9 个固件资产，无 distro bundle；
真板坏包场景无载具。处置——三项按登记不冒充；本轮把清单单点化收尾提前（批244/245）。

**2026-11-28 批244 清单单点化收尾裁定 + pack-arm64 派生**：事实——stage-image /
image-smoke / pack-arm64 / publish 仍显式列模块，前一轮只把测试/安装门改成派生。裁定——
发布镜像与发布清单（stage-image / image-smoke / publish）属**发布策略**，保留显式，
由 `workflow_release.PUBLISH_MODULES` 钉住并在三处加注"勿与派生面混"；板侧部署工具
`pack-arm64` 属**运行面真值**，改为 `rr3_modules` 派生（brain 例外显式：musl 无 C 工具链，
manifest 先落位、bin 由板上 build-onboard 回填），与 pack-bundle 同源。判据——commit
`30f987c7` 的 gate run `46049793002` success（hygiene/审计）；pack-arm64 不在 CI 执行，
其证据为 bash -n、⑤⑬⑯、流水线审计与逐行评审——**如实记，不冒充执行回执**。

**2026-11-28 批245 install-smoke 静默失败诊断升级 + 本轮收口**：事实——`30f987c7` 的
integration 里 install-smoke 失败，公开注解只剩 "exit code 1"（systemd notice 之后、
bundle notice 之前，说明是未设防的裸命令退出）。定位——该区间唯一不设防的命令是
install-bundle 成功行的四段用时**信息提取**（`grep -o` 无命中即 1，set -e 静默退）。
处置——信息提取改容错（`|| true`，诊断缺失不得判死安装门）；ERR trap 改为先打
`FAIL  意外退出：${BASH_COMMAND}` 再 dump 现场，下一次再现即可点名失败命令。判据——
commit `1581ae2e`：gate run `46050687789`、integration run `46050687975`（install-smoke
job `140680937458` 六条 notice 齐，含四段用时与负例）、platforms run `46050687890`
三线 success；⑤⑬⑯、度量与审计自检绿。根因未复现到逐字证据，故按"信息提取不得判死
安装门"的机制原则修复并留点名能力，不宣称已定位原始触发条件。

**2026-11-28 批246 下轮任务拟定**：① 11-30 后 release-anon 首跑归因；② distro 首发后
`--deep` 与 B.2 实测；③ 真板坏包场景取证与 30s 探针窗口决策；④ 发布/部署面清单一致性
巡检单点（publish 显式六件 vs stage-image/image-smoke；pack-arm64/bundle 派生集）——
先出对拍脚本与文档，不接门；⑤ 三线收口与再拟下轮。

**2026-11-28 批250 发布/部署面清单对拍单点**：事实——模块清单在四处独立写法
（`workflow_release.PUBLISH_MODULES`、`publish.yml` 构建循环、`stage-image.sh` 两处装机
循环、`image-smoke.sh` 起齐循环与 `-eq` 阈值），板侧 `pack-bundle.sh` / `pack-arm64.sh`
另按真值派生并留 brain 例外；新增模块时漏改任一处，流水线仍绿而某载体静默缺件。
处置——立 [release_sets.py](../../tools/ci/audit/release_sets.py)：真值取
`modules/rr3-*/module.toml`，判据是**集合相等**（策略必须等于真值；镜像三处必须等于
策略；派生脚本必须 source 单点、保留 brain 例外、且不得出现字面量循环），缺/多逐项
点名；`--self-test` 1 正例 + 6 负例（发布循环漏件 / 装机漏件 / 起齐漏件 / 两个派生脚
失去 brain 例外 / 新增模块未入策略）。判据——自检 7/7 绿；真仓四面对拍 PASS（真值 6 ·
策略 6 · 镜像同集 · 派生 5 = 真值−brain），解析结果逐文件人工核对（publish:75、
stage-image:49/57、image-smoke:80 各命中真实循环）。本轮不接 check.sh，先随批观察。

**2026-11-28 批251 窗口复核（release-anon / distro 首发）**：事实——`release-anon.yml`
运行数 0（首跑窗口 2026-11-30 03:00 UTC 未到，今日 11-28）；最新 release 仍为 v0.0.8
（2026-11-26T04:55:16Z），资产 9 件全为固件面（双 ELF、双 bootloader、合并 bin、
分区表、FLASHING、sha256），**无 distro bundle**。处置——窗口未到不造夹具冒充：
`release-verify --deep` 与 runbook B.2 仍缺真包，真板坏包场景仍无载具，两项如实留欠，
不在门禁里写"看起来验过了"。判据——GitHub API 事实：
`release-anon.yml/runs total_count=0`；`releases` 最新条目资产名单逐项核对为固件九件。

**2026-11-28 批251 下轮任务拟定**：① 批252 把 `release_sets.py` 接门（check.sh ⑫ 块
加自检+执法、gate `scripts` job 加同名两行、ci/README 撤"暂未接"）——已立机制不跑等于
没有；② 批253 把 brain 例外从 `pack-bundle.sh` / `pack-arm64.sh` 两处收敛进
`runtime-members.sh`（新增派生入口），并让 `release_sets.py` 核"板侧包走派生入口"；
③ 批254 三线查红与回执收口，复核 11-30 窗口是否触发；④ 每步 commit+push 三远端。

**2026-11-28 批252 清单对拍接门（⑰）**：事实——批250 立的 `release_sets.py` 若无任何
门调用，等价于死代码；"已立机制真正发挥作用"要求它进入例行执法位。处置——新增 ⑰
步骤（[steps/17.sh](../../tools/ci/steps/17.sh) 只执法，自检归 gate `scripts`），
`check.sh` 调用序与 [gate_steps.txt](../../tools/ci/baseline/gate_steps.txt) 逐字登记
（30 步）；gate `scripts` job 增自检（1 正例 + 6 负例）与执法两行；ci/README 步骤表、
对拍章节与"暂未接"表述同批订正。判据——`gate_steps_audit.py` PASS（30 步逐字同序）；
`workflows_audit.py` PASS；`bash -n` 两脚本通过；⑰ 单步执行 PASS（真值 6 · 策略 6 ·
镜像同集 · 派生 5）；⑤ 文档门与 ⑯ 行数门绿（752 源文件、硬线 0）。

**2026-11-28 批253 真值面成对（⑰ 扩面）**：事实——模块真值 = `modules/rr3-*/module.toml`
的 glob；新建模块 crate 却漏写 `module.toml` 时，真值、发布策略、镜像、板侧包**四处
同时看不见它**，流水线全绿，装上板才发现少一个模块——⑰ 此前只对拍"已有清单之间"，
未核真值面自身完整性。处置——`release_sets.py` 增成对判据（`Cargo.toml` 与
`module.toml` 缺一即点名，双向），并入同一门 ⑰（不新增门号）；夹具补 Cargo.toml 并加
第 7 条负例（ghost crate）。判据——`--self-test` 8/8 绿；真仓对拍 PASS（6/6/同集/派生 5）；
真文件镜像注入 `modules/rr3-ghost/Cargo.toml` 后 ⑰ 点名"缺 module.toml"退 1；
⑯ 行数门 PASS（脚本 318 行）。

**2026-11-28 批254 本轮收口（⑰ 三线实证）**：事实——`4cf1d121` 三线全绿：
gate `46054103788` / integration `46054103533` / platforms `46054103755`；其中 gate
`conventions`（check.sh 全步含 ⑰）success、`scripts`（⑰ 自检 + 执法两行）success，
scripts 作业 19s 完成（纯 Python，不编译）。中途 `937bb3a2` / `b05ea77a` 的
conventions/release 为 `cancelled`——被后续提交的并发组取代，非真红（三线在最新提交
全绿）。处置——同批补齐规约文档：[conventions.md](conventions.md) 门表增 ⑰ 行、
两处"①–⑯"订正为"①–⑰"，避免门表与 check.sh 漂移。判据——`check_docs` 绿；
`gate_steps_audit` 30 步逐字一致；⑰ 本地自检 8/8、真仓 PASS。

**2026-11-28 批254 下轮任务拟定**：① 批255 装机回滚早停——`install-bundle.sh` 健康探针
读 `systemctl show -p NRestarts`，新树崩溃重启即回滚并点名"崩溃重启 N 次"；今日
`rollback_ms≈32.6s` 全耗在 30s 探针窗口，目标降到 <5s，证据取 install-smoke 公开
notice 的回滚用时与失败行。② 批256 ⑰ 阈值断言收紧为 `[ "$n" -eq N ]` 精确形状
（别处 `-eq 6` 不得掩盖计数漂移）+ 负例。③ 批257 11-30 03:00 UTC release-anon 首跑
归因；distro 首发后评估把 `distro` 类纳入匿名回读（现在纳入会对 v0.0.8 假红）。
④ 批258 三线收口与再拟下轮。铁律不变：每步 commit+push 三远端并查漂红。

**2026-11-28 批255 坏包回滚早停（NRestarts 增量）**：事实——install-smoke 回滚用时
32619ms 中 30s 是固定健康探针窗口：坏 rrd 起不来、`rr3-ctl` 永不应答，而 unit 在
`Restart=always` 下已反复重启——结论早已确定，窗口只是在计时。处置——`install-bundle.sh`
在 `reset-failed` 后记 `NRestarts` 基线，探针每秒读增量：涨了即"新树崩溃"，立即回滚并
打印 `FAIL 新树 rrd 崩溃重启 N 次（启动探针 Xms 内即判，不等满 30s）`；成功路径不受影响
（正常启动计数不涨）。判据——commit `ee9d7bdf`：gate `46056257563` success、integration
`46056257566` success；install-smoke 公开 notice 实测 `rollback_ms=4513`（本次尝试
2205ms，旧 32619ms，降 86%），同拍 `boot_systemd_ms=2137`、`bundle_swap_ms=2449`
（停机 26 / 换树 7 / 二进制 21 / 启动探针 1773ms）——成功路径无回归，六条 notice 齐。
status.md 第 29 行按实测值订正。

**2026-11-28 批256 下轮任务拟定**：① ⑰ 阈值断言收紧为 `[ "$n" -eq N ]` 精确形状
（别处 `-eq 6` 不得掩盖计数漂移）+ 一条"无关 -eq 6 遮蔽"负例；② 三线收口复核
（gate/integration/platforms on `ee9d7bdf` 全绿后回执）；③ 11-30 03:00 UTC
release-anon 首跑归因窗口（今日 11-28 未到，不得预写结论）；④ distro 首发后
`--deep` / B.2 实测与 `distro` 类纳入匿名回读的裁定；⑤ 再拟下轮。

**2026-11-28 批256 ⑰ 阈值断言收紧**：事实——起齐阈值判据原是全文 `-eq N` 正则，文件里
任何一处无关的 `-eq 6`（重试上限、超时参数）都能掩盖"循环改了、阈值没改"的漂移，
把假红变成假绿。处置——正则收紧为断言形状 `[ "$n" -eq N ]`；断言缺失单独点名（不再
静默通过）；新增第 8 条负例（`-eq 9` + 无关 `-eq 6` 遮蔽必须被抓）。判据——
`--self-test` 9/9 绿（1 正例 + 8 负例）；真仓对拍 PASS；⑤ 文档门与 ⑯ 行数门绿
（脚本 326 行）。

**2026-11-28 批257 本轮收口（6 提交三线全绿）**：事实——本轮提交 `937bb3a2`（⑰ 脚本）→
`cd294765`（窗口复核）→ `b05ea77a`（接门）→ `4cf1d121`（成对判据）→ `66260daf`
（门表订正）→ `ee9d7bdf`（回滚早停）→ `109352fc`（回执/状态）→ `1e360c99`（阈值收紧），
最终提交三线全绿：gate / integration / platforms（run 见各提交 check-runs，均 success）；
中间提交的 `cancelled` 均为并发组被后续提交取代，非真红。关键实证：gate `scripts` 19s
success（⑰ 自检 + 执法），gate `conventions` success（check.sh 全步含 ⑰）；
install-smoke 公开回读 `rollback_ms=4513`（旧 32619ms）、`boot_systemd_ms=2137`、
`bundle_swap_ms=2449`——回滚早停生效且成功路径零回归。判据——`release_sets --self-test`
9/9、真仓 PASS；`gate_steps_audit` 30 步；⑤⑯ 绿；三远端同指 `1e360c99`。

**2026-11-28 批257 下轮任务拟定**：① 批258 `distro-smoke` 时延公开回传——把
`boot_spawn6_ms` / `status_rtt_ms` 以 `::notice title=distro-smoke-timing::` 打到注解面
（与 install-smoke 同通道，日志与 job summary 匿名不可读），解除 status 第 29 行
"数值未取回"的阻塞；② 批259 用连续两拍实测值校准 20s/5s 上限（无数据不调参，保留
余量）；③ 批260 11-30 03:00 UTC `release-anon` 首跑归因；distro 首发后 `--deep` /
B.2 实测与 `distro` 类纳入匿名回读的裁定；④ 批261 三线收口与再拟下轮。

**2026-11-28 批258 distro-smoke 时延公开回传**：事实——`distro-smoke` 的
`boot_spawn6_ms` / `status_rtt_ms` 只写 job summary 与日志（summary 需鉴权、日志匿名
403），status 第 29 行"上限贴合实测"因此长期缺证据。处置——容器退出后在宿主把 timing
文件拼成一行 `::notice title=distro-smoke-timing::`（与 install-smoke 同通道），数字
进注解面。判据——commit `2d3ae48a`：integration success，distro-smoke job
`140710054135` 注解实测 `boot_spawn6_ms=1010 ceiling_ms=20000`、`status_rtt_ms=2
ceiling_ms=5000`；install-smoke 同拍 success（rollback 早停无回归）。上限本批不动：
单拍不足以校准，第二拍后再按实测余量收紧。

**2026-11-28 批259 distro-smoke 时延上限校准（两拍实测）**：事实——批258 后注解面
连续两拍可读：`2d3ae48a` 拍 `boot_spawn6_ms=1010` / `status_rtt_ms=2`，`3eb4ffbf` 拍
`boot_spawn6_ms=1014` / `status_rtt_ms=2`（偏差 0.4%）。处置——上限由保守值
20000/5000 收紧到 10000/1000（boot 约 10 倍余量，status 只拦总线卡顿/守护假死），
两处上限行带校准出处；status 第 29 行由"未校准"改为已校准并记两拍原值。判据——
`3eb4ffbf` integration success，distro-smoke job `140712122709` 注解实测
`boot_spawn6_ms=1014 ceiling_ms=10000`、`status_rtt_ms=2 ceiling_ms=1000`；install-smoke
同拍 success。

**2026-11-28 批260 二段收口（⑰ 治理 + 回滚时延 + 时延上限）**：本轮二段四件全部落地
并各有公开实证——① ⑰ 接门后 gate `scripts` 19s、`conventions` 全步绿；
② 真值成对判据（模块 crate 漏写 module.toml 即红）自检 9/9；③ 坏包回滚 `NRestarts`
早停：install-smoke `rollback_ms=4513`（旧 32619ms，降 86%），成功换树 2449ms / boot
2137ms 无回归；④ distro-smoke 时延上注解面并两拍校准：`boot_spawn6_ms=1010/1014`
（上限 10000）、`status_rtt_ms=2/2`（上限 1000）。三线最终提交 `3eb4ffbf`
gate/integration/platforms 全 success；文档提交 `777e4def` gate success
（integration/platforms 按 paths-ignore 跳过）。三远端同指 `777e4def`。
判据——各 run/job id 与注解在本段回执逐条在案；`release_sets --self-test` 9/9；
`gate_steps_audit` 30 步；⑤⑯ 绿。

**2026-11-28 批260 下轮任务拟定**：① 批261 ⑰ 处置提示——新增模块触发失败时点名
"同批改 workflow_release.PUBLISH_MODULES / stage-image.sh / image-smoke.sh 三处"，
让报错自带改法；② 批262 11-30 03:00 UTC `release-anon` 首跑归因（窗口未到不预写；
首跑红则按 check-run 注解与出参定位）；③ 批263 distro 首发后 `--deep` / runbook B.2
实测，并裁定 `distro` 类是否进匿名回读（当前纳入会对 v0.0.8 假红）；④ 批264 复核
install-smoke 回滚新路径连续三拍稳定（4513ms 为单拍），三线收口与再拟下轮。

**2026-11-28 批261 ⑰ 漂移报错自带改法**：事实——新增模块触发 ⑰ 时，原报错只说
"策略与真值不一致：缺 X"，改哪几处仍要靠读脚本（三处字面量清单，漏一处就复发）。
处置——`check_policy` 的失败行追加"新增模块须同批改三处：`workflow_release.PUBLISH_MODULES`、
`stage-image.sh`、`image-smoke.sh`"，自检新增一条断言（报错必须含"同批改三处"）。
判据——`--self-test` 10/10 绿（1 正例 / 8 负例 / 10 断言）；真仓 PASS。

**2026-11-28 批262 回滚新路径三拍稳定与窗口复核**：事实——① install-smoke 回滚新路径
连续三拍公开回读：`ee9d7bdf` 4513ms（本次尝试 2205ms）、`2d3ae48a` 4359ms（1317ms）、
`3eb4ffbf` 4493ms（2362ms）——均 ~4.4s，旧值 32619ms，波动在探针 1s 粒度内，判稳；
同拍 boot_systemd 1651/2092/2137ms、bundle_swap 2319/2378/2449ms，无回归趋势。
② 窗口未变：`release-anon.yml` runs 仍为 0（首跑 11-30 03:00 UTC 未到）；最新 release
仍 v0.0.8，资产 9 件无 distro——`--deep`/B.2 与真板坏包场景继续如实留欠。处置——不改
判据、不预写窗口结论；三拍数字入册为回滚路径基线。判据——API 事实与 check-run 注解
逐条在案（job 140699361128 / 140710053699 / 140712122738）。

**2026-11-28 批263 探针细粒度无实证收益，回退**：事实——安装健康探针由 1s 改 0.2s
粒度并改截止时间后，install-smoke 实测 `bundle_swap_ms=2531`（基线三拍
2319/2378/2449）、`rollback_ms=4453`（基线 4359/4493/4513）、本次尝试 2235ms
（基线 1317/2205/2362）——三项均无改善；等待时间由模块真实启动与 systemd 重启节奏
主导，不是轮询粒度。处置——`git revert` 整体回退（文件与 `14dff030` 逐字一致，
diff 0 行），不做"留着也许有用"的复杂度留存。判据——install-smoke job
`140727013254` 注解；回退后 `bash -n` 通过、与改动前 diff 为空。

**2026-11-28 批264 本轮收口（⑰ 提示 / 基线 / 负结果回退）**：事实——本轮四件：
① ⑰ 漂移报错点名三处改法（自检 10/10）；② 回滚新路径三拍基线入册
（4359/4493/4513ms）；③ 探针 0.2s 粒度实验无实证收益（2531/4453ms vs 基线
2319-2449/4359-4513），整体 `git revert` 且文件与改动前 diff 0 行；④ 回退后第四拍
`bundle_swap_ms=2321`、`rollback_ms=4374`（本次尝试 2159ms），回到基线带内。
最终提交 `4fd72e8a` 三线全绿：gate / integration / platforms 均 success；三远端同指。
判据——install-smoke job `140727013254`（实验拍）与 `140729061012`（回退拍）注解；
`release_sets --self-test` 10/10；⑤⑯ 绿。

**2026-11-28 批264 下轮任务拟定**：① 批265 `install-smoke` 时延上限门——现有回执
只有测量没有门：回滚从 32619ms 降到 ~4.4s 后，回涨无人拦；按四拍基线
（rollback 4359/4374/4453/4513、bundle_swap 2319-2531、boot_systemd 1651-2137）设
保守上限（rollback ≤15000ms、swap ≤10000ms、boot ≤10000ms），超限即红并打印实测；
② 批266 11-30 03:00 UTC `release-anon` 首跑归因（窗口未到不预写；红按镜像/固件/签名
分类定位，绿则首份匿名回执入册）；③ 批267 distro 首发后 `--deep` / B.2 实测与
`distro` 类纳入匿名回读的裁定；④ 批268 三线收口与再拟下轮。

**2026-11-28 批265 install-smoke 三段时延上限门**：事实——批221 起三段时延只有测量
没有门：回滚从 32619ms 降到 ~4.4s 后，将来回涨到 30s 也无人拦（distro-smoke 有上限，
install-smoke 没有）。处置——新增 `tt_ceiling`（超限 `die` 并打印实测/上限），三段上限
boot 10000 / swap 10000 / rollback 15000ms（约 3-5 倍余量，只拦回涨），常量旁标注
四拍基线（4359/4374/4453/4513 等）；notice 同步带上限，公开面直接可判。判据——
commit `0d2e03f8`：integration success，install-smoke job `140736807175` 注解
`boot=2118ms/上限 10000ms`、`窗口=2545ms/上限 10000ms`、`恢复=3903ms/上限 15000ms`，
三段全在限内；distro-smoke 同拍 success。

**2026-11-28 批266 本轮收口（上限门 / 部署文档订正）**：事实——`tools/deploy/README.md`
仍写"三段时延只记录不判红"，与批265 的上限门相悖；陈旧表述会让人以为回归无人拦。
处置——同批订正为"带上限判据（boot/换树各 10000ms、回滚 15000ms，超限即红）"并指回
status 敞口 29 的基线。判据——commit `0d2e03f8` 三线全绿（gate / integration /
platforms 均 success），install-smoke job `140736807175` 三段实测均在限内
（2118/10000、2545/10000、3903/15000）；文档提交 `91dfb6c3` gate success；⑤ 文档门绿。
三远端同指 `91dfb6c3`。

**2026-11-28 批266 下轮任务拟定**：① 批267 11-30 03:00 UTC `release-anon` 首跑归因
（窗口未到不预写结论；红按镜像/固件/签名三类定位，绿则首份匿名回执入册）；② 批268
distro 首发后 `--deep` / runbook B.2 实测，并裁定 `distro` 类是否进匿名回读（当前纳入
会对 v0.0.8 假红）；③ 批269 时延上限门连续两拍观察，数据支持才收紧（不无据调参）；
④ 批270 三线收口与再拟下轮。

**2026-11-28 批267 crates 重试参数收敛单点**：事实——七个平台脚本（espidf-std /
sentinel / esp32 / esp32s3 periph-base / esp-uart-echo / rr3-act-sense / ipct003）各抄
同一份"3 行 export + 4 行注释"，策略一变要改七处，且 metrics 只扫 `.rs`，这类重复
无人抓。处置——立 [cargo-net.sh](../../tools/ci/lib/cargo-net.sh)（显式传值优先，
不覆盖调用方环境），七脚本在 root 确定后 source 单点。判据——`grep -rn
CARGO_NET_RETRY tools/` 仅 lib 一处命中；七脚本 `bash -n` 全通过；体量净减 42 行
（7 增 49 删）；⑯ 行数门 PASS（最大 220 行）。

**2026-11-28 批268 本轮收口（重试参数单点）**：事实——七个平台脚本各抄同一份重试
参数，策略变化要改七处且 metrics 只扫 Rust 抓不到。处置——`cargo-net.sh` 单点 + 七处
source，README 共享件表补登；净减 42 行。判据——commit `af4a2571` 三线全绿：gate /
integration / platforms 均 success（esp32 classic/S3、ipct003、riscv-image 等腿实跑
sourced 脚本），证明 source 路径与优先级正确；`grep CARGO_NET_RETRY` 仅 lib 一处；
⑯ 行数门 PASS（最大 220 行）；三远端同指 `af4a2571`。

**2026-11-28 批268 下轮任务拟定**：① 批269 11-30 03:00 UTC `release-anon` 首跑归因
（窗口未到不预写结论；红按镜像/固件/签名三类定位，绿则首份匿名回执入册）；② 批270
distro 首发后 `--deep` / runbook B.2 实测与 `distro` 类匿名回读裁定（当前纳入会对
v0.0.8 假红）；③ 批271 时延上限连续两拍观察（`af4a2571` 拍 boot 2118 / 换树 2545 /
回滚 3903，上一拍 2118/2545/3903 附近），数据支持才收紧；④ 批272 三线收口与再拟下轮。

**2026-11-28 批269 状态敞口 29 订正**：事实——该行仍写"上限暂不动，取第二拍后校准"
"不据此改 20s/5s"，而批259 已两拍校准、批265 已给 install-smoke 加三段上限；状态表
留旧话会让人误判回归无人拦。处置——同批订正为"已校准（10000/1000）"与"install-smoke
三段上限（boot/换树 10000、回滚 15000）"。判据——⑤ 文档门绿；状态行逐字核对无
"暂不动"/"不据此改"残留。

**2026-11-28 批270 回滚行归因**：事实——`rollback()` 的失败行只说"新树启动验证未过"，
板侧只读最后一行时无法区分系统调用失败、崩溃重启还是探针超时；批255 的崩溃原因只
留在 rollback 之前的独立行，最终 die 行反而丢了原因。处置——`rollback` 接受原因参数，
五处调用各带具体原因（二进制安装/换位、`systemctl start` 失败、崩溃重启 N 次、30s
探针超时），两条 die 行统一带"原因：…"；install-smoke 回滚例新增断言（原因必须命中
崩溃重启或 start 失败），防止未来退化成无归因。判据——commit `37ff12ae`：
install-smoke job `140751782663` success（断言通过），本拍 boot 2685/10000、换树
1521/10000、回滚 4405/15000；conventions / scripts / distro-smoke 同拍 success。

**2026-11-28 批271 本轮收口（回滚归因落地）**：事实——回滚最后一行现已带"原因：…"，
install-smoke 回滚例断言命中崩溃重启或 start 失败；本拍三段 2685/1521/4405ms 均在
上限内。窗口复核未变：`release-anon` runs=0（首跑 11-30 03:00 UTC 未到），最新 release
v0.0.8 资产 9 件无 distro。判据——`37ff12ae` 三线全绿（gate / integration /
platforms 均 success），文档提交 `926f4ac2` gate success；三远端同指 `926f4ac2`；
install-smoke job `140751782663` success（含新断言）。

**2026-11-28 批271 下轮任务拟定**：① 批272 回滚原因进公开 notice——install-smoke 从
install-bundle 输出提取"原因：…"并入 `install-smoke-rollback` 注解，匿名面即可读到
失败归因（现在原因只在 job 日志，403 不可读），并加一条 notice 含原因的断言；
② 批273 11-30 03:00 UTC `release-anon` 首跑归因（窗口未到不预写；红按镜像/固件/签名
三类定位，绿则首份匿名回执入册）；③ 批274 distro 首发后 `--deep` / B.2 实测与
`distro` 类匿名回读裁定；④ 批275 时延上限第三拍观察 + 三线收口与再拟下轮。

**2026-11-28 批272 回滚原因进公开 notice**：事实——批270 的原因只落在 job 日志
（匿名 403），公开注解仍只有"回滚成立"，外部回执看不到失败归因。处置——install-smoke
从 install-bundle 输出提取"原因：…"并入 `install-smoke-rollback` 注解，并断言注解
必含原因（缺则红），防止未来退化成无归因。判据——commit `9753f414`：install-smoke
job `140759568395` success，公开注解逐字含"原因：rrd 崩溃重启 1 次（启动探针 2053ms
内即判）"；本拍 boot 2091/10000、换树 2315/10000、回滚 4356/15000。

**2026-11-28 批273 本轮收口（回滚原因公开可读）**：事实——回滚原因已进公开注解，
外部回执不再需要登 runner 读日志；本拍三段 2091/2315/4356ms 均在上限内，boot_systemd
近四拍 1651-2685ms 波动约 1.6 倍，上限 10000ms 留 3.7 倍余量，暂不收紧。窗口复核
未变：`release-anon` runs=0（11-30 未到）、v0.0.8 无 distro。判据——`9753f414` 三线
全绿（gate / integration / platforms 均 success），文档提交 `de6e880b` gate success；
三远端同指 `de6e880b`。

**2026-11-28 批273 下轮任务拟定**：① 批274 `install-bundle.sh` 补 ERR trap——该脚本
无 ERR 陷阱，`set -e` 下的裸命令失败（如 chmod/install 之外的意外）只会以退出码
结束，板侧看不到失败命令；按 install-smoke 批245 同款先打 `FAIL 意外退出：<命令>`
再退出，并在 install-smoke 断言该能力可用（构造一次预期外失败的成本高，改为静态：
trap 行存在且语法过）；② 批275 11-30 03:00 UTC `release-anon` 首跑归因（红按镜像/
固件/签名三类定位，绿则首份匿名回执入册）；③ 批276 distro 首发后 `--deep` / B.2
实测与 `distro` 类匿名回读裁定；④ 批277 三线收口与再拟下轮。

**2026-11-28 批274 install-bundle 意外退出归因**：事实——该脚本只有 die/rollback 两条
显式失败路径，`set -e` 下的裸命令失败只留退出码，板侧看不到失败命令与行号。处置——
补 ERR trap：`FAIL 意外退出：<命令>（行 N；装机树 …）`（与 install-smoke 批245 同款）；
成功路径与预期回滚路径不触发（回滚经 die 退出，不走 ERR）。判据——commit `94a4cd73`：
conventions / install-smoke / distro-smoke 同拍 success；本拍三段 2133/2437/4852ms
均在限内，回滚注解含"原因：rrd 崩溃重启 1 次（启动探针 2078ms 内即判）"。

**2026-11-28 批275 本轮收口**：本轮交付——回滚行带原因（批270）、原因进公开注解
（批272）、install-bundle 补 ERR 归因（批274）；时延四拍 boot_systemd 1651-2685ms、
swap 1521-2545ms、rollback 3903-4852ms，全部远低于上限（10000/10000/15000），暂不
收紧。窗口复核未变：`release-anon` runs=0（11-30 未到）、v0.0.8 无 distro。判据——
`94a4cd73` 的 install-smoke 等作业 success；三远端同指 `94a4cd73`（待本轮文档提交后
刷新）。

**2026-11-28 批275 下轮任务拟定**：① 批276 11-30 03:00 UTC `release-anon` 首跑归因
（窗口未到不预写；红按镜像/固件/签名三类定位，绿则首份匿名回执入册）；② 批277
distro 首发后 `--deep` / runbook B.2 实测与 `distro` 类匿名回读裁定；③ 批278 跨载体
时延对照入册（distro 1010/1014ms 对 install 2133-2685ms，差在 systemd 拉起与环境，
不据此改判据）；④ 批279 三线收口与再拟下轮。

**2026-11-28 批276 ghcr 匿名交付面预检（首跑前基线）**：事实——`release-anon` 首跑
（11-30 03:00 UTC）未到，但"无凭据可下载"是公开承诺，需在窗口前确认当前状态。预检
（匿名 registry API + token 流程，只读）：镜像 `v0.0.8` 清单 200（amd64/arm64/riscv64
+ 3 条 attestation），签名 tag `sha256-07d1…9b3e.sig` 200；固件 `v0.0.8` 200（9 层，
与 release 资产同名），其签名 tag 200；`rockrobot3-distro` 403（未发布，符合现状）；
镜像 tag 集仅 `latest`/`edge*`/`v0.0.8*`。另查明固件仓 v0.0.17-23 属归档线
`archive/v0.0.x/*` 的历史制品，不是脏数据，`--tag auto` 只取镜像仓 tag 不受影响——已
写入 status 第 30 行，防止后续误清理。判据——HTTP 200/403 逐项在案；⑤ 文档门绿。

**2026-11-28 批277 签名身份离线核对**：事实——"签名 tag 存在"不等于"身份合规"，
周程验签若因身份判据不匹配会红在窗口里。核对（匿名拉 sig manifest → 层注解取证书 →
`openssl x509` 读 SAN/issuer）：镜像合并清单与固件包两份证书均命中 SAN
`https://github.com/rockrobot3/rockrobot3/.github/workflows/publish.yml@refs/tags/v0.0.8`、
issuer `https://token.actions.githubusercontent.com`，与 `release-verify` 的身份正则
同源。处置——status 第 30 行补记该结论，首跑窗口只需确认"跑起来"，判据本身已预验。
判据——两份证书 `Not After` 分别为 2026-11-26 05:00:30Z / 05:05:29Z（签名时刻有效），
SAN/issuer 逐字在案；⑤ 文档门绿。

**2026-11-28 批278 本轮收口（公开交付面预验）**：本轮无代码改动，交付为窗口前的
**可证结论**：镜像与固件两类公开制品匿名可读（200）、九层资产齐、双签名存在且身份
离线核对合规（SAN/issuer 与 release-verify 判据同源），distro 未发布（403）符合现状；
归档线固件制品（v0.0.17-23）语义已入 status 第 30 行，防止误清理。判据——匿名 API
逐项 HTTP 码与证书字段在案；最终文档提交 `6b0a6e37` gate success；三远端同指
`6b0a6e37`。

**2026-11-28 批278 下轮任务拟定**：① 批279 11-30 03:00 UTC `release-anon` 首跑归因
——判据已预验，只需确认 cron 真触发（runs>0）并按注解记录 tag/类别结论；若红，按
镜像/固件/签名三类逐项定位；② 批280 distro 首发后 `--deep` / runbook B.2 实测与
`distro` 类匿名回读裁定；③ 批281 把"归档线 ghcr 制品"语义补进
[tools/deploy/README.md](../../tools/deploy/README.md)（面向制品使用者，避免选错固件
版本）；④ 批282 三线收口与再拟下轮。

**2026-11-28 批281 部署 README 补归档线语义**：事实——固件仓可见 v0.0.17…v0.0.23，
版本号高于当前 0.0.8，使用者会误以为"有更新版本可拉"（实为归档线，编号重置）。
处置——部署 README 制品表后补一段：归档线不随当前线发布、编号高不代表更新、选版本
以镜像仓 tag 与 release 页为准（`release-anon` 的 `--tag auto` 同理只取镜像仓）。
判据——⑤ 文档门绿；status 第 30 行与本段互为指针。

**2026-11-28 批282 正式制品面清点（OTA/buildroot 标签口径过渡）**：事实——`release-anon`
只覆盖 image+firmware，而正式制品还有 OTA/buildroot 两包。匿名清点：`rockrobot3-ota`
与 `rockrobot3-buildroot` 在库只有 legacy `0.0.8`（无 v 前缀，2026-11-26 06:12 推送，
层齐：rootfs img/manifest/sig/公钥/checksums/bundle 与 zero3 img/双 dtb/checksums），
`v0.0.8` 不存在——buildroot.yml 的 `v<VER>` 口径修正发生在首发之后，下次发布才产生
v 前缀。处置——不改 release-anon 类别（现在纳入会对 v0.0.8 假红），把两包现状与扩面
条件写入 status 第 30 行；下轮发布后同批扩面。判据——registry 清单与 tags 列表逐项
在案；⑤ 文档门绿。

**2026-11-28 批283 本轮收口**：本轮交付为正式制品面的完整清点——镜像/固件（v0.0.8，
匿名可读、双签名身份合规）、OTA/buildroot（legacy `0.0.8`，层齐；v 前缀待下次发布）、
distro（未首发）；归档线固件制品语义已登记。`release-anon` 首跑窗口仍未到
（runs=0，11-30 03:00 UTC）。判据——匿名 registry 逐包清单与 tags 在案；⑰ 与 ⑤ 门绿；
三远端同指 `7569693b`。

**2026-11-28 批283 下轮任务拟定**：① 批284 11-30 03:00 UTC `release-anon` 首跑归因
（预检基线已备：镜像三架构 + 固件九层 + 双签名身份；确认 cron 真触发并按注解记录
tag/类别结论）；② 批285 下次 `v*` 发布后把 OTA/buildroot 纳入 `release-anon` 类别
（v 前缀口径，同批核 buildroot.yml 的 `release-verify --tag v${VER}` 实跑），并裁定
distro 类别的纳入时点；③ 批286 distro 首发后 `--deep` / B.2 实测；④ 批287 三线收口
与再拟下轮。

**2026-11-28 批284 制品标签口径入审计（补历史缺陷盲区）**：事实——buildroot.yml 注释
自述"v0.0.8 首发时此处曾漏 v"，但 `workflow_release.release_artifacts_problems` 只核
"推了哪个包"，不核标签形状——同一缺陷可再次静默发生（registry 现存 legacy `0.0.8`
即物证）。处置——该判据增一条：`rockrobot3-ota` / `rockrobot3-buildroot` 的引用必须是
`<name>:v${VER}`，否则点名"标签必须 v<VER>"；自检夹具把 ota 改成 v 口径并增第 31 条
负例（改回 `${VER}` 必被抓）。判据——`workflows_audit.py --self-test` 31/31 绿；
真仓审计 PASS；⑤ 文档门绿。

**2026-11-28 批285 本轮收口（标签口径门落地）**：事实——制品标签口径判据已在
gate `scripts`（自检 31/31 + 执法）与 `conventions`（check.sh）双点生效；真仓审计
PASS，证明 buildroot.yml 当前两包引用均为 `v${VER}`；`release-anon.yml` 工作流
state=active，cron 具备触发条件；首跑窗口仍未到（runs=0，11-30 03:00 UTC）。
判据——commit `ae9f764d` 三线全绿（gate / integration / platforms 均 success）；
三远端同指 `ae9f764d`。

**2026-11-28 批285 下轮任务拟定**：① 批286 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0，按注解记录 tag/类别结论；红按镜像/固件/签名三类定位；预检基线已在
status 第 30 行）；② 批287 下次 `v*` 发布后把 OTA/buildroot 纳入匿名回读（v 前缀
口径，判据已由批284 钉住），并核 buildroot 的 `release-verify --tag v${VER}` 实跑；
③ 批288 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批289 三线收口与
再拟下轮。

**2026-11-28 批286 匿名回读命令入审计**：事实——`release-anon` 此前只钉 job 名
（`REQUIRED_JOBS`），命令内容无人核：把 `--classes image,firmware` 砍成 `image`、
去掉 `--with-signature` 或改成固定 tag，周程仍绿而公开面覆盖静默消失（首跑未到，
正是最该先钉住的时刻）。处置——`workflow_release.anon_problems` 增四条判据（复用
release-verify.sh / `--with-signature` / `--classes image,firmware` / `--tag auto`），
自检增三条负例（丢签名 / 砍类别 / 固定 tag）。判据——`workflows_audit.py --self-test`
34/34 绿；真仓审计 PASS（现行命令四条全中）；⑤ 文档门绿。

**2026-11-28 批287 匿名回读周程触发入审计**：事实——`schedule:` 被删只剩
`workflow_dispatch` 时，workflow 仍 active、runs 永远 0，是"周程巡检静默消失"的最隐蔽
形态（正是首跑前最需要防的）。处置——`anon_problems` 增两条：必须有 `schedule` 段、
且段内有合法 `cron` 行；自检增负例（删 schedule 只剩手动必须点名）。判据——
`--self-test` 35/35 绿；真仓 PASS；⑤ 文档门绿。

**2026-11-28 批288 本轮收口（匿名回读面全部钉住）**：事实——`release-anon` 的 job 名、
命令四要素（复用判据 / `--with-signature` / `--classes image,firmware` / `--tag auto`）
与周程触发（`schedule` + cron）现已全部入审计，任何削弱当场红；自检 35/35。窗口仍未
到（runs=0，11-30 03:00 UTC），本轮把"首跑前能钉的先钉死"。判据——commit `b7ebf3f6`
三线全绿（gate / integration / platforms 均 success）；gate `scripts` /
`conventions` success；三远端同指 `b7ebf3f6`。

**2026-11-28 批288 下轮任务拟定**：① 批289 11-30 03:00 UTC `release-anon` 首跑归因
（命令/周程已钉；确认 runs>0 并按注解记录 tag 与类别结论；红按镜像/固件/签名三类
定位）；② 批290 下次 `v*` 发布后把 OTA/buildroot 纳入匿名回读（v 前缀已由批284
钉住）并核 buildroot 的 `release-verify --tag v${VER}` 实跑；③ 批291 distro 首发后
`--deep` / B.2 实测与 distro 类裁定；④ 批292 三线收口与再拟下轮。

**2026-11-28 批289 集成腿验证脚本入审计**：事实——`REQUIRED_JOBS` 只钉 job 名，把
`run: bash tools/ci/platform/install-smoke.sh` 换成 `echo` 后名与超时仍在、流水线照绿，
"腿名存实亡"没有门；平台腿早有 `JOB_STEPS` 同型判据，集成腿一直缺。处置——
`INTEGRATION_STEPS` 按名钉住七条腿的验证脚本（distro-smoke / install-smoke / ota-sign /
rk3566-onboard / buildroot-assembly / sim-assemble / sim-chain），缺脚本即点名；
自检夹具把 distro-smoke 换成 echo、其余腿保留脚本，新增负例（只该点名它）。判据——
`--self-test` 全绿（新增用例 PASS，无 FAIL）；真仓审计 PASS；⑤ 文档门绿。

**2026-11-28 批290 本轮收口（集成腿脚本钉住）**：事实——集成面七条验证腿的脚本
调用现已入审计（step 换 echo 即红）；与平台腿 `JOB_STEPS`、匿名回读命令、发布制品
标签口径共同构成本轮"名在场 → 行为在场"的补齐。窗口仍未到（runs=0）。判据——
commit `30c0f266` 三线全绿（gate / integration / platforms 均 success），gate
`scripts`（自检）与 `conventions` success；三远端同指 `30c0f266`。

**2026-11-28 批290 下轮任务拟定**：① 批291 11-30 03:00 UTC `release-anon` 首跑归因
（机制已全部钉住：job 名 / 命令四要素 / 周程触发；确认 runs>0 并按注解记录 tag 与
类别结论，红按镜像/固件/签名三类定位）；② 批292 下次 `v*` 发布后把 OTA/buildroot
纳入匿名回读并核 buildroot 的 `release-verify --tag v${VER}` 实跑；③ 批293 distro
首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批294 三线收口与再拟下轮。

**2026-11-28 批291 门禁主线判据入口入审计**：事实——gate 的 conventions 若把
`bash tools/ci/check.sh` 换成 `echo`，job 名与超时仍在、其余五作业照跑，整套门禁空转
而流水线全绿（比单腿失效更彻底）。处置——`GATE_STEPS` 按名钉住六作业的判据入口
（check.sh / release 的 `cargo test --release --workspace` / supply-audit --strict /
alsa feature / ota-bundle.sh / scripts 的四审计），缺即点名"门禁空转"；自检新增负例
（conventions 换 echo、其余入口保留，只该点名它）。判据——`--self-test` 36/36 绿；
真仓审计 PASS；⑤ 文档门绿。

**2026-11-28 批292 本轮收口（门禁入口钉住）**：事实——gate 六作业的判据入口现已
逐名入审计，配合前几轮的平台腿 / 集成腿 / 匿名回读 / 制品标签，"名在场"与"行为在场"
在发布与门禁两条主线上均已对齐；自检 36/36。窗口仍未到（runs=0）。判据——commit
`398bb0c2` 三线全绿（gate / integration / platforms 均 success）；三远端同指
`398bb0c2`。

**2026-11-28 批292 下轮任务拟定**：① 批293 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别结论；红按镜像/固件/签名三类定位）；② 批294
下次 `v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的
`release-verify --tag v${VER}` 实跑；③ 批295 distro 首发后 `--deep` / B.2 实测与
distro 类裁定；④ 批296 取证 coverage.yml / buildroot.yml 非发布作业是否也需钉判据
入口（先有证据再改，不无据扩面）；⑤ 批297 三线收口与再拟下轮。

**2026-11-28 批293 覆盖率观测入口入审计**：事实——coverage 两腿先跑 `cargo llvm-cov`
再 upload-artifact，而该 action 默认 `if-no-files-found=warn`：把 llvm-cov 换成 echo 后
job 仍绿、覆盖率数据静默消失（非阻断≠可以无声消失）。处置——`COVERAGE_STEPS` 按名钉住
两腿的 llvm-cov 入口（workspace 全量 / independent 按清单），缺即点名；自检夹具把
workspace 换成 echo、independent 保留，新增负例。判据——`--self-test` 37/37 绿；真仓
审计 PASS；⑤ 文档门绿。

**2026-11-28 批294 本轮收口（覆盖率入口钉住）**：事实——coverage 两腿的 llvm-cov
入口已入审计（换 echo 即红），补上"非阻断观测面也可以无声消失"的盲区；自检 37/37。
窗口仍未到（runs=0）。判据——commit `3aa62d0b` 三线全绿（gate / integration /
platforms 均 success）；三远端同指 `3aa62d0b`。

**2026-11-28 批294 下轮任务拟定**：① 批295 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批296 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批297 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批298 判据入口扩面
收口裁定——buildroot `image` 已由 `release`（缺产物即红）间接覆盖、`dtb-backport`
仅有编译器探针可锚，**不再扩面**（避免为检查而检查），理由入册；⑤ 批299 三线收口
与再拟下轮。

**2026-11-28 批295 pack-bundle 意外退出归因**：事实——出包链只有 `die` 显式失败路径，
`set -e` 下的裸命令失败只剩退出码；出包是发布载荷来源，卡住时必须能看到失败命令与
产物路径。处置——补 ERR trap：`FAIL 意外退出：<命令>（行 N；产物 <path>）`，与
install-bundle 同款；install-smoke 每次真跑 pack-bundle，成功路径不误触发。判据——
commit `335556fb`：install-smoke / distro-smoke / scripts / conventions 同拍 success。

**2026-11-28 批296 本轮收口与扩面裁定**：本轮两件——覆盖率入口入审计（批293）、
pack-bundle 补意外退出归因（批295）。**扩面裁定**：buildroot 的 `image` 已由 `release`
（缺产物即红）间接覆盖；`dtb-backport` 仅编译器探针可锚、价值低——判据入口扩面**到此
为止**，不再为检查而检查（理由入册，防后续无据扩面）。窗口仍未到（runs=0）。判据——
`335556fb` 的 gate / integration 见上；文档提交 gate success；三远端同指。

**2026-11-28 批296 下轮任务拟定**：① 批297 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批298 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批299 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批300 三线收口与
再拟下轮。

**2026-11-28 批297 审计自检计数与通过面订正**：事实——自检实际 37 条断言，而文件头
仍写"24 条"、`_cases`/`self_test` 写"30 条"、README 写"二十三条负例"；审计 PASS 摘要
也未含新增的四类钉住判据（集成腿/门禁入口/覆盖率/匿名命令）——过期数字与范围会让
后来者误判覆盖面。处置——逐处订正为 37 条，PASS 摘要补"入口与命令钉住"。判据——
自检 37/37；真仓审计 PASS 且摘要含新家族；⑤ 文档门绿；⑯ 行数门（文件 280 行）。

**2026-11-28 批298 本轮收口（覆盖范围表述与数字对齐）**：事实——审计自检 37 条与
PASS 摘要七家族已对齐，后来者读到的覆盖面即真实覆盖面。窗口仍未到（runs=0）。
判据——commit `610360dd`：gate（scripts / conventions）与 integration success；
⑯ 行数门 PASS；三远端同指 `610360dd`。

**2026-11-28 批298 下轮任务拟定**：① 批299 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批300 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批301 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批302 三线收口与
再拟下轮。

**2026-11-28 批299 判据表共用实现（去重）**：事实——gate / coverage / integration
三处"按 job 钉入口"是同一循环的三份拷贝（仅 kind 与提示语不同），改口径要改三处。
处置——抽 `_entry_problems(text, rel, table, kind, note)`，三个函数只留"按文件名分派
+ 表"；报错文案逐字不变。判据——自检 37/37、0 FAIL；真仓审计 PASS；文件 353→338 行；
⑯ 行数门 PASS。

**2026-11-28 批300 本轮收口（判据实现去重）**：事实——三张判据表共用实现后，
判据口径的修改点从三处收敛到一处，行数 353→338，文案未变。窗口仍未到（runs=0）。
判据——commit `886eb8b6`：gate（scripts / conventions）与 integration success；
自检 37/37；三远端同指 `886eb8b6`。

**2026-11-28 批300 下轮任务拟定**：① 批301 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批302 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批303 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批304 三线收口与
再拟下轮。

**2026-11-28 批301 ota-bundle 意外退出归因**：事实——OTA 包是发布载荷来源，脚本只有
`die` 显式失败路径，`set -e` 下的裸命令失败只剩退出码与空日志。处置——补 ERR trap：
`ERROR: 意外退出：<命令>（行 N）`（与 install-bundle / pack-bundle 同款）；gate 的
ota-bundle 腿每拍真跑（负例走 `if !` 不触发 trap）。判据——commit `0c2f412d`：
ota-bundle / scripts / conventions 同拍 success，gate 与 integration success。

**2026-11-28 批302 本轮收口**：本轮交付——ota-bundle 补意外退出归因；三处部署脚本
（install-bundle / pack-bundle / ota-bundle）的失败归因形成同一形态。窗口仍未到
（runs=0）。判据——`0c2f412d` 的 gate / integration 见上；三远端同指 `0c2f412d`。

**2026-11-28 批302 下轮任务拟定**：① 批303 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批304 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批305 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批306 三线收口与
再拟下轮。

**2026-11-28 批303 全量本地门体检 + 索引补登**：事实——本轮对全部非编译门做了一次
逐项体检（⑤ 文档 / ⑬ 仓面卫生 / ⑯ 行数 / ⑫ 度量 8 项 / ⑮ 锁 I/O 120 文件 48 处 /
⑰ 清单对拍 / 工作流审计 / 步骤清单 30 步 / unsafe / 六个库自检），全部 PASS，无需
修复；体检中发现 `audit/` 目录索引漏登 `release_sets.py`（批250 立的审计未进索引表，
后来者按索引找会漏）。处置——索引行补登。判据——体检输出逐项在案；⑤ 文档门绿。

**2026-11-28 批304 本轮收口（体检无红 + 索引补登）**：本轮无代码缺陷需修复：全量
非编译门体检全 PASS，唯一问题是索引漏登 `release_sets.py`，已补。窗口仍未到
（runs=0，11-30 03:00 UTC）。判据——体检逐项输出在案；⑤ 文档门绿。

**2026-11-28 批304 下轮任务拟定**：① 批305 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批306 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批307 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批308 三线收口与
再拟下轮。

**2026-11-28 批305 平台腿意外退出归因**：事实——`cross-artifacts.sh`（三条交叉腿
共用）与 `arm64-deb-check.sh` 都没有 ERR trap：`set -e` 下的裸命令失败只剩退出码，
而 `build-riscv-image.sh` 早已有点名行号的原命令。处置——两脚本补同款
`FAIL 意外退出：<命令>（行 N）`。判据——commit `d9590581`：cross 三腿（英特尔/AMD、
瑞芯微、进迭时空）与 arm64-deb 全部 success，gate 与 integration success——trap 在
真实路径不误触发。

**2026-11-28 批306 本轮收口**：本轮交付——两条平台腿补意外退出归因，四条平台/部署
载体的失败归因趋于同形（install-bundle / pack-bundle / ota-bundle /
cross-artifacts / arm64-deb / riscv-image）。窗口仍未到（runs=0）。判据——`d9590581`
的 cross 三腿 / arm64-deb / gate / integration 见上；三远端同指 `d9590581`。

**2026-11-28 批306 下轮任务拟定**：① 批307 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批308 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批309 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批310 三线收口与
再拟下轮。

**2026-11-28 批307 载体脚本归因约定 + pack-arm64 补齐**：事实——`pack-arm64.sh` 是唯一
没装 ERR trap 的载体脚本（也是唯一不进 CI 的），失败只剩退出码；六处 trap 形态此前
没有成文约定，新脚本容易漏。处置——pack-arm64 补同款 `FAIL 意外退出：<命令>（行 N）`；
`tools/deploy/README.md` 增"载体脚本失败归因约定"一段（set -e 脚本必须装 ERR trap，
可带产物路径上下文）。判据——`bash -n` 通过；gate `scripts` 的全量 `.sh` 语法检查覆盖；
⑤ 文档门绿。

**2026-11-28 批308 本轮收口**：本轮交付——`pack-arm64.sh` 补齐意外退出归因，载体脚本
"ERR trap 点名命令+行号"从六处同形上升为成文约定（新增脚本不会再漏）。窗口仍未到
（runs=0，11-30 03:00 UTC）。判据——commit `deca814e`：gate`scripts`/`conventions`
与 integration success；三远端同指 `deca814e`。

**2026-11-28 批308 下轮任务拟定**：① 批309 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批310 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批311 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批312 三线收口与
再拟下轮。

**2026-11-28 批309 坏包停树前预检（避免无谓停机）**：事实——原流程对坏包先停 rrd、
换树、起探针、再回滚：一次坏包换来整机停机约 4.5s 与一次回滚，而"包内模块结构是否
完整"在停树前就能判定。处置——`install-bundle.sh` 在停树前校验包内每个模块的
`module.toml` 与同名 bin，缺件即拒并点名"未停 rrd"；install-smoke 增坏包预检例
（删 `rr3-sense` bin 重打包，断言 rc≠0、点名缺件、BUNDLE_VERSION 未变、rrd 仍 active）。
判据——commit `2400b6bd`：install-smoke job `140848322649` success，公开注解含
`install-smoke-malformed::缺模块二进制的包在停 rrd 前被拒（rc=1），树与 unit 未变`；
正常换树 2439ms / 回滚 4552ms 无回归；distro-smoke 同拍 success。

**2026-11-28 批310 本轮收口**：本轮交付把"坏包"的第一道闸前移到停树之前——结构
不合格的包不再换来停机与回滚。窗口仍未到（runs=0，11-30 03:00 UTC）。判据——
`2400b6bd` 的 gate / integration 见上；三远端同指 `2400b6bd`。

**2026-11-28 批310 下轮任务拟定**：① 批311 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批312 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批313 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批314 三线收口与
再拟下轮。

**2026-11-28 批311 必需件预检前移 + 双变体坏包用例**：事实——批309 只把模块结构前移；
必需的 bin/契约/profile 仍要等解包后才断言，缺件依旧换来停机 + 回滚。处置——把
`bin/rrd`、`bin/rr3-ctl`、`bin/rr3-ota-ctl`、`contracts/{topics,slots}.toml` 与 unit
`--profile` 所指 profile 文件一并前移到停 rrd 之前校验；install-smoke 坏包例抽
`expect_pre_reject`，跑"缺模块 bin"与"缺契约"两变体（各自断言点名、树未变、unit 未停）。
判据——commit `464e3ccb`：install-smoke job `140853213588` success，公开注解两条
`install-smoke-malformed::{contract,mod-bin}：在停 rrd 前被拒（rc=1），树与 unit 未变`；
正常换树 2419ms、回滚 4491ms 无回归；distro-smoke 同拍 success。

**2026-11-28 批312 本轮收口**：本轮把"坏包"闸门覆盖到全部 A10 必需件与 profile，
结构不合格的包一律不停机拒绝。窗口仍未到（runs=0）。判据——`464e3ccb` 的 gate /
integration 见上；三远端同指 `464e3ccb`。

**2026-11-28 批312 下轮任务拟定**：① 批313 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批314 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批315 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批316 三线收口与
再拟下轮。

**2026-11-28 批313 镜像抖动退避升级（红因归因）**：事实——`464e3ccb` 的 platforms 红在
ESP32 classic 的 `espidf-std`：注解命中 `rsproxy.cn` 返回 504（`cc/1.6.0` 下载），
guarded-run 15s×2 重试用尽；该提交只改 install-bundle/install-smoke，与 ESP 无关——
属外部镜像中断，不是回归。处置——`guarded-run` 对网络镜像失败改为 15/30/60s 三次退避
（总约 105s，镜像恢复通常在一分钟内）；代码/构建错误仍当场红、不重试。判据——
提交后由 esp32-gates 实跑验证（本地不编译）。

**2026-11-28 批314 本轮收口（红因处置闭环）**：事实——`464e3ccb` 的 platforms 红已
归因外部镜像 504（非本轮改动），退避升级后 `ed2bbcf8` 的 esp32-gates classic/S3 双双
success、platforms / gate / integration 三线全绿。本轮另交付坏包预检前移（必需件 +
profile，双变体用例公开可读）。窗口仍未到（runs=0）。判据——`ed2bbcf8` 三线全绿；
三远端同指 `ed2bbcf8`。

**2026-11-28 批314 下轮任务拟定**：① 批315 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别；红按镜像/固件/签名三类定位）；② 批316 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑；
③ 批317 distro 首发后 `--deep` / B.2 实测与 distro 类裁定；④ 批318 三线收口与
再拟下轮。

**2026-11-28 批315 载体脚本 ERR trap 扩齐**：事实——`tools/deploy/README.md` 约定
"凡是 `set -e` 的部署/打包脚本都在顶部装 ERR trap"，实际 14 个载体脚本只有 6 个在位；
`image-smoke` / `stage-image` / `flash-artifact` / `flash-esp32` / `image-esp32` /
`fw-set` / `audio-rec` / `build-onboard` 失败只剩退出码。处置——八处按同句式补装
（上下文分别带 stage / 镜像 / ELF / 固件 / 输出 / 串口 / 板），README 改为"载体全部
在位"，并写明巡检器 `verify-board-config` 不套：非零退出码表达"有漂移"，属判据而非
意外。判据——`bash -n` 全量过；强制失败探针（副本注入 `false`）八处均打印
`FAIL 意外退出：false（行 N；<上下文>）`；提交 `c5f9275a` gate / integration success
（platforms 被下一提交按并发组取消，不作绿证）。

**2026-11-28 批316 回滚后核验旧版恢复**：事实——`install-bundle.sh` 回滚只"尝试"
`systemctl start rrd` 就报"已回滚到上一版"；旧树因部署面变化起不来时，板侧会误以为
服务在跑。处置——回滚后 3s 内探 `is-active`：起则消息带"rrd 已重新 active"，未起则
点名"旧树也未起来，查 journalctl -u rrd"；恢复时延上限不变。判据——`bef53eae`
install-smoke job `140869355592` success，公开注解 `rollback 恢复=4404ms/上限
15000ms；原因：rrd 崩溃重启 1 次`，换树 1799ms / 装机 2321ms 无回归。

**2026-11-28 批317 本轮收口（窗口未到）**：本轮两项交付都属"让已有机制真正生效"：
归因覆盖从 6/14 扩到 13/13 载体，回滚成功声明与旧版实际状态对齐。窗口事实复核——
`release-anon` runs=0（首跑 11-30 03:00 UTC，不预写结论）；最新正式 tag 仍 `v0.0.8`；
`rockrobot3-distro:v0.0.8` 匿名 403（未发）；OTA / buildroot 仅 legacy `0.0.8`
（无 v 前缀，200；下次发布产 v 前缀）。判据——`bef53eae` gate / integration /
platforms 三线 success；三远端同指 `bef53eae`。

**2026-11-28 批317 下轮任务拟定**：① 批318 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别/签名结论；红按镜像/固件/签名三类定位）；② 批319
下次 `v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify`
实跑，distro 首发后把 distro 类纳入；③ 批320 distro 首发后 `--deep` / B.2 实测与
distro 类裁定；④ 批321 三线收口与再拟下轮。

**2026-11-28 批318 健康探针按 running 判据核验**：事实——`install-bundle.sh` 健康
探针的注释承诺"enabled_slots 的每个模块都 running"，实现却只查"模块名出现 + 无
failed"：degraded / 未起齐也会判过，成功声明与门文案不一致。处置——逐模块按
`$3=="running"` 判（awk 与 install-smoke 外部判据同口径），未 running 的模块名并入
回滚原因，板侧读最后一行即可定位是哪个模块；期望集仍取 unit `--profile` 的
`[enabled_slots]`，不引入第二份清单。判据——`bash -n` 过；三行样例（running /
degraded / failed）验证 miss 点名；`19bcc2b0` install-smoke job `140876369479`
success：正例全模块 running、换树 1799ms / 回滚 4173ms / 坏包双变体停树前拒，
无时延回归；gate / integration / platforms 三线 success。

**2026-11-28 批319 本轮收口（窗口未到）**：窗口事实复核——`release-anon` runs=0
（首跑 11-30 03:00 UTC，不预写结论）；最新正式 tag 仍 `v0.0.8`；匿名只读回读：固件
9 层齐、镜像索引三架构齐、每架构 attestation 双谓词（SLSA provenance + SPDX SBOM）
齐；distro 仍未发（匿名 403）、OTA / buildroot 仅 legacy `0.0.8`。判据——`19bcc2b0`
三线 success；三远端同指 `19bcc2b0`。

**2026-11-28 批319 下轮任务拟定**：① 批320 11-30 03:00 UTC `release-anon` 首跑归因
（确认 runs>0 并按注解记录 tag/类别/签名结论；红按镜像/固件/签名三类定位）；② 批321
下次 `v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify`
实跑，distro 首发后把 distro 类纳入；③ 批322 distro 首发后 `--deep` / B.2 实测与
distro 类裁定；④ 批323 三线收口与再拟下轮。

**2026-11-28 批319 健康探针轮询细化**：事实——探针 1s 粒度把就绪时刻向上取整，且
崩溃循环检测同样受 1s 粒度限制。处置——窗口仍 30s（150 拍 × 0.2s），轮询细化到
0.2s；只读 status，零副作用。判据——`de1962e8` gate success（integration 被后续
提交按并发组取消，不作绿证）；`0d50b1fd` install-smoke job `140883664345` success：
启动探针 2270ms、换树 2599ms、回滚 3952ms（上限 15000ms）。本拍未见显著提速（模块
起齐耗时主导），如实取"无回归 + 检测粒度细化"，不冒领提速。

**2026-11-28 批320 rock3 镜像随装 brain**：事实——`profiles/distro.toml` 的
`enabled_slots` 含 rr3-brain，而 aarch64-musl 交叉面因 ring 需目标 C 工具链不含它，
Buildroot 镜像只装五模块；开机必有"manifest 读取失败"拒绝启动，镜像与装配单不一致
（构建工作流的注入步注释同源写"五模块"）。处置——镜像构建用 Buildroot 自带
glibc 工具链（与镜像 glibc 同源，避免宿主 apt gcc 的 GLIBC_2.x 过新）交叉构建
brain 并入 payload；镜像内容断言补 brain 的 manifest 与二进制两件；distro 档头注
纠正 Python 承载残留（brain 自 0.0.8 批N32 起为 Rust）。判据——`0d50b1fd` gate /
integration / platforms success；buildroot image job `140883805524` 的 U-Boot、
工具链、brain 交叉构建三步 success，全量 make 进行中——**镜像内容断言未收，不以
"构建过"代替"内容对"**，下轮首项收口。

**2026-11-28 批321 本轮收口（窗口未到 + 镜像待收）**：窗口事实——`release-anon`
runs=0（首跑 11-30 03:00 UTC）；最新正式 tag 仍 `v0.0.8`；匿名只读回读镜像三架构、
固件九层、每架构 attestation 双谓词齐；distro 未发、OTA/buildroot 仅 legacy
`0.0.8`。判据——`0d50b1fd` 三线 success；三远端同指 `0d50b1fd`。

**2026-11-28 批321 下轮任务拟定**：① 批322 收 buildroot image job `140883805524`：
绿则记 brain 两件断言通过与镜像 digest；红按 br-toolchain/br-brain/br-build/
br-rootfs 注解定位并修复（根因修复，不改档掩盖）；② 批323 11-30 03:00 UTC
`release-anon` 首跑归因（runs>0；按注解记 tag/类别/签名，红按镜像/固件/签名三类
定位）；③ 批324 下次 `v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的
`release-verify` 实跑，distro 首发后把 distro 类纳入；④ 批325 distro 首发后
`--deep` / B.2 实测与 distro 类裁定；⑤ 批326 三线收口与再拟下轮。

**2026-11-28 批322 rock3 镜像回执（批320 收口）**：事实——buildroot run
`46114023878` success；image job `140883805524` 的 U-Boot、Buildroot 工具链、
brain 交叉构建、全量 make、GPT/dtb 与镜像内容只读断言（新增 rr3-brain 的 manifest
与二进制两件）全过。处置——无代码改动，只收证据。判据——同上。诚实边界：内容
断言证明文件在位，不证明板上可运行（动态依赖与启动行为需真板或 QEMU 启动证据，
硬件未采购，不以"构建绿"冒充"运行绿"）。

**2026-11-28 批323 rock3 镜像装配集纳入对拍**：事实——批320 暴露的盲区是"镜像装
什么"没有任何静态判据：装配单启用六件、镜像装配循环只列五件，brain 靠专属步补装，
新增/移除模块时快门抓不到。处置——`release_sets.py` 新增 `profile_modules` 与
`check_buildroot_image`：distro 装配单 `[enabled_slots]` = 发布策略；buildroot.yml
模块循环 = 装配单−brain；brain 专属步必须在位。判据——`--self-test` 13/13（1 正例 /
12 负例，新增镜像漏件/缺 brain 步/装配单漂移三例）；真实对拍 PASS（真值 6 · 策略 6 ·
镜像同集 · rock3=distro · 派生 5）；`e9e9853c` gate / integration / platforms
三线 success。

**2026-11-28 批324 本轮收口**：本轮把 rock3 镜像从"装五模块而装配单启六件"改为
"Buildroot 同源工具链随装 brain"，并把镜像装配集纳入既有对拍。窗口仍未到
（`release-anon` runs=0，首跑 11-30 03:00 UTC）。判据——`e9e9853c` 三线 success；
三远端同指 `e9e9853c`。

**2026-11-28 批324 下轮任务拟定**：① 批325 11-30 03:00 UTC `release-anon` 首跑
归因（runs>0；按注解记 tag/类别/签名，红按镜像/固件/签名三类定位）；② 批326 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑，
distro 首发后把 distro 类纳入；③ 批327 distro 首发后 `--deep` / B.2 实测与 distro
类裁定；④ 批328 镜像内 brain 的运行证据（真板或 QEMU 启动；硬件未到则以已知边界
记账，不冒领）；⑤ 批329 三线收口与再拟下轮。

**2026-11-28 批325 装配冒烟覆盖 brain 三件**：事实——brain 随装后，快速装配冒烟
仍只垫 sense：镜像的 brain 打包面只有小时级 buildroot 镜像作业覆盖，日常推送没有
快信号。处置——冒烟夹具加入仓库真 brain manifest + 假 bin，断言 manifest 归位、
entry 绝对化后 bin 可执行、config 与仓源逐字节一致三件（与 sense 同口径）。判据——
`7d94d4a9` integration 的 buildroot-assembly job success（三件断言全过）；gate /
integration / platforms 三线 success。

**2026-11-28 批326 本轮收口（窗口未到）**：本轮把 rock3 镜像的 brain 随装从"只有
小时级镜像作业验证"补到"每次集成的装配冒烟也覆盖三件"，并在 status 的 Buildroot
行登记六模块对齐与运行证据边界。窗口事实——`release-anon` runs=0（首跑 11-30
03:00 UTC，不预写结论）。判据——`7d94d4a9` 三线 success；三远端同指 `7d94d4a9`。

**2026-11-28 批326 下轮任务拟定**：① 批327 11-30 03:00 UTC `release-anon` 首跑
归因（runs>0；按注解记 tag/类别/签名，红按镜像/固件/签名三类定位）；② 批328 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑，
distro 首发后把 distro 类纳入；③ 批329 镜像内 brain 的运行证据（真板或 QEMU 启动；
硬件未到则以已知边界记账，不冒领）；④ 批330 三线收口与再拟下轮。

**2026-11-28 批327 测试抖动根因修复（红因）**：事实——纯文档提交 `622e46b4` 的 gate
在 `killed_eof_skipped` 红（注解：4 通过 + 1 失败共 6.4 s，`support/client.rs:118`
panic；同一用例前一提交绿），与文档改动无因果关系。根因——`rr3_read_where` 把
SO_RCVTIMEO 的 5 s 读超时当成"读结束"直接终态；CI 负载下 state 广播晚到就随机红。
处置——正向前提改为墙钟预算 20 s 内重试读超时；`Closed`/`Torn*`/其它 IO 仍立即
终态；若超时恰落在半帧中间，后续读因 magic/长度/CRC 失败而显性红，不会把错位读成
"等到了"。判据——`086b40c6` gate / integration / platforms 三线 success（全量测试
含该用例）；`622e46b4` 的 gate 红已归因为测试面抖动、非文档内容问题。

**2026-11-28 批327 下轮任务维持批326清单**（窗口未到）：① 批328 11-30 03:00 UTC
`release-anon` 首跑归因；② 批329 下次 `v*` 发布后把 OTA/buildroot 纳入匿名回读并核
buildroot 的 `release-verify` 实跑，distro 首发后纳入 distro 类；③ 批330 镜像内
brain 的运行证据（真板或 QEMU 启动；硬件未到则如实记边界）；④ 批331 三线收口与
再拟下轮。

**2026-11-28 批328 brain 动态依赖齐套判据**：事实——镜像内容断言只证明 brain 文件
在位，不证明动态链接可满足；brain 由 post-build 注入、不在 Buildroot 包图里，缺
`libgcc_s`/ELF interpreter 只有真板启动才暴露。处置——镜像构建后新增一步：readelf
读 brain 的 interpreter 与 NEEDED，逐个在 `/tmp/br-out/target` 找同名件，缺即红并
点名。判据——`3f83de64` buildroot run `46125760742` success，image job
`140920059315` 的该步 success（interpreter + NEEDED 全齐）；gate / integration /
platforms success。边界——只证明依赖件在镜像根里，不证明 rrd 真起 brain（运行证据
仍归真板/QEMU）。

**2026-11-28 批329-330 夹具并发/缓存红因修复**：事实——`3f83de64` 之后 platforms
`host-macos` 红在 `audio_sense_end_end`：cargo 报 fresh 但
`/tmp/rr3-build/debug/rr3-sense` 不存在（canonicalize 失败）。根因——同一测试二进制
两个用例并发编译同一模块、共享 target-dir；rust-cache 恢复的指纹可能"报 fresh 而
产物不在"。处置——夹具编译进程内串行（`OnceLock<Mutex>`），产物缺失时 `clean -p`
+ 重编一次并保留首次证据；首批改动触发 clippy `map_unwrap_or` 红（`4847b0d2`），
改为 `unwrap_or_else` + `assert`；gate 归因签名补 `error(\[|:)`，clippy/rustc 的
lint 行今后进公开注解（`b86c0462`）。判据——`b86c0462` gate 仍红在另一条：
`killed_eof_skipped` 20s 等不到 unregistered。

**2026-11-28 批331 abort 夹具显式 shutdown（红因根治）**：事实——`622e46b4` 与
`b86c0462` 两次 gate 都红在 `killed_eof_skipped`；加 20s 重试后第二次在 21.79s 仍红，
说明不是"晚到"而是"根本没来"。根因——abort 支只 drain 不 close：drain 与进程死亡
之间再过一帧广播，内核把关闭改成 RST → rrd 读 ECONNRESET → 广播 `failed` 而非
`unregistered`；测试要排掉的混淆项从未被确定性排除。处置——sim_peer 的 abort 支在
abort 前显式 `shutdown(Both)`，把连接状态定格为 FIN；仍不 unregister、不跑析构，
保留"被暴力终止"语义。判据——`b9eae3ae` gate / integration / platforms（含 macOS
夹具编译）三线 success；`killed_eof_skipped` 在 gate 两个测试入口与 macOS 腿均过。

**2026-11-28 批332 本轮收口**：本轮把 rock3 镜像的 brain 从"文件在位"推进到
"动态依赖齐套"，并连续处理两条测试面红（clippy、abort 混淆项），gate 归因面同时
补强。窗口仍未到（`release-anon` runs=0，首跑 11-30 03:00 UTC）。判据——
`b9eae3ae` 三线 success；三远端同指 `b9eae3ae`。

**2026-11-28 批332 下轮任务拟定**：① 批333 11-30 03:00 UTC `release-anon` 首跑
归因（runs>0；按注解记 tag/类别/签名，红按镜像/固件/签名三类定位）；② 批334 下次
`v*` 发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑，
distro 首发后把 distro 类纳入；③ 批335 镜像内 brain 的运行证据（真板或 QEMU 启动；
硬件未到则如实记边界）；④ 批336 三线收口与再拟下轮。

**2026-11-28 批333 brain 加载自证（qemu）**：事实——批328 的 NEEDED/interpreter
齐套只证明"文件在"，不证明 ELF 能被加载器接上；brain 无 CLI 参数面，起手先打版本行
再连总线。处置——镜像构建后在既有验证段加一步：qemu-aarch64 以目标根为 sysroot
直接执行 brain（`RR3_BUS_PATH` 指向不存在路径），判据 = 出现 `rr3-brain 0.0.1`
版本行且无 `error while loading shared libraries` / `cannot open shared object`；
无总线按预期非 0 退出，不以退出码冒充功能运行。判据——`9caa24e4` gate /
integration / platforms success；buildroot image job 的该步待收（run `46130592655`，
全量 make 进行中）。

**2026-11-28 批334 本轮收口（镜像待收）**：本轮把 brain 的镜像证据从"依赖件齐套"
推进到"加载自证"。窗口仍未到（`release-anon` runs=0，首跑 11-30 03:00 UTC）。判据——
`9caa24e4` 三线 success；三远端同指 `9caa24e4`。

**2026-11-28 批334 下轮任务拟定**：① 批335 收 buildroot image job（brain 加载自证
步骤；红按 br-brain-run 定位）；② 批336 11-30 03:00 UTC `release-anon` 首跑归因
（runs>0；按注解记 tag/类别/签名，红按镜像/固件/签名三类定位）；③ 批337 下次 `v*`
发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑，distro
首发后把 distro 类纳入；④ 批338 镜像内 brain 的系统级运行证据（真板/QEMU 整机；
硬件未到则如实记边界）；⑤ 批339 三线收口与再拟下轮。

**2026-11-28 批335 release-anon 首跑预演（同脚本、同类别）**：事实——11-30 首跑
会执行 `release-verify.sh --tag auto --with-signature --classes image,firmware`；
此前只有零散 curl 预检，脚本本体未在真实 registry 上跑过。处置——本机装 oras 1.2.0
（只读工具，非构建），以仓内脚本原命令（去 `--with-signature`，本机无 cosign）跑
两遍。判据——`--tag auto → v0.0.8`；镜像三架构齐、每架构 SLSA provenance + SPDX
SBOM 双谓词齐；固件九件齐；`结论：发布面齐套`，rc=0；digest 镜像
`sha256:07d189…9b3e`、固件 `sha256:39cc0f…0315`。边界——签名半边与首跑结论仍以
11-30 任务为权威，不预写。

**2026-11-28 批335 下轮任务拟定**：① 批336 收 buildroot image job（brain 加载自证
步骤；红按 br-brain-run 定位）；② 批337 11-30 03:00 UTC `release-anon` 首跑归因
（runs>0；按注解记 tag/类别/签名，红按镜像/固件/签名三类定位）；③ 批338 下次 `v*`
发布后把 OTA/buildroot 纳入匿名回读并核 buildroot 的 `release-verify` 实跑，distro
首发后把 distro 类纳入；④ 批339 镜像内 brain 的系统级运行证据（真板/QEMU 整机；
硬件未到则如实记边界）；⑤ 批340 三线收口与再拟下轮。

**2026-11-28 批336 brain 随镜像与加载自证（构建回执）**：事实——批320 起 brain 用
Buildroot 自带 glibc 工具链交叉构建并入 payload（与镜像 glibc 同源），批328/333 在
镜像 job 里补了两道判据：动态依赖齐套（readelf 读 interpreter 与 NEEDED，逐个在
`/tmp/br-out/target` 找到同名件）与加载自证（qemu-aarch64 以目标根为 sysroot 直接
执行，判据=打印版本行且无 loader 错误；无总线按预期非 0 退出，不以退出码冒充功能）。
处置——推送 `9caa24e4` 触发全量构建。判据——image job `140934886114`（run
`46130592655`）全部步骤 success：全量 make、步骤 15「brain 动态依赖在目标根齐套」、
步骤 16「brain 加载自证」、镜像产物断言（GPT 六分区 + 双 dtb）、镜像内容只读断言
（e2fsck/标签/rrd/S99rrd/fstab/双 dtb/atf-bl31）与摘要上传。边界——这是**构建与
加载器面**的证据，不是板上运行证据；串口出 shell、maskrom 实烧与系统级启动仍待硬件。
**下轮任务拟定**：① 批337 出厂镜像去 Python3（目标侧零 python 消费者，脑已 Rust），
以 `[buildroot-full]` 全量构建回执收口；② 批338 11-30 03:00 UTC `release-anon` 首跑
归因（runs>0；按注解记 tag/类别/签名）；③ 批339 下次 `v*` 发布后把 OTA/buildroot/
distro 纳入匿名回读；④ 批340 出厂镜像网络与时间面的选型裁定（NetworkManager 或换
链路后端、时间同步机制），并给目标运行证据的判据；⑤ 批341 三线收口与再拟下轮。

**2026-11-28 批337 出厂镜像去 Python3（构建回执）**：事实——brain 自 ADR-0017 起是
Rust ELF，镜像装配的六个模块全是 ELF；defconfig 的 `BR2_PACKAGE_PYTHON3` 是 Python
brain 时代遗留，目标侧零 python 消费者（post-build/post-image 与镜像内容断言都在宿主
执行；`git grep python3 -- base/os/buildroot tools/ci` 只剩宿主依赖行）。处置——移除
该包，defconfig 内留"复核判据"注释（新增目标侧 python 需求必须重跑该 grep 并评审，
不接受按印象加回）；512M 容量注释同批去掉 Python3 字样。判据——`26326682`
（`[buildroot-full]`）run `46135736231`：dtb-backport 与 image 两 job success，
全量 make、brain 依赖齐套、brain 加载自证、GPT 六分区、镜像内容断言、摘要上传全过；
同头 gate / integration / platforms 三线 success。边界——"镜像内不再选入 python3"是
**Kconfig 面事实**（保持集只有 ALSA / busybox / uboot / kernel / rkbin，无包以依赖
形式 select 它）；镜像体积变化未取数（artifact 下载需凭据），不写未测的数字。
**下轮任务拟定**：① 批338 11-30 03:00 UTC `release-anon` 首跑归因；② 批339 下次
`v*` 发布后把 OTA/buildroot/distro 纳入匿名回读；③ 批340 出厂镜像网络与时间面的
选型裁定与目标运行证据判据；④ 批341 Python 语言支持面清理（schema `lang` 收为
rust、删 rr3-pack python shim，先出契约级评审）；⑤ 批342 三线收口与再拟下轮。
