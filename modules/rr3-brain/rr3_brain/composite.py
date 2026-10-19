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

import base64
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
from rr3_brain.cloud.llm import LLM_MODE_API, LLM_MODE_LOCAL, Rr3LlmClient
from rr3_brain.cloud.tts import Rr3TtsClient
from rr3_brain.dialog import Rr3Dialog, Rr3DialogTurn
from rr3_brain.intent import rr3_extract_actions

MANIFEST_NAME = "rr3-brain"
PUBLISHES = ["rr3-brain/intent/result", "rr3-brain/audio/tts_pcm"]
# 申报订阅面（与 module.toml 同源双写）：C14（ADR-0019 修订 2 ②）后对齐实然面——
# asr/text 走 retired 撤申报、net/link 撤申报（不补实现）；实际消费入口 = pcm + vad_start
SUBSCRIBES = ["rr3-sense/audio/pcm", "rr3-sense/audio/vad_start"]
RPC_EXPOSES = ["rr3-brain/llm/completion"]

TOPIC_PCM = "rr3-sense/audio/pcm"
TOPIC_VAD = "rr3-sense/audio/vad_start"
TOPIC_TTS = "rr3-interact/tts/text"
TOPIC_TTS_PCM = "rr3-brain/audio/tts_pcm"
TOPIC_INTENT = "rr3-brain/intent/result"
TOPIC_COMPLETION = "rr3-brain/llm/completion"

# TTS 下行流分帧规格（MCP-1 定形）：逐字段复用上行 rr3-sense/audio/pcm 的事件体，
# 同 20ms 一块（@16k/mono/s16le = 320 样点 = 640 字节），base64 后 ≈856 字符。
# 采样规格与 sense/interact 同一张表（16k/mono/16bit）。
RR3_TTS_SAMPLE_RATE = 16_000
RR3_TTS_PCM_FRAME_BYTES = 640

# 总线 RPC 超时缺省（B-13①：`[rr3-brain.bus].call_timeout_s` 缺键时的回落值——
# legacy `RR3_CALL_TIMEOUT_S = 30.0` 常量已退役，真值点 = config.toml 注释）
RR3_BUS_CALL_TIMEOUT_DEFAULT_S = 30.0


def rr3_config_load() -> dict:
    """读 config.toml：环境变量 RR3_BRAIN_CONFIG 优先，缺省取包上一级的仓内文件。"""
    path = os.environ.get("RR3_BRAIN_CONFIG")
    if not path:
        path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "config.toml")
    with open(path, "rb") as f:
        return tomllib.load(f)


