"""云端适配器：ASR / LLM / TTS 三家的调用边界。

密钥**只**从环境变量 `SILICONFLOW_API_KEY` 读入（legacy/core/*/src/*_config.h 里
曾硬编码，那串值已视为泄露并在提供方轮换）。任何日志、异常文案都不得带出密钥值。
"""

import os

API_KEY_ENV = "SILICONFLOW_API_KEY"

__all__ = ["API_KEY_ENV", "rr3_brain_cloud_api_key"]


def rr3_brain_cloud_api_key() -> str:
    """取密钥；缺失即报错。错误文案只点名环境变量，不回显任何凭据。"""
    key = os.environ.get(API_KEY_ENV, "").strip()
    if not key:
        raise RuntimeError(f"缺少环境变量 {API_KEY_ENV}，云端链路不可用")
    return key
