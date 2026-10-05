# rr3-connect

连接模块：WiFi 链路、云端会话、蓝牙配网备选。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-connect/
├── module.toml
├── config.toml
├── Cargo.toml
└── src/
    ├── main.rs    ← 入口：总线接入 + 链路拍（翻转必发/周期保底）+ session 应答（0.0.2 T4c）
    │                + wifi/scan|connect|forget 三 RPC（0.0.4 批D）
    ├── wifi.rs    ← net/link 状态机（Rr3LinkEdge：首拍基线 + 翻转必发）+ 事件体（mode 配网态）
    │                + 配网面（Rr3Provision 状态机 + 三 RPC 参数/结果裁决，0.0.4 批D）
    ├── session.rs ← cloud/session 应答面（action=open，链路就绪才应答；sim 回 mock）
    └── bt.rs      ← 蓝牙（BlueZ）：配网备选通道，主题未登记，落地前先改契约
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| publish | `rr3-connect/net/link` | event |
| rpc (exposes) | `rr3-connect/cloud/session` | request |
| rpc (exposes) | `rr3-connect/wifi/scan` | request（0.0.4 批D：sim 后端模拟列表，`backend:"sim"` 诚实标注） |
| rpc (exposes) | `rr3-connect/wifi/connect` | request（0.0.4 批D：`ssid`+`pass` 密码≥8 位 legacy 语义） |
| rpc (exposes) | `rr3-connect/wifi/forget` | request（0.0.4 批D：恢复出厂 WiFi，≠ M6 整机恢复出厂） |

`subscribes` 为空是**设计**：本模块没有可订阅的命令流（广播帧只排空）。
`net.online` 是基础层恒在位格，运行期读数由本模块经 `net/link` 发布，
反应归策略层（[`slots.toml`](../../contracts/slots.toml) 同条注）。

配网面（0.0.4 批D）：AP 配网状态机归本模块（`Rr3Provision`，凭据内存
`Option<(ssid, pass)>`，无凭据 = `mode:"ap"`、有凭据 = `mode:"station"`），
`mode` 随 `net/link` 事件体发布；**凭据不落盘**（conventions §3，重启即忘），
真凭据持久化（wpa_supplicant/NVS）与 NetworkManager 真扫描属板上件
cfg 门后置——如实声明，不 claim。

蓝牙（`net.bt` 格位）由本模块 provide，但主题尚未登记——落地前须先改
[`topics.toml`](../../contracts/topics.toml)（守"不预先占位"）。

## 本地自检

```bash
cargo run --manifest-path modules/rr3-connect/Cargo.toml --bin rr3-connect
```

**交付到哪一批不在本文件登记**：NetworkManager / BlueZ 接入（rr-hal linux 后端，
板上件 cfg 门骨架）今天有没有实现，唯一落点是 [`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
