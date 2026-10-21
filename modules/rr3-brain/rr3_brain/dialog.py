"""对话状态机：拾音 → 判停 → (ASR → LLM → TTS 的轮次编排归 __main__，见 ADR-0014 裁定 3)。

状态机只管"一轮 utterance 的边界"：
- **onset** 边沿起一轮，此后到达的 pcm 帧持续积累（s16le 小端字节）；
- **offset** 边沿判停（sense 的 VAD 已按 silence_tail_ms 判静默——brain 不重复一套
  静默计时，两本真值是错误形状）；
- **时长上限** max_utterance_ms 到点同样判停（继承 legacy asr_begin(max_seconds)
  的精神：连续语音无 offset 时也要有界）；
- 上限之内的 pcm 帧在无 utterance 时丢弃（不开轮不积累）。

判停后交出的 PCM = **onset 前的预滚**（至多 [`PREROLL_MS`]）+ onset 起到判停帧为止的
全部字节。预滚的必要性：sense 的 onset 事件要凑满 `min_speech_ms`（config 200ms）
连续有声才发出——这段「判据语音」本体在 onset 事件**之前**就已流过总线；无预滚则
每轮削头 200ms+，短句（~0.5s 的「你好」级）所剩过短，云端 ASR 对过短音频回空文本
（2026-10-21 真板 E2E 实测「识别为空文本」的健康流成因）。
"""

from __future__ import annotations

import base64
import time
from collections import deque

# 预滚窗长（机制量）：≥ sense 侧 min_speech_ms（200）+ 事件 transit 余量；
# 以帧自身 ms 字段计长，空帧（饥饿拍 pcm_b64=""）不入环不计时。
PREROLL_MS = 400


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
        # 预滚环：未开轮期间滚动保留最近 PREROLL_MS 的 pcm，onset 时拼回采集头。
        # 开轮期间的帧只进 buf（采集连续，预滚无职责）；判停复位后预滚照滚——
        # 下一轮 onset 拿到的永远是「此刻之前」的窗。
        self._preroll: deque[tuple[bytes, int]] = deque()
        self._preroll_ms = 0

    # -- 事件入口 --

    def rr3_feed_pcm(self, body: dict) -> None:
        """吃一帧 rr3-sense/audio/pcm（{"pcm_b64", "ms", ...}）。开轮中积累进
        utterance；未开轮进预滚环（超窗丢最旧）。空帧（饥饿拍）两边都不收。"""
        pcm = base64.b64decode(body.get("pcm_b64", ""))
        if not pcm:
            return
        if not self._active:
            ms = int(body.get("ms") or 0)
            self._preroll.append((pcm, ms))
            self._preroll_ms += ms
            while self._preroll_ms > PREROLL_MS and len(self._preroll) > 1:
                _, old_ms = self._preroll.popleft()
                self._preroll_ms -= old_ms
            return
        self._buf += pcm

    def rr3_feed_vad(self, body: dict) -> bool:
        """吃一帧 rr3-sense/audio/vad_start。返回 True = 本边沿触发判停。"""
        edge = body.get("edge")
        if edge == "onset":
            self._active = True
            self._buf = bytearray(b"".join(p for p, _ in self._preroll))
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
