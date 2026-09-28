# rock3 命名规约速查

- **状态**：v0.2（对应 ADR-0005）
- **决策**：项目所有全局符号统一以 `rr3` 为归属标识；基座基础设施 crate 例外（`rr-`，无 3），用于一眼区分"基座 vs 模块"。

## 1. 命名矩阵

| 对象 | 规则 | 示例 |
|---|---|---|
| 模块目录 / crate / 进程 / bin | `rr3-<域>` | `rr3-sense`、`rr3-brain`、`rr3-ctl` |
| 基座内部 crate（workspace 成员） | `rr-<设施名>`（无 3） | `rr-ipc`、`rr-bus`、`rr-hal` |
| Rust lib target（crate 真名） | 连字符转下划线 | `rr3_sense` |
| Rust 跨 crate 导出自由函数 | `rr3_<域>_<动作>` | `rr3_ipc_publish()`、`rr3_hal_audio_open()` |
| Rust 导出类型 | `Rr3` 前缀 | `Rr3Envelope`、`Rr3Error` |
| Rust trait | `Rr3` 前缀；方法不加（挂在 trait 下无歧义） | `trait Rr3AudioInput { async fn start_capture(); }` |
| Rust crate 内部私有函数 | 不加前缀（crate 可见性已限定归属） | `fn parse_vad_frame()` |
| Python 包 / 模块 | `rr3_<域>` | `rr3_brain.dialog` |
| Python 包级可见全局函数 | `rr3_<域>_<动作>` | `rr3_brain_submit_text()` |
| C（未来新增；legacy 已冻结） | `rr3_<域>_<动作>` | `rr3_ota_slot_switch()` |
| 事件 / RPC 主题 | `rr3-<模块>/<域>/<事件>` | `rr3-sense/audio/vad_start` |
| 配置键 / 环境变量 | `RR3_` 大写前缀 | `RR3_LOG_LEVEL`、`RR3_BUS_PATH` |
| systemd 单元 | `rr3-<域>.service` | `rr3-sense.service` |
| D-Bus 总线名 | `cn.rockrobot.rr3.<域>` | `cn.rockrobot.rr3.connect` |

## 2. 边界规则（Rust 前缀的精确落点）

决策精神：**全项目任意代码中出现的"跨边界符号"都能一眼识别归属**，而非每个私有方法都带前缀。

- **加前缀**：跨 crate 导出的自由函数、导出类型、trait 名 —— 消费方在自己代码里调用时可见归属
- **不加前缀**：crate 内部私有函数、trait 的方法（方法命名空间已挂在 trait/impl 下）
- 该边界由 CI 规约检查兜底（后续可在 tools/ 加 lint 脚本）

## 3. 一致性校验（rrd 启动时强制）

1. 目录名 == crate 名 == 进程名 == manifest.name
2. 事件/RPC 主题前缀 == manifest.name（越权发布拒绝注册）
3. 主题全量登记于 `contracts/topics.toml`

## 4. 模块目录四件套（mkmodule 生成模板）

```
modules/rr3-<域>/
├── module.toml      ← manifest：身份/能力/事件/资源/健康策略
├── config.toml      ← 运行参数（改参数不碰源码）
├── src/             ← Rust crate 或 Python 包
└── README.md        ← 能力表 + 事件表 + 配置说明
```
