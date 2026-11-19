#!/usr/bin/env python3
"""基线只降不升审计：把 baseline 文件与 base ref 的同名文件逐键比对。

判据（只做静态比对，不跑门禁本体）：
- 已存在的键：当前值 > 基线值即红（只降不升）；下降/持平通过。
- 新键：只告警（新门的新预算须评审，不能与"放水"混为一谈）。
- 缺键：只告警（键被删时对应门通常 fail-closed，删键救不了它）。
- base ref 无该文件：告警跳过（新基线文件，无棘轮可比）。
- 基线形状坏 / 未登记基线文件：红（fail-closed）。

base ref 解析顺序：
  1. `--base` 显式参数；
  2. PR：`GITHUB_BASE_REF` → `origin/<base>`；
  3. `GITHUB_EVENT_NAME=push`：`HEAD~1`（一次推送一个提交的纪律；
     多提交推送须逐提交审计）；
  4. 本地：`origin/dev/hubs-01`（存在时），否则 `HEAD~1`。
比对点是 `merge-base(HEAD, ref)`，避免把 base 分支自身的前进误判成回涨。

用法：
    baselines_audit.py [--base <ref>] [--root <dir>]
    baselines_audit.py --self-test
退出码：0 = 无回涨；1 = 有回涨 / 形状坏 / 基准不可用。
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

# 已登记基线文件：新增基线必须同批加入本表；未登记文件由审计判红。
BASELINES = (
    "tools/ci/gate-a-baseline.txt",
    "tools/ci/gate-d-baseline.txt",
    "tools/ci/gate-lines-baseline.txt",
    "tools/ci/gate-size-baseline.txt",
    "tools/ci/gate-w-baseline.txt",
    "tools/ci/gate-win-baseline.txt",
    "tools/ci/metrics-baseline.json",
    "tools/ci/unsafe-baseline.txt",
)
KEY_LINE = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(\d+)$")
BARE_INT = re.compile(r"^\d+$")


def repo_root():
    p = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True, text=True, check=False,
    )
    if p.returncode != 0:
        raise RuntimeError("不在 git 仓库内")
    return p.stdout.strip()


def _git(root, *args):
    return subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)


def _ref_exists(root, ref):
    return _git(root, "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}").returncode == 0


def resolve_base(root, explicit=None):
    """基准 ref：显式参数 > PR base > CI push 上一提交 > 本地远端 dev 分支。"""
    if explicit:
        return explicit
    base_env = os.environ.get("GITHUB_BASE_REF")
    if base_env:
        return f"origin/{base_env}"
    if os.environ.get("GITHUB_EVENT_NAME") == "push":
        return "HEAD~1"
    for cand in ("origin/dev/hubs-01", "github/dev/hubs-01", "gitcode/dev/hubs-01", "HEAD~1"):
        if _ref_exists(root, cand):
            return cand
    raise RuntimeError("找不到可用基准 ref（origin/dev/hubs-01 / HEAD~1）")


def compare_point(root, ref):
    """比对点 = merge-base；无共同祖先时退回 ref 本身。"""
    p = _git(root, "merge-base", "HEAD", ref)
    if p.returncode == 0 and p.stdout.strip():
        return p.stdout.strip()
    if not _ref_exists(root, ref):
        raise RuntimeError(f"基准 ref 不可用: {ref}")
    return ref


def _show(root, ref, path):
    p = _git(root, "show", f"{ref}:{path}")
    return p.stdout if p.returncode == 0 else None


def parse_txt(text):
    """文本基线：`名 = 整数` 键值，或按出现顺序编号的裸整数行。"""
    out, bare = {}, 0
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        m = KEY_LINE.match(line)
        if m:
            out[m.group(1)] = int(m.group(2))
            continue
        if BARE_INT.match(line):
            bare += 1
            out[f"bare#{bare}"] = int(line)
            continue
        raise ValueError(f"无法解析行 {raw!r}")
    return out


def _flatten(obj, prefix, out):
    for key, val in obj.items():
        name = f"{prefix}.{key}" if prefix else key
        if isinstance(val, dict):
            _flatten(val, name, out)
        elif isinstance(val, bool):
            continue
        elif isinstance(val, (int, float)):
            out[name] = val


def parse_json(text):
    data = json.loads(text)
    if not isinstance(data, dict):
        raise ValueError("顶层不是对象")
    out = {}
    _flatten(data, "", out)
    return out


def _load(path):
    text = Path(path).read_text(encoding="utf-8")
    return parse_json(text) if str(path).endswith(".json") else parse_txt(text)


def audit(root, point, files=BASELINES, strict_unlisted=True):
    """返回 (fails, warns, infos)；fails 非空即整门红。"""
    fails, warns, infos = [], [], []
    root = Path(root)
    if strict_unlisted:
        known = set(files)
        for p in sorted((root / "tools" / "ci").glob("*-baseline.*")):
            rel = p.relative_to(root).as_posix()
            if rel not in known:
                fails.append(f"未登记基线文件 {rel}（新增基线须加入审计表）")
    for rel in files:
        cur_path = root / rel
        if not cur_path.is_file():
            warns.append(f"{rel} 不存在（审计跳过；门禁应 fail-closed）")
            continue
        try:
            cur = _load(cur_path)
        except (OSError, ValueError, json.JSONDecodeError) as e:
            fails.append(f"{rel} 形状坏: {e}")
            continue
        base_text = _show(str(root), point, rel)
        if base_text is None:
            warns.append(f"{rel} 在基准 {point} 不存在（新基线，无棘轮可比）")
            continue
        try:
            base = parse_json(base_text) if rel.endswith(".json") else parse_txt(base_text)
        except (ValueError, json.JSONDecodeError) as e:
            fails.append(f"{rel} 基准形状坏（{point}）: {e}")
            continue
        for key, val in cur.items():
            if key not in base:
                warns.append(f"{rel}: 新键 {key}={val}（新预算须评审）")
                continue
            old = base[key]
            if val > old:
                fails.append(f"{rel}: {key} {old} → {val}（只降不升）")
            elif val < old:
                infos.append(f"{rel}: {key} {old} → {val}（可同批下调）")
        for key, old in base.items():
            if key not in cur:
                warns.append(f"{rel}: 缺键 {key}（原 {old}）")
    return fails, warns, infos


def _write(path, text):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _commit(root, msg):
    subprocess.run(
        ["git", "-c", "user.email=ci@example.invalid", "-c", "user.name=ci",
         "-c", "commit.gpgsign=false", "commit", "-q", "-m", msg],
        cwd=root, check=True,
    )


def self_test():
    """夹具自检：上调/下调/新键/缺键/未登记五类判定各一条。"""
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        txt = root / "tools/ci/gate-x-baseline.txt"
        js = root / "tools/ci/metrics-baseline.json"
        _write(txt, "# 基线\n5\n")
        _write(js, '{"unwrap_max": 10}\n')
        subprocess.run(["git", "init", "-q"], cwd=td, check=True)
        subprocess.run(["git", "add", "-A"], cwd=td, check=True)
        _commit(td, "base")
        files = ("tools/ci/gate-x-baseline.txt", "tools/ci/metrics-baseline.json")

        def run_case(name, text_txt, text_json, expect_fail):
            _write(txt, text_txt)
            _write(js, text_json)
            fails, warns, _ = audit(td, "HEAD", files, strict_unlisted=False)
            got = bool(fails)
            return (f"{name}（失败 {len(fails)} / 告警 {len(warns)}）", got == expect_fail, fails)

        cases = [
            run_case("文本基线上调即红", "6\n", '{"unwrap_max": 10}\n', True),
            run_case("文本基线下调放行", "4\n", '{"unwrap_max": 10}\n', False),
            run_case("JSON 基线上调即红", "5\n", '{"unwrap_max": 11}\n', True),
            run_case("新增 JSON 键只告警", "5\n", '{"unwrap_max": 10, "new_max": 1}\n', False),
            run_case("删键只告警", "5\n", "{}\n", False),
        ]
        _write(root / "tools/ci/extra-baseline.txt", "1\n")
        fails, _, _ = audit(td, "HEAD", files, strict_unlisted=True)
        cases.append(("未登记基线文件即红", any("未登记" in f for f in fails), fails))

    for name, ok, detail in cases:
        print(("PASS  " if ok else "FAIL  ") + "基线自检：" + name)
        if not ok:
            for line in detail[:3]:
                print("      " + line)
    return 0 if all(ok for _, ok, _ in cases) else 1


def main():
    if "--self-test" in sys.argv[1:]:
        return self_test()
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", help="基准 ref（缺省按 CI/本地口径解析）")
    ap.add_argument("--root", help="仓库根（缺省 git rev-parse）")
    a = ap.parse_args()
    try:
        root = a.root or repo_root()
        ref = resolve_base(root, a.base)
        point = compare_point(root, ref)
    except RuntimeError as e:
        print(f"FAIL  基线审计：{e}", file=sys.stderr)
        return 1
    fails, warns, infos = audit(root, point)
    print(f"基线审计：基准 {ref}（merge-base {point[:12]}），{len(BASELINES)} 个文件")
    for line in infos:
        print(f"INFO  {line}")
    for line in warns:
        print(f"WARN  {line}")
    for line in fails:
        print(f"FAIL  {line}", file=sys.stderr)
    if fails:
        return 1
    print(f"PASS  基线只降不升（无回涨；{len(warns)} 条告警）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
