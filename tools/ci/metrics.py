#!/usr/bin/env python3
# 质量度量门（B11，规约门第 ⑫ 步）：重复率 / 圈复杂度 / 生产函数名长度。
#
# 判据与口径（真值单点在本文件，check.sh 只调它，不重复实现）：
# - 重复率：**硬门槛 3%**。相邻 W=10 条"有效行"滑窗取哈希，出现 ≥2 次的窗口
#   所覆盖行计为重复行；有效行 = 去注释、字符串/字符字面量内容折占位符、去纯
#   标点行后的非空行。生成物（文件头 @generated/自动生成）与测试面（tests/ 目录、
#   首个 #[cfg(...test...)] 之后的尾部）不参与——它们是夹具/生成数据，不是可
#   复用缺陷。
# - 圈复杂度：均值与单函数最大值按 `tools/ci/metrics-baseline.json` **钉基线、
#   只降不升**（与仓内红账基线门同款纪律）。目标均值 2.5 尚未达到，红账在账；
#   基线只能随重构下调，上调须在评审里说明理由。
# - 生产函数名：>20 字节计红；FFI 外部符号（AudioQueue*，名字由平台 ABI 决定）
#   豁免。基线 = 存量 14 个 rr3_reg_* 域 API（21–25B），只降不升——新代码必须
#   ≤20B。
#
# 只用 Python 标准库，无外网依赖；python3 缺位时按 SKIP 纪律明说（CI runner 自带）。

import hashlib
import json
import os
import re
import subprocess
import sys

WINDOW = 10
NAME_MAX = 20
FFI_PREFIXES = ("AudioQueue",)


def repo_root():
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True, text=True, check=False,
    )
    if out.returncode != 0:
        print("FAIL  ⑫ 度量门在 git 仓库外运行", file=sys.stderr)
        sys.exit(1)
    return out.stdout.strip()


def rust_files(root):
    out = subprocess.run(
        ["git", "ls-files", "*.rs"],
        cwd=root, capture_output=True, text=True, check=True,
    )
    return [f for f in out.stdout.splitlines() if f]


def strip_code(src):
    """去注释；字符串/字符字面量内容折占位符，保留换行与代码行结构。"""
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i + 2)
            end = n if j < 0 else j + 2
            out.append("\n" * src.count("\n", i, end))
            i = end
        elif c == '"':
            out.append('""')
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                elif src[j] == '"':
                    j += 1
                    break
                else:
                    if src[j] == "\n":
                        out.append("\n")
                    j += 1
            i = j
        else:
            out.append(c)
            i += 1
    return "".join(out)


def production_part(src):
    """截掉测试尾巴：首个 #[cfg(...test...)] 之后不进度量。"""
    m = re.search(r"#\[cfg\([^\]]*test", src)
    return src[: m.start()] if m else src


