#!/usr/bin/env bash
# distro-smoke —— 发行版快路轨冒烟（0.0.2 T5 / D8，ADR-0013 裁定 5 首证）。
#
# 单步容器（rust:bookworm，glibc 自洽）内完成：构建 → 打包器单测 → 组装注入产物 →
# rr3-pack 出 deb → dpkg -i → rrd 装配单正门冒烟（六 Rust 模块 spawn——0.0.8 批N32
# 件四起 brain 也是 Rust，随 deb 装机进 boot 队列，distro.toml 四格绑定兑现）。
# 容器无 RK3566 硬件面：装机 distro.toml 的部署真值 hal_backend=linux 在容器内
# 不可兑现（P0-3 起 supervisor 按装配单注入子进程），⑥ 前把装机副本变换为
# sim（与 tools/sim::assemble 沙箱变换同一语义——仓源装配单零改，只改装机副本）。
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

  echo "== ① 构建：base/runtime 工作区 + 六个 Rust 模块 + rr3-ctl + rr3-pack =="
  cargo build -q --locked --release --manifest-path base/runtime/Cargo.toml -p rrd
  for m in act brain connect interact sense power; do
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
  for m in act brain connect interact sense power; do
    mkdir -p "$PAY/rr3-modules/rr3-$m"
    cp "$REL/rr3-$m" "$PAY/rr3-modules/rr3-$m/bin"
    cp "modules/rr3-$m/module.toml" "$PAY/rr3-modules/rr3-$m/manifest.toml"
  done

  echo "== ④ rr3-pack 出 deb 并安装 =="
  ARCH=$(dpkg --print-architecture)
  VER="${RR3_DEB_VERSION:-0.0.6}"
  export RR3_DEB_VERSION="$VER"
  /tmp/rr3-target/release/rr3-pack \
    --payload "$PAY" --repo /work --arch "$ARCH" \
    --out "/tmp/rockrobot3_${VER}_${ARCH}.deb"
  dpkg -i "/tmp/rockrobot3_${VER}_${ARCH}.deb"

  echo "== ⑤ 装机位断言（rrd.service 同一组路径；点名式——败即报缺席路径与装机实况，不再裸退）=="
  # 点名断言 helper：失败打印缺席路径 + 父目录 ls + deb 清单/校验比对（装机里到底
  # 有没有）。断言集与断言语义不变——只把"哪条败了、为什么"从猜变成报。
  must_file() {
    test -f "$1" && return 0
    echo "FAIL ⑤: 缺普通文件 $1"
    ls -la "$(dirname "$1")" 2>&1 || true
    echo "--- dpkg -L 同名项（deb 数据包清单）---"
    dpkg -L rockrobot3 2>/dev/null | grep -F "$(basename "$1")" || echo "  （清单无 $(basename "$1")——deb 未含此路径）"
    echo "--- dpkg -V 校验（仅异常项）---"
    dpkg -V rockrobot3 2>&1 || true
    exit 1
  }
  must_exec() {
    test -x "$1" && return 0
    if test -e "$1"; then echo "FAIL ⑤: $1 存在但无可执行位"; ls -la "$1"; else echo "FAIL ⑤: 缺可执行 $1"; ls -la "$(dirname "$1")" 2>&1 || true; fi
    echo "--- dpkg -L 同名项（deb 数据包清单）---"
    dpkg -L rockrobot3 2>/dev/null | grep -F "$(basename "$1")" || echo "  （清单无 $(basename "$1")——deb 未含此路径）"
    exit 1
  }
  must_dir() {
    test -d "$1" && return 0
    echo "FAIL ⑤: 缺目录 $1"
    ls -la "$(dirname "$1")" 2>&1 || true
    exit 1
  }
  must_grep() {
    grep -qF "$1" "$2" && return 0
    echo "FAIL ⑤: $2 不含 [$1]（文件实况如下）"
    cat "$2" 2>&1 || true
    exit 1
  }
  must_exec /usr/bin/rrd
  must_exec /usr/bin/rr3-ctl
  must_file /usr/lib/systemd/system/rrd.service
  must_dir /var/lib/rockrobot3 # postinst 建目录（RR3_DATA_DIR，0.0.6 批E-R3）
  # 注：本文件 ⑤-⑧ 整体在 docker bash -c 单引号串内，注释/文案禁用 ASCII 撇号
  # （撇号曾把 -c 串提前闭合，L93 起溢出到宿主执行——run#1-5 五连败根因）
  must_file /usr/lib/rockrobot3/profiles/distro.toml
  must_file /usr/lib/rockrobot3/contracts/topics.toml
  must_file /usr/lib/rockrobot3/contracts/slots.toml
  must_file /usr/lib/rockrobot3/contracts/roles.toml
  must_file /usr/lib/rockrobot3/contracts/devices.toml
  # 平台事实表（装载必需件，缺即拒 boot——块-15：此前装机断言看不见它缺席的洞）。
  # 册位随装配单 [platform].chip 选册（distro→rk3566）；清单真值 = rrd
  # rr3_runtime_truth_set（基-25），本断言集与 rr3-pack 打包面同单对账。
  must_file /usr/lib/rockrobot3/contracts/platform/rk3566.toml
  must_file /usr/lib/rockrobot3/hardware/distro/wiring.toml
  for m in act brain connect interact sense power; do
    must_exec "/usr/lib/rockrobot3/modules/rr3-$m/rr3-$m"
    must_file "/usr/lib/rockrobot3/modules/rr3-$m/module.toml"
    must_file "/usr/lib/rockrobot3/modules/rr3-$m/config.toml"
  done
  must_grep "/usr/lib/rockrobot3/modules/rr3-act/rr3-act" \
    /usr/lib/rockrobot3/modules/rr3-act/module.toml # entry 已装机绝对化

  echo "== ⑥ 装机装配单 sim 变换（部署真值零改；容器无 RK3566 硬件面，见文件头）=="
  must_grep 'hal_backend = "linux"' /usr/lib/rockrobot3/profiles/distro.toml # 变换前在位
  sed -i 's/^hal_backend = "linux"$/hal_backend = "sim"/' /usr/lib/rockrobot3/profiles/distro.toml
  must_grep 'hal_backend = "sim"' /usr/lib/rockrobot3/profiles/distro.toml # 变换后生效

  echo "== ⑦ rrd 装配单正门冒烟（参数与 rrd.service 的 ExecStart 一致，仅套接字换测试位；RR3_DATA_DIR 同 unit Environment）=="
  SOCK=/tmp/smoke-ipc.sock
  # RR3_BRAIN_CLOUD=mock：brain 云端压成确定性行为（不发网络，与 tools/sim 演练同一纪律）
  RR3_DATA_DIR=/var/lib/rockrobot3 RR3_BRAIN_CLOUD=mock /usr/bin/rrd --socket "$SOCK" \
    --contracts /usr/lib/rockrobot3/contracts \
    --profile /usr/lib/rockrobot3/profiles/distro.toml >/tmp/rrd-smoke.log 2>&1 &
  RRDPID=$!
  trap "kill $RRDPID 2>/dev/null || true" EXIT

  echo "== ⑧ 冒烟断言：六模块 spawn（含 Rust brain）+ 0 拒 boot + 0 denied =="
  ok=""
  for _ in $(seq 1 30); do
    n=0
    for m in act connect interact sense power brain; do
      grep -q "spawn rr3-$m pid=" /tmp/rrd-smoke.log && n=$((n + 1))
    done
    if [ "$n" -eq 6 ]; then ok=1; break; fi
    sleep 1
  done
  if [ -z "$ok" ]; then
    echo "--- rrd 日志（spawn 不齐）---"
    cat /tmp/rrd-smoke.log
    exit 1
  fi
  if grep -q "拒绝 boot" /tmp/rrd-smoke.log; then
    echo "--- rrd 日志（出现 拒绝 boot）---"
    cat /tmp/rrd-smoke.log
    exit 1
  fi
  /usr/bin/rr3-ctl --bus-path "$SOCK" status | grep -q "denied_publish = 0"

  echo "== ⑨ counters 回写断言（0.0.6 批E-R3：持久化启用 + boots 落盘）=="
  grep -q "counters 持久化已启用: /var/lib/rockrobot3/counters.json（boots 自增已回写）" /tmp/rrd-smoke.log
  test -f /var/lib/rockrobot3/counters.json
  grep -q "\"boots\"" /var/lib/rockrobot3/counters.json

  echo "distro-smoke 绿：deb 装机 → rrd 正门起六 Rust 模块（sim 面冒烟，部署真值零改），counters 回写落盘"
'
