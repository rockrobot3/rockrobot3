#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# crates 下载抖动兜底单点（批267）：esp 门与跨架构对拍的构建走镜像源，共享出口偶发
# "transfer too slow / transferred 0 bytes"（2026-11-15 实测：一轮 20 分钟的 esp 门
# 白跑在 winnow 的下载上）。cargo 自带重试默认 3 次、低速阈值 10B/30s 太紧，这里放宽；
# 依赖源与锁文件都不动。此前七个平台脚本各抄一份同样的三行，策略一变就要改七处。
#
# 用法（在 root 确定后、任何 cargo 调用前 source；显式传值优先，不覆盖调用方环境）：
#   . "$root/tools/ci/lib/cargo-net.sh"
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-120}"
export CARGO_HTTP_LOW_SPEED_LIMIT="${CARGO_HTTP_LOW_SPEED_LIMIT:-1}"
