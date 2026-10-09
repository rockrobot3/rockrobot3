# rr3-interact 模块设计

> 证据：`modules/rr3-interact/module.toml`、`atoms/{face,player}/`、`src/composite.rs`、
> `src/panel/server.rs`、`config.toml`、`rr-hal` 的 `rr3_hal_display` / `rr3_hal_audio_sink`。

## 1 职责

把语义结果变成**可看见、可听见的表现**（表情帧 + 播放），并提供**只读运维面板**。它不是编排者：
不决定说什么、不决定动不动（那是 brain / act）。格账：`ui.face` + `audio.playback`。

## 2 事实面

| 成分 | 面 | 现状 |
|---|---|---|
| `face` 件 | `ui.face` | 七态白名单状态机 + 8×8 帧表；出帧经 `Rr3Display`（`Rr3FaceFrame = [u8; 8]` 类型事实在 rr-hal）；`publishes = []`——face 无事件发布，只应答 `face/expression` 点名 |
| `player` 件 | `audio.playback` | 消费 `tts/text` 点名的 PCM，talk 期间挂起表情切换；`publishes = []` |
| `panel`（组合件成分） | 状态面 | 手写 HTTP **十二端点**（状态 / 硬件 / 系统九 + 配网三：`scan`/`connect`/`forget` 全部 BusCall 透传，裁决在 owner 模块）；静态页 `include_str!` 内嵌；监听面 `RR3_PANEL_ADDR` 优先、缺省 `127.0.0.1:8080`；`rr3.ctl.reboot` 走双重门 |
| 主循环 | 接入 + 运行 | 订阅 `rr3-brain/intent/result` 与 `rr3-connect/net/link`；beep/face 经 mpsc 由主循环串行代执行（sink/face 所有权不出主循环） |

申报面 `publishes = ["rr3-interact/asr/text", "rr3-interact/dialog/turn"]`——**全模块零 `rr3_publish`
调用**，两条主题在生产侧无生产者（发布只存在于 `tools/sim`）。参数面六项
（`prebuffer_ms = 500`、`volume_pct = 70`、`default_expression = "idle"`、`log_level`、`bind`、`port`）
在 crate 里**没有读者**（`Cargo.toml` 无 toml 依赖，也无配置解析）；面板地址真值是常量 + env。
健康：心跳 1000 / 看门狗 5000；`start_priority = 60`，`mem_limit_mb = 96`。

## 3 件级切分与闭包（设计目标）

- `face` 的**帧表是策略**（哪一态配哪些帧是人格/品牌决定），**帧机是机制**（状态转移、点名单元、
  与 talk 的挂起关系）。设计切法：件留帧机 + 白名单契约，帧表与默认表情进 ops/形态配置。
- `player` 的预缓冲与音量是 ops 槽的天然样本（换形态要改手感 → 属"形态"类参数，落 config；
  换设备要改 sink 名 → 属"板"类参数，落 `[hardware].alsa_device`，两者不得混在一处）。
- 显示真后端未实装（MAX7219 SPI 属真件档，现落占位显式回错），所以本单元的 `ui.face` 在板上是
  "句柄在位、能力缺"的中间态——**设计要一个明确记法**：格在位 + 后端未实装 ≠ 格在位。

## 4 职责四面

| 面 | 落点 |
|---|---|
| 接入 | `face/expression`、`tts/text` 两点名；`intent/result`、`net/link` 两订阅 |
| 运行 | 表情推进（约 250 ms 墙钟节拍推帧，不随总线流量漂移）+ 播放 |
| 状态 | **panel 十二端点是本单元的状态面主体**（面板不当发布者，ADR-0006） |
| 守护 | 开机对 display / audio sink 探活，`Unsupported` 即 `exit(1)`；业务性失败折 error 应答不断链 |

## 5 时序设计

- 表情推进名义 250 ms；播放侧受 `prebuffer_ms`（500）约束——这两个数**今天只存在于未被读的配置里**，
  真实行为用的是代码默认。设计定：ops 槽落地后，二者必须同源（一处声明、一处读取、boot 期回显生效值）。
- 待给而未给的量：TTS 首帧可听延迟上界、talk 挂起表情切换的等待窗、面板并发上限（有 `RR3_PANEL_MAX_CONNS`
  与在途计数，但连接线程异常退出时的计数回收路径未证）。

## 6 降级与缺格语义

| 情形 | 设计行为 |
|---|---|
| `ui.face` 缺（无屏形态） | 本单元仍可存在（只播不显），但帧机不得被调用——装配段按 `present` 决定，件不得自己猜 |
| `audio.playback` 缺（无声形态） | 同上，且 `tts/text` 点名应回 `Unsupported`（显式），不可静默吞文本 |
| 两格都缺 | 本单元不该被启用（等于只剩面板一个运维面，那属 rrd/rr3-ctl 的职责面） |
| 申报的 `asr/text` / `dialog/turn` 无生产者 | 悬空边；按 ADR-0019 修订 2 ② 走 retired 或补实现，二选一，不许长期挂着 |

## 7 红线

两 RPC 名与应答形状、七态白名单字符串、帧的位序（`bit7` 为行最左）、面板十二端点与透传语义
（面板零发布权）、`RR3_PANEL_ADDR` 缺省只绑回环（安全边界）、SURFACE 自述。

## 8 待证与敞口

1. 真屏与真回放从未发生（显示真后端未实装、USB 麦/喇叭未采购）。
2. 配置死文件问题（六参数无读者）修法属代码，本册定口径：**要么被读、要么删除**，不允许第三种状态。
3. 面板在 rtos / bare 档是否存在，索引 §3 未定；设计倾向：**面板是 host 档专属**（它依赖文件内嵌页 +
   HTTP 栈），rtos 档用桥侧的 `rr3-ctl` 看状态。此条待裁。
