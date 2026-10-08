"""rr3-brain 组合件实现（0.0.5 批C⑤ 原子重铺，自原 __main__.py 逐字迁入，语义零改动）。

组合件 = dialog ⊗ asr ⊗ llm ⊗ tts（composite.toml 的 ⊗ 链；⊗=装配期、执行单元内
import + 函数调用，ADR-0019 裁定 2）。本文件承载装配与运行编排：config 解析、
云端三客户端装配、对话轮次编排（rr3_finish_turn：ASR → LLM → TTS → interact
播放 → intent 发布）、事务面 llm/completion 点名应答与主循环；bus 总线壳留
组合件机制壳（0.0.5 方案 §12.2，Rust 迁移随 ADR-0016 线）；意图词表归
intent.py（策略层唯一归属在 brain，0.0.3 Q1 裁定）。

启动序列与 rr3-sense / rr3-interact 同构：读 `RR3_MANIFEST`（rrd spawn 注入）→
`rr3_bus_connect`（连接/注册/起心跳）→ 事件循环。循环四件事：

1. 收 `rr3-sense/audio/pcm`（stream 事件，`pcm_b64` s16le）——开轮中积累；
2. 收 `rr3-sense/audio/vad_start`——onset 起轮、offset 判停；时长上限同判停
   （`[rr3-brain.dialog].max_utterance_ms`）；
3. 判停 → ASR →（空文本弃轮）→ LLM → TTS（验证性消费）→ `rr3.call`
   `rr3-interact/tts/text` → 发布 `rr3-brain/intent/result`；
4. 应答 exposes 的 `rr3-brain/llm/completion` 点名（rr3.call 转发帧的
   trace_id = 调用方 task，顺原路应答）。

失败语义（与 rr3-sense/rr3-interact 同一契约）：业务失败（云端 4xx/5xx、空文本、
被调方超时/拒答）折日志弃轮，进程不死；传输失败（发布/应答写不出去、总线断开）
`eprintln` + `exit(1)`——模块不装"活着"，死交给 supervisor 按 restart_policy 裁定。

云端路径由 `RR3_BRAIN_CLOUD` 选择（real / mock，ADR-0014 裁定 2；mock 是测试
注入点不是产品路径，缺 key 的 real 调用照报缺环境变量，不静默降级）。
"""

from __future__ import annotations

import json
import os
import sys
import tomllib

from rr3_brain import __version__
from rr3_brain.bus import (
    RR3_MANIFEST_ENV,
    RR3_PT_EVENT,
    RR3_PT_REQUEST,
    Rr3BusClient,
    rr3_bus_connect,
)
from rr3_brain.cloud.asr import Rr3AsrClient
from rr3_brain.cloud.llm import Rr3LlmClient
from rr3_brain.cloud.tts import Rr3TtsClient
from rr3_brain.dialog import Rr3Dialog, Rr3DialogTurn
from rr3_brain.intent import rr3_extract_actions

MANIFEST_NAME = "rr3-brain"
PUBLISHES = ["rr3-brain/intent/result"]
SUBSCRIBES = ["rr3-interact/asr/text", "rr3-connect/net/link"]
RPC_EXPOSES = ["rr3-brain/llm/completion"]

TOPIC_PCM = "rr3-sense/audio/pcm"
TOPIC_VAD = "rr3-sense/audio/vad_start"
TOPIC_TTS = "rr3-interact/tts/text"
TOPIC_INTENT = "rr3-brain/intent/result"
TOPIC_COMPLETION = "rr3-brain/llm/completion"

RR3_CALL_TIMEOUT_S = 30.0


def rr3_config_load() -> dict:
    """读 config.toml：环境变量 RR3_BRAIN_CONFIG 优先，缺省取包上一级的仓内文件。"""
    path = os.environ.get("RR3_BRAIN_CONFIG")
    if not path:
        path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "config.toml")
    with open(path, "rb") as f:
        return tomllib.load(f)


def main() -> int:
    try:
        return rr3_brain_run()
    except Exception as e:  # noqa: BLE001——main 边界统一收口（退出语义见文件头）
        print(f"rr3-brain 退出: {e}", file=sys.stderr)
        return 1


