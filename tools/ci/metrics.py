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
#   只降不升**（与仓内红账基线门同款纪律）。目标均值 2.5，当前 2.49 已达标；
#   基线只能随重构下调，上调须在评审里说明理由。
# - 生产函数体量：单函数 >80 行计红（测试面不计——表驱动用例允许更长）；
#   基线 = 存量 5 件，只降不升——先 80，再 60，最终 50。
# - 生产 panic 落点：生产面 `.unwrap()` / `.expect(` 计数（测试面按上面的同一口径
#   截除；独立成文件的 `test_support.rs` / `test_fixtures.rs` 由 `#[cfg(test)]`
#   挂载，同样不计），基线 60、**只钉不回涨**。它不是"越少越好"的绝对值判据——启动期
#   fail-fast 与外键自证式 expect 是合理形态；这条钉的是"新增 panic 必须被看见"。
# - 生产函数名：>20 字节计红；FFI 外部符号（AudioQueue*，名字由平台 ABI 决定）
#   豁免。基线 = 存量 14 个 rr3_reg_* 域 API（21–25B），只降不升——新代码必须
#   ≤20B。
# - 长睡字面量（长阻塞主循环的静态半边）：只扫**模块与 rrd 守护**的生产路径
#   （`modules/rr3-*` 与 `base/runtime/rrd/src`，排除 bin），计
#   `sleep(Duration::from_millis|from_secs(数字))` 且 ≥1000ms 的落点，基线 0。
#   局限如实写：动态等待（`sleep(tick)`）、阻塞 IO、锁内长临界区不在本项扫面，
#   它们归评审与运行时观测；本项只钉"有人把秒级 sleep 写进主循环"这一种。
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
# 生产函数体量门槛：>80 行计红；测试面不参与（表驱动用例允许更长）。
FN_LINES_MAX = 80
FFI_PREFIXES = ("AudioQueue",)
# 独立成文件的测试夹具：由 `#[cfg(test)]` / `#[cfg(all(test, …))]` 挂载，
# `git ls-files` 会单独列出；不按文件名排除，panic/CC/重复率口径就会被测试面污染。
TEST_ONLY_FILES = ("test_support.rs", "test_fixtures.rs")
# 长睡门槛：≥1000ms 的字面量 sleep 在模块/守护生产路径上计红。
LONG_SLEEP_MS = 1000
SLEEP_LITERAL = re.compile(
    r"sleep\s*\(\s*(?:[A-Za-z_][A-Za-z0-9_]*::)*"
    r"Duration::from_(millis|secs)\s*\(\s*([0-9_]+)\s*\)"
)
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
    整体摘除、单项（fn/const/use/…）就地剔除，其余文本原样保留。

    只认"cfg(test) 属性（可连挂其他属性）+ 项"形状；逐项门是散点测试辅助，
    只摘项不伤其后生产（`fn main` 必须留在度量内，实测 rr3-audio-rec main
    CC 46 曾被整段藏出）。测试 mod 亦只摘块、不截尾——生产项可以排在测试
    模块之后（httpd.rs 实证），位置不是判据。入参须是 strip_code 之后的
    文本：注释/字符串里提到 `#[cfg(test)]` 不构成依据。"""
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
            # 测试专用 mod：只摘掉这个 mod 块，不截断其后文本——此前按"mod 之后
            # 整段是测试尾巴"直接截尾，实测把 httpd.rs 测试模块之后的生产段
            # （线程起子与白名单 CString 自证）整段藏出度量，拆分搬文件后才暴露。
            # 语法上生产项完全可以排在测试模块之后，度量不得以位置代替判据。
            end = _item_end(src, m.start() + im.end(1))
            src = src[: m.start()] + src[end:]
            pos = m.start()
            continue
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
    long_sleeps = []
    fn_over = []
    for rel in rust_files(root):
        base_name = os.path.basename(rel)
        if ("/tests/" in rel or rel.startswith("tests/")
                or base_name == "tests.rs" or base_name in TEST_ONLY_FILES):
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
        long_sleeps.extend(_long_sleeps(rel, src))
        for name, body in functions(src):
            ccs.append(cyclomatic(body))
            n = len(body.splitlines())
            if n > FN_LINES_MAX:
                fn_over.append((rel, name, n))
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
    return total_lines, len(dup), ccs, long_names, unwrap_sites, long_sleeps, fn_over


def _sleep_scope(rel):
    """长睡扫面：模块生产码与 rrd 守护主循环；bin 夹具/测试面不在内。"""
    if rel.startswith("modules/rr3-"):
        return True
    return rel.startswith("base/runtime/rrd/src/") and "/bin/" not in rel


def _long_sleeps(rel, src):
    """该文件生产面的长睡落点（≥[`LONG_SLEEP_MS`] 的字面量）。"""
    if not _sleep_scope(rel):
        return []
    out = []
    for m in SLEEP_LITERAL.finditer(src):
        unit, raw = m.group(1), int(m.group(2).replace("_", ""))
        ms = raw if unit == "millis" else raw * 1000
        if ms >= LONG_SLEEP_MS:
            out.append((rel, ms))
    return out


def load_baseline(root):
    path = os.path.join(root, "tools", "ci", "metrics-baseline.json")
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def _selftest_repo(td):
    """自检夹具：生产/测试/夹具/生成物四类文件各就位，并纳入临时 git 索引。"""
    files = {
        "base/prod.rs": (
            "fn short() { let _ = 1; }\n"
            "fn rr3_this_name_is_way_too_long_for_limit() {}\n"
            "fn long_one() {\n"
            "    let x = foo().unwrap();\n"
            "    let _ = x;\n"
            "}\n"
        ),
        "base/tests/t.rs": "fn t() { let x = foo().unwrap(); }\n",
        "base/src/tests.rs": "fn t() { let x = foo().unwrap(); }\n",
        "base/src/test_support.rs": "fn s() { let x = foo().unwrap(); }\n",
        "base/src/test_fixtures.rs": "fn f() { let x = foo().unwrap(); }\n",
        "base/src/gen.rs": "// @generated\nfn g() { let x = foo().unwrap(); }\n",
        "modules/rr3-demo/src/lib.rs": (
            "fn wait() {\n"
            "    let _ = foo().unwrap();\n"
            "    std::thread::sleep(std::time::Duration::from_millis(1500));\n"
            "}\n"
        ),
        "modules/rr3-demo/tests/t.rs": (
            "fn t() {\n"
            "    std::thread::sleep(std::time::Duration::from_millis(1500));\n"
            "}\n"
        ),
        "base/longfn.rs": (
            "fn too_long() {\n"
            + "".join(f"    let v{i} = {i};\n" for i in range(FN_LINES_MAX + 2))
            + "}\n"
        ),
    }
    dup = "".join(f"const D{i}: u8 = {i};\n" for i in range(WINDOW))
    files["base/src/dup_a.rs"] = dup
    files["base/src/dup_b.rs"] = dup
    for rel, text in files.items():
        path = os.path.join(td, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
    subprocess.run(["git", "init", "-q"], cwd=td, check=True)
    subprocess.run(["git", "add", "-A"], cwd=td, check=True)


def self_test():
    """夹具自检：生产面计数、测试面剔除、长睡扫面与重复窗口各一条判据。"""
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        _selftest_repo(td)
        total, dup_lines, ccs, long_names, unwrap_sites, long_sleeps, fn_over = measure(td)

    checks = [
        (f"生产面 unwrap=2（实得 {unwrap_sites}）", unwrap_sites == 2),
        (f"生产长睡=1（实得 {len(long_sleeps)}）", len(long_sleeps) == 1),
        (f"超长函数名=1（实得 {len(long_names)}）", len(long_names) == 1),
        (f"生产函数>{FN_LINES_MAX} 行=1（实得 {len(fn_over)}）", len(fn_over) == 1),
        (f"重复行=20（实得 {dup_lines}）", dup_lines == 2 * WINDOW),
        (f"函数数=5（实得 {len(ccs)}）", len(ccs) == 5),
        (f"有效行>0（实得 {total}）", total > 0),
    ]
    for name, ok in checks:
        print(("PASS  " if ok else "FAIL  ") + "⑫ 自检：" + name)
    return 0 if all(ok for _, ok in checks) else 1


def main():
    if "--self-test" in sys.argv[1:]:
        return self_test()
    root = repo_root()
    base = load_baseline(root)
    total, dup_lines, ccs, long_names, unwrap_sites, long_sleeps, fn_over = measure(root)
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
              f"最大 {max_cc} ≤ 基线 {base['cc_max_fn']}（目标均值 2.5，已达标）")
    else:
        print(f"FAIL  ⑫ 圈复杂度回归：均值 {mean_cc}（基线 {base['cc_mean_max']:.2f}）、"
              f"最大 {max_cc}（基线 {base['cc_max_fn']}）——拆函数或下调基线须评审",
              file=sys.stderr)
        ok = False

    if len(fn_over) <= base["fn_over_max"]:
        print(f"PASS  ⑫ 生产函数 >{FN_LINES_MAX} 行 {len(fn_over)} ≤ 基线 {base['fn_over_max']}"
              f"（测试面不计；目标逐档降到 0）")
    else:
        top = sorted(fn_over, key=lambda x: -x[2])[:5]
        print(f"FAIL  ⑫ 生产函数 >{FN_LINES_MAX} 行回涨：{len(fn_over)} > 基线 {base['fn_over_max']}："
              f"{top}", file=sys.stderr)
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

    sleep_max = base.get("long_sleep_max", 0)
    if len(long_sleeps) <= sleep_max:
        print(f"PASS  ⑫ 生产长睡字面量 {len(long_sleeps)} ≤ 基线 {sleep_max}"
              f"（≥{LONG_SLEEP_MS}ms，模块/rrd 主循环面；动态等待不在扫面）")
    else:
        print(f"FAIL  ⑫ 生产长睡字面量 {len(long_sleeps)} > 基线 {sleep_max}："
              f"{long_sleeps[:5]}——秒级 sleep 不得进主循环，改事件等待或拆线程",
              file=sys.stderr)
        ok = False

    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