def rr3_brain_model_row(cfg: dict) -> tuple[str, str, str | None, str | None]:
    """LLM 连接行解析（0.0.6 抽象批）：[[rr3-brain.model]] 恰一 active 行。

    返回 (base_url, model_id, key_ref, mode)；无表 → 旧路径四元组
    （cloud 段 base_url + llm_model，key_ref=None 走通用环境变量键，mode=None
    随全局档）。fail-closed：active 数≠1、mode/api_format 不认、缺 base_url/
    model_id 一律 RuntimeError（点名行 name，不静默挑行）。
    """
    brain = cfg.get("rr3-brain", {})
    rows = brain.get("model", [])
    if not rows:
        cloud_cfg = brain["cloud"]
        return cloud_cfg["base_url"], cloud_cfg["llm_model"], None, None
    active = [r for r in rows if r.get("active")]
    if len(active) != 1:
        names = [r.get("name", "?") for r in rows]
        raise RuntimeError(f"[[rr3-brain.model]] 须恰一行 active=true（现 {len(active)} 行: {names}）")
    row = active[0]
    name = row.get("name", "?")
    mode = row.get("mode", LLM_MODE_API)
    if mode not in (LLM_MODE_API, LLM_MODE_LOCAL):
        raise RuntimeError(f"模型行 {name} mode 不认: {mode}（允许 {LLM_MODE_API}/{LLM_MODE_LOCAL}）")
    fmt = row.get("api_format", "openai")
    if fmt != "openai":
        raise RuntimeError(f"模型行 {name} api_format 暂不支持: {fmt}（本批仅 openai）")
    base_url = str(row.get("base_url", "")).strip()
    model_id = str(row.get("model_id", "")).strip()
    if not base_url or not model_id:
        raise RuntimeError(f"模型行 {name} 缺 base_url/model_id")
    key_ref = row.get("api_key_ref")
    if key_ref is not None:
        key_ref = str(key_ref).strip() or None
    return base_url, model_id, key_ref, mode


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
    # config 先于总线连接装载（B-13①）：connect_timeout_s 是连总线的参数，
    # 缺文件/坏键在连接前拒（fail-closed 更早，省一次注定失败的注册）
    cfg = rr3_config_load()
    cloud_cfg = cfg["rr3-brain"]["cloud"]
    dialog_cfg = cfg["rr3-brain"]["dialog"]
    bus_cfg = cfg["rr3-brain"].get("bus", {})
    call_timeout_s = float(bus_cfg.get("call_timeout_s", RR3_BUS_CALL_TIMEOUT_DEFAULT_S))
    connect_timeout_s = float(bus_cfg.get("connect_timeout_s", 5.0))
    bus = rr3_bus_connect(manifest_path, connect_timeout_s=connect_timeout_s)
    print(f"总线就绪: name={bus.rr3_name()} task={bus.rr3_task()}", flush=True)

    asr = Rr3AsrClient(cloud_cfg["base_url"], cloud_cfg["asr_model"], float(cloud_cfg["timeout_s"]))
    base_url, model_id, key_ref, mode = rr3_brain_model_row(cfg)
    llm = Rr3LlmClient(base_url, model_id, float(cloud_cfg["timeout_s"]), key_ref=key_ref, mode=mode)
    tts = Rr3TtsClient(cloud_cfg["base_url"], cloud_cfg["tts_model"], float(cloud_cfg["timeout_s"]))
    dialog = Rr3Dialog(max_utterance_ms=int(dialog_cfg.get("max_utterance_ms", 8000)))
    print(
        f"config 就绪: cloud timeout_s={cloud_cfg['timeout_s']} "
        f"bus call={call_timeout_s}s connect={connect_timeout_s}s "
        f"dialog max_utterance_ms={dialog.max_utterance_ms}",
        flush=True,
    )
    trace_seq = 0
    # TTS 下行流的 seq 水位：模块级单调，跨轮不复位（对位 sense 的 pcm seq 语义）
    pcm_seq = 0

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
                        trace_seq, pcm_seq = rr3_finish_turn(
                            bus, dialog, asr, llm, tts, trace_seq, pcm_seq, call_timeout_s
                        )
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
            trace_seq, pcm_seq = rr3_finish_turn(
                bus, dialog, asr, llm, tts, trace_seq, pcm_seq, call_timeout_s
            )


def rr3_tts_pcm_frames(pcm: bytes, seq: int) -> tuple[list[dict], int]:
    """把一轮 TTS 的 PCM 分帧为 `rr3-brain/audio/tts_pcm` 的载荷体列表（纯函数）。

    逐字段复用上行 `rr3-sense/audio/pcm` 已定的形状（`rr3_pcm_event_body` 为形状
    事实源）：`{seq, format:"s16le", rate, channels, ms, pcm_b64}`，20ms 一块
    （640 字节）。`seq` 从入参水位起逐帧 +1（**模块级单调，不随轮次复位**）；`ms`
    由块内样点数导出（尾块不足 20ms 时按实算）。`topic` 不含（由 `rr3_publish`
    并入）。返回 `(frames, 新 seq 水位)`——总线发布与跨语言一致性用例共用同一份组装。
    """
    frames: list[dict] = []
    for off in range(0, len(pcm), RR3_TTS_PCM_FRAME_BYTES):
        chunk = pcm[off : off + RR3_TTS_PCM_FRAME_BYTES]
        samples = len(chunk) // 2
        seq += 1
        frames.append({
            "seq": seq,
            "format": "s16le",
            "rate": RR3_TTS_SAMPLE_RATE,
            "channels": 1,
            "ms": samples * 1000 // RR3_TTS_SAMPLE_RATE,
            "pcm_b64": base64.b64encode(chunk).decode("ascii"),
        })
    return frames, seq


def rr3_publish_tts_pcm(bus: Rr3BusClient, pcm: bytes, seq: int) -> int:
    """按 [`rr3_tts_pcm_frames`] 分帧发布 tts_pcm 流，返回新的 seq 水位。"""
    frames, seq = rr3_tts_pcm_frames(pcm, seq)
    for body in frames:
        bus.rr3_publish(TOPIC_TTS_PCM, body)
    return seq


