"""云客户端单测：WAV/multipart 形状、mock 路径、真 HTTP 路径（本地假 SiliconFlow）。

真路径用 `http.server` 起一个本地假端点（记录请求、回罐头应答），base_url 指向它——
覆盖 urllib 调用面与应答解析，不碰外网；密钥经环境变量注入（仓里不落任何凭据）。
"""

import json
import os
import struct
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

from rr3_brain.cloud import (
    API_KEY_ENV,
    CLOUD_MODE_ENV,
    LLM_HISTORY_ROUNDS,
    rr3_brain_cloud_mode,
    rr3_brain_http_post,
    rr3_brain_sine_pcm,
    rr3_brain_wav_header,
)
from rr3_brain.cloud.asr import Rr3AsrClient
from rr3_brain.cloud.llm import Rr3LlmClient
from rr3_brain.cloud.tts import Rr3TtsClient


class _Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        server = self.server
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        server.requests.append((self.path, self.headers.get("Authorization", ""), body))
        if server.fail_path and self.path == server.fail_path:
            self.send_response(500)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"error":"boom"}')
            return
        if self.path == "/v1/audio/transcriptions":
            out = b'{"text": "\xe4\xbd\xa0\xe5\xa5\xbd"}'  # 你好
        elif self.path == "/v1/chat/completions":
            out = json.dumps({
                "choices": [{"message": {"role": "assistant", "content": "好的"}}]
            }).encode()
        else:  # /v1/audio/speech → 原始 PCM
            out = b"\x01\x00" * 1600
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


class FakeSiliconFlow(HTTPServer):
    """一次一请求的假端点：记录 (path, headers, body)，按 path 回罐头。"""

    def __init__(self):
        super().__init__(("127.0.0.1", 0), _Handler)
        self.requests: list[tuple[str, str, bytes]] = []
        self.fail_path: str | None = None


class TestPureHelpers(unittest.TestCase):
    def test_wav_header_shape(self):
        hdr = rr3_brain_wav_header(16000, 3200)
        self.assertEqual(len(hdr), 44)
        self.assertEqual(hdr[:4], b"RIFF")
        self.assertEqual(struct.unpack_from("<I", hdr, 4)[0], 36 + 3200)
        self.assertEqual(hdr[8:16], b"WAVEfmt ")
        self.assertEqual(struct.unpack_from("<H", hdr, 20)[0], 1)  # PCM
        self.assertEqual(struct.unpack_from("<I", hdr, 24)[0], 16000)
        self.assertEqual(hdr[36:40], b"data")
        self.assertEqual(struct.unpack_from("<I", hdr, 40)[0], 3200)

    def test_sine_pcm_deterministic_and_sized(self):
        a = rr3_brain_sine_pcm("你好", 16000, ms_per_char=80)
        b = rr3_brain_sine_pcm("你好", 16000, ms_per_char=80)
        self.assertEqual(a, b)
        self.assertEqual(len(a) % 2, 0)
        self.assertGreater(len(a), 0)


class TestCloudMode(unittest.TestCase):
    def test_mode_parse(self):
        old = os.environ.get(CLOUD_MODE_ENV)
        try:
            os.environ[CLOUD_MODE_ENV] = "mock"
            self.assertEqual(rr3_brain_cloud_mode(), "mock")
            os.environ[CLOUD_MODE_ENV] = "bogus"
            with self.assertRaises(RuntimeError):
                rr3_brain_cloud_mode()
        finally:
            if old is None:
                os.environ.pop(CLOUD_MODE_ENV, None)
            else:
                os.environ[CLOUD_MODE_ENV] = old


