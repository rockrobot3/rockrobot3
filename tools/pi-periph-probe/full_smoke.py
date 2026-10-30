"""板侧全功能冒烟（行面 11 条命令逐条断言 + 边界/交叉项）。

用法: python3 full_smoke.py [termios 常量名，缺省 B1500000]
      换口用 env `RR3_PROBE_TTY`。

覆盖的功能链（板固件 commands.rs 的 11 个 match 臂 + 边界）：
- 显示面：OLED 文本、16 种 FACE 表情（白名单逐个下发）、非法表情应拒
- 运动面：PAN / TILT / SERVO 双轴、行程越界应拒、SCAN 五点扫描（串行 pan→tilt）
- 自主面：WANDER 开 / STOP 关
- 观测面：RAIL 单次采样（异步回包）、INFO 观测字段全在
- 语音面：AUDIO ON / OFF（真流由 rr3-audio-rec 另测，本探针只验控制面）

判据：每条回包必须命中期望正则；SCAN 必须恰好 5 个 `SCAN point=` 且以
`OK scan done` 结尾；RAIL 必须回报 `RAIL rail=<mv>mV`。末行打印
`通过 N/M` 与**判据集 digest**（每步 命令:是否命中 的 sha256）——跨次/跨档
跑同一判据集，digest 相同即行为一致（回包里的 mV/计数器等易变值不进 digest）。
"""
import hashlib
import os
import re
import sys

from probe_common import baud_from_argv_or_env, flush_input, open_tty, pump

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)

#: FACE 白名单（与固件 topics_gen::RR3_FACE_EXPRS 同值；16 个）
FACE_EXPRS = [
    "idle", "blink", "talk", "think", "happy", "listen", "turn", "sleepy",
    "surprised", "angry", "sad", "confused", "focus", "love", "dizzy", "scared",
]

fd = open_tty(DEV, BAUD)
flush_input(fd)
pump(fd, 1.2)  # 吸收启动/上一会话残留

results = []   # (cmd, ok, text)


def text(sec):
    """收 `sec` 秒的行面文本（UTF-8 容错解码）。"""
    return pump(fd, sec).decode("utf-8", "replace")


def ask(cmd, pattern, wait=1.2, note=""):
    """发一条命令、收 `wait` 秒、按期望正则判定。"""
    os.write(fd, (cmd + "\n").encode())
    got = text(wait)
    ok = re.search(pattern, got) is not None
    results.append((cmd, ok, got))
    tag = "PASS" if ok else "FAIL"
    print("[%s] %-14s %s%s" % (tag, cmd, note, "" if ok else "｜实收: " + repr(got.strip()[:120])))
    return got


print("=== 显示面 ===")
ask("HELP", r"CMDS:")
ask("OLED SMOKE", r'OK oled "SMOKE"')
for e in FACE_EXPRS:
    ask("FACE " + e, r"OK face=%s" % e, wait=0.6)
ask("FACE nope", r"ERR expression 不在白名单", wait=0.8)
ask("OLED", r"ERR OLED 缺文本参数", wait=0.8)

print("=== 运动面 ===")
ask("PAN 100", r"OK pan=100")
ask("TILT 95", r"OK tilt=95")
# 两级拒绝判据要分开测：>180 是**解析期**拒绝（角度超范围），在 0-180 内但出轴行程
# 是**行程期**拒绝（轴超出行程）。两条都有，才能真正覆盖 fail-closed 的两条腿。
ask("PAN 200", r"ERR 角度超范围")
ask("PAN 130", r"ERR pan 轴超出行程")
ask("TILT 50", r"ERR tilt 轴超出行程")
ask("SERVO 90 90", r"OK pan=90 tilt=90", wait=2.5)  # 串行 pan→tilt，整定需时间

print("=== 自主面 ===")
ask("WANDER", r"OK wander on")
ask("WANDER STOP", r"OK wander off")

print("=== SCAN（串行五点，约 10s）===")
os.write(fd, b"SCAN\n")
scan = text(16.0)
points = re.findall(r"SCAN point=(\d+)", scan)
ok_pts = len(points) == 5 and sorted(points) == ["1", "2", "3", "4", "5"]
ok_done = re.search(r"OK scan done", scan) is not None
results.append(("SCAN", ok_pts and ok_done, scan))
print("[%s] SCAN 点数=%d 收尾=%s%s" % (
    "PASS" if ok_pts and ok_done else "FAIL", len(points), bool(ok_done),
    "" if ok_pts and ok_done else "｜实收: " + repr(scan.strip()[-160:])))

print("=== 观测面 ===")
# RAIL 是异步回包：ack 与采样结果可能在**同一次收流**里到齐，也可能分两次。
# 必须把 ask() 那段文本也算进来，否则结果被 ack 那段收走、下一段收空（实测踩过）。
rail_ack = ask("RAIL", r"OK rail sample queued", wait=0.8)
rail = rail_ack + text(3.0)
ok_rail = re.search(r"RAIL rail=\d+mV", rail) is not None
results.append(("RAIL 回包", ok_rail, rail))
print("[%s] RAIL 回包 %s" % ("PASS" if ok_rail else "FAIL",
                            re.search(r"RAIL rail=\d+mV raw=\d+", rail).group(0) if ok_rail else repr(rail.strip()[:120])))

info = ask("INFO", r"INFO pan=\d+ tilt=\d+ lim=pan\[", wait=1.5)
# INFO 字段齐全性（观测面缺字段 = 诊断能力下降，按失败计）
info_fields = re.search(
    r"seq=\d+ sent=\d+ drop=\d+ q=\d+ job_drop=\d+ reply_drop=\d+", info)
results.append(("INFO 字段齐全", info_fields is not None, info))
print("[%s] INFO 计数段 %s" % (
    "PASS" if info_fields else "FAIL",
    info_fields.group(0) if info_fields else "缺失"))

print("=== 语音面（仅控制面）===")
ask("AUDIO ON", r"OK audio on", wait=0.8)
text(1.0)  # 让真流跑一会儿（本探针不解析帧，帧完整性由 bw_probe 判）
ask("AUDIO OFF", r"OK audio off", wait=1.2)

# ---- 汇总 ----
passed = sum(1 for _, ok, _ in results if ok)
total = len(results)
digest = hashlib.sha256(
    "\n".join("%s:%d" % (c, 1 if ok else 0) for c, ok, _ in results).encode()
).hexdigest()
print("\n通过 %d/%d ｜ 判据集 digest=%s" % (passed, total, digest[:16]))
os.close(fd)
sys.exit(0 if passed == total else 1)
