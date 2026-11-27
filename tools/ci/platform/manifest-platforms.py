#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""多架构合并清单判据（纯函数面）：合并结果必须真含三架构。

为什么单列：单腿都绿但合并丢一条，是"看起来绿"的经典形态——`imagetools inspect`
只打印不判等，必须把平台集合当判据。`buildx` 构建会在每腿附一条 attestation
manifest（provenance/SBOM，platform=unknown/unknown），它合法存在但不能算进
架构集（2026-11-15 v0.0.13 首跑就是被它判成假红）。

用法：
  docker buildx imagetools inspect --raw <image:tag> > /tmp/manifest.json
  python3 tools/ci/platform/manifest-platforms.py /tmp/manifest.json
  python3 tools/ci/platform/manifest-platforms.py --self-test
退出码：0 = 恰三架构；1 = 架构集不符/不是 index；2 = 用法错。
"""

import json
import sys

WANT = ["amd64", "arm64", "riscv64"]


def platforms(doc):
    """返回（排序后的架构集，attestation 条数）。"""
    plats, att = [], 0
    for m in doc.get("manifests", []):
        ann = m.get("annotations") or {}
        if ann.get("vnd.docker.reference.type") == "attestation-manifest":
            att += 1
            continue
        plats.append((m.get("platform") or {}).get("architecture", "?"))
    return sorted(plats), att


def check(doc):
    """判据本体：返回（是否通过，PASS/FAIL 文案）。"""
    if "manifests" not in doc:
        return False, "输入不是 OCI image index（无 manifests 数组）"
    plats, att = platforms(doc)
    if plats != WANT:
        return False, f"合并清单架构 {plats}（期望 {WANT}）"
    return True, f"多架构清单架构 {plats}（attestation {att} 条，不计入架构集）"


def self_test():
    ok = True
    good = {
        "manifests": [
            {"platform": {"architecture": "amd64"}},
            {"platform": {"architecture": "arm64"}},
            {"platform": {"architecture": "riscv64"}},
            {
                "platform": {"architecture": "unknown", "os": "unknown"},
                "annotations": {"vnd.docker.reference.type": "attestation-manifest"},
            },
        ]
    }
    bad = {"manifests": [{"platform": {"architecture": "amd64"}}]}
    not_index = {"schemaVersion": 2}
    for name, doc, want in (
        ("三架构+attestation", good, True),
        ("缺 arm64/riscv64", bad, False),
        ("非 index", not_index, False),
    ):
        got, msg = check(doc)
        if got != want:
            print(f"FAIL  self-test[{name}]：期望 {want} 得 {got}（{msg}）")
            ok = False
        elif want:
            print(f"PASS  self-test[{name}]：{msg}")
        else:
            print(f"PASS  self-test[{name}]：按预期拒绝（{msg}）")
    return 0 if ok else 1


def main():
    args = sys.argv[1:]
    if args == ["--self-test"]:
        return self_test()
    if len(args) != 1:
        print("用法: manifest-platforms.py <manifest.json> | --self-test", file=sys.stderr)
        return 2
    try:
        with open(args[0], encoding="utf-8") as f:
            doc = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        print(f"::error title=manifest-platforms::读清单失败: {e}")
        return 1
    ok, msg = check(doc)
    if not ok:
        print(f"::error title=manifest-platforms::{msg}")
        return 1
    print(f"PASS  {msg}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
