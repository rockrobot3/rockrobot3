"""树莓派探针公共层——波特率解析 / 开口 / 收流 / 帧扫描与字节分类的唯一实现。

本目录六支探针此前各自复制了一套 `baud_of` / termios 开口 / `pump` / 帧扫描，
同一判据在不同脚本里含义不同（典型：混跑把命令回包文本逐字节计成「头失配」，
数值失真；收尾固定 pump 2s 导致回包计数系统性偏低）。抽到这里后，判据只有一份
实现，改一次全目录一致。

三个真值口径（后续任何探针都必须用这里的定义，不得再各写一套）：

1. **帧面**：magic `31 45 52 41`；`payload_len` 在帧头偏移 40。帧与帧之间的字节
   不是「坏头」，而是一个待分类的**段**。
2. **段分类**（[`classify_segment`]）：段内全部字节都是可打印 ASCII（0x20–0x7E
   加 CR/LF/TAB）→ 判为**行面回包文本**（合法流量，不计误码）；段内出现任何非
   可打印字节 → 判为**乱码段**（这才是线面误码的证据，并保留样本）。
3. **收尾**（[`drain_until_quiet`]）：连续 `quiet` 秒无新字节才认为回包到齐，
   固定时长收尾（原先 2s）会把末轮回包截断，制造「假丢包」。

用法（其他探针）::

    from probe_common import baud_of, open_tty, pump, drain_until_quiet, scan_frames
"""
import base64
import os
import re
import select
import sys
import termios
import time

#: 帧 magic 的线上字节序（LE u32 0x41524531 → 31 45 52 41）。
MAGIC = b"\x31\x45\x52\x41"
#: 帧头固定长度（128B）。
HEADER_LEN = 128
#: `payload_len` 在帧头内的偏移。
OFF_PAYLOAD_LEN = 40
#: 线上合法 payload 上限（防御：坏头里读出的长度不可信）。
MAX_PAYLOAD = 20000


def baud_of(name):
    """档名 → speed_t：Linux 取 termios 常量；macOS 的 termios 无 921600+ 常量，直接用数值。"""
    if hasattr(termios, name):
        return getattr(termios, name)
    if name.startswith("B") and name[1:].isdigit():
        return int(name[1:])
    raise SystemExit("未知波特率档名: " + name)


def baud_from_argv_or_env(argv_index=1, env_key="RR3_PROBE_BAUD", default="B1500000"):
    """命令行第 N 位优先，其次 env，最后缺省档（缺省与板固件 `UART2_BAUD_HZ` 同档）。

    `argv_index=None` 表示**只读 env**——留位置参数给别的用途（如 u3_capture 的
    输出文件路径），避免把路径误当档名。
    """
    if argv_index is not None and len(sys.argv) > argv_index:
        return baud_of(sys.argv[argv_index])
    return baud_of(os.environ.get(env_key, default))


