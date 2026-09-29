"""对话状态机：拾音 → ASR → LLM → TTS 的轮次编排。

静默判停阈值等策略参数来自 config.toml 的 [rr3-brain.dialog]，其数值继承
legacy/main.c 的对话策略；跨进程语义见 ADR-0007 §决策一（对话属事务面）。
"""

from dataclasses import dataclass, field


@dataclass
class Rr3DialogTurn:
    """一轮对话的载体。字段先钉形状，填充逻辑随 M4 落地。"""

    user_text: str = ""
    reply_text: str = ""
    trace_id: int = 0


@dataclass
class Rr3Dialog:
    """状态机骨架：只做轮次登记与历史裁剪，不碰网络。"""

    max_turn_chars: int = 800
    history: list[Rr3DialogTurn] = field(default_factory=list)

    def submit(self, user_text: str) -> Rr3DialogTurn:
        turn = Rr3DialogTurn(user_text=user_text[: self.max_turn_chars])
        self.history.append(turn)
        return turn
