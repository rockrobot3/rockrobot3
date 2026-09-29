# legacy —— ESP32 在编固件（冻结区）

**状态**：功能在编、只接受缺陷修复；新特性一律去 `modules/` + `base/`（见 `docs/architecture.md`）。
迁移日期 2026-09-29，锚点 tag `v0-pre-legacy`（提交 `eecdb1f1f269efea8c5c491b4c158f3fb92c6719`，由 `git rev-parse v0-pre-legacy` 实测取得）。

## 目录

| 路径 | 内容 | 工具链 |
|---|---|---|
| `firmware-c/` | 主系统固件（ESP-IDF v6.1，语音助手在编版） | 需 `source $IDF_PATH/export.sh` |
| `core/` | 12 个可复用 ESP-IDF 组件（llm/asr/tts/link/audio/relay/panel/applog/data） | 同上 |
| `firmware-rs/` | Rust 语音固件实验（esp-idf-hal） | ESP-IDF v5.5.5 |
| `rust/` | Rust 外设模块试验（esp-hal，blink/OLED/舵机/DHT11） | espup |

## 已知不可构建项（Windows 本机）

`firmware-rs/.cargo/config.toml`、`rust/.cargo/config.toml`、`firmware-rs/sdkconfig.defaults`
中写死了 `/Users/qianmi/...` 的绝对路径与特定工具链版本。这些文件在 legacy 内**按原样冻结**，
不做修复；如要复用，改自己环境副本（`.gitignore` 已忽略 `**/sdkconfig`）。

## 回滚

```bash
git log --oneline --diff-filter=R -3        # 找到重命名提交
git revert <该提交>                          # 恢复原顶层布局（推荐，不改历史）
```

## 与主系统的关系

`firmware-c/` 与 `core/` 的相对引用（`../core/...`）整体保留，二者必须同批移动。
密钥不再硬编码：构建需 `export SILICONFLOW_API_KEY=…`（见 `core/README.md`）。
