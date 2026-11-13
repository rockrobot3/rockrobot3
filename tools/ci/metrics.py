#!/usr/bin/env python3
# 质量度量门（B11，规约门第 ⑫ 步）：重复率 / 圈复杂度 / 生产 panic 落点 / 函数名长度。
#
# 判据与口径（真值单点在本文件，check.sh 只调它，不重复实现）：
# - 重复率：**硬门槛 3%**。相邻 W=10 条"有效行"滑窗取哈希，出现 ≥2 次的窗口
#   所覆盖行计为重复行；有效行 = 去注释、字符串/字符字面量内容折占位符、去纯
#   标点行后的非空行。生成物（文件头 @generated/自动生成）与测试面（tests/ 目录、
#   `tests.rs` 测试模块文件、cfg(test) 门 mod 块之后的尾部）不参与——它们是夹具/
#   生成数据，不是可复用缺陷。测试面判定：cfg 谓词**测试专用**（`test` 裸项/
#   `all(…test…)`），`any(…)`/`not(…)` 门是生产件不剔除（backend.rs 613 行教训）。
# - 圈复杂度：均值与单函数最大值按 `tools/ci/metrics-baseline.json` **钉基线、
#   只降不升**（与仓内红账基线门同款纪律）。目标均值 2.5 尚未达到，红账在账；
#   基线只能随重构下调，上调须在评审里说明理由。
# - 生产 panic 落点：生产面 `.unwrap()` / `.expect(` 计数（测试面按上面的同一口径
#   截除），基线 71、**只钉不回涨**。它不是"越少越好"的绝对值判据——启动期
#   fail-fast 与外键自证式 expect 是合理形态；这条钉的是"新增 panic 必须被看见"。
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
# cfg(test) 门 mod 的形状：属性本体 +（可连挂的其他属性）+ mod 声明。
TEST_ATTR = re.compile(r"#\[cfg\(")
TEST_MOD_TAIL = re.compile(
    r"\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\b"
)
TEST_ITEM_TAIL = re.compile(
    r"\s*(?:#\[[^\]]*\]\s*)*"
    r"(?:(?:pub|pub\(crate\)|pub\(super\))\s+)?"
    r"(mod|fn|const|static|use|type)\b"
)


def _pred_test_only(src, open_paren):
    """cfg 谓词是否**测试专用**：`test` 裸项或 `all(…test…)` 算；
    `any(…)`/`not(…)` 不算——`#[cfg(any(target_os = "linux", test))]` 是
    linux 生产件（树莓派档必需），误判会把整段生产代码藏出度量
    （实测 rr-hal/backend.rs 613 行）。"""
    depth, k = 1, open_paren + 1
    while k < len(src) and depth:
        if src[k] == "(":
            depth += 1
        elif src[k] == ")":
            depth -= 1
        k += 1
    pred = src[open_paren + 1 : k - 1]
    head = pred.lstrip()
    if head.startswith("any(") or head.startswith("not("):
        return False
    return re.search(r"\btest\b", pred) is not None


def _item_end(src, head_at):
    """项体终点（含）：`{…}` 平衡配对，或语句形到 `;`；找不到按体处理。"""
    body_at = src.find("{", head_at)
    semi_at = src.find(";", head_at)
    if body_at < 0 or (0 <= semi_at < body_at):
        return semi_at + 1
    depth, k = 0, body_at
    while k < len(src):
        if src[k] == "{":
            depth += 1
        elif src[k] == "}":
            depth -= 1
            if depth == 0:
                return k + 1
        k += 1
    return len(src)


