# RockRobot3 — 桌面陪伴机器人（ESP32）

一块经典 ESP32（伍陆电子学习板）做的桌面机器人：语音对话、语音控制继电器，计划逐步加上屏幕表情、身体动作和传感器感知。

## 项目结构（按模块分离）

```
RockRobot3/
├── core/         ← 独立可复用模块库（ESP-IDF 组件，高内聚低耦合）
│   ├── llm/      ← 云端大模型对话（带历史记忆，纯文字↔文字）
│   ├── asr/      ← 云端语音识别（流式上传 PCM→文字）
│   ├── tts/      ← 云端语音合成（文字→PCM 回调交付）
│   ├── applog/   ← 日志：接管 ESP_LOGx + RAM 环形缓冲 + 蓝牙镜像转发
│   ├── data/     ← 运行数据：开机/对话/失败/继电器计数（NVS 断电保留）
│   ├── link/     ← 通信链路模块组（一条链路一个模块）
│   │   ├── wifi/     ← WiFi STA+AP（NVS 凭据、断线重连）
│   │   ├── btctl/    ← 经典蓝牙 SPP 串口控制台（免 WiFi 应急通道）
│   │   ├── nearlink/ ← 星闪（接口预留，本硬件无射频）
│   │   └── eth/      ← 以太网（接口预留，未接 PHY）
│   ├── audio/    ← I2S 麦克风 + 功放、提示音、VAD 前置处理、麦克风静音开关
│   ├── relay/    ← 板载继电器开关
│   └── panel/    ← Web 面板 v2：状态/WiFi 配网/硬件控制/实时日志/运行数据/单项自检
│   （统一布局：include/ 公开 API + src/ 实现 + *_config.h 模块参数）
├── firmware-c/   ← 主系统平台：语音管家固件（主流程/对话策略，薄壳）
│   └── main/     ← config.h(对话策略) + main.c(编排各模块)
├── rust/         ← Rust 侧（规划中：esp-idf-svc 全量重写，见「Rust 重写计划」）
└── README.md
```

- **版本号**：git tag 自动生成（`git describe` → PROJECT_VER），面板状态页/蓝牙 `ver` 命令/开机日志三处可见，排查问题先报它
- **问题排查入口**：面板「日志」页看实时交互流，「系统」页看失败计数；不联网时用蓝牙串口 App 连 `RockRobot3` 发 `help`

## 应用使用方法

1. 接线（见下表），板子通电
2. 手机开热点（iPhone：设置→个人热点，打开「最大兼容性」）
3. 打开串口监视器（115200），重插 USB 复位
4. 听到"滴滴"两声后直接说话（离麦克风 30cm 内），说完停顿 1 秒自动提交
5. 约 2~5 秒后喇叭播报回答

**语音指令**：说「开灯」/「关灯」→ 板载继电器吸合/断开（本地关键词，秒响应）

### Web 操作面板

手机连热点 **RockRobot3**（密码 `12345678`）→ 浏览器开 `http://192.168.4.1`（或同一 WiFi 下访问设备 IP）：

- **状态**：联网/IP/信号/运行时长/空闲内存，一键硬件自检（喇叭+继电器+麦克风）
- **WiFi**：扫描附近网络点选连接，账号存 NVS 重启不丢（换 WiFi 不用改代码重烧了）
- **硬件**：继电器开灯关灯、喇叭测试音
- **系统**：重启设备、恢复出厂 WiFi

## 硬件接线

| 模块 | 引脚 | 接 ESP32 |
|------|------|----------|
| INMP441 麦克风 | VDD / GND / SCK / WS / SD / L/R | 3V3 / GND / GPIO4 / GPIO5 / GPIO18 / GND（左声道） |
| MAX98357A 功放 | Vin / BCLK / LRC / DIN / SD / GAIN | 5V / GPIO4（共用）/ GPIO5（共用）/ GPIO25 / 5V（必须，接地=静音）/ 悬空 |
| 喇叭 | OUT+ / OUT- | 功放输出端子 |
| 板载继电器 | REL | GPIO21（板载，免接线） |

I2S 全双工：麦克风和功放共享 BCLK/WS 时钟（GPIO4/GPIO5）。
**注意**：GPIO4/5/18/25 被音频占用，点阵屏（DIN=GPIO5）不能同时使用。

## 软件结构

```
firmware-c/
├── main/
│   ├── config.h      ← 对话策略：VAD 阈值、最长录音、播放音量
│   └── main.c        ← 编排各模块：对话循环、VAD 断句、语音指令分发、TTS 播放回调
└── CMakeLists.txt    ← 通过 EXTRA_COMPONENT_DIRS 挂载 core/ 各组件
```

