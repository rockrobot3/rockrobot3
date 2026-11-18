# 供给自证表——esp32-mini（F-B 桥接微型，轻承载档首行）

> 批G ③ 列（点验矩阵轻承载档首行）的填实行。定性沿用
> [模板](供给自证表模板.md)：**装载门，非行为证据**——只证"描述符没撒谎"
> （声明值 × 载体事实逐键一致），不证 port 正确；运行期实测归 ⑦ 列 / E4 真板。
> 声明值唯一数据事实源 = [`profiles/esp32-mini.toml`](../../profiles/esp32-mini.toml)
> （照抄，不手填第二份）；核对门 = rrd 装载链 `rr3_profile_boot_inputs`
> （matrix.sh ③ 列 24 条 + ④ 列 contract 38 条，含 G6 档位↔watchdog 一致性与
> L-10 tier↔tier_scope）。

## 表（esp32-mini 装配演练档 × ESP32 真板哨兵）

| 键（来源段） | 声明值 | 载体事实/实测值 | 证据 | 环境（指纹） |
|---|---|---|---|---|
| `os.route`（[os]） | 段缺省（rtos 档不适用 route 轴） | 无 Linux 底座镜像路线：esp-rs std + FreeRTOS 直接承载，事件循环 = rr-core::runtime 单任务节拍环 + reader/心跳线程 | esp32-mini.toml [os] 缺省注记；批W1 espidf-std-hello 冒烟 | E2 xtensa / E4 真板 |
| `platform.tier`（[platform]） | `rtos` | 实锤：有 RTOS（FreeRTOS）无主承载层——ADR-0018 修订 1 三档归 rtos（0.0.7 修订版 7 定位订正） | 哨兵真板 D1/E7；0.0.7 方案修订版 7 | E4 真板 |
| `watchdog_present`（[supplies]） | `true` | 存在性成立：ESP32 RTC WDT / idle WDT 硬件在位，esp-idf v5.2.2 构建默认启用 | esp-idf v5.2.2 默认配置（批W1 版本组合登记） | E4 真板 |
| `watchdog_timeout_ms`（[supplies]） | `15000` | **未实测**——平台兜底超时未做真板标定，不冒领 | 触发条件：板上 WDT 行为标定（后续真板批） | 待 E4 |
| `watchdog_feeder`（[supplies]） | `"rr-core"` | rr-core::runtime 运行环喂：心跳拍点即喂点，`RR3G feed` 守护行真板全程在产 | D1 板上 8 拍 + E7 e2e 心跳恢复段 | E4 真板 |
| `tick_period_ms`（[supplies]） | `200` | 哨兵 HEARTBEAT_MS=200（manifest [health].heartbeat_ms 同值）；G4 真板实测 200 拍 avg=200ms（min=198/max=299，首拍 299ms 为注册对齐点） | D1 harness 判据回执；[G4 时序证据](G4时序证据-esp32-mini-20261011.md)（C12 四段落仓） | E4 真板（Darwin 21.6.0 x86_64） |
| `tick_jitter_ms`（[supplies]） | `100` | 预算位；G4 真板实测抖动 ≤6ms（200 拍除首拍外落 [198,206]ms，直方图 [190-199)=43/[200-209)=156），远低于声明预算 100ms | D1 实测；G4 直方图复核 | E4 真板（Darwin 21.6.0 x86_64） |
| `safety_core_interval_ms`（[supplies]） | `1000` | 预算位；功能面已证（⑤ 列逐拍对拍两腿汇合），周期量未单独实测 | 批G ⑤ 列；触发条件：板上反射环周期任务实装（0.0.8+） | 待 E4 |

## 判据对账（模板 §判据逐条）

- 逐键不一致即红：八键声明值与 esp32-mini.toml 原文逐字一致（照抄自描述符）。
- 行缺 = 谎报：八键全在。
- 实测列只有 E4 真板能填：`watchdog_timeout_ms` 与 `safety_core_interval_ms`
  两键如实记"未实测 + 触发条件"，其余六键有 D1/E7/G4 真板或编译面证据
  （G4 后 `tick_period_ms`/`tick_jitter_ms` 升级为真板 200 拍实测）。
- 证据三样（环境指纹/采样窗口/原始数据）归 ⑦ 列 C12 证据文件
  [G4时序证据-esp32-mini-20261011.md](G4时序证据-esp32-mini-20261011.md)，
  本表不重复承载。
