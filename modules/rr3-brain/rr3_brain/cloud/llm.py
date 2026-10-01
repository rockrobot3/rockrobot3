"""LLM 适配器（DeepSeek-V3 via SiliconFlow）：多轮对话带历史记忆。

历史策略移植 legacy/core/llm/src/llm.c：system + 最近 LLM_HISTORY_ROUNDS 轮
（一轮 = user + assistant 两条），裁剪只动最旧的成对话轮。
"""

import json

from rr3_brain.cloud import (
    CLOUD_MODE_MOCK,
    LLM_HISTORY_ROUNDS,
    SYSTEM_PROMPT,
    rr3_brain_cloud_api_key,
    rr3_brain_cloud_mode,
    rr3_brain_http_post,
)

LLM_MAX_TOKENS = 150  # llm_config.h 原值


class Rr3LlmClient:
    def __init__(self, base_url: str, model: str, timeout_s: float):
        self.base_url = base_url.rstrip("/")
        self.model = model
        self.timeout_s = timeout_s
        self.history: list[dict] = [{"role": "system", "content": SYSTEM_PROMPT}]

    def complete(self, prompt: str) -> str:
        """一轮补全：追加 user →（带历史）请求 → 回文本并追加 assistant。"""
        if rr3_brain_cloud_mode() == CLOUD_MODE_MOCK:
            reply = f"（mock 回复）{prompt}"
            self.history.append({"role": "user", "content": prompt})
            self.history.append({"role": "assistant", "content": reply})
            return reply
        key = rr3_brain_cloud_api_key()
        self.history.append({"role": "user", "content": prompt})
        while len(self.history) > 1 + LLM_HISTORY_ROUNDS * 2:
            del self.history[1:3]
        body = json.dumps({
            "model": self.model,
            "messages": self.history,
            "max_tokens": LLM_MAX_TOKENS,
        }).encode()
        status, resp = rr3_brain_http_post(
            f"{self.base_url}/v1/chat/completions", key, body,
            "application/json", self.timeout_s,
        )
        if status != 200:
            self.history.pop()  # 失败轮不入账（对位 legacy：只有成功才追加 assistant）
            raise RuntimeError(f"大模型接口 HTTP {status}")
        try:
            reply = json.loads(resp)["choices"][0]["message"]["content"]
        except (KeyError, IndexError, json.JSONDecodeError) as e:
            self.history.pop()
            raise RuntimeError(f"大模型应答形状不符: {e}") from e
        self.history.append({"role": "assistant", "content": reply})
        return reply
