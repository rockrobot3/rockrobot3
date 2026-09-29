# rr3-interact

交互模块：表情屏、TTS 播放、Web 面板。骨架由 `rr3-mkmod` 生成（conventions §3）。

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
    ├── panel/server.rs   ← HTTP 服务（默认只绑回环，随 M5）
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

## 实现进度

v0.0.1 交付结构 + manifest + 静态页占位。屏驱随 M3，HTTP 监听面随 M5。本地自检：

```bash
cargo run --manifest-path modules/rr3-interact/Cargo.toml --bin rr3-interact
```
