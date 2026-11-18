# rr-core —— 机制核切片：总线语义层

承 [ADR-0018](../../../docs/process/adr/ADR-0018-跨操作系统移植-机制核与主承载层分离.md)
裁定 1，0.0.5 第 II 轮批D₁ 落地。`no_std`（core+alloc）可达：裸金属目标（如
xtensa-esp32-none-elf）可编译；不读时钟、不睡眠、不碰 io——时间戳与 task 号
全部由调用方注入。

本 crate 是**总线语义**的唯一真值点：

- 常量层：task 号空间（0 广播 / 1 rrd / ≥2 模块）、EVENT 优先级占位带、
  主题分隔符、契约 schema 版本、`rr3.call` 方法名、trace_id 直连语义；
- 寻址规则：`rr3_task_kind` 归属判定；
- 帧组装：`rr3_frame_compose` / `rr3_event_compose`（payload 类型前缀复用
  `rr-ipc` 的 body 编解码，整帧字节化走 `rr-ipc` 的 `rr3_frame_to_bytes`）。

`rr-bus`（std 同步客户端：UDS + 线程）re-export 本 crate 的常量保持既有 import
路径不变；`net.online` 格的机制核切片落位于本 crate 的执行环部分（批D₂ 进
`runtime` 模块）。

主承载层语义（跨进程监督、进程派生、serde_json、心跳线程）**不在这里**——
那是 `rr-bus` / `rrd` 的事（ADR-0018 裁定 1 的两层切线）。