固件是"薄壳"：联网（core/wifi）、音频（core/audio）、继电器（core/relay）、云端 AI（core/llm）都是 `../core/` 下的独立组件，`main.c` 只做编排。改任何模块参数去对应模块的 `*_config.h`。

## 可调参数（config.h）

| 参数（firmware-c/main/config.h） | 默认 | 说明 |
|------|------|------|
| `PLAY_GAIN` | 3 | 播放音量 4=满，2=半 |
| `VAD_START` / `VAD_END` | 3200 / 2200 | 环境太吵误触发 → 调大 |
| `IDLE_RESTART_MS` | 45000 | 没人说话多久重连服务器 |

模块参数在各模块的 `*_config.h`：WiFi 账号在 `core/wifi`，录音增益 `REC_GAIN` 在 `core/audio`，云端模型/音色在 `core/llm`（`LLM_MODEL` 默认 Qwen2.5-7B 免费，`TTS_VOICE` 默认 bella 女声）。

## 编译烧录

```bash
export IDF_TOOLS_PATH=~/.espressif
. ~/.espressif/v6.1/esp-idf/export.sh
idf.py build
idf.py -p /dev/tty.usbserial-1410 flash monitor   # 烧录前先关掉串口监视器
```

## Rust 重写计划（已决策）

全量用 Rust 实现（esp-idf-svc 路线：跑在 ESP-IDF/FreeRTOS 之上，保住经典蓝牙 SPP 能力）。
分阶段迁移，每阶段 C 固件保持可用，跑通一个模块替换一个：

1. 工具链（espup + ldproxy）→ blink 点灯
2. WiFi + HTTP 客户端（验证云链路）
3. panel HTTP 服务（复用现有 HTML）
4. audio I2S + 语音链路（asr/llm/tts）
5. 蓝牙 SPP（esp-idf-sys 原生绑定）+ applog/data

`rust/` 现存的 esp-hal no_std 脚手架保留作参考（no_std 路线无法实现经典蓝牙 SPP，故主路线选 esp-idf-svc）。

## 面向瑞芯微（Rockchip）的迁移策略

core/ 的模块按平台耦合度分两类，移植 Rockchip 主板时区别对待：

| 类别 | 模块 | 迁移方式 |
|------|------|----------|
| 可移植核心（纯 C + HTTP/存储） | asr / llm / tts / applog / data | 近乎原样平移：HTTP 换 libcurl，NVS 换 SQLite/文件，对话逻辑零改动 |
| 平台适配层（硬件强耦合） | link/*（wifi/btctl/eth/nearlink）、audio、relay、panel | 在 Rockchip/Linux 上重写实现：ALSA/NetworkManager/BlueZ/ NearLink SDK / GPIO sysfs，`include/*.h` 接口形态不变，上层业务代码不动 |

main.c 的「对话策略 + 编排」与面板 HTML 属于第三类：平台无关资产，直接复用。
瑞芯微上 RockRobot3 将从"单芯片固件"升级为"Linux 机器人主控 + 可选 ESP32 协处理器"架构。

## 云端服务

硅基流动（siliconflow.cn）一个 API 密钥通吃三个能力，账户需有余额（实名认证有赠款）：

- 识别：`Qwen/Qwen3-ASR-1.7B`（实测 0.4 秒；SenseVoiceSmall 晚高峰要 47 秒，弃用）
- 对话：`Qwen/Qwen2.5-7B-Instruct`（免费，带 4 轮历史记忆）
- 合成：`FunAudioLLM/CosyVoice2-0.5B`

踩坑记录：识别接口必须固定 Content-Length（先声明 6 秒长度，说完提前结束再补零，服务端不支持 chunked）；先连接后说话（握手 1.5 秒期间不能丢录音，否则识别出乱码）。

## 历史实验存档（此前在此工程上做过的实验）

灯带/点阵屏类实验文档已移至 `../LightSignal/`，倒车雷达实验文档已移至 `../倒车雷达/`。其余实验接线与玩法记录如下，需要时可在 git 历史或对话记录中恢复：

| 实验 | 接线要点 | 玩法/效果 |
|------|----------|-----------|
| LED 流水灯 | GPIO 18,19,21,22,23 串 220Ω~1kΩ | 依次点亮 |
| 8 路呼吸灯 | GPIO 18,19,21,22,23,16,17,4（高电平亮） | PWM 呼吸渐变 |
| PS2 摇杆贪吃蛇 | VRx→GPIO34, VRy→GPIO35, SW→GPIO32 | 摇杆转向+加速，撞墙闪红，数码管显分 |
| 声音/光电/激光对射 | 声音 AO→GPIO4；槽型光电 DO→GPIO4；激光 S 脚短接 VCC | 检测/计数/对射报警 |
| 实时对讲 | 同语音助手接线 | 录音实时回放，LOOP_GAIN 调音量 |
