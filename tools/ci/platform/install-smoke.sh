#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 装机器正例（批217）：真 deb → systemd 起 rrd（sim 档）→ 版本化 bundle 换树 → 回滚。
#
# 为什么需要它：distro-smoke 在容器里直接跑 rrd（容器没有 systemd），platforms 的
# 装机器只验"干净机拒绝"；install-bundle.sh 的**成功换树**与**失败回滚**两条路径
# 此前没有任何流水线执行过——它们是 RK3566 distro 快路轨的唯一落地方式，坏了要等
# 真板才发现。GitHub hosted runner 是带 systemd 的 VM，本检查因此装真 deb、由
# unit 拉起 rrd，再以真 unit 跑 bundle 换树与坏包回滚。
#
# 部署真值零改：仓内 profiles/distro.toml 是 hal_backend=linux（真板语义），CI 机
# 没有硬件面。装配单是部署数据，本脚本只改**装机副本**与**bundle 解包副本**（与
# distro-smoke ⑥、tools/sim::assemble 同一纪律），仓源一行不动；探测档与云端同由
# systemd drop-in 显式注入：RR3_PROBE=sim、RR3_BRAIN_CLOUD=mock。
#
# 用法：bash tools/ci/platform/install-smoke.sh
# 前置：Linux + 运行中的 systemd + 免密 sudo + dpkg-deb + Rust 工具链。
# 退出：0 = 正例与回滚例都成立；1 = 任一步失败（ERR trap 打印 journal 与树实况）。
set -euo pipefail
cd "$(dirname "$0")/../../.." # 仓根

TRIPLE=x86_64-unknown-linux-gnu
ARCH=x86-64
REL=/tmp/rr3-build/$TRIPLE/release
WORK=/tmp/rr3-install-smoke
ROOT=/usr/lib/rockrobot3
SOCK=/var/run/rock3-ipc.sock
MODULES="act brain connect interact sense power"
UNITS="rr3-act rr3-connect rr3-interact rr3-sense rr3-power rr3-brain"

die() { echo "FAIL  $*" >&2; exit 1; }

# root 直跑或免密 sudo 二选一：CI runner 是非 root + sudo，root 面（容器/真板）直接执行。
as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo -n "$@"; fi
}

# 失败现场：unit 状态 + journal + 装机树。归因不能只看脚本自己的一行 FAIL——
# rrd 为什么没起来在 journal 里，现装的是哪棵树在 BUNDLE_VERSION 里。
dump_state() {
  echo "=== 失败现场（systemd / journal / 装机树）===" >&2
  as_root systemctl status rrd --no-pager -l 2>&1 | tail -30 >&2 || true
  as_root journalctl -u rrd --no-pager -n 60 2>&1 | tail -60 >&2 || true
  ls -la /usr/lib/.rockrobot3-prev 2>&1 | head -5 >&2 || true
  sed -n 's/^commit=/装机树 BUNDLE_VERSION commit=/p' \
    "$ROOT/BUNDLE_VERSION" 2>/dev/null >&2 || true
  as_root /usr/bin/rr3-ctl --bus-path "$SOCK" status 2>&1 | head -20 >&2 || true
}
trap dump_state ERR

# 宿主前置：systemd 真在跑（容器直跑会在第一步就点名，不装到一半才发现没 init）。
preflight() {
  local st
  [ -d /run/systemd/system ] \
    || die "本机无运行中的 systemd（/run/systemd/system 缺席）：本检查只跑在带 systemd 的宿主/runner"
  st=$(systemctl is-system-running 2>&1 || true)
  case "$st" in
    running | degraded) echo "systemd: $st" ;;
    *) die "systemd 不可用（is-system-running=${st}）" ;;
  esac
  command -v dpkg-deb >/dev/null || die "缺 dpkg-deb（发行版打包器出不了 deb）"
  command -v cargo >/dev/null || die "缺 cargo（本检查自建原生产物）"
  if [ "$(id -u)" -ne 0 ]; then
    command -v sudo >/dev/null || die "非 root 且无 sudo：装机器无法落位"
    sudo -n true 2>/dev/null || die "sudo 需免密（CI runner 面）；交互式 sudo 不做"
  fi
}

