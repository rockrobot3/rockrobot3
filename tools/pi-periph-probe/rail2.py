"""轨压两点采样探针（批N37 条目4：静止态 + WANDER 负载态各一次 RAIL）。

用法: python3 rail2.py [termios 常量名，缺省 B1500000]

口径：RAIL 为异步回包（ack + 采样结果可能分段到达，full_smoke 先例——收 3.5s
把两段都算进来）；负载态用 WANDER 连续扫摆制造舵机电流波动。判据对表在
批N37 计划册：板读数 vs 万用表误差 ≤50mV，全程无 BOD（板复位会打 READY 横幅，
本探针把横幅也当异常证据打印）。
"""
import os
import re
import sys
import time

from probe_common import baud_from_argv_or_env, flush_input, open_tty, pump

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")

fd = open_tty(DEV, baud_from_argv_or_env(1))
flush_input(fd)
boot = pump(fd, 1.2)
if b"READY" in boot:
    print("异常：捕获到板启动横幅（板复位过）:", boot[:80])


def rail(tag):
    os.write(fd, b"RAIL\n")
    got = pump(fd, 3.5).decode("utf-8", "replace")
    if "READY" in got:
        print("%s: 异常——回包中混入启动横幅（BOD 复位证据）" % tag)
        return None
    m = re.search(r"RAIL rail=(\d+)mV raw=(\d+)", got)
    print("%s: %s" % (tag, m.group(0) if m else "无回包: " + repr(got.strip()[:80])))
    return int(m.group(1)) if m else None


r1 = rail("静止态")
os.write(fd, b"WANDER\n")
pump(fd, 1.0)
time.sleep(1.0)
r2 = rail("负载态(WANDER 扫摆中)")
os.write(fd, b"WANDER STOP\n")
pump(fd, 1.0)
print("RESULT 静止=%s 负载=%s" % (r1, r2))
