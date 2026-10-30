"""写前记账 / 队列上限探针（长行程 120→60 的即时 INFO 判据）。

用法: python3 probe2.py    （缺省 1.5M 档；换档 env `RR3_PROBE_BAUD`、换口
env `RR3_PROBE_TTY`）

A) PAN 120 稳到后发 PAN 60，1ms 内 INFO 应报 **120**（作业未写前记账则假报 60）；
B) 1.2s 后 INFO 应报 60；C) 交错 30 条后看 job_drop/reply_drop 与 30 条回包。
"""
import os
import re
import time

from probe_common import baud_from_argv_or_env, open_tty, pump

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
fd = open_tty(DEV, baud_from_argv_or_env())


def text(sec):
    """收流并按行面文本返回。"""
    return pump(fd, sec).decode(errors="replace")


text(0.4)
# 先稳到 pan=120
os.write(fd, b"PAN 120\n")
text(1.2)
os.write(fd, b"PAN 60\nINFO\n")
imm = text(0.35)
m = re.search(r"INFO pan=(\d+)", imm)
print("A) 长行程 PAN 60 后 1ms 的 INFO pan =", m.group(1) if m else "?")
print("   （写前记账已修 → 应为 120；旧版会报 60 假姿态）")
time.sleep(1.2)
text(0.0)
os.write(fd, b"INFO\n")
after = text(0.5)
m2 = re.search(r"INFO pan=(\d+)", after)
print("B) 1.2s 后 INFO pan =", m2.group(1) if m2 else "?（应为 60）")

# 队列上限：交错发不同形状，无法合并
seq = b""
for i in range(30):
    seq += (b"PAN 90\n" if i % 3 == 0 else b"TILT 90\n" if i % 3 == 1 else b"SERVO 90 90\n")
os.write(fd, seq)
resp = text(3.0)          # 这 30 条的回包落在这里，必须在本段计数（旧版把它扔掉再数尾部 INFO，恒为 0）
os.write(fd, b"INFO\n")
txt = text(0.8)
m3 = re.search(r"job_drop=\d+ reply_drop=\d+", txt)
print("C) 交错 30 条后:", m3.group(0) if m3 else txt.strip()[:120])
print("   回复条数 OK =", resp.count("OK "), " / 30 期望")
os.close(fd)
