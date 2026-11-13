//! ports 的单测（自同名单测模块原样抽出，判据不变）。

use std::fs;
use std::io::{Read, Write};
use std::os::unix::io::FromRawFd;

use rr_ipc::{Rr3Frame, Rr3FrameBuf};
use rr_periph_proto::{rr3_bridge_call, rr3_bridge_decode, RR3_SERVO_TOPIC};
use serde_json::json;

use super::test_support::fake;
use super::*;
use crate::board::{ADS1115_ADDR, DUTY_MAX_TICKS, DUTY_MIN_TICKS};

// ----- UART：pty 帧回环（真实 open(path) 入口；帧层与端口共用同一 conn） -----

extern "C" {
    /// pty 对（测试伪串口）。macOS libSystem / glibc ≥2.34 直接提供。
    fn openpty(
        amaster: *mut i32,
        aslave: *mut i32,
        name: *mut i8,
        termp: *const std::ffi::c_void,
        winp: *const std::ffi::c_void,
    ) -> i32;
}

fn pty_pair() -> (fs::File, fs::File, PathBuf) {
    let mut master = 0i32;
    let mut slave = 0i32;
    let mut name = [0i8; 128];
    // SAFETY：三个出参都是本栈合法内存，其余按 NULL 传。
    let rc = unsafe {
        openpty(
            &mut master,
            &mut slave,
            name.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0, "openpty 失败: {}", std::io::Error::last_os_error());
    let len = name
        .iter()
        .position(|&c| c == 0)
        .expect("openpty name 未 NUL 结尾");
    let bytes: Vec<u8> = name[..len].iter().map(|&c| c as u8).collect();
    let path = PathBuf::from(String::from_utf8(bytes).expect("pty 路径非 UTF-8"));
    // SAFETY：master/slave 是 openpty 刚出的合法 fd，各自 File 接管并负责 close。
    unsafe {
        (
            fs::File::from_raw_fd(master),
            fs::File::from_raw_fd(slave),
            path,
        )
    }
}

/// UART 端口 + 帧面真值：宿主帧整帧写入 → 板侧 conn 收帧 → 应答帧整帧回读。
/// 这里同时证明 `Rr3UartConn` 经 rr-ipc blanket 自动满足 [`Rr3UartPort`]。
#[test]
fn uart_frame_roundtrip_via_pty() {
    fn assert_uart_port<T: rr_periph_proto::ports::Rr3UartPort>() {}
    assert_uart_port::<Rr3UartConn>();

    let (mut master, _slave_keepalive, path) = pty_pair();
    let mut conn = Rr3UartConn::open_with_baud(&path, 115_200).expect("pty slave 打开失败");
    let req_frame = rr3_bridge_call(7, RR3_SERVO_TOPIC, &json!({ "angle_deg": 95 }));
    let req_bytes = Rr3FrameBuf::from(&req_frame)
        .rr3_as_sealed_bytes()
        .expect("请求帧须已 seal")
        .to_vec();
    master.write_all(&req_bytes).expect("写宿主帧失败");

    let req = periph_mech::frame::frame_recv(&mut conn).expect("板侧收帧/协议门必须过");
    assert_eq!(req.id, 7);
    assert_eq!(req.topic, RR3_SERVO_TOPIC);
    assert_eq!(req.args["angle_deg"], 95);

    periph_mech::frame::frame_send_response(
        &mut conn,
        &req,
        Ok(json!({ "angle_deg": 95, "move_ms": 0 })),
        &MonotonicClock,
    )
    .expect("应答帧写出失败");
    // 首次写带空闲前导 `\n`（rr-ipc 的 1.5M 线面处置），跳过后读整帧。
    let mut pre = [0u8; 1];
    master.read_exact(&mut pre).expect("读前导失败");
    assert_eq!(pre[0], 0x0A, "空闲后首发必须带 \\n 前导");
    let resp_frame_len = {
        let resp =
            rr_periph_proto::rr3_resp_frame(&req, Ok(json!({ "angle_deg": 95, "move_ms": 0 })), 0);
        rr_ipc::rr3_frame_to_bytes(&resp)
            .expect("应答帧长度可算")
            .len()
    };
    let mut got = vec![0u8; resp_frame_len];
    master.read_exact(&mut got).expect("读应答帧失败");
    let mut cursor = std::io::Cursor::new(got);
    let frame_buf = rr_ipc::rr3_framebuf_read(&mut cursor).expect("应答字节须能进帧缓冲");
    let frame = Rr3Frame::try_from(frame_buf).expect("应答帧还原失败");
    let (ty, body) = rr_ipc::rr3_body_split(&frame.payload).expect("payload 前缀合法");
    let v = rr3_bridge_decode(ty, &body, 7).expect("宿主应答门必须过");
    assert_eq!(v["angle_deg"], 95);
}

/// 帧内断流必须**有界退出**（批B15）：旧版帧面走阻塞 `Read`（VMIN=1、无
/// VTIME），半帧后对端不再给字节就永久停在 128B 头里，控制面只能整机重启。
/// 判据是"拒绝 + 有时间上界"，不是某个具体错误码——任何 `Err` 都让调用方
/// 残尾吞净后复位失步，这正是坏帧可恢复的形状。读取放独立线程 + 主线程
/// 限时收：**回归时测试不会自己挂死**，而是明确报出"没有上界"。
#[test]
fn frame_read_half_frame_times_out() {
    let (mut master, _slave_keepalive, path) = pty_pair();
    let mut conn = Rr3UartConn::open_with_baud(&path, 115_200).expect("pty slave 打开失败");
    master
        .write_all(&[rr_periph_proto::RR3_MAGIC_FIRST, 0x00, 0x00, 0x00])
        .expect("写半帧失败");
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut io = crate::UartIo {
            uart: &mut conn,
            pre: Some(rr_periph_proto::RR3_MAGIC_FIRST),
        };
        let _ = tx.send(periph_mech::frame::frame_recv(&mut io).is_err());
    });
    let rejected = rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .expect("半帧读取没有上界（旧版会永久阻塞）：控制面永久挂死的同一缺陷");
    assert!(rejected, "半帧不得当完整帧放行");
    let _ = worker.join();
}

