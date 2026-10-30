"""音频流 + 命令面混跑压测（树莓派直连 classic 板）。

用法: python3 bw_mixed.py [termios 常量名，缺省 B1500000] [总秒数，缺省 30]
      换口用 env `RR3_PROBE_TTY`。

边收音频边每 5s 打 50 条 PAN + INFO，末尾 AUDIO OFF 后**等静默**收尾（不再固定 2s）。

判据（全部用 [`probe_common`] 的单点口径，不再本地各写一套）：
- 帧面：0 撕裂、0 seq 断档、PCM 全 8192、无「乱码段」
- 帧间字节按段分类：**行面回包文本**是合法流量，**乱码段**（含非可打印字节）
  才是线面误码证据 —— 原先把回包逐字节计成「头失配」的口径已废弃
- 命令面四分类对账（`sent = ok + err + mangled + missing`）：
  `ok` 严格回包 / `err` 板侧拒绝（含被误码破坏的命令）/ `mangled` 回包文本本身被坏
  / `missing` 完全没有回包（真正的静默丢失或截断）
- 板侧 ↔ 宿主侧音频对账：`seq/sent/drop` 增量 vs 宿主解出的帧数
"""
import os
import re
import sys
import time

from probe_common import (
    baud_from_argv_or_env, drain_until_quiet, flush_input, open_tty, pump,
    read_avail, scan_frames, seq_gaps,
)

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)
SECS = int(sys.argv[2]) if len(sys.argv) > 2 else 30

PAN = 90
ROUND = 5          # 每轮间隔秒数
PER_ROUND = 50     # 每轮 PAN 条数

fd = open_tty(DEV, BAUD)
pump(fd, 0.6)

def info_snapshot(text):
    """从 INFO 行里抽出板侧累计计数（audio_seq / audio_sent / audio_dropped）。"""
    m = re.search(r"seq=(\d+) sent=(\d+) drop=(\d+)", text)
    if not m:
        return None
    return tuple(int(x) for x in m.groups())

flush_input(fd)                       # 起流前清残留：否则缓冲区从帧中间开始，零字节会被误判乱码
os.write(fd, b"INFO\n")               # 基线必须在起流**前**采，否则增量窗口比宿主缓冲窗口短（实测差 6 帧）
base = info_snapshot(pump(fd, 0.6).decode(errors="replace"))
os.write(fd, b"AUDIO ON\n")
buf = bytearray(pump(fd, 0.8))        # 起流后的字节一律入账，不再白扔

t0 = time.time()
n_sent = 0
while time.time() - t0 < SECS:
    d = read_avail(fd, 0.05)
    if d:
        buf += d
    el = time.time() - t0
    if el > (n_sent + 1) * ROUND and n_sent < SECS // ROUND:
        os.write(fd, b"".join(b"PAN %d\n" % PAN for _ in range(PER_ROUND)) + b"INFO\n")
        n_sent += 1
os.write(fd, b"AUDIO OFF\n")
# 等静默收尾：末轮回包还压在线上时固定 2s 收尾会制造「假丢包」
buf += drain_until_quiet(fd)
os.write(fd, b"INFO\n")
final_txt = drain_until_quiet(fd, quiet=0.5, cap=2.0).decode(errors="replace")
buf += final_txt.encode()
fin = info_snapshot(final_txt)

allt = bytes(buf).decode(errors="replace")
st = scan_frames(buf)
# 取证开关：设 `RR3_PROBE_DUMP=<path>` 时把原始字节落盘，供离线逐段复核
# （判据有争议时不要靠猜——先抓原始流）。
_dump = os.environ.get("RR3_PROBE_DUMP")
if _dump:
    with open(_dump, "wb") as f:
        f.write(bytes(buf))

# 命令面四分类：只用 `scan_frames` 从**帧外文本段**切出的行
# （在整条解码文本上切行会把帧载荷 JSON 和回包挤进同一行，制造假 mangled）
CONTROL_OK = {"OK audio on", "OK audio off"}   # 起停控制回包，不属于 PAN 计数
lines = st["lines"]
ok = sum(1 for l in lines if l.startswith("OK pan=%d" % PAN))
errs = [l for l in lines if l.startswith("ERR")]
mangled = [l for l in lines
           if l.startswith("OK") and l not in CONTROL_OK
           and not l.startswith("OK pan=%d" % PAN)]
sent = n_sent * PER_ROUND
missing = sent - (ok + len(errs) + len(mangled))

print("[混跑 %ds] 音频 %d B (%.1f KB/s) 帧=%d 撕裂=%d seq断档=%d PCM长度集=%s"
      % (SECS, len(buf), len(buf) / 1024.0 / SECS, st["frames"], st["torn"],
         seq_gaps(st["seqs"]), sorted(set(st["lens"]))))
print("           帧间字节: 行面回包文本=%dB(%d 行) 乱码=%dB%s"
      % (st["text_bytes"], len(st["lines"]), st["noise_bytes"],
         (" 样本: " + " | ".join(repr(s) for s in st["noise_samples"])) if st["noise_bytes"] else ""))
print("           命令面: 发=%d ｜ ok=%d err=%d mangled=%d missing=%d"
      % (sent, ok, len(errs), len(mangled), missing))
if errs:
    print("           err 样本:", errs[:5])
if mangled:
    print("           mangled 样本:", [repr(m) for m in mangled[:5]])
if base and fin:
    dseq, dsent, ddrop = fin[0] - base[0], fin[1] - base[1], fin[2] - base[2]
    print("           板侧增量: seq=%+d sent=%+d drop=%+d ｜ 宿主帧=%d ｜ 差值=%+d"
          % (dseq, dsent, ddrop, st["frames"], st["frames"] + ddrop - dsent))
elif base or fin:
    print("           板侧对账: INFO 缺失（base=%s fin=%s）" % (bool(base), bool(fin)))
os.close(fd)
