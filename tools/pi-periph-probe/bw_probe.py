"""UART 带宽/误码连续性探针（树莓派 PL011 直连 classic 板）。

用法: python3 bw_probe.py [termios 常量名，缺省 B1500000] [音频秒数，缺省 60]
      换口用 env `RR3_PROBE_TTY`。

测三件事：
1. READY 横幅里的 baud 值（板固件当前档，对表用）
2. 控制面突发 120 条 PAN 的回包完整性（命令面吞吐/丢包）
3. AUDIO ON 连续音频流的帧完整性（头失配 / 撕裂帧 / seq 断档 / PCM 长度）
"""
import base64
import os
import re
import select
import sys
import termios
import time

def baud_of(name):
    """档名 → speed_t：Linux 取 termios 常量；macOS 的 termios 无 921600+ 常量，直接用数值。"""
    if hasattr(termios, name):
        return getattr(termios, name)
    if name.startswith("B") and name[1:].isdigit():
        return int(name[1:])
    raise SystemExit("未知波特率档名: " + name)


BAUD = baud_of(sys.argv[1] if len(sys.argv) > 1 else os.environ.get("RR3_PROBE_BAUD", "B1500000"))
SECS = int(sys.argv[2]) if len(sys.argv) > 2 else 60
DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
MAGIC = b"\x31\x45\x52\x41"  # magic LE 线上字节 31 45 52 41

fd = os.open(DEV, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
a = termios.tcgetattr(fd)
a[0] = 0
a[1] = 0
a[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
a[3] = 0
a[4] = BAUD
a[5] = BAUD
a[6][termios.VMIN] = 0
a[6][termios.VTIME] = 0
termios.tcsetattr(fd, termios.TCSANOW, a)
termios.tcflush(fd, termios.TCIOFLUSH)


def read_avail(timeout):
    r, _, _ = select.select([fd], [], [], timeout)
    if not r:
        return b""
    try:
        return os.read(fd, 16384)
    except BlockingIOError:
        return b""


def pump(sec):
    out = b""
    t0 = time.time()
    while time.time() - t0 < sec:
        d = read_avail(0.05)
        if d:
            out += d
    return out


ready = pump(1.5)
rline = [l for l in ready.decode(errors="replace").splitlines() if "READY" in l]
print("[1] READY:", rline[0] if rline else "(未见 READY 横幅)")

os.write(fd, b"HELP\n")
h = pump(1.0).decode(errors="replace")
print("[2] HELP:", "OK" if "CMDS" in h else "FAIL " + h[:90])

os.write(fd, b"".join(b"PAN 90\n" for _ in range(120)))
t = pump(3.0).decode(errors="replace")
print("[3] 突发 120xPAN: OK=%d/120 ERR=%d" % (t.count("OK pan=90"), t.count("ERR")))

os.write(fd, b"AUDIO ON\n")
pump(0.8)
buf = bytearray()
t0 = time.time()
while time.time() - t0 < SECS:
    d = read_avail(0.05)
    if d:
        buf += d
el = time.time() - t0
os.write(fd, b"AUDIO OFF\n")
buf += pump(2.0)

n = len(buf)
i = 0
skip = 0
frames = 0
badmagic = 0
torn = 0
seqs = []
lens = []
first = buf.find(MAGIC)
if first < 0:
    print("[4] 音频: 流中找不到帧 magic —— 链路疑似不通")
    os.close(fd)
    sys.exit(1)
i = 0
skip = first
i = first
while i + 128 <= n:
    if buf[i:i + 4] != MAGIC:
        badmagic += 1
        i += 1
        continue
    plen = int.from_bytes(buf[i + 40:i + 44], "little")
    if plen == 0 or plen > 20000:
        torn += 1
        i += 1
        continue
    if i + 128 + plen > n:
        torn += 1
        break
    payload = bytes(buf[i + 128:i + 128 + plen])
    frames += 1
    m = re.search(rb'"seq":(\d+)', payload)
    if m:
        seqs.append(int(m.group(1)))
    b = re.search(rb'"pcm_b64":"([^"]*)"', payload)
    if b:
        s = b.group(1)
        try:
            lens.append(len(base64.b64decode(s + b"=" * ((4 - len(s) % 4) % 4))))
        except Exception:
            lens.append(-1)
    else:
        lens.append(-2)
    i += 128 + plen

gap = sum(1 for x, y in zip(seqs, seqs[1:]) if y != x + 1)
kbs = n / 1024.0 / el
avg = sum(lens) / len(lens) if lens else 0
print("[4] 音频 %.1fs: %d B (%.1f KB/s) 帧=%d 头失配=%d 撕裂=%d 前置丢弃=%dB"
      % (el, n, kbs, frames, badmagic, torn, skip))
print("    seq 断档=%d 首=%s 末=%s；PCM 长度 min/max/均=%.0f/%.0f/%.0f（期望 8192）"
      % (gap, seqs[0] if seqs else "-", seqs[-1] if seqs else "-",
         min(lens) if lens else 0, max(lens) if lens else 0, avg))
os.close(fd)
