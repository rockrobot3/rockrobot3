"""千次 ping 稳定性探针（T1 验收判据条目 5：1000 次往返 0 失败，RTT 记档）。

用法: python3 ping_probe.py [termios 常量名，缺省 B1500000] [次数，缺省 1000]
      换口用 env `RR3_PROBE_TTY`。

载波 = 行面 INFO（只读观测命令，不动舵机/表情/音频——千次往返零执行面磨损）。
0.0.6 sim 时代判据原文写的是 rrd UDS 的 `rr3.ping`；T1 载体（Pi 宿主 × classic 板）
上等价的命令面往返 = 板行面命令收发，判据三条原样平移：
0 失败 / RTT 中位数记档 / 与基线比（≤2×）。

判据（三平台验证计划 §7.2 条目 5）：
- 应答缺失 = 0（2.0s 内未收到 `INFO pan=… tilt=… lim=pan[…` 行）
- RTT 中位数 / P95 / 最大值记档，随批落册与基线对表

进度每 100 次一行；终局 PASS/FAIL + 统计（exit 0/1）。
"""
import os
import re
import statistics
import sys
import time

from probe_common import baud_from_argv_or_env, flush_input, open_tty, pump, read_avail

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)
N = int(sys.argv[2]) if len(sys.argv) > 2 else 1000
TIMEOUT = 2.0
POLL = 0.005  # RTT 量化粒度（5ms；UART INFO 往返远大于此，量化误差可忽略）

INFO_RE = re.compile(rb"INFO pan=\d+ tilt=\d+ lim=pan\[")

fd = open_tty(DEV, BAUD)
flush_input(fd)
pump(fd, 1.2)  # 吸收启动/上一会话残留

rtts = []
missing = 0
t0 = time.monotonic()
for i in range(1, N + 1):
    os.write(fd, b"INFO\n")
    buf = b""
    sent = time.monotonic()
    while time.monotonic() - sent < TIMEOUT:
        chunk = read_avail(fd, POLL)
        if chunk:
            buf += chunk
            if INFO_RE.search(buf):
                break
    if INFO_RE.search(buf):
        rtts.append((time.monotonic() - sent) * 1000.0)
    else:
        missing += 1
        print("[%d] MISS %.2fs 实收: %r" % (i, time.monotonic() - sent, buf.strip()[:100]))
    if i % 100 == 0:
        print("… %d/%d  累计缺失=%d  已用 %.1fs" % (i, N, missing, time.monotonic() - t0))


def pctl(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100.0 * (len(xs) - 1))))]


print("=== 千次 ping 结算 ===")
ok_all = missing == 0
print("次数=%d 应答=%d 缺失=%d" % (N, len(rtts), missing))
if rtts:
    print("RTT ms: 中位=%.1f P95=%.1f 最大=%.1f 最小=%.1f" % (
        statistics.median(rtts), pctl(rtts, 95), max(rtts), min(rtts)))
print("[%s] 千次 ping %s" % ("PASS" if ok_all else "FAIL",
      "0 失败" if ok_all else "存在缺失——现场处置：查串口线/波特率档/板是否复位"))
sys.exit(0 if ok_all else 1)
