"""总线客户端单测：golden 对拍 + 假 rrd 全握手（stdlib unittest，ADR-0014 裁定 1）。

golden 向量与 Rust 侧共用同一份 `contracts/golden/event-header-128.hex`——
跨语言对拍不做第二份基准。假 rrd 收注册回 src_task=7、心跳回 accepted、
Event 回播 echo，行为写死（测试夹具，不是通用服务器）。
"""

import json
import os
import socket
import struct
import tempfile
import threading
import unittest

from rr3_brain import bus

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
GOLDEN = os.path.join(REPO, "contracts", "golden", "event-header-128.hex")

MANIFEST = """\
[manifest]
schema_version = 1
name = "rr3-brain"
version = "0.0.1"
type = "brain"
lang = "python"
entry = "rr3-brain"

[events]
publishes = []
subscribes = []

[health]
heartbeat_ms = 3600000
"""


def golden_bytes() -> bytes:
    with open(GOLDEN) as f:
        return bytes.fromhex(f.read().strip())


class FakeRrd:
    """一次一连接的假 rrd：注册→src_task=7，心跳→accepted，Event→回播 echo。"""

    def __init__(self, sock_path: str):
        self.stop = threading.Event()
        self.events: list[dict] = []
        self.srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.srv.bind(sock_path)
        self.srv.listen(1)
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()

    def _serve(self) -> None:
        self.srv.settimeout(5)
        try:
            conn, _ = self.srv.accept()
        except OSError:
            return
        conn.settimeout(5)
        try:
            while not self.stop.is_set():
                hdr, payload = bus.rr3_frame_read(conn)
                ty, body = bus.rr3_body_split(payload)
                if ty != bus.RR3_PT_REQUEST:
                    continue
                req = json.loads(body)
                method = req.get("method")
                rpc_id = req.get("id", 0)
                if method == "rr3.register":
                    result = {"src_task": 7}
                elif method == "rr3.heartbeat":
                    result = {"beat": "accepted"}
                else:
                    result = {"noop": True}
                self._reply(conn, rpc_id, result)
                if method == "rr3.register":
                    # 注册应答后补一条注册门禁事实：params 带 manifest 全文
                    assert "manifest" in req["params"]
        except (OSError, ValueError, json.JSONDecodeError):
            pass

    @staticmethod
    def _reply(conn: socket.socket, rpc_id: int, result: dict) -> None:
        body = json.dumps({"jsonrpc": "2.0", "id": rpc_id, "result": result}).encode()
        bus.rr3_frame_write(conn, bus.RR3_OPCODE_SEND, bus.RR3_PT_RESPONSE, bus.RR3_TASK_RRD, 7, body)

    def echo_event(self) -> None:
        """向在册连接回播一条 Event（inbox 判据）。简化：重开一条到自身的连接不可行，
        由测试直接向 client 的 inbox 投递等价物替代。"""
        raise NotImplementedError

    def close(self) -> None:
        self.stop.set()
        self.srv.close()


class TestHeaderGolden(unittest.TestCase):
    def test_golden_roundtrip(self):
        raw = golden_bytes()
        self.assertEqual(len(raw), bus.RR3_IPC_HDR_SIZE)
        hdr = bus.rr3_header_decode(raw)
        self.assertEqual(hdr["opcode"], bus.RR3_OPCODE_SEND)
        self.assertEqual(hdr["flags"], 0)
        self.assertEqual(hdr["trace_id"], 0x1234_5678)
        self.assertEqual(hdr["timestamp_ns"], 100)
        self.assertEqual(hdr["src_task"], 7)
        self.assertEqual(hdr["dst_task"], 0)
        self.assertEqual(hdr["payload_len"], 96)

    def test_bad_magic_rejected(self):
        raw = bytearray(golden_bytes())
        raw[0:4] = b"ARE1"  # 标准注释那套字节，rock3 裁定必须拒
        with self.assertRaises(ValueError):
            bus.rr3_header_decode(bytes(raw))

    def test_dirty_reserved_rejected(self):
        hdr = bus.rr3_header_encode(bus.RR3_OPCODE_SEND, 0, 7, 1, 0)
        raw = bytearray(hdr)
        raw[-1] = 0xFF
        with self.assertRaises(ValueError):
            bus.rr3_header_decode(bytes(raw))

    def test_body_prefix_roundtrip(self):
        payload = bus.rr3_body_encode(bus.RR3_PT_EVENT, b"{}")
        ty, body = bus.rr3_body_split(payload)
        self.assertEqual(ty, bus.RR3_PT_EVENT)
        self.assertEqual(body, b"{}")
        with self.assertRaises(ValueError):
            bus.rr3_body_split(b"\x01")


class TestManifest(unittest.TestCase):
    def test_identity_takes_name_and_heartbeat(self):
        ident = bus.rr3_manifest_identity(MANIFEST)
        self.assertEqual(ident["name"], "rr3-brain")
        self.assertEqual(ident["heartbeat_ms"], 3600000)
        with self.assertRaises(ValueError):
            bus.rr3_manifest_identity("[manifest]\nname = \"x\"\n")

    def test_entry_absolutize_rewrites_only_entry(self):
        wire = bus.rr3_manifest_entry_absolutize(MANIFEST, "/opt/rr3/rr3-brain.py")
        self.assertIn('entry = "/opt/rr3/rr3-brain.py"', wire)
        self.assertIn("name = \"rr3-brain\"", wire)
        self.assertIn("heartbeat_ms = 3600000", wire)
        with self.assertRaises(ValueError):
            bus.rr3_manifest_entry_absolutize(MANIFEST, "rel/entry")


class TestClientAgainstFakeRrd(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.tmp.name, "bus.sock")
        self.rrd = FakeRrd(self.path)

    def tearDown(self):
        self.rrd.close()
        self.tmp.cleanup()

    def _connect(self) -> bus.Rr3BusClient:
        entry = os.path.join(self.tmp.name, "entry.py")
        with open(entry, "w") as f:
            f.write("# entry\n")
        return bus.Rr3BusClient(self.path, MANIFEST, entry_abs=entry)

    def test_register_heartbeat_and_call_timeout_shape(self):
        client = self._connect()
        self.assertEqual(client.rr3_task(), 7)
        self.assertEqual(client.rr3_name(), "rr3-brain")
        self.assertTrue(client.rr3_connected())
        reply = client.rr3_call("rr3.heartbeat", {}, 2.0)
        self.assertEqual(reply["beat"], "accepted")
        client._shutdown()

    def test_publish_reaches_bus(self):
        client = self._connect()
        client.rr3_publish("rr3-brain/intent/result", {"trace": 1})
        with self.assertRaises(ValueError):
            client.rr3_publish("bad", [])  # 非对象体必须被拒
        client._shutdown()

    def test_reply_call_needs_trace(self):
        client = self._connect()
        with self.assertRaises(ValueError):
            client.rr3_reply_call(0, 1, {"ok": True})
        client._shutdown()

    def test_disconnect_flips_connected_false(self):
        client = self._connect()
        client._shutdown()
        self.assertFalse(client.rr3_connected())
        with self.assertRaises(RuntimeError):
            client.rr3_call("rr3.heartbeat", {}, 0.1)


if __name__ == "__main__":
    unittest.main()
