#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# crates 下载抖动兜底（本地与 CI 同口径）：esp 门的构建走镜像源，共享出口偶发
# "transfer too slow / transferred 0 bytes"（2026-11-15 实测：一轮 20 分钟的 esp 门
# 白跑在 winnow 的下载上）。cargo 自带重试默认 3 次、低速阈值 10B/30s 太紧，这里放宽；
# 依赖源与锁文件都不动。
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-120}"
export CARGO_HTTP_LOW_SPEED_LIMIT="${CARGO_HTTP_LOW_SPEED_LIMIT:-1}"
# ESP32-S3 外设底座构建门（W1/T2，批N35 件八）。与 esp32-periph-base-check.sh
# 同款门禁形态：宿主工具链 check 必红（xtensa + build-std + esp-idf-sys），本脚本
# 单独覆盖。SKIP 纪律同款：工具链缺位 = 本地 SKIP / CI 即红（tools/ci/lib/skip.sh）；在位但编译失败 = FAIL；
# 退出 0 但产物不在树 = FAIL。
#
# 与 classic 门的差异只有三处：MCU=esp32s3、--target xtensa-esp32s3-espidf、ELF 路径；
# 外加芯片档 feature（`--no-default-features --features esp32s3`——esp-idf-sys 的
# rustc-cfg 不跨 crate 传播，本 crate 以 Cargo feature 表达芯片档，见 Cargo.toml）。
# 2026-11-27 批123：前置四查与 clippy 腿已抽 `tools/ci/lib/esp-env.sh` 公共实现，
# 两门只保留各自的构建腿；S3 门同批补齐此前漏掉的 clippy -D warnings 面。
# 芯片专属 sdkconfig 由 esp-idf-sys 按 MCU 自动叠加 sdkconfig.defaults.esp32s3
# （BUILD-OPTIONS.md：<path>.<mcu> 更具体者覆盖），本脚本不显式传 SDKCONFIG。
#
# 本门只验**构建面**；上板验收（Flash/PSRAM 识别、I2C 0x3C、OLED、双轴、ADC 标定、
# 双麦 DoA、与 classic 的字节/行为对拍）走 W1/T2 runbook，不属 CI。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——当前目录不在 git 仓库内。" >&2
  exit 1
}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
# 管理令牌门（httpd.rs 编译期断言）：本门只验构建面、不产生产镜像，显式允许
# 公开缺省令牌；生产镜像必须带 RR3_BOARD_ADMIN_TOKEN（见 tools/deploy/flash-esp32.sh）。
export RR3_ALLOW_DEV_ADMIN_TOKEN=1
fail=0

cd "$root/tools/esp32-periph-base" || { echo "ERROR: 无法进入 tools/esp32-periph-base" >&2; exit 1; }
# 产物定位共享单点：target-dir 由仓根 .cargo/config.toml 移出源码区，不得写死
# crate 内 target/（否则编译成功也会被判"ELF 不在树"）。
. "$root/tools/ci/lib/cargo-artifact.sh"
. "$root/tools/ci/lib/skip.sh"

# W7-b1a 分区表核验时间锚：先于本门任何构建创建（语义同 classic 门）。
part_marker=/tmp/rr3-part-marker-esp32s3
: > "$part_marker"

# 前置（工具链/目标/IDF/esp-clang/镜像/venv/芯片档 MCU）走公共实现：两条 esp 门
# 共用一份，2026-11-27 批123 起不再各写复制体。
. "$root/tools/ci/lib/esp-env.sh"
rr3_esp_preflight xtensa-esp32s3-espidf esp32s3-periph-base esp32s3

# ⑧pre clippy 门（2026-11-27 批123 补面）：classic 门自 2026-11-21 起就有 clippy
# -D warnings，S3 门此前只有构建面——同一 crate 的 S3 专属 cfg 分支因此零告警覆盖。
# 以固件最全特性档（esp32s3,autonomy,tts）+ all-targets 跑同一门。
if ! rr3_esp_clippy esp32s3-periph-base /tmp/rr3-esp32s3-periph-clippy.log \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3,autonomy,tts; then
  fail=1
