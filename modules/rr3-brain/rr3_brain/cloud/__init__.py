"""云端适配器：ASR / LLM / TTS 三家的调用边界（ADR-0014 裁定 2）。

线上契约逐项对齐 legacy/core/{asr,llm,tts}（SiliconFlow /v1）：
- ASR  POST {base}/v1/audio/transcriptions   multipart(model + WAV) → {"text": …}
- LLM  POST {base}/v1/chat/completions       {model, messages, max_tokens} → choices[0].message.content
- TTS  POST {base}/v1/audio/speech           {model, input, voice, response_format, sample_rate} → 原始 PCM

密钥**只**从环境变量 `SILICONFLOW_API_KEY` 读入（legacy/core/*/src/*_config.h 里
曾硬编码，那串值已视为泄露并在提供方轮换）。任何日志、异常文案都不得带出密钥值。

mock 后端：环境变量 `RR3_BRAIN_CLOUD=mock`（默认 `real`）时三家换确定性行为——
测试注入点，不是产品路径；缺 key 不静默降级，真云调用照报缺环境变量。
HTTP 一律 stdlib `urllib`（零第三方依赖，pyproject dependencies 不动）。
"""

import base64
import json
import math
import os
import struct
import urllib.error
import urllib.request

API_KEY_ENV = "SILICONFLOW_API_KEY"
CLOUD_MODE_ENV = "RR3_BRAIN_CLOUD"
CLOUD_MODE_REAL = "real"
CLOUD_MODE_MOCK = "mock"

# legacy/core/llm/src/llm_config.h 的原值移植（M4 不改人设与参数）
SYSTEM_PROMPT = "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，不要用列表、符号和表情。"
LLM_HISTORY_ROUNDS = 4
TTS_VOICE_SUFFIX = ":bella"  # 女声 bella；男声改 alex（tts_config.h）

__all__ = [
    "API_KEY_ENV", "CLOUD_MODE_ENV", "CLOUD_MODE_REAL", "CLOUD_MODE_MOCK",
    "SYSTEM_PROMPT", "LLM_HISTORY_ROUNDS",
    "rr3_brain_cloud_api_key", "rr3_brain_cloud_mode",
    "rr3_brain_http_post", "rr3_brain_wav_header", "rr3_brain_sine_pcm",
]


def rr3_brain_cloud_api_key() -> str:
    """取密钥；缺失即报错。错误文案只点名环境变量，不回显任何凭据。"""
    key = os.environ.get(API_KEY_ENV, "").strip()
    if not key:
        raise RuntimeError(f"缺少环境变量 {API_KEY_ENV}，云端链路不可用")
    return key


def rr3_brain_cloud_mode() -> str:
    mode = os.environ.get(CLOUD_MODE_ENV, CLOUD_MODE_REAL).strip().lower()
    if mode not in (CLOUD_MODE_REAL, CLOUD_MODE_MOCK):
        raise RuntimeError(f"{CLOUD_MODE_ENV} 取值不认: {mode}（允许 {CLOUD_MODE_REAL}/{CLOUD_MODE_MOCK}）")
    return mode


def rr3_brain_http_post(url: str, key: str, body: bytes, content_type: str, timeout_s: float) -> tuple[int, bytes]:
    """一次 POST；返回 (status, body)。HTTPError 也带回状态码与错误体（调用方折业务失败）。"""
    req = urllib.request.Request(
        url, data=body, method="POST",
        headers={"Authorization": f"Bearer {key}", "Content-Type": content_type},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout_s) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def rr3_brain_wav_header(sample_rate: int, data_len: int) -> bytes:
    """44B WAV 头（PCM s16le 单声道），字节序对齐 legacy asr.c 的 build_wav_header。"""
    return b"RIFF" + struct.pack("<I", 36 + data_len) + b"WAVEfmt " + struct.pack(
        "<IHHIIHH", 16, 1, 1, sample_rate, sample_rate * 2, 2, 16
    ) + b"data" + struct.pack("<I", data_len)


def rr3_brain_sine_pcm(text: str, sample_rate: int, ms_per_char: int = 80) -> bytes:
    """mock TTS 的确定性 PCM：440Hz 正弦，时长随文本长度（s16le）。"""
    n = max(len(text.strip()), 1)
    total = int(sample_rate * n * ms_per_char / 1000)
    pcm = bytearray()
    for i in range(total):
        v = int(12000 * math.sin(2 * math.pi * 440.0 * i / sample_rate))
        pcm.extend(struct.pack("<h", v))
    return bytes(pcm)


def rr3_brain_b64_pcm(samples_b64: str) -> bytes:
    return base64.b64decode(samples_b64)
