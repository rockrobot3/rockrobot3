#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""规约门步骤清单审计：check.sh 的 step 序列必须与 gate_steps.txt 逐字同序。

判据（只做静态比对，不执行门禁）：
- 解析 check.sh 的 `step "..."` 与 `printf '\n=== ... ===\n'` 两种出口；
  `汇总` 不是门步骤，排除。
- 与 `tools/ci/baseline/gate_steps.txt` 的非注释行逐项比对：缺失/新增/改名/换序均红。
- 门号唯一：圈号（①…⑳）是一把门的身份，同一圈号出现两次即双义红
  （子号 ①b/⑨-a 不冲突；历史实证：⑭ 同时用于装载门与源文件行数门）。
- 任一侧打空即门禁失效（fail-closed）。

为什么需要它：门禁脚本是唯一真源，但"删掉一个 step"不会让任何现有门变红；
本审计把步骤清单变成显式契约，删/改/换序都必须同批改清单，diff 可见。

用法：
    gate_steps_audit.py
    gate_steps_audit.py --self-test
退出码：0 = 一致；1 = 漂移 / 打空 / 文件缺失。
"""

import re
import sys
from itertools import zip_longest
from pathlib import Path

STEP = re.compile(r'^\s*step "([^"]+)"')
PRINTF = re.compile(r"^\s*printf '\\n=== (.*) ===\\n'\s*$")
GATE_ID = re.compile("^([①-⑳][^ ]*)")
MANIFEST = "tools/ci/baseline/gate_steps.txt"
SCRIPT = "tools/ci/check.sh"
SUMMARY = "汇总"


def parse_steps(text):
    """check.sh 的 (行号, 标题) 序列；`汇总` 不是门步骤。"""
    out = []
    for lineno, line in enumerate(text.splitlines(), 1):
        m = STEP.match(line)
        if not m:
            m = PRINTF.match(line)
        if not m:
            continue
        title = m.group(1)
        if title != SUMMARY:
            out.append((lineno, title))
    return out


def load_manifest(path):
    lines = []
    for raw in Path(path).read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            lines.append(line)
    return lines


def compare(parsed, manifest):
    """逐项比对；返回失败清单（空 = 一致）。"""
    fails = []
    if not parsed:
        fails.append("check.sh 解析打空（step/printf 出口一个都没找到）")
    if not manifest:
        fails.append(f"{MANIFEST} 打空（清单无步骤）")
    if fails:
        return fails
    for idx, (got, want) in enumerate(zip_longest(parsed, manifest), 1):
        if got is None:
            fails.append(f"第 {idx} 步缺失：清单有「{want}」，check.sh 没有")
        elif want is None:
            fails.append(f"第 {idx} 步未登记：check.sh 有「{got[1]}」，清单没有")
        elif got[1] != want:
            fails.append(
                f"第 {idx} 步漂移：check.sh 第 {got[0]} 行「{got[1]}」≠ 清单「{want}」"
            )
    fails.extend(check_gate_ids(parsed))
    return fails


def check_gate_ids(parsed):
    """门号唯一性：同一门号出现两次即双义（子号 ①b/⑨-a 是独立门号）。"""
    fails = []
    seen = {}
    for lineno, title in parsed:
        m = GATE_ID.match(title)
        if not m:
            continue
        seen.setdefault(m.group(1), []).append(lineno)
    for gid, linenos in seen.items():
        if len(linenos) > 1:
            where = "、".join(str(n) for n in linenos)
            fails.append(
                f"门号 {gid} 重复使用（check.sh 第 {where} 行）"
                "——门号是一把门的身份，先改号再登记"
            )
    return fails


def self_test():
    """夹具自检：一致/删除/新增/改名/换序/门号重复/子号放行/打空八类判定。"""
    base_script = 'step "① a"\nprintf \'\\n=== ② b ===\\n\'\n'
    base_manifest = ["① a", "② b"]
    cases = [
        ("两步一致", compare(parse_steps(base_script), base_manifest), False),
        ("删除一步即红", compare(parse_steps('step "① a"\n'), base_manifest), True),
        ("新增一步即红", compare(parse_steps(base_script + 'step "③ c"\n'), base_manifest), True),
        ("改名即红", compare(
            parse_steps('step "① a2"\nprintf \'\\n=== ② b ===\\n\'\n'), base_manifest), True),
        ("换序即红", compare(
            parse_steps('step "② b"\nprintf \'\\n=== ① a ===\\n\'\n'), base_manifest), True),
        ("门号重复即红", compare(parse_steps('step "① a"\nstep "① c"\n'), ["① a", "① c"]), True),
        ("子号不同不误伤", compare(parse_steps('step "① a"\nstep "①b a"\n'), ["① a", "①b a"]), False),
        ("两侧打空即红", compare([], []), True),
    ]
    ok = True
    for name, fails, expect_fail in cases:
        passed = bool(fails) == expect_fail
        ok = ok and passed
        print(("PASS  " if passed else "FAIL  ") + "步骤审计自检：" + name)
        if not passed:
            for line in fails[:3]:
                print("      " + line)
    return 0 if ok else 1


def main():
    if "--self-test" in sys.argv[1:]:
        return self_test()
    root = Path(__file__).resolve().parents[3]
    try:
        parsed = parse_steps((root / SCRIPT).read_text(encoding="utf-8"))
        manifest = load_manifest(root / MANIFEST)
    except OSError as e:
        print(f"FAIL  步骤审计：读取失败 {e}", file=sys.stderr)
        return 1
    fails = compare(parsed, manifest)
    for line in fails:
        print(f"FAIL  {line}", file=sys.stderr)
    if fails:
        return 1
    print(f"PASS  规约门步骤清单一致（{len(manifest)} 步，顺序/名称逐字相同）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
