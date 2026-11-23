#!/usr/bin/env bash
# cargo 产物定位（规约门共享单点）。
#
# 仓根 .cargo/config.toml 把 target-dir 统一移出源码区（/tmp/rr3-build），
# 开发者还可用 CARGO_TARGET_DIR 覆盖；因此**不得**在门脚本里写死
# `<crate>/target/...`——产物根本不在那里，编译成功也会被判"不在树"。
# 本函数以 cargo 自报的 target_directory 为准（它已合并 config 与 env）。
#
# 用法（在目标工程目录内调用）：
#   rr3_cargo_artifact <target-triple> <debug|release> <bin>
# 失败（metadata 不可用）返回非 0，调用方须按门禁 FAIL 点名。

rr3_cargo_artifact() {
  local td
  td="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
        | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
  [ -n "$td" ] || return 1
  printf '%s/%s/%s/%s\n' "$td" "$1" "$2" "$3"
}

# 分区表条目核验（W7-b1a 共享单点）：返回 0 = 指定 triple 的构建树里存在
# **与仓源 partitions.csv 内容一致**的 out/partitions.csv，且同一 out 树里的
# partition-table.bin 含 label。2026-11-23 根因：缓存命中的构建不会重生成
# partition-table.bin，旧的 mtime 时间锚把正确表判红；改为内容对拍，既排除
# 旧绿表冒充，也不惩罚缓存复用。无 CSV 入参时回退旧 marker 语义。
# 用法：rr3_partition_has <target-triple> <label> <marker-file> [repo-csv]
rr3_partition_has() {
  local td f out_csv out_dir table
  td="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
        | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
  [ -n "$td" ] || return 1
  if [ -n "${4:-}" ]; then
    while IFS= read -r out_csv; do
      cmp -s "$4" "$out_csv" || continue
      out_dir="$(dirname "$(dirname "$out_csv")")"
      table="$(find "$out_dir" -name partition-table.bin -print -quit 2>/dev/null)"
      if [ -n "$table" ] && grep -qa -- "$2" "$table"; then
        return 0
      fi
    done < <(find "$td/$1/debug/build" -path '*esp-idf-sys-*/out/partitions.csv' 2>/dev/null)
  fi
  if [ -n "${3:-}" ]; then
    while IFS= read -r f; do
      if grep -qa -- "$2" "$f"; then
        return 0
      fi
    done < <(find "$td/$1/debug/build" -name partition-table.bin -newer "$3" 2>/dev/null)
  fi
  # 失败归因（只在红时打印）：本轮表里到底有哪些 label、CSV 有没有进 out/、
  # sdkconfig 的分区档位是什么——三者足以区分"配置没生效 / 文件没搬入 / 表没重建"。
  echo "DIAG 分区表核验未命中 triple=$1 label=$2 marker=${3:-} csv=${4:-}" >&2
  find "$td/$1/debug/build" -path '*esp-idf-sys-*/out/partitions.csv' -print >&2
  if [ -n "${3:-}" ]; then
    find "$td/$1/debug/build" -name partition-table.bin -newer "$3" 2>/dev/null \
      | while IFS= read -r f; do
          printf 'DIAG table=%s labels=' "$f" >&2
          grep -aoE 'nvs|phy_init|factory|spool' "$f" 2>/dev/null | sort -u | tr '\n' ',' >&2
          printf '\n' >&2
        done
    cfg="$(find "$td/$1/debug/build" -path '*esp-idf-sys-*/out/sdkconfig' -newer "$3" -print 2>/dev/null | head -1)"
    if [ -n "$cfg" ]; then
      grep -E '^CONFIG_PARTITION_TABLE_(CUSTOM|CUSTOM_FILENAME|SINGLE_APP)' "$cfg" >&2 || true
    fi
  fi
  return 1
}

# bootloader 定位（发布固件集共享单点）：与 app 同一 IDF 构建树，先用
# esp-idf-sys 复制到 ELF 同目录的那份（批N18 登记），没有再回 IDF 的 CMake
# 产出树找。两处都没有返回 1；调用方按各自纪律点名（发布面 fail-closed，
# 不得静默退回 espflash 自带镜像——IDF 版本与 app 不保证一致）。
# 用法（在目标工程目录内调用）：rr3_idf_bootloader <elf>
rr3_idf_bootloader() {
  local dir hit
  dir="$(dirname "$1")"
  if [ -s "$dir/bootloader.bin" ]; then
    printf '%s\n' "$dir/bootloader.bin"
    return 0
  fi
  hit=$(find "$dir/build" -type f \
        -path '*/out/build/bootloader/bootloader.bin' -print -quit 2>/dev/null)
  [ -n "$hit" ] || return 1
  printf '%s\n' "$hit"
}

# esp-clang 定位（ESP 门共享单点）：espup 把乐鑫 LLVM fork 的库目录挂到
# ~/.espup/esp-clang，宿主库名随平台而异（macOS libclang.dylib / Linux libclang.so）。
# 打印首个存在的库路径；缺位返回非 0，调用方按各自纪律 SKIP（CI 门）或 die（产线脚本）。
# 允许 RR3_ESP_CLANG_DIR 覆盖（非 espup 布局的等价安装）。
rr3_esp_clang_lib() {
  local d="${RR3_ESP_CLANG_DIR:-$HOME/.espup/esp-clang}"
  local f
  for f in "$d/libclang.dylib" "$d/libclang.so" \
           "$d/lib/libclang.dylib" "$d/lib/libclang.so"; do
    if [ -e "$f" ]; then
      printf '%s\n' "$f"
      return 0
    fi
  done
  return 1
}
