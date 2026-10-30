"""U3 命令面 canonical digest 抓取（树莓派 PL011 直连 classic 板）。

用法: python3 u3_capture.py <输出文件路径>

串行发 10 条代表命令（HELP/OLED/4×FACE/2×SERVO 轴/SCAN），逐条抓回包并算
「回包行集（排序后去空行）」的 sha256，用于跨次/跨档自等对拍。缺省 2M 档，
改档用 env `RR3_PROBE_BAUD`（如 B921600），换口用 `RR3_PROBE_TTY`。
"""
import hashlib
import os
import select
import sys
import termios
import time

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
def baud_of(name):
    """档名 → speed_t：Linux 取 termios 常量；macOS 的 termios 无 921600+ 常量，直接用数值。"""
    if hasattr(termios, name):
        return getattr(termios, name)
    if name.startswith("B") and name[1:].isdigit():
        return int(name[1:])
    raise SystemExit("未知波特率档名: " + name)


BAUD = baud_of(os.environ.get("RR3_PROBE_BAUD", "B2000000"))

OUT = sys.argv[1]
CMDS = ["HELP", "OLED U3-A", "FACE happy", "FACE sad", "FACE idle",
        "PAN 90", "TILT 90", "PAN 60", "TILT 100", "SCAN"]

fd = os.open(DEV, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
attrs = termios.tcgetattr(fd)
attrs[0] = 0; attrs[1] = 0; attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
attrs[3] = 0
attrs[4] = BAUD; attrs[5] = BAUD
attrs[6][termios.VMIN] = 0; attrs[6][termios.VTIME] = 0
termios.tcsetattr(fd, termios.TCSANOW, attrs)
termios.tcflush(fd, termios.TCIOFLUSH)

def pump(sec):
    out = b""
    t0 = time.time()
    while time.time() - t0 < sec:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            d = os.read(fd, 4096)
            if d: out += d
    return out

pump(1.0)  # 吸收启动段 READY/横幅
log = open(OUT, "w")
digests = []
for c in CMDS:
    os.write(fd, (c + "\n").encode())
    raw = pump(13.0 if c == "SCAN" else 1.2)
    text = raw.decode(errors="replace")
    log.write(f">>> {c}\n{text}")
    lines = sorted(l.strip() for l in text.splitlines() if l.strip())
    d = hashlib.sha256("\n".join(lines).encode()).hexdigest()
    digests.append((c, d))
    print(f"{c:14s} {d[:16]}… ({len(lines)} 行)")
log.close()
os.close(fd)
print("saved:", OUT)
