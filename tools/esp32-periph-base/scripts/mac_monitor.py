#!/usr/bin/env python3
"""mac_monitor.py —— Mac 实时数据 → ESP32 OLED 三行显示（批B 增量批演示件）。

用法: python3 scripts/mac_monitor.py /dev/cu.usbserial-XXXX
依赖: python3 + pyserial（pip3 install pyserial）+ osx-cpu-temp（brew install osx-cpu-temp）

接线（CP2102 ↔ ESP32 UART2，3.3V TTL 严禁 5V）：CP2102 RXD←GPIO17 / TXD→GPIO16 / GND 共地。

屏面布局（固件 oled_show 21 列硬切 3 行 + row3 角度行固件自管）：
  row0: CPU <温度> MEM <占用%>
  row1: IP <地址>
  row2: <HH:MM:SS>
  row3: (固件) P:xxx T:xxx

开口帧同步：open 后先发一个空行冲掉 boot 期 RX 线毛刺拼成的半行（探针实锤：
首行指令会被噪声前缀污染回 ERR unknown cmd，空行让固件丢弃残行后再发正式命令）。
"""
import subprocess
import sys
import time

import serial

COLS = 21


def sh(cmd: str) -> str:
    try:
        out = subprocess.run(
            cmd, shell=True, capture_output=True, text=True, timeout=3
        ).stdout.strip()
        return out
    except Exception:
        return ""


def cpu_temp() -> str:
    out = sh("osx-cpu-temp")  # 形如 "69.8°C"
    if out:
        return out.replace("°C", "C").split()[0]
    return "n/a"


def mem_used_pct() -> str:
    out = sh("memory_pressure -Q")  # "System-wide memory free percentage: 45%"
    for token in out.split():
        if token.endswith("%"):
            try:
                return str(100 - int(token[:-1]))
            except ValueError:
                pass
    return "n/a"


def ip_addr() -> str:
    for iface in ("en0", "en1"):
        ip = sh(f"ipconfig getifaddr {iface}")
        if ip:
            return ip
    return "--"


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(f"用法: {sys.argv[0]} /dev/cu.usbserial-XXXX")
    port = sys.argv[1]
    s = serial.Serial(port, 921600, timeout=1)
    time.sleep(0.3)
    s.reset_input_buffer()
    s.write(b"\n")  # 开口帧同步：冲掉 boot 毛刺残行
    time.sleep(0.2)
    s.reset_input_buffer()
    print(f"[mac_monitor] {port} @921600, 1Hz 刷新, Ctrl+C 退出")
    while True:
        line0 = f"CPU {cpu_temp()} MEM {mem_used_pct()}%"
        line1 = f"IP {ip_addr()}"
        line2 = time.strftime("%H:%M:%S")
        payload = (
            (line0 + " " * COLS)[:COLS]
            + (line1 + " " * COLS)[:COLS]
            + (line2 + " " * COLS)[:COLS]
        )
        s.write(f"OLED {payload}\n".encode())
        try:
            reply = s.readline().decode("utf-8", "replace").strip()
        except Exception:
            reply = ""
        print(f"{line0} | {line1} | {line2}  -> {reply}")
        time.sleep(1)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n[mac_monitor] 退出")
