"""模块侧同步总线客户端（M4，ADR-0012 裁定 1）。

stdlib-only（socket / struct / json / threading / queue / tomllib），逐语义镜像
`base/runtime/rr-bus/src/client.rs`（Rust 侧唯一真值点）：

- 128B 定长消息头（Layout C）：偏移与 `contracts/envelope.schema.json` 的
  x-offset-table 一致，`contracts/golden/event-header-128.hex` 是跨语言共用的
  字节基准（tests/test_bus.py 逐字节对拍，不做第二份 golden）；
- payload = 2 字节小端类型前缀 + JSON 体（对位 rr-ipc `protocol::body`，
  `RR3_BODY_TYPE_PREFIX_LEN` = 2）；
- 并发形状：reader 线程独占读；应答按 JSON-RPC id 投 pending 表，其余帧进
  inbox 队列；写半边一把锁、一帧一次 `sendall`（整帧原子）。应答与广播在同一
  socket 上的相对次序不保证——客户端永远不假设帧序；
- 心跳线程按 manifest 的 heartbeat_ms 喂 `rr3.heartbeat`，失败 = stderr 一行 +
  `os._exit(1)`（模块不装"活着"，死交 supervisor 按 restart_policy 裁定）；
- 注册：首帧 `Request` + dst_task = RR3_TASK_RRD + 方法 `rr3.register`，params
  带 manifest 全文，`entry` 改写为本进程入口绝对路径（对位 Rust 侧
  `rr3_manifest_entry_absolutize` 的 current_exe；Python 侧用 sys.argv[0] 的
  realpath，`python -m rr3_brain` 时即 `__main__.py` 的绝对路径）。

密钥/凭据不入本文件；错误文案一律不回显线上数据全文。
"""

from __future__ import annotations

import json
import os
import queue
import socket
import struct
import sys
import threading
import time
import tomllib

# ---------------------------------------------------------------------------
# 线格式常量（与 rr-ipc protocol/header.rs 逐项对齐；改这里先改契约）
# ---------------------------------------------------------------------------

RR3_IPC_HDR_SIZE = 128
RR3_MAGIC = 0x4152_4531  # LE 线上字节 31 45 52 41
RR3_MAX_PAYLOAD_LEN = 64 * 1024 * 1024

RR3_OFF_OPCODE = 4
RR3_OFF_TRACE_ID = 8
RR3_OFF_TIMESTAMP_NS = 16
RR3_OFF_SRC_TASK = 24
RR3_OFF_DST_TASK = 32
RR3_OFF_PAYLOAD_LEN = 40
RR3_OFF_RESERVED = 44
RR3_LEN_RESERVED = 84

RR3_OPCODE_SEND = 0x0001

# 5 种 payload 类型，枚举值 L1 冻结（AOS-STD-IPC-005）
RR3_PT_REQUEST = 0x0001
RR3_PT_RESPONSE = 0x0002
RR3_PT_EVENT = 0x0003
RR3_PT_STREAM = 0x0004
RR3_PT_NOTIFICATION = 0x0005

RR3_BODY_TYPE_PREFIX_LEN = 2

RR3_TASK_RRD = 1
RR3_BROADCAST_DST_TASK = 0

RR3_BUS_PATH_ENV = "RR3_BUS_PATH"
RR3_UDS_PATH_DEFAULT = "/var/run/rock3-ipc.sock"
RR3_MANIFEST_ENV = "RR3_MANIFEST"

RR3_METHOD_CALL = "rr3.call"

RR3_CONNECT_TIMEOUT_S = 5.0
_RR3_HDR = struct.Struct("<IHHQQQQI")  # magic/opcode/flags/trace/ts/src/dst/len


def rr3_bus_now_ns() -> int:
    return time.time_ns()


def rr3_header_encode(opcode: int, trace_id: int, src_task: int, dst_task: int, payload_len: int) -> bytes:
    """128B 头编码（reserved 自动全 0；flags 恒 0——v0.0.1 不用任何位）。"""
    if payload_len > RR3_MAX_PAYLOAD_LEN:
        raise ValueError(f"payload_len 超上限 {RR3_MAX_PAYLOAD_LEN}: {payload_len}")
    buf = bytearray(RR3_IPC_HDR_SIZE)
    _RR3_HDR.pack_into(
        buf, 0, RR3_MAGIC, opcode, 0, trace_id, rr3_bus_now_ns(), src_task, dst_task, payload_len
    )
    return bytes(buf)