// ----- I2C / ADS1115 -----
#[test]
fn i2c_port_forwards_write_transaction() {
    let (bus, f) = fake();
    let mut port = bus.clone();
    port.rr3_i2c_write(0x3C, &[0x00, 0xAE])
        .expect("写事务应转发");
    assert_eq!(f.borrow().writes, vec![(0x3C, vec![0x00, 0xAE])]);
}

/// ADS1115：配置字 = 0x8383（A0/±4.096V/单次/128SPS/比较器关）；raw 20960
/// （A0=2.62V，直入模块目标向量）→ 轨压 5240mV（2:1 分压）。
#[test]
fn ads1115_config_and_conversion() {
    let (bus, f) = fake();
    f.borrow_mut().read_data = vec![0x51, 0xE0];
    let mut ads = Ads1115::new(bus, ADS1115_ADDR);
    let (rail_mv, raw) = ads.read_rail().expect("在位件必须读回");
    assert_eq!(raw, 20_960);
    assert_eq!(rail_mv, 5_240);
    {
        let f = f.borrow();
        assert_eq!(f.writes[0].0, ADS1115_ADDR);
        assert_eq!(f.writes[0].1, vec![0x01, 0x83, 0x83]);
        assert_eq!(f.reads, vec![(ADS1115_ADDR, 0x00)]);
    }
}