def rr3_brain_run() -> int:
    print(f"{MANIFEST_NAME} {__version__} (M4 实装)")
    for topic in PUBLISHES:
        print(f"  publish   {topic}")
    for topic in SUBSCRIBES:
        print(f"  subscribe {topic}")
    for topic in RPC_EXPOSES:
        print(f"  rpc       {topic}")

    manifest_path = os.environ.get(RR3_MANIFEST_ENV)
    if not manifest_path:
        raise RuntimeError(f"缺环境变量 {RR3_MANIFEST_ENV}（rrd spawn 注入；直跑请指向 module.toml）")
    bus = rr3_bus_connect(manifest_path)
    print(f"总线就绪: name={bus.rr3_name()} task={bus.rr3_task()}", flush=True)

    cfg = rr3_config_load()
    cloud_cfg = cfg["rr3-brain"]["cloud"]
    dialog_cfg = cfg["rr3-brain"]["dialog"]
    asr = Rr3AsrClient(cloud_cfg["base_url"], cloud_cfg["asr_model"], float(cloud_cfg["timeout_s"]))
    llm = Rr3LlmClient(cloud_cfg["base_url"], cloud_cfg["llm_model"], float(cloud_cfg["timeout_s"]))
    tts = Rr3TtsClient(cloud_cfg["base_url"], cloud_cfg["tts_model"], float(cloud_cfg["timeout_s"]))
    dialog = Rr3Dialog(max_utterance_ms=int(dialog_cfg.get("max_utterance_ms", 8000)))
    trace_seq = 0

    while True:
        # 总线死亡探针：断开后 poll_wait 立即返回空，与"暂时没帧"不可区分
        if not bus.rr3_connected():
            raise RuntimeError("总线已断开（reader EOF）：交 supervisor 按 restart_policy 裁定")

        for frame in bus.rr3_poll_wait(0.25):
            if frame["header"]["opcode"] != 0x0001:  # opcode::SEND
                continue
            if frame["type"] == RR3_PT_EVENT:
                body = json.loads(frame["body_raw"])
                topic = body.get("topic", "")
                if topic == TOPIC_PCM:
                    dialog.rr3_feed_pcm(body)
                elif topic == TOPIC_VAD:
                    if dialog.rr3_feed_vad(body):
                        trace_seq = rr3_finish_turn(bus, dialog, asr, llm, tts, trace_seq)
            elif frame["type"] == RR3_PT_REQUEST:
                body = json.loads(frame["body_raw"])
                if body.get("method") != "rr3.call":
                    continue
                params = body.get("params", {})
                if params.get("topic") != TOPIC_COMPLETION:
                    result = {"error": f"rr3-brain 只 exposes {TOPIC_COMPLETION}，收到 {params.get('topic')}"}
                else:
                    text = str(params.get("args", {}).get("text", ""))
                    try:
                        result = {"text": llm.complete(text)}
                    except RuntimeError as e:
                        result = {"error": str(e)}
                bus.rr3_reply_call(frame["header"]["trace_id"], body.get("id", 0), result)

        if dialog.rr3_utterance_expired():
            trace_seq = rr3_finish_turn(bus, dialog, asr, llm, tts, trace_seq)


def rr3_finish_turn(bus: Rr3BusClient, dialog: Rr3Dialog, asr: Rr3AsrClient, llm: Rr3LlmClient,
                    tts: Rr3TtsClient, trace_seq: int) -> int:
    """判停后的一整轮：ASR → LLM → TTS → interact 播放 → intent 发布。

    业务失败（云端错误 / 空文本 / 被调方超时）折日志弃轮返回；传输失败
    （总线写不出去）抛 OSError/RuntimeError 由主循环按退出语义收口。
    """
    pcm = dialog.rr3_take_utterance()
    if not pcm:
        return trace_seq
    trace_seq += 1

    try:
        user_text = asr.transcribe(pcm)
    except RuntimeError as e:
        bus.rr3_counter_bump("asr_fail")
        print(f"轮次 {trace_seq}: ASR 失败，弃轮（{e}）", flush=True)
        return trace_seq
    bus.rr3_counter_bump("asr_ok")
    if not user_text.strip():
        print(f"轮次 {trace_seq}: 识别为空文本，弃轮", flush=True)
        return trace_seq

    try:
        reply_text = llm.complete(user_text)
    except RuntimeError as e:
        bus.rr3_counter_bump("llm_fail")
        print(f"轮次 {trace_seq}: LLM 失败，弃轮（user={user_text!r}，{e}）", flush=True)
        return trace_seq

    # TTS 验证性消费（M4 不接播放面，ADR-0014 裁定 3）：非空 + 时长换算入日志
    try:
        reply_pcm = tts.synthesize(reply_text)
    except RuntimeError as e:
        bus.rr3_counter_bump("tts_fail")
        print(f"轮次 {trace_seq}: TTS 失败，弃轮（{e}）", flush=True)
        return trace_seq
    if len(reply_pcm) % 2:
        reply_pcm = reply_pcm[:-1]
    tts_ms = len(reply_pcm) // 2 // 16000 * 1000 + (len(reply_pcm) // 2 % 16000) * 1000 // 16000

    # 播放走事务面：interact 的 tts/text 是 registered request 主题。
    # 被调方不在册/超时是业务失败（弃轮），不应答不致命。
    played = ""
    try:
        reply = bus.rr3_call_topic(TOPIC_TTS, {"text": reply_text}, RR3_CALL_TIMEOUT_S)
        played = f" played_ms={reply.get('played_ms')}"
    except RuntimeError as e:
        print(f"轮次 {trace_seq}: interact 播放点名失败（{e}）", flush=True)

    # 意图→动作映射在 brain（策略层唯一归属，0.0.3 Q1 裁定）：结构化 actions
    # 随 intent/result 广播，执行器按 exposes 归属门各自认领（brain 不单播不等待）
    actions = rr3_extract_actions(user_text)
    bus.rr3_publish(TOPIC_INTENT, {
        "trace": trace_seq,
        "user_text": user_text,
        "reply_text": reply_text,
        "tts_ms": tts_ms,
        "actions": actions,
    })
    dialog.rr3_record(Rr3DialogTurn(user_text=user_text, reply_text=reply_text, trace_id=trace_seq))
    # 完成一轮才记账（计数点 = rr3_record 成功，方案 §4 归属表）
    bus.rr3_counter_bump("chat_rounds")
    print(
        f"对话轮完成: trace={trace_seq} user={user_text!r} reply={reply_text!r}"
        f" tts_pcm_ms={tts_ms}{played} actions={len(actions)}",
        flush=True,
    )
    return trace_seq
