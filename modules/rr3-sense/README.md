# rr3-sense

感知模块：麦克风阵列采集、环境传感器、IMU。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-sense/
├── module.toml          ← manifest；发布面只能引用 contracts/topics.toml 已登记主题
├── config.toml          ← 运行参数（VAD 阈值等），改参数不碰源码
├── Cargo.toml           ← 独立 workspace；path 依赖 rr-ipc / rr-bus / rr-hal
└── src/
    ├── main.rs          ← 真入口：RR3_MANIFEST → rr3_connect（注册/心跳）→ 采集循环
    ├── audio/mod.rs     ← 事件体构造 + config 加载 + surface
    ├── audio/capture.rs ← 采集循环：自 pacing 到 frame_ms 节拍，发布 pcm(stream) + vad(event)
    ├── audio/vad.rs     ← 本地 VAD 边沿检测（纯 DSP，golden PCM 向量本机全量测试）
    ├── env.rs           ← 环境温度
    └── imu.rs           ← IMU 姿态摘要
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
（`audio/mod.rs` 的 `rr3_vad_event_body` 与 schema
[`contracts/events/sense-audio-vad_start.schema.json`](../../contracts/events/sense-audio-vad_start.schema.json)）。

**实况不在本文件登记**：ALSA 后端能否真机上板、env/imu 两路是否已接 HAL，唯一落点
[`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
