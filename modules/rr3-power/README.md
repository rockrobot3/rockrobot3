# rr3-power

电源模块：电池、充电、低压关断兜底。结构由 `rr3-mkmod` 生成
（[`conventions.md`](../../docs/process/conventions.md) §3）。

## 目录结构

```
modules/rr3-power/
├── module.toml
├── config.toml
├── Cargo.toml
└── src/
    ├── main.rs      ← 入口：总线接入 + 采样/低电/充电三拍主循环（0.0.2 T4b 实装）
    ├── battery.rs   ← 电池采样 + 低电边沿机（同一次低电段恰发一次）
    └── charger.rs   ← 充电状态（由电池读数的 charging 位派生）
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| publish | `rr3-power/battery/state` | event |
| publish | `rr3-power/battery/low` | event |
| publish | `rr3-power/charger/state` | event |

`subscribes` 为空是**设计**：低压关断是本模块的本地决策，不依赖总线、不等 brain
（[`architecture.md`](../../docs/architecture/architecture.md) §7.3 安全兜底、
[`contracts/topics.toml`](../../contracts/topics.toml) 同条 note）。总线路径故障不得影响关断。

## 本地自检

```bash
cargo run --manifest-path modules/rr3-power/Cargo.toml --bin rr3-power
```

**交付到哪一批不在本文件登记**：ADC / gauge 读取（rr-hal linux 后端）今天有没有实现，
唯一落点是 [`docs/status.md`](../../docs/status.md)；门禁与用例条数见
[`docs/process/conventions.md`](../../docs/process/conventions.md) §2。
