# firmware-rs — RockRobot3 Rust 固件（esp-idf-svc 路线）

C 固件（`../firmware-c`）保持可用的同时，用 Rust 全量重写的平行工程。
跑在 ESP-IDF 之上（std），保住经典蓝牙 SPP 能力（esp-hal no_std 路线做不到，已否）。

**迁移路线**：阶段1 blink ✅ → 阶段2 WiFi+HTTPS ✅ → 阶段3 panel → 阶段4 audio+语音链路 → 阶段5 BT SPP

当前版本：**阶段 2 已完成并烧录实测通过**（WiFi 连接 + TLS 证书校验 + HTTPS 200）。
> 设备日常仍烧 C 固件（语音可用），Rust 固件按阶段推进、验证后切回。

---

## 一、工具链安装（Intel Mac，macOS x86_64）

新版 espup（1.91 起）**不再提供 x86_64-apple-darwin 资产**，官方安装器会失败。
手动安装最后一个可用版本 **Rust 1.90.0.0（esp 工具链，内含 Xtensa esp-clang）**：

```bash
# 1) esp Rust 工具链（rust-build 最后一个带 darwin x86_64 的版本）
curl -L -o /tmp/esp-rust.tar.xz \
  "https://ghfast.top/https://github.com/esp-rs/rust-build/releases/download/v1.90.0.0/rust-1.90.0.0-x86_64-apple-darwin.tar.xz"
mkdir -p /tmp/esprust && tar -xf /tmp/esp-rust.tar.xz -C /tmp/esprust
cd /tmp/esprust/rust-1.90.0.0-x86_64-apple-darwin && ./install.sh
# 装完 rustup toolchain list 应出现名为 esp 的工具链

# 2) ldproxy（esp-idf-svc 的链接器包装）
cargo install ldproxy

# 3) 环境脚本（esp-clang 在工具链内部，bindgen 编译绑定必需）
cat > ~/export-esp.sh <<'EOF'
# Xtensa Rust 工具链环境（Rust 1.90.0.0 + esp-clang，手动安装版）
# 用法：source ~/export-esp.sh
export PATH="$HOME/.rustup/toolchains/esp/xtensa-esp32-elf-clang/bin:$PATH"
export LIBCLANG_PATH="$HOME/.rustup/toolchains/esp/xtensa-esp32-elf-clang/lib"
export CLANG_PATH="$HOME/.rustup/toolchains/esp/xtensa-esp32-elf-clang/bin/clang"
EOF
```

## 二、ESP-IDF 预克隆（国内网络）

embuild 支持 GitHub 直连克隆，但国内网络不通且 ghfast.top 不支持 `git clone`。
用 **Gitee 镜像**预克隆到本地，然后在工程里用 `ESP_IDF` 环境变量指向它：

```bash
git clone -b v5.5.5 --recursive --depth 1 \
  https://gitee.com/EspressifSystems/esp-idf.git ~/.espressif/esp-idf/v5.5.5
```

- 选 **v5.5.5**：embuild/esp-idf-svc 0.53 官方支持矩阵内的最新版（v6.1 太新，绑定不兼容）
- 与 C 固件用的 IDF v6.1 完全独立，互不影响

## 三、工程文件清单（可整目录复制）

```
firmware-rs/
├── Cargo.toml            # 依赖与 profile
├── build.rs              # 3 行，固定写法
├── rust-toolchain.toml   # 固定用 esp 工具链
├── .cargo/config.toml    # 目标三元组 + ldproxy + 指向本地 ESP-IDF
├── partitions.csv        # 大分区表（Rust 镜像 ~1.2MB，默认 1MB 装不下）
├── sdkconfig.defaults    # 分区表配置（注意绝对路径坑）
├── flash.sh              # 烧录脚本（含 elf2image 关键步骤）
└── src/main.rs           # 阶段2：WiFi + HTTPS
```

**Cargo.toml**

