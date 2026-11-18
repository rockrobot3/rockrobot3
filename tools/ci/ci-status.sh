#!/usr/bin/env bash
# rock3 CI 漂红巡检：只读 GitHub REST 的 runs 列表，按 workflow 名取最新一条，
# 回答"当前 dev 头在各条流水线上是什么结论"——把每拍手工 curl 的动作固化成一条命令。
#
# 判据（退出码）：
#   0 = 与本地头同提交的各 workflow 最新 run 均 success（cancelled 不算红：同 ref
#       新推送由并发组取消，由下一拍补覆盖，口径见计划册 §4）
#   1 = 存在 failure / timed_out / action_required / stale
#   2 = 仍有 in_progress / queued / pending，或 API 不可读（未定，不冒充绿）
#
# 只读面：匿名可读 runs 列表（公开仓库）；job 日志需鉴权（403 已实证），本脚本不碰。
# env GITHUB_TOKEN 可选，仅用于提高 API 限额（未设置即匿名）。
#
# 用法: bash tools/ci/ci-status.sh [--branch dev/hubs-01] [--repo owner/name]
set -u

branch="dev/hubs-01"
repo="rockrobot3/rockrobot3"
while [ $# -gt 0 ]; do
  case "$1" in
    --branch) branch="${2:?--branch 缺参数}"; shift 2 ;;
    --repo) repo="${2:?--repo 缺参数}"; shift 2 ;;
    *)
      echo "用法: $0 [--branch <名>] [--repo <owner/name>]" >&2
      exit 2
      ;;
  esac
done

qlocal=$(git rev-parse HEAD 2>/dev/null || true)
enc=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1],safe=""))' "$branch")
url="https://api.github.com/repos/${repo}/actions/runs?branch=${enc}&per_page=40"
hdr=(-H "Accept: application/vnd.github+json")
if [ -n "${GITHUB_TOKEN:-}" ]; then
  hdr+=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
fi

tmp=$(mktemp "${TMPDIR:-/tmp}/rr3-ci-runs.XXXXXX")
trap 'rm -f "$tmp"' EXIT INT TERM
if ! curl -s -m 25 "${hdr[@]}" -o "$tmp" "$url"; then
  echo "ci-status: API 请求失败（网络或限流）" >&2
  exit 2
fi

python3 - "$tmp" "${qlocal}" "$branch" <<'PY'
import json
import sys

path, local, branch = sys.argv[1], sys.argv[2], sys.argv[3]
try:
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
except Exception as e:  # 响应不是 JSON：多半是网关错误页
    print(f"ci-status: 响应不可解析（{e}）", file=sys.stderr)
    sys.exit(2)
if "workflow_runs" not in doc:
    print(f"ci-status: API 拒绝或限流（{doc.get('message', '缺 workflow_runs')}）", file=sys.stderr)
    sys.exit(2)

latest = {}
for run in doc["workflow_runs"]:
    name = run.get("name") or "?"
    if name not in latest or (run.get("created_at") or "") > (latest[name].get("created_at") or ""):
        latest[name] = run

red = {"failure", "timed_out", "action_required", "stale"}
pending = {"in_progress", "queued", "pending", "requested", "waiting"}
has_red = has_pending = 0
same_head = 0
for name in sorted(latest):
    run = latest[name]
    sha = run.get("head_sha") or ""
    status = run.get("status") or "?"
    conc = run.get("conclusion") or "-"
    on_head = bool(local) and sha == local
    if on_head:
        same_head += 1
        if status != "completed":
            has_pending = 1
        elif conc in red:
            has_red = 1
    if status != "completed":
        mark = "RUNNING"
    elif conc in red:
        mark = "RED"
    elif conc == "success":
        mark = "OK"
    else:
        mark = "CANCELLED"
    print(f"{mark:9} {name:12} conclusion={conc:12} head={sha[:7]}{'  <- 本地头' if on_head else ''}")

if not local:
    print("ci-status: 不在 git 仓库内，跳过'本地头'过滤（上面是各 workflow 最新一条）")
elif same_head == 0:
    print(f"ci-status: 本地头 {local[:7]} 在 {branch} 上尚无 run（刚推送或全被 paths-ignore 跳过）")
code = 1 if has_red else (2 if has_pending or (bool(local) and same_head == 0) else 0)
sys.exit(code)
PY
