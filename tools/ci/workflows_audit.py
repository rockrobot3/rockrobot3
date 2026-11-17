#!/usr/bin/env python3
"""GitHub 流水线结构审计：并发组 / job 超时 / action 版本矩阵。

判据（文本扫描，不做 YAML 解析——workflow 由本仓独占维护，形状稳定）：
- 每个 workflow 必须有顶层 `concurrency:`（同 ref 重复跑互相踩是历史事故源）；
- 每个 job 必须有 `timeout-minutes:`（挂死不能烧满 6 小时默认额度）；
- `uses:` 的第三方 action 必须落在 ACTION_MATRIX 登记的引用上——版本升级
  走显式提交改本表，与 rust-toolchain 钉版同一纪律；本地 action（`./`）不查。

用法：
    workflows_audit.py [--root <仓根>]
    workflows_audit.py --self-test

退出码：0 = 通过；1 = 有缺口或自检失败。
"""

import argparse
import pathlib
import re
import sys

# action 引用矩阵（升级 = 显式提交改本表；值 = 允许的完整引用）
ACTION_MATRIX = {
    "actions/checkout": {"v7"},
    "actions/upload-artifact": {"v7"},
    "actions/download-artifact": {"v8"},
    "actions/cache": {"v4"},
    "Swatinem/rust-cache": {"v2"},
    "dtolnay/rust-toolchain": {"stable"},
    "docker/setup-qemu-action": {"v4"},
    "docker/setup-buildx-action": {"v3"},
    "docker/login-action": {"v3"},
    "taiki-e/install-action": {"v2"},
    "softprops/action-gh-release": {"v2"},
    "oras-project/setup-oras": {"v2"},
}

USES = re.compile(r"^\s*-?\s*uses:\s*(\S+)")
JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")


def uses_problems(text, where):
    """逐行核对 action 引用（本地 action 跳过）。"""
    bad = []
    for i, line in enumerate(text.splitlines(), 1):
        m = USES.match(line)
        if not m or m.group(1).startswith(("./", "docker://")):
            continue
        ref = m.group(1)
        if "@" not in ref:
            bad.append(f"{where}:{i} action 未钉版本: {ref}")
            continue
        name, ver = ref.rsplit("@", 1)
        allowed = ACTION_MATRIX.get(name)
        if allowed is None:
            bad.append(f"{where}:{i} action 未在矩阵登记: {ref}")
        elif ver not in allowed:
            bad.append(f"{where}:{i} action 版本漂移: {ref}（允许 {'/'.join(sorted(allowed))}）")
    return bad


def jobs_of(text):
    """返回 [(job 名, 块行)]——jobs: 之后的 2 空格键为 job 边界。"""
    jobs, name, buf = [], None, []
    in_jobs = False
    for line in text.splitlines():
        if re.match(r"^jobs:\s*$", line):
            in_jobs, name, buf = True, None, []
            continue
        if in_jobs and re.match(r"^\S", line):
            if name:
                jobs.append((name, buf))
            in_jobs, name, buf = False, None, []
            continue
        if not in_jobs:
            continue
        m = JOB.match(line)
        if m:
            if name:
                jobs.append((name, buf))
            name, buf = m.group(1), []
        elif name:
            buf.append(line)
    if name:
        jobs.append((name, buf))
    return jobs


def workflow_problems(path, root):
    text = path.read_text(encoding="utf-8")
    rel = path.relative_to(root)
    bad = uses_problems(text, str(rel))
    if not re.search(r"^concurrency:\s*$", text, re.M):
        bad.append(f"{rel} 缺顶层 concurrency（同 ref 并发组）")
    jobs = jobs_of(text)
    if not jobs:
        bad.append(f"{rel} 没有可识别的 job")
    for name, block in jobs:
        if not any(re.match(r"^\s{4}timeout-minutes:", l) for l in block):
            bad.append(f"{rel} job {name} 缺 timeout-minutes")
    return bad


def audit(root):
    bad = []
    wf_dir = root / ".github" / "workflows"
    files = sorted(wf_dir.glob("*.yml"))
    if not files:
        return ["未找到 .github/workflows/*.yml"]
    for p in files:
        bad.extend(workflow_problems(p, root))
    for p in sorted((root / ".github" / "actions").glob("*/action.yml")):
        bad.extend(uses_problems(p.read_text(encoding="utf-8"), str(p.relative_to(root))))
    return bad


def self_test():
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        wf = root / ".github" / "workflows"
        wf.mkdir(parents=True)
        (wf / "good.yml").write_text(
            "name: good\nconcurrency:\n  group: g\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - uses: actions/checkout@v7\n",
            encoding="utf-8",
        )
        (wf / "bad.yml").write_text(
            "name: bad\njobs:\n  b:\n    runs-on: ubuntu-latest\n"
            "    steps:\n      - uses: actions/checkout@v5\n",
            encoding="utf-8",
        )
        bad = audit(root)

    cases = [
        ("缺 concurrency 被抓", any("concurrency" in b for b in bad)),
        ("缺 timeout 被抓", any("timeout-minutes" in b for b in bad)),
        ("版本漂移被抓", any("版本漂移" in b for b in bad)),
        ("好例零误报", not any("good.yml" in b for b in bad)),
    ]
    for name, ok in cases:
        print(("PASS  " if ok else "FAIL  ") + name)
    return 0 if all(ok for _, ok in cases) else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    bad = audit(pathlib.Path(a.root))
    for b in bad:
        print("FAIL  " + b)
    if bad:
        print(f"FAIL  流水线结构缺口 {len(bad)} 处（修法见 tools/ci/README.md）")
        return 1
    print("PASS  流水线结构审计通过（并发组 / job 超时 / action 版本矩阵）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
