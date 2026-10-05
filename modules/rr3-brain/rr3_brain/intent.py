"""意图→动作规则面（0.0.3 T3，方案 Q1 裁定）：user_text → 结构化 actions。

策略层唯一归属在 brain（architecture §2 原则 3"brain 是唯一编排者"）：brain 说
"意图"，执行器广播认领——actions 元素形状 `{topic, args}`，词汇即契约主题
（零新造）；执行侧另有归属门（topic 必须 ∈ 执行模块 exposes，rr3-act/src/intent.rs）。

词表来源 legacy/core/relay 语义（语音"开灯/关灯"，盘点见
docs/process/legacy-digest.md 的 relay 行）。规则面是**最小确定性注入点**：
精确词表命中 + 前邻否定护栏，不做通用否定语义解析（"语义级理解随云 LLM
function-calling 演进"属 0.0.3 方案范围外）。只喂 user_text（ASR 结果），
机器人自己的 reply_text 不参与匹配——防止机器人的话触发机器人的动作。
"""

from __future__ import annotations

TOPIC_RELAY_SET = "rr3-act/relay/set"

# brain 侧自限词汇：rr3_extract_actions 只产这些 topic（执行侧 exposes 门是第二道）
ACTION_TOPICS = (TOPIC_RELAY_SET,)

# 吸合词表（legacy relay README："语音开灯/关灯"场景）
_ON_WORDS = ("开灯", "打开灯", "把灯打开", "开一下灯", "点亮")
# 断开词表
_OFF_WORDS = ("关灯", "把灯关掉", "关闭灯", "关掉灯", "熄灯")
# 最小否定护栏：命中词前两个字符内出现这些字则弃——"别开灯"不吸合。
# 这是词表匹配的边界护栏，不是否定语义解析（负样本由 tests/test_intent.py 钉死）
_NEGATIVE_CHARS = ("别", "不", "莫", "勿", "未", "没")


def rr3_extract_actions(user_text: str) -> list:
    """词表命中 → actions 清单（元素 {topic, args}）。

    吸合与断开各取第一个命中词表（同一句话先开后关则产两条动作，按序执行——
    继电器最终态由最后一条决定，这是词表匹配的既定语义，测试在册）。
    """
    text = user_text.strip()
    actions = []
    for words, on in ((_ON_WORDS, True), (_OFF_WORDS, False)):
        for word in words:
            hit = text.find(word)
            if hit < 0:
                continue
            prefix = text[max(0, hit - 2):hit]
            if any(neg in prefix for neg in _NEGATIVE_CHARS):
                continue
            actions.append({"topic": TOPIC_RELAY_SET, "args": {"on": on}})
            break
    return actions
