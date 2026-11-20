#!/usr/bin/env python3
"""流水线结构审计 CLI（规则核在 workflow_rules.py，自检在 workflows_audit_selftest.py）。"""

import argparse
import pathlib
import sys

from workflow_rules import audit


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        from workflows_audit_selftest import self_test

        return self_test()
    bad = audit(pathlib.Path(a.root))
    for b in bad:
        print("FAIL  " + b)
    if bad:
        print(f"FAIL  流水线结构缺口 {len(bad)} 处（修法见 tools/ci/README.md）")
        return 1
    print(
        "PASS  流水线结构审计通过（并发组 / job 超时 / action 版本矩阵 / 权限与失败显形 / "
        "五芯片覆盖 / 发布面完整性 / 制品托管）"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
