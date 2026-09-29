"""TTS 适配器（CosyVoice2）。实现随 M4 落地。"""

from dataclasses import dataclass

from rr3_brain.cloud import rr3_brain_cloud_api_key


@dataclass
class Rr3TtsClient:
    model: str = "FunAudioLLM/CosyVoice2-0.5B"

    def synthesize(self, text: str) -> bytes:
        rr3_brain_cloud_api_key()
        raise NotImplementedError("Rr3TtsClient.synthesize 随 M4 落地")