class TestRealPathAgainstFakeEndpoint(unittest.TestCase):
    def setUp(self):
        self.srv = FakeSiliconFlow()
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.base = f"http://127.0.0.1:{self.srv.server_address[1]}"
        os.environ[API_KEY_ENV] = "test-key"

    def tearDown(self):
        self.srv.shutdown()
        os.environ.pop(API_KEY_ENV, None)
        os.environ.pop(CLOUD_MODE_ENV, None)

    def test_asr_sends_multipart_with_model_and_wav(self):
        os.environ[CLOUD_MODE_ENV] = "real"
        client = Rr3AsrClient(self.base, "iic/SenseVoiceSmall", 5.0)
        text = client.transcribe(b"\x00\x01" * 16)
        self.assertEqual(text, "你好")
        path, auth, body = self.srv.requests[-1]
        self.assertEqual(path, "/v1/audio/transcriptions")
        self.assertEqual(auth, "Bearer test-key")
        self.assertIn(b'name="model"', body)
        self.assertIn(b"iic/SenseVoiceSmall", body)
        self.assertIn(b"RIFF", body)  # WAV 头仓内自建
        self.assertIn(b'filename="a.wav"', body)

    def test_llm_history_trim_and_failure_rollback(self):
        os.environ[CLOUD_MODE_ENV] = "real"
        client = Rr3LlmClient(self.base, "deepseek-ai/DeepSeek-V3", 5.0)
        for i in range(LLM_HISTORY_ROUNDS + 2):
            self.assertEqual(client.complete(f"问{i}"), "好的")
        # system + 最近 4 轮（一轮两条）
        self.assertEqual(len(client.history), 1 + LLM_HISTORY_ROUNDS * 2)
        self.assertEqual(client.history[0]["role"], "system")
        self.assertEqual(client.history[1]["content"], "问2")
        path, _, body = self.srv.requests[-1]
        self.assertEqual(path, "/v1/chat/completions")
        self.assertEqual(json.loads(body)["max_tokens"], 150)
        # 失败轮不入账：user 先入列、失败后回滚（裁剪 -2：失败轮的 user 一并出列）
        before = len(client.history)
        self.srv.fail_path = "/v1/chat/completions"
        with self.assertRaises(RuntimeError):
            client.complete("会失败")
        self.srv.fail_path = None
        self.assertEqual(len(client.history), before - 2)

    def test_tts_returns_pcm_stream(self):
        os.environ[CLOUD_MODE_ENV] = "real"
        client = Rr3TtsClient(self.base, "FunAudioLLM/CosyVoice2-0.5B", 5.0)
        pcm = client.synthesize("好的", sample_rate=16000)
        self.assertEqual(pcm, b"\x01\x00" * 1600)
        path, _, body = self.srv.requests[-1]
        self.assertEqual(path, "/v1/audio/speech")
        req = json.loads(body)
        self.assertEqual(req["voice"], "FunAudioLLM/CosyVoice2-0.5B:bella")
        self.assertEqual(req["response_format"], "pcm")
        self.assertEqual(req["sample_rate"], 16000)


class TestMockPath(unittest.TestCase):
    def setUp(self):
        os.environ[CLOUD_MODE_ENV] = "mock"
        # mock 路径不要求 key：显式清掉，证明不静默取 key
        os.environ.pop(API_KEY_ENV, None)

    def tearDown(self):
        os.environ.pop(CLOUD_MODE_ENV, None)

    def test_mock_asr_fixed_text_without_key(self):
        client = Rr3AsrClient("http://127.0.0.1:1", "m", 1.0)
        self.assertEqual(client.transcribe(b"\x00\x00"), "你好，今天天气怎么样")

    def test_mock_llm_echo_and_history(self):
        client = Rr3LlmClient("http://127.0.0.1:1", "m", 1.0)
        self.assertEqual(client.complete("在吗"), "（mock 回复）在吗")
        self.assertEqual(client.history[-1]["content"], "（mock 回复）在吗")

    def test_mock_tts_sine_without_key(self):
        client = Rr3TtsClient("http://127.0.0.1:1", "m", 1.0)
        pcm = client.synthesize("hi")
        self.assertGreater(len(pcm), 0)


if __name__ == "__main__":
    unittest.main()