# 原生 x86_64 十件产物（六模块 + rrd + 三工具）。--target 让 pack-bundle 的
# 产物定位（rr3_cargo_artifact）与后续 bundle 出包同一路径，不复制第二套规则。
build_all() {
  local m
  cargo build -q --locked --release --target "$TRIPLE" \
    --manifest-path base/runtime/Cargo.toml -p rrd
  for m in $MODULES; do
    cargo build -q --locked --release --target "$TRIPLE" \
      --manifest-path "modules/rr3-$m/Cargo.toml"
  done
  cargo build -q --locked --release --target "$TRIPLE" \
    --manifest-path tools/rrctl/Cargo.toml
  cargo build -q --locked --release --target "$TRIPLE" \
    --manifest-path base/ota/rr3-ota-ctl/Cargo.toml
  cargo build -q --locked --release --target "$TRIPLE" \
    --manifest-path tools/rr3-pack/Cargo.toml
}

# 注入载荷（两轨同源约定：rr3-modules/<name>/{bin,manifest.toml}）。
make_payload() {
  local m
  rm -rf "$WORK/payload"
  mkdir -p "$WORK/payload/rr3-modules"
  cp "$REL/rrd" "$REL/rr3-ctl" "$REL/rr3-ota-ctl" "$WORK/payload/"
  for m in $MODULES; do
    mkdir -p "$WORK/payload/rr3-modules/rr3-$m"
    cp "$REL/rr3-$m" "$WORK/payload/rr3-modules/rr3-$m/bin"
    cp "modules/rr3-$m/module.toml" "$WORK/payload/rr3-modules/rr3-$m/manifest.toml"
  done
}

# 真 deb 装机：postinst 建数据目录、装 unit、enable（不 --now，起停归 systemd）。
install_deb() {
  local arch ver deb
  arch=$(dpkg --print-architecture)
  ver=$(cat VERSION)
  deb="$WORK/rockrobot3_${ver}_${arch}.deb"
  "$REL/rr3-pack" --payload "$WORK/payload" --repo "$PWD" \
    --arch "$arch" --out "$deb"
  as_root dpkg -i "$deb"
  [ -f /usr/lib/systemd/system/rrd.service ] || die "deb 未装 rrd.service"
  as_root systemctl daemon-reload
}

# 装机副本 sim 变换 + 探测档注入（仓源零改；语义见文件头）。
sim_deploy() {
  local prof="$ROOT/profiles/distro.toml" drop=/etc/systemd/system/rrd.service.d
  grep -q '^hal_backend = "linux"' "$prof" \
    || die "装机装配单缺 hal_backend=linux（deb 面漂移）"
  as_root sed -i 's/^hal_backend = "linux"/hal_backend = "sim"/' "$prof"
  as_root mkdir -p "$drop"
  printf '[Service]\nEnvironment=RR3_PROBE=sim\nEnvironment=RR3_BRAIN_CLOUD=mock\n' \
    | as_root tee "$drop/50-ci-sim.conf" >/dev/null
  as_root systemctl daemon-reload
}

# 六模块 running 判据（逐模块断言行，不拿"表里有字"冒充；denied_publish 必须为 0）。
wait_modules() {
  local i m out ok
  for i in $(seq 1 30); do
    if out=$(as_root /usr/bin/rr3-ctl --bus-path "$SOCK" status 2>/dev/null); then
      ok=1
      for m in $UNITS; do
        awk -v m="$m" '$1==m && $3=="running"{f=1} END{exit !f}' <<<"$out" \
          || ok=0
      done
      grep -q 'denied_publish = 0' <<<"$out" || ok=0
      [ "$ok" -eq 1 ] && return 0
    fi
    sleep 1
  done
  echo "--- 最近一次 rr3-ctl status ---" >&2
  printf '%s\n' "${out:-<无应答>}" >&2
  die "30s 内六模块未全 running（${UNITS}）"
}

# unit 拉起 + 六模块判据（正例第一段：deb 装机面由 systemd 真拉起）。
start_unit() {
  as_root systemctl start rrd || die "systemctl start rrd 失败"
  as_root systemctl is-active --quiet rrd || die "rrd 未 active"
  wait_modules
  echo "     unit active，六模块 running"
  echo "::notice title=install-smoke-systemd::deb 装机 + systemd unit 拉起成立，六模块 running"
}

