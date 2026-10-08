# rr3-sense

感知模块：麦克风阵列采集、环境传感器、IMU。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-sense/
├── module.toml          ← manifest；发布面只能引用 contracts/topics.toml 已登记主题
├── composite.toml       ← 组合件装配清单（⊗ 链：capture ⊗ vad ⊗ env ⊗ imu）
├── config.toml          ← 运行参数（VAD 阈值等），改参数不碰源码
├── Cargo.toml           ← 独立 workspace；path 依赖 rr-ipc / rr-bus / rr-hal
├── atoms/               ← 原子件（每格一件，五件套：atom.toml + 入口 + mech + assemble.toml + private.rs）
│   ├── capture/         ← audio.capture：采集/下混/DoA 判向 + pcm 事件体
│   ├── vad/             ← audio.vad：边沿状态机 + vad_start 事件体（纯 DSP，golden PCM 向量本机全量测试）
│   ├── env/             ← sensor.env：占位件（真件后端随采购批填实）
│   └── imu/             ← sensor.imu：占位件（融合/闭环归 act 反射环）
└── src/
    ├── main.rs          ← 入口：只装配与启动（mod composite + main）
    └── composite.rs     ← 组合件：config 加载 + 事务面 dump 应答 + 采集主循环
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| publish | `rr3-sense/audio/pcm` | stream |
| publish | `rr3-sense/audio/vad_start` | event |
| publish | `rr3-sense/env/temperature` | event |
| publish | `rr3-sense/imu/state` | stream |
| rpc | `rr3-sense/capture/dump` | request |

PCM 与 IMU 属实时控制面摘要；shm 零拷贝传输的裁定见 ADR-0007 §裁定 3。

## 本地自检

```bash
cargo test --manifest-path modules/rr3-sense/Cargo.toml
```

运行要挂总线：`RR3_MANIFEST` 指向 module.toml（entry 会被客户端绝对化为当前 exe）、
`RR3_BUS_PATH` 指向 rrd 的 UDS；音频后端由 `RR3_HAL_BACKEND` 选择（sim / linux）。
开发调试最省事的方式是让 rrd 以 `--manifest` boot spawn 本模块，两个环境变量都由它注入。
capture/dump 点名的应答形状、pcm/vad 事件体的字节级事实源在源码
（`atoms/vad/mech.rs` 的 `rr3_vad_event_body` 与 schema
[`contracts/events/sense-audio-vad_start.schema.json`](../../contracts/events/sense-audio-vad_start.schema.json)）。

**实况不在本文件登记**：ALSA 后端能否真机上板、env/imu 两路是否已接 HAL，唯一落点
[`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