def rr3_header_decode(buf: bytes) -> dict:
    """128B 头解码。magic / reserved 非法即 ValueError（对位 Rr3HdrError）。"""
    if len(buf) < RR3_IPC_HDR_SIZE:
        raise ValueError(f"头不足 {RR3_IPC_HDR_SIZE} 字节: {len(buf)}")
    magic, opcode, flags, trace_id, ts_ns, src_task, dst_task, payload_len = _RR3_HDR.unpack_from(buf, 0)
    if magic != RR3_MAGIC:
        raise ValueError(f"magic 不符: {magic:#x}")
    if any(buf[RR3_OFF_RESERVED : RR3_OFF_RESERVED + RR3_LEN_RESERVED]):
        raise ValueError("reserved 区非全 0")
    if payload_len > RR3_MAX_PAYLOAD_LEN:
        raise ValueError(f"payload_len 超上限: {payload_len}")
    return {
        "opcode": opcode,
        "flags": flags,
        "trace_id": trace_id,
        "timestamp_ns": ts_ns,
        "src_task": src_task,
        "dst_task": dst_task,
        "payload_len": payload_len,
    }


def rr3_body_encode(payload_type: int, body: bytes) -> bytes:
    return payload_type.to_bytes(RR3_BODY_TYPE_PREFIX_LEN, "little") + body


def rr3_body_split(payload: bytes) -> tuple[int, bytes]:
    if len(payload) < RR3_BODY_TYPE_PREFIX_LEN:
        raise ValueError(f"payload 不足类型前缀 {RR3_BODY_TYPE_PREFIX_LEN} 字节")
    ty = int.from_bytes(payload[:RR3_BODY_TYPE_PREFIX_LEN], "little")
    return ty, payload[RR3_BODY_TYPE_PREFIX_LEN:]


def rr3_frame_bytes(opcode: int, payload_type: int, src_task: int, dst_task: int, body: bytes, trace_id: int = 0) -> bytes:
    """整帧字节（头 + 类型前缀 + JSON 体）。"""
    payload = rr3_body_encode(payload_type, body)
    head = rr3_header_encode(opcode, trace_id, src_task, dst_task, len(payload))
    return head + payload


def rr3_frame_read(sock: socket.socket) -> tuple[dict, bytes]:
    """读一整帧：128B 头 + payload_len 字节。对端先断（EOF）时读不满即抛 OSError。"""
    head = _recv_exact(sock, RR3_IPC_HDR_SIZE)
    hdr = rr3_header_decode(head)
    payload = _recv_exact(sock, hdr["payload_len"])
    return hdr, payload


def rr3_frame_write(sock: socket.socket, opcode: int, payload_type: int, src_task: int, dst_task: int, body: bytes, trace_id: int = 0) -> None:
    sock.sendall(rr3_frame_bytes(opcode, payload_type, src_task, dst_task, body, trace_id))


def _recv_exact(sock: socket.socket, n: int) -> bytes:
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise OSError("对端已关闭（EOF）")
        buf.extend(chunk)
    return bytes(buf)


# ---------------------------------------------------------------------------
# manifest 身份（对位 rr3_manifest_identity / rr3_manifest_entry_absolutize）
# ---------------------------------------------------------------------------


def rr3_manifest_identity(text: str) -> dict:
    """从 manifest 文本取 name + heartbeat_ms。校验是 rrd 的事，客户端只取自用两字段。"""
    root = tomllib.loads(text)
    try:
        name = root["manifest"]["name"]
        heartbeat_ms = root["health"]["heartbeat_ms"]
    except (KeyError, TypeError) as e:
        raise ValueError(f"manifest 缺身份字段（{e}）") from e
    if not isinstance(name, str) or not isinstance(heartbeat_ms, int) or heartbeat_ms < 0:
        raise ValueError("manifest 的 name / heartbeat_ms 类型非法")
    return {"name": name, "heartbeat_ms": heartbeat_ms}


