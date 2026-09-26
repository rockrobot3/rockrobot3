# core —— 独立可复用模块库

每个子目录是一个独立的 ESP-IDF 组件，高内聚、低耦合：任何工程（firmware-c 或新固件）都能按需挂载复用。**新模块照此模板新增**。

## 统一目录模板

```
core/<模块名>/
├── include/<模块名>.h   ← 公开 API（使用方只 include 这个头）
├── src/                 ← 实现与模块配置（源码都集中在 src/）
│   ├── <模块名>.c
│   └── <模块名>_config.h ← 模块自己的参数（密钥/引脚/阈值，改这里不碰源码）
├── CMakeLists.txt       ← 组件注册：SRCS "src/xxx.c" + INCLUDE_DIRS "include" + PRIV_INCLUDE_DIRS "src"
└── README.md            ← API 表 + 配置说明 + 复用方法
```

模块根目录只允许这 4 类文件/目录（include、src、CMakeLists、README），不出现任何源码或配置散件。

## 模块清单

| 模块 | 能力 | 依赖 |
|------|------|------|
| `llm` | 云端 AI：流式语音识别(ASR) + 大模型对话(带记忆) + 语音合成(TTS, PCM 回调) | esp_http_client / cJSON |
| `wifi` | WiFi STA 联网（NVS 凭据优先）+ 断线重连 + AP 热点常开 + 扫描/状态查询 | esp_wifi / nvs_flash |
| `audio` | I2S 全双工：INMP441 麦克风 + MAX98357A 功放、提示音、VAD 前置处理 | esp_driver_i2s |
| `relay` | 板载继电器开关（含状态查询） | esp_driver_gpio |
| `panel` | Web 操作面板：状态/WiFi 配网/硬件控制/自检/重启/恢复出厂 | esp_http_server |

## 组件间依赖关系（单向，无环）

```
main（主流程/对话策略）──► llm / audio / relay / wifi / panel
panel ──► wifi / audio / relay          （面板操作各模块）
```

模块之间互不依赖：llm 的音频输出走 PCM 回调（audio 模块或任何播放器实现），wifi 只管联网。硬件引脚等参数各模块自持（`*_config.h`），固件层只留对话策略（VAD 阈值、最长录音、播放增益）。

## 复用方法（以 firmware-c 为例）

1. 工程根 `CMakeLists.txt`（`project()` 之前）：
   ```cmake
   list(APPEND EXTRA_COMPONENT_DIRS "${CMAKE_CURRENT_LIST_DIR}/../core/llm"
                                    "${CMAKE_CURRENT_LIST_DIR}/../core/wifi"
                                    "${CMAKE_CURRENT_LIST_DIR}/../core/audio"
                                    "${CMAKE_CURRENT_LIST_DIR}/../core/relay"
                                    "${CMAKE_CURRENT_LIST_DIR}/../core/panel")
   ```
2. 使用方组件 `REQUIRES` 加模块名（如 `audio`），代码 `#include "audio.h"`
3. 调模块参数：改对应模块 `src/` 里的 `<模块名>_config.h`
