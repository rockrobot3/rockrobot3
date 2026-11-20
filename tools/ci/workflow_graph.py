"""流水线 needs 图审计：引用存在性 + 成环判定（与规则核分离，保持单文件 <500 行）。"""

import re


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


