# ADR-0016：brain 迁移路线——Python 转 Rust 渐进式首裁定

- **状态**：已接受（设计裁定；实施批次归属原定 0.0.6 专项，经 2026-10-05 修订顺延
  为 0.0.8——见文末修订记录，本裁定不等于实施解锁）
- **日期**：2026-10-05
- **关联**：[ADR-0014](ADR-0014-python模块接入与M4对话闭环.md)（被引用并**收缩适用范围**，不废止）·
  [ADR-0013](ADR-0013-OS双轨-发行版快路与Buildroot保底.md) 裁定 4（OCI 依赖冻结是消解而非根除）·
  [ADR-0006](ADR-0006-采用AiryIPC线格式与rr-ipc分层.md)（rr-bus/rr-ipc 是迁移首件的复用件）·
  conventions §3（主语言 Rust 规约，2026-10-04 立规——本裁定的上位依据）·
  0.0.5 方案 §2 Q2 裁定记录

## 背景

`rr3-brain` 是仓里唯一的 Python 模块（M4 实装，见
[ADR-0014](ADR-0014-python模块接入与M4对话闭环.md)）。conventions §3 于
2026-10-04 立规"仓内一切新实现一律 Rust；存量非 Rust 面按功能语义不变渐进迁移"，
brain 与 CI bash 胶水是仅存的两块非 Rust 面。立规晚于 ADR-0013/0014 接受日，两者
均未裁定 brain 的迁移路线——本 ADR 补这一块，且只落**设计裁定**：迁不迁不再讨论
（上位规约已裁），裁的是"怎么迁、切几刀、部署形态变成什么、哪一批动手"。

直接动因有二：

1. **ADR-0014 裁定 1 的"逐语义镜像"纪律本是临时垫层**：`bus.py` 手写 128B 协议栈
   对齐 `rr-bus` 客户端，防线是 golden hex 用例 + e2e 真总线回路——两端谁漂移谁红。
   这条防线成立的前提是"Rust 侧已定型、Python 侧跟上"，它是漂移**检测器**不是
   漂移**消除器**；主语言裁定之后，镜像纪律的存在理由只剩"迁移未启动"。
2. **Python 运行时是 payload 的整块负担**：deb / Buildroot rootfs 要背 python3
   解释器；`tomllib` 钉死 ≥3.11 门槛；`rr3-sim` 要为 brain 单独维护
   `which_python3` 能力验证面；CI 要为 brain 单列 `brain-unittest` job（stdlib
   unittest 不在任何 Rust workspace 的覆盖内）。ADR-0013 裁定 4 用 OCI 依赖冻结
   消解"Python 环境漂移"，是**消解**（问题进镜像）不是**根除**（问题不存在）——
   迁移是更彻底的解：payload 去运行时。

## 决策

### 裁定 1：迁移按"功能语义不变"渐进切件，首件 = `bus.py` → 复用 `rr-bus`

切分单位是**语义件**不是文件行数，每件独立过七步门禁（生成器产出、契约校验、
测试一样不少），迁完一件删一件 Python 对应物，仓库任何时刻不存在同一语义的
双语言真值：

| 序 | 件 | 迁移落点 | 语义对照（不变量） |
|---|---|---|---|
| 1 | `rr3_brain/bus.py` | `rr-bus` crate 直接路径依赖（对位 Rust 模块的既有用法，不再镜像） | 注册带 manifest 全文 + entry 绝对化、心跳喂狗失败 `exit(1)`、publish / `rr3.call` / reply / 死亡探针逐语义对照 |
| 2 | `cloud/{asr,llm,tts}.py` | brain crate 内 `cloud/` 模块（stdlib `urllib` → Rust HTTP 客户端，端点/请求体/应答解析逐项照 ADR-0014 裁定 2 表） | 三家线上契约逐项对照；密钥只走 `SILICONFLOW_API_KEY` 环境变量 |
| 3 | 对话状态机 + 意图规则 | brain crate 主体（`rr3.call rr3-interact/tts/text` 事务面、utterance 判停两判据、历史窗口、人设参数原值） | ADR-0014 裁定 3 的回路归属与失败语义逐条对照 |

- **首件选 bus.py 的理由**：它是镜像纪律的载体，迁掉它 ADR-0014 裁定 1 自然退役；
  且 rr-bus 是现成件——迁移首件**零协议代码**，风险最小的刀口。
- 否决 **一次重写整个 brain**：单体迁移无法逐件验证功能语义不变，回滚粒度也是
  整个模块——违背"渐进"本意。
- 否决 **保留 bus.py 的 Python 测试作为迁移后回归**：测试跟实现走，Python 测试
  随对应件删除；回归面由迁移件的 Rust 测试 + e2e（四真进程编排不变）接管。

### 裁定 2：功能语义不变量——迁移的验收锚点（缺一不收）

