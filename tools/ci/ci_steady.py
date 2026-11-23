#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# ci_steady.py —— 只读取数：把 CI 的 sim-chain 稳态摘要从 check-run 注解里捞出来。
#
# 为什么不读 job 日志：匿名读日志 403（已实证），注解是唯一无需凭据的通道。
# 判据口径：**取到数值才算绿**——找不到 check-run 或没有摘要注解都非 0 退出，
# 不拿"没查到"当"很健康"。
#
# 用法:
#   python3 tools/ci/ci_steady.py                 # 本地 HEAD
#   python3 tools/ci/ci_steady.py --head 2a03b7b
#   python3 tools/ci/ci_steady.py --repo owner/name
#   python3 tools/ci/ci_steady.py --head <sha> --assert   # 叠加实时性判据
#
# 退出码: 0 = 打印到摘要（--assert 时且判据全过）; 1 = 有 check-run 但无摘要注解;
#         2 = API 不可读/无 check-run; 3 = --assert 判据越界
#
# --assert 阈值口径（暂行，待多点跨环境后定稿）：稳态续命间隔与峰值均 ≤ 500ms
# （看门狗下界 1000ms 的一半）；积压丢帧/写超时/慢写恒 0；单次日志落盘 ≤ 50ms
# （与 rrd 的 RR3_SINK_SLOW_US 同源——慢写计数就是按这条线记的，判据不另立口径）；
# 冷启动 ≤ 2s。任何一项越界即 3，不静默。

import argparse
import json
import re
import subprocess
import sys
import urllib.error
import urllib.request

DEFAULT_REPO = "rockrobot3/rockrobot3"
JOB = "sim-chain"
TITLE_KEY = "稳态"
ASSERT_LIMITS = {
    "since_max": 500,
    "peak_max": 500,
    "fd": 0,
    "wr": 0,
    "slow": 0,
    "log_max_us": 50_000,
    "boot_max": 2_000,
}


def api(url):
    req = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json"})
    with urllib.request.urlopen(req, timeout=25) as r:
        return json.load(r)


def local_head():
    try:
        return subprocess.run(
            ["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True
        ).stdout.strip()
    except Exception:
        return ""


def find_job(repo, head):
    doc = api(f"https://api.github.com/repos/{repo}/commits/{head}/check-runs?per_page=100")
    for c in doc.get("check_runs", []):
        if c.get("name") == JOB:
            return c
    return None


def annotations(repo, run_id):
    return api(f"https://api.github.com/repos/{repo}/check-runs/{run_id}/annotations")


def parse_metrics(message):
    """摘要注解 → {键: 整数}；`k=v` 片段里只取纯数字，`?` 等非数值跳过。"""
    out = {}
    for k, v in re.findall(r"([a-z_]+)=([0-9]+)", message):
        out[k] = int(v)
    return out


def assert_limits(hits):
    """按 ASSERT_LIMITS 判据逐键核对；缺键也算未达标（不拿缺数当健康）。"""
    metrics = {}
    for x in hits:
        metrics.update(parse_metrics(x.get("message") or ""))
    fails = []
    for key, limit in ASSERT_LIMITS.items():
        if key not in metrics:
            fails.append(f"{key} 缺失")
        elif metrics[key] > limit:
            fails.append(f"{key}={metrics[key]} > {limit}")
    return metrics, fails


def main():
    ap = argparse.ArgumentParser(description="读取 sim-chain 稳态摘要（check-run 注解，无需凭据）")
    ap.add_argument("--repo", default=DEFAULT_REPO)
    ap.add_argument("--head", default="")
    ap.add_argument("--assert", dest="do_assert", action="store_true", help="按暂行阈值判实时性")
    a = ap.parse_args()
    head = a.head or local_head()
    if not head:
        print("ci_steady: 不在 git 仓库内且未给 --head", file=sys.stderr)
        return 2
    try:
        job = find_job(a.repo, head)
        if job is None:
            print(f"ci_steady: {head[:7]} 上没有 {JOB} check-run（刚推送或未触发）", file=sys.stderr)
            return 2
        anns = annotations(a.repo, job["id"])
    except urllib.error.HTTPError as e:
        print(f"ci_steady: API 拒绝（HTTP {e.code}，匿名限额用尽或仓库不可读）", file=sys.stderr)
        return 2
    except Exception as e:
        print(f"ci_steady: API 请求失败（{e}）", file=sys.stderr)
        return 2
    hits = [x for x in anns if TITLE_KEY in (x.get("title") or "")]
    for x in hits:
        print(f"{x.get('annotation_level')}\t{x.get('message')}")
    if not hits:
        print(f"ci_steady: {JOB} 的 check-run {job['id']} 没有稳态摘要注解", file=sys.stderr)
        return 1
    if not a.do_assert:
        return 0
    _, fails = assert_limits(hits)
    if fails:
        print("ci_steady: 实时性判据越界：" + "；".join(fails), file=sys.stderr)
        return 3
    print("ci_steady: 实时性判据全过（暂行阈值）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
