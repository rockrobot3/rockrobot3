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

## 密钥注入（SILICONFLOW_API_KEY）

`core/llm|asr|tts` 的 `*_config.h` 只引用宏 `SILICONFLOW_API_KEY`，**密钥不在仓库里**。
构建前注入环境变量，缺失则 CMake 直接 FATAL_ERROR（不产出能编译但上线 401 的固件）：

```bash
# bash / zsh
export SILICONFLOW_API_KEY=sk-…
idf.py build
```

```powershell
# PowerShell
$env:SILICONFLOW_API_KEY = "sk-…"
idf.py build
```

注入链路：组件 `CMakeLists.txt` 在 `idf_component_register` 之后用
`target_compile_definitions(${COMPONENT_LIB} PRIVATE SILICONFLOW_API_KEY="$ENV{SILICONFLOW_API_KEY}")`
把环境变量变成编译期宏；三个 `*_config.h` 里的 `#define API_KEY SILICONFLOW_API_KEY`
因此零改动地沿用原有 `snprintf(auth, …, "Bearer %s", API_KEY)` 使用点。
`*_config.h` 的 `#ifndef SILICONFLOW_API_KEY / #error` 是第二道保险：
若某个新工程忘记挂载这段 CMake，构建同样在编译该组件时报错而不是静默产出坏固件。

历史注记：2026-09-29 之前该 key 明文写在三个头文件里并已推送远程，**已按泄露处理并在服务商侧轮换**；
仓库不做历史重写（filter-repo + force push 的协作代价大于收益），旧密钥视为永久失效凭据。
`firmware-c/sdkconfig` 已从索引移出（生成物，且会把密钥相关配置落进本地产物），`sdkconfig.defaults` 保持跟踪。

## 模块清单

| 模块 | 能力 | 依赖 |
|------|------|------|
| `llm` | 云端大模型对话（带历史记忆，纯文字↔文字） | esp_http_client / cJSON |
| `asr` | 云端语音识别：流式上传 PCM → 文字 | esp_http_client / cJSON |
| `tts` | 云端语音合成：文字 → PCM 回调交付（模块不管喇叭） | esp_http_client / cJSON |
| `applog` | 日志：接管 ESP_LOGx + RAM 环形缓冲 + 镜像回调（面板/蓝牙回放） | esp_log |
| `data` | 运行数据计数（开机/对话/失败/继电器），NVS 断电保留 | nvs_flash |
| `link/wifi` | WiFi STA 联网（NVS 凭据优先）+ 断线重连 + AP 热点常开 + 扫描/状态查询 | esp_wifi / nvs_flash |
| `link/btctl` | 经典蓝牙 SPP 串口控制台：免 WiFi 收日志、发命令控设备 | IDF bt（Bluedroid） |
| `link/nearlink` | 星闪 NearLink（接口预留：本硬件无射频） | 无 |
| `link/eth` | 以太网（接口预留：未接 PHY） | 无 |
| `audio` | I2S 全双工：INMP441 麦克风 + MAX98357A 功放、提示音、VAD、麦克风静音开关 | esp_driver_i2s |
| `relay` | 板载继电器开关（含状态查询） | esp_driver_gpio |
| `panel` | Web 面板 v2：状态/WiFi 配网/硬件控制/单项自检/实时日志/运行数据 | esp_http_server |

⚠ **组件命名**：不能用 IDF 内置组件名（`log`/`bt` 会把内置组件顶掉导致全工程编译崩），
故日志模块叫 `applog`、蓝牙模块叫 `btctl`。

## 组件间依赖关系（单向，无环）

```
main（主流程/对话策略）──► asr / llm / tts / link/* / audio / relay / panel / applog / data
panel ──► link/wifi / audio / relay / applog / data      （面板操作各模块）
btctl ──► link/wifi / audio / relay / applog / data      （蓝牙控制台，免 WiFi 应急）
applog ◄── 各模块（log_writef 打点，蓝牙连接时实时镜像转发）
```

模块之间互不依赖：llm 的音频输出走 PCM 回调（audio 模块或任何播放器实现），wifi 只管联网。
硬件引脚等参数各模块自持（`*_config.h`），固件层只留对话策略（VAD 阈值、最长录音、播放增益）。

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
