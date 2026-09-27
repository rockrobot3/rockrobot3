# core/data — 运行时数据模块

全系统结构化运行数据，存 NVS 断电不丢。定位"机器慢/不稳定"先看数字：
`asr_fail` 高 → 网络或识别服务差；`llm_fail` 高 → 大模型接口问题；`boots` 暴涨 → 设备在反复重启。

## 公开 API（include/data.h）

```c
data_init();                    // 开机调用：载入 NVS，开机次数 +1
data_inc(DATA_ASR_FAIL);        // 计数 +1（立即持久化）
data_get(DATA_CHAT_ROUNDS);     // 读当前值
data_get_json(buf, sz);         // 全部计数 → {"boots":12,"chat_rounds":33,...}
```

## 计数项

`boots` 开机次数 / `chat_rounds` 对话轮数 / `asr_ok` `asr_fail` 识别成败 /
`llm_fail` 大模型失败 / `tts_fail` 合成失败 / `relay_on` `relay_off` 继电器动作 / `errors` 其他错误。

需要新计数：在 `data.h` 枚举加一项（`DATA_COUNT` 之前）+ `data.c` 的 `s_names` 数组加同名键即可。
