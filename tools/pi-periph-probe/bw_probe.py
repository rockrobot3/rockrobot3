"""UART 带宽/误码连续性探针（树莓派直连 classic 板）。

用法: python3 bw_probe.py [termios 常量名，缺省 B1500000] [音频秒数，缺省 60]
      换口用 env `RR3_PROBE_TTY`。

测三件事：
1. READY 横幅里的 baud 值（板固件当前档，对表用）
2. 控制面突发 120 条 PAN 的回包完整性（命令面吞吐/丢包）
3. AUDIO ON 连续音频流的帧完整性

口径（[`probe_common`] 单点定义，本脚本不再自造）：
- 帧面：撕裂帧 / seq 断档 / PCM 长度集
- 帧间字节按**段**分类：行面回包文本（合法）vs 乱码段（含非可打印字节 = 误码证据）。
  原先把回包逐字节计成「头失配」的旧口径已废弃——那会把合法回包算成线面噪声。
"""
import os
import sys
import time

from probe_common import (
    baud_from_argv_or_env, flush_input, fmt_noise, open_tty, pump, read_avail,
    scan_frames, seq_gaps,
)

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)
SECS = int(sys.argv[2]) if len(sys.argv) > 2 else 60

fd = open_tty(DEV, BAUD)

ready = pump(fd, 1.5)
rline = [l for l in ready.decode(errors="replace").splitlines() if "READY" in l]
print("[1] READY:", rline[0] if rline else "(未见 READY 横幅)")

os.write(fd, b"HELP\n")
h = pump(fd, 1.0).decode(errors="replace")
print("[2] HELP:", "OK" if "CMDS" in h else "FAIL " + h[:90])

os.write(fd, b"".join(b"PAN 90\n" for _ in range(120)))
t = pump(fd, 3.0).decode(errors="replace")
print("[3] 突发 120xPAN: OK=%d/120 ERR=%d" % (t.count("OK pan=90"), t.count("ERR")))

flush_input(fd)
os.write(fd, b"AUDIO ON\n")
buf = bytearray(pump(fd, 0.8))  # 起流后的字节一律入账（旧版在这里白扔 0.8s，缓冲区从帧中间开始）
t0 = time.time()
while time.time() - t0 < SECS:
    d = read_avail(fd, 0.05)
    if d:
        buf += d
el = time.time() - t0
os.write(fd, b"AUDIO OFF\n")
buf += pump(fd, 2.0)

st = scan_frames(buf)
if st["frames"] == 0:
    print("[4] 音频: 流中找不到帧 magic —— 链路疑似不通")
    os.close(fd)
    sys.exit(1)

lens = st["lens"]
avg = sum(lens) / len(lens) if lens else 0
print("[4] 音频 %.1fs: %d B (%.1f KB/s) 帧=%d 撕裂=%d 前置=%dB"
      % (el, len(buf), len(buf) / 1024.0 / el, st["frames"], st["torn"], st["leading"]))
print("    帧间: 行面文本=%dB ｜ %s"
      % (st["text_bytes"], fmt_noise(st) or "乱码=0B（无段级误码）"))
print("    seq 断档=%d 首=%s 末=%s；PCM 长度 min/max/均=%.0f/%.0f/%.0f（期望 8192）"
      % (seq_gaps(st["seqs"]), st["seqs"][0] if st["seqs"] else "-",
         st["seqs"][-1] if st["seqs"] else "-",
         min(lens) if lens else 0, max(lens) if lens else 0, avg))
os.close(fd)
