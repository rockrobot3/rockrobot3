"""TTS 适配器（CosyVoice2）：文本 → PCM 字节流（s16le）。

M4 的 PCM 是验证性消费（非空 + 时长换算入日志）；真播放随 M5 板上 ALSA sink
（ADR-0014 裁定 3）。响应无 Content-Length、服务端发完即断（read-until-close），
urllib 的 resp.read() 天然就是这个语义。
"""

import json

from rr3_brain.cloud import (
    CLOUD_MODE_MOCK,
    TTS_VOICE_SUFFIX,
    rr3_brain_cloud_api_key,
    rr3_brain_cloud_mode,
    rr3_brain_http_post,
    rr3_brain_sine_pcm,
)


class Rr3TtsClient:
    def __init__(self, base_url: str, model: str, timeout_s: float):
        self.base_url = base_url.rstrip("/")
        self.model = model
        self.timeout_s = timeout_s

    def synthesize(self, text: str, sample_rate: int = 16000) -> bytes:
        if rr3_brain_cloud_mode() == CLOUD_MODE_MOCK:
            return rr3_brain_sine_pcm(text, sample_rate)
        key = rr3_brain_cloud_api_key()
        body = json.dumps({
            "model": self.model,
            "input": text,
            "voice": self.model + TTS_VOICE_SUFFIX,
            "response_format": "pcm",
            "sample_rate": sample_rate,
        }).encode()
        status, resp = rr3_brain_http_post(
            f"{self.base_url}/v1/audio/speech", key, body,
            "application/json", self.timeout_s,
        )
        if status != 200:
            raise RuntimeError(f"合成接口 HTTP {status}")
        return resp
