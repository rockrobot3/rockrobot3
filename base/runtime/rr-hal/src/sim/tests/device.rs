// SPDX-License-Identifier: Apache-2.0
//! sim 外设面：舵机/数字量/显示/电池/链路模拟件的读回语义与分工边界。

use crate::backend::RR3_HAL_SIM_LINK_SSID_DEFAULT;
use crate::sim::*;
use crate::{Rr3Display, Rr3GpioOut, Rr3Link, Rr3Power, Rr3PwmServo};

/// 扫描列表（N-9 迁入）：首行 SSID 与 sim 链路缺省 SSID 同一真值
#[test]
fn scan_ties_link_def() {
    let list = rr3_sim_scan_list();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].0, RR3_HAL_SIM_LINK_SSID_DEFAULT);
    assert_eq!(list[0].1, -42);
    assert!(list[0].2);
}

/// sim 舵机：初始角即构造值，set 后读回最后一次的值——设备面只记录不裁决，
/// 越界裁决属模块侧（angle_limit 在模块 config），这条用例钉住分工边界。
#[test]
fn sim_last_set_judging() {
    let mut s = Rr3SimServo::rr3_sim_servo_new(90);
    assert_eq!(s.rr3_sim_servo_angle(), 90, "初始角 = 中位 90°");
    assert_eq!(
        s.set_angle(135).unwrap(),
        (),
        "set_angle 如实接受任何 u16——裁决不在这里"
    );
    assert_eq!(s.rr3_sim_servo_angle(), 135);
    s.set_angle(0).unwrap();
    s.set_angle(200).unwrap();
    assert_eq!(s.angle().unwrap(), 200, "读回最后一次 set 的值");
}

/// sim 数字输出：电平写读回环 + 覆写。LED/继电器各持一路，行为同一件。
#[test]
fn sim_gpio_overwrite() {
    let mut g = Rr3SimGpioOut::rr3_sim_gpio_new(false);
    assert!(!g.rr3_sim_gpio_level(), "初始断电态");
    assert!(!g.level().unwrap());
    g.set(true).unwrap();
    assert!(g.rr3_sim_gpio_level());
    g.set(false).unwrap();
    assert!(!g.level().unwrap(), "覆写生效");
}

/// sim 显示件：整帧替换 + 读回一致。draw 全帧替换无局部改写。
/// （批I F1 起 trait 去 `Send + Sync`、写面 `&mut self`——单所有者独占写，
/// 旧"跨线程写读"用例随线程面拆除，共享写归承载层的互斥面。）
#[test]
fn sim_draws_reads_back() {
    let mut d = Rr3SimDisplay::rr3_sim_display_new([0; 8]);
    assert_eq!(d.rr3_sim_frame(), [0; 8], "构造初始黑屏");
    d.draw(&[0b1111_1111, 0, 0, 0, 0, 0, 0, 0b1010_1010])
        .unwrap();
    assert_eq!(
        d.rr3_sim_frame(),
        [0b1111_1111, 0, 0, 0, 0, 0, 0, 0b1010_1010],
        "draw 即整帧替换"
    );
    d.draw(&[1; 8]).unwrap();
    assert_eq!(d.rr3_sim_frame(), [1; 8], "覆写读回一致");
}

/// sim 电池：trait battery 读回构造/覆写后的三元组，电压曲线按
/// 钉死的线性映射逐点核对——"设备只报数、策略不在这里"的分工同舵机那条。
#[test]
fn sim_batt_reads_volt() {
    let b = Rr3SimBattery::rr3_sim_battery_new(80, false);
    let st = b.battery().unwrap();
    assert_eq!(
        (st.percent, st.charging, st.voltage_mv),
        (80, false, 4000),
        "初始 80% 未充电，电压 = 3200 + 80×10"
    );
    b.rr3_sim_battery_set(5, true);
    let st = b.battery().unwrap();
    assert_eq!(
        (st.percent, st.charging, st.voltage_mv),
        (5, true, 3250),
        "覆写后读回同一真值"
    );
    // 特性曲线两端钉死：e2e 的低电断言（5% → 3250 ≤ low_voltage_mv=3300）靠它成立
    assert_eq!(Rr3SimBattery::rr3_sim_batt_mv(0), 3200);
    assert_eq!(Rr3SimBattery::rr3_sim_batt_mv(100), 4200);
}

/// sim 链路：trait state 读回构造/覆写后的三元组；断链时
/// ssid/rssi 强制 None——设备面如实表现，模块侧不许从死链里读出网络信息。
#[test]
fn sim_up_them_down() {
    let l = Rr3SimLink::rr3_sim_link_new(true, "rock-sim", -50);
    let st = l.state().unwrap();
    let ssid = st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str());
    assert_eq!(
        (st.up, ssid, st.rssi_dbm),
        (true, Some("rock-sim"), Some(-50)),
        "初始在线，三元组原样读回"
    );
    l.rr3_sim_link_set(false, "rock-sim", -90);
    let st = l.state().unwrap();
    assert_eq!(
        (
            st.up,
            st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
            st.rssi_dbm
        ),
        (false, None, None),
        "断链后 ssid/rssi 必须被设备面抹掉"
    );
    l.rr3_sim_link_set(true, "other-net", -40);
    let st = l.state().unwrap();
    assert_eq!(
        (
            st.up,
            st.ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
            st.rssi_dbm
        ),
        (true, Some("other-net"), Some(-40)),
        "覆写恢复在线后读回新真值"
    );
}
