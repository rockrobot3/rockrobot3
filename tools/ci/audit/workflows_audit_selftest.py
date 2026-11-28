#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""流水线结构审计自检夹具：37 条断言（与规则核分离，保持单文件 <500 行）。"""

import pathlib

from workflow_rules import MAX_JOBS, audit


def _write_good(wf):
    """写入 good.yml 零误报夹具。"""
    (wf / "good.yml").write_text(
        "name: good\non:\n  push:\n    branches: [main]\n"
        "permissions:\n  contents: read\n"
        "concurrency:\n  group: g\njobs:\n"
        "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - uses: actions/checkout@v7\n"
        "      - uses: dtolnay/rust-toolchain@stable\n"
        '        with:\n          toolchain: "1.100.0"\n'
        "      - uses: Swatinem/rust-cache@v2\n"
        "        with:\n          workspaces: base/runtime\n",
        encoding="utf-8",
    )


def _write_bad(wf):
    """写入 bad.yml 常规违规夹具。"""
    (wf / "bad.yml").write_text(
        "name: bad\non:\n  push:\njobs:\n  b:\n    runs-on: ubuntu-latest\n"
        "    needs: [nope]\n"
        "    steps:\n      - uses: actions/checkout@v5\n"
        "      - uses: dtolnay/rust-toolchain@stable\n"
        "      - uses: Swatinem/rust-cache@v2\n"
        "      - run: echo hi\n        continue-on-error: true\n",
        encoding="utf-8",
    )


def _write_flows(wf):
    """写入 noperm/integration/publish 流程夹具。"""
    (wf / "noperm.yml").write_text(
        "name: noperm\non:\n  push:\n    branches: [main]\n"
        "concurrency:\n  group: n\njobs:\n"
        "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - uses: actions/checkout@v7\n",
        encoding="utf-8",
    )
    (wf / "integration.yml").write_text(
        "name: integ\non:\n  push:\n    branches: [main]\n  pull_request:\n"
        "concurrency:\n  group: i\njobs:\n"
        "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - uses: actions/checkout@v7\n"
        # 集成腿负例（批289）：distro-smoke 换成 echo，其余腿保留脚本——只有它该被点名。
        "  distro-smoke:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: echo no-op\n"
        "  install-smoke:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: bash tools/ci/platform/install-smoke.sh\n"
        "  ota-sign:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: bash tools/ci/platform/ota-sign-smoke.sh\n"
        "  rk3566-onboard:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: bash tools/ci/platform/rk3566-onboard-check.sh\n"
        "  buildroot-assembly:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: bash tools/ci/platform/buildroot-assembly-smoke.sh\n"
        "  sim-assemble:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: bash tools/ci/platform/sim-assemble.sh\n"
        "  sim-chain:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: bash tools/ci/platform/sim-chain.sh\n",
        encoding="utf-8",
    )
    (wf / "publish.yml").write_text(
        "name: pub\non:\n  push:\n    tags: [\"v*\"]\n"
        "concurrency:\n  group: p\n  cancel-in-progress: true\njobs:\n"
        "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - uses: actions/checkout@v7\n"
        "  build:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    strategy:\n      matrix:\n        include:\n"
        "          - arch: amd64\n            platform: linux/amd64\n"
        "    steps:\n      - uses: actions/checkout@v7\n"
        "      - run: |\n"
        "          cargo build -q --locked --release -p rrd\n"
        "          for m in act brain connect interact power; do\n"
        '            cargo build -q --locked --release --manifest-path "modules/rr3-$m/Cargo.toml"\n'
        "          done\n"
        "          cargo build -q --locked --release --manifest-path tools/rrctl/Cargo.toml\n"
        "          cargo build -q --locked --release --manifest-path base/ota/rr3-ota-ctl/Cargo.toml\n"
        "          cargo build -q --locked --release --manifest-path tools/rr3-pack/Cargo.toml\n"
        "  firmware:\n    if: startsWith(github.ref, 'refs/tags/v')\n"
        "    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: echo classic\n"
        # distro 腿故意只留出包一步（缺契约断言/双载体托管/sha256）：新判据要有靶子。
        "  distro-bundle:\n    if: startsWith(github.ref, 'refs/tags/v')\n"
        "    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: bash tools/deploy/pack-bundle.sh\n"
        # 总账腿同样只留空壳（不接 release-verify.sh、不声明三类）：判据要有靶子。
        "  release-verify:\n    if: startsWith(github.ref, 'refs/tags/v')\n"
        "    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: echo no-op\n",
        encoding="utf-8",
    )


