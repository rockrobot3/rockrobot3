#!/usr/bin/env python3
"""unsafe 注释审计：生产面每个 `unsafe` 关键字都要有 SAFETY 前言。

判据（只做静态扫描，不编译）：
- 扫描 base/ modules/ tools/ 下的 .rs，跳过测试路径（/tests/、tests.rs、
  test_support.rs）——测试夹具的 unsafe 不承载产品安全前置；
- `unsafe` 按关键字匹配（`unsafe_shapes` 这类标识符不算）；
- 同一行或紧邻上方（到空行为止，最多 12 行）出现 `SAFETY` 即视为已注释；
  跨多行的 SAFETY 说明按"到空行"整段收，不要求贴在同一行。

基线纪律（与 gate-a/gate-d/行数/体积门同款）：`tools/ci/unsafe-baseline.txt`
是允许的未注释条数上限，只降不升。新增未注释 unsafe 即红；降到基线以下通过，
提示同批下调基线（基线不回写不算失败，但数字不会自己变小）。

用法：
    unsafe_audit.py [--baseline <文件>] [--verbose]

退出码：0 = 未超基线；1 = 超基线或基线文件坏/缺（fail-closed）。
"""

import argparse
import pathlib
import re
import sys

KEYWORD = re.compile(r"(?<![A-Za-z0-9_])unsafe(?![A-Za-z0-9_])")
ROOTS = ("base", "modules", "tools")
SKIP = ("/tests/", "/tests.rs", "/test_support.rs", "/target/")


def undocumented(path, lines):
    """返回该文件里未注释 unsafe 的行号列表。"""
    out = []
    for i, line in enumerate(lines):
        code = line.split("//", 1)[0]
        if not KEYWORD.search(code):
            continue
        if not has_safety(lines, i):
            out.append(i + 1)
    return out


def has_safety(lines, i):
    """同一行或紧邻上方（到空行为止，最多 12 行）是否有 SAFETY/# Safety。"""
    def mark(s):
        # `// SAFETY:` 与 `/// # Safety` 都是合法前言（后者是 unsafe fn 的调用方契约）
        low = s.lower()
        return "safety" in low

    if mark(lines[i]):
        return True
    j = i - 1
    seen = 0
    while j >= 0 and seen < 12:
        if mark(lines[j]):
            return True
        if not lines[j].strip():
            return False
        j -= 1
        seen += 1
    return False


def scan():
    hits = {}
    for root in ROOTS:
        for p in pathlib.Path(root).rglob("*.rs"):
            s = str(p)
            if any(k in s for k in SKIP):
                continue
            lines = p.read_text(errors="replace").splitlines()
            bad = undocumented(p, lines)
            if bad:
                hits[s] = bad
    return hits


def baseline_of(path):
    raw = pathlib.Path(path).read_text().strip()
    if not raw.isdigit():
        raise ValueError(f"基线不是非负整数: {raw!r}")
    return int(raw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--baseline", default="tools/ci/unsafe-baseline.txt")
    ap.add_argument("--verbose", action="store_true")
    a = ap.parse_args()

    try:
        base = baseline_of(a.baseline)
    except (OSError, ValueError) as e:
        print(f"FAIL  基线不可用（{a.baseline}）：{e}", file=sys.stderr)
        return 1

    hits = scan()
    total = sum(len(v) for v in hits.values())
    if a.verbose or total > base:
        for f, lines in sorted(hits.items(), key=lambda kv: -len(kv[1])):
            print(f"  {len(lines):3d}  {f}")
            if total > base:
                print("       行: " + ", ".join(map(str, lines[:12])))
    if total > base:
        print(f"FAIL  未注释 unsafe {total} > 基线 {base}（新增项先补 SAFETY 前言）")
        return 1
    note = "（基线可下调）" if total < base else ""
    print(f"PASS  未注释 unsafe {total} ≤ 基线 {base}{note}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
