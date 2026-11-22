# RR3 板侧固件烧录（发布制品）

本说明随 tag release 资产与 ghcr OCI 制品
`ghcr.io/<owner>/rockrobot3-firmware:<tag>` 一同发布；制品由流水线构建，
本地不编译。

## 资产

| 文件 | 说明 |
|---|---|
| `rr3-periph-base-esp32-safe-dev.elf` | ESP32 classic safe 档（默认 feature：外设底座 + 麦克风采集，不含云/播放） |
| `rr3-periph-base-esp32s3-full-dev.elf` | ESP32-S3 full 档（`esp32s3,autonomy,tts`：全链） |
| `rr3-bootloader-esp32-dev.bin` | classic bootloader，与 app 同一 IDF v5.2.2 构建树 |
| `rr3-bootloader-esp32s3-dev.bin` | S3 bootloader，同上 |
| `rr3-partitions.csv` | 分区表真值（factory 0x2F0000 + spool 1MB） |
| `firmware-sha256.txt` | 全文件 sha256 |

拉取：`oras pull ghcr.io/<owner>/rockrobot3-firmware:<tag>`（或从 release 资产直接下载）。
`-dev` 表示构建期 `RR3_ALLOW_DEV_ADMIN_TOKEN=1`（公开缺省管理令牌），
不冒充生产凭据。

## 烧录（推荐：仓内脚本，校验后烧）

```bash
bash tools/deploy/flash-artifact.sh ./rr3-periph-base-esp32-safe-dev.elf \
  --bootloader ./rr3-bootloader-esp32-dev.bin --monitor

bash tools/deploy/flash-artifact.sh ./rr3-periph-base-esp32s3-full-dev.elf \
  --chip esp32s3 --bootloader ./rr3-bootloader-esp32s3-dev.bin
```

## 裸 espflash（不经脚本时三件必须显式给）

```bash
espflash flash --chip esp32 \
  --bootloader rr3-bootloader-esp32-dev.bin \
  --partition-table rr3-partitions.csv \
  rr3-periph-base-esp32-safe-dev.elf
```

## 为什么不能只给 ELF

- ESP-IDF ELF 只含 app 镜像，**不含** bootloader 与分区表。espflash 未给
  `--bootloader` 时用其自带引导镜像，IDF 版本与 app 不保证一致；本包随带同版
  bootloader，就是为了消掉这个错配。
- 未给 `--partition-table` 时 espflash 从当前目录猜 CSV。缺 `spool` 分区的表
  会让语音段静默降级——烧前确认 CSV 含 `spool` 行（脚本会 fail-closed 拒烧）。
- `nvs` 区（0x9000/0x6000）不在擦除范围，WiFi 凭据保留。

## 验收（串口 115200）

- **采集方式**：仓内 `tools/deploy/console-log.py`（raw 8N1，打开后**显式清
  DTR/RTS**）。不要用裸 `cat`/`stty`——打开 CH340 时 DTR/RTS 常停在置位态，
  芯片被按在复位/下载态，表现为"ROM 日志可读、随后全是噪声"，极易误判成
  固件控制台配错。自实现时同样必须先清两线再收流。
- boot 横幅前两行自证：`surface=...` 与 `console=uart0@<baud> secondary=none`
  ——能读到这两行，说明日志口与终端波特率都正确。
- 其余：boot 横幅点名芯片与 PSRAM；`[mem]` 给堆账，`[mic]` 给采样率（周期
  诊断行 10s 一行），full 档另有 `[spool]` 行。
- safe 档不连云；full 档需 NVS 已有 WiFi 与 API key，缺则显式点名。
