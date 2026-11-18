#!/usr/bin/env python3
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
#
# 退出码: 0 = 打印到摘要; 1 = 有 check-run 但无摘要注解; 2 = API 不可读/无 check-run

import argparse
import json
import subprocess
import sys
import urllib.error
import urllib.request

DEFAULT_REPO = "rockrobot3/rockrobot3"
JOB = "sim-chain"
TITLE_KEY = "稳态"


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


def main():
    ap = argparse.ArgumentParser(description="读取 sim-chain 稳态摘要（check-run 注解，无需凭据）")
    ap.add_argument("--repo", default=DEFAULT_REPO)
    ap.add_argument("--head", default="")
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
    return 0


if __name__ == "__main__":
    sys.exit(main())