1. **对话闭环不变形**：sense → brain → interact 的 M4 回路、`RR3_BRAIN_CLOUD=mock`
   注入点及其确定性行为（ASR 固定文本 / LLM 回显 / TTS 正弦 PCM）逐语义保留；
   mock 是测试注入点不是产品路径的定性不变。
2. **失败语义同契约**：业务失败折错误应答/日志弃轮进程不死；传输失败 `exit(1)`
   交 supervisor——与 rr3-sense/rr3-interact 同一口径。
3. **门禁与契约校验不减**：七步门禁、manifest 契约校验、topics/slots 查册、
   装配单驱动 boot 对 brain 一视同仁；brain 单测从 stdlib unittest 换成 cargo
   test，条数随迁移批次登记 conventions §2（brain-unittest CI job 随最后一件
   迁移同步退役）。
4. **`config.toml` 语义不变形**：`[rr3-brain.cloud]` 与 `max_utterance_ms` 等键名、
   缺省值、mock/real 判据逐键对照。

### 裁定 3：部署形态目标——payload 去运行时

迁移全部完成后（不以件为单位、以仓库终态为准）：

- deb / Buildroot rootfs **不再包含 python3 运行时**；Buildroot defconfig 里
  Python 相关包的去向随实施批核对（快路轨 deb 由打包链自然减重）。
- `module.toml` 的 `lang` 字段改 `rust`；"Python 语法检查属人工检查点"随
  conventions §3 的预告收敛为 cargo 检查。
- `tomllib` 带来的 **≥3.11 运行时门槛自然消失**（Rust 侧 toml 解析已有）。
- `tools/sim` 的 `which_python3` 能力验证面随最后一件迁移退役；ADR-0014 裁定 4
  的 brain shim 脚本（`exec python3 -m rr3_brain` + `PYTHONPATH`）同步退役，
  e2e 装配单里 brain 与 sense/interact 同构（entry 指真二进制）。
- **ADR-0013 裁定 4 维持不动**：OCI 分发格式与"容器不是编排层"不受本裁定影响；
  变化的只是 OCI 镜像内容物（Python 依赖树 → 静态 Rust 二进制），镜像更小、
  更可复现。CI 双矩阵执法口径不变。

### 裁定 4：实施批次归属 0.0.6 专项，0.0.5 只落本设计裁定

- 实施哪怕首件 bus.py 都牵动打包链（deb 内容物）、装配单（shim → 真二进制）、
  CI（brain-unittest job）三面联动，属独立版本体量——**不塞 0.0.5**。
- 0.0.6 立专项方案（WBS 按裁定 1 的三件切分排批），动工前过评审裁定；本裁定
  是该方案的输入，不是它的代替。
- CI bash 胶水的迁移**不在本专项内**（conventions §3"可最后迁"口径维持，
  届时另行裁定）。

## 明确不做（本裁定边界）

1. **0.0.5 内的任何实施动作**（裁定 4）。
2. **增补 ADR-0014**：决策不可变、新决策新编号——本 ADR 引用并收缩 0014 的
   适用范围（裁定 1 的镜像纪律随首件迁移退役；裁定 2/3/4 的云 API 形状、
   对话回路归属、e2e 编排语义继续有效直到被迁移件按裁定 2 逐语义对照通过）。
3. **CI bash 胶水迁移**（裁定 4 第 3 点）。
4. **对 brain 的功能增强**：迁移批冻结新特性，语义不变量是验收锚点，边迁边改
   无法对账。

## 后果

- 正面：全仓语言栈单一化（除 CI 胶水）；镜像纪律这一漂移源消除（bus.py 复用
  rr-bus 后，协议真值只剩 Rust 一份）；payload 去 python3 运行时，deb/rootfs
  减重、≥3.11 门槛消失；CI 去掉 brain 专属 job，门禁面归一。
- 负面：一次性专项成本（三件迁移 + 打包链/装配单/CI 三面联动）；迁移期间
  brain 处于"半 Python 半 Rust"过渡态——切分纪律（迁移件与 Python 对应物
  同批一进一退）是防止双真值并存的关键约束。
- 修订记录（2026-10-05）：**裁定 4 的"实施批次归属 0.0.6 专项"由 0.0.6 版本方案
  Q1 裁定修订为"归属 0.0.8"**——0.0.6 改立树莓派验证专项（跨平台快速部署可行性，
  ADR-0013 裁定 1/2 的真板兑现）、0.0.7 为 ESP32 边界预留。本 ADR 裁定 1/2/3
  （切分表、不变量、部署形态目标）一字不动；实施动工前仍须按裁定 4 立专项方案
  过评审。
- 校验项：实施批次每件照正常批次纪律收尾（七步门禁 + conventions §2 登记 +
  双远端核对）；brain 单测条数换轨时登记；`which_python3` 与 brain-unittest
  的退役动作随最后一件进收口清单，不提前删除。
