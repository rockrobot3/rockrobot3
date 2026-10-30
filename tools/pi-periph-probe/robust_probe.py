"""行面 robustness 六探（超长行 / 半行超时 / 拼接检测 / 连发 / 写前记账）。

用法: python3 robust_probe.py    （缺省 1.5M 档；换档 env `RR3_PROBE_BAUD`、
换口 env `RR3_PROBE_TTY`）

T6a/T6b 的判据是写前记账（1.5s 稳态角），"从 PAN 90 起步测不出回退"这个
脚本局限以 probe2.py 的长行程 120→60 为准。
"""
import os, select, termios, time, sys, re

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
def baud_of(name):
    """档名 → speed_t：Linux 取 termios 常量；macOS 的 termios 无 921600+ 常量，直接用数值。"""
    if hasattr(termios, name):
        return getattr(termios, name)
    if name.startswith("B") and name[1:].isdigit():
        return int(name[1:])
    raise SystemExit("未知波特率档名: " + name)


BAUD = baud_of(os.environ.get("RR3_PROBE_BAUD", "B1500000"))

fd = os.open(DEV, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
a = termios.tcgetattr(fd)
a[0] = 0; a[1] = 0; a[2] = termios.CS8 | termios.CREAD | termios.CLOCAL; a[3] = 0
a[4] = BAUD; a[5] = BAUD
a[6][termios.VMIN] = 0; a[6][termios.VTIME] = 0
termios.tcsetattr(fd, termios.TCSANOW, a)
termios.tcflush(fd, termios.TCIOFLUSH)

def pump(sec):
    out = b""; t0 = time.time()
    while time.time() - t0 < sec:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            d = os.read(fd, 4096)
            if d: out += d
    return out.decode(errors="replace")

def run(tag, payload, wait):
    os.write(fd, payload)
    txt = pump(wait)
    print(f"--- {tag} ---")
    print(txt.strip() or "(无回包)")
    return txt

pump(0.5)

# T1 超长行：200 字节 + \n
t1 = run("T1 超长行(200B)", b"A"*200 + b"\n", 1.0)
# T2 紧跟一条正常命令：证明残尾已被吞掉（不会出现被污染的应答）
t2 = run("T2 超长行后 HELP", b"HELP\n", 1.0)
# T3 半行断线：发 "PAN 9" 不带换行，等 3s
t3 = run("T3 半行超时(PAN 9 无换行)", b"PAN 9", 3.0)
# T4 再发 "0"：若半行被拼进下一条，会回 OK pan=90；期望是 unknown cmd "0"
t4 = run("T4 半行后发 0(拼接检测)", b"0\n", 1.0)
# T5 高频 PAN 连发 20 条
os.write(fd, b"".join(b"PAN 90\n" for _ in range(20)))
t5 = pump(2.0)
print("--- T5 20x PAN 连发 ---")
print(f"OK pan=90 条数 = {t5.count(chr(79)+chr(75)+chr(32)+chr(112)+chr(97)+chr(110)+chr(61)+chr(57)+chr(48))}")
# T6 写前记账检测：PAN 90 后立刻 INFO（应仍是旧角），1.5s 后再 INFO（应为 90）
t6a = run("T6a PAN 90 后立即 INFO", b"PAN 90\n", 0.0)
os.write(fd, b"INFO\n")
t6a = pump(0.4)
import re
m = re.search(r"pan=(\d+)", t6a)
print("--- T6a 立即 INFO ---"); print(t6a.strip()); print("即时 pan =", m.group(1) if m else "?")
time.sleep(1.5)
t6b = run("T6b 1.5s 后 INFO", b"INFO\n", 0.6)
m2 = re.search(r"pan=(\d+)", t6b)
print("稳态 pan =", m2.group(1) if m2 else "?")
os.close(fd)
