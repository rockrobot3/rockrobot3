#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# rock3 CI 漂红巡检：只读 GitHub REST 的 runs 列表，按 workflow 名取最新一条，
# 回答"当前 dev 头在各条流水线上是什么结论"——把每拍手工 curl 的动作固化成一条命令。
#
# 判据（退出码；机制单点在 lib/ci_verdict.py，与 tools/ci/README.md 同文）：
#   0 = 各 workflow 最新一条均 success（cancelled 不算红：同 ref 新推送由并发组取消）
#   1 = 任一最新一条 failure / timed_out / action_required / stale
#   2 = 任一最新一条仍在跑，或本地头没有任何 run（未定，不冒充绿）
#
# 为什么"上一头仍在跑"也报 2：纯文档提交被 paths-ignore 跳过时本地头没有 run，
# 而上一头的平台腿可能还没跑完——只看本地头会把"未定"读成绿（2026-11-27 批172
# 实测病形：头绿 + 上一头 platforms in_progress，旧实现退出 0）。
#
# 只读面：匿名可读 runs 列表（公开仓库）；job 日志需鉴权（403 已实证），本脚本不碰。
# env GITHUB_TOKEN 可选，仅用于提高 API 限额（未设置即匿名）。
#
# 用法: bash tools/ci/ci-status.sh [--branch dev/hubs-01] [--repo owner/name] [--self-test]
set -u

verdict_py="$(cd "$(dirname "$0")" && pwd)/lib/ci_verdict.py"
# EXIT trap 在进程退出时跑，函数局部变量已出栈（set -u 下报 unbound）——用全局。
ST_DIR=""
st_cleanup() {
  [ -n "$ST_DIR" ] && rm -rf "$ST_DIR"
  return 0
}

# 自检：判据的四条退出码各一例，喂 JSON 夹具、不触网（网络面只做取数，不进判据）。
# 旧头 pending 一例是本批病形的回归钉：它曾经被判 0。
st_case() { # 名称 期望码 夹具
  local rc=0
  python3 "$verdict_py" "$3" "$st_head" >/dev/null 2>&1 || rc=$?
  if [ "$rc" -eq "$2" ]; then
    printf 'PASS  self-test/%s（退出码 %d）\n' "$1" "$rc"
  else
    printf 'FAIL  self-test/%s（期望 %d 实得 %d）\n' "$1" "$2" "$rc"
    st_fail=1
  fi
}

self_test() {
  local d
  d=$(mktemp -d)
  ST_DIR="$d"
  trap st_cleanup EXIT
  st_fail=0
  st_head=0123456789ab
  printf '{"workflow_runs":[{"name":"gate","status":"completed","conclusion":"success","head_sha":"%s","created_at":"2026-01-02T00:00:00Z"},{"name":"platforms","status":"completed","conclusion":"success","head_sha":"%s","created_at":"2026-01-02T00:00:00Z"}]}' \
    "$st_head" "$st_head" >"$d/green.json"
  printf '{"workflow_runs":[{"name":"gate","status":"completed","conclusion":"success","head_sha":"%s","created_at":"2026-01-02T00:00:00Z"},{"name":"platforms","status":"in_progress","conclusion":null,"head_sha":"000000000000","created_at":"2026-01-01T00:00:00Z"}]}' \
    "$st_head" >"$d/old-pending.json"
  printf '{"workflow_runs":[{"name":"gate","status":"completed","conclusion":"failure","head_sha":"%s","created_at":"2026-01-02T00:00:00Z"},{"name":"platforms","status":"completed","conclusion":"success","head_sha":"%s","created_at":"2026-01-02T00:00:00Z"}]}' \
    "$st_head" "$st_head" >"$d/red.json"
  printf '{"workflow_runs":[{"name":"gate","status":"completed","conclusion":"success","head_sha":"000000000000","created_at":"2026-01-02T00:00:00Z"}]}' >"$d/no-head.json"
  st_case 全绿过 0 "$d/green.json"
  st_case 旧头仍在跑报未定 2 "$d/old-pending.json"
  st_case 任一红 1 "$d/red.json"
  st_case 本地头无run报未定 2 "$d/no-head.json"
  [ "$st_fail" -eq 0 ] || return 3
  return 0
}

branch="dev/hubs-01"
repo="rockrobot3/rockrobot3"
while [ $# -gt 0 ]; do
  case "$1" in
    --branch) branch="${2:?--branch 缺参数}"; shift 2 ;;
    --repo) repo="${2:?--repo 缺参数}"; shift 2 ;;
    --self-test) self_test; exit $? ;;
    *)
      echo "用法: $0 [--branch <名>] [--repo <owner/name>]" >&2
      exit 2
      ;;
  esac
done

qlocal=$(git rev-parse HEAD 2>/dev/null || true)
enc=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1],safe=""))' "$branch")
# per_page=10 足够（同分支同 workflow 只取最新一条），并把响应压到 ~150KB——
# per_page=40 时 588KB 的响应在弱网/网关限速下会半包截断（curl 18/56），本脚本
# 因此偶发"API 请求失败"（2026-11-19 实测三取一败）。小包 + 有界重试。
url="https://api.github.com/repos/${repo}/actions/runs?branch=${enc}&per_page=10"
hdr=(-H "Accept: application/vnd.github+json")
if [ -n "${GITHUB_TOKEN:-}" ]; then
  hdr+=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
fi

tmp=$(mktemp "${TMPDIR:-/tmp}/rr3-ci-runs.XXXXXX")
trap 'rm -f "$tmp"' EXIT INT TERM
ok=0
for _ in 1 2 3; do
  if curl -s -m 25 "${hdr[@]}" -o "$tmp" "$url"; then
    ok=1
    break
  fi
  sleep 2
done
if [ "$ok" != 1 ]; then
  echo "ci-status: API 请求失败（网络或限流，已重试 3 次）" >&2
  exit 2
fi

python3 "$verdict_py" "$tmp" "$qlocal"
