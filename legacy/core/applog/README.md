# core/log — 日志模块

拦截 ESP-IDF 全部 `ESP_LOGx` 日志 + 应用事件日志，存入 RAM 环形缓冲（200 条），
面板 `/api/logs` 和蓝牙 `logs` 命令可随时回放——设备出问题不再只能靠盯串口。

## 公开 API（include/log.h）

| 函数 | 说明 |
|---|---|
| `log_init()` | 开机尽早调用，接管日志输出（串口照常打印） |
| `log_writef(fmt, ...)` | 应用事件写日志（带运行秒数前缀，同步打印串口） |
| `log_dump_json(out, sz, n)` | 最近 n 条日志导出为 JSON 数组（新的在前） |
| `log_count()` | 缓冲中现有条数 |
| `log_set_mirror(fn)` | 每条新日志回调 `fn(line)`（core/bt 用它做蓝牙转发） |

## 注意

- 环形缓冲在 RAM 里，重启后清零；要留证据先在面板/蓝牙里导出再重启。
- 裸 `printf` 不经过 esp_log 钩子，抓不到；应用代码一律用 `log_writef` 或 `ESP_LOGx`。
