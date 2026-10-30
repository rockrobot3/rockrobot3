"""行面 robustness 六探（超长行 / 半行超时 / 拼接检测 / 连发 / 写前记账）。

用法: python3 robust_probe.py    （缺省 1.5M 档；换档 env `RR3_PROBE_BAUD`、
换口 env `RR3_PROBE_TTY`）

T6a/T6b 的判据是写前记账（1.5s 稳态角），"从 PAN 90 起步测不出回退"这个
脚本局限以 probe2.py 的长行程 120→60 为准。
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


def run(tag, payload, wait):
    os.write(fd, payload)
    txt = text(wait)
    print(f"--- {tag} ---")
    print(txt.strip() or "(无回包)")
    return txt


text(0.5)

# T1 超长行：200 字节 + \n
run("T1 超长行(200B)", b"A" * 200 + b"\n", 1.0)
# T2 紧跟一条正常命令：证明残尾已被吞掉（不会出现被污染的应答）
run("T2 超长行后 HELP", b"HELP\n", 1.0)
# T3 半行断线：发 "PAN 9" 不带换行，等 3s
run("T3 半行超时(PAN 9 无换行)", b"PAN 9", 3.0)
# T4 再发 "0"：若半行被拼进下一条，会回 OK pan=90；期望是 unknown cmd "0"
run("T4 半行后发 0(拼接检测)", b"0\n", 1.0)
# T5 高频 PAN 连发 20 条
os.write(fd, b"".join(b"PAN 90\n" for _ in range(20)))
t5 = text(2.0)
print("--- T5 20x PAN 连发 ---")
print(f"OK pan=90 条数 = {t5.count('OK pan=90')}")
# T6 写前记账检测：PAN 90 后立刻 INFO（应仍是旧角），1.5s 后再 INFO（应为 90）
run("T6a PAN 90 后立即 INFO", b"PAN 90\n", 0.0)
os.write(fd, b"INFO\n")
t6a = text(0.4)
m = re.search(r"pan=(\d+)", t6a)
print("--- T6a 立即 INFO ---")
print(t6a.strip())
print("即时 pan =", m.group(1) if m else "?")
time.sleep(1.5)
t6b = run("T6b 1.5s 后 INFO", b"INFO\n", 0.6)
m2 = re.search(r"pan=(\d+)", t6b)
print("稳态 pan =", m2.group(1) if m2 else "?")
os.close(fd)
