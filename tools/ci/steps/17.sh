#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# ⑰ 发布/部署面清单对拍步骤体（批252 接门）。
#
# 判据住 tools/ci/audit/release_sets.py：仓内真值（modules/rr3-*/module.toml）与
# 发布策略（workflow_release.PUBLISH_MODULES + publish.yml 构建循环）、镜像面
# （stage-image.sh 两处装机循环、image-smoke.sh 起齐循环与阈值）、板侧包
# （pack-bundle.sh / pack-arm64.sh 按真值派生且保留 brain 例外）必须同一集合。
# 为什么单列一门：模块增删漏改任一处，流水线仍会绿，某个载体静默缺件——只在
# 真板装上才暴露。自检（1 正例 + 6 负例）归 gate 的 scripts job，本函数只执法。

rr3_step_17() {
  if ! command -v python3 >/dev/null 2>&1; then
    printf 'SKIP  ⑰ python3 缺位——清单对拍不执行（CI runner 自带 python3）\n'
    return
  fi
  if ! python3 tools/ci/audit/release_sets.py; then
    fail=1
  fi
}
