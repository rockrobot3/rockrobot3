"""0.0.6 模型连接抽象批单测：连接行解析 + 密钥双源 + local 免鉴权请求形状。

纯逻辑测试（零真网络）：http_post 以 monkeypatch urlopen 捕获请求头（真函数
真路径，只替传输层）；secrets 文件落 tmp。环境变量一律 setUp 备份 / tearDown
还原——测试面不得污染宿主 shell。
"""

import json
import os
import tempfile
import unittest
import urllib.request

from rr3_brain.cloud import (
    API_KEY_ENV,
    CLOUD_MODE_ENV,
    CLOUD_MODE_REAL,
    SECRETS_FILE_ENV,
    rr3_brain_http_post,
    rr3_brain_secrets_get,
)
from rr3_brain.cloud.llm import Rr3LlmClient
from rr3_brain.composite import rr3_brain_model_row


def _cfg(rows: list | None = None) -> dict:
    """最小 cfg：cloud 段给旧路径锚点，model 表按需注入。"""
    brain: dict = {
        "cloud": {"base_url": "https://legacy.example", "llm_model": "legacy-model", "timeout_s": 5}
    }
    if rows is not None:
        brain["model"] = rows
    return {"rr3-brain": brain}


def _row(**over) -> dict:
    row = {
        "name": "测试行", "mode": "api", "api_format": "openai",
        "base_url": "https://api.example", "model_id": "m-1",
        "api_key_ref": "env:RR3_TEST_KEY", "active": True,
    }
    row.update(over)
    return row


class Rr3ModelRowTest(unittest.TestCase):
    """[[rr3-brain.model]] 解析：恰一 active、白名单 fail-closed、旧路径回退。"""

    def test_no_table_falls_back_to_cloud_section(self):
        base, model, key_ref, mode = rr3_brain_model_row(_cfg())
        self.assertEqual(
            (base, model, key_ref, mode),
            ("https://legacy.example", "legacy-model", None, None),
        )

    def test_active_row_carries_connection(self):
        base, model, key_ref, mode = rr3_brain_model_row(_cfg([_row()]))
        self.assertEqual(
            (base, model, key_ref, mode),
            ("https://api.example", "m-1", "env:RR3_TEST_KEY", "api"),
        )

    def test_zero_or_two_active_rows_fail_closed(self):
        for rows in ([_row(active=False)], [_row(), _row(name="第二行")]):
            with self.assertRaisesRegex(RuntimeError, "active"):
                rr3_brain_model_row(_cfg(rows))

    def test_bad_mode_and_format_fail_closed(self):
        with self.assertRaisesRegex(RuntimeError, "mode"):
            rr3_brain_model_row(_cfg([_row(mode="grpc")]))
        with self.assertRaisesRegex(RuntimeError, "api_format"):
            rr3_brain_model_row(_cfg([_row(api_format="anthropic")]))

    def test_missing_endpoint_fields_fail_closed(self):
        with self.assertRaisesRegex(RuntimeError, "base_url"):
            rr3_brain_model_row(_cfg([_row(base_url="")]))
        with self.assertRaisesRegex(RuntimeError, "model_id"):
            rr3_brain_model_row(_cfg([_row(model_id="")]))

    def test_local_mode_row_passes_without_key(self):
        _, _, key_ref, mode = rr3_brain_model_row(
            _cfg([_row(mode="local", api_key_ref=None)])
        )
        self.assertEqual((key_ref, mode), (None, "local"))


class Rr3KeyRefTest(unittest.TestCase):
    """密钥双源解析：env: 前缀 / 裸名 secrets / local 免鉴权 / 旧路径回退。"""

    def setUp(self):
        self._saved = {
            k: os.environ.get(k)
            for k in (CLOUD_MODE_ENV, API_KEY_ENV, "RR3_TEST_KEY", SECRETS_FILE_ENV)
        }
        os.environ.pop(CLOUD_MODE_ENV, None)
        os.environ.pop(API_KEY_ENV, None)
        os.environ.pop("RR3_TEST_KEY", None)
        os.environ.pop(SECRETS_FILE_ENV, None)

    def tearDown(self):
        for k, v in self._saved.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v

    def test_env_ref_resolves_and_reports_missing_by_name(self):
        client = Rr3LlmClient("https://api.example", "m", 5.0, key_ref="env:RR3_TEST_KEY")
        os.environ["RR3_TEST_KEY"] = "sk-test-value"
        self.assertEqual(client._resolve_key(), "sk-test-value")
        del os.environ["RR3_TEST_KEY"]
        with self.assertRaisesRegex(RuntimeError, "RR3_TEST_KEY"):
            client._resolve_key()

    def test_bare_ref_reads_secrets_file_and_misses_by_ref_name(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "secrets.json")
            with open(path, "w") as f:
                json.dump({"keys": {"面板测试键": "sk-panel-value"}}, f)
            os.environ[SECRETS_FILE_ENV] = path
            client = Rr3LlmClient("https://api.example", "m", 5.0, key_ref="面板测试键")
            self.assertEqual(client._resolve_key(), "sk-panel-value")
            missing = Rr3LlmClient("https://api.example", "m", 5.0, key_ref="没有的键")
            with self.assertRaisesRegex(RuntimeError, "没有的键"):
                missing._resolve_key()

    def test_local_mode_needs_no_key(self):
        client = Rr3LlmClient(
            "http://127.0.0.1:11434", "qwen3:4b", 5.0,
            key_ref="env:RR3_CLOUD_API_KEY", mode="local",
        )
        self.assertEqual(client._resolve_key(), "")

    def test_none_ref_keeps_legacy_env_key(self):
        os.environ[API_KEY_ENV] = "sk-legacy"
        client = Rr3LlmClient("https://api.example", "m", 5.0)
        self.assertEqual(client._resolve_key(), "sk-legacy")
        del os.environ[API_KEY_ENV]
        with self.assertRaisesRegex(RuntimeError, API_KEY_ENV):
            client._resolve_key()