```toml
[package]
name = "firmware-rs"
version = "0.1.0"
edition = "2021"
description = "RockRobot3 Rust 固件（esp-idf-svc 路线，阶段2：WiFi+HTTPS）"
rust-version = "1.82"

[[bin]]
name = "firmware-rs"
harness = false

[profile.release]
opt-level = "s"

[profile.dev]
debug = true
opt-level = "z"

[dependencies]
log = "0.4"
anyhow = "1"
esp-idf-svc = { version = "0.53", features = ["critical-section"] }
embedded-svc = { version = "0.29", features = ["std"] }

[build-dependencies]
embuild = "0.33.5"
```

**build.rs**

```rust
fn main() {
    embuild::espidf::sysenv::output();
}
```

**rust-toolchain.toml**

```toml
[toolchain]
channel = "esp"
```

**.cargo/config.toml**

```toml
[build]
target = "xtensa-esp32-espidf"

[target.'cfg(target_os = "espidf")']
linker = "ldproxy"
rustflags = ["--cfg", "espidf_time64"]

[unstable]
build-std = ["std", "panic_abort"]

[env]
MCU = "esp32"
# 用 Gitee 镜像预克隆好的本地 ESP-IDF v5.5.5（GitHub 直连和 ghfast.top 都不支持 git clone）
ESP_IDF = "/Users/qianmi/.espressif/esp-idf/v5.5.5"
# 工具装进 ~/.espressif（与 C 固件 v6.1 工具共存），避免 target 目录清理后重复下载
ESP_IDF_TOOLS_INSTALL_DIR = "global"
# 自定义分区表/配置（factory 1984KB，Rust 镜像 ~1.2MB 需要大分区）
ESP_IDF_SDKCONFIG_DEFAULTS = "/Users/qianmi/Documents/trae_projects/my_test/rockrobot3/firmware-rs/sdkconfig.defaults"
```

**partitions.csv**

```
# Name,   Type, SubType, Offset,  Size, Flags
nvs,      data, nvs,     0x9000,  24K,
phy_init, data, phy,     0xf000,  4K,
factory,  app,  factory, 0x10000, 1984K,
```

**sdkconfig.defaults**

```
# Rust 固件 sdkconfig 默认值（2MB flash）
# 注意：embuild 的 cmake 在 build/out 里跑，相对路径会找不到，必须写绝对路径
CONFIG_PARTITION_TABLE_CUSTOM=y
CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="/Users/qianmi/Documents/trae_projects/my_test/rockrobot3/firmware-rs/partitions.csv"
```

**src/main.rs（阶段2）**

```rust
//! RockRobot3 Rust 阶段2：WiFi 连接 + HTTPS 云链路验证
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::http::client::{Configuration as HttpConf, EspHttpConnection};
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::link_patches;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration as WifiConf, EspWifi};
use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use log::info;

const SSID: &str = "WXYYBG";
const PASSWORD: &str = "12345678";

fn main() -> anyhow::Result<()> {
    link_patches();                    // esp-idf-sys 运行时补丁，必须最先调用
    EspLogger::initialize_default();   // log 绑定到 ESP-IDF 日志系统

    println!("=== RockRobot3 Rust 阶段2：WiFi + HTTPS 云链路 ===");

    let peripherals = Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // ---- WiFi 连接（阻塞等待拿到 IP） ----
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sysloop.clone(), Some(nvs))?,
        sysloop,
    )?;
    wifi.set_configuration(&WifiConf::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: PASSWORD.try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    let ip = wifi.wifi().sta_netif().get_ip_info()?;
    println!("WiFi 已连接：{:?}", ip);

    // ---- HTTPS GET 验证云链路（证书走 ESP-IDF 内置 bundle） ----
    let http_conf = HttpConf {
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        ..Default::default()
    };
    let conn = EspHttpConnection::new(&http_conf)?;
    let mut client = Client::wrap(conn);
    let mut request = client.request(Method::Get, "https://www.baidu.com", &[
        ("User-Agent", "RockRobot3-Rust/0.2"),
    ])?;
    let mut response = request.submit()?;
    println!("HTTP 状态码: {}", response.status());
    let mut buf = [0u8; 256];
    let mut total = 0usize;
    loop {
        let n = response.read(&mut buf)?;
        if n == 0 { break; }
        total += n;
    }
    println!("HTTPS GET https://www.baidu.com → 收到 {} 字节（云链路通）", total);

    // ---- 收尾：LED 常亮表示在线 ----
    let mut led = PinDriver::output(peripherals.pins.gpio2)?;
    led.set_high()?;
    let mut n: u32 = 0;
    loop {
        info!("阶段2 验证完成，运行中 {}", n);
        FreeRtos::delay_ms(5000);
        n += 1;
    }
}
```

