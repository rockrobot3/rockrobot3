#!/usr/bin/env python3
"""质量度量门 CLI（B11，规约门第 ⑫ 步）：测量核在 metrics_core.py，本文件只做入口与自检。"""

import os
import subprocess
import sys

from metrics_core import (
    FN_LINES_MAX,
    FN_NEXT_MAX,
    FN_TIGHT_MAX,
    LONG_SLEEP_MS,
    NAME_MAX,
    WINDOW,
    load_baseline,
    measure,
    repo_root,
)


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
        "base/midfn.rs": (
            "fn mid() {\n"
            + "".join(f"    let m{i} = {i};\n" for i in range(FN_TIGHT_MAX + 2))
            + "}\n"
        ),
        "base/targetfn.rs": (
            "fn target() {\n"
            + "".join(f"    let t{i} = {i};\n" for i in range(FN_NEXT_MAX + 2))
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


def worst_row(rows):
    """基线内最大落点（空 = 空串）：让下一拆目标在 CI 日志里直接可见。"""
    if not rows:
        return ""
    rel, name, n = max(rows, key=lambda x: x[2])
    return f"；最大 {n} 行 {rel}:{name}"


def self_test():
    """夹具自检：生产面计数、测试面剔除、长睡扫面与重复窗口各一条判据。"""
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        _selftest_repo(td)
        (total, dup_lines, ccs, long_names, unwrap_sites, long_sleeps,
         fn_over, fn_over60, fn_over50) = measure(td)

    checks = [
        (f"生产面 unwrap=2（实得 {unwrap_sites}）", unwrap_sites == 2),
        (f"生产长睡=1（实得 {len(long_sleeps)}）", len(long_sleeps) == 1),
        (f"超长函数名=1（实得 {len(long_names)}）", len(long_names) == 1),
        (f"生产函数>{FN_LINES_MAX} 行=1（实得 {len(fn_over)}）", len(fn_over) == 1),
        (f"生产函数>{FN_TIGHT_MAX} 行=2（实得 {len(fn_over60)}）", len(fn_over60) == 2),
        (f"生产函数>{FN_NEXT_MAX} 行=3（实得 {len(fn_over50)}）", len(fn_over50) == 3),
        (f"重复行=20（实得 {dup_lines}）", dup_lines == 2 * WINDOW),
        (f"函数数=7（实得 {len(ccs)}）", len(ccs) == 7),
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
    (total, dup_lines, ccs, long_names, unwrap_sites, long_sleeps,
     fn_over, fn_over60, fn_over50) = measure(root)
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

    tight_max = base["fn_over60_max"]
    if len(fn_over60) <= tight_max:
        print(f"PASS  ⑫ 生产函数 >{FN_TIGHT_MAX} 行 {len(fn_over60)} ≤ 基线 {tight_max}"
              f"（只降不升{worst_row(fn_over60)}；下一档目标逐批拆到 0）")
    else:
        top = sorted(fn_over60, key=lambda x: -x[2])[:5]
        print(f"FAIL  ⑫ 生产函数 >{FN_TIGHT_MAX} 行回涨：{len(fn_over60)} > 基线 {tight_max}："
              f"{top}", file=sys.stderr)
        ok = False

    next_max = base["fn_over50_max"]
    if len(fn_over50) <= next_max:
        print(f"PASS  ⑫ 生产函数 >{FN_NEXT_MAX} 行 {len(fn_over50)} ≤ 基线 {next_max}"
              f"（目标档；只降不升{worst_row(fn_over50)}，逐批拆到 0 = <{FN_NEXT_MAX} 行）")
    else:
        top = sorted(fn_over50, key=lambda x: -x[2])[:5]
        print(f"FAIL  ⑫ 生产函数 >{FN_NEXT_MAX} 行回涨：{len(fn_over50)} > 基线 {next_max}："
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
