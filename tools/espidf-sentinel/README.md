# espidf-sentinel——0.0.7 批D1 层③ 哨兵（esp-rs std，机制核真板消费者）

目的：全案 §2.1 修订版 9 口径——层③ 哨兵**内跑 `rr-core::runtime`**（`Rr3LinkState`
连接态转移 / `Rr3ModuleRuntime` 心跳拍点 / `rr3_dispatch_class` 收帧分类）+ 窄缝
`Rr3ByteStream` 两方法（esp-idf-hal UART2 半分体），**不另写状态机**。传输下底只做
平台事；帧语义与 std 驱动器（rr-bus client）同构：reader 线程 + 心跳线程 + 一次一答
pending 表。

## 目录

| 路径 | 内容 |
|---|---|
| `src/main.rs` | 哨兵本体（xtensa-esp32-espidf 目标；组帧/发帧/线程/堆采样） |
| `build.rs` / `.cargo/config.toml` / `sdkconfig.defaults` | 批W1 同款构建链（版本对表见下） |

宿主对手件 harness 落 `tools/rr3-sentinel-harness/`（独立宿主 crate，`rr3-sentinel-harness`，
macOS/Linux 直跑扮总线侧；**不落本目录内**——cargo 配置发现沿 CWD 向上拾取，落子目录会
继承本目录 `.cargo/config.toml` 钉死的 xtensa target，宿主构建必炸，批D1 实爆留痕）。

## 版本钉死登记（对表 tools/espidf-std-hello/README.md，升级需重验整链）

esp-idf-sys =0.35.0 ↔ ESP-IDF v5.2.2（`~/.espressif/frameworks/esp-idf-v5.2.2`）↔
esp-idf-hal =0.44.1（与 sys 0.35 同代，UART2 半分体 UartTx/UartRx）↔ embuild =0.32.0
↔ ldproxy 0.3.5 ↔ esp-clang 19.1.2 ↔ rustup 工具链 alias `esp`。

## 构建与烧录（本机）

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export IDF_PATH="$HOME/.espressif/frameworks/esp-idf-v5.2.2"
cargo +esp build    # 双证之①：编译面（rr-core/rr-ipc/哨兵在 xtensa-esp32-espidf 可编译）
cargo +esp run      # 烧录 + monitor（runner=espflash，退出键 Ctrl+]）
```

接线（批C 载体同款硬约束）：UART2 = GPIO17→适配器 RXD / GPIO16←适配器 TXD，115200，
3.3V TTL，共地；UART0 = 控制台（monitor 观测面）。

## 板上 runbook（双证之②，方案 §9.3）

1. **monitor 先行**：`cargo +esp run`。预期启动横幅 + SURFACE 自述行
   `[sentinel] surface=esp-rs-std ...` + `RR3G boot registering` +
   `[D1-RAM] point=1-boot free=…B min_free=…B`。
2. **harness 对跑**（另开终端，宿主直跑）：

   ```sh
   cd tools/rr3-sentinel-harness && cargo run --release -- /dev/tty.usbserial-XXXX [拍数，缺省 8]
   ```

   判据逐条 PASS：① 注册（Request/rr3.register + manifest 在场）→ 回
   `{"src_task": 2}`；② 心跳 N 拍全收全应（间隔 ≈ 200ms，停摆 >5s 即 FAIL）；
   ③ round-trip（下行 rr3.call → Response id 回显 + result.echo 原样回带）。
3. **三点堆采样**（monitor 侧）：`point=1-boot` / `point=2-registered` /
   `point=3-roundtrip` 各一行 `free=…B min_free=…B`；`min_free` 即运行峰值剩余堆
   水位，**< 20% 总堆（经典 ESP32 ≈104KB）或 std 不可起 → 触发批D2（S3）**
   （§9.3 ④ 数字口径）。
4. 本地守护行：`RR3G boot registering` → `RR3G boot online task=2` →
   `RR3G feed seq=k`（rr-core::runtime H6 行文法的真板消费者证据）。

## 实测报告（板上跑完回填本节）

| 点 | free | min_free（峰值剩余） | 判定（<104KB？） |
|---|---|---|---|
| 1-boot | — | — | — |
| 2-registered | — | — | — |
| 3-roundtrip | — | — | — |

## 门禁

本工程被 `tools/ci/check.sh` ④/⑥ 显式排除（xtensa 工具链宿主必红），由
`tools/ci/espidf-sentinel-check.sh` 单独覆盖（甲面 xtensa 构建 = 双证之①），SKIP
纪律同 espidf-std-check.sh。伴生对手件 harness 是普通宿主 crate
（tools/rr3-sentinel-harness），由主门禁 ④/⑥ 与 CI lint 全覆盖，不入专用门。
