# base/os/distro —— 发行版快路（ADR-0013 裁定 3）

OS 双轨的**快路**：不自己构建 OS，用现成发行版（Armbian / Debian / 发行版镜像）
直接启动，rock3 以 **payload 层**形态安装上去——rrd、rr3-\* 模块二进制、systemd
单元、配置与 profile，全部按"包"交付，不碰内核与引导链。

与 `../buildroot/`（保底轨，见其 patches/README.md）的关系：

| | distro 快路 | buildroot 保底 |
|---|---|---|
| OS 来源 | 发行版现成镜像 | 全量 make 自建 |
| 自烧录 | 随发行版（SD / USB / 网络启动） | maskrom 不拆机（rkdeveloptool） |
| 定位 | 开发与功能收口先行 | M1 终态：可复现、可出厂 |

**本目录要交出的文件**（有没有落、算哪一档属实况，唯一落点
[`docs/status.md`](../../../docs/status.md)，本文件不记进度）：

- `payload/` —— payload 装配胶水（如需）：打包器实体在
  [`tools/rr3-pack`](../../../tools/rr3-pack)（0.0.2 T5，Rust 独立工程，staging 装配
  纯函数本机可单测、真出包调 dpkg-deb）——把 `base/runtime` 交叉产物（rrd、rr3-ctl、
  `rr3-modules/<name>/{bin,manifest.toml}`）与仓内契约/装配单/systemd 单元打成 .deb
  （OCI image 属后置批次，分发裁定见 ADR-0013）；CI 真装冒烟走 `distro-smoke` job
- `units/` —— systemd 单元（[rrd.service](units/rrd.service) 已落，0.0.2 T5，
  rr3-pack 编译期嵌入 deb）
- `boards/` —— 各发行版在各板卡上的已验证笔记（镜像来源、内核版本、
  ALSA/UART 可用性），一板一文件（如 [boards/raspberry-pi-64.md](boards/raspberry-pi-64.md)——树莓派 64-bit，
  0.0.6 真板验证批）

模块层与 OS 层的边界语义见 [ADR-0013](../../../docs/process/adr/ADR-0013-OS双轨-发行版快路与Buildroot保底.md)；
机型选择（哪些格位、哪个 profile）不落在本层——那是 `profiles/`（ADR-0012 装配单）的活。

## HAL env 键位总表（0.0.5 增补轮四 批W7，A3）

全部 `RR3_HAL_*` 环境变量与缺省值——**每个键与代码 env 名逐字一致**
（常量定义 `base/runtime/rr-hal/src/backend.rs`，符号名锚定；真件档装配单
env 覆写、service 注入、e2e 注入均照此表，不读代码不另猜）。取值不认一律
回落缺省（不报错）；显式 `RR3_HAL_BACKEND=linux` 在 sim-only 构建上会被
误落 placeholder——R11 boot 语义，见 backend.rs `rr3_hal_backend_resolve` 注。

### 后端选择

| 键 | 取值 | 缺省 | 说明 |
|---|---|---|---|
| `RR3_HAL_BACKEND` | `sim` / `linux` / `placeholder` | 平台默认（linux 构建且 `alsa` feature 编入 = `linux`，否则 `sim`） | 后端选择总开关（`RR3_HAL_BACKEND_ENV`）；真件档装配单显式 `=linux`（R11①） |

### linux 档（真件面，`linux*.rs` 后端）

