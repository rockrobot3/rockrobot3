# rr3-interact

交互模块：表情屏、TTS 播放、Web 面板。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-interact/
├── module.toml
├── config.toml
├── Cargo.toml
└── src/
    ├── main.rs           ← 入口：打印自述与发布面，未接总线
    ├── face.rs           ← 表情渲染（idle/blink/talk/think/happy）
    ├── player.rs         ← 出话播放（预缓冲/抗抖动参数继承 legacy 调优）
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
cargo run --manifest-path modules/rr3-interact/Cargo.toml --bin rr3-interact
```

本目录交出的是结构 + manifest + 静态页占位（面板内容由总线数据填充）。**屏驱与 HTTP 监听面
今天有没有实现**属实况，唯一落点 [`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
