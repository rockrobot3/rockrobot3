#!/usr/bin/env bash
# flash-esp32 —— 微型形态（ESP32）烧录封装脚本（0.0.5 增补轮六 批N9 件二，敞口 13
# 烧录封装半边收口；另三件镜像装配/OTA/回滚仍敞）。
#
# 为什么需要它：esp32-periph-base 的手工档 runner（.cargo/config.toml，`cargo +esp
# run` = 烧录 + monitor）没有版本钉与产物校验。espflash 4.x 构镜像时硬校验 ELF 内
# ESP-IDF app descriptor（缺它镜像被拒，esp32-uart-echo 批C④ 现场补丁 6d3a1ee 先证），
# 而 descriptor 的段尺寸随构建路径漂移——坏镜像应在烧录前拦下，不送上板。
#
# 与 6d3a1ee 先例不同源，尺寸不互借：那边是 esp-hal 裸金属手挂 EspAppDesc
# （.flash.appdesc 0x100 字节）；本工程走 espidf std 路径，descriptor 由 IDF v5.2.2
# 构建自带（.flash.appdesc 0x120 字节，2026-10-22 实测）。两路径各随其版本钉死。
#
# 用法: flash-esp32.sh [--release] [--monitor] [--port <dev>] [--check-only]
#   --release     release 构建（缺省 debug）
#   --monitor     烧录后进 monitor（退出键 Ctrl+]）
#   --port <dev>  串口设备（缺省 espflash 自动探测）
#   --check-only  干跑：构建 + descriptor 校验后即止，不触串口（无板机器的验证面）
#
# 前置门（产线脚本语义：缺件 FAIL 点名，非 CI 门 SKIP 口径）：espflash 4.x（实测
# 4.6.0）、esp 工具链含 rust-src（esp-1.90.0.0）、ldproxy、esp-clang、ESP-IDF v5.2.2
# 本地克隆（路径同 tools/esp32-periph-base/.cargo/config.toml [env] IDF_PATH；版本
# 钉死登记表见 tools/espidf-std-hello/README.md）。
set -euo pipefail

cd "$(dirname "$0")/../.." # 仓根

PROJ=tools/esp32-periph-base
ELF_NAME=esp32-periph-base
BUILD=debug
MONITOR=0
PORT=
CHECK_ONLY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --release) BUILD=release; shift ;;
    --monitor) MONITOR=1; shift ;;
    --port)
      [ $# -ge 2 ] || { echo "FAIL  --port 需要设备路径参数" >&2; exit 2; }
      PORT="$2"; shift 2 ;;
    --check-only) CHECK_ONLY=1; shift ;;
    *) echo "FAIL  未知参数: $1（用法见脚本头注释）" >&2; exit 2 ;;
  esac
done

die() { echo "FAIL  $1" >&2; exit 1; }

export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never

echo "== ① 工具链门（espflash 4.x + esp 工具链/rust-src + ldproxy + esp-clang + IDF v5.2.2）=="
espflash_ver="$(espflash --version 2>/dev/null)" || die "espflash 未安装（cargo install espflash --locked）"
# ${…} 截断必加：bash 3.2（macOS 默认）C locale 下把全角字符首字节并入变量名，
# $espflash_ver（ 直接写即 unbound variable——本脚本三处同坑（2026-10-22 实测）
if ! printf '%s\n' "$espflash_ver" | grep -qE '^espflash 4\.'; then
  die "espflash 非 4.x: ${espflash_ver}（版本钉 4.x，实测 4.6.0）"