> 注：阶段1 的 main.rs 只是去掉 WiFi/HTTP 部分、保留 blink 循环，见 git 历史。

## 四、构建 / 烧录 / 验证

```bash
cd legacy/firmware-rs

# 1) 构建（必须先 source 工具链环境，否则 bindgen 用错 esp-clang 报 xtensa 三元组错误）
source ~/export-esp.sh
cargo +esp build --release

# 2) 烧录（elf2image + 分区表，脚本内已封装）
./flash.sh                       # 默认 /dev/tty.usbserial-1410

# 3) 串口验证（115200，退出 Ctrl+]）
~/.espressif/python_env/idf6.1_py3.12_env/bin/miniterm /dev/tty.usbserial-1410 115200
```

成功标志（串口输出）：

```
=== RockRobot3 Rust 阶段2：WiFi + HTTPS 云链路 ===
WiFi 已连接：IpInfo { ip: 192.168.0.154, ... }
HTTP 状态码: 200
HTTPS GET https://www.baidu.com → 收到 29506 字节（云链路通）
```

## 五、踩坑清单（每一条都真实踩过）

| # | 坑 | 根源 | 解法 |
|---|-----|------|------|
| 1 | espup 官方安装失败 | 1.91 起 rust-build 不再发布 x86_64-apple-darwin | 手动装最后可用版 rust-build v1.90.0.0 |
| 2 | `bindgen: unknown target triple 'xtensa'` | embuild 往 PATH 塞了 IDF 工具里的旧 esp-clang | 先 `source ~/export-esp.sh`（LIBCLANG_PATH 指向工具链内 20.1.1） |
| 3 | 烧 libespidf.bin 后 app_main 秒返回、无 Rust 输出 | 它只是 CMake 桩 app，不含 Rust 代码 | 用 `esptool elf2image` 把 cargo 链接产物 `firmware-rs`（ELF）转 app 镜像烧 0x10000 |
| 4 | `Image length 1151504 doesn't fit in partition length 1048576` | Rust+std 镜像 ~1.2MB，超默认 1MB factory | partitions.csv（factory 1984KB）+ sdkconfig.defaults |
| 5 | 构建报 `No such file or directory: .../out/partitions.csv` | embuild 的 cmake 在 build/out 目录里跑，相对路径解析不到 | sdkconfig 里分区表文件名写**绝对路径** |
| 6 | 921600 烧录偶发 `Unable to verify flash chip connection` | 板子供电/杜邦线在高速率下不稳 | 降到 460800 |
| 7 | GitHub 下载/克隆不通 | 网络 | 下载走 ghfast.top 前缀；git clone 用 Gitee 镜像 |
| 8 | `esp_idf_svc::http::client::EspHttp` 不存在 | 0.53 API 变化 | 用 `EspHttpConnection` + `embedded_svc::http::client::Client::wrap()` |

## 六、与 C 固件的关系

- 两套固件共用一块 2MB flash，**同一时间只能跑一套**（各自带 bootloader/分区表，切换只需整片重烧）
- C 固件切回：`cd ../firmware-c && idf.py -p /dev/cu.usbserial-1410 flash`
- 瑞芯微迁移时：本工程的「对话流程/云端协议」逻辑将平移到 Linux 主控，官方 DeepSeek API 的 TLS 指纹拦截问题在 Linux 上不存在
