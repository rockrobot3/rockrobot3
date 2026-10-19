"""LLM 适配器（0.0.6 模型连接抽象批）：OpenAI Chat Completions 形状，任意供应商。

连接参数（base_url/model_id/密钥引用）由 config `[[rr3-brain.model]]` 行承载——
填"格式/地址/模型名"即可接 DeepSeek 官方 / SiliconFlow / 通义或本地 Ollama 等
OpenAI 兼容服务，换云只改配置不改码。历史策略移植 legacy/core/llm/src/llm.c：
system + 最近 LLM_HISTORY_ROUNDS 轮（一轮 = user + assistant 两条），裁剪只动
最旧的成对话轮。

密钥双源（ref 语法，面板维护面见 cloud/__init__ 模块 docstring）：
- `env:VAR` → 环境变量（缺省 None = 通用键 `RR3_CLOUD_API_KEY` 先例不破）；
- 裸名 → secrets 文件（面板 upsert，每次调用现读，填完下一轮生效）；
- 行 `mode="local"` → 免鉴权：请求**不带 Authorization 头**，不解析任何 key。
错误文案只点名 ref/环境变量名，任何路径都不得带出密钥值。
"""

from __future__ import annotations  # 注解不求值（PEP 604 形在 3.9 亦 import 安全）

import json
import os

from rr3_brain.cloud import (
    CLOUD_MODE_MOCK,
    LLM_HISTORY_ROUNDS,
    SYSTEM_PROMPT,
    rr3_brain_cloud_api_key,
    rr3_brain_cloud_mode,
    rr3_brain_http_post,
    rr3_brain_secrets_get,
)

LLM_MAX_TOKENS = 150  # llm_config.h 原值
LLM_MODE_API = "api"
LLM_MODE_LOCAL = "local"
KEY_REF_ENV_PREFIX = "env:"


class Rr3LlmClient:
    def __init__(self, base_url: str, model: str, timeout_s: float,
                 key_ref: str | None = None, mode: str | None = None):
        self.base_url = base_url.rstrip("/")
        self.model = model
        self.timeout_s = timeout_s
        self.key_ref = key_ref
        self.mode = mode  # None=随全局档（RR3_BRAIN_CLOUD）；"local"=免鉴权
        self.history: list[dict] = [{"role": "system", "content": SYSTEM_PROMPT}]

    def _resolve_key(self) -> str:
        """本轮密钥；local 免鉴权回空串（http_post 空串 → 无 Authorization 头）。"""
        if self.mode == LLM_MODE_LOCAL:
            return ""
        if self.key_ref is None:
            return rr3_brain_cloud_api_key()  # 旧路径：通用环境变量键
        if self.key_ref.startswith(KEY_REF_ENV_PREFIX):
            var = self.key_ref[len(KEY_REF_ENV_PREFIX):]
            val = os.environ.get(var, "").strip()
            if not val:
                raise RuntimeError(f"缺少环境变量 {var}（模型行 key_ref={self.key_ref}）")
            return val
        val = rr3_brain_secrets_get(self.key_ref)
        if not val:
            raise RuntimeError(
                f"secrets 缺引用 {self.key_ref}（面板填 key 后下一轮生效）"
            )
        return val

    def complete(self, prompt: str) -> str:
        """一轮补全：追加 user →（带历史）请求 → 回文本并追加 assistant。"""
        if rr3_brain_cloud_mode() == CLOUD_MODE_MOCK:
            reply = f"（mock 回复）{prompt}"
            self.history.append({"role": "user", "content": prompt})
            self.history.append({"role": "assistant", "content": reply})
            return reply
        key = self._resolve_key()
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