def functions(src):
    """(名字, 函数体) 列表；花括号配对取体，声明无体（trait 方法）跳过。"""
    out = []
    for m in re.finditer(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", src):
        body_at = src.find("{", m.end())
        semi_at = src.find(";", m.end())
        if body_at < 0 or (0 <= semi_at < body_at):
            continue
        depth, k = 0, body_at
        while k < len(src):
            if src[k] == "{":
                depth += 1
            elif src[k] == "}":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        out.append((m.group(1), src[body_at + 1 : k]))
    return out


def cyclomatic(body):
    """McCabe 近似：1 + 判定点（if/for/while/loop/匹配臂/布尔短路/?）。"""
    points = 0
    for pat in (r"\bif\b", r"\bfor\b", r"\bwhile\b", r"\bloop\b", r"=>",
                r"&&", r"\|\|", r"\?"):
        points += len(re.findall(pat, body))
    return 1 + points


def significant(lines):
    """有效行：去空白归一，剔掉纯标点行（}、}); 等不构成克隆证据）。"""
    keep = []
    for raw in lines:
        line = re.sub(r"\s+", " ", raw).strip()
        if not line:
            continue
        core = re.sub(r"[\s{}()\[\];,]+", "", line)
        if len(core) < 4:
            continue
        keep.append(line)
    return keep


def measure(root):
    dup_occ = {}
    total_lines = 0
    ccs = []
    long_names = []
    for rel in rust_files(root):
        if "/tests/" in rel or rel.startswith("tests/"):
            continue
        path = os.path.join(root, rel)
        try:
            raw = open(path, encoding="utf-8", errors="ignore").read()
        except OSError as e:
            print(f"FAIL  ⑫ 读取失败 {rel}: {e}", file=sys.stderr)
            sys.exit(1)
        head = "\n".join(raw.splitlines()[:5])
        if "@generated" in head or "自动生成" in raw[:400]:
            continue
        src = strip_code(production_part(raw))
        for name, body in functions(src):
            ccs.append(cyclomatic(body))
            if len(name.encode()) > NAME_MAX and not name.startswith(FFI_PREFIXES):
                long_names.append(name)
        lines = significant(src.splitlines())
        total_lines += len(lines)
        for i in range(len(lines) - WINDOW + 1):
            h = hashlib.md5("\n".join(lines[i : i + WINDOW]).encode()).hexdigest()
            dup_occ.setdefault(h, []).append((rel, i))
    dup = set()
    for occ in dup_occ.values():
        if len(occ) > 1:
            for rel, i in occ:
                for k in range(WINDOW):
                    dup.add((rel, i + k))
    return total_lines, len(dup), ccs, long_names


def load_baseline(root):
    path = os.path.join(root, "tools", "ci", "metrics-baseline.json")
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def main():
    root = repo_root()
    base = load_baseline(root)
    total, dup_lines, ccs, long_names = measure(root)
    if total == 0 or not ccs:
        print("FAIL  ⑫ 度量面为空（git ls-files 无生产 Rust 行）", file=sys.stderr)
        return 1
    dup_pct = dup_lines / total * 100
    mean_cc = round(sum(ccs) / len(ccs), 2)
    max_cc = max(ccs)
    ok = True

    if dup_pct <= base["dup_max_pct"]:
        print(f"PASS  ⑫ 重复率 {dup_pct:.2f}% ≤ 门槛 {base['dup_max_pct']:.2f}%"
              f"（有效行 {total}，重复 {dup_lines}，W={WINDOW}）")
    else:
        print(f"FAIL  ⑫ 重复率 {dup_pct:.2f}% > 门槛 {base['dup_max_pct']:.2f}%"
              f"（有效行 {total}，重复 {dup_lines}；先合公共件，别复制粘贴）",
              file=sys.stderr)
        ok = False

    if mean_cc <= base["cc_mean_max"] + 1e-9 and max_cc <= base["cc_max_fn"]:
        print(f"PASS  ⑫ 圈复杂度 均值 {mean_cc} ≤ 基线 {base['cc_mean_max']:.2f}，"
              f"最大 {max_cc} ≤ 基线 {base['cc_max_fn']}（目标均值 2.5，红账在账）")
    else:
        print(f"FAIL  ⑫ 圈复杂度回归：均值 {mean_cc}（基线 {base['cc_mean_max']:.2f}）、"
              f"最大 {max_cc}（基线 {base['cc_max_fn']}）——拆函数或下调基线须评审",
              file=sys.stderr)
        ok = False

    if len(long_names) <= base["long_names_max"]:
        print(f"PASS  ⑫ 超长生产函数名 {len(long_names)} ≤ 基线 {base['long_names_max']}"
              f"（>{NAME_MAX}B，FFI AudioQueue* 豁免；目标 0）")
    else:
        print(f"FAIL  ⑫ 超长生产函数名 {len(long_names)} > 基线 {base['long_names_max']}："
              f"{sorted(long_names)[:8]}", file=sys.stderr)
        ok = False

    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
