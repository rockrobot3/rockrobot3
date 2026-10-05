# rr3-interact

交互模块：表情屏、TTS 播放、Web 面板。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-interact/
├── module.toml
├── config.toml
├── Cargo.toml           ← 独立 workspace；path 依赖 rr-ipc / rr-bus / rr-hal
└── src/
    ├── main.rs          ← 真入口：RR3_MANIFEST → rr3_connect（注册/心跳）→ 收帧循环应答两个点名 + 面板动作代执行
    ├── face.rs          ← 最小表情机（五态白名单；talk 自动挂起/恢复，显式指令可打断）
    ├── player.rs        ← 占位 TTS：440Hz 正弦 burst，60ms/字符封顶 10s（真 TTS 随 M4 云面）
    ├── panel/mod.rs      ← 面板总览（静态页 include_str! 内嵌）
    ├── panel/server.rs   ← HTTP 服务（std 手写 HTTP/1.1 子集，零新依赖）
    └── panel/www/        ← 静态页（单文件 index.html，四页签）
```

## Web 面板（0.0.4 批C）

std 手写 HTTP/1.1 子集（GET/POST + Content-Length，**零新 crates.io 依赖**，Q3 裁定）：
不做 keep-alive、不做 chunked、不做 TLS。**监听面缺省只绑回环 `127.0.0.1:8080`**，
env `RR3_PANEL_ADDR` 可改（板上 `0.0.0.0` 随装配单注入）——本面板是**局域网可信面**
的调试工具，不承诺公网暴露，无鉴权是有意为之的边界声明。

端点九个（legacy 面板四页签语义逐项承接；WiFi 配网页随批D 挂上）：

| 方法 | 路径 | 数据面 | 承接 legacy |
|---|---|---|---|
| GET | `/` | 内嵌静态页（`include_str!`，无运行时文件依赖） | `/` |
| GET | `/api/status` | `rr3.ctl.health` 短连 + `rr3-connect/net/link` 订阅缓存 | `/api/status` |
| GET | `/api/data` | `rr3.ctl.counters` 短连 | `/api/data` |
| GET | `/api/logs` | `rr3.ctl.logs` 短连 | `/api/logs` |
| POST | `/api/relay` | `rr3.call` → `rr3-act/relay/set`（act 既有 exposes，零新增） | `/api/relay` |
| POST | `/api/beep` | 进程内动作通道（主循环代执行） | `/api/beep` |
| POST | `/api/face` | 进程内动作通道（主循环代执行） | `/api/face` |
| POST | `/api/selftest` | 并列三调（beep+relay+face），各答各展示，无依赖链**不算编排** | `/api/selftest` |
| POST | `/api/reboot` | `rr3.ctl.reboot`（rrd 双重门缺省恒拒，面板如实转达拒因） | `/api/reboot` |

架构边界（`server.rs` 的 [`Rr3PanelDispatch`] 类型面钉死）：

- **面板不当发布者**——`Rr3PanelDispatch` 无 Publish 变体，硬件控制只产
  `BusCall`（rr3.call 经 rrd 归属门），越权发布在路由器里无从构造（ADR-0006 裁定 3）；
- **ctl 查询走短连**——进程内另开裸 UDS 一问一答即断（src=0），与外部运维探针
  同权同拒（ADR-0010）；
- **beep/face 不出进程**——sink/face 归主循环所有，HTTP 线程经 mpsc 动作通道递
  请求，主循环每圈 `try_recv` 串行代执行，与 TTS 点名天然互斥（无锁）；
- **selftest 不是编排**——三个动作并列发起、单项失败不连锁、无状态机；
  编排权归 brain（ADR-0007）。

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| rpc | `rr3-interact/face/expression` | request |
| rpc | `rr3-interact/tts/text` | request |
| publish | `rr3-interact/asr/text` | event |
| publish | `rr3-interact/dialog/turn` | event |
| subscribe | `rr3-brain/intent/result` | event |
| subscribe | `rr3-connect/net/link` | event（面板状态页链路卡片数据源，0.0.4 批C 订阅；发布方 rr3-connect 随批D 上线） |

面板**不是第二条控制面**：它经 rr-bus 收发、与模块同权，不得绕过 rrd 的注册与
前缀归属检查（ADR-0006 §裁定 3）。

## 本地自检

```bash
cargo test --manifest-path modules/rr3-interact/Cargo.toml
```

运行要挂总线：`RR3_MANIFEST` 指向 module.toml、`RR3_BUS_PATH` 指向 rrd 的 UDS、
音频后端 `RR3_HAL_BACKEND`（sim / linux）；让 rrd boot spawn 本模块则三者都由它注入。
两个 rpc 点名的应答形状（`{"played_ms","chars"}` / `{"expression","previous"}`）与
业务性失败折 `result.error`（而非断链）的口径，事实源在 `main.rs` /
[`docs/reference/rrd-rpc.md`](../../docs/reference/rrd-rpc.md) §8。

本目录交出的是 M3 实装 + manifest + Web 面板（0.0.4 批C：九端点 HTTP 服务 +
单文件静态页，监听面缺省回环）。**屏驱与真 TTS 今天有没有实现**属实况，唯一落点
[`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