def rr3_finish_turn(bus: Rr3BusClient, dialog: Rr3Dialog, asr: Rr3AsrClient, llm: Rr3LlmClient,
                    tts: Rr3TtsClient, trace_seq: int, pcm_seq: int,
                    call_timeout_s: float = RR3_BUS_CALL_TIMEOUT_DEFAULT_S) -> tuple[int, int]:
    """判停后的一整轮：ASR → LLM → TTS → 下发 PCM 流 → interact 播放 → intent 发布。

    业务失败（云端错误 / 空文本 / 被调方超时）折日志弃轮返回；传输失败
    （总线写不出去）抛 OSError/RuntimeError 由主循环按退出语义收口。
    返回 `(trace_seq, pcm_seq)`——pcm_seq 是模块级单调的 TTS 下行流 seq 水位。
    计账拆两本（B-16①，不许一个词盖两件事）：`rounds_replied` = LLM 应答发布
    完毕（rr3_record 成功后 bump）；`rounds_audible` = interact 回执 played_ms>0
    （点名失败/播出 0ms 不计）。`/api/data` counters 表同时可见两键。
    """
    pcm = dialog.rr3_take_utterance()
    if not pcm:
        return trace_seq, pcm_seq
    trace_seq += 1

    try:
        user_text = asr.transcribe(pcm)
    except RuntimeError as e:
        bus.rr3_counter_bump("asr_fail")
        print(f"轮次 {trace_seq}: ASR 失败，弃轮（{e}）", flush=True)
        return trace_seq, pcm_seq
    bus.rr3_counter_bump("asr_ok")
    if not user_text.strip():
        print(f"轮次 {trace_seq}: 识别为空文本，弃轮", flush=True)
        return trace_seq, pcm_seq

    try:
        reply_text = llm.complete(user_text)
    except RuntimeError as e:
        bus.rr3_counter_bump("llm_fail")
        print(f"轮次 {trace_seq}: LLM 失败，弃轮（user={user_text!r}，{e}）", flush=True)
        return trace_seq, pcm_seq

    try:
        reply_pcm = tts.synthesize(reply_text)
    except RuntimeError as e:
        bus.rr3_counter_bump("tts_fail")
        print(f"轮次 {trace_seq}: TTS 失败，弃轮（{e}）", flush=True)
        return trace_seq, pcm_seq
    if len(reply_pcm) % 2:
        reply_pcm = reply_pcm[:-1]
    tts_ms = len(reply_pcm) // 2 // 16000 * 1000 + (len(reply_pcm) // 2 % 16000) * 1000 // 16000

    # 语音下行：先把 PCM 按 MCP-1 的形状分帧发布到 rr3-brain/audio/tts_pcm（stream），
    # 再走事务面点名 interact 的 tts/text——点名是"该不该播"的**唯一判据**，流只是
    # 载荷通路（§20 第 1 条 (a)）。点名的 args 带 pcm_bytes，供播放面校验缓冲完整性
    # （半流/空缓冲不静默，MC-1）。通路对引擎中立：今天接 cloud/tts.py，R31 Q3 的
    # 本地 Piper adapter 落地后接 Piper，两腿共用同一份分帧。
    pcm_seq = rr3_publish_tts_pcm(bus, reply_pcm, pcm_seq)
    played = ""
    try:
        reply = bus.rr3_call_topic(
            TOPIC_TTS, {"text": reply_text, "pcm_bytes": len(reply_pcm)}, call_timeout_s
        )
        played_ms = reply.get("played_ms")
        played = f" played_ms={played_ms}"
        # audible 记账（B-16①）：播出且 >0ms 才算"用户听得见"——点名失败
        # （RuntimeError）与 played_ms=0 都不进这本账（I-21① 修后 interact 侧
        # 0ms 已折 error 应答，此处的 isinstance 防御异常形状）
        if isinstance(played_ms, (int, float)) and not isinstance(played_ms, bool) and played_ms > 0:
            bus.rr3_counter_bump("rounds_audible")
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
    # 完成一轮才记账（计数点 = rr3_record 成功，方案 §4 归属表）。键名拆账
    # （B-16①）：chat_rounds 一个词盖"应答"与"听见"两件事，退役拆两本
    bus.rr3_counter_bump("rounds_replied")
    print(
        f"对话轮完成: trace={trace_seq} user={user_text!r} reply={reply_text!r}"
        f" tts_pcm_ms={tts_ms}{played} actions={len(actions)}",
        flush=True,
    )
    return trace_seq, pcm_seq
