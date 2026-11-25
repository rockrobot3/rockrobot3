# ADR-0030：ESP 全链档的配置面可选与网络服务先行

- **状态**：草案（2026-11-21，已施工验证，待评审）
- **日期**：2026-11-21
- **关联**：
  [ADR-0029](ADR-0029-单板自治与云直连-三独立形态全链收口.md)（三独立形态与全链口径）·
  [单板全链开发计划 · 归档册 §20.2](../dev/0.0.8/单板全链/回执归档/0.0.8版本单板全链计划回执归档-十至二十.md)（W7 实测与存储/流式候选）·
  [实测 0014](../../evidence/实测/0014.md)（classic 真板堆账与复位环）

## 背景

classic W7 真板实测（2026-11-21）暴露两个不同性质的问题：

1. **网络服务与 HTTP 配置面耦合**：`net` 心跳原先绑在 `httpd` 起栈成功之后；
   classic 上 httpd 以 `0xB008`（`ESP_ERR_HTTPD_ALLOC_MEM`，实际栈
   `HTTPD_STACK_SIZE=16,384B`）分配失败，net 永不心跳，监督壳判死并整机复位。
   源码 `60381a0e` 已把 net 心跳前移到 httpd 之前，复位环消失。
2. **全链档仍常驻配置面**：即使复位修好，httpd 仍是全链档的常驻面；classic
   播放面后 `free=34,336B`、`int_free=61,160B`，没有空间再承担 16KB httpd 栈与
   会话分配。全链档不需要 HTTP 配网面，配置面应可关闭。

本 ADR 只裁定**网络服务与 HTTP 配置面的边界**；ASR/TTS 的全量缓冲问题归下一步
存储/流式承载裁定，不在本 ADR 解决。

## 决策

### 裁定 1：网络服务是核心，HTTP 配置面是可选策略

- WiFi 起栈、凭据读链、net 心跳属于**网络服务**，任何需要联网的档都必须先起。
- HTTP 配置面（`esp_http_server`、配网写口、`/api/board/*`）属于**配置策略**，
  可以按档关闭。
- 关闭 HTTP 不等于关闭网络；net 心跳不得依赖 HTTP 起栈结果。

### 裁定 2：新增编译期 `no-http` 档，默认固件行为不变

- Cargo feature `no-http`：与 `autonomy`/`tts` 组合成 ESP 全链档。
- `wifi_bringup` 成功后立即起 net 心跳；`no-http` 档在此停止，不调用
  `httpd_register`，并打一行显式点名。
- 默认固件（不带 `no-http`）继续起 HTTP 配置面，行为与当前逐点一致。
- 不用运行期开关：classic 上没有 HTTP 时无法下发运行期开关，编译期档位是唯一
  可达入口。

### 裁定 3：失败语义保持 fail-honest

- WiFi bringup 失败：点名失败原因，net 不伪造心跳，不因“没有 HTTP”而复位。
- `no-http` 档：打印“HTTP 配置面按 no-http 关闭”，配置面缺席可见。
- 需要配网/诊断时：使用带 HTTP 的独立档或维护窗口，不把配置面重新塞回全链档。

### 裁定 4：本 ADR 不解决全量音频缓冲

- ASR 8s 段、TTS 512KB 回包与播放队列仍是全链 No-Go 的主因。
- 存储/流式承载（语音段落 flash、TTS 边收边播、无整段 Vec）另立 ADR；
  `no-http` 只释放 HTTP 面，不把 classic 全链判成 Go。

## 备选与否决

| 备选 | 否决理由 |
|---|---|
| 保留 HTTP，降 `HTTPD_STACK_SIZE` | 现值已是 16,384B；`0xB008` 是动态分配失败，不是单一栈尺寸问题；继续压栈会把 HTTP 服务变成不稳定面 |
| 运行期关闭 HTTP | classic 无 HTTP 时无法下发开关；且“先起再关”仍占起栈峰值 |
| 全链档继续常驻 HTTP | 与 W7 实测冲突：播放面后余量不足以承担配置面，且配网并非全链每轮必需 |
| 改硬件加 PSRAM | 换板/换模组属另一条硬件路线，不在本 ADR 的软件结构裁定内 |

## 后果

- 正面：classic 全链档释放 httpd 的 16KB 栈与会话分配；net 不再依赖 HTTP；
  为后续存储/流式承载提供稳定网络底座。
- 负面：全链档没有 HTTP 配网面，配网需独立档/窗口；CI 需增加 `no-http`
  构建腿，防止档位漂移。
- 风险：`no-http` 档的 WiFi 起栈失败会显式暴露为 net 无心跳；必须保留点名，
  不得用“降级”掩盖。

## 施工与验收

- `tools/esp32-periph-base/Cargo.toml` 增 `no-http = []`。
- `start_httpd` 在 `wifi_bringup` + net 心跳之后按 `cfg(feature = "no-http")`
  提前返回；默认档不变。
- CI 增 classic `autonomy,tts,no-http` 构建腿（不新增工作流）。
- 真板验收：`no-http` 档 60s 无复位、HTTP 关闭点名可见、播放面后 `[mem]`
  读数入 0014；全链 Go/No-Go 仍由 W7 存储/流式裁定。

## 施工回执（2026-11-21）

- 代码 `3a43c6cb`；CI gate / integration / platforms 全绿；产物
  `esp32-classic-nohttp-firmware`（7,367,414 B）。
- 真板 60s：单次 boot、0 次 `rst:0xc`；日志命中
  `全链档 no-http：WiFi 已起…HTTP 配置面关闭（ADR-0030）`。
- 内存：播放面后 `free=34,200B` / `int_free=61,024B`，`status` 42,200B /
  `min_free=7,512B`；与带 httpd 档基本持平，仅少掉失败路径与瞬时低水位。
- 结论：本 ADR 的档位边界成立；但**不能**把 classic 全链判成 Go，下一步仍是
  ASR 存储分块与 TTS 边收边播。