def open_tty(dev, baud):
    """裸 termios 开口（8N1，无流控，非阻塞，VMIN=0/VTIME=0）。返回 fd。"""
    fd = os.open(dev, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    a = termios.tcgetattr(fd)
    a[0] = 0  # iflag
    a[1] = 0  # oflag
    a[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    a[3] = 0  # lflag
    a[4] = baud
    a[5] = baud
    a[6][termios.VMIN] = 0
    a[6][termios.VTIME] = 0
    termios.tcsetattr(fd, termios.TCSANOW, a)
    termios.tcflush(fd, termios.TCIOFLUSH)
    return fd


def flush_input(fd):
    """清空接收侧残留字节。

    **起流前必调**：否则缓冲里可能压着上一轮/开机横幅的残段，缓冲区会从一帧的
    中间开始，帧头保留区的零字节会被口径误判成「乱码」（实测 11124B 假乱码）。
    这是消除假象的构造性做法，不靠事后按长度猜。
    """
    termios.tcflush(fd, termios.TCIFLUSH)


def read_avail(fd, timeout):
    """等就绪再读一次（非阻塞口，读空返回 b""）。"""
    r, _, _ = select.select([fd], [], [], timeout)
    if not r:
        return b""
    try:
        return os.read(fd, 16384)
    except BlockingIOError:
        return b""


def pump(fd, sec):
    """固定时长收流（仅用于已知时长的短动作，如吸收启动横幅）。"""
    out = b""
    t0 = time.time()
    while time.time() - t0 < sec:
        d = read_avail(fd, 0.05)
        if d:
            out += d
    return out


def drain_until_quiet(fd, quiet=0.8, cap=4.0):
    """收流直到连续 `quiet` 秒无新字节（或达 `cap` 秒上限）。

    这是**收尾的唯一正确做法**：固定时长收尾会在回包还压在线上时提前返回，
    表现为「回包 OK 计数偏低」这种假丢包。
    """
    out = b""
    t0 = time.time()
    t_last = t0
    while True:
        d = read_avail(fd, 0.05)
        now = time.time()
        if d:
            out += d
            t_last = now
        if now - t_last >= quiet or now - t0 >= cap:
            return out


def is_text_byte(b):
    """该字节是否属于行面文本（可打印 ASCII + CR/LF/TAB）。"""
    return 0x20 <= b <= 0x7E or b in (0x0A, 0x0D, 0x09)


def classify_segment(seg):
    """段分类：全文本 → "text"；混有非文本字节 → "noise"。

    行面回包（`OK pan=90\\r\\n`、`ERR ...`、`INFO ...`）全部是可打印 ASCII，
    故「帧间字节段」里出现非文本字节才是真正的线面误码证据。
    """
    return "text" if all(is_text_byte(b) for b in seg) else "noise"


def _account_segment(st, seg):
    """把帧外字节段记账：文本段入 `text_bytes`/`lines`，乱码段入 `noise_bytes`。"""
    if not seg:
        return
    if classify_segment(seg) == "text":
        st["text_bytes"] += len(seg)
        st["lines"].extend(
            ln.decode("ascii", "replace") for ln in bytes(seg).split(b"\r\n") if ln.strip()
        )
    else:
        st["noise_bytes"] += len(seg)
        if len(st["noise_samples"]) < 3:
            st["noise_samples"].append(bytes(seg[:24]))


def scan_frames(buf):
    """扫描音频帧并按段分类统计。返回 dict（键见下）。

    帧面统计：`frames` / `seqs` / `lens`（PCM 字节数；-1=截断片、-2=无 b64 字段）/
    `torn`（坏长度或尾部截断）/ `leading`（首帧前字节数，仅长度，内容计入下方口径）。
    口径统计：`text_bytes`（行面回包文本字节，合法）/ `noise_bytes`（乱码段字节，
    误码证据）/ `noise_samples`（乱码段原文样本，最多 3 条）/ `lines`（文本段里
    切出的行，**回包计数必须只用这里**——按整条解码文本切行会把帧载荷 JSON 和
    回包挤进同一「行」，制造假 mangled）。
    `text_bytes` 覆盖**全部**帧外字节：首帧前的启动残段、帧间段、末帧后的尾段。
    """
    n = len(buf)
    first = buf.find(MAGIC)
    st = {
        "frames": 0, "seqs": [], "lens": [], "torn": 0, "leading": 0,
        "text_bytes": 0, "noise_bytes": 0, "noise_samples": [], "lines": [],
    }
    if first < 0:
        st["leading"] = n
        _account_segment(st, buf)
        return st
    st["leading"] = first
    _account_segment(st, buf[:first])
    i = first
    while i + HEADER_LEN <= n:
        if buf[i:i + 4] != MAGIC:
            j = buf.find(MAGIC, i + 1)
            if j < 0:
                j = n
            _account_segment(st, buf[i:j])
            i = j
            continue
        plen = int.from_bytes(buf[i + OFF_PAYLOAD_LEN:i + OFF_PAYLOAD_LEN + 4], "little")
        if plen == 0 or plen > MAX_PAYLOAD:
            st["torn"] += 1
            i += 1
            continue
        if i + HEADER_LEN + plen > n:
            st["torn"] += 1
            break
        p = bytes(buf[i + HEADER_LEN:i + HEADER_LEN + plen])
        st["frames"] += 1
        m = re.search(rb'"seq":(\d+)', p)
        if m:
            st["seqs"].append(int(m.group(1)))
        b = re.search(rb'"pcm_b64":"([^"]*)"', p)
        if b:
            s = b.group(1)
            # 流尾截断片会抓出半截 b64（padding 非法）——记 -1 不算板缺陷
            try:
                st["lens"].append(len(base64.b64decode(s + b"=" * ((4 - len(s) % 4) % 4))))
            except Exception:
                st["lens"].append(-1)
        else:
            st["lens"].append(-2)
        i += HEADER_LEN + plen
    # 末帧之后的尾段（AUDIO OFF 回包等）同样是帧外字节，必须入账
    _account_segment(st, buf[i:])
    return st


def seq_gaps(seqs):
    """相邻 seq 不连续计数（断档）。"""
    return sum(1 for x, y in zip(seqs, seqs[1:]) if y != x + 1)


def fmt_noise(st):
    """乱码段的可读摘要（无乱码时返回空串）。"""
    if not st["noise_bytes"]:
        return ""
    samples = " | ".join(repr(s) for s in st["noise_samples"])
    return "乱码=%dB 样本: %s" % (st["noise_bytes"], samples)
