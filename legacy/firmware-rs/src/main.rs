//! RockRobot3 Rust 阶段2：WiFi 连接 + HTTPS 云链路验证
//! 阶段1（blink）已验证工具链；本阶段验证「连网 → TLS 证书校验 → 云端 HTTP」，
//! 为后续 asr/llm/tts 云协议平移铺路。
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
    // esp-idf-sys 运行时补丁（必须最先调用）
    link_patches();
    // 绑定 log 到 ESP-IDF 日志系统
    EspLogger::initialize_default();

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
        if n == 0 {
            break;
        }
        total += n;
    }
    println!("HTTPS GET https://www.baidu.com → 收到 {} 字节（云链路通）", total);

    // ---- 收尾：LED 常亮表示在线，随后循环证明系统稳定 ----
    let mut led = PinDriver::output(peripherals.pins.gpio2)?;
    led.set_high()?;
    let mut n: u32 = 0;
    loop {
        info!("阶段2 验证完成，运行中 {}", n);
        FreeRtos::delay_ms(5000);
        n += 1;
    }
}