# 版本化 bundle：真出包 → 解包副本 profile 改 sim → 重打包（BUNDLE_VERSION 原样，
# 顶层名不变，A10 契约仍由 bundle-contract.sh 复核）。
make_bundle() {
  local bundle="$WORK/good.tar.gz" tmp top
  bash tools/deploy/pack-bundle.sh "$TRIPLE" "$bundle"
  tmp=$(mktemp -d "$WORK/unpack.XXXXXX")
  tar -xzf "$bundle" -C "$tmp"
  top=$(basename "$(find "$tmp" -mindepth 1 -maxdepth 1 -type d)")
  sed -i 's/^hal_backend = "linux"/hal_backend = "sim"/' \
    "$tmp/$top/profiles/distro.toml"
  grep -q '^hal_backend = "sim"' "$tmp/$top/profiles/distro.toml" \
    || die "bundle 副本 sim 变换未生效"
  tar -C "$tmp" -czf "$WORK/sim.tar.gz" "$top"
  bash tools/ci/platform/bundle-contract.sh "$WORK/sim.tar.gz" "$TRIPLE" "$ARCH"
  echo "     $WORK/sim.tar.gz（profile=sim，BUNDLE_VERSION 原样）"
}

# 正例第二段：真 unit 下换树。install-bundle 自己会停 rrd、换树、起 rrd、逐模块
# 探针；这里只补外部判据（成功行、BUNDLE_VERSION 是本提交、unit active、六模块）。
install_bundle() {
  local out commit
  commit=$(git rev-parse --short=12 HEAD)
  if ! out=$(as_root bash tools/deploy/install-bundle.sh "$WORK/sim.tar.gz" 2>&1); then
    printf '%s\n' "$out" >&2
    die "install-bundle 正例失败（失败现场见上）"
  fi
  printf '%s\n' "$out"
  grep -q '^PASS  新树已就位' <<<"$out" || die "install-bundle 未打印成功行"
  grep -qx "commit=$commit" "$ROOT/BUNDLE_VERSION" \
    || die "换树后 BUNDLE_VERSION 不是本提交（${commit}）"
  as_root systemctl is-active --quiet rrd || die "换树后 rrd 未 active"
  wait_modules
  echo "::notice title=install-smoke-bundle::换树成功 commit=${commit}，六模块 running"
}

# 回滚例：坏 rrd（非 ELF）能通过解包前检查，但起不来——install-bundle 必须回滚。
# 判据：非 0 退出 + 点名回滚 + 装机树仍是上一版 + rrd 恢复 running。
rollback_case() {
  local bad="$WORK/bad.tar.gz" tmp top out rc=0 commit
  commit=$(git rev-parse --short=12 HEAD)
  tmp=$(mktemp -d "$WORK/bad.XXXXXX")
  tar -xzf "$WORK/sim.tar.gz" -C "$tmp"
  top=$(basename "$(find "$tmp" -mindepth 1 -maxdepth 1 -type d)")
  printf 'not-an-elf\n' >"$tmp/$top/bin/rrd"
  tar -C "$tmp" -czf "$bad" "$top"
  out=$(as_root bash tools/deploy/install-bundle.sh "$bad" 2>&1) || rc=$?
  if [ "$rc" -eq 0 ]; then
    printf '%s\n' "$out" >&2
    die "坏 rrd 的包竟然安装成功（回滚路径未触发）"
  fi
  if ! grep -q '已回滚到上一版' <<<"$out"; then
    printf '%s\n' "$out" >&2
    die "失败信息未点名回滚（rc=${rc}）"
  fi
  grep -qx "commit=$commit" "$ROOT/BUNDLE_VERSION" \
    || die "回滚后装机树不是上一版（BUNDLE_VERSION 漂移）"
  as_root systemctl is-active --quiet rrd || die "回滚后 rrd 未 active"
  wait_modules
  echo "     回滚证据：rc=${rc}，$(grep '已回滚' <<<"$out" | tail -1)"
  echo "::notice title=install-smoke-rollback::坏包回滚成立 rc=${rc}（树与二进制已回上一版，六模块 running）"
}

main() {
  preflight
  mkdir -p "$WORK"
  build_all
  make_payload
  install_deb
  sim_deploy
  start_unit
  make_bundle
  install_bundle
  rollback_case
  echo "PASS  装机器正例：deb→systemd 六模块 running；bundle 换树成功；坏包回滚成功"
}

main "$@"
