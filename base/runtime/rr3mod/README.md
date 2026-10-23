# rr3mod —— 模块公共骨架

0.0.8 W-A 批0 落地（[方案](../../../docs/process/dev/0.0.8版本洛克三号模块内部分层实施方案.md)
§2 签名级规格；[ADR-0017](../../../docs/process/adr/ADR-0017-模块内部分层-机制与策略分离.md)
裁定 1）。五模块（批1–批5 收敛）与 brain 迁移件（ADR-0016）共用的**同构启动序列编排**：
bootstrap（读 manifest → 连总线 → 读 config → 解析）+ serve（死亡探针 → 分帧 →
RPC/订阅分发 → 节拍）+ Dispatcher（模块机制层注册面）。

**只编排不重写**：全部语义真值在既有 crate——连接/注册/心跳/发布/应答在
`rr-bus-api`（承载契约面），总线常量与帧组装在 `rr-core`，帧编解码在 `rr-ipc`；
本 crate 不给任何既有语义立第二份实现。

| 公开面 | 职责 |
|---|---|
| `RR3_MANIFEST_ENV` | rrd spawn 注入的 manifest 绝对路径契约环境变量（自五模块本地常量收编单点） |
| `Rr3ModConfig<C>` | 装配参数：config 覆盖 env 名 + config 文本解析函数（schema 解析与文案留模块侧） |
| `bootstrap` | 收敛序列前四段，返 `(BusHandle, C)`；打印"总线就绪"一行 |
| `load_config_text` | config 路径解析单点：env 覆盖优先，缺省 manifest 同目录 config.toml |
| `BusHandle` | 总线薄封装（publish / call / counter_bump / connected），Clone = 同一连接 Arc 句柄 |
| `Dispatcher` | 模块机制层注册面：on_rpc（业务性失败折 error）/ on_event、on_tick（传输性失败上抛致命） |
| `serve` | 主循环骨架：死亡探针 → 分帧 → RPC/订阅分发 → 节拍；`poll_wait` 作实参（五模块现值不同，不进公共配置） |

四条边界（方案 §2.4/§2.2 原义）：

- **HAL 选择 + 探活不进 bootstrap**——后端选择函数各模块不同，归各模块 boot.rs
  （接入层），`Unsupported` 即 `Err` → `exit(1)`；
- **SURFACE 启动自述不进 bootstrap**——自述留 main（装配面，ADR-0017 裁定 3）；
- **刻意不提供 subscribe**——线上无订阅注册帧（全量扇出 + 本地过滤），给"注册"
  形状即假 API；订阅过滤是 `Dispatcher::on_event` 的本地职责；
- **不引新锁、不新增线程**——跨线程物只有 `BusHandle`（Clone）；`Dispatcher` 只在
  主线程被 `serve` 以 `&mut` 回调，不要求 Send/Sync。

`#[cfg(unix)]` 门与 rr-bus-api 同形状：承载是平台事，UDS 承载只对 unix 目标在册；
非 unix 目标本 crate 只余 `RR3_MANIFEST_ENV` 常量。

**六消费方同批改**（方案 §5 批0 附加纪律）：本 crate 落地即受五模块 + brain 迁移件
约束，API 修订须六消费方同批改，不许骨架单方漂移。依赖单向：rr3mod →
rr-bus-api / rr-ipc / rr-core（反向零依赖由批0 验收的 `cargo tree` 断言把门）。
