"""V4 舵机运动正确性探针——到位回读 / 连续动作不丢帧 / 双轴串行 / WANDER 让位。

四相设计（每相判据独立，任一失败都点名到具体相与角度）：

  A. 单轴到位回读：PAN/TILT 各四点扫行程，逐点下发后**轮询 INFO**（200ms 周期，
     上限 4s）直到 `pan=/tilt=` 等于目标——`st` 角度只由运行面在作业成功后镜像
     硬件真值（commands.rs PAN/TILT 注释口径），所以 INFO 等于目标 = 确实到位，
     不是控制面复述。记录时间到达标。
  B. 连续动作不丢帧：30 条 PAN 交替 75/105 以 50ms 间隔连发，全部应 ack OK；
     `push_job` 的同形状 Move 合批语义下最终角必须 = 最后一条（105）。
  C. SERVO 双轴串行：SERVO 70 85 → 双轮询 → pan=70 且 tilt=85。
  D. WANDER 让位：WANDER 开 1.5s → PAN 点名 → wander 必须自动 off 且 pan=90。

全程对账：起止各取一次 INFO 的 `job_drop/reply_drop`，增量非零即点名。
用法（Pi 上）::

    python3 -u servo_motion.py B1500000
"""
import os
import re
import sys
import time

from probe_common import baud_from_argv_or_env, open_tty, read_avail, drain_until_quiet

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)
#: 轮询 INFO 的周期与上限（SG90 0.1s/60° + 队列整定，4s 足够 120° 行程）
POLL_PERIOD = 0.2
POLL_CAP = 4.0
#: B 相连发条数与间隔
BURST_N = 30
BURST_GAP = 0.05

INFO_PAN_TILT = re.compile(r"pan=(\d+) tilt=(\d+)")
INFO_CNTRS = re.compile(r"job_drop=(\d+) reply_drop=(\d+)")
INFO_WANDER = re.compile(r"wander=(on|off)")


def line(fd, cmd, wait=0.8):
    """发一条行面命令（带空闲前导，与生产宿主同口径）并收回包文本。"""
    os.write(fd, b"\n")
    time.sleep(0.01)
    os.write(fd, (cmd + "\n").encode())
    out = b""
    t0 = time.time()
    while time.time() - t0 < wait:
        d = read_avail(fd, 0.05)
        if d:
            out += d
    return out.decode("utf-8", "replace")


def info(fd):
    """取一次 INFO 并解析 (pan, tilt, wander, job_drop, reply_drop)；失败返回 None。

    INFO 等待 0.25s：回包 <100ms 就到；等 1.0s 会把 poll_until 的"到位时间"底噪
    抬到 ~1s（两版实测全部 1016ms 的假读数真凶）。"""
    txt = line(fd, "INFO", 0.25)
    m = INFO_PAN_TILT.search(txt)
    if not m:
        return None
    pan, tilt = int(m.group(1)), int(m.group(2))
    w = INFO_WANDER.search(txt)
    c = INFO_CNTRS.search(txt)
    return pan, tilt, (w.group(1) == "on" if w else None), (
        (int(c.group(1)), int(c.group(2))) if c else (None, None)
    )


def poll_until(fd, axis_idx, target, cap=POLL_CAP):
    """轮询 INFO 直到指定轴（0=pan 1=tilt）等于 target；返回耗时秒，超时返回 None。"""
    t0 = time.time()
    while time.time() - t0 < cap:
        st = info(fd)
        if st and st[axis_idx] == target:
            return time.time() - t0
        time.sleep(POLL_PERIOD)
    return None


def phase_a(fd):
    """A 相：单轴四点扫行程 + 到位回读。返回失败数。"""
    print("=== A 相：单轴到位回读（PAN 90→60→120→90 / TILT 93→80→100→93）===")
    moves = [("PAN", 90, 0), ("PAN", 60, 0), ("PAN", 120, 0), ("PAN", 90, 0),
             ("TILT", 93, 1), ("TILT", 80, 1), ("TILT", 100, 1), ("TILT", 93, 1)]
    bad = 0
    for cmd, target, ax in moves:
        # ack 等待只给 0.3s：ack 通常 <100ms 回；首版等 1.0s 会把"到位时间"全部
        # 抬成 1016ms 的假读数（轮询第一次 INFO 时早已到位），量不出真实整定。
        ack = line(fd, f"{cmd} {target}", 0.3)
        if f"OK {cmd.lower()}={target}" not in ack:
            print(f"  ✗ {cmd} {target} ack 异常: {ack.strip()[:60]!r}")
            bad += 1
            continue
        dt = poll_until(fd, ax, target)
        if dt is None:
            st = info(fd)
            print(f"  ✗ {cmd} {target} 4s 未到位（INFO={st[:2] if st else None}）")
            bad += 1
        else:
            print(f"  ✓ {cmd} {target} 到位 {dt * 1000:.0f}ms")
    return bad


