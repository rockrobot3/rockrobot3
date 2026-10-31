"""帧面（128B 头 + CRC32）实机验证：rr3.call 主题点名 + rr3.set_bindings 注入。

用法: python3 frame_probe.py [termios 常量名，缺省 B1500000]
      换口用 env `RR3_PROBE_TTY`。

这是**板侧帧面的首次真机验证**——行面探针只能验行协议，帧面（128B 头 / payload
类型前缀 / 载荷 CRC32 / trace_id 回带 / dst 门）此前只有单测与假帧。

信封与板侧单点一致（`rr-periph-proto::host_call`）：`{"jsonrpc":"2.0","id":N,
"method":M,"params":P}`；请求帧头 = SEND / trace_id=0（direct）/ src=宿主 1 / dst=板 2；
payload = 2 字节 LE 类型前缀（Request=1）+ JSON。应答 = Response(2) / src=2 / dst=req src。

判据：
- 六个 rr3.call（舵机读/写/越界、表情写/非法、未知主题）必须逐条回 Response，
  且 `id` 回带、`payload_crc32` 与实算一致；
- `rr3.set_bindings` 十键注入必须 applied；随后行面 INFO 的 `selfheal_skip` 增量
  归零（注入前每拍必增，注入后立即停增——这是绑定链真正接通的判据）。
"""
import json
import os
import re
import sys
import time
import zlib

from probe_common import (
    HEADER_LEN, MAGIC, OFF_CRC32, OFF_PAYLOAD_LEN, baud_from_argv_or_env,
    flush_input, open_tty, pump,
)

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)

OP_SEND = 1
T_REQ, T_RESP = 1, 2
SRC_HOST, DST_PERIPH = 1, 2
TOPIC_SERVO = "rr3-act/servo/command"
TOPIC_FACE = "rr3-interact/face/expression"

#: 板侧十键绑定真值（与 board.rs / RR3_BOARD_BINDING_* 同值，字符串形态）
BINDINGS = {
    "i2c_sda": "21", "i2c_scl": "22", "adc_rail_pin": "34",
    "uart2_tx": "18", "uart2_rx": "19",
    "ledc_pan": "32", "ledc_tilt": "33",
    "i2s_bclk": "4", "i2s_sd": "27", "i2s_ws": "26",
}

fd = open_tty(DEV, BAUD)
flush_input(fd)
pump(fd, 1.2)

results = []


def build_frame(ty, body_obj, trace_id=0, dst=DST_PERIPH, src=SRC_HOST):
    """组一帧：128B 头（含载荷 CRC32）+ 2B 类型前缀 + JSON 体。"""
    js = json.dumps(body_obj, separators=(",", ":")).encode()
    payload = ty.to_bytes(2, "little") + js
    hdr = bytearray(HEADER_LEN)
    hdr[0:4] = MAGIC
    hdr[4:6] = OP_SEND.to_bytes(2, "little")
    hdr[8:16] = trace_id.to_bytes(8, "little")
    hdr[16:24] = int(time.time() * 1e9).to_bytes(8, "little")
    hdr[24:32] = src.to_bytes(8, "little")
    hdr[32:40] = dst.to_bytes(8, "little")
    hdr[40:44] = len(payload).to_bytes(4, "little")
    hdr[OFF_CRC32:OFF_CRC32 + 4] = (zlib.crc32(payload) & 0xFFFFFFFF).to_bytes(4, "little")
    return bytes(hdr) + payload


def parse_frames(raw):
    """从原始字节里拆出所有完整帧。返回 [(hdr_fields, body_bytes, crc_ok)]。"""
    out = []
    i = raw.find(MAGIC)
    n = len(raw)
    while i >= 0 and i + HEADER_LEN <= n:
        plen = int.from_bytes(raw[i + OFF_PAYLOAD_LEN:i + OFF_PAYLOAD_LEN + 4], "little")
        if plen == 0 or plen > 20000 or i + HEADER_LEN + plen > n:
            i = raw.find(MAGIC, i + 1)
            continue
        p = raw[i + HEADER_LEN:i + HEADER_LEN + plen]
        declared = int.from_bytes(raw[i + OFF_CRC32:i + OFF_CRC32 + 4], "little")
        out.append(({
            "trace_id": int.from_bytes(raw[i + 8:i + 16], "little"),
            "src": int.from_bytes(raw[i + 24:i + 32], "little"),
            "dst": int.from_bytes(raw[i + 32:i + 40], "little"),
            "opcode": int.from_bytes(raw[i + 4:i + 6], "little"),
        }, p, declared == (zlib.crc32(p) & 0xFFFFFFFF)))
        i = raw.find(MAGIC, i + HEADER_LEN + plen)
    return out


def wait_response(id_, cap=6.0):
    """等到 id 匹配的 Response 或 deadline。**不用固定窗口**——固定时长会把晚到的
    应答判成"没回"（这正是本探针首跑 3 条假失败的原因）。返回 (resp, 时延, 帧数)。"""
    raw = b""
    t0 = time.time()
    seen = 0
    while time.time() - t0 < cap:
        raw += pump(fd, 0.1)
        frames = parse_frames(raw)
        seen = max(seen, len(frames))
        for h, body, crc_ok in frames:
            if h["opcode"] != OP_SEND:
                continue
            if int.from_bytes(body[:2], "little") != T_RESP:
                continue
            try:
                v = json.loads(body[2:])
            except Exception:
                continue
            if v.get("id") == id_:
                return (v, crc_ok, h), time.time() - t0, seen, raw
    return None, time.time() - t0, seen, raw


