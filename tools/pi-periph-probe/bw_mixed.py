"""音频流 + 命令面混跑压测（树莓派 PL011 直连 classic 板）。

用法: python3 bw_mixed.py [termios 常量名，缺省 B2000000] [总秒数，缺省 30]
      换口用 env `RR3_PROBE_TTY`。

边收音频边每 5s 打 50 条 PAN + INFO，末尾 AUDIO OFF 并统计：
音频字节率/帧数/头失配/撕裂/seq 断档/PCM 长度集，回包 OK 与 ERR 计数。
判据：0 失配、0 撕裂、0 断档、PCM 全 8192、OK = 50×轮数、ERR = 0。
"""
import os, select, termios, time, re, base64, sys
DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
def baud_of(name):
    """档名 → speed_t：Linux 取 termios 常量；macOS 的 termios 无 921600+ 常量，直接用数值。"""
    if hasattr(termios, name):
        return getattr(termios, name)
    if name.startswith("B") and name[1:].isdigit():
        return int(name[1:])
    raise SystemExit("未知波特率档名: " + name)


BAUD = baud_of(sys.argv[1] if len(sys.argv) > 1 else os.environ.get("RR3_PROBE_BAUD", "B2000000"))
SECS = int(sys.argv[2]) if len(sys.argv) > 2 else 30
fd = os.open(DEV, os.O_RDWR|os.O_NOCTTY|os.O_NONBLOCK)
a = termios.tcgetattr(fd)
a[0]=0; a[1]=0; a[2]=termios.CS8|termios.CREAD|termios.CLOCAL; a[3]=0
a[4]=BAUD; a[5]=BAUD; a[6][termios.VMIN]=0; a[6][termios.VTIME]=0
termios.tcsetattr(fd, termios.TCSANOW, a); termios.tcflush(fd, termios.TCIOFLUSH)
def rd(t):
    r,_,_=select.select([fd],[],[],t)
    if not r: return b""
    try: return os.read(fd,16384)
    except BlockingIOError: return b""
def pump(s):
    o=b""; t0=time.time()
    while time.time()-t0<s:
        d=rd(0.05)
        if d: o+=d
    return o
pump(0.6)
os.write(fd,b"AUDIO ON\n"); pump(0.8)
buf=bytearray(); t0=time.time(); ok=0; err=0; n_sent=0
while time.time()-t0<SECS:
    d=rd(0.05)
    if d: buf+=d
    el=time.time()-t0
    if el> (n_sent+1)*5 and n_sent < SECS//5:
        os.write(fd, b"".join(b"PAN 90\n" for _ in range(50)) + b"INFO\n"); n_sent+=1
os.write(fd,b"AUDIO OFF\n"); tail=pump(2.0); buf+=tail
allt=bytes(buf).decode(errors="replace")
ok=allt.count("OK pan=90"); err=allt.count("ERR")
MAGIC=b"\x31\x45\x52\x41"; n=len(buf); i=buf.find(MAGIC); frames=0; bad=0; torn=0; seqs=[]; lens=[]
while i>=0 and i+128<=n:
    if buf[i:i+4]!=MAGIC: bad+=1; i+=1; continue
    plen=int.from_bytes(buf[i+40:i+44],"little")
    if plen==0 or plen>20000: torn+=1; i+=1; continue
    if i+128+plen>n: torn+=1; break
    p=bytes(buf[i+128:i+128+plen]); frames+=1
    m=re.search(rb'"seq":(\d+)',p); seqs.append(int(m.group(1))) if m else None
    b=re.search(rb'"pcm_b64":"([^"]*)"',p)
    if b:
        s=b.group(1)
        # 流尾截断片会抓出半截 b64（padding 非法）——记 -1 不算板缺陷
        try: lens.append(len(base64.b64decode(s+b"="*((4-len(s)%4)%4))))
        except Exception: lens.append(-1)
    i+=128+plen
gap=sum(1 for x,y in zip(seqs,seqs[1:]) if y!=x+1)
print("[混跑 %ds] 音频 %d B (%.1f KB/s) 帧=%d 头失配=%d 撕裂=%d seq断档=%d PCM长度集=%s"
      % (SECS, n, n/1024.0/SECS, frames, bad, torn, gap, sorted(set(lens))))
print("           期间发包 %d 轮(每轮50 PAN+INFO)：回包 OK=%d/%d ERR=%d" % (n_sent, ok, n_sent*50, err))
real_err=re.findall(r"ERR[^\r\n]*", allt)
print("           ERR 样本:", real_err[:5])
infos=re.findall(r"INFO pan=[^\r\n]*", allt)
print("           末条 INFO:", infos[-1] if infos else "(无)")
os.close(fd)
