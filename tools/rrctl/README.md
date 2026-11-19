# tools/rrctl —— `rr3-ctl` 总线侧运维 CLI

八个**查询/控制**子命令经 UDS **真连 rrd 的事务面**（F3 / [ADR-0010](../../docs/process/adr/ADR-0010-控制通道与查询面.md)；
0.0.4 补 `counters`/`reboot`，续轮十五补 `wiring`/`contracts`——只读渲染；
CLI 与 ctl 白名单逐一同名对齐，`domain.set` 写口按安全链评审单列）：
查在册模块、看健康状态、取日志、读运维计数、读装配快照与契约审计；OTA 恒拒随 M6
（[ADR-0007](../../docs/process/adr/ADR-0007-控制面分层与模块级升级.md) 决策二的
模块级更新轨道）；reboot 由 rrd 执行面双重门把守（cfg(linux) 且 env
`RR3_REBOOT_OK=1`，缺省恒拒），**CLI 不加本地门**——rrd 的双重门是唯一真值点。
第九个子命令 `tail` 语义不同：注册为在册观察者，持续收 Event（见下）。

    rr3-ctl [--bus-path <path>] <status|health|logs|ota|counters|reboot|wiring|contracts>
    rr3-ctl [--bus-path <path>] tail <topic> [--count N] [--secs S]

套接字路径优先级：`--bus-path` > 环境变量 `RR3_BUS_PATH` > 契约默认值
`/var/run/rock3-ipc.sock`。默认值与环境变量名都从 `rr-ipc` 引入，不在本文件重抄，
所以契约改动时 CLI 不会留在旧值上。

连接语义（ADR-0010 裁定 2/3）：八个查询子命令各是一次**一次性控制连接**——同一套接字、
同一 ADR-0006 线格式，但**不注册、不进在册表、不受 watchdog 管**，一问一答即断。
`status` 列在册模块（NAME/TASK/PHASE + 拒发布与广播丢帧计数 + 落盘状态
`log_sink` + 装配摘要 `boot`）、`health` 加心跳与
冷启动列（间隔 / 看门狗超时 / 距上次心跳 / boot_ms / 慢消费者丢帧）、`logs` 取 rrd
内存日志环（512 行；落盘与重启回填见 rrd-cli.md）、`counters` 读运维计数全表
（health 回答"活没活"，counters 回答"机器慢先看数字"：`模块名/键` 按模块前缀分组
成 MODULE/KEY/VALUE 表，rrd 自有键如 `boots` 归 `rrd` 组，空表打"无计数"）、
`wiring` 出装配快照六段（引脚 / 供电域 / 接线 / 轴 / 角色 / 交叉判据，空段打
"（无）"，`available:false` 如实报不可得）、`contracts` 出四册账 / 真值集账 / 判果
（sha256 全量供对账）、`ota` 与 `reboot` 由 rrd 回错误（文案分别点 M6 与双重门），
CLI 回显文案并以非 0 退出——reboot 的解锁面在装机环境（rrd.service 注入
`RR3_REBOOT_OK=1`）。

独立工程：自带空 `[workspace]`，**不在** `base/runtime` 的 workspace 内（与 rr3-mkmod 同构）。
路径依赖 `rr-bus` + `rr-bus-api` + `rr-ipc` + `serde_json`，命令面受同一套契约类型约束。
`tail` 是唯一的**注册型**子命令：rrd 只向在册连接广播 Event，一次性控制连接收不到
业务帧，所以它经 `rr-bus-api` 的 `rr3_connect_at`（显式端点入口）注册为观察者模块
`rr3-tail`，manifest 里声明 `events.subscribes = [topic]`（主题必须已登记，拼错在
注册门被拒）。同机同一时刻只允许一个 tail（`module_name` 规则不含数字），重复时
rrd 回 Busy 并原样点名。输出为 stdout 上的 JSONL：
`{"ts_ms":<转发时刻>,"src_task":<发布者>,"body":{...}}`，`body` 是线上原样事件体；
DoA 标定直接取 `.body.doa.deg`：

    rr3-ctl tail rr3-sense/audio/vad_start --count 30 --secs 60 | jq -c '.body.doa'

收满 `--count` 退出 0；超时 / 断链 / 注册失败退出 1（stderr 带已收条数）。

    cargo run -q --manifest-path tools/rrctl/Cargo.toml --bin rr3-ctl -- status
    cargo test --manifest-path tools/rrctl/Cargo.toml

编译由 `tools/ci/check.sh` 第 ④ 步覆盖（独立 workspace 不在 ①/①b 的作用域内）；
单测含真 UDS 往返用例（fake rrd：status/counters/wiring 实答渲染、ota/reboot 恒拒
回显），**不在门禁步序内**，收口时按 conventions §2 备注本地跑。
