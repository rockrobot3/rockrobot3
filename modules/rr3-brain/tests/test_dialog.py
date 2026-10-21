"""对话状态机单测：utterance 边界（onset 起轮 / offset 判停 / 时长上限 / 事件体形状）。"""

import base64
import unittest

from rr3_brain.dialog import Rr3Dialog, Rr3DialogTurn


def pcm_body(payload: bytes, ms: int = 32) -> dict:
    return {"seq": 1, "format": "s16le", "rate": 16000, "channels": 1, "ms": ms,
            "pcm_b64": base64.b64encode(payload).decode()}


class TestUtteranceBoundary(unittest.TestCase):
    def test_pcm_dropped_when_not_active(self):
        d = Rr3Dialog()
        d.rr3_feed_pcm(pcm_body(b"\x01\x02"))
        self.assertEqual(d.rr3_take_utterance(), b"")

    def test_onset_starts_and_offset_stops(self):
        d = Rr3Dialog()
        self.assertFalse(d.rr3_feed_vad({"seq": 1, "edge": "onset", "rms_peak": 1, "speech_ms": 0}))
        self.assertTrue(d.rr3_active)
        d.rr3_feed_pcm(pcm_body(b"\xaa\xbb"))
        self.assertTrue(d.rr3_feed_vad({"seq": 2, "edge": "offset", "speech_ms": 700}))
        pcm = d.rr3_take_utterance()
        self.assertEqual(pcm, b"\xaa\xbb")
        self.assertFalse(d.rr3_active)
        # 复位后：未开轮的 pcm 丢弃，take 交空
        d.rr3_feed_pcm(pcm_body(b"\xcc\xdd"))
        self.assertEqual(d.rr3_take_utterance(), b"")

    def test_offset_before_onset_does_not_stop(self):
        d = Rr3Dialog()
        self.assertFalse(d.rr3_feed_vad({"seq": 1, "edge": "offset", "speech_ms": 0}))
        self.assertFalse(d.rr3_active)

    def test_onset_resets_previous_buffer(self):
        d = Rr3Dialog()
        d.rr3_feed_vad({"edge": "onset"})
        d.rr3_feed_pcm(pcm_body(b"\x01\x02"))
        d.rr3_feed_vad({"edge": "onset"})  # 未判停的第二次 onset：重新起轮
        d.rr3_feed_pcm(pcm_body(b"\x03\x04"))
        d.rr3_feed_vad({"edge": "offset"})
        self.assertEqual(d.rr3_take_utterance(), b"\x03\x04")

    def test_duration_cap_expires(self):
        d = Rr3Dialog(max_utterance_ms=0)  # 0 上限：onset 即刻到点
        d.rr3_feed_vad({"edge": "onset"})
        d.rr3_feed_pcm(pcm_body(b"\x01\x02"))
        self.assertTrue(d.rr3_utterance_expired())
        self.assertEqual(d.rr3_take_utterance(), b"\x01\x02")
        self.assertFalse(d.rr3_utterance_expired())

    def test_unknown_edge_ignored(self):
        d = Rr3Dialog()
        self.assertFalse(d.rr3_feed_vad({"edge": "whatever"}))
        self.assertFalse(d.rr3_active)

    def test_preroll_prepended_at_onset(self):
        """onset 前的判据语音（min_speech 段）经预滚环拼回采集头，短句不削头。"""
        d = Rr3Dialog()
        d.rr3_feed_pcm(pcm_body(b"\x01\x02", ms=200))  # onset 前流过的帧（判据段）
        d.rr3_feed_pcm(pcm_body(b"\x03\x04", ms=200))
        d.rr3_feed_vad({"edge": "onset"})
        d.rr3_feed_pcm(pcm_body(b"\x05\x06", ms=32))  # onset 后第一帧
        d.rr3_feed_vad({"edge": "offset"})
        self.assertEqual(d.rr3_take_utterance(), b"\x01\x02\x03\x04\x05\x06")

    def test_preroll_capped_to_window(self):
        """预滚只留最近 PREROLL_MS，更早的帧丢最旧（窗有界， utterance 不虚胖）。"""
        from rr3_brain.dialog import PREROLL_MS

        d = Rr3Dialog()
        n_frames = 30  # 30 × 32ms = 960ms ≫ 400ms 窗
        for i in range(n_frames):
            d.rr3_feed_pcm(pcm_body(bytes([i + 1]), ms=32))
        d.rr3_feed_vad({"edge": "onset"})
        d.rr3_feed_vad({"edge": "offset"})
        pcm = d.rr3_take_utterance()
        # 稳态：每推一帧弹至 ≤400ms，环内恒保窗长内最完整的整帧组（384ms=12帧）
        n_kept = PREROLL_MS // 32
        self.assertEqual(pcm, bytes(range(n_frames - n_kept + 1, n_frames + 1)))

    def test_empty_frames_not_in_preroll(self):
        """饥饿空帧（pcm_b64=""）不入预滚不计时——窗长不被空拍虚占。"""
        d = Rr3Dialog()
        d.rr3_feed_pcm(pcm_body(b"\x01\x02", ms=200))
        d.rr3_feed_pcm(pcm_body(b"", ms=5000))  # 空帧带大 ms：不得挤掉真帧
        d.rr3_feed_vad({"edge": "onset"})
        d.rr3_feed_vad({"edge": "offset"})
        self.assertEqual(d.rr3_take_utterance(), b"\x01\x02")


class TestHistory(unittest.TestCase):
    def test_record_keeps_turns(self):
        d = Rr3Dialog()
        d.rr3_record(Rr3DialogTurn(user_text="你好", reply_text="你好呀", trace_id=1))
        self.assertEqual(len(d.history), 1)
        self.assertEqual(d.history[0].user_text, "你好")
        self.assertEqual(d.history[0].trace_id, 1)


if __name__ == "__main__":
    unittest.main()
