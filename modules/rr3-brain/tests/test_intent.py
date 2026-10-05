"""意图规则面单测（0.0.3 T3）。依赖面与 test_dialog 同款：只 import intent.py
（stdlib、3.9 兼容语法），本机 python3 3.9.6 可跑。"""

import unittest

from rr3_brain.intent import TOPIC_RELAY_SET, rr3_extract_actions


def relay_actions(text):
    return rr3_extract_actions(text)


class Rr3IntentExtractTest(unittest.TestCase):
    def test_on_word_yields_relay_true(self):
        for text in ("开灯", "帮我把灯打开", "请开一下灯", "点亮"):
            actions = relay_actions(text)
            self.assertEqual(len(actions), 1, f"{text!r} 应恰产一条动作: {actions}")
            self.assertEqual(actions[0]["topic"], TOPIC_RELAY_SET)
            self.assertEqual(actions[0]["args"], {"on": True})

    def test_off_word_yields_relay_false(self):
        for text in ("关灯", "把灯关掉", "关闭灯", "熄灯"):
            actions = relay_actions(text)
            self.assertEqual(len(actions), 1, f"{text!r} 应恰产一条动作: {actions}")
            self.assertEqual(actions[0]["topic"], TOPIC_RELAY_SET)
            self.assertEqual(actions[0]["args"], {"on": False})

    def test_negative_guard_rejects(self):
        # 命中词前两字符内有否定字 → 弃（最小护栏；通用否定解析属云 LLM 演进）
        for text in ("别开灯", "不要关灯", "先别开灯，等等"):
            self.assertEqual(relay_actions(text), [], f"{text!r} 不得触发动作")

    def test_unrelated_text_yields_nothing(self):
        for text in ("今天天气怎么样", "你好", "讲个故事", ""):
            self.assertEqual(relay_actions(text), [], f"{text!r} 不得触发动作")

    def test_on_and_off_in_one_sentence_yields_in_order(self):
        actions = relay_actions("开灯，等等再关灯")
        self.assertEqual(len(actions), 2, f"开与关同句产两条按序动作: {actions}")
        self.assertEqual(actions[0]["args"], {"on": True})
        self.assertEqual(actions[1]["args"], {"on": False})

    def test_action_shape_matches_contract(self):
        # 载荷 schema（contracts/events/brain-intent-result.schema.json）钉的两键形状
        for action in relay_actions("开灯"):
            self.assertEqual(set(action.keys()), {"topic", "args"})
            self.assertIsInstance(action["args"], dict)


if __name__ == "__main__":
    unittest.main()
