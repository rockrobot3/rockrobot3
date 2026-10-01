"""对话状态机：拾音 → 判停 → (ASR → LLM → TTS 的轮次编排归 __main__，见 ADR-0012 裁定 3)。

状态机只管"一轮 utterance 的边界"：
- **onset** 边沿起一轮，此后到达的 pcm 帧持续积累（s16le 小端字节）；
- **offset** 边沿判停（sense 的 VAD 已按 silence_tail_ms 判静默——brain 不重复一套
  静默计时，两本真值是错误形状）；
- **时长上限** max_utterance_ms 到点同样判停（继承 legacy asr_begin(max_seconds)
  的精神：连续语音无 offset 时也要有界）；
- 上限之内的 pcm 帧在无 utterance 时丢弃（不开轮不积累）。

判停后交出的 PCM 含 onset 前的预滚（onset 那一帧起算）到判停帧为止的全部字节。
"""

from __future__ import annotations

import base64
import time


class Rr3DialogTurn:
    """一轮对话的载体。"""

    __slots__ = ("user_text", "reply_text", "trace_id")

    def __init__(self, user_text: str = "", reply_text: str = "", trace_id: int = 0):
        self.user_text = user_text
        self.reply_text = reply_text
        self.trace_id = trace_id


class Rr3Dialog:
    """utterance 边界状态机（纯逻辑，不碰网络不碰总线）。

    `rr3_feed_pcm(body)` / `rr3_feed_vad(body)` 吃总线事件体（rr3-sense 的
    rr3_pcm_event_body / rr3_vad_event_body 形状）；判停时 `rr3_take_utterance`
    一次性交出累积的 PCM 字节并复位。
    """

    def __init__(self, max_utterance_ms: int = 8000):
        self.max_utterance_ms = max_utterance_ms
        self.history: list[Rr3DialogTurn] = []
        self._active = False
        self._buf = bytearray()
        self._started_ms = 0.0

    # -- 事件入口 --

    def rr3_feed_pcm(self, body: dict) -> None:
        """吃一帧 rr3-sense/audio/pcm（{"pcm_b64", "ms", ...}）。开轮中才积累。"""
        if not self._active:
            return
        self._buf += base64.b64decode(body.get("pcm_b64", ""))

    def rr3_feed_vad(self, body: dict) -> bool:
        """吃一帧 rr3-sense/audio/vad_start。返回 True = 本边沿触发判停。"""
        edge = body.get("edge")
        if edge == "onset":
            self._active = True
            self._buf = bytearray()
            self._started_ms = time.monotonic() * 1000
            return False
        if edge == "offset" and self._active:
            return True
        return False

    def rr3_utterance_expired(self) -> bool:
        """开轮中且时长上限到点（主循环每圈问一次）。"""
        return self._active and (time.monotonic() * 1000 - self._started_ms) >= self.max_utterance_ms

    # -- 判停出口 --

    def rr3_take_utterance(self) -> bytes:
        """交出本轮 PCM 并复位。未开轮时交空串（调用方应先看判停条件）。"""
        pcm = bytes(self._buf)
        self._buf = bytearray()
        self._active = False
        return pcm

    @property
    def rr3_active(self) -> bool:
        return self._active

    # -- 历史登记 --

    def rr3_record(self, turn: Rr3DialogTurn) -> None:
        self.history.append(turn)
