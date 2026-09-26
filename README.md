# AI 语音管家（ESP32）

一块经典 ESP32（伍陆电子学习板）+ 麦克风 + 喇叭做的免唤醒语音助手：直接开口说话，它听懂后用女声回答，还能听懂"开灯/关灯"控制板载继电器。相当于自制的小爱同学。

## 应用使用方法

1. 接线（见下表），板子通电
2. 手机开热点（iPhone：设置→个人热点，打开「最大兼容性」）
3. 打开串口监视器（115200），重插 USB 复位
4. 听到"滴滴"两声后直接说话（离麦克风 30cm 内），说完停顿 1 秒自动提交
5. 约 2~5 秒后喇叭播报回答

**语音指令**：说「开灯」/「关灯」→ 板载继电器吸合/断开（本地关键词，秒响应）

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
main/
├── config.h      ← 所有可调参数：WiFi、API 密钥、模型、音量、VAD 阈值
├── main.c        ← 主流程：对话循环、VAD 断句、语音指令分发
├── app_wifi.c    ← WiFi 联网 + 自动重连
├── app_audio.c   ← I2S 麦克风/功放驱动、提示音、继电器
└── app_cloud.c   ← 硅基流动云端：语音识别 → 大模型 → 语音合成
```

## 可调参数（config.h）

| 参数 | 默认 | 说明 |
|------|------|------|
| `WIFI_SSID` / `WIFI_PASS` | iPhone 热点 | WiFi 名字**区分大小写** |
| `LLM_MODEL` | Qwen2.5-7B（免费） | 换 `"deepseek-ai/DeepSeek-V3"`（收费） |
| `TTS_VOICE` | bella（女声） | 换 `alex` 男声 |
| `PLAY_GAIN` | 3 | 播放音量 4=满，2=半 |
| `REC_GAIN` | 1 | 识别不准可试 2 |
| `VAD_START` / `VAD_END` | 3200 / 2200 | 环境太吵误触发 → 调大 |
| `IDLE_RESTART_MS` | 45000 | 没人说话多久重连服务器 |

## 编译烧录

```bash
export IDF_TOOLS_PATH=~/.espressif
. ~/.espressif/v6.1/esp-idf/export.sh
idf.py build
idf.py -p /dev/tty.usbserial-1410 flash monitor   # 烧录前先关掉串口监视器
```

## 云端服务

硅基流动（siliconflow.cn）一个 API 密钥通吃三个能力，账户需有余额（实名认证有赠款）：

- 识别：`Qwen/Qwen3-ASR-1.7B`（实测 0.4 秒；SenseVoiceSmall 晚高峰要 47 秒，弃用）
- 对话：`Qwen/Qwen2.5-7B-Instruct`（免费，带 4 轮历史记忆）
- 合成：`FunAudioLLM/CosyVoice2-0.5B`

踩坑记录：识别接口必须固定 Content-Length（先声明 6 秒长度，说完提前结束再补零，服务端不支持 chunked）；先连接后说话（握手 1.5 秒期间不能丢录音，否则识别出乱码）。

## 历史实验存档（此前在此工程上做过的实验）

以下实验代码已被后续实验覆盖，接线与玩法记录如下，需要时可在 git 历史或对话记录中恢复：

| 实验 | 接线要点 | 玩法/效果 |
|------|----------|-----------|
| LED 流水灯 | GPIO 18,19,21,22,23 串 220Ω~1kΩ | 依次点亮 |
| 8 路呼吸灯 | GPIO 18,19,21,22,23,16,17,4（高电平亮） | PWM 呼吸渐变 |
| RGB 彩灯 | GPIO5 + WS 排针（开 POWER 开关） | 彩虹流光 |
| WS2812B 点阵屏 8×32 | 5V/GND（适配器 5.0V 严格）/ DIN→GPIO5 | 贪吃蛇/跑马灯/3D立方体/6种动画轮播（数字雨→火焰→海洋→等离子→流星→生命游戏，各60秒） |
| PS2 摇杆贪吃蛇 | VRx→GPIO34, VRy→GPIO35, SW→GPIO32 | 摇杆转向+加速，撞墙闪红，数码管显分 |
| 文字跑马灯（网页配字） | 点阵屏 + SoftAP `ESP32-Marquee` | 手机连热点访问 192.168.4.1 发字，静态居中+彩虹流动，限 5 字 |
| 3D 旋转立方体 | 点阵屏 | 双彩虹线框立方体反向旋转+发光拖尾（ANG_STEP/SCALE 可调） |
| 倒车雷达 | HC-SR04: TRIG→GPIO4, ECHO→GPIO18（3V3 供电）；蜂鸣器 I/O→GPIO19；数码管 CLK→GPIO16, DIO→GPIO17 | 数码管显距离，>50cm绿/20-50cm黄/<20cm红，蜂鸣渐急 |
| 声音/光电/激光对射 | 声音 AO→GPIO4；槽型光电 DO→GPIO4；激光 S 脚短接 VCC | 检测/计数/对射报警 |
| 板载数码管 | CLK→GPIO16, DIO→GPIO17（74HC595） | 显示数字 |
| 板载继电器 | REL→GPIO21 | <10cm 吸合（超声波联动） |
| 实时对讲 | 同语音助手接线 | 录音实时回放，LOOP_GAIN 调音量 |
