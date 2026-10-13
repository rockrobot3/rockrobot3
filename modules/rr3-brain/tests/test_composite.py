"""组合件纯函数单测：TTS 下行流分帧形状（MC-1）。

只测 `rr3_tts_pcm_frames`/`rr3_publish_tts_pcm` 的组装语义（逐字段复用上行
`rr3-sense/audio/pcm` 已定的形状、20ms 一块、seq 模块级单调），不碰总线与云端。
依赖面与 test_bus 同款：`from rr3_brain import composite`（PYTHONPATH 指向包目录）。
"""

import base64
import unittest

from rr3_brain import composite

FRAME = composite.RR3_TTS_PCM_FRAME_BYTES
RATE = composite.RR3_TTS_SAMPLE_RATE


def pcm(nbytes: int) -> bytes:
    """确定性伪 PCM（s16le，字节值随下标变化，便于逐块断言原样切分）。"""
    return bytes((i % 251) for i in range(nbytes))


class TestTtsPcmFrames(unittest.TestCase):
    def test_empty_pcm_yields_no_frames_and_seq_unchanged(self):
        frames, seq = composite.rr3_tts_pcm_frames(b"", 7)
        self.assertEqual(frames, [])
        self.assertEqual(seq, 7)

    def test_exact_multiple_is_split_at_20ms_blocks(self):
        frames, seq = composite.rr3_tts_pcm_frames(pcm(FRAME * 3), 0)
        self.assertEqual([f["seq"] for f in frames], [1, 2, 3])
        self.assertEqual(seq, 3)
        for f in frames:
            self.assertEqual(f["format"], "s16le")
            self.assertEqual(f["rate"], RATE)
            self.assertEqual(f["channels"], 1)
            self.assertEqual(f["ms"], 20)  # 320 样点 @16k
            self.assertEqual(len(base64.b64decode(f["pcm_b64"])), FRAME)

    def test_tail_block_ms_derived_from_actual_samples(self):
        # 640 + 320：尾块 320 字节 = 160 样点 = 10ms（不足 20ms 按实算）
        raw = pcm(FRAME + FRAME // 2)
        frames, seq = composite.rr3_tts_pcm_frames(raw, 0)
        self.assertEqual(len(frames), 2)
        self.assertEqual(frames[0]["ms"], 20)
        self.assertEqual(frames[1]["ms"], 10)
        self.assertEqual(seq, 2)
        # 逐块拼回原流，证明切分无损
        joined = b"".join(base64.b64decode(f["pcm_b64"]) for f in frames)
        self.assertEqual(joined, raw)

    def test_seq_is_monotonic_across_calls(self):
        # 首轮一帧 → seq 水位抬到 6；下一轮从 6 续接 → 帧号 7、8（不随轮次复位）
        _, seq = composite.rr3_tts_pcm_frames(pcm(FRAME), 5)
        frames, seq = composite.rr3_tts_pcm_frames(pcm(FRAME * 2), seq)
        self.assertEqual([f["seq"] for f in frames], [7, 8])
        self.assertEqual(seq, 8)


if __name__ == "__main__":
    unittest.main()
