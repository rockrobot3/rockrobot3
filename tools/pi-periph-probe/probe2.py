"""写前记账 / 队列上限探针（长行程 120→60 的即时 INFO 判据）。

用法: python3 probe2.py    （缺省 2M 档；换档 env `RR3_PROBE_BAUD`、换口
env `RR3_PROBE_TTY`）

A) PAN 120 稳到后发 PAN 60，1ms 内 INFO 应报 **120**（作业未写前记账则假报 60）；
B) 1.2s 后 INFO 应报 60；C) 交错 30 条后看 job_drop/reply_drop。
注意末行"回复条数 OK"只数尾部 INFO 回包文本，是脚本口径缺陷，判据以
`job_drop=0 reply_drop=0` 为准。
"""
import os, select, termios, time, re

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
def baud_of(name):
    """档名 → speed_t：Linux 取 termios 常量；macOS 的 termios 无 921600+ 常量，直接用数值。"""
    if hasattr(termios, name):
        return getattr(termios, name)
    if name.startswith("B") and name[1:].isdigit():
        return int(name[1:])
    raise SystemExit("未知波特率档名: " + name)


BAUD = baud_of(os.environ.get("RR3_PROBE_BAUD", "B2000000"))

fd = os.open(DEV, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
a = termios.tcgetattr(fd)
a[0]=0; a[1]=0; a[2]=termios.CS8|termios.CREAD|termios.CLOCAL; a[3]=0
a[4]=BAUD; a[5]=BAUD
a[6][termios.VMIN]=0; a[6][termios.VTIME]=0
termios.tcsetattr(fd, termios.TCSANOW, a); termios.tcflush(fd, termios.TCIOFLUSH)
def pump(sec):
    out=b""; t0=time.time()
    while time.time()-t0<sec:
        r,_,_=select.select([fd],[],[],0.05)
        if r:
            d=os.read(fd,4096)
            if d: out+=d
    return out.decode(errors="replace")
pump(0.4)
# 先稳到 pan=120
os.write(fd,b"PAN 120\n"); pump(1.2)
os.write(fd,b"PAN 60\nINFO\n")
imm = pump(0.35)
m = re.search(r"INFO pan=(\d+)", imm)
print("A) 长行程 PAN 60 后 1ms 的 INFO pan =", m.group(1) if m else "?")
print("   （写前记账已修 → 应为 120；旧版会报 60 假姿态）")
time.sleep(1.2)
after = pump(0.0); os.write(fd,b"INFO\n"); after = pump(0.5)
m2 = re.search(r"INFO pan=(\d+)", after)
print("B) 1.2s 后 INFO pan =", m2.group(1) if m2 else "?（应为 60）")
# 队列上限：交错发不同形状，无法合并
seq = b""
for i in range(30):
    seq += (b"PAN 90\n" if i%3==0 else b"TILT 90\n" if i%3==1 else b"SERVO 90 90\n")
os.write(fd, seq)
pump(3.0)
os.write(fd,b"INFO\n")
txt = pump(0.8)
print("C) 交错 30 条后:", re.search(r"job_drop=\d+ reply_drop=\d+", txt).group(0) if re.search(r"job_drop=\d+ reply_drop=\d+", txt) else txt.strip()[:120])
print("   回复条数 OK =", txt.count("OK "), " / 30 期望")
os.close(fd)