def _write_limits(wf):
    """写入 big/cycle 规模与成环夹具。"""
    many = "".join(
        f"  j{i}:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        for i in range(MAX_JOBS + 1)
    )
    (wf / "big.yml").write_text(
        "name: big\non:\n  push:\n    branches: [main]\n"
        "concurrency:\n  group: b\njobs:\n" + many,
        encoding="utf-8",
    )
    (wf / "cycle.yml").write_text(
        "name: cyc\non:\n  push:\n    branches: [main]\n"
        "concurrency:\n  group: c\njobs:\n"
        "  a:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    needs: [b]\n"
        "  b:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n    needs: [a]\n",
        encoding="utf-8",
    )


def _write_platforms(wf):
    """写入 platforms/gate/buildroot 平台与门禁夹具。"""
    (wf / "platforms.yml").write_text(
        "name: platforms\non:\n  push:\n    branches: [main]\n"
        "permissions:\n  contents: read\n"
        "concurrency:\n  group: p\njobs:\n"
        "  cross-artifacts:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - run: echo x86_64-unknown-linux-gnu\n",
        encoding="utf-8",
    )
    (wf / "gate.yml").write_text(
        "name: g\non:\n  push:\n    branches: [main]\n"
        "permissions:\n  contents: read\n"
        "concurrency:\n  group: g\njobs:\n"
        "  lint:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    steps:\n      - uses: actions/checkout@v7\n",
        encoding="utf-8",
    )
    (wf / "buildroot.yml").write_text(
        "name: br\non:\n  push:\n    tags: [\"v*\"]\n"
        "permissions:\n  contents: read\n"
        "concurrency:\n  group: br\njobs:\n"
        "  release:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
        "    if: startsWith(github.ref, 'refs/tags/v')\n"
        "    permissions:\n      contents: read\n      packages: write\n"
        "    steps:\n      - run: oras push ghcr.io/o/rockrobot3-ota:v${VER}\n",
        encoding="utf-8",
    )


def _cases(bad):
    """返回 30 条常备断言用例表（另有 7 条由各负例夹具在 self_test 内追加）。"""
    return [
        ("缺 concurrency 被抓", any("concurrency" in b for b in bad)),
        ("缺 timeout 被抓", any("timeout-minutes" in b for b in bad)),
        ("版本漂移被抓", any("版本漂移" in b for b in bad)),
        ("needs 悬空被抓", any("不存在的 job" in b for b in bad)),
        ("needs 成环被抓", any("成环" in b for b in bad)),
        ("push 无范围被抓", any("branches/tags" in b for b in bad)),
        ("工具链未钉版被抓", any("工具链未显式钉" in b for b in bad)),
        ("缓存缺 workspaces 被抓", any("缺 workspaces" in b for b in bad)),
        ("paths-ignore 缺失被抓", any("paths-ignore" in b for b in bad)),
        ("publish 可取消被抓", any("cancel-in-progress: false" in b for b in bad)),
        ("job 超限被抓", any("> 上限" in b for b in bad)),
        ("continue-on-error 被抓", any("continue-on-error" in b for b in bad)),
        ("缺 permissions 被抓", any("缺顶层 permissions" in b for b in bad)),
        ("芯片矩阵缩水被抓", any("五芯片覆盖不得缩水" in b for b in bad)),
        ("芯片 job 缺失被抓", any("缺 job esp32-gates" in b for b in bad)),
        ("芯片验证步骤缺失被抓", any("验证步骤" in b for b in bad)),
        ("riscv 镜像腿缺失被抓", any("缺 job riscv-image" in b for b in bad)),
        ("发布面缺腿被抓", any("缺发布 job" in b for b in bad)),
        ("发布矩阵缺 arm64 被抓", any("build 矩阵缺" in b for b in bad)),
        ("发布镜像冒烟缺失被抓", any("stage-image.sh" in b for b in bad)),
        ("发布运行面缺模块被抓", any("运行面缺模块 sense" in b for b in bad)),
        ("门禁 job 缩水被抓", any("缺 job conventions" in b for b in bad)),
        ("固件缺 S3 被抓", any("未构建 S3 目标" in b for b in bad)),
        ("发布包腿未托管被抓", any("未推 rockrobot3-distro" in b for b in bad)),
        ("发布总账腿缺失被抓", any("未复用 tools/ci/lib/release-verify.sh" in b for b in bad)),
        ("匿名回读 workflow 缺失被抓", any("缺 workflow release-anon.yml" in b for b in bad)),
        ("制品面缺齐套巡检被抓", any("未接 release-verify.sh" in b for b in bad)),
        ("制品面缺名被抓", any("未推 rockrobot3-buildroot" in b for b in bad)),
        ("集成腿缺验证脚本被抓", any("缺验证脚本 distro-smoke.sh" in b for b in bad)),
        ("好例零误报", not any("good.yml" in b for b in bad)),
    ]


