#!/usr/bin/env bash
# aarch64-musl 静态产物门（树莓派 / 瑞芯微部署面）——兼容入口。
#
# 机制已收归 tools/ci/cross-artifacts.sh（多芯片共用一份构建与断言）；
# 本文件保留原命令名，供 tools/deploy/pack-bundle.sh 与既有文档继续按原名调用。
set -euo pipefail
exec bash "$(dirname "$0")/cross-artifacts.sh" \
  aarch64-unknown-linux-musl 'ARM aarch64'
