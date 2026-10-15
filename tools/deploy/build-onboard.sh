#!/usr/bin/env bash
# build-onboard —— 板上原生构建承载件（0.0.6 批F1，洞 L；R13 音频件换装）。
#
# 为什么需要它：musl 全静态与 alsa 结构性互斥（alsa-sys 绑 C libasound 动态库 + 运行时
# 插件），交叉路径不存在——真麦采集 / 真播放的音频件（rr3-sense / rr3-interact 带
# `alsa` feature）只能在**板上原生构建**。本脚本把"源码上板 → 板上取依赖 → 板上构建 →
# 产物归位 staging"四步收敛成一条链，供 pack-arm64.sh 出的 staging 树合流后板上
# `dpkg-deb --build`（装机步骤见 base/os/distro/boards/raspberry-pi-64.md）。
#
# 分工：宿主 musl 静态件（rrd / rr3-ctl / 五模块宿主构建件）仍走 pack-arm64.sh；
# 本脚本**只覆盖**音频两件（sense / interact）的 bin——先跑 pack-arm64.sh 备好 payload，
# 再跑本脚本把音频两件的 bin 换成板上原生构建件（动态 ELF + libasound）。
#
# 用法: build-onboard.sh <user@host> [out_dir] [remote_src_dir]
#   user@host       板上 SSH 目标（免密已配，如 pi@192.168.124.52）
#   out_dir         pack-arm64.sh 的同一出参目录（缺省 /tmp/rr3-arm64-deploy）
#   remote_src_dir  板上源码根（缺省 ~/rockrobot3-src）
#
# 前置门（任一不过即点名退出）：板上 cargo/rustc（rustup）且 rustc ≥1.82（workspace 锁
# 解锁门）、`libasound2-dev` + `pkg-config`（Debian 系：sudo apt-get install -y
# libasound2-dev pkg-config）。
set -euo pipefail

cd "$(dirname "$0")/../.." # 仓根

HOST="${1:-}"
OUT="${2:-/tmp/rr3-arm64-deploy}"
RMT_SRC="${3:-~/rockrobot3-src}"
PAY="$OUT/payload"

if [ -z "$HOST" ]; then
  echo "用法: build-onboard.sh <user@host> [out_dir] [remote_src_dir]" >&2
  exit 2
fi

# 远端命令统一走登录 shell 的 PATH（非交互 ssh 默认不带 ~/.cargo/bin）
rrun() { ssh "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; $1"; }

echo "== ① 板上依赖门（cargo/rustc ≥1.82 + alsa 开发件）=="
if ! rrun 'command -v cargo >/dev/null && cargo --version && rustc --version'; then
  echo "板上缺 cargo/rustc：装 rustup（https://rustup.rs）" >&2
  exit 1
fi
ver="$(rrun 'rustc --version')"
# 数值比较（awk 拆主次版本；字符串比较会把 1.9 误判 > 1.82）
if ! printf '%s\n' "$ver" | awk '{split($2,a,"."); exit !(a[1]>1 || (a[1]==1 && a[2]>=82))}'; then
  echo "板上 rustc < 1.82（workspace 锁解锁门未过）：rustup 升级 aarch64，或转备选 gnu 自产 sysroot（R13）" >&2
  exit 1
fi
if ! rrun 'pkg-config --exists alsa && pkg-config --modversion alsa'; then
  echo "板上缺 alsa 开发件：sudo apt-get install -y libasound2-dev pkg-config" >&2
  exit 1
fi

echo "== ② 源码同步（rsync 排除 target/、.git/ 与四个 ESP32 宿主工具目录——板上构建闭包只含 base/+modules/，0.0.6 批F5 收口批收窄）=="
rsync -a --delete \
  --exclude 'target/' --exclude '.git/' \
  --exclude 'tools/esp32-periph-base/' --exclude 'tools/espidf-sentinel/' \
  --exclude 'tools/espidf-std-hello/' --exclude 'tools/esp32-uart-echo/' \
  ./ "$HOST:$RMT_SRC/"

echo "== ③ 依赖通道（板上有外网则直连 crates.io；无外网则预置 vendor）=="
# 仓库根无 Cargo.toml（分立 workspace 布局：base/runtime、modules/*、tools/* 各自独立，
# 首跑真板踩实）——对本脚本 ④ 实际构建的两个 manifest 各自 fetch
for m in sense interact; do
  if ! rrun "cd $RMT_SRC/modules/rr3-$m && cargo fetch --locked"; then
    echo "cargo fetch 失败（rr3-$m）：板无外网时在宿主 \`cargo vendor --versioned-dirs <vendor_dir>\`，" >&2
    echo "把 vendor/ 与指向它的 .cargo/config.toml 一并同步上板，或改用有网环境后重跑。" >&2
    exit 1
  fi
done

echo "== ④ 板上原生构建（音频两件带 alsa feature，release）=="
for m in sense interact; do
  echo "-- rr3-$m"
  rrun "cd $RMT_SRC && cargo build --locked --release --features alsa --manifest-path modules/rr3-$m/Cargo.toml"
done

echo "== ⑤ 产物归位 staging（payload/rr3-modules/<name>/bin）=="
for m in sense interact; do
  mkdir -p "$PAY/rr3-modules/rr3-$m"
  rsync "$HOST:$RMT_SRC/modules/rr3-$m/target/release/rr3-$m" "$PAY/rr3-modules/rr3-$m/bin"
done

echo "== ⑥ 板上构建件断言（动态 ELF aarch64 + ldd 含 libasound）=="
for m in sense interact; do
  bin="$PAY/rr3-modules/rr3-$m/bin"
  file "$bin" | grep -q "ELF 64-bit.*ARM aarch64" # 板上原生构建件是 aarch64 ELF
  if file "$bin" | grep -q "statically linked"; then
    echo "rr3-$m 是静态产物（应为动态链 libasound——板上构建未生效？）" >&2
    exit 1
  fi
  rrun "ldd $RMT_SRC/modules/rr3-$m/target/release/rr3-$m" | grep -q libasound
done

echo "payload 音频件已换成板上原生构建件: $PAY"
echo "下一步：在主 staging 树合流后板上 dpkg-deb --build（见 boards/raspberry-pi-64.md）"