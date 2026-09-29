"""rr3_brain：认知层的编排入口（ASR → LLM → TTS）。

结构随 v0.0.1 建立，实现随 M4 落地（legacy/core/{asr,llm,tts} 的能力搬过来）。
命名规约见 docs/architecture/naming.md §1：Python 包 `rr3_<域>`，包级函数 `rr3_<域>_<动作>`。
"""

from rr3_brain.dialog import Rr3Dialog

__all__ = ["Rr3Dialog", "rr3_brain_submit_text", "__version__"]

__version__ = "0.0.1"


def rr3_brain_submit_text(text: str) -> str:
    """把识别文本交给对话状态机，返回待朗读回复。

    M4 前不接云端：函数签名与归属先钉住，供 rr3-interact / rrd 侧对齐接口。
    """
    raise NotImplementedError("rr3_brain_submit_text 随 M4（云端链路）落地")
