# rr3-connect

连接模块：WiFi 链路、云端会话、蓝牙配网。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-connect/
├── module.toml
├── config.toml
├── Cargo.toml
└── src/
    ├── main.rs       ← 入口：打印自述与发布面，未接总线
    ├── wifi.rs       ← NetworkManager / D-Bus 链路管理
    ├── session.rs    ← 云端会话（凭据不持久化在本模块）
    └── bt.rs         ← BlueZ 配网备选
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| publish | `rr3-connect/net/link` | event |
| rpc | `rr3-connect/cloud/session` | request |

`bt.rs` 尚无登记主题：启用前必须先改 `contracts/topics.toml`（改契约 = 走 review）。

## 本地自检

```bash
cargo run --manifest-path modules/rr3-connect/Cargo.toml --bin rr3-connect
```

**交付到哪一批不在本文件登记**：D-Bus 交互与 TLS 会话今天有没有实现，唯一落点是
[`docs/status.md`](../../docs/status.md)；密钥只经构建期环境变量注入这条纪律见
[`conventions.md`](../../docs/process/conventions.md) §3。
