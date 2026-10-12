# rr3-connect

连接模块：WiFi 链路、云端会话、蓝牙配网备选。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-connect/
├── module.toml
├── config.toml
├── composite.toml ← 组合件装配清单（⊗ 链：wifi ⊗ bt）
├── Cargo.toml
├── atoms/         ← 原子件（每格一件，五件套：atom.toml + 入口 + mech + assemble.toml + private.rs）
│   ├── wifi/      ← net.wifi：net/link 状态机（Rr3LinkEdge：首拍基线 + 翻转必发）+ 事件体（mode 配网态）
│   │               + 配网面（Rr3Provision 状态机 + wifi/scan|connect|forget 三 RPC，0.0.4 批D）
│   └── bt/        ← net.bt：蓝牙链路状态机骨架（Rr3BtState 三态转移，0.0.4 批E）；
│                    格记 placeholder（0.0.5 MC-9），主题不登记，BlueZ 真后端板上后置（cfg 门）
└── src/
    ├── main.rs    ← 入口：只装配与启动（≤80 行）
    ├── composite.rs ← 组合件：config 解析 + HAL 链路面探活 + 链路拍（翻转必发/周期保底）
    │                + 事务面四 exposes 的点名分发
    └── session.rs ← cloud/session 应答面（action=open，链路就绪才应答；sim 回 mock）
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
`mode` 随 `net/link` 事件体发布；**件内不写文件**，凭据持久化由**后端承载**
（MC-6 已结 `084d081`：linux = NetworkManager 连接配置，`forget` 真删除，
connect/forget 先过 store 再转状态机 fail-closed），重启恢复 station；
NetworkManager 真扫描属板上件 cfg 门后置——如实声明，不 claim。

蓝牙（`net.bt` 格位）由本模块 provide：链路状态机骨架已立（0.0.4 批E，
`Rr3BtState` Off/Discoverable/Connected 三态纯函数转移，非法转移拒绝且
状态不动，单测钉死；开机初值经 config `[rr3-connect.bt]` up 数据化注入
（0.0.5 批I N-1：env 注入口退役），缺省关）。**主题不登记**（0.0.4 方案
D5：btctl 的日志回放/远程命令语义已被
rrd 日志环 + `rr3.ctl.logs` 与 rr3-ctl 承接，蓝牙链路暂无可发布事件）——
启用事件面前须先改 [`topics.toml`](../../contracts/topics.toml)（守
"不预先占位"）；真后端属板上件（cfg 门，实装依赖硬件，后置板上
批次），本骨架不 claim 真射频链路——`net.bt` 格因此记 `placeholder`
（0.0.5 批M MC-9 裁定 (a)：承诺 ≤ 事实；触发条件 = 配网备选通道立项 /
BlueZ 真后端批，⑧-a 门只读 [`slots.toml`](../../contracts/slots.toml)
的 status 数据位）。

## 本地自检

```bash
cargo run --manifest-path modules/rr3-connect/Cargo.toml --bin rr3-connect
```

**交付到哪一批不在本文件登记**：NetworkManager / BlueZ 接入（rr-hal linux 后端，
板上件 cfg 门骨架）今天有没有实现，唯一落点是 [`docs/status.md`](../../docs/status.md)；
门禁与用例条数见 [`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
