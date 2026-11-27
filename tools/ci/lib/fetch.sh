#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# cargo 依赖预取（有界重试）：托管 macOS runner 偶发注册表 DNS 解析失败
# （`Could not resolve host: index.crates.io`，2026-11-27 实测；cargo 自带 retry
# 不覆盖解析失败），一次瞬态就让整腿红、白跑一轮测试。本助手 3 次退避重试，
# 末次仍失败才点名退出——判据不变，只是把"网络噪音"与"真依赖错误"分开。
#
# 用法：
#   bash tools/ci/lib/fetch.sh                          # 当前目录 workspace
#   bash tools/ci/lib/fetch.sh --manifest-path <清单>    # 独立工程
set -uo pipefail

args=(fetch --locked)
case "${1:-}" in
  "")
    ;;
  --manifest-path)
    if [ $# -ne 2 ] || [ -z "${2}" ]; then
      echo "用法: $0 [--manifest-path <Cargo.toml>]" >&2
      exit 2
    fi
    args+=(--manifest-path "${2}")
    ;;
  *)
    echo "用法: $0 [--manifest-path <Cargo.toml>]" >&2
    exit 2
    ;;
esac

for i in 1 2 3; do
  if cargo "${args[@]}"; then
    exit 0
  fi
  echo "cargo fetch 失败（第 ${i}/3 次，5s 后重试）" >&2
  if [ "${i}" -lt 3 ]; then
    sleep 5
  fi
done
echo "::error title=cargo-fetch::cargo fetch 连续 3 次失败（网络/注册表不可达）" >&2
exit 1
