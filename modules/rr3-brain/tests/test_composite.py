"""组合件纯函数单测：TTS 下行流分帧形状（MC-1）。

只测 `rr3_tts_pcm_frames`/`rr3_publish_tts_pcm` 的组装语义（逐字段复用上行
`rr3-sense/audio/pcm` 已定的形状、20ms 一块、seq 模块级单调），不碰总线与云端。
依赖面与 test_bus 同款：`from rr3_brain import composite`（PYTHONPATH 指向包目录）。

批N4 件六追加：`rr3_finish_turn` 的计账拆分（B-16①）与总线 RPC 超时注入
（B-13①）——bus/dialog/asr/llm/tts 五参鸭子类型，stub 注入不碰真总线。
"""

import base64
import json
import os
import unittest

from rr3_brain import composite
from rr3_brain.dialog import Rr3DialogTurn
FRAME = composite.RR3_TTS_PCM_FRAME_BYTES
RATE = composite.RR3_TTS_SAMPLE_RATE

REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..")
GOLDEN = os.path.join(REPO, "contracts", "golden", "tts-pcm-frames.json")


def pcm(nbytes: int) -> bytes:
    """确定性伪 PCM（s16le，字节值随下标变化，便于逐块断言原样切分）。"""
    return bytes((i % 251) for i in range(nbytes))


class _FakeBus:
    """计数/发布/点名三动作记账——拆账判据的观测面。"""

    def __init__(self, reply=None, call_error=None):
        self.bumps: list[tuple[str, int]] = []
        self.published: list[tuple[str, dict]] = []
        self.calls: list[tuple[str, dict, float]] = []
        self._reply = reply
        self._call_error = call_error

    def rr3_counter_bump(self, key, by=1):
        self.bumps.append((key, by))

    def rr3_publish(self, topic, body):
        self.published.append((topic, body))

    def rr3_call_topic(self, topic, args, timeout_s):
        self.calls.append((topic, args, timeout_s))
        if self._call_error is not None:
            raise RuntimeError(self._call_error)
        return self._reply


class _StubDialog:
    def rr3_take_utterance(self):
        return pcm(320)

    def rr3_record(self, turn: Rr3DialogTurn):
        pass


class _StubAsr:
    def transcribe(self, pcm_bytes, sample_rate=16000):
        return "你好"


class _StubLlm:
    def complete(self, prompt):
        return "答"


class _StubTts:
    def synthesize(self, text, sample_rate=16000):
        return pcm(640)


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


class TestGoldenCrossLanguage(unittest.TestCase):
    """MC-1③ 跨语言一致性：分帧输出与 contracts/golden 共享基准逐字段一致。

    同一份 `tts-pcm-frames.json` 由 Rust 腿（rr3-interact 收流解码）同读对拍——
    两侧各自的单测钉语义，这条钉的是"同一份字节基准两侧对得上"。
    """

    def test_frames_match_shared_golden_vector(self):
        with open(GOLDEN, encoding="utf-8") as f:
            golden = json.load(f)
        raw = bytes.fromhex(golden["pcm_hex"])
        frames, seq = composite.rr3_tts_pcm_frames(raw, 0)
        self.assertEqual(frames, golden["frames"], "分帧输出与共享基准逐字段一致")
        self.assertEqual(seq, len(golden["frames"]))
        # 拼回无损：帧列 b64 拼接 == 基准 PCM（Rust 腿按同一判据对拍）
        joined = b"".join(base64.b64decode(f["pcm_b64"]) for f in frames)
        self.assertEqual(joined, raw)


class TestFinishTurnLedger(unittest.TestCase):
    """B-16① 拆账 + B-13① 超时注入：`rounds_replied` / `rounds_audible` 两本账，
    点名超时经 call_timeout_s 参数注入（码内常量已退役）。"""

    def _run(self, bus):
        return composite.rr3_finish_turn(
            bus, _StubDialog(), _StubAsr(), _StubLlm(), _StubTts(), 0, 0, 12.5
        )

    def _bumps_of(self, bus, key):
        return sum(by for (k, by) in bus.bumps if k == key)

    def test_played_rounds_counted_in_both_ledgers_and_timeout_injected(self):
        bus = _FakeBus(reply={"played_ms": 120})
        trace_seq, pcm_seq = self._run(bus)
        self.assertEqual(trace_seq, 1)
        self.assertEqual(self._bumps_of(bus, "rounds_replied"), 1, "应答一本账")
        self.assertEqual(self._bumps_of(bus, "rounds_audible"), 1, "听见一本账")
        self.assertEqual(self._bumps_of(bus, "chat_rounds"), 0, "旧键退役不再记")
        self.assertEqual(bus.calls[0][2], 12.5, "点名超时经参数注入（B-13①）")
        topics = [t for (t, _) in bus.published]
        self.assertIn(composite.TOPIC_INTENT, topics, "点名成功意图照发")

    def test_call_failure_counts_replied_only_and_intent_still_published(self):
        # 播放点名失败（含 0ms 折 error）：只进应答账，听见账不动——
        # "完成一轮"与"用户听见"自此可分（不许一个词盖两件事）
        bus = _FakeBus(call_error="tts 播出 0ms（…），不得记 ok（I-21①）")
        trace_seq, _ = self._run(bus)
        self.assertEqual(trace_seq, 1, "轮次照常推进")
        self.assertEqual(self._bumps_of(bus, "rounds_replied"), 1)
        self.assertEqual(self._bumps_of(bus, "rounds_audible"), 0)
        topics = [t for (t, _) in bus.published]
        self.assertIn(composite.TOPIC_INTENT, topics, "点名失败意图照发（动作不丢）")

    def test_zero_played_ms_does_not_count_audible(self):
        bus = _FakeBus(reply={"played_ms": 0})
        self._run(bus)
        self.assertEqual(self._bumps_of(bus, "rounds_replied"), 1)
        self.assertEqual(self._bumps_of(bus, "rounds_audible"), 0, "0ms 不算听见")

    def test_asr_failure_counts_neither_ledger(self):
        # 云端 ASR 失败弃轮：两本账都不动（既有 asr_fail 计数语义不变）
        class _FailAsr:
            def transcribe(self, pcm_bytes, sample_rate=16000):
                raise RuntimeError("HTTP 500")

        bus = _FakeBus(reply={"played_ms": 120})
        composite.rr3_finish_turn(
            bus, _StubDialog(), _FailAsr(), _StubLlm(), _StubTts(), 0, 0, 12.5
        )
        self.assertEqual(self._bumps_of(bus, "rounds_replied"), 0)
        self.assertEqual(self._bumps_of(bus, "rounds_audible"), 0)
        self.assertEqual(self._bumps_of(bus, "asr_fail"), 1, "弃轮仍有失败计数")


if __name__ == "__main__":
    unittest.main()
