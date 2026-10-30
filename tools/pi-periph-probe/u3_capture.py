"""U3 命令面 canonical digest 抓取（树莓派直连 classic 板）。

用法: python3 u3_capture.py <输出文件路径>

串行发 10 条代表命令（HELP/OLED/4×FACE/2×SERVO 轴/SCAN），逐条抓回包并算
「回包行集（排序后去空行）」的 sha256，用于跨次/跨档自等对拍。缺省 1.5M 档，
改档用 env `RR3_PROBE_BAUD`（如 B921600），换口用 `RR3_PROBE_TTY`。
"""
import hashlib
import os
import sys

from probe_common import baud_from_argv_or_env, open_tty, pump

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
OUT = sys.argv[1]
CMDS = ["HELP", "OLED U3-A", "FACE happy", "FACE sad", "FACE idle",
        "PAN 90", "TILT 90", "PAN 60", "TILT 100", "SCAN"]

fd = open_tty(DEV, baud_from_argv_or_env(argv_index=None))  # argv[1] 是输出路径，档位只认 env

pump(fd, 1.0)  # 吸收启动段 READY/横幅
log = open(OUT, "w")
digests = []
for c in CMDS:
    os.write(fd, (c + "\n").encode())
    raw = pump(fd, 13.0 if c == "SCAN" else 1.2)
    text = raw.decode(errors="replace")
    log.write(f">>> {c}\n{text}")
    lines = sorted(l.strip() for l in text.splitlines() if l.strip())
    d = hashlib.sha256("\n".join(lines).encode()).hexdigest()
    digests.append((c, d))
    print(f"{c:14s} {d[:16]}… ({len(lines)} 行)")
log.close()
os.close(fd)
print("saved:", OUT)
