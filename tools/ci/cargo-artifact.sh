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
