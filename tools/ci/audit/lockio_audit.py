#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# 锁内 I/O 审计门（续轮二十二）：把"人工巡检锁内日志"变成机器判据。
#
# 已实证的三类回潮（续轮十七/十九/二十各修一次，靠人眼复查不可持续）：
#   R1 同一行同时出现 `rr3_lock(` 与 `rr3_log(`——两个调用谁先求值都躲不掉
#      "持锁期间做磁盘 write"；
#   R2 `rr3_lock(` 出现在 `if`/`while`/`match` 的条件位置：Rust 会把临时
#      `MutexGuard` 延寿到整个语句（含块体）结束，块里打日志＝锁内 I/O；
#   R3 `rr3_lock(..).rr3_reg_(fanout|shadow|ctr_merge)` 直接调用——这三个方法
#      会写"待落盘日志"，锁内只收集、锁外统一落盘（`rr3_reg_with` 是唯一入口）。
#
# 扫面 = base/runtime/rrd/src 下全部 .rs 的生产面（行注释剥离；`#[cfg(test)]`
# 的 mod 块与 mod 声明摘除——只摘块、不截尾）。扫面打空即门禁失效，不放行。
# 本门不做全控制流分析：只拦上述三种形态，其余形态靠评审。
#
# 用法: python3 tools/ci/audit/lockio_audit.py [--self-test]

import pathlib
import re
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[3]
SRC = ROOT / "base" / "runtime" / "rrd" / "src"

COND_KW = re.compile(r"\b(if|while|match)\b")
R3 = re.compile(r"rr3_lock\([^)]*\)\s*\.\s*rr3_reg_(?:fanout|shadow|ctr_merge)\s*\(")


def strip_comments(text):
    return "\n".join(re.sub(r"//.*$", "", ln) for ln in text.splitlines())


def prod_lines(text):
    """生产面行（行号, 代码）：摘掉 #[cfg(test)] 的 mod 块/声明，其余逐行保留。"""
    lines = text.splitlines()
    out, i = [], 0
    while i < len(lines):
        if lines[i].strip().startswith("#[cfg(test)]"):
            j = i + 1
            while j < len(lines) and lines[j].strip().startswith("#["):
                j += 1
            if j < len(lines) and re.match(r"\s*mod\s+\w+\s*;", lines[j]):
                i = j + 1
                continue
            if j < len(lines) and re.match(r"\s*mod\s+\w+\s*\{", lines[j]):
                depth, k = 0, j
                while k < len(lines):
                    depth += lines[k].count("{") - lines[k].count("}")
                    if depth <= 0:
                        break
                    k += 1
                i = k + 1
                continue
        out.append((i + 1, lines[i]))
        i += 1
    return out


def block_from(lines, idx):
    """第 idx 行起的语句块文本（从该行首个 `{` 配平到闭括号）。"""
    text = "\n".join(ln for _, ln in lines[idx:])
    start = text.find("{")
    if start < 0:
        return ""
    depth = 0
    for k in range(start, len(text)):
        if text[k] == "{":
            depth += 1
        elif text[k] == "}":
            depth -= 1
            if depth == 0:
                return text[start : k + 1]
    return text[start:]


def scan_text(text):
    """返回 (违规, rr3_lock 出现次数)；违规为 (行号, 规则, 片段)。"""
    lines = prod_lines(strip_comments(text))
    bad, locks = [], 0
    for idx, (no, code) in enumerate(lines):
        if "rr3_lock(" not in code:
            continue
        locks += 1
        cut = code.strip()[:70]
        if "rr3_log(" in code:
            bad.append((no, "R1", cut))
        elif COND_KW.search(code) and "rr3_log(" in block_from(lines, idx):
            bad.append((no, "R2", cut))
        if R3.search(code):
            bad.append((no, "R3", cut))
    return bad, locks


def scan_file(path):
    return scan_text(path.read_text(encoding="utf-8", errors="replace"))


def run(root):
    files = sorted(root.rglob("*.rs"))
    total_locks, hits = 0, []
    for f in files:
        bad, locks = scan_file(f)
        total_locks += locks
        hits.extend((f.relative_to(root), no, rule, cut) for no, rule, cut in bad)
    return files, total_locks, hits


def self_test():
    cases = {
        "good.rs": (
            "fn f() {\n"
            "    let v = rr3_lock(&m).rr3_reg_x();\n"
            "    if !rr3_lock(&m).rr3_reg_lp_is_set() {\n"
            "        return;\n"
            "    }\n"
            "    rr3_log(\"ok\");\n"
            "}\n",
            set(),
        ),
        "r1.rs": (
            "fn f() { rr3_lock(&m).rr3_reg_x(); rr3_log(\"boom\"); }\n",
            {"R1"},
        ),
        "r2.rs": (
            "fn f() {\n    if rr3_lock(&m).rr3_reg_x() {\n        rr3_log(\"boom\");\n    }\n}\n",
            {"R2"},
        ),
        "r3.rs": (
            "fn f() { rr3_lock(&m).rr3_reg_fanout(&frame, 1); }\n",
            {"R3"},
        ),
        "t.rs": (
            "#[cfg(test)]\nmod tests {\n    fn f() { rr3_lock(&m).rr3_reg_x(); rr3_log(\"x\"); }\n}\n",
            set(),
        ),
    }
    with tempfile.TemporaryDirectory() as d:
        root = pathlib.Path(d)
        for name, (text, _) in cases.items():
            (root / name).write_text(text, encoding="utf-8")
        files, locks, hits = run(root)
        got = {(h[0].name, h[2]) for h in hits}
        want = {(name, rule) for name, (_, rules) in cases.items() for rule in rules}
        if len(files) != len(cases) or locks != 5:
            print(f"self-test FAIL: 扫面 {len(files)} 文件 / {locks} 处 lock（期望 {len(cases)} / 5）")
            return 1
        if got != want:
            print(f"self-test FAIL: 违规集 {sorted(got)} ≠ 期望 {sorted(want)}")
            return 1
        empty = scan_text("fn f() {}\n")
        if empty[1] != 0:
            print("self-test FAIL: 空扫面样例应报 0 处 lock")
            return 1
    print("self-test PASS: 好例零违规 / R1 R2 R3 三负例各命中 / 测试块不参与")
    return 0


def main():
    if "--self-test" in sys.argv[1:]:
        return self_test()
    if not SRC.is_dir():
        print(f"FAIL  ⑮ 扫面根不存在: {SRC}（fail-closed）")
        return 1
    files, locks, hits = run(SRC)
    if locks == 0 or not files:
        print(f"FAIL  ⑮ 扫面为空（{len(files)} 文件 / {locks} 处 rr3_lock）——门禁失效，不放行")
        return 1
    for path, no, rule, cut in hits:
        print(f"FAIL  ⑮ [{rule}] {path}:{no}  {cut}")
    if hits:
        return 1
    print(f"PASS  ⑮ 锁内 I/O 0 违规（扫面 {len(files)} 文件 / {locks} 处 rr3_lock）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
