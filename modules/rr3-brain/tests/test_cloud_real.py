"""真云 opt-in 冒烟：ASR / LLM / TTS 三客户端各对 SiliconFlow 发一发（C4）。

opt-in 纪律：只有环境变量 `RR3_CLOUD_API_KEY` 在场才执行，否则整模块 skip——
日常单测与 CI 零网络零红（`python3 -m unittest discover -s tests` 照绿）；
有 key 时才真连。base_url / 模型名从 config.toml 读入（复用 composite 的
`rr3_config_load`，`RR3_BRAIN_CONFIG` 优先语义同生产）——本测试同时是
config 真值的守门员：base_url 必须是主机根（客户端自拼 /v1/<路径>）。

断言面只证「端点 / 模型 / 密钥 / HTTP 形状四件在真云上走通」：ASR/LLM 回
文本 str、TTS 回非空 PCM。不锁语义内容（云侧行为不归本仓管），任何断言与
输出都不得回显密钥。real 档在 setUp 显式落位（云适配器缺省即 real，写明是
防外层套件泄漏 mock 档），tearDown 复原现场。
"""

import os
import unittest

from rr3_brain.cloud import (
    API_KEY_ENV,
    CLOUD_MODE_ENV,
    CLOUD_MODE_REAL,
    rr3_brain_sine_pcm,
)
from rr3_brain.cloud.asr import Rr3AsrClient
from rr3_brain.cloud.llm import Rr3LlmClient
from rr3_brain.cloud.tts import Rr3TtsClient
from rr3_brain.composite import rr3_config_load

# ASR 喂 2s 正弦（"你好"×8 × 125ms）：真云识别出什么文本不论，只证链路通
_SINE_TEXT = "你好" * 8
_SINE_MS_PER_CHAR = 125


@unittest.skipUnless(
    os.environ.get(API_KEY_ENV, "").strip(),
    f"缺 {API_KEY_ENV}，真云冒烟 skip（opt-in）",
)
class TestRealCloudSmoke(unittest.TestCase):
    def setUp(self):
        self._old_mode = os.environ.get(CLOUD_MODE_ENV)
        os.environ[CLOUD_MODE_ENV] = CLOUD_MODE_REAL
        cloud = rr3_config_load()["rr3-brain"]["cloud"]
        self.base_url = cloud["base_url"]
        self.timeout_s = float(cloud["timeout_s"])
        # 契约守门：base_url 是主机根，带 /v1 会拼出 /v1/v1/... 必 404
        self.assertFalse(
            self.base_url.rstrip("/").endswith("/v1"),
            f"config base_url 须为主机根（客户端自拼 /v1）: {self.base_url}",
        )

    def tearDown(self):
        if self._old_mode is None:
            os.environ.pop(CLOUD_MODE_ENV, None)
        else:
            os.environ[CLOUD_MODE_ENV] = self._old_mode

    def test_asr_transcribes_sine_to_text(self):
        client = Rr3AsrClient(self.base_url, _cloud("asr_model"), self.timeout_s)
        text = client.transcribe(rr3_brain_sine_pcm(_SINE_TEXT, 16000, _SINE_MS_PER_CHAR))
        self.assertIsInstance(text, str)

    def test_llm_completes_short_prompt(self):
        client = Rr3LlmClient(self.base_url, _cloud("llm_model"), self.timeout_s)
        reply = client.complete("在吗")
        self.assertIsInstance(reply, str)
        self.assertTrue(reply.strip(), "LLM 应答为空")

    def test_tts_synthesizes_nonempty_pcm(self):
        client = Rr3TtsClient(self.base_url, _cloud("tts_model"), self.timeout_s)
        pcm = client.synthesize("好的")
        self.assertIsInstance(pcm, (bytes, bytearray))
        self.assertGreater(len(pcm), 0, "TTS 应答为空")


def _cloud(key: str) -> str:
    return rr3_config_load()["rr3-brain"]["cloud"][key]


if __name__ == "__main__":
    unittest.main()