class Rr3SecretsReaderTest(unittest.TestCase):
    """rr3_brain_secrets_get：现读、缺文件/坏形状一律 None、不抛。"""

    def setUp(self):
        self._saved = os.environ.get(SECRETS_FILE_ENV)

    def tearDown(self):
        if self._saved is None:
            os.environ.pop(SECRETS_FILE_ENV, None)
        else:
            os.environ[SECRETS_FILE_ENV] = self._saved

    def test_reads_ref_value(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "secrets.json")
            with open(path, "w") as f:
                json.dump({"keys": {"r1": "sk-v", "空": "   "}}, f)
            os.environ[SECRETS_FILE_ENV] = path
            self.assertEqual(rr3_brain_secrets_get("r1"), "sk-v")
            self.assertIsNone(rr3_brain_secrets_get("没有"))  # 缺键
            self.assertIsNone(rr3_brain_secrets_get("空"))  # 空白值视同缺

    def test_missing_file_and_bad_shapes_are_none(self):
        with tempfile.TemporaryDirectory() as d:
            os.environ[SECRETS_FILE_ENV] = os.path.join(d, "没有.json")
            self.assertIsNone(rr3_brain_secrets_get("r"))
            bad = os.path.join(d, "bad.json")
            with open(bad, "w") as f:
                f.write("不是json")
            os.environ[SECRETS_FILE_ENV] = bad
            self.assertIsNone(rr3_brain_secrets_get("r"))
            shape = os.path.join(d, "shape.json")
            with open(shape, "w") as f:
                json.dump({"keys": "不是字典"}, f)
            os.environ[SECRETS_FILE_ENV] = shape
            self.assertIsNone(rr3_brain_secrets_get("r"))
            num = os.path.join(d, "num.json")
            with open(num, "w") as f:
                json.dump({"keys": {"r": 123}}, f)
            os.environ[SECRETS_FILE_ENV] = num
            self.assertIsNone(rr3_brain_secrets_get("r"))


class _FakeResp:
    status = 200

    def read(self) -> bytes:
        return b"{}"

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False


class Rr3HttpPostHeaderTest(unittest.TestCase):
    """http_post 头形状：空 key 不带 Authorization（local 免鉴权）；有 key 带 Bearer。"""

    def _capture(self, key: str) -> urllib.request.Request:
        captured: dict = {}

        def fake_urlopen(req, timeout=None):
            captured["req"] = req
            return _FakeResp()

        original = urllib.request.urlopen
        urllib.request.urlopen = fake_urlopen
        try:
            status, _ = rr3_brain_http_post(
                "http://stub/v1/chat/completions", key, b"{}",
                "application/json", 1.0,
            )
        finally:
            urllib.request.urlopen = original
        self.assertEqual(status, 200)
        return captured["req"]

    def test_empty_key_omits_auth_header(self):
        req = self._capture("")
        self.assertNotIn("Authorization", req.headers)

    def test_non_empty_key_sends_bearer(self):
        req = self._capture("sk-x")
        self.assertEqual(req.headers.get("Authorization"), "Bearer sk-x")


class Rr3LocalCompleteShapeTest(unittest.TestCase):
    """local 行全链形状：真 complete 走打桩 http_post——key 空串、URL 契约、回文本。"""

    def test_local_complete_posts_empty_key(self):
        import rr3_brain.cloud.llm as llm_mod

        captured: dict = {}

        def fake_post(url, key, body, content_type, timeout_s):
            captured.update(url=url, key=key, body=body)
            return 200, json.dumps({"choices": [{"message": {"content": "好的"}}]}).encode()

        os.environ[CLOUD_MODE_ENV] = CLOUD_MODE_REAL
        original = llm_mod.rr3_brain_http_post
        llm_mod.rr3_brain_http_post = fake_post
        try:
            client = Rr3LlmClient("http://127.0.0.1:11434", "qwen3:4b", 5.0, mode="local")
            self.assertEqual(client.complete("在吗"), "好的")
        finally:
            llm_mod.rr3_brain_http_post = original
            os.environ.pop(CLOUD_MODE_ENV, None)
        self.assertEqual(captured["key"], "")
        self.assertTrue(captured["url"].endswith("/v1/chat/completions"))
        self.assertEqual(json.loads(captured["body"])["model"], "qwen3:4b")


if __name__ == "__main__":
    unittest.main()
