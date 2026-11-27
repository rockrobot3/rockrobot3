# tools —— 主机侧工具

| 目录 | bin | 用途 |
|---|---|---|
| `mkmodule/` | `rr3-mkmod` | 生成合规模块骨架，落盘**五个文件**（`module.toml` / `config.toml` / `Cargo.toml` / `src/main.rs` / `README.md`），产出物自动过 schema 校验 |
| `ci/` | — | `check.sh`：规约检查的唯一实现与唯一入口，**全步**（① workspace 编译含测试目标 / ①b workspace 全量测试 / ② manifest schema 与注册表形状 / ②b mkmod 产物即契约 / ③ 主题前缀归属 / ④ 独立 Rust 工程编译 / ⑤ 文档门：已跟踪 md 非空、相对链接不断 / ⑥ 独立工程测试接线 / ⑦ §13 静态断言四件 / ⑧ 件级判据三门：对拍计数·依赖闭包·注入面校验 / ⑨ 增量门 / ⑩ 面板覆盖率 / ⑫ 质量度量：重复率·圈复杂度·函数名长度），本地与 CI 跑同一命令；标题沿用方案 D7 的"三检查"语义命名。步序、命令与每步条数只登记在 [`docs/process/conventions.md`](../docs/process/conventions.md) §2 |
| `rrctl/` | `rr3-ctl` | 订阅总线查状态/日志/触发 OTA 的运维 CLI（命令面与参数见其 README） |
| `sim/` | `rr3-sim` | mock HAL 设备源 + 假模块，x86 全链路联调 |
| `pi-periph-probe/` | — | 树莓派直连板子的**七支** Python 探针（全功能冒烟、带宽/混跑压测、U3 digest、帧面残尾、行面 robustness、写前记账），缺省 1.5M 档，`RR3_PROBE_TTY`/`RR3_PROBE_BAUD` 可换口换档；公共层 `probe_common.py` 是波特率解析/开口/收流/帧扫描与字节分类的**唯一实现**（判据口径不得在各脚本里另写一套） |
| `deploy/` | — | 部署/烧录/打包/OTA 的 **14 支 .sh** 主机侧脚本与 `Containerfile`、`console-log.py`（`audio-rec`、`build-onboard`、`build-riscv-image`、`flash-artifact`、`flash-esp32`、`fw-set`、`image-esp32`、`image-smoke`、`install-bundle`、`ota-bundle`、`pack-arm64`、`pack-bundle`、`stage-image`、`verify-board-config`），按名即义；每支脚本头部注释是唯一详注 |
| `esp32-periph-base/` | `esp32-periph-base` | ESP32 外设底座档固件：OLED + 双轴舵机 + UART 帧行指令 + I2S 麦（esp 专用工程，归专用门） |
| `esp32-uart-echo/` | `esp32-uart-echo` | UART 板级 round-trip 裸金属 echo 夹具（esp 专用工程，归专用门） |
| `espidf-sentinel/` | `espidf-sentinel` | 层③ 真板哨兵：esp-rs std，板内跑 rr-core 运行时（机制核真板消费者） |
| `espidf-std-hello/` | `espidf-std-hello` | ESP-IDF std 构建链冒烟件：版本钉死对表与三坑复现（见其 README） |
| `periph-mech/` | — | 外设机制核库（lib `periph_mech`，无 bin）：帧收发 / OLED 编码 / 舵机 / 轨压判定，板侧工程共享 |
| `pi-periph-base/` | `pi-periph-base` | 树莓派 4B 板侧五端口后端：UART / I2C / PWM / ADS1115 |
| `rr3-audio-rec/` | `rr3-audio-rec` | 板面音频 Event 帧 → wav 落盘收流端 |
| `rr3-bridge/` | `rr3-bridge` | UART ↔ UDS 字节中继桥：零策略零解析 |
| `rr3-codegen/` | `rr3-codegen` | 契约真值 → 板侧常量表生成器 |
| `rr3-pack/` | `rr3-pack` | deb 打包器 |
| `rr3-sentinel-harness/` | `rr3-sentinel-harness` | 哨兵宿主对手件：扮总线侧收注册/心跳 |

`rr3-mkmod` 是**独立工程**（`tools/mkmodule/Cargo.toml` 自带空 `[workspace]`），不在 `base/runtime` 的 workspace 内：它以路径依赖消费 `rr-registry`，把自己挂进 workspace 会让"生成器"变成"运行时的一部分"。测试用 `cargo test --manifest-path tools/mkmodule/Cargo.toml` 单独跑，CI 里单列为 ②b。
mkmod 是规约的执行器：命名、模块骨架的五件结构、主题前缀规则都在生成器里强制，而不是靠人记
[`docs/process/naming.md`](../docs/process/naming.md)。

**板侧后端目录同形**（2026-11-27 批68 裁定）：承载某个芯片档的板侧底座（今天是
`esp32-periph-base/` 与 `pi-periph-base/`）各有一份根 `README.md`，写同一组四件事——端口表、
策略面单点、接线表、未收口；其余主机侧小工具不单立 README，只在上表占一行（一支一行的
唯一说明）。新增板档按此形状落，不再散落第二份专题文档。