fi

# target-dir 由仓根 .cargo/config.toml 统一到 /tmp/rr3-build（P0-5）
elf="$(rr3_cargo_artifact xtensa-esp32s3-espidf debug esp32-periph-base)" || {
  echo "FAIL  无法解析 cargo target_directory（metadata 不可用）" >&2
  exit 1
}
if cargo +esp build --locked \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3 \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$elf" ]; then
    echo "PASS  esp32s3-periph-base xtensa-esp32s3-espidf 构建（W1/T2 构建面）: $elf"
  else
    echo "FAIL  编译退出 0 但 ELF 不在树: $elf" >&2
    fail=1
  fi
else
  echo "FAIL  esp32-periph-base xtensa-esp32s3-espidf 构建失败" >&2
  fail=1
fi

# ⑦b W4-1 I2S TX 验收档编译面（spk-tone feature；默认固件零行为变化）
if cargo +esp build --locked \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3,spk-tone \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  echo "PASS  S3 spk-tone 档构建（W4-1 I2S TX 验收面）"
else
  echo "FAIL  S3 spk-tone 档构建失败" >&2
  fail=1
fi

# ⑦c W3-2a 自治档编译面（autonomy feature：SNTP 对时；默认固件零行为变化）
if cargo +esp build --locked \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3,autonomy \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  echo "PASS  S3 autonomy 档构建（W3-2a SNTP）"
else
  echo "FAIL  S3 autonomy 档构建失败" >&2
  fail=1
fi

# ⑦d W4-2 播放面编译（tts feature；与 autonomy 组合＝W6 全链固件形态；
#     与 spk-tone 互斥，故单独一条腿）
if cargo +esp build --locked \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3,autonomy,tts \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  echo "PASS  S3 autonomy+tts 档构建（W4-2 播放面）"
else
  echo "FAIL  S3 autonomy+tts 档构建失败" >&2
  fail=1
fi

# ⑦g W7-b1a 分区表落地核验（ADR-0031）：两芯片共用 partitions.csv，S3 腿同样
#     必须看到 spool 条目（跨芯片快速部署是本项目核心口径，不按芯片漂移）。
if rr3_partition_has xtensa-esp32s3-espidf spool "$part_marker" \
     "$root/tools/esp32-periph-base/partitions.csv"; then
  echo "PASS  S3 分区表含 spool 条目（W7-b1a 存储承载）"
else
  echo "FAIL  S3 分区表未含 spool（查 sdkconfig.defaults CUSTOM/FILENAME 与 partitions.csv）" >&2
  fail=1
fi

# ⑧ 纯逻辑单测编译面（同 classic 门 ⑧；xtensa 目标本机不可跑，--no-run 只验编译）
# panic=abort 下测试 harness 必须显式 `-Zpanic_abort_tests`；RUSTFLAGS 会覆盖
# config 的 target rustflags，故整条重写（与 [target.xtensa-esp32s3-espidf] 同值）。
s3_test_log=/tmp/rr3-esp32s3-periph-test.log
s3_test_rf="--cfg espidf_time64 -Zunstable-options -Cpanic=immediate-abort -Zpanic_abort_tests"
if RUSTFLAGS="$s3_test_rf" cargo +esp test --locked --no-run \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3,spk-tone,autonomy \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' \
     >"$s3_test_log" 2>&1; then
  echo "PASS  甲面单测编译（--no-run）：S3 目标纯逻辑层单测在编"
else
  errs=$(grep -E "error(\[|:)|panicked at" "$s3_test_log" | head -4 | tr '\n' ' ')
  echo "FAIL  esp32-periph-base S3 单测编译失败（cargo +esp test --no-run）: ${errs:0:900}" >&2
  tail -3 "$s3_test_log" >&2
  fail=1