/// 无器件/坏总线：`rail` 回 Err 且点名 ADS1115，不 panic；错通道点名拒。
#[test]
fn ads1115_absent_reports_err_without_panic() {
    let (bus, f) = fake();
    f.borrow_mut().read_err = Some("NACK".into());
    let mut ads = Ads1115::new(bus, ADS1115_ADDR);
    let e = ads.read_rail().expect_err("无器件必须 Err");
    assert!(e.contains("ADS1115"), "{e}");
    assert!(e.contains("NACK"), "{e}");

    let absent = RailSensor::Absent("总线不可用".into());
    let mut sensor = absent;
    let e = sensor
        .rr3_adc_read_mv(ADS1115_RAIL_CHANNEL)
        .expect_err("缺席必须 Err");
    assert!(e.contains("ADS1115 未装配"), "{e}");
    let e = sensor.rr3_adc_read_mv(34).expect_err("错通道必须拒");
    assert!(e.contains("拒绝通道 34"), "{e}");
}

// ----- PWM -----

fn pwm_fixture() -> (PathBuf, PwmPort) {
    let root = std::env::temp_dir().join(format!(
        "rr3-pi-pwm-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("建 PWM 临时根失败");
    for dir in PWM_CHANNELS {
        let d = root.join(dir);
        fs::create_dir_all(&d).expect("建通道目录失败");
        fs::write(d.join("period"), "0\n").expect("写 period 失败");
        fs::write(d.join("duty_cycle"), "0\n").expect("写 duty 失败");
        fs::write(d.join("enable"), "0\n").expect("写 enable 失败");
    }
    (root.clone(), PwmPort::new(root))
}

/// tick → sysfs 纳秒：1229 tick（90°）= 1,500,244ns；0 = 释放（enable 0）。
#[test]
fn pwm_duty_writes_ns_and_releases_at_zero() {
    let (root, mut pwm) = pwm_fixture();
    assert_eq!(pwm_ticks_to_ns(0), 0);
    assert_eq!(pwm_ticks_to_ns(DUTY_MIN_TICKS), 500_488);
    assert_eq!(pwm_ticks_to_ns(DUTY_MAX_TICKS), 2_500_000);

    pwm.rr3_pwm_duty(Rr3PwmChannel::Pan, 1229)
        .expect("pan 写占空失败");
    let duty = read_u64(&root.join("pwm0").join("duty_cycle")).unwrap_or(0);
    assert_eq!(duty, 1_500_244, "90° 应为 1.500244ms");
    assert_eq!(
        read_u64(&root.join("pwm0").join("period")).unwrap_or(0),
        PWM_PERIOD_NS as u64
    );
    assert_eq!(read_u64(&root.join("pwm0").join("enable")).unwrap_or(0), 1);

    pwm.rr3_pwm_duty(Rr3PwmChannel::Tilt, 0)
        .expect("tilt 释放失败");
    assert_eq!(
        read_u64(&root.join("pwm1").join("duty_cycle")).unwrap_or(1),
        0
    );
    assert_eq!(read_u64(&root.join("pwm1").join("enable")).unwrap_or(1), 0);
    let _ = fs::remove_dir_all(&root);
}

/// 根不在位：export 写失败必须回 Err 并点名（不 panic、不静默）。
#[test]
fn pwm_missing_root_is_err() {
    let mut pwm = PwmPort::new("/nonexistent/rr3-pi-pwmchip");
    let e = pwm
        .rr3_pwm_duty(Rr3PwmChannel::Pan, 1229)
        .expect_err("缺 sysfs 根必须 Err");
    assert!(e.contains("pwm export"), "{e}");
}

// ----- Clock -----

#[test]
fn clock_is_monotonic_nondecreasing() {
    let c = MonotonicClock;
    let a = c.rr3_now_ns();
    std::thread::sleep(Duration::from_millis(5));
    let b = c.rr3_now_ns();
    assert!(b >= a, "单调时钟不得回退: {a} -> {b}");
    assert!(b > a, "5ms 间隔必须可辨: {a} -> {b}");
}
