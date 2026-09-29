# 事件负载 schema 模板

每个 `rr3-<模块>/<域>/<事件>` 的负载在此登记一份 JSON Schema，文件名把 `/` 换成 `-`
并去掉 `rr3-` 前缀，例如 `rr3-sense/audio/vad_start` → `sense-audio-vad_start.schema.json`。

约定（与 `contracts/README.md` 演进规则一致）：

- `schema_version` 必填整数，只增不改
- 负载内不重复放 topic 字符串与 phase：topic 在帧外层由 rr-bus 承载，phase 随 payload 走
  （ADR-0006 §2「语义字段置于 payload 内」的落点）

本目录今天有没有真实 schema 属实况，唯一落点
[`../../docs/status.md`](../../docs/status.md)，本文件不记进度。

模板：

    {
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "title": "rr3-sense/audio/vad_start",
      "schema_version": 1,
      "type": "object",
      "required": ["rms_peak"],
      "properties": { "rms_peak": { "type": "integer", "minimum": 0, "maximum": 65535 } },
      "additionalProperties": false
    }