fi
rustup toolchain list 2>/dev/null | grep -q '^esp' || die "esp 工具链未安装（rustup toolchain install esp）"
esp_sysroot="$(rustc +esp --print sysroot 2>/dev/null)" || die "rustc +esp 不可用"
[ -d "$esp_sysroot/lib/rustlib/src/rust" ] || die "esp 工具链缺 rust-src（build-std 依赖）"
command -v ldproxy >/dev/null 2>&1 || die "ldproxy 未安装（cargo install ldproxy）"
[ -e "$HOME/.espup/esp-clang/libclang.dylib" ] || die "esp-clang 缺位（~/.espup/esp-clang，bindgen 需 Xtensa 后端）"
[ -f "$HOME/.espressif/frameworks/esp-idf-v5.2.2/tools/idf.py" ] \
  || die "ESP-IDF v5.2.2 本地克隆缺位（~/.espressif/frameworks，见 tools/espidf-std-hello/README.md 登记表）"
echo "PASS  $espflash_ver + esp 工具链（rust-src 在位）+ ldproxy + esp-clang + IDF v5.2.2"

echo "== ② 构建（cargo +esp build，${BUILD}；镜像注入同 espidf-sentinel-check.sh ⑤）=="
build_args=(
  --config 'source.crates-io.replace-with="rsproxy-sparse"'
  --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'
)
if [ "$BUILD" = release ]; then build_args+=(--release); fi
# 必须在工程目录跑：cargo 只从 cwd 向上找 .cargo/config.toml，工程 target/build-std/
# [env] 配置在 $PROJ/.cargo/ 下，--manifest-path 不改变 config 搜索路径
if ! ( cd "$PROJ" && cargo +esp build "${build_args[@]}" ); then
  die "xtensa-esp32-espidf 构建（${BUILD}）失败"
fi

echo "== ③ 产物在树断言 =="
ELF="$PROJ/target/xtensa-esp32-espidf/$BUILD/$ELF_NAME"
[ -f "$ELF" ] || die "ELF 不在树: $ELF"
echo "PASS  ELF 在树: $ELF"

echo "== ④ descriptor 校验（.flash.appdesc 三查）=="
objdump_bin="$(find "$PROJ/.embuild/espressif/tools/xtensa-esp-elf" \
  -name 'xtensa-esp32-elf-objdump' -type f 2>/dev/null | head -1)"
[ -n "$objdump_bin" ] || die "xtensa-esp32-elf-objdump 缺位（.embuild 未装 xtensa esp-gcc？重跑构建）"
# 查一：节在册（espidf std 构建应自带；esp-hal 裸金属缺它即 6d3a1ee 同款症状）
"$objdump_bin" -h "$ELF" | grep -q '\.flash\.appdesc' || die "ELF 无 .flash.appdesc 节"
# 查二：节尺寸＝IDF v5.2.2 自带 0x120（objdump -h size 列；升级 IDF 需重验整链）
appdesc_size="$("$objdump_bin" -h "$ELF" | awk '$2==".flash.appdesc"{print $3}')"
[ "$appdesc_size" = "00000120" ] \
  || die ".flash.appdesc 尺寸 $appdesc_size ≠ 00000120（IDF v5.2.2 自带尺寸钉）"
# 查三：magic 0xABCD5432（节首四字节，文件内小端 32 54 cd ab）
"$objdump_bin" -s -j .flash.appdesc "$ELF" | grep -q 3254cdab \
  || die ".flash.appdesc 首四字节非 magic 0xABCD5432"
echo "PASS  .flash.appdesc 节在册（0x120 字节）+ magic 0xABCD5432"

if [ "$CHECK_ONLY" = 1 ]; then
  echo "== ⑤ 干跑止步（--check-only）：构建 + 校验已过，未触串口 =="
  exit 0
fi

echo "== ⑤ 烧录（espflash flash$([ "$MONITOR" = 1 ] && echo ' + monitor，退出键 Ctrl+]')）=="
flash_args=()
if [ -n "$PORT" ]; then flash_args+=(--port "$PORT"); fi
if [ "$MONITOR" = 1 ]; then flash_args+=(--monitor); fi
espflash flash "${flash_args[@]}" "$ELF" || die "espflash 烧录失败"
echo "PASS  烧录完成: $ELF → 板"
