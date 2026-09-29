# panel —— Web 操作面板模块

ESP32 内置 HTTP 服务 + 单文件内嵌网页（无外部资源），手机直连即可管理设备。

## 使用

- 手机连热点 `RockRobot3`（密码 `12345678`）→ 浏览器开 `http://192.168.4.1`
- 或设备联网后，与手机同一 WiFi 下访问设备 IP（串口/面板状态页可查）
- 固件侧：`wifi_start()` 之后调用一次 `panel_start()`

## API 表

| 函数 | 说明 |
|------|------|
| `void panel_start(void)` | 启动 HTTP 面板（仅调一次） |

## 面板功能

| 页签 | 功能 |
|------|------|
| 状态 | 联网状态 / WiFi 名 / IP / 信号 / 运行时长 / 空闲内存 / 热点在线数 / 继电器状态；硬件自检（喇叭+继电器+麦克风峰值） |
| WiFi | 扫描附近网络 → 点选输入密码连接（NVS 持久化，重启不丢） |
| 硬件 | 继电器开灯关灯、喇叭测试音 |
| 系统 | 重启设备、恢复出厂 WiFi（清除 NVS 凭据回到内置默认账号） |

## HTTP 接口（也可脚本调用）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET  | `/` | 面板页面 |
| GET  | `/api/status` | 状态 JSON |
| GET  | `/api/wifi/scan` | 扫描结果 JSON |
| POST | `/api/wifi/connect` | body `{"ssid":"x","pass":"y"}` |
| POST | `/api/relay` | body `{"on":1}` 或 `{"on":0}` |
| POST | `/api/beep` | 播放测试音 |
| POST | `/api/selftest` | 硬件自检，返回结果 JSON |
| POST | `/api/reboot` | 重启设备 |
| POST | `/api/factory_reset` | 清 WiFi 凭据并重启 |

## 配置

`src/panel_config.h`：`PANEL_PORT`（HTTP 端口，默认 80）。
