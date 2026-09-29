"""ASR 适配器（SenseVoice）。实现随 M4 落地，本文件先钉接口与密钥来源。"""

from dataclasses import dataclass

from rr3_brain.cloud import rr3_brain_cloud_api_key


@dataclass
class Rr3AsrClient:
    model: str = "iic/SenseVoiceSmall"

    def transcribe(self, pcm: bytes) -> str:
        rr3_brain_cloud_api_key()
        raise NotImplementedError("Rr3AsrClient.transcribe 随 M4 落地")
