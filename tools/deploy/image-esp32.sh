#!/usr/bin/env bash
# image-esp32 —— 微型形态（ESP32）镜像装配件（0.0.5 增补轮六 批N18 件一，敞口 13
# 产线闭环四件之"镜像装配"；另两件 OTA/回滚语义裁定随件二/件三同批落档）。
#
# 为什么需要它：产线交付需要一个"从 ELF 到可直接烧录的完整镜像"的装配面——
# [flash-esp32.sh](flash-esp32.sh) 是"构建+校验+烧板"（板在手），本件是"构建+
# 校验+装配成单镜像"（板不在手也能产出产线交付物）。espflash 4.x save-image
# --merge 把 bootloader、分区表与 app 段按 flash 偏移合成单文件（0x1000/0x8000/
# 0x10000 三段，缺省 padding 到整片 flash 尺寸——本工程 4MB 板恰 4194304 B，供
# esptool write_flash_address 0x0 整片直烧或产线复制分发），随附布局三点 magic
# 断言与整片尺寸断言——装配错误应在出厂前拦下，不上产线。
#
# 验证面真值单点：构建与 descriptor 三查复用 flash-esp32.sh --check-only（批N9
# 件二五步链：工具链门→构建→产物在树断言→descriptor 三查→干跑止步），本件不抄
# 第二份——验证判据变了只改一处。
#
# 用法: image-esp32.sh [--release]
#   --release     release 构建（缺省 debug）
# 本件全程不触串口（装配是纯文件面）——直接运行即干跑语义，无板机器可验
# （先例：flash-esp32.sh --check-only，批N9 件二口径）。前置门与工具链要求
# 同 flash-esp32.sh（espflash 4.x / esp 工具链 / ldproxy / esp-clang / IDF v5.2.2）。
#
# 分区事实（2026-10-23 侦查手读 partition-table.bin，magic 0xAA50 逐项）：
# nvs@0x9000/0x6000（批N14/15 凭据源）+ phy_init@0xf000/0x1000 +
# factory@0x10000/0x100000 —— factory 单槽、无 ota_0/ota_1/otadata，是批N18
# 件二/件三（升级=重烧即升级、不可回滚终态裁定）的物理前提；语义明文落
# base/ota/partition-layout.md 批N18 段。
set -euo pipefail

cd "$(dirname "$0")/../.." # 仓根

BUILD=debug
while [ $# -gt 0 ]; do
  case "$1" in
    --release) BUILD=release; shift ;;
    *) echo "FAIL  未知参数: $1（用法见脚本头注释）" >&2; exit 2 ;;
  esac
done

die() { echo "FAIL  $1" >&2; exit 1; }

echo "== ① 验证面（flash-esp32.sh --check-only：工具链门+构建+descriptor 三查）=="
verif_args=()
if [ "$BUILD" = release ]; then verif_args+=(--release); fi
# 哨兵展开必加（批N9 教训）：bash 3.2 + set -u 下空数组 "${arr[@]}" 即 unbound
# variable——空时不展开任何词
bash tools/deploy/flash-esp32.sh --check-only ${verif_args[@]+"${verif_args[@]}"} \
  || die "验证面未过（flash-esp32.sh --check-only）"

ARTDIR="tools/esp32-periph-base/target/xtensa-esp32-espidf/${BUILD}"
OUT="$ARTDIR/esp32-periph-base-merged.bin"

echo "== ② 装配面（save-image --merge：bootloader+分区表+app 按偏移合单镜像）=="
export PATH="$HOME/.cargo/bin:$PATH"
espflash_ver="$(espflash --version 2>/dev/null)" || die "espflash 未安装（cargo install espflash --locked）"
# ${…} 截断必加（批N9 教训）：bash 3.2 C locale 下 $espflash_ver 后接全角字符
# 即 unbound variable
printf '%s\n' "$espflash_ver" | grep -qE '^espflash 4\.' \
  || die "espflash 非 4.x: ${espflash_ver}（版本钉 4.x，实测 4.6.0）"
espflash save-image --chip esp32 --merge --flash-size 4mb \
  "$ARTDIR/esp32-periph-base" "$OUT" || die "save-image 装配失败"
echo "PASS  merged 镜像在树: $OUT"

echo "== ③ 自校验面（merged 布局三点 magic + 整片尺寸）=="
# 查一：bootloader@0x1000 首字节 0xE9（ESP image magic，2026-10-23 实测钉值）
boot_magic="$(xxd -p -s 4096 -l 1 "$OUT")"
[ "$boot_magic" = "e9" ] || die "bootloader@0x1000 首字节 $boot_magic ≠ e9"
# 查二：分区表@0x8000 头两字节 AA50（分区表条目 magic；与 2026-10-23 侦查
# partition-table.bin 手读同源）
pt_magic="$(xxd -p -s 32768 -l 2 "$OUT")"
[ "$pt_magic" = "aa50" ] || die "分区表@0x8000 头两字节 $pt_magic ≠ aa50"
# 查三：app@0x10000 首字节 0xE9（factory 槽 app image magic）
app_magic="$(xxd -p -s 65536 -l 1 "$OUT")"
[ "$app_magic" = "e9" ] || die "app@0x10000 首字节 $app_magic ≠ e9"
# 查四：整片尺寸恰 4MB（merge 缺省 padding 到 flash 全尺寸 4194304 B——
# 非整片即装配参数漂移，直烧面失效）
img_bytes="$(wc -c < "$OUT" | tr -d ' ')"
[ "$img_bytes" = "4194304" ] || die "镜像尺寸 $img_bytes ≠ 4194304（4MB 整片；merge padding 行为漂移即红）"
echo "PASS  布局三点 magic（0x1000=e9 / 0x8000=aa50 / 0x10000=e9）+ 整片 4194304 B"

# ${…} 截断必加（批N9 教训）：$OUT 后紧跟全角括号，bash 3.2 C locale 下即 unbound
# variable（2026-10-23 实测，本脚本第四处同坑）
echo "== ④ 产线交付物就绪: ${OUT}（整片直烧面，不触串口）=="
