# core/bt — 经典蓝牙 SPP 串口控制台

不依赖 WiFi 的近场应急通道。手机装蓝牙串口 App（iOS/Android 的
**Serial Bluetooth Terminal** 均可），配对连接设备名 **RockRobot3**，即可：

- **实时收日志**：所有 `log_writef`/`ESP_LOGx` 输出自动转发到手机（log 模块镜像）
- **发命令控制**：`help` `status` `ver` `logs` `relay on|off` `mic on|off` `selftest` `reboot`

WiFi 崩了、面板打不开时，蓝牙永远能救场。

## 公开 API（include/bt.h）

```c
bt_start();          // 开机调用（NVS 之后，不依赖 WiFi）
bt_connected();      // 是否有蓝牙客户端在线
```

## 技术要点

- 经典蓝牙 SPP（ESP_BT_MODE_CLASSIC_BT），BLE 已关省内存。
- 命令在独立工作线程执行（队列缓冲 4 条），不阻塞蓝牙协议栈回调。
- 日志转发通过 `log_set_mirror` 注册回调实现，与 log 模块解耦。
- 依赖：sdkconfig 需 `CONFIG_BT_ENABLED` / `CONFIG_BT_BLUEDROID_ENABLED` /
  `CONFIG_BT_CLASSIC_ENABLED` / `CONFIG_BT_SPP_ENABLED`。
