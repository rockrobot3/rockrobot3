# rr-hal-api

能力面契约（no_std，ADR-0018 修订 1 ③，0.0.5 批I F1）：件 ↔ 硬件的唯一契约面。

- 9 trait **同步签名**（`Rr3GpioOut` / `Rr3PwmServo` / `Rr3AudioSource` / `Rr3AudioSink` / `Rr3Power` / `Rr3Link` / `Rr3Display` / `Rr3Sensor` / `Rr3DigitalIn`），去 `Send + Sync`，写 `&mut self`、读 `&self`。批N3 起后两个为**契约面先行**（无实现件——件一 `Rr3Actuator` 先例），传感/数字输入两半边缺口在能力面立户（全案 §3.5 家族 E）。
- 值类型去 `String`/`Vec`（core-only）：`Rr3SsidText`（定长 32 octets）、`Rr3ErrText`（定长 128 字节错误文案）、`read`/`write` 走借用切片。
- 占位后端 `Rr3UnavailableSource`：能力缺失显式回 `Unsupported`，不静默退化。
- 实现层在 [`rr-hal`](../rr-hal)（Linux/sim 后端），对外路径由其 re-export 保持；契约名不落地两份。
- 禁词执法：本 crate 属机制核面（⑦-a 扫描面 + xtensa 编译门两处清单，C8）。