| 键 | 取值 | 缺省 | 说明 |
|---|---|---|---|
| `RR3_HAL_ALSA_DEVICE` | ALSA 设备名（`hw:0,0` / `plughw:1,0`…） | `default` | 采集/回放设备（`RR3_HAL_ALSA_DEVICE_ENV`） |
| `RR3_HAL_PWM_CHIP` | 十进制非负整数 | `0` | sysfs PWM 芯片号（`RR3_HAL_PWM_CHIP_ENV`）；pwmchip0 的 pwm0=GPIO12、pwm1=GPIO13（R22 定案，`dtoverlay=pwm-2chan`） |
| `RR3_HAL_PWM_CHANNEL` | 十进制非负整数 | `0`（=GPIO12） | sysfs PWM 通道号（`RR3_HAL_PWM_CHANNEL_ENV`）；50Hz / 0.5–2.5ms↔0–180°，boot 落中位 90° |
| `RR3_HAL_GPIO_CHIP` | 十进制非负整数 | `0` | chardev GPIO 芯片号（`RR3_HAL_GPIO_CHIP_ENV`），`/dev/gpiochipN` |
| `RR3_HAL_GPIO_LED_LINE` | 十进制非负整数（BCM 编号） | `4` | LED 线偏移（role=`led`，`rr3_gpio_line_default`） |
| `RR3_HAL_GPIO_RELAY_LINE` | 十进制非负整数（BCM 编号） | `17` | 继电器线偏移（role=`relay`，`rr3_gpio_line_default`） |
| `RR3_HAL_I2C_BUS` | 十进制非负整数 | `1` | INA219 总线号（`RR3_HAL_I2C_BUS_ENV`）；i2c1=GPIO2/3（树莓派硬件 I2C） |
| `RR3_HAL_INA219_ADDR` | `0x` 前缀十六进制（7-bit 域 0x08–0x77） | `0x40` | INA219 从地址（`RR3_HAL_INA219_ADDR_ENV`；裸写无 `0x` 不认——十进制歧义拒收） |
| `RR3_HAL_LINK_IFNAME` | 任意非空串 | `wlan0` | NetworkManager 链路读数接口名（`RR3_HAL_LINK_IFNAME_ENV`） |

GPIO 线偏移按 **role 泛化**：`rr3_hal_gpio_out` 的 role 参数拼键
`RR3_HAL_GPIO_<ROLE>_LINE`（`rr3_gpio_line_env_key`，role 只认非空 ASCII
字母数字）；代码缺省只备 `led`→4 / `relay`→17 两档（R8 引脚预算表软件路），
白名单外 role 无缺省——env 也没给时 ensure 显式回错，不静默选脚。

### sim 档（e2e 注入正门，`sim.rs` 后端）

| 键 | 取值 | 缺省 | 说明 |
|---|---|---|---|
| `RR3_HAL_SIM_WAVE` | `sine` / `noise` / `silence` / `burst` | `sine` | 波形选择（`RR3_HAL_SIM_WAVE_ENV`）；`burst`=1s 正弦+1s 静音循环，VAD 周期 onset/offset |
| `RR3_HAL_SIM_DOA_DEG` | -90–90 十进制小数（负=左/正=右） | 未设不注入 | 声源方位角注入（`RR3_HAL_SIM_DOA_DEG_ENV`）；多通道采集链路才生效 |
| `RR3_HAL_SIM_BATTERY_PERCENT` | 0–100 十进制整数 | `80` | sim 电池初始电量（`RR3_HAL_SIM_BATTERY_PERCENT_ENV`） |
| `RR3_HAL_SIM_BATTERY_CHARGING` | `1` / `0` | `0` | sim 电池充电态（`RR3_HAL_SIM_BATTERY_CHARGING_ENV`） |
| `RR3_HAL_SIM_LINK_UP` | `1` / `0` | `1` | sim 链路在线态（`RR3_HAL_SIM_LINK_UP_ENV`）；e2e 断链场景正门 |
| `RR3_HAL_SIM_LINK_SSID` | 任意字符串 | `rock-sim` | sim 链路 SSID（`RR3_HAL_SIM_LINK_SSID_ENV`） |
| `RR3_HAL_SIM_LINK_RSSI` | -100 – -1 十进制整数（dBm） | `-50` | sim 链路信号强度（`RR3_HAL_SIM_LINK_RSSI_ENV`） |

注：I2C 设备节点根为构造常量 `RR3_I2C_DEV_ROOT_DEFAULT = "/dev"`
（`linux_power.rs`，非 env）；显示件 trait 的 linux 后端为占位（MAX7219
SPI 后端归真件档），无 env 键；本表不含非 `RR3_HAL_` 前缀键
（如面板口 `RR3_PANEL_ADDR`、演练口 `RR3_SIM_PANEL_ADDR`——归各自 README）。
