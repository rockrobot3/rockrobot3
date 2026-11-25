#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 文档门（check.sh ⑤）自检入口：把 steps/5.sh 的夹具四例接到 scripts 作业。
set -euo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: 不在 git 仓库内" >&2
  exit 1
}
# shellcheck disable=SC1090
. "$root/tools/ci/steps/5.sh"
check_docs_selftest
