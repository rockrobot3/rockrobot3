# 事件负载 schema 模板

每个 `rr3-<模块>/<域>/<事件>` 的负载在此登记一份 JSON Schema，文件名把 `/` 换成 `-`
并去掉 `rr3-` 前缀，例如 `rr3-sense/audio/vad_start` → `sense-audio-vad_start.schema.json`。

约定（与 `contracts/README.md` 演进规则一致）：

- `schema_version` 必填整数，只增不改
- 负载内不重复放 topic 字符串与 phase：topic 在帧外层由 rr-bus 承载，phase 随 payload 走
  （ADR-0006 §2「语义字段置于 payload 内」的落点）
- v0.0.1 不产出真实 schema，首个随 M3（rr3-sense 的 VAD 事件）落地

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
