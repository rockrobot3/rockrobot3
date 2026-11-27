#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""发布/部署面模块清单一致性对拍（批250，2026-11-28）。

为什么需要它：模块清单现在有四处"同一事实的不同写法"——
  · 仓内真值：modules/rr3-*/module.toml（tools/ci/lib/runtime-members.sh 派生）；
  · 发布策略：workflow_release.PUBLISH_MODULES 与 publish.yml 的构建循环；
  · 镜像面：stage-image.sh 的两处装机循环、image-smoke.sh 的起齐断言；
  · 板侧包：pack-bundle.sh / pack-arm64.sh 按真值派生、brain 例外。
任一处偷跑，CI 仍绿而某个载体静默缺件（缺失只在真板装上才暴露）。本脚本只做
一件事：把这些集合逐项对拍，缺/多都点名；并核对真值面自身成对——模块 crate
（Cargo.toml）漏写 module.toml 时它对所有派生的载体都不可见。**判据是集合相等**，不是"包含"——
新增模块不入选发布集，必须显式改策略，不许默默漏。

用法：
  release_sets.py [--root DIR]       对拍，一致退 0，不一致逐项点名退 1
  release_sets.py --self-test        正/负例夹具自检（不读仓、不编译）
"""

import argparse
import pathlib
import re
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from workflow_release import PUBLISH_MODULES  # noqa: E402

BRAIN = "brain"
LOOP_RE = re.compile(r"for m in ([\w ]+); do")
COUNT_RE = re.compile(r'\[\s*"\$n"\s*-eq\s+(\d+)\s*\]')


def truth_modules(root):
    """仓内真值：modules/rr3-*/module.toml 的模块名（排序）。"""
    mods = sorted(p.parent.name[len("rr3-"):] for p in root.glob("modules/rr3-*/module.toml"))
    return mods


def loop_sets(text):
    """字面量模块循环：(行号, 集合)；`for m in $mods` 这类派生写法不在此列。"""
    out = []
    for i, line in enumerate(text.splitlines(), 1):
        m = LOOP_RE.search(line)
        if m:
            out.append((i, set(m.group(1).split())))
    return out


def diff(want, got):
    """集合差的双向点名（缺什么、多什么）。"""
    bad = []
    miss = sorted(set(want) - set(got))
    extra = sorted(set(got) - set(want))
    if miss:
        bad.append("缺 " + "/".join(miss))
    if extra:
        bad.append("多 " + "/".join(extra))
    return bad


def check_pairing(root, truth):
    """模块目录成对：Cargo.toml 与 module.toml 必须同在（真值面自身的盲区）。

    新建模块 crate 却漏写 module.toml → 真值派生的每个载体都看不见它，流水线全绿，
    装上板才发现少一个模块；反向（只有 module.toml）则是清单指向不存在的 crate。
    """
    crates = sorted(p.parent.name[len("rr3-"):] for p in root.glob("modules/rr3-*/Cargo.toml"))
    bad = []
    for m in sorted(set(crates) - set(truth)):
        bad.append(f"modules/rr3-{m} 有 Cargo.toml 但缺 module.toml（打包面不可见，载体静默缺件）")
    for m in sorted(set(truth) - set(crates)):
        bad.append(f"modules/rr3-{m} 有 module.toml 但缺 Cargo.toml（清单指向不存在的模块）")
    return bad


def check_policy(policy, truth):
    """发布策略 = 真值：新增模块必须显式决定是否进发布面。"""
    bad = diff(truth, policy)
    if not bad:
        return []
    return ["发布策略 PUBLISH_MODULES 与仓内真值不一致：" + "；".join(bad)
            + f"（真值 {len(truth)} 件 / 策略 {len(policy)} 件）"]


def check_publish_yml(text, policy):
    """publish.yml 构建循环必须与策略逐名相同。"""
    loops = loop_sets(text)
    if not loops:
        return ["publish.yml 缺字面量构建循环（for m in ...; do），发布面清单不可核"]
    bad = []
    for line, got in loops:
        d = diff(policy, got)
        if d:
            bad.append(f"publish.yml:{line} 构建循环与发布策略不一致：" + "；".join(d))
    return bad


def check_stage_image(text, policy):
    """stage-image.sh 的两处装机循环（产物校验 + 载荷拷贝）都要等于策略。"""
    loops = loop_sets(text)
    if len(loops) < 2:
        return [f"stage-image.sh 只剩 {len(loops)} 处装机循环（产物校验与载荷拷贝应各一处）"]
    bad = []
    for line, got in loops:
        d = diff(policy, got)
        if d:
            bad.append(f"stage-image.sh:{line} 装机循环与发布策略不一致：" + "；".join(d))
    return bad


def check_image_smoke(text, policy):
    """image-smoke.sh：起齐循环等于策略，且计数据此对齐（数错 = 假绿/假红）。"""
    loops = loop_sets(text)
    if not loops:
        return ["image-smoke.sh 缺六模块起齐循环（运行冒烟失去判据）"]
    bad = []
    for line, got in loops:
        d = diff(policy, got)
        if d:
            bad.append(f"image-smoke.sh:{line} 起齐循环与发布策略不一致：" + "；".join(d))
    nums = [int(x) for x in COUNT_RE.findall(text)]
    if not nums:
        bad.append('image-smoke.sh 缺起齐阈值断言（形如 [ "$n" -eq N ]），循环无判据')
    elif len(loops[0][1]) not in nums:
        bad.append(f"image-smoke.sh 起齐断言未用 {len(loops[0][1])}（实见阈值 "
                   f"{sorted(set(nums))}），循环与阈值不同源")
    return bad


def check_derived(text, rel, truth):
    """板侧包按真值派生：必须 source 单点、必须 brain 例外、不得出现字面量清单。"""
    bad = []
    if "runtime-members.sh" not in text:
        bad.append(f"{rel} 未 source tools/ci/lib/runtime-members.sh（模块清单回落字面量）")
    if not re.search(r'\$\{?m\}?"?\s*=\s*brain', text):
        bad.append(f"{rel} 缺 brain 例外（musl/交叉腿不含 brain，派生集将失真）")
    for line, got in loop_sets(text):
        bad.append(f"{rel}:{line} 出现字面量模块循环 {sorted(got)}（应派生，勿再抄一份）")
    expect = set(truth) - {BRAIN}
    if not expect:
        bad.append(f"{rel} 派生集为空（真值只有 brain，包内无模块）")
    return bad


def _read(root, *parts):
    p = root.joinpath(*parts)
    if not p.is_file():
        return None
    return p.read_text(encoding="utf-8")


def check(root):
    """全部对拍项；返回问题清单（空 = 一致）。"""
    truth = truth_modules(root)
    if not truth:
        return ["仓内没有任何 modules/rr3-*/module.toml（真值缺席，无法对拍）"]
    policy = list(PUBLISH_MODULES)
    bad = check_policy(policy, truth)
    bad.extend(check_pairing(root, truth))
    specs = (
        (".github/workflows/publish.yml", lambda t: check_publish_yml(t, policy)),
        ("tools/deploy/stage-image.sh", lambda t: check_stage_image(t, policy)),
        ("tools/deploy/image-smoke.sh", lambda t: check_image_smoke(t, policy)),
        ("tools/deploy/pack-bundle.sh", lambda t: check_derived(t, "pack-bundle.sh", truth)),
        ("tools/deploy/pack-arm64.sh", lambda t: check_derived(t, "pack-arm64.sh", truth)),
    )
    for rel, fn in specs:
        text = _read(root, *rel.split("/"))
        if text is None:
            bad.append(f"{rel} 不存在（发布/部署面被删）")
            continue
        bad.extend(fn(text))
    return bad


def summary(root):
    truth = truth_modules(root)
    return (f"PASS  发布/部署面清单一致（真值 {len(truth)} 件 · 策略 {len(PUBLISH_MODULES)} 件 · "
            f"镜像同集 · 派生 {len(truth) - 1} 件 = 真值−{BRAIN}）")


FIX_FILES = {
    ".github/workflows/publish.yml": """jobs:
  build:
    steps:
      - run: |
          for m in @MODS@; do
            echo "$m"
          done
