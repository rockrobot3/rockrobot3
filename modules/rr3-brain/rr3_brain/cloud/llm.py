"""LLM 适配器（DeepSeek-V3 via SiliconFlow）。实现随 M4 落地。"""

from dataclasses import dataclass

from rr3_brain.cloud import rr3_brain_cloud_api_key


@dataclass
class Rr3LlmClient:
    model: str = "deepseek-ai/DeepSeek-V3"

    def complete(self, prompt: str) -> str:
        rr3_brain_cloud_api_key()
        raise NotImplementedError("Rr3LlmClient.complete 随 M4 落地")
