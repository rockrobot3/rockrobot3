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