def phase_b(fd):
    """B 相：30 条 PAN 连发不丢帧。返回失败数。"""
    print(f"=== B 相：连续动作 {BURST_N} 条 PAN 75/105 交替 @{BURST_GAP * 1000:.0f}ms ===")
    acks = 0
    t0 = time.time()
    last = None
    for i in range(BURST_N):
        ang = 75 if i % 2 == 0 else 105
        last = ang
        out = line(fd, f"PAN {ang}", 0.15)
        if f"OK pan={ang}" in out:
            acks += 1
    print(f"  ack {acks}/{BURST_N}（连发 {time.time() - t0:.1f}s）")
    dt = poll_until(fd, 0, last)
    if acks != BURST_N:
        print(f"  ✗ ack 缺失 {BURST_N - acks} 条")
    if dt is None:
        print(f"  ✗ 连发后最终角未到 {last}")
        return 2 if acks == BURST_N else 1
    print(f"  ✓ 最终角 = {last}（合批语义正确，到位 {dt * 1000:.0f}ms）")
    return 0 if acks == BURST_N else 1


def phase_c(fd):
    """C 相：SERVO 双轴串行移动。返回失败数。"""
    print("=== C 相：SERVO 70 85（双轴）===")
    ack = line(fd, "SERVO 70 85", 0.3)
    if "OK pan=70 tilt=85" not in ack:
        print(f"  ✗ ack 异常: {ack.strip()[:60]!r}")
        return 1
    p = poll_until(fd, 0, 70)
    t = poll_until(fd, 1, 85)
    if p is None or t is None:
        print(f"  ✗ 双轴未到位 pan70:{p} tilt85:{t}")
        return 1
    print(f"  ✓ 双轴到位（pan {p * 1000:.0f}ms / tilt {t * 1000:.0f}ms）")
    return 0


def phase_d(fd):
    """D 相：WANDER 让位——外部点名优先，自主扫摆自动让位。返回失败数。"""
    print("=== D 相：WANDER on → 1.5s → PAN 90（点名让位）===")
    ack = line(fd, "WANDER", 0.8)
    if "OK wander on" not in ack:
        print(f"  ✗ WANDER ack 异常: {ack.strip()[:60]!r}")
        return 1
    time.sleep(1.5)
    st = info(fd)
    if not st or st[2] is not True:
        print(f"  ✗ WANDER 未生效: {st}")
        return 1
    ack = line(fd, "PAN 90", 1.0)
    if "OK pan=90" not in ack:
        print(f"  ✗ PAN ack 异常: {ack.strip()[:60]!r}")
        return 1
    dt = poll_until(fd, 0, 90)
    st = info(fd)
    if dt is None or not st or st[2] is not False:
        print(f"  ✗ 点名后 pan 未到位或 wander 未让位: dt={dt} st={st}")
        return 1
    print(f"  ✓ 点名让位（pan=90 @{dt * 1000:.0f}ms，wander 自动 off）")
    line(fd, "WANDER STOP", 0.8)  # 收尾归中清理
    return 0


def main():
    fd = open_tty(DEV, BAUD)
    drain_until_quiet(fd, 0.5, 2.0)
    print(f"=== V4 舵机运动正确性（{DEV} @ {BAUD}）===")
    line(fd, "AUDIO OFF", 0.5)  # 音频流会挤占控制面带宽，全程关
    line(fd, "WANDER STOP", 0.8)
    base = info(fd)
    if not base:
        raise SystemExit("INFO 无响应")
    print(f"  基线: pan={base[0]} tilt={base[1]} wander={base[2]} 计数={base[3]}")

    bad = phase_a(fd) + phase_b(fd) + phase_c(fd) + phase_d(fd)

    end = info(fd)
    djob = dend = None
    if end and base[3][0] is not None:
        djob = end[3][0] - base[3][0]
        dend = end[3][1] - base[3][1]
    print(f"=== 对账：job_drop 增量={djob} reply_drop 增量={dend} | 失败 {bad} 项 ===")
    if djob or dend:
        print("  ✗ 有丢作业/丢应答——上面各项结果不可信，先查链路")
        bad += 1
    print("V4 结论:", "PASS" if bad == 0 else f"FAIL（{bad}）")


if __name__ == "__main__":
    main()
