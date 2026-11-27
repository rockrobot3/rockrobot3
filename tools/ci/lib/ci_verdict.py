#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""CI 漂红判据（机制单点，2026-11-27 批172 自 ci-status.sh 拆出）。

输入：Actions runs 列表 JSON + 本地头 sha；输出：逐 workflow 结论行与退出码。
判据口径（与 tools/ci/README.md 同文）：
  0 = 各 workflow 最新一条均 success（cancelled 不算红：同 ref 新推送由并发组取消）
  1 = 任一 workflow 最新一条 failure / timed_out / action_required / stale
  2 = 任一最新一条仍在跑，或本地头没有任何 run（未定不冒充绿）

为什么要"任一仍在跑"而不是只看本地头的 run：纯文档提交会被 paths-ignore 跳过，
本地头没有 run，而上一头的平台腿可能还没跑完——那时代码结论未定，报 0 就是冒充
（2026-11-27 批172 实测：头绿 + 上一头 platforms in_progress 曾得退出 0）。
"""

import json
import sys

RED = {"failure", "timed_out", "action_required", "stale"}


def load_runs(path):
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
    if "workflow_runs" not in doc:
        raise ValueError(doc.get("message", "缺 workflow_runs"))
    return doc["workflow_runs"]


def latest_by_name(runs):
    latest = {}
    for run in runs:
        name = run.get("name") or "?"
        cur = latest.get(name)
        if cur is None or (run.get("created_at") or "") > (cur.get("created_at") or ""):
            latest[name] = run
    return latest


def mark(run):
    if (run.get("status") or "?") != "completed":
        return "RUNNING"
    conc = run.get("conclusion")
    if conc in RED:
        return "RED"
    if conc == "success":
        return "OK"
    return "CANCELLED"


def verdict(runs, local):
    latest = latest_by_name(runs)
    lines, has_red, has_pending, same_head = [], 0, 0, 0
    for name in sorted(latest):
        run = latest[name]
        on_head = bool(local) and (run.get("head_sha") or "") == local
        same_head += 1 if on_head else 0
        has_pending |= 1 if (run.get("status") or "?") != "completed" else 0
        has_red |= 1 if run.get("conclusion") in RED else 0
        tag = "  <- 本地头" if on_head else ""
        lines.append(
            f"{mark(run):9} {name:12} conclusion={(run.get('conclusion') or '-'):12}"
            f" head={(run.get('head_sha') or '')[:7]}{tag}"
        )
    if local and same_head == 0:
        lines.append(f"ci-status: 本地头 {local[:7]} 尚无 run（刚推送或全被 paths-ignore 跳过）")
    code = 1 if has_red else (2 if has_pending or (bool(local) and same_head == 0) else 0)
    return lines, code


def main():
    if len(sys.argv) != 3:
        print("用法: ci_verdict.py <runs.json> <本地头 sha|空>", file=sys.stderr)
        return 2
    try:
        runs = load_runs(sys.argv[1])
    except Exception as e:
        print(f"ci-status: 响应不可解析（{e}）", file=sys.stderr)
        return 2
    lines, code = verdict(runs, sys.argv[2])
    print("\n".join(lines))
    return code


if __name__ == "__main__":
    sys.exit(main())
