"""`python -m rr3_brain` 入口。

只打印本模块的自述（身份 + 声明的总线面 + 密钥来源），不做任何网络调用：
握手/注册随 M2（rrd + UDS）与 M4（云端）接入。
"""

from rr3_brain import __version__

MANIFEST_NAME = "rr3-brain"
PUBLISHES = ["rr3-brain/intent/result"]
SUBSCRIBES = ["rr3-interact/asr/text", "rr3-connect/net/link"]
RPC_EXPOSES = ["rr3-brain/llm/completion"]


def main() -> int:
    print(f"{MANIFEST_NAME} {__version__} (骨架，未接入总线)")
    for topic in PUBLISHES:
        print(f"  publish   {topic}")
    for topic in SUBSCRIBES:
        print(f"  subscribe {topic}")
    for topic in RPC_EXPOSES:
        print(f"  rpc       {topic}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
