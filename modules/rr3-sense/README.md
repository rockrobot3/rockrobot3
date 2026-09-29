# rr3-sense

感知模块：麦克风阵列采集、环境传感器、IMU。骨架由 `rr3-mkmod` 生成（conventions §3）。

## 目录结构

```
modules/rr3-sense/
├── module.toml          ← manifest；发布面只能引用 contracts/topics.toml 已登记主题
├── config.toml          ← 运行参数（VAD 阈值等），改参数不碰源码
├── Cargo.toml           ← 独立 workspace；M2 起 path 依赖 rr-ipc / rr-bus
└── src/
    ├── main.rs          ← 入口：打印自述与发布面，未接总线
    ├── audio/mod.rs
    ├── audio/capture.rs ← ALSA 采集 16k/mono/S16_LE
    ├── audio/vad.rs     ← 本地 VAD 边沿检测
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

PCM 与 IMU 属实时控制面摘要；shm 零拷贝传输随 M5（ADR-0007 §裁定 3）。

## 实现进度

v0.0.1 交付结构 + manifest（已过 `tools/ci/check.sh` ②④）。总线接入随 M2（rrd + UDS），
ALSA / 传感器读取随 M3（rr-hal linux 后端）。本地自检：

```bash
cargo run --manifest-path modules/rr3-sense/Cargo.toml --bin rr3-sense
```