""",
    "tools/deploy/stage-image.sh": """for m in @MODS@; do
  test -x "$rel/rr3-$m"
done
for m in @MODS@; do
  cp "$rel/rr3-$m" "$pay/"
done
""",
    "tools/deploy/image-smoke.sh": """for m in @MODS@; do
  grep -q "spawn rr3-$m pid=" /tmp/rrd.log && n=$((n+1))
done
[ "$n" -eq @N@ ] && ok=1
""",
    "tools/deploy/pack-bundle.sh": """. "$ROOT/tools/ci/lib/runtime-members.sh"
mods=$(rr3_modules "$ROOT") || die "真值缺席"
for m in $mods; do
  if [ "$m" = brain ]; then
    continue
  fi
done
""",
    "tools/deploy/pack-arm64.sh": """. tools/ci/lib/runtime-members.sh
mods=$(rr3_modules .) || { echo "真值缺席" >&2; exit 2; }
for m in $mods; do
  if [ "$m" = brain ]; then
    continue
  fi
done
""",
}


def _fixture(tmp, mods=("act", "brain", "connect", "interact", "sense", "power")):
    """按给定真值铺最小夹具（只含被核文件，不含真实源码）。"""
    root = pathlib.Path(tmp)
    for m in mods:
        d = root / "modules" / f"rr3-{m}"
        d.mkdir(parents=True, exist_ok=True)
        (d / "module.toml").write_text(f'[module]\nname = "rr3-{m}"\n', encoding="utf-8")
        (d / "Cargo.toml").write_text(f'[package]\nname = "rr3-{m}"\n', encoding="utf-8")
    for rel, body in FIX_FILES.items():
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(body.replace("@MODS@", " ".join(PUBLISH_MODULES)).replace("@N@", str(len(PUBLISH_MODULES))), encoding="utf-8")
    return root


def _expect(bad, needle, label):
    if any(needle in b for b in bad):
        print(f"PASS  自检：{label} 被点名")
        return True
    print(f"FAIL  自检：{label} 未点名（实见 {bad or '空'}）")
    return False


def selftest():
    """正例（夹具应零问题）+ 五条负例（各改一处，必须被点名）。"""
    ok = True
    with tempfile.TemporaryDirectory() as base:
        root = _fixture(pathlib.Path(base) / "ok")
        bad = check(root)
        if bad:
            print(f"FAIL  自检：正例不该有问题，实见 {bad}")
            ok = False
        else:
            print("PASS  自检：正例零问题")
        cases = (
            ("publish.yml", "power", "publish.yml", "发布循环漏件"),
            ("stage-image.sh", "sense", "stage-image.sh", "镜像装机漏件"),
            ("image-smoke.sh", "connect", "image-smoke.sh", "起齐循环漏件"),
            ("pack-arm64.sh", "brain", "pack-arm64.sh", "派生脚本失去 brain 例外"),
            ("pack-bundle.sh", "brain", "pack-bundle.sh", "派生脚本失去 brain 例外"),
        )
        for name, token, needle, label in cases:
            root_m = _mutate(root, pathlib.Path(base) / f"m-{name}", name, token)
            ok &= _expect(check(root_m), needle, label)
        root_g = _fixture(pathlib.Path(base) / "new-mod",
                          ("act", "brain", "connect", "interact", "sense", "power", "guard"))
        ok &= _expect(check(root_g), "发布策略", "新增模块未入发布策略")
        root_p = _copy(root, pathlib.Path(base) / "pair")
        (root_p / "modules" / "rr3-ghost").mkdir(parents=True, exist_ok=True)
        (root_p / "modules" / "rr3-ghost" / "Cargo.toml").write_text(
            '[package]\nname = "rr3-ghost"\n', encoding="utf-8")
        ok &= _expect(check(root_p), "缺 module.toml", "模块 crate 漏写 module.toml")
        root_c = _copy(root, pathlib.Path(base) / "masked")
        f = root_c / "tools/deploy/image-smoke.sh"
        f.write_text(f.read_text(encoding="utf-8").replace(
            '[ "$n" -eq 6 ] && ok=1',
            '[ "$n" -eq 9 ] && ok=1\n[ "$unrelated" -eq 6 ]'), encoding="utf-8")
        ok &= _expect(check(root_c), "起齐断言", "阈值漂移被无关 -eq 6 遮蔽")
    print("PASS  发布面清单自检通过（1 正例 / 8 负例）" if ok else "FAIL  发布面清单自检未通过")
    return 0 if ok else 1


def _copy(root, dst, mutate=None):
    """复制夹具到 dst；mutate=(文件名片段, 模块名) 时削掉该名（模拟清单漂移）。"""
    for rel in FIX_FILES:
        p = dst / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        text = (root / rel).read_text(encoding="utf-8")
        if mutate and mutate[0] in rel:
            text = re.sub(r"\b" + mutate[1] + r"\b", "", text)
        p.write_text(text, encoding="utf-8")
    for p in root.glob("modules/rr3-*/*.toml"):
        d = dst / p.parent.relative_to(root)
        d.mkdir(parents=True, exist_ok=True)
        (d / p.name).write_text(p.read_text(encoding="utf-8"), encoding="utf-8")
    return dst


def _mutate(root, dst, name, token):
    """复制夹具并削掉目标文件里的一个模块名（模拟漂移），返回新根。"""
    return _copy(root, dst, (name, token))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return selftest()
    root = pathlib.Path(a.root).resolve()
    bad = check(root)
    for b in bad:
        print("FAIL  " + b)
    if bad:
        print(f"FAIL  发布/部署面清单漂移 {len(bad)} 处（清单真值见 tools/ci/lib/runtime-members.sh）")
        return 1
    print(summary(root))
    return 0


if __name__ == "__main__":
    sys.exit(main())
