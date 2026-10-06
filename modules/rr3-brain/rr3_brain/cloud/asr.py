"""ASR 适配器（SenseVoice）：整段 WAV 一次上传（ADR-0014 裁定 2；流式是 legacy ESP32
内存约束的产物，本机不复刻——见该 ADR"明确不做"第 3 条）。"""

import json
import os
import uuid

from rr3_brain.cloud import (
    CLOUD_MODE_MOCK,
    rr3_brain_cloud_api_key,
    rr3_brain_cloud_mode,
    rr3_brain_http_post,
    rr3_brain_wav_header,
)

MOCK_TEXT = "你好，今天天气怎么样"

# mock ASR 的文本注入键（批F2）：判向-转向闭环 e2e 用它注入"向左/向右"类
# 识别文本驱动意图面。取值不设/置空回落 MOCK_TEXT——mock 是测试注入点，
# 真云路径永不读它（与 rr3_brain_cloud_mode 的注入点分工同一纪律）。
MOCK_TEXT_ENV = "RR3_BRAIN_MOCK_ASR_TEXT"


class Rr3AsrClient:
    def __init__(self, base_url: str, model: str, timeout_s: float):
        self.base_url = base_url.rstrip("/")
        self.model = model
        self.timeout_s = timeout_s

    def transcribe(self, pcm: bytes, sample_rate: int = 16000) -> str:
        """PCM（s16le 单声道）→ 识别文本。云端失败/非 200 抛 RuntimeError（业务失败）。"""
        if rr3_brain_cloud_mode() == CLOUD_MODE_MOCK:
            return os.environ.get(MOCK_TEXT_ENV, "").strip() or MOCK_TEXT
        key = rr3_brain_cloud_api_key()
        wav = rr3_brain_wav_header(sample_rate, len(pcm)) + pcm
        boundary = uuid.uuid4().hex
        parts = [
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="model"\r\n\r\n'
            f"{self.model}\r\n".encode(),
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="file"; filename="a.wav"\r\n'
            f"Content-Type: audio/wav\r\n\r\n".encode(),
            wav,
            f"\r\n--{boundary}--\r\n".encode(),
        ]
        body = b"".join(parts)
        status, resp = rr3_brain_http_post(
            f"{self.base_url}/v1/audio/transcriptions", key, body,
            f"multipart/form-data; boundary={boundary}", self.timeout_s,
        )
        if status != 200:
            raise RuntimeError(f"识别接口 HTTP {status}")
        text = json.loads(resp).get("text")
        if not isinstance(text, str):
            raise RuntimeError("识别应答缺 text 字段")
        return text
