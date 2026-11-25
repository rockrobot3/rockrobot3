#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""⑬ 提交面卫生：提交信息与提交身份不得含本机标识。

追踪文件扫描（git grep）看不到提交对象，故本脚本补该盲区。判据：
命中集合必须与 tools/ci/baseline/commit-msg-allow.txt 逐条相等——
新增命中即红（豁免表不许追加），表内哈希不再命中亦红（历史重写须重锚）。

退出码：0 干净 / 1 红 / 2 环境错误 / 3 非 git 仓库（调用方按 SKIP 处理）。
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

PAT = re.compile(r"/Users/|trae_projects|C:\\Users")
LOCAL_DOM = re.compile(r"@(?:localhost|[^@\s]*\.local)$")
ALLOW_REL = "tools/ci/baseline/commit-msg-allow.txt"
FMT = "%H%x1f%an%x1f%ae%x1f%cn%x1f%ce%x1f%B"


def _load_allow(root: Path) -> set[str]:
    path = root / ALLOW_REL
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as e:
        raise RuntimeError(f"豁免表不可读 {ALLOW_REL}：{e}") from e
    out = set()
    for line in text.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


def _hits(records: list[str]) -> set[str]:
    hits = set()
    for rec in records:
        parts = rec.split("\x1f")
        if len(parts) < 6:
            continue
        sha = parts[0].strip()
        if not sha:
            continue
        ident = " ".join(parts[1:5])
        body = "\x1f".join(parts[5:])
        bad = bool(PAT.search(body) or PAT.search(ident))
        bad = bad or bool(LOCAL_DOM.search(parts[2])) or bool(LOCAL_DOM.search(parts[4]))
        if bad:
            hits.add(sha)
    return hits


def _judge(hits: set[str], allow: set[str]) -> int:
    new = sorted(hits - allow)
    stale = sorted(allow - hits)
    for sha in new[:5]:
        print(f"NEW   {sha[:8]}（提交信息或身份含本机标识）")
    for sha in stale[:5]:
        print(f"STALE {sha[:8]}（豁免条目已不命中，历史被重写须重锚）")
    if not new and not stale:
        print(f"提交信息与身份无本机标识（{len(hits)} 条豁免命中）")
        return 0
    return 1


def _git_log() -> list[str]:
    out = subprocess.run(
        ["git", "log", "-z", f"--format={FMT}"],
        capture_output=True,
        check=False,
    )
    if out.returncode != 0:
        raise RuntimeError(f"git log 失败（退出码 {out.returncode}）")
    return out.stdout.decode("utf-8", "replace").split("\x00")


def _is_shallow() -> bool:
    out = subprocess.run(
        ["git", "rev-parse", "--is-shallow-repository"], capture_output=True, check=False
    )
    return out.stdout.decode().strip() == "true"


def _repo_root() -> Path:
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, check=False
    )
    if out.returncode != 0:
        raise SystemExit(3)
    return Path(out.stdout.decode().strip())


def _self_test() -> int:
    leak = "a" * 40 + "\x1fN\x1fn@h\x1fN\x1fn@h\x1f正文提到 /Users/x/ 的路径"
    clean = "b" * 40 + "\x1fN\x1fn@h\x1fN\x1fn@h\x1f正常提交"
    local = "c" * 40 + "\x1fN\x1fn@localhost\x1fN\x1fn@localhost\x1f正常提交"
    cases = [
        (_hits([clean]), set(), 0),
        (_hits([clean, leak]), {"a" * 40}, 0),
        (_hits([clean, leak]), set(), 1),
        (_hits([clean]), {"a" * 40}, 1),
        (_hits([local]), set(), 1),
    ]
    for hits, allow, want in cases:
        got = _judge_quiet(hits, allow)
        if got != want:
            print(f"FAIL 自检：hits={hits} allow={allow} 期望 {want} 实得 {got}")
            return 1
    print("PASS 提交面审计自检五例")
    return 0


def _judge_quiet(hits: set[str], allow: set[str]) -> int:
    new = hits - allow
    stale = allow - hits
    return 1 if new or stale else 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return _self_test()
    try:
        root = _repo_root()
        if _is_shallow():
            print("浅克隆无法校验提交面（actions/checkout 须 fetch-depth: 0）", file=sys.stderr)
            return 2
        payload = _git_log()
    except SystemExit:
        raise
    except RuntimeError as e:
        print(str(e), file=sys.stderr)
        return 2
    try:
        allow = _load_allow(root)
    except RuntimeError as e:
        print(str(e), file=sys.stderr)
        return 2
    return _judge(_hits(payload), allow)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