def rr3_manifest_entry_absolutize(text: str, entry_abs: str) -> str:
    """把 manifest.entry 改写成绝对路径（注册门禁），其余字段原样保留。"""
    if not os.path.isabs(entry_abs):
        raise ValueError(f"entry 绝对化失败：{entry_abs} 不是绝对路径")
    # tomllib 只读；门禁只看 entry 一行，按行改写并保留其余原文（含注释）
    out = []
    seen = False
    for line in text.splitlines(keepends=True):
        stripped = line.strip()
        if stripped.startswith("entry") and "=" in stripped:
            key, _, _ = stripped.partition("=")
            out.append(f"{key}= \"{entry_abs}\"\n")
            seen = True
        else:
            out.append(line)
    if not seen:
        raise ValueError("manifest 缺 [manifest].entry")
    return "".join(out)


# ---------------------------------------------------------------------------
# 总线客户端
# ---------------------------------------------------------------------------


class Rr3BusClient:
    """一站式连接：读环境变量 → 取身份 → 绝对化 entry → 连接 → 注册 → 起心跳。"""

    def __init__(self, path: str, manifest_text: str, entry_abs: str | None = None):
        identity = rr3_manifest_identity(manifest_text)
        if entry_abs is None:
            entry_abs = os.path.realpath(sys.argv[0] or ".")
        wire = rr3_manifest_entry_absolutize(manifest_text, entry_abs)

        self._name = identity["name"]
        self._task = 0
        self._stop = threading.Event()
        self._pending: dict[int, queue.Queue] = {}
        self._pending_lock = threading.Lock()
        self._inbox: queue.Queue = queue.Queue()
        self._next_id = 0
        self._id_lock = threading.Lock()

        self._sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._sock.connect(path)
        self._write_lock = threading.Lock()

        # reader 必须先于注册帧上路：注册应答也走 pending 路由
        self._reader = threading.Thread(target=self._reader_loop, name="rr3-bus-reader", daemon=True)
        self._reader.start()

        reply = self._call_locked(0, "rr3.register", {"name": self._name, "manifest": wire}, RR3_CONNECT_TIMEOUT_S)
        task = reply.get("src_task") if isinstance(reply, dict) else None
        if not isinstance(task, int):
            self._shutdown()
            raise RuntimeError("注册应答缺 result.src_task")
        self._task = task

        self._heartbeat = threading.Thread(
            target=self._heartbeat_loop, args=(max(identity["heartbeat_ms"], 1) / 1000.0,),
            name="rr3-bus-heartbeat", daemon=True,
        )
        self._heartbeat.start()

    # -- 身份 --

    def rr3_task(self) -> int:
        return self._task

    def rr3_name(self) -> str:
        return self._name

    def rr3_connected(self) -> bool:
        """死亡探针：reader 在 = 活。轮询型主循环每圈探一次，假了走退出语义。"""
        return not self._stop.is_set()

    # -- 上行 RPC（一次一答；心跳线程也走这里） --

    def rr3_call(self, method: str, params: dict, timeout_s: float) -> dict:
        if self._stop.is_set():
            raise RuntimeError("总线已断开")
        return self._call_locked(self._task, method, params, timeout_s)

    def rr3_call_topic(self, topic: str, args: dict, timeout_s: float) -> dict:
        return self.rr3_call(RR3_METHOD_CALL, {"topic": topic, "args": args}, timeout_s)

    def _call_locked(self, src_task: int, method: str, params: dict, timeout_s: float) -> dict:
        with self._id_lock:
            self._next_id += 1
            rpc_id = self._next_id
        box: queue.Queue = queue.Queue(maxsize=1)
        with self._pending_lock:
            self._pending[rpc_id] = box
        body = json.dumps({"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params}).encode()
        try:
            self._write_frame(RR3_PT_REQUEST, src_task, RR3_TASK_RRD, body)
        except OSError:
            with self._pending_lock:
                self._pending.pop(rpc_id, None)
            raise
        try:
            reply = box.get(timeout=timeout_s)
        except queue.Empty:
            with self._pending_lock:
                self._pending.pop(rpc_id, None)
            raise RuntimeError(f"RPC {method} 等应答超时（{timeout_s}s）") from None
        if isinstance(reply, RuntimeError):
            raise reply
        return reply

    # -- 业务发布 / 应答 --

    def rr3_publish(self, topic: str, body: dict) -> None:
        if not isinstance(body, dict):
            raise ValueError("Event 体必须是 JSON 对象")
        body = dict(body)
        body["topic"] = topic
        self._write_frame(RR3_PT_EVENT, self._task, RR3_TASK_RRD, json.dumps(body).encode())

    def rr3_reply_call(self, trace_id: int, rpc_id: int, result: dict) -> None:
        if trace_id == 0:
            raise ValueError("trace_id=0 的请求没有回路，无从应答")
        body = json.dumps({"jsonrpc": "2.0", "id": rpc_id, "result": result}).encode()
        self._write_frame(RR3_PT_RESPONSE, self._task, RR3_TASK_RRD, body, trace_id=trace_id)

    # -- 收帧 --

    def rr3_poll(self) -> list[dict]:
        """非阻塞排空 inbox。每帧 = {"header": 头dict, "type": payload类型, "body": 解析后的JSON}。"""
        out = []
        while True:
            try:
                out.append(self._inbox.get_nowait())
            except queue.Empty:
                return out

    def rr3_poll_wait(self, timeout_s: float) -> list[dict]:
        """带等待的收帧：先等第一帧（至多 timeout），再非阻塞排空余量。
        断开时立即返回空 list（与 Rust 侧 rr3_poll_wait 同语义）。"""
        out = []
        try:
            out.append(self._inbox.get(timeout=timeout_s if not self._stop.is_set() else 0))
        except queue.Empty:
            if self._stop.is_set():
                return out
        return out + self.rr3_poll()

    @staticmethod
    def rr3_frame_body(frame: dict) -> tuple[int, dict]:
        ty, body = rr3_body_split(frame["payload"])
        try:
            return ty, json.loads(body)
        except json.JSONDecodeError as e:
            raise ValueError(f"帧体不是 JSON: {e}") from e

    # -- 内部 --

    def _write_frame(self, payload_type: int, src_task: int, dst_task: int, body: bytes, trace_id: int = 0) -> None:
        frame = rr3_frame_bytes(RR3_OPCODE_SEND, payload_type, src_task, dst_task, body, trace_id)
        with self._write_lock:
            self._sock.sendall(frame)

    def _reader_loop(self) -> None:
        try:
            while not self._stop.is_set():
                hdr, payload = rr3_frame_read(self._sock)
                ty, body = rr3_body_split(payload)
                if ty == RR3_PT_RESPONSE:
                    v = json.loads(body)
                    rpc_id = v.get("id", 0)
                    with self._pending_lock:
                        box = self._pending.pop(rpc_id, None)
                    if box is not None:
                        if "result" in v:
                            box.put(v["result"])
                        else:
                            box.put(RuntimeError(f"RPC 被拒: {v.get('error')}"))
                    # 无主应答：超时已被调用方记账，静默丢弃
                else:
                    self._inbox.put({"header": hdr, "payload": payload, "type": ty, "body_raw": body})
        except (OSError, ValueError, json.JSONDecodeError):
            pass  # EOF / 坏帧：统一按断开处理
        finally:
            self._stop.set()
            with self._pending_lock:
                pending = list(self._pending.values())
                self._pending.clear()
            for box in pending:
                box.put(RuntimeError("总线已断开"))

    def _heartbeat_loop(self, interval_s: float) -> None:
        while not self._stop.wait(interval_s):
            try:
                self.rr3_call("rr3.heartbeat", {}, 5.0)
            except (RuntimeError, OSError):
                if not self._stop.is_set():
                    print("rr3-bus: 心跳失败，退出（supervisor 按 restart_policy 裁定）", file=sys.stderr)
                    os._exit(1)

    def _shutdown(self) -> None:
        self._stop.set()
        try:
            self._sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        self._sock.close()


def rr3_bus_connect(manifest_path: str, path: str | None = None) -> Rr3BusClient:
    """模块 main 的第一行：读 RR3_MANIFEST（rrd spawn 注入）→ 连接注册起心跳。"""
    if path is None:
        path = os.environ.get(RR3_BUS_PATH_ENV, RR3_UDS_PATH_DEFAULT)
    with open(manifest_path, "rb") as f:
        manifest_text = f.read().decode()
    return Rr3BusClient(path, manifest_text)
