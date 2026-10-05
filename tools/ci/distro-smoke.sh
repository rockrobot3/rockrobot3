#!/usr/bin/env bash
# distro-smoke —— 发行版快路轨冒烟（0.0.2 T5 / D8，ADR-0013 裁定 5 首证）。
#
# 单步容器（rust:bookworm，glibc 自洽）内完成：构建 → 打包器单测 → 组装注入产物 →
# rr3-pack 出 deb → dpkg -i → rrd 装配单正门冒烟（5 模块 spawn + rr3-brain 缺格点名）。
# macOS 本机无法真装 deb（0.0.2 方案 P1）——打包器装配单测本机跑，真装只认这里（远端回执才算数）。
#
# 仅 CI 调用（需 docker）；本地想单跑装配逻辑用 rr3-pack --stage-only。
set -euo pipefail

cd "$(dirname "$0")/../.." # 仓根

IMAGE="rust:bookworm"
docker run --rm -v "$(pwd)":/work -w /work "$IMAGE" bash -euo pipefail -c '
  # 产物统一落 /tmp：容器以 root 跑，不往挂载的仓里写 target/（避免本地残根属主文件）
  export CARGO_TARGET_DIR=/tmp/rr3-target
  REL=$CARGO_TARGET_DIR/release

  echo "== ① 构建：base/runtime 工作区 + 五个 Rust 模块 + rr3-ctl + rr3-pack =="
  cargo build -q --locked --release --manifest-path base/runtime/Cargo.toml -p rrd
  for m in act connect interact sense power; do
    cargo build -q --locked --release --manifest-path "modules/rr3-$m/Cargo.toml"
  done
  cargo build -q --locked --release --manifest-path tools/rrctl/Cargo.toml
  cargo build -q --locked --release --manifest-path tools/rr3-pack/Cargo.toml

  echo "== ② 打包器单测（D8 判据：打包脚本单测）=="
  cargo test -q --locked --manifest-path tools/rr3-pack/Cargo.toml

  echo "== ③ 组装注入产物（两轨同源约定：rr3-modules/<name>/{bin,manifest.toml}）=="
  PAY=/tmp/rr3-payload
  rm -rf "$PAY"
  mkdir -p "$PAY/rr3-modules"
  cp "$REL/rrd" "$REL/rr3-ctl" "$PAY/"
  for m in act connect interact sense power; do
    mkdir -p "$PAY/rr3-modules/rr3-$m"
    cp "$REL/rr3-$m" "$PAY/rr3-modules/rr3-$m/bin"
    cp "modules/rr3-$m/module.toml" "$PAY/rr3-modules/rr3-$m/manifest.toml"
  done

  echo "== ④ rr3-pack 出 deb 并安装 =="
  ARCH=$(dpkg --print-architecture)
  /tmp/rr3-target/release/rr3-pack \
    --payload "$PAY" --repo /work --arch "$ARCH" \
    --out "/tmp/rockrobot3_0.0.6_${ARCH}.deb"
  dpkg -i "/tmp/rockrobot3_0.0.6_${ARCH}.deb"

  echo "== ⑤ 装机位断言（rrd.service 同一组路径）=="
  test -x /usr/bin/rrd
  test -x /usr/bin/rr3-ctl
  test -f /usr/lib/systemd/system/rrd.service
  test -d /var/lib/rockrobot3 # postinst 建目录（RR3_DATA_DIR，0.0.6 批E' R3）
  test -f /usr/lib/rockrobot3/profiles/distro.toml
  test -f /usr/lib/rockrobot3/contracts/topics.toml
  test -f /usr/lib/rockrobot3/contracts/slots.toml
  for m in act connect interact sense power; do
    test -x "/usr/lib/rockrobot3/modules/rr3-$m/rr3-$m"
    test -f "/usr/lib/rockrobot3/modules/rr3-$m/module.toml"
    test -f "/usr/lib/rockrobot3/modules/rr3-$m/config.toml"
  done
  grep -q "/usr/lib/rockrobot3/modules/rr3-act/rr3-act" \
    /usr/lib/rockrobot3/modules/rr3-act/module.toml # entry 已装机绝对化

  echo "== ⑥ rrd 装配单正门冒烟（参数与 rrd.service 的 ExecStart 一致，仅套接字换测试位；RR3_DATA_DIR 同 unit Environment）=="
  SOCK=/tmp/smoke-ipc.sock
  RR3_DATA_DIR=/var/lib/rockrobot3 /usr/bin/rrd --socket "$SOCK" \
    --contracts /usr/lib/rockrobot3/contracts \
    --profile /usr/lib/rockrobot3/profiles/distro.toml >/tmp/rrd-smoke.log 2>&1 &
  RRDPID=$!
  trap "kill $RRDPID 2>/dev/null || true" EXIT

  echo "== ⑦ 冒烟断言：5 模块 spawn + rr3-brain 缺格点名 + 0 拒 boot + 0 denied =="
  ok=""
  for _ in $(seq 1 30); do
    n=0
    for m in act connect interact sense power; do
      grep -q "spawn rr3-$m pid=" /tmp/rrd-smoke.log && n=$((n + 1))
    done
    if [ "$n" -eq 5 ]; then ok=1; break; fi
    sleep 1
  done
  if [ -z "$ok" ]; then
    echo "--- rrd 日志（spawn 不齐）---"
    cat /tmp/rrd-smoke.log
    exit 1
  fi
  grep -q "拒绝启动 rr3-brain：缺格 cognition.dialog" /tmp/rrd-smoke.log # P2 缺格点名，D2 语义首证
  if grep -q "拒绝 boot" /tmp/rrd-smoke.log; then
    echo "--- rrd 日志（出现 拒绝 boot）---"
    cat /tmp/rrd-smoke.log
    exit 1
  fi
  /usr/bin/rr3-ctl --bus-path "$SOCK" status | grep -q "denied_publish = 0"

  echo "== ⑧ counters 回写断言（0.0.6 批E' R3：持久化启用 + boots 落盘）=="
  grep -q "counters 持久化已启用: /var/lib/rockrobot3/counters.json（boots 自增已回写）" /tmp/rrd-smoke.log
  test -f /var/lib/rockrobot3/counters.json
  grep -q '"boots"' /var/lib/rockrobot3/counters.json

  echo "distro-smoke 绿：deb 装机 → rrd 正门起 5 模块，缺格点名生效，counters 回写落盘"
'
