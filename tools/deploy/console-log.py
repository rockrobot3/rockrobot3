#!/usr/bin/env python3
"""串口控制台采集器：显式释放 DTR/RTS 后收原始字节（macOS/Linux 通用）。

现场教训（W7 控制台根修）：`cat` 或裸 `stty` 打开 CH340 时 DTR/RTS 常停在
置位态，EN/GPIO0 被拉低，实测表现为"ROM 日志可读、随后全是噪声"，极易误判
成固件控制台配错（真板复核：本工具下同一固件日志 100% 可读）。故本工具先
清两线再收流，收尾用可打印率自检并对低可打印率给出排查指向。

用法：
  python3 tools/deploy/console-log.py                 # 自动探测口，115200，直到 Ctrl-C
  python3 tools/deploy/console-log.py -s 15 -o /tmp/board.log
"""

import argparse
import fcntl
import glob
import os
import signal
import struct
import sys
import termios
import time

TIOCM_DTR = 0x0002
TIOCM_RTS = 0x0004
CANDIDATES = (
    "/dev/cu.usbserial-*",
    "/dev/cu.SLAB_USBtoUART*",
    "/dev/cu.wchusbserial*",
    "/dev/cu.usbmodem*",
    "/dev/ttyUSB*",
    "/dev/ttyACM*",
)


def pick_port(explicit):
    """显式优先；否则要求候选唯一（多口时点名列出，不猜）。"""
    if explicit:
        return explicit
    found = sorted(p for pat in CANDIDATES for p in glob.glob(pat))
    if len(found) == 1:
        return found[0]
    if not found:
        sys.exit("未找到串口设备，用 --port 指定")
    sys.exit("多个候选串口，用 --port 指定：\n  " + "\n  ".join(found))


def open_raw(port, baud):
    """raw 8N1 + 清 DTR/RTS（关键：否则芯片可能被按在复位/下载态）。"""
    fd = os.open(port, os.O_RDONLY | os.O_NONBLOCK)
    a = termios.tcgetattr(fd)
    a[0] = 0
    a[1] = 0
    a[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    a[3] = 0
    speed = getattr(termios, f"B{baud}")
    a[4] = speed
    a[5] = speed
    termios.tcsetattr(fd, termios.TCSANOW, a)
    fcntl.ioctl(fd, termios.TIOCMBIC, struct.pack("i", TIOCM_DTR | TIOCM_RTS))
    return fd


def report(buf, sec):
    """收尾自检：可打印率低即点名下一步（不静默返回一堆乱码）。"""
    n = len(buf)
    pr = sum(1 for c in buf if 32 <= c < 127 or c in (10, 13))
    ratio = pr / n * 100 if n else 0.0
    print(f"\n[console-log] {n}B / {sec:.1f}s printable={ratio:.1f}%", file=sys.stderr)
    if n and ratio < 70:
        print(
            "[console-log] 可打印率低：核对 --baud；固件侧 boot 横幅含 "
            "「console=uart0@…」自证行，读到即日志口正确",
            file=sys.stderr,
        )


def main():
    ap = argparse.ArgumentParser(description="RR3 板侧串口控制台采集（清 DTR/RTS）")
    ap.add_argument("-p", "--port", help="串口设备（缺省自动探测）")
    ap.add_argument("-b", "--baud", type=int, default=115200)
    ap.add_argument("-s", "--sec", type=float, default=0, help="采集秒数，0=直到 Ctrl-C")
    ap.add_argument("-o", "--out", help="同时落盘到文件")
    args = ap.parse_args()

    port = pick_port(args.port)
    fd = open_raw(port, args.baud)
    sink = open(args.out, "wb") if args.out else None
    stop = {"v": False}
    signal.signal(signal.SIGINT, lambda *_: stop.update(v=True))
    print(f"[console-log] {port} @{args.baud}（已清 DTR/RTS）", file=sys.stderr)

    t0 = time.time()
    buf = bytearray()
    try:
        while not stop["v"] and (args.sec <= 0 or time.time() - t0 < args.sec):
            try:
                chunk = os.read(fd, 8192)
            except BlockingIOError:
                time.sleep(0.005)
                continue
            except OSError as e:
                print(f"[console-log] 读失败: {e}", file=sys.stderr)
                break
            if not chunk:
                continue
            buf += chunk
            if sink:
                sink.write(chunk)
                # 逐块落盘：文件对象默认 8KB 缓冲，采集器被 SIGTERM/中断时
                # 最后一段日志会丢（2026-11-23 真板取证时实证），标定行不可丢。
                sink.flush()
            sys.stdout.write(chunk.decode("utf-8", "replace"))
            sys.stdout.flush()
    finally:
        os.close(fd)
        if sink:
            sink.close()
    report(buf, time.time() - t0)


if __name__ == "__main__":
    main()
