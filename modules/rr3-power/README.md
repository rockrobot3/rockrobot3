# rr3-power

电源模块：电池、充电、低压关断兜底。骨架由 `rr3-mkmod` 生成（conventions §3）。

## 目录结构

```
modules/rr3-power/
├── module.toml
├── config.toml
├── Cargo.toml
└── src/
    ├── main.rs      ← 入口：打印自述与发布面，未接总线
    ├── battery.rs   ← 电池采样 + 关断决策（本地）
    └── charger.rs   ← 充电状态
```

## 总线面

| 方向 | 主题 | kind |
|---|---|---|
| publish | `rr3-power/battery/state` | event |
| publish | `rr3-power/charger/state` | event |

`subscribes` 为空是**设计**：低压关断是本模块的本地决策，不依赖总线、不等 brain
（architecture §7.3 安全兜底、topics.toml 同条 note）。总线路径故障不得影响关断。

## 实现进度

v0.0.1 交付结构 + manifest。ADC/ gauge 读取随 M5（rr-hal linux 后端）。本地自检：

```bash
cargo run --manifest-path modules/rr3-power/Cargo.toml --bin rr3-power
```