def call(id_, method, params, expect, cap=6.0, note=""):
    """发一帧请求，按期望在应答体里判定（期望是 dict 时逐键比对，是 str 时正则）。"""
    # 判定实验用：`RR3_FRAME_GAP_MS` 给请求之间插间隔（缺省 0），用于区分
    # "帧边界时序问题"与"线面丢字节"——加间隔后失败消失 = 时序；不消失 = 物理层。
    gap = float(os.environ.get("RR3_FRAME_GAP_MS", "0")) / 1000.0
    if gap:
        time.sleep(gap)
    # 前导字节（缺省开）：空闲后首突发的首字节偶发失步，`\n` 替它挡枪——板面
    # 对孤立 `\n` 是空行静默跳过。生产宿主（rr-ipc `Rr3UartConn::tx_idle_preamble`）
    # 已在发送单点固化同一行为，本探针与其对齐；设 `RR3_FRAME_PREAMBLE=0` 可
    # 复现无前导的失步形态（判定实验：无前导 6 轮 2-3 条/10 失败，带前导 30/30）。
    if os.environ.get("RR3_FRAME_PREAMBLE", "1") == "1":
        os.write(fd, b"\n")
        time.sleep(0.01)
    os.write(fd, build_frame(T_REQ, {
        "jsonrpc": "2.0", "id": id_, "method": method, "params": params,
    }))
    resp, lat, seen, raw = wait_response(id_, cap)
    ok = False
    detail = ""
    if resp is None:
        # 取证：超时窗口里到底回了什么（空 = 板面没发；ERR 文本 = 首字节分流走了行面）
        detail = "%.2fs 内未收到 id=%d 的 Response（见到 %d 帧；原始 %dB: %r）" % (
            lat, id_, seen, len(raw), bytes(raw[:60]))
    else:
        v, crc_ok, h = resp
        if isinstance(expect, dict):
            ok = all(v.get("result", {}).get(k) == val for k, val in expect.items())
        else:
            ok = re.search(expect, json.dumps(v.get("result"), ensure_ascii=False)) is not None
        ok = ok and crc_ok
        detail = "%.2fs result=%s crc=%s trace=%d dst=%d" % (
            lat, json.dumps(v.get("result"), ensure_ascii=False)[:80],
            "ok" if crc_ok else "BAD", h["trace_id"], h["dst"])
    results.append((note or method + "/" + str(id_), ok, detail))
    print("[%s] %-22s %s%s" % ("PASS" if ok else "FAIL", note or method, detail,
                               "" if ok else "  << 期望 " + str(expect)[:60]))
    return resp


def line_text(sec=1.2):
    """同一端口上的行面收流（帧面/行面在板上按首字节分流，可混用）。"""
    return pump(fd, sec).decode("utf-8", "replace")


print("=== 帧面：rr3.call 舵机 ===")
call(11, "rr3.call", {"topic": TOPIC_SERVO, "args": {}}, {"move_ms": 0},
     note="舵机读")
call(12, "rr3.call", {"topic": TOPIC_SERVO, "args": {"axis": "pan", "angle_deg": 100}},
     {"angle_deg": 100, "move_ms": 0}, cap=6.0, note="舵机写 pan=100")
call(13, "rr3.call", {"topic": TOPIC_SERVO, "args": {"axis": "tilt", "angle_deg": 95}},
     {"angle_deg": 95}, cap=6.0, note="舵机写 tilt=95")
call(14, "rr3.call", {"topic": TOPIC_SERVO, "args": {"angle_deg": 130}},
     r"轴超出行程", note="行程越界拒绝")
call(15, "rr3.call", {"topic": TOPIC_SERVO, "args": {"axis": "roll", "angle_deg": 90}},
     r"axis 非法", note="非法轴拒绝")

print("=== 帧面：rr3.call 表情 ===")
call(16, "rr3.call", {"topic": TOPIC_FACE, "args": {"expression": "happy"}},
     {"expression": "happy"}, note="表情写 happy")
call(17, "rr3.call", {"topic": TOPIC_FACE, "args": {"expression": "nope"}},
     r"不在白名单", note="非法表情拒绝")
call(18, "rr3.call", {"topic": "rr3-act/nonexistent/thing", "args": {}},
     r"未知主题", note="未知主题拒绝")

print("=== 帧面：rr3.set_bindings 十键注入 ===")
call(19, "rr3.set_bindings", {"args": BINDINGS}, {"applied": True}, note="绑定注入")

# 注入前 selfheal_skip 每拍必增；注入后应立即停增（绑定链真正接通的判据）
os.write(fd, b"INFO\n")
before = re.search(r"selfheal_skip=(\d+)", line_text(1.2))
time.sleep(2.0)
os.write(fd, b"INFO\n")
after = re.search(r"selfheal_skip=(\d+)", line_text(1.2))
if before and after:
    d = int(after.group(1)) - int(before.group(1))
    ok = d == 0
    results.append(("selfheal 停增", ok, "增量 %d（注入后应 0）" % d))
    print("[%s] selfheal_skip 增量=%d（注入后应 0；注入前每拍必增）" % ("PASS" if ok else "FAIL", d))
else:
    results.append(("selfheal 停增", False, "INFO 缺字段"))
    print("[FAIL] INFO 缺 selfheal_skip 字段")

passed = sum(1 for _, ok, _ in results if ok)
print("\n通过 %d/%d" % (passed, len(results)))
os.close(fd)
sys.exit(0 if passed == len(results) else 1)
