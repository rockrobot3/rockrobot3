#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 运行面模块清单单点（批239，2026-11-28）。
#
# 为什么需要它：六模块与三个入口的名字此前在 cross-artifacts / distro-smoke /
# install-smoke / arm64-deb / pack-bundle / bundle-contract 各抄一份字面量——
# 新增一个模块时漏改任一处，流水线仍会绿，而某个载体（deb/bundle/镜像）静默缺件。
# 本文件只回答"仓里有哪些模块"这一个问题：真值 = modules/rr3-*/module.toml。
#
# 谁该进哪个载体是**策略**，由消费方显式声明，不藏在这里：
#   · musl bundle 排除 brain（ring 需目标 C 工具链，板上原生构建）；
#   · 发布镜像/发布清单仍显式列六件（发布决策，由 workflow_release 审计钉住）。
#
# 用法（调用方在 set -e 下取赋值，空清单即失败，不静默跑空）：
#   . tools/ci/lib/runtime-members.sh
#   mods=$(rr3_modules "$ROOT")        # act brain connect interact sense power（每行一个）
#   bins=$(rr3_module_bins "$ROOT")    # rr3-act rr3-brain ...（每行一个）
#
# 返回：0 = 至少一个模块；1 = 一个都没有（modules/ 不在或全缺 module.toml）。
rr3_modules() {
  local root="${1:-.}" f name
  local -a found=()
  for f in "$root"/modules/rr3-*/module.toml; do
    [ -f "$f" ] || continue
    name=${f%/module.toml}
    found+=("${name##*/rr3-}")
  done
  [ "${#found[@]}" -gt 0 ] || return 1
  printf '%s\n' "${found[@]}" | sort
}

rr3_module_bins() {
  rr3_modules "${1:-.}" | sed 's/^/rr3-/'
}
