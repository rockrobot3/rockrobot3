"""意图→动作规则面（0.0.3 T3，方案 Q1 裁定）：user_text → 结构化 actions。

策略层唯一归属在 brain（architecture §2 原则 3"brain 是唯一编排者"）：brain 说
"意图"，执行器广播认领——actions 元素形状 `{topic, args}`，词汇即契约主题
（零新造）；执行侧另有归属门（topic 必须 ∈ 执行模块 exposes，rr3-act/src/intent.rs）。

词表来源 legacy/core/relay 语义（语音"开灯/关灯"，盘点见
docs/process/legacy-digest.md 的 relay 行）+ 批F2 转向三档（"向左/向右/回中"
→ servo/command，判据 8）。规则面是**最小确定性注入点**：
精确词表命中 + 前邻否定护栏，不做通用否定语义解析（"语义级理解随云 LLM
function-calling 演进"属 0.0.3 方案范围外）。只喂 user_text（ASR 结果），
机器人自己的 reply_text 不参与匹配——防止机器人的话触发机器人的动作。
"""

from __future__ import annotations

TOPIC_RELAY_SET = "rr3-act/relay/set"
TOPIC_SERVO_COMMAND = "rr3-act/servo/command"

# brain 侧自限词汇：rr3_extract_actions 只产这些 topic（执行侧 exposes 门是第二道）
ACTION_TOPICS = (TOPIC_RELAY_SET, TOPIC_SERVO_COMMAND)

# 吸合词表（legacy relay README："语音开灯/关灯"场景）
_ON_WORDS = ("开灯", "打开灯", "把灯打开", "开一下灯", "点亮")
# 断开词表
_OFF_WORDS = ("关灯", "把灯关掉", "关闭灯", "关掉灯", "熄灯")

# 转向档位角（批F2，判据 8 的"对应档位"）：中位 90° 对齐 sense 侧 DoA 的 center
# 档，左右各让 20°——落 pan 轴行程 [70,110]（0.0.5 方案 C 裁定 2026-10-12：
# 云台机械行程远小于 SG90 全程，越程被 act 的行程裁决拒收点名，故三档必须
# 落在行程内；不做满行程摆动）。
_SERVO_ANGLE_LEFT = 70
_SERVO_ANGLE_CENTER = 90
_SERVO_ANGLE_RIGHT = 110

# 转向词表（与 sense 侧 DoA ±90° 三档同一语义坐标系：左/中/右）
_LEFT_WORDS = ("向左", "往左", "左转", "看左边")
_RIGHT_WORDS = ("向右", "往右", "右转", "看右边")
_CENTER_WORDS = ("回中", "看前面", "看中间", "回中间")

# 最小否定护栏：命中词前两个字符内出现这些字则弃——"别开灯"不吸合、"别向左"不转向。
# 这是词表匹配的边界护栏，不是否定语义解析（负样本由 tests/test_intent.py 钉死）
_NEGATIVE_CHARS = ("别", "不", "莫", "勿", "未", "没")


def rr3_hit_word(text: str, words) -> bool:
    """词表命中 + 前邻否定护栏：任一词命中且其前两字符内无否定字才算真。"""
    for word in words:
        hit = text.find(word)
        if hit < 0:
            continue
        prefix = text[max(0, hit - 2):hit]
        if any(neg in prefix for neg in _NEGATIVE_CHARS):
            continue
        return True
    return False


def rr3_extract_actions(user_text: str) -> list:
    """词表命中 → actions 清单（元素 {topic, args}）。

    吸合与断开各取第一个命中词表（同一句话先开后关则产两条动作，按序执行——
    继电器最终态由最后一条决定，这是词表匹配的既定语义，测试在册）；
    转向三档（左/中/右）各取一次命中，产 servo/command 动作（批F2 判据 8）。
    """
    text = user_text.strip()
    actions = []
    for words, on in ((_ON_WORDS, True), (_OFF_WORDS, False)):
        if rr3_hit_word(text, words):
            actions.append({"topic": TOPIC_RELAY_SET, "args": {"on": on}})
    for words, angle in (
        (_LEFT_WORDS, _SERVO_ANGLE_LEFT),
        (_RIGHT_WORDS, _SERVO_ANGLE_RIGHT),
        (_CENTER_WORDS, _SERVO_ANGLE_CENTER),
    ):
        if rr3_hit_word(text, words):
            actions.append({"topic": TOPIC_SERVO_COMMAND, "args": {"angle_deg": angle}})
    return actions
