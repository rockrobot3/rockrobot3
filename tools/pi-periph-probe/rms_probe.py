"""流内 PCM RMS 实测探针（VAD 阈值对表用，批N37 条目6/7 排查件）。

用法: python3 rms_probe.py [termios 常量名，缺省 B1500000] [秒数，缺省 30]
      换口用 env `RR3_PROBE_TTY`。

口径：AUDIO ON 抓流 → [`probe_common.scan_frames`] 单点解析（含帧/CRC/seq 统计）
→ 解码全部 `pcm_b64` → s16 整体 RMS ＋ 每秒 RMS 序列。

对表真值：sense VAD 阈 750 / doa_window_rms_min 500（config-esp32.toml 现值，
U 轮续4 重标口径）。判据：正常说话段每秒 RMS 应显著高于 750；若说话段 < 750
⇒ VAD 漏检根因是声学电平不足（增益/距离/麦克风位），不是状态机问题——
阈值调整属策略面，归 config，不在本探针内改。
"""
import base64
import array
import math
import os
import re
import sys
import time

from probe_common import baud_from_argv_or_env, open_tty, scan_frames

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
SECS = float(sys.argv[2]) if len(sys.argv) > 2 else 30.0

fd = open_tty(DEV, baud_from_argv_or_env(1))
os.write(fd, b"AUDIO ON\r\n")
buf = b""
t0 = time.monotonic()
while time.monotonic() - t0 < SECS:
    try:
        buf += os.read(fd, 65536)
    except Exception:
        pass
    time.sleep(0.01)
os.write(fd, b"AUDIO OFF\r\n")
time.sleep(0.5)
try:
    buf += os.read(fd, 65536)
except Exception:
    pass

st = scan_frames(buf)
print("raw=%dB frames=%d torn=%d crc_bad=%d lens_ok=%d seq_gap=%d" % (
    len(buf), st["frames"], st["torn"], st["crc_bad"],
    sum(1 for x in st["lens"] if x > 0),
    sum(1 for x, y in zip(st["seqs"], st["seqs"][1:]) if y != x + 1)))

pcm = b""
for m in re.finditer(rb'"pcm_b64":"([^"]*)"', buf):
    try:
        pcm += base64.b64decode(m.group(1) + b"=" * ((4 - len(m.group(1)) % 4) % 4))
    except Exception:
        pass
s = array.array("h")
s.frombytes(pcm[:len(pcm) // 2 * 2])
n = len(s)
print("pcm_bytes=%d samples=%d" % (len(pcm), n))
if n:
    rms = math.sqrt(sum(x * x for x in s) / n)
    print("整体 RMS = %.1f （VAD 阈 750 / doa_window_rms_min 500 对表）" % rms)
    win = 32000  # 16kHz × 2ch × 1s
    rows = [round(math.sqrt(sum(x * x for x in s[i:i + win]) / win), 1)
            for i in range(0, n - win, win)]
    print("每秒 RMS（说话段应显著 > 750）:")
    for i in range(0, len(rows), 10):
        print(" ", " ".join(map(str, rows[i:i + 10])))
else:
    print("NO PCM —— 检查 AUDIO ON 是否被板接受、流是否在发")
