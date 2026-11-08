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
