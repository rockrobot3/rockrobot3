<p align="center">
  <img src=".github/logo/logo.png" width="100%" alt="Rockrobot 3 · 洛克三号 Logo">
</p>

<h1 align="center">Rockrobot-3（洛克三号）</h1>

<p align="center">
  <a href="https://atomgit.com/rockrobot/rockrobot3"><img src="https://atomgit.com/rockrobot/rockrobot3/star/badge.svg" alt="AtomGit"></a>
  <a href="https://github.com/rockrobot3/rockrobot3"><img src="https://img.shields.io/github/stars/rockrobot3/rockrobot3?style=flat&logo=github&label=stars" alt="GitHub stars"></a>
  <a href="https://github.com/rockrobot3/rockrobot3/releases/latest"><img src="https://img.shields.io/github/v/release/rockrobot3/rockrobot3?label=version&color=5a6b7e" alt="version"></a>
  <img src="https://img.shields.io/badge/Rust-1.100.0-DEA584?logo=rust&logoColor=white" alt="Rust 1.100.0">
  <a href="https://atomgit.com/openairymax"><img src="https://img.shields.io/badge/OpenAirymax-DA203E" alt="AirymaxOS"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-4a90d9?logo=apache&logoColor=white" alt="license Apache-2.0"></a>
</p>

## 简介

Rockrobot-3（洛克三号）是一个机器人系统底座。  

同一个机器本体换芯片，**3天**即可快速跨芯片平台部署，无需修改代码。  


## 架构特点

洛克三号同时包含基于该底座构建的机器人本体。底座提供进程管理、进程间通信、硬件抽象与模块框架等通用能力，使用 Rust 编写；本体在其上实现语音感知、动作执行、表情交互、电源管理、网络连接与对话编排，其中对话由云端的语音识别、LLM与语音合成服务驱动。同一套代码在架构上支持两类目标平台：Linux 单板计算机（树莓派、RK3566 等）与 ESP32 微控制器。更换平台时，无需重写软件。

- **跨平台**：程序按是否依赖操作系统分为两层。消息格式、控制逻辑等与操作系统无关，线程、网络、驱动等与平台相关。迁移到新平台时，前者保持不变，后者按统一接口重新实现，并提供该机型的配置文件。
- **模块化**：功能划分为六个模块，模块之间不直接调用，统一通过消息总线通信。各模块可以发布和订阅哪些消息由契约文件（`contracts/`）规定；契约只允许新增内容、不允许修改已有定义，以保证新旧版本兼容。
- **配置与代码分离**：芯片型号、引脚分配、启用哪些模块等硬件信息记录在配置文件（`profiles/`、`hardware/`）中，更换硬件或机型通过修改配置完成，无需改动程序代码。
- **启动前校验**：系统启动时逐项校验契约、机型配置与接线信息，任何一项不满足即拒绝启动（fail-closed），不在配置不完整的情况下运行。

## 功能模块

本体由以下六个模块构成：

| 模块 | 职责 |
|---|---|
| rr3-sense | 语音采集与人声检测（VAD） |
| rr3-act | 舵机、灯效、继电器控制；实时控制回路在本地运行，不经过消息总线 |
| rr3-interact | 表情显示、音频播放、本地面板 |
| rr3-power | 电池、充电与供电管理；低电量处理在本地执行 |
| rr3-connect | Wi-Fi 与网络连接 |
| rr3-brain | 对话流程编排，串联语音识别、大语言模型与语音合成（云端服务） |

## 仓库结构

```
rockrobot3/
├── contracts/    # 公共契约：消息格式、主题注册表、能力定义；只增不改，保证版本兼容
├── base/
│   ├── runtime/  # 底座运行时：消息总线、核心状态机、守护进程等
│   ├── os/       # 系统镜像构建（deb 软件包与 Buildroot 镜像）
│   └── ota/      # 远程升级
├── modules/      # 机器人功能模块（见上表）
├── profiles/     # 机型配置：芯片、外设、引脚、启用的模块
├── hardware/     # 接线配置：器件与引脚的对应关系
├── tools/        # 检查脚本、模块生成器、仿真工具（详见 tools/README.md）
└── docs/         # 架构文档、开发文档与状态说明
```

两层结构与目录的对应关系：底座位于 `base/`，本体功能位于 `modules/`，公共契约位于 `contracts/`，机型与硬件信息位于 `profiles/` 与 `hardware/`。引入新机型时，需要改动的只有 `profiles/`、`hardware/` 与平台接口适配部分。

## 快速开始

```bash
git clone https://github.com/rockrobot3/rockrobot3.git
cd rockrobot3
bash tools/ci/check.sh
```

`check.sh` 运行仓库的完整检查，与 CI 执行的内容一致，需要 Bash 环境。

- 生成新模块骨架：`tools/mkmodule`
- 无硬件仿真：`tools/sim`
- 运行期接口说明（启动参数、RPC、注册规则）：[docs/reference/](docs/reference/)

## 参与

欢迎通过 Issue 参与讨论与贡献。

## 许可

本项目采用 [Apache License 2.0](LICENSE)。
