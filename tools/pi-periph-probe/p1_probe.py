"""P1 稳健性四探（帧面残尾吞净 / 行面干净 / 观测面字段 / 突发回包）。

用法: python3 p1_probe.py    （缺省 1.5M 档；换档 env `RR3_PROBE_BAUD`、换口
env `RR3_PROBE_TTY`）

T1 首字节 0x31 + 128B 坏头 + 300B 残尾 → 期望 0 条 ERR（残尾被吞净而非当行执行）
T2 紧随的 HELP 应答干净；T3 INFO 带 job_drop/reply_drop；T4 120×PAN 全回包。
"""
import os
import re

from probe_common import baud_from_argv_or_env, open_tty, pump

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
fd = open_tty(DEV, baud_from_argv_or_env())


def text(sec):
    """收流并按行面文本返回（本探针只判行面回包，不解析帧）。"""
    return pump(fd, sec).decode(errors="replace")


text(1.2)

# T1 帧面失败残尾：首字节 0x31（帧面分流）→ 128B 坏头（magic 全 0）→ 300B 受污染残尾
os.write(fd, b"\x31" + b"\x00" * 127 + b"\x00" * 300)
t1 = text(1.0)
print("--- T1 坏帧头 + 300B 残尾 ---")
print(t1.strip() or "(无回包)")
print("残尾被当行解析的 ERR 行数 =", t1.count("ERR"), "（期望 0）")

# T2 紧接着正常行命令：应答必须干净、不被残尾污染
os.write(fd, b"HELP\n")
t2 = text(1.0)
lines = [l for l in t2.splitlines() if l.strip()]
print("--- T2 HELP ---")
print("首行 =", lines[0] if lines else "(无回包)")
print("含垃圾 ERR =", t2.count("ERR"), "（期望 0）")

# T3 观测面：job_drop / reply_drop 字段在
os.write(fd, b"INFO\n")
info = text(0.9)
m = re.search(r"job_drop=\d+ reply_drop=\d+", info)
print("--- T3 INFO ---")
print(m.group(0) if m else info.strip()[:160])

# T4 残尾吞净后的突发：120 条 PAN 全部回包（RX 环 4096 回归）
os.write(fd, b"".join(b"PAN 90\n" for _ in range(120)))
t4 = text(3.5)
print("--- T4 120 条 PAN 突发：OK pan=90 条数 =", t4.count("OK pan=90"), "/120")
os.close(fd)