fi

# ⑧b W4-2b 云 TTS/播放面测试码编译（tts 与 spk-tone 互斥，故单列一条）
s3_tts_test_log=/tmp/rr3-esp32s3-periph-tts-test.log
if RUSTFLAGS="$s3_test_rf" cargo +esp test --locked --no-run \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3,autonomy,tts \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' \
     >"$s3_tts_test_log" 2>&1; then
  echo "PASS  甲面 S3 tts 档单测编译（--no-run）：cloud/playout 测试码在编"
else
  errs=$(grep -E "error(\[|:)|panicked at" "$s3_tts_test_log" | head -4 | tr '\n' ' ')
  echo "FAIL  esp32-periph-base S3 tts 单测编译失败: ${errs:0:900}" >&2
  tail -3 "$s3_tts_test_log" >&2
  fail=1
fi

# ⑨ release 构建面（生产烧录档 = 发布 full 档）：dev 档 opt-level=2 只代表
# 开发构建，release 的 codegen/LTO 差异必须独立出产物证据；feature 集与
# publish firmware job 逐字同参（esp32s3,autonomy,tts），tag 时不再首次编译。
rel_elf="$(rr3_cargo_artifact xtensa-esp32s3-espidf release esp32-periph-base)" || {
  echo "FAIL  无法解析 cargo target_directory（metadata 不可用）" >&2
  exit 1
}
if cargo +esp build --locked --release \
     --target xtensa-esp32s3-espidf \
     --no-default-features --features esp32s3,autonomy,tts \
     --config 'source.crates-io.replace-with="rsproxy-sparse"' \
     --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"'; then
  if [ -f "$rel_elf" ]; then
    echo "PASS  S3 release 构建（生产烧录档 = full 档）: $rel_elf"
    # 发布固件集随带同版 bootloader（与 publish firmware job 同一导航函数）。
    if ! rel_boot="$(rr3_idf_bootloader "$rel_elf")"; then
      echo "FAIL  S3 release bootloader 不在产出树（发布集缺件）" >&2
      fail=1
    elif [ "$(head -c 1 "$rel_boot" | od -An -tx1 | tr -d ' \n')" != "e9" ]; then
      echo "FAIL  S3 release bootloader magic≠e9: $rel_boot" >&2
      fail=1
    else
      echo "PASS  S3 release bootloader 同树: $rel_boot"
    fi
  else
    echo "FAIL  S3 release 编译退出 0 但 ELF 不在树: $rel_elf" >&2
    fail=1
  fi
else
  echo "FAIL  S3 release 构建失败" >&2
  fail=1
fi

# ⑩ 生产烧录档装配（full 档 = 单文件整片镜像）：产线与无工具链机器只需整片
#    直烧 0x0。espflash 缺位按 SKIP 纪律（CI 预装 4.6.0）。
if [ -f "$rel_elf" ]; then
  if ! command -v espflash >/dev/null 2>&1; then
    rr3_skip "espflash 缺位——整片镜像装配不执行"
  fi
  merged=/tmp/rr3-build/esp32s3-full-merged.bin
  img_args=(--elf "$rel_elf" --chip esp32s3 --flash-size 16mb --out "$merged"
            --partitions "$root/tools/esp32-periph-base/partitions.csv")
  [ -n "${rel_boot:-}" ] && img_args+=(--bootloader "$rel_boot")
  if bash "$root/tools/deploy/image-esp32.sh" ${img_args[@]+"${img_args[@]}"} \
       >/tmp/rr3-esp32s3-merge.log 2>&1; then
    echo "PASS  S3 整片镜像: ${merged}（$(wc -c <"$merged" | tr -d ' ') B，直烧 0x0）"
  else
    echo "FAIL  S3 整片镜像装配失败" >&2
    tail -5 /tmp/rr3-esp32s3-merge.log >&2
    fail=1
  fi
fi

exit "$fail"
