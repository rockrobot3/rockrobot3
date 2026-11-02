"""U3 命令面 canonical digest 抓取（Pi 直连板，classic/S3 同口径）。

用法: python3 u3_capture.py <输出文件路径>

串行发 10 条代表命令（HELP/OLED/FACE×4/2×SERVO 轴/SCAN），逐条抓**行面文本
回包**并算「回包行集（排序后去空行）」的 sha256，用于跨次/跨固件自等对拍。

判据口径（2026-11-02 批N39 修正）：
- 先发 `AUDIO OFF`，把上一会话的音频流停掉并排空——音频帧/`pcm_b64` 不是命令
  面回包，混进 digest 会让同一固件两次抓取都不同（旧版实测 SCAN 行数 26 vs 40）。
- 只保留行面文本行：剔 NUL/不可打印控制符、含 `pcm_b64`/`jsonrpc` 的 JSON 帧、
  UTF-8 替换符；`READY` boot 横幅不计入（跨次/跨复位会漂）。
- SCAN 等到终局行 `OK scan done`/`ERR SCAN` 再收；其余命令按静默窗口收齐，
  不用固定 1.2s 硬窗（异步姿态回包会晚到）。
- 缺省 1.5M 档；改档用 env `RR3_PROBE_BAUD`，换口用 `RR3_PROBE_TTY`。
"""
import hashlib
import os
import sys
import time

from probe_common import baud_from_argv_or_env, drain_until_quiet, open_tty, pump, read_avail

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
OUT = sys.argv[1]
CMDS = ["HELP", "OLED U3-A", "FACE happy", "FACE sad", "FACE idle",
        "PAN 90", "TILT 90", "PAN 60", "TILT 100", "SCAN"]

fd = open_tty(DEV, baud_from_argv_or_env(argv_index=None))  # argv[1] 是输出路径，档位只认 env

pump(fd, 1.0)  # 吸收启动段 READY/横幅
os.write(fd, b"AUDIO OFF\n")
pump(fd, 0.8)  # 停流并排空上一会话音频帧
drain_until_quiet(fd, 0.4, 2.0)

log = open(OUT, "w")
digests = []
for c in CMDS:
    os.write(fd, (c + "\n").encode())
    if c == "SCAN":
        # 等终局行（最多 25s）：逐点回包与 done 行是判据的一部分，早收会少行。
        buf = b""
        t0 = time.monotonic()
        while time.monotonic() - t0 < 25.0:
            buf += read_avail(fd, 0.05)
            if b"OK scan done" in buf or b"ERR SCAN" in buf:
                break
        drain_until_quiet(fd, 0.4, 1.5)
    else:
        buf = b""
        # 静默窗口收齐：0.4s 无新字节且至少 1.2s（异步姿态回包兜底）。
        t0 = time.monotonic()
        last = t0
        while True:
            d = read_avail(fd, 0.05)
            if d:
                buf += d
                last = time.monotonic()
            if time.monotonic() - t0 > 1.2 and time.monotonic() - last > 0.4:
                break
            if time.monotonic() - t0 > 3.5:
                break
    text = buf.decode("utf-8", "replace")
    log.write(f">>> {c}\n{text}\n")
    lines = []
    for l in text.splitlines():
        l = l.strip("\x00\r\n ")
        if not l or "READY" in l:
            continue
        if '"pcm_b64"' in l or "jsonrpc" in l or "\ufffd" in l:
            continue
        if any(ord(ch) < 0x20 for ch in l):
            continue
        lines.append(l)
    d = hashlib.sha256("\n".join(sorted(lines)).encode()).hexdigest()
    digests.append((c, d))
    print(f"{c:14s} {d[:16]}… ({len(lines)} 行)")
log.close()
os.close(fd)
print("saved:", OUT)
