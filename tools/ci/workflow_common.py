# SPDX-License-Identifier: Apache-2.0
"""workflow 文本扫描公共件：job / 触发面 / 步骤块的切分与键查。

切法只有一份：workflow_rules（结构面）与 workflow_release（发布面）共用，
避免两处各写一套扫描逻辑后对同一文件给出不同结论。
"""

import re

JOB = re.compile(r"^  ([A-Za-z0-9_-]+):\s*$")

def step_block(lines, i):
    """第 i 行（`- uses:`）之后的步骤块（缩进更深的行）。"""
    indent = len(lines[i]) - len(lines[i].lstrip())
    out = []
    for line in lines[i + 1 :]:
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        out.append(line)
    return out


def has_key(block, key, value=None):
    """块内 `key:` 是否存在（给了 value 则值必须相等，去引号）。"""
    for line in block:
        m = re.match(rf"^\s+{re.escape(key)}:\s*(.*)$", line)
        if not m:
            continue
        got = m.group(1).strip().strip('"').strip("'")
        if value is None or got == value:
            return True
    return False


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


def top_block(lines, key):
    """顶层键（缩进 0）之后的子块。"""
    idx = next((i for i, l in enumerate(lines) if l.rstrip() == f"{key}:"), None)
    if idx is None:
        return []
    out = []
    for line in lines[idx + 1 :]:
        if line.strip() and not line.startswith("  "):
            break
        out.append(line)
    return out
