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
    ├── main.rs          ← 真入口：RR3_MANIFEST → rr3_connect（注册/心跳）→ 收帧循环应答两个点名
    ├── face.rs          ← 最小表情机（五态白名单；talk 自动挂起/恢复，显式指令可打断）
    ├── player.rs        ← 占位 TTS：440Hz 正弦 burst，60ms/字符封顶 10s（真 TTS 随 M4 云面）
    ├── panel/mod.rs      ← 面板总览
    ├── panel/server.rs   ← HTTP 服务（默认只绑回环）
    └── panel/www/        ← 静态页
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| rpc | `rr3-interact/face/expression` | request |
| rpc | `rr3-interact/tts/text` | request |
| publish | `rr3-interact/asr/text` | event |
| publish | `rr3-interact/dialog/turn` | event |
| subscribe | `rr3-brain/intent/result` | event |

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

本目录交出的是 M3 实装 + manifest + 静态页占位（面板内容由总线数据填充）。**屏驱、
HTTP 监听面与真 TTS 今天有没有实现**属实况，唯一落点
[`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