def self_test():
    """建临时夹具跑 audit 并逐条打印 37 条断言结果。"""
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        wf = root / ".github" / "workflows"
        wf.mkdir(parents=True)
        _write_good(wf)
        _write_bad(wf)
        _write_flows(wf)
        _write_limits(wf)
        _write_platforms(wf)
        bad = audit(root)
        # 标签口径负例（批284）：同一 job 把 v<VER> 改回 <VER>，必须被当场点名。
        (wf / "buildroot.yml").write_text(
            "name: br\non:\n  push:\n    tags: [\"v*\"]\n"
            "permissions:\n  contents: read\n"
            "concurrency:\n  group: br\njobs:\n"
            "  release:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    if: startsWith(github.ref, 'refs/tags/v')\n"
            "    permissions:\n      contents: read\n      packages: write\n"
            "    steps:\n      - run: oras push ghcr.io/o/rockrobot3-ota:${VER}\n",
            encoding="utf-8",
        )
        bad2 = audit(root)
        # 匿名回读命令负例（批286）：丢签名、砍类别、固定 tag——三者都必须被点名。
        (wf / "release-anon.yml").write_text(
            "name: release-anon\non:\n  schedule:\n    - cron: \"0 3 * * 1\"\n"
            "permissions:\n  contents: read\nconcurrency:\n  group: a\n"
            "  cancel-in-progress: true\njobs:\n"
            "  anon:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: bash tools/ci/lib/release-verify.sh --tag v0.0.8 --classes image\n",
            encoding="utf-8",
        )
        bad3 = audit(root)
        # 周程触发负例（批287）：删 schedule 只剩手动，必须点名（runs 永远 0 的隐蔽失效）。
        (wf / "release-anon.yml").write_text(
            "name: release-anon\non:\n  workflow_dispatch:\n"
            "permissions:\n  contents: read\nconcurrency:\n  group: a\n"
            "  cancel-in-progress: true\njobs:\n"
            "  anon:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: bash tools/ci/lib/release-verify.sh --tag auto "
            "--with-signature --classes image,firmware\n",
            encoding="utf-8",
        )
        bad4 = audit(root)
        # 门禁主线入口负例（批291）：conventions 换成 echo，其余 job 保留判据入口。
        (wf / "gate.yml").write_text(
            "name: g\non:\n  push:\n    branches: [main]\n"
            "permissions:\n  contents: read\nconcurrency:\n  group: g\njobs:\n"
            "  conventions:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: echo no-op\n"
            "  release:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: cargo test --locked --release --workspace\n"
            "  supply:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: bash tools/ci/platform/supply-audit.sh --strict\n"
            "  alsa:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: cargo check -p rr-hal --features alsa\n"
            "  ota-bundle:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: bash tools/deploy/ota-bundle.sh\n"
            "  scripts:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: |\n"
            "          python3 tools/ci/audit/workflows_audit.py\n"
            "          python3 tools/ci/audit/metrics.py\n"
            "          python3 tools/ci/audit/release_sets.py\n"
            "          python3 tools/ci/audit/gate_steps_audit.py\n",
            encoding="utf-8",
        )
        bad5 = audit(root)
        # 覆盖率入口负例（批293）：workspace 换成 echo，independent 保留——只该点名前者。
        (wf / "coverage.yml").write_text(
            "name: coverage\non:\n  push:\n    branches: [main]\n"
            "permissions:\n  contents: read\nconcurrency:\n  group: cov\njobs:\n"
            "  workspace:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: echo no-op\n"
            "  independent:\n    runs-on: ubuntu-latest\n    timeout-minutes: 5\n"
            "    steps:\n      - run: cargo llvm-cov --locked --manifest-path tools/x/Cargo.toml\n",
            encoding="utf-8",
        )
        bad6 = audit(root)

    cases = _cases(bad)
    cases.append(("制品标签漏 v 被抓", any("标签必须 v<VER>" in b for b in bad2)))
    cases.append(("匿名回读丢签名被抓", any("缺 --with-signature" in b for b in bad3)))
    cases.append(("匿名回读类别被削被抓", any("类别必须 image,firmware" in b for b in bad3)))
    cases.append(("匿名回读固定 tag 被抓", any("必须 --tag auto" in b for b in bad3)))
    cases.append(("匿名回读缺周程被抓", any("缺 schedule" in b for b in bad4)))
    cases.append(("门禁主线入口被削被抓", any("缺判据入口 tools/ci/check.sh" in b for b in bad5)))
    cases.append(("覆盖率入口被削被抓", any("缺覆盖率入口 cargo llvm-cov --locked --workspace" in b for b in bad6)))
    for name, ok in cases:
        print(("PASS  " if ok else "FAIL  ") + name)
    return 0 if all(ok for _, ok in cases) else 1
