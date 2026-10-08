"""rr3_brain：认知层的编排入口（ASR → LLM → TTS，M4 实装）。

结构随 v0.0.1 建立；M4 起：`bus`（stdlib-only 总线客户端，ADR-0014 裁定 1）、
`cloud`（legacy 三家移植 + mock 注入，裁定 2）、`dialog`（utterance 边界状态机，
裁定 3）进入实装。命名规约见 docs/architecture/naming.md §1：Python 包
`rr3_<域>`，包级函数 `rr3_<域>_<动作>`。
"""

from rr3_brain.dialog import Rr3Dialog, Rr3DialogTurn

__all__ = ["Rr3Dialog", "Rr3DialogTurn", "rr3_brain_submit_text", "__version__"]

__version__ = "0.1.0"


def rr3_brain_submit_text(dialog: Rr3Dialog, llm, user_text: str) -> str:
    """把识别文本交给对话状态机与 LLM 客户端，返回待朗读回复。

    主循环（composite.rr3_finish_turn）的直通路径；独立成函数供点名词用方
    与单测对齐接口。云端失败抛 RuntimeError（业务失败，由调用方折应答）。
    """
    reply = llm.complete(user_text)
    dialog.rr3_record(Rr3DialogTurn(user_text=user_text, reply_text=reply))
    return reply
