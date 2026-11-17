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


def trigger_block(lines, i):
    """`  push:` 之后的子块（缩进 ≥4 的行）。"""
    out = []
    for line in lines[i + 1 :]:
        if line.strip() and not line.startswith("    "):
            break
        out.append(line)
    return out


def trigger_problems(text, rel):
    """触发面：必须有 on:；push 必须收窄到 branches/tags；publish 必须吃 tag。"""
    bad = []
    if not re.search(r"^on:\s*$", text, re.M):
        bad.append(f"{rel} 缺顶层 on: 触发面")
        return bad
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if line.rstrip() != "  push:":
            continue
        block = trigger_block(lines, i)
        if not any(re.match(r"^\s+(branches|tags):", b) for b in block):
            bad.append(f"{rel} push 触发缺 branches/tags 范围（全分支推送会白烧 CI）")
        if rel.name == "publish.yml" and not any(
            re.match(r"^\s+tags:", b) for b in block
        ):
            bad.append(f"{rel} 发布面必须由 tag 触发（push.tags 缺失）")
    return bad


def needs_of(block):
    """job 块的 needs 列表（支持内联 [a, b] / 标量 / 多行 - a）。"""
    for idx, line in enumerate(block):
        m = re.match(r"^\s{4}needs:\s*(.*)$", line)
        if not m:
            continue
        rest = m.group(1).strip()
        if rest.startswith("["):
            return [x.strip().strip("'\"") for x in rest.strip("[]").split(",") if x.strip()]
        if rest:
            return [rest.strip("'\"")]
        out = []
        for nxt in block[idx + 1 :]:
            m2 = re.match(r"^\s{6}-\s*(\S+)", nxt)
            if not m2:
                break
            out.append(m2.group(1).strip("'\""))
        return out
    return []


def graph_problems(rel, jobs):
    """needs 图：引用必须存在，且不得成环。"""
    names = {n for n, _ in jobs}
    graph = {n: needs_of(b) for n, b in jobs}
    bad = []
    for n, deps in graph.items():
        for d in deps:
            if d not in names:
                bad.append(f"{rel} job {n} needs 指向不存在的 job {d}")
    if cycle := graph_cycle(graph):
        bad.append(f"{rel} needs 成环: {' -> '.join(cycle)}")
    return bad


def graph_cycle(graph):
    state, stack = {}, []

    def dfs(n):
        if state.get(n) == 1:
            return stack[stack.index(n) :] + [n]
        if state.get(n) == 2:
            return None
        state[n] = 1
        stack.append(n)
        for d in graph.get(n, []):
            if found := dfs(d):
                return found
        stack.pop()
        state[n] = 2
        return None

    for n in graph:
        if found := dfs(n):
            return found
    return None


def workflow_problems(path, root):
    text = path.read_text(encoding="utf-8")
    rel = path.relative_to(root)
    bad = uses_problems(text, str(rel))
    bad.extend(trigger_problems(text, rel))
    if not re.search(r"^concurrency:\s*$", text, re.M):
        bad.append(f"{rel} 缺顶层 concurrency（同 ref 并发组）")
    jobs = jobs_of(text)
    if not jobs:
        bad.append(f"{rel} 没有可识别的 job")
    for name, block in jobs:
        if not any(re.match(r"^\s{4}timeout-minutes:", l) for l in block):
            bad.append(f"{rel} job {name} 缺 timeout-minutes")
    bad.extend(graph_problems(rel, jobs))
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
            "name: good\non:\n  push:\n    branches: [main]\n"
            "concurrency:\n  group: g\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - uses: actions/checkout@v7\n",
            encoding="utf-8",
        )
        (wf / "bad.yml").write_text(
            "name: bad\non:\n  push:\njobs:\n  b:\n    runs-on: ubuntu-latest\n"
            "    needs: [nope]\n"
            "    steps:\n      - uses: actions/checkout@v5\n",
            encoding="utf-8",
        )
        (wf / "cycle.yml").write_text(
            "name: cyc\non:\n  push:\n    branches: [main]\n"
            "concurrency:\n  group: c\njobs:\n"
            "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    needs: [b]\n"
            "  b:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    needs: [a]\n",
            encoding="utf-8",
        )
        bad = audit(root)

    cases = [
        ("缺 concurrency 被抓", any("concurrency" in b for b in bad)),
        ("缺 timeout 被抓", any("timeout-minutes" in b for b in bad)),
        ("版本漂移被抓", any("版本漂移" in b for b in bad)),
        ("needs 悬空被抓", any("不存在的 job" in b for b in bad)),
        ("needs 成环被抓", any("成环" in b for b in bad)),
        ("push 无范围被抓", any("branches/tags" in b for b in bad)),
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