def production_part(src):
    """截掉测试面：**测试专用 cfg 门**（`test` 裸项/`all(…test…)`）的 mod 块
    整体截断、单项（fn/const/use/…）就地剔除，其后即生产面。

    只认"cfg(test) 属性（可连挂其他属性）+ 项"形状——mod 之后整段是测试
    尾巴；逐项门是散点测试辅助，只摘项不伤其后生产（`fn main` 必须留在
    度量内，实测 rr3-audio-rec main CC 46 曾被整段藏出）。入参须是
    strip_code 之后的文本：注释/字符串里提到 `#[cfg(test)]` 不构成依据。"""
    pos = 0
    while True:
        m = TEST_ATTR.search(src, pos)
        if not m:
            return src
        if not _pred_test_only(src, m.end() - 1):
            pos = m.end()
            continue
        im = TEST_ITEM_TAIL.match(src[m.start() :])
        if not im:
            pos = m.end()
            continue
        if im.group(1) == "mod":
            return src[: m.start()]
        end = _item_end(src, m.start() + im.end(1))
        src = src[: m.start()] + src[end:]
        pos = m.start()


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


def _raw_string_at(src, i):
    """`i` 处是否生字符串前缀（r"…" / r#"…"#，b/br 前缀同理）。
    返回 (引号下标, 井号数)；否则 None。识别失败按普通字符处理。"""
    j = i
    if j < len(src) and src[j] == "b":
        j += 1
    if j >= len(src) or src[j] != "r":
        return None
    j += 1
    hashes = 0
    while j < len(src) and src[j] == "#":
        hashes += 1
        j += 1
    if j >= len(src) or src[j] != '"':
        return None
    return j, hashes


def strip_code(src):
    """去注释；字符串/字符字面量/生字符串内容折占位符，保留换行与代码行结构。

    字符字面量与生命周期用"引号后形制"消歧：`'x'`/`'\\''` 是字面量（折
    `''`），`'a`/`'static`/`'label:` 是生命周期（单引号直通）——不消歧则
    `.strip_prefix('"')` 这类内嵌双引号的字面量会让字符串态翻转级联，整段
    生产代码被当字符串内容吞掉（实测 evidence.rs 测试 mod 因此漏截断）。
    生字符串终止符是 `"` + 等量 `#`，不认转义。"""
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
        elif c == '"' or (rs := _raw_string_at(src, i)):
            if c == '"':
                out.append('""')
                j = i + 1
                term = None
            else:
                q, hashes = rs
                out.append('""')
                j = q + 1
                term = '"' + "#" * hashes
            while j < n:
                if term is None and src[j] == "\\":
                    j += 2
                elif term is not None and src.startswith(term, j):
                    j += len(term)
                    break
                elif term is None and src[j] == '"':
                    j += 1
                    break
                else:
                    if src[j] == "\n":
                        out.append("\n")
                    j += 1
            i = j
        elif c == "'" and (i + 2 < n and src[i + 2] == "'" or i + 1 < n and src[i + 1] == "\\"):
            # 字符/字节字面量：'x' 或 '\…'；折 '' 占位
            out.append("''")
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                elif src[j] == "'":
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
    unwrap_sites = 0
    for rel in rust_files(root):
        if "/tests/" in rel or rel.startswith("tests/") or os.path.basename(rel) == "tests.rs":
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
        src = production_part(strip_code(raw))
        # 生产面 panic 落点（unwrap/expect）：测试面已被 production_part 截掉。
        # 此项不判"越少越好"的绝对值，只钉"不回涨"——新增落点必须同批给理由。
        unwrap_sites += len(re.findall(r"\.unwrap\(\)|\.expect\(", src))
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
    return total_lines, len(dup), ccs, long_names, unwrap_sites


def load_baseline(root):
    path = os.path.join(root, "tools", "ci", "metrics-baseline.json")
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def main():
    root = repo_root()
    base = load_baseline(root)
    total, dup_lines, ccs, long_names, unwrap_sites = measure(root)
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

    if unwrap_sites <= base["unwrap_max"]:
        print(f"PASS  ⑫ 生产面 panic 落点 {unwrap_sites} ≤ 基线 {base['unwrap_max']}"
              f"（unwrap/expect；测试面已截除，只钉不回涨）")
    else:
        print(f"FAIL  ⑫ 生产面 panic 落点回涨：{unwrap_sites} > 基线 {base['unwrap_max']}"
              f"——新增落点须同批给理由或改成 Result", file=sys.stderr)
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
