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
# 没有硬件面。装配单是部署数据，本脚本只在 /etc 铺一份 sim 部署副本、以 unit
# drop-in 把 --profile 指过去（与 distro-smoke ⑥、tools/sim::assemble 同一纪律）；
# 仓源与 bundle 逐字节不动，换树后才能用 verify-board-config 以**真包**判装机面
# 一致。探测档与云端同由 drop-in 显式注入：RR3_PROBE=sim、RR3_BRAIN_CLOUD=mock。
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
# 模块清单取仓内真值（tools/ci/lib/runtime-members.sh）；空清单即失败，不跑空。
. tools/ci/lib/runtime-members.sh
MODULES=$(rr3_modules .)
UNITS=$(rr3_module_bins . | tr '\n' ' ')

# 时延观测（批221）与上限（批265）：三段各记一次并经 notice 公开回传。
# 四拍基线（notice 可回读）：rollback 4359/4374/4453/4513ms、swap 2319-2531ms、
# boot_systemd 1651-2137ms。上限取约 3-5 倍余量，只拦回涨——旧回滚 32619ms 会被
# 15000ms 直接拦下；正常抖动不触发。
BOOT_CEIL_MS=10000
SWAP_CEIL_MS=10000
ROLLBACK_CEIL_MS=15000
BOOT_MS=""
SWAP_MS=""
ROLLBACK_MS=""

die() {
  echo "FAIL  $*" >&2
  trap - ERR
  dump_state
  exit 1
}

# root 直跑或免密 sudo 二选一：CI runner 是非 root + sudo，root 面（容器/真板）直接执行。
as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo -n "$@"; fi
}

# 时延上限判据（批265）：超限即红并打印实测/上限——回归必须能定位到具体段。
tt_ceiling() {
  local name=$1 ms=$2 ceil=$3
  if [ "$ms" -le "$ceil" ]; then
    return 0
  fi
  die "${name} ${ms}ms 超过上限 ${ceil}ms（流畅度回归；基线见 status 部署面时延四拍）"
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
# 意外退出也必须可归因：set -e 下的裸命令失败不会经过 die，公开注解只会剩一句
# "exit code 1"。ERR trap 先打一行 FAIL（带失败命令），再 dump 现场。
trap 'echo "FAIL  意外退出：${BASH_COMMAND}" >&2; dump_state' ERR

# systemd 存在性门：只看"是否有 system manager 可管理 unit"，不看全局收敛态。
# hosted runner 的 VM 在 cloud-init 收尾前 is-system-running 会长时间停在
# "starting"——那是全局目标未收敛，不是 systemctl 不能用（2026-11-27 批217 三次
# 实跑：同一提交一次 running、两次 starting；按全局态判会把可用宿主判死）。
# 真正的功能判据是后面的 systemctl start：起不来时 journal 带根因。
systemd_gate() {
  local st
  [ -d /run/systemd/system ] \
    || die "本机无运行中的 systemd（/run/systemd/system 缺席）：本检查只跑在带 systemd 的宿主/runner"
  command -v systemctl >/dev/null || die "缺 systemctl（systemd 宿主才有 unit 面）"
  st=$(systemctl is-system-running 2>&1 || true)
  case "$st" in
    offline | unknown | "")
      die "systemd 未在管理本机（is-system-running=${st:-空}）"
      ;;
  esac
  echo "systemd: ${st}（unit 管理可用；全局收敛态不作门槛）"
}

# 宿主前置：systemd 可管理 unit + 打包器 + 构建面 + 提权面。
preflight() {
  systemd_gate
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

# 部署环境注入（仓源与 bundle 都不改）：在 /etc 铺一份与装机同形的 sim 部署副本
# （profiles/<档>.toml + hardware/<档>/wiring.toml——rrd 按装配单两级父目录推导
# hardware-root，副本必须同形）；unit 用 drop-in 重写 ExecStart 的 --profile 指向
# 这份副本，其余命令行逐字取自已装 unit（unit 改形时自动跟随）。探测档同由
# drop-in 注入。
sim_deploy() {
  local src="$ROOT/profiles/distro.toml" etc=/etc/rockrobot3
  local drop=/etc/systemd/system/rrd.service.d line override
  grep -q '^hal_backend = "linux"' "$src" \
    || die "装机装配单缺 hal_backend=linux（deb 面漂移）"
  as_root mkdir -p "$etc/profiles" "$etc/hardware/distro" "$drop"
  as_root install -m644 "$src" "$etc/profiles/distro.toml"
  as_root install -m644 "$ROOT/hardware/distro/wiring.toml" \
    "$etc/hardware/distro/wiring.toml"
  # modules-root 与 contracts 直接链到装机树：rrd 由装配单两级父目录推导
  # modules-root（profile_root），副本根必须能看见模块面；链而不是拷，换树后
  # 自动跟到新树，且不在 /etc 复制第二份二进制。先删再链：重跑时不把链建到旧链里。
  as_root rm -rf "$etc/modules" "$etc/contracts"
  as_root ln -s "$ROOT/modules" "$etc/modules"
  as_root ln -s "$ROOT/contracts" "$etc/contracts"
  as_root sed -i 's/^hal_backend = "linux"/hal_backend = "sim"/' \
    "$etc/profiles/distro.toml"
  grep -q '^hal_backend = "sim"' "$etc/profiles/distro.toml" \
    || die "部署副本 profiles/distro.toml 的 sim 变换未生效"
  line=$(systemctl cat rrd | sed -n 's/^ExecStart=//p' | tail -1)
  override=$(sed "s|--profile [^ ]*|--profile $etc/profiles/distro.toml|" <<<"$line")
  grep -q -- "--profile $etc/profiles/distro.toml" <<<"$override" \
    || die "unit ExecStart 无 --profile 装机位（对表 base/os/distro/units/rrd.service）"
  printf '[Service]\nEnvironment=RR3_PROBE=sim\nEnvironment=RR3_BRAIN_CLOUD=mock\nExecStart=\nExecStart=%s\n' \
    "$override" | as_root tee "$drop/50-ci-sim.conf" >/dev/null
  as_root systemctl daemon-reload
}

# 毫秒时钟（GNU date；本脚本只在 Linux systemd 宿主跑，preflight 已把容器面挡住）。
now_ms() { date +%s%3N; }

# 三段时延回传：unit 拉起（boot）、换树窗口（停 rrd→换树→起→六模块）、回滚恢复。
# 写入 step summary（公开面）并发一条 notice，便于匿名回读与发版回执引用。
report_timing() {
  local line="boot_systemd_ms=${BOOT_MS} bundle_swap_ms=${SWAP_MS} rollback_ms=${ROLLBACK_MS}"
  echo "timing ${line}"
  echo "::notice title=install-smoke-timing::${line}（unit 拉起 / 换树停机+启动 / 坏包回滚恢复）"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      echo ""
      echo "### install-smoke timing"
      echo '```'
      echo "$line"
      echo '```'
    } >>"$GITHUB_STEP_SUMMARY"
  fi
}

# rrd 拒 boot 的根因行（journal 里最后几条带错误词的）：die 文案因此带上现场，
# 公开注解与本地输出都能直接看到"为什么没起来"，不用登 runner 翻 journal。
unit_reason() {
  as_root journalctl -u rrd --no-pager -n 40 2>/dev/null \
    | grep -E '错误|失败|拒绝|拒 boot|缺件|无法' | tail -3 | tr '\n' ' '
}

# install-bundle 成功行自带的四段用时（批226）。抓到就并进公开 notice：
# 回滚时长到底是探针窗口还是别处，公开面直接可判，不靠登 runner 翻日志。
# 批255 起坏包由 NRestarts 增量提前判死（不再空等满 30s 窗口）：回滚用时按实测对表。
install_phases() {
  grep -o '停机 [0-9]*ms / 换树 [0-9]*ms / 二进制 [0-9]*ms / 启动探针 [0-9]*ms' \
    <<<"$1" | tail -1
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
  die "30s 内全模块未全 running（${UNITS}）：$(unit_reason)"
}

# unit 拉起 + 六模块判据（正例第一段：deb 装机面由 systemd 真拉起）。
start_unit() {
  local t0
  t0=$(now_ms)
  as_root systemctl start rrd || die "systemctl start rrd 失败：$(unit_reason)"
  as_root systemctl is-active --quiet rrd || die "rrd 未 active：$(unit_reason)"
  wait_modules
  BOOT_MS=$(( $(now_ms) - t0 ))
  tt_ceiling boot_systemd "$BOOT_MS" "$BOOT_CEIL_MS"
  echo "     unit active，全模块 running（boot ${BOOT_MS}ms / 上限 ${BOOT_CEIL_MS}ms）"
  echo "::notice title=install-smoke-systemd::deb 装机 + systemd unit 拉起成立，全模块 running（boot=${BOOT_MS}ms/上限 ${BOOT_CEIL_MS}ms）"
}

# 版本化 bundle：真出包（仓内 profile 原样 linux，不做任何改写）+ A10 契约复核。
# 部署面差异全在 /etc 与 unit drop-in，换树载荷必须是发布面那一份。
make_bundle() {
  bash tools/deploy/pack-bundle.sh "$TRIPLE" "$WORK/bundle.tar.gz"
  bash tools/ci/platform/bundle-contract.sh "$WORK/bundle.tar.gz" "$TRIPLE" "$ARCH"
  echo "     $WORK/bundle.tar.gz（真包：仓内 profile/契约/模块逐字节原样）"
}

# 正例第二段：真 unit 下换树。install-bundle 自己会停 rrd、换树、起 rrd、逐模块
# 探针；这里只补外部判据（成功行、BUNDLE_VERSION 是本提交、unit active、六模块）。
install_bundle() {
  local out commit t0 phases
  commit=$(git rev-parse --short=12 HEAD)
  t0=$(now_ms)
  if ! out=$(as_root bash tools/deploy/install-bundle.sh "$WORK/bundle.tar.gz" 2>&1); then
    printf '%s\n' "$out" >&2
    die "install-bundle 正例失败（失败现场见上）"
  fi
  printf '%s\n' "$out"
  grep -q '^PASS  新树已就位' <<<"$out" || die "install-bundle 未打印成功行"
  grep -qx "commit=$commit" "$ROOT/BUNDLE_VERSION" \
    || die "换树后 BUNDLE_VERSION 不是本提交（${commit}）"
  as_root systemctl is-active --quiet rrd || die "换树后 rrd 未 active"
  wait_modules
  SWAP_MS=$(( $(now_ms) - t0 ))
  tt_ceiling bundle_swap "$SWAP_MS" "$SWAP_CEIL_MS"
  phases=$(install_phases "$out" || true)
  echo "::notice title=install-smoke-bundle::换树成功 commit=${commit}，全模块 running（窗口=${SWAP_MS}ms/上限 ${SWAP_CEIL_MS}ms${phases:+；$phases}）"
}

# 装机面漂移门的单次调用（正例与负例共用同一条命令，避免两处各写一套参数）。
drift_probe() {
  bash tools/deploy/verify-board-config.sh --repo "$PWD" --local "$ROOT" \
    --unit-dir /usr/lib/systemd/system --profile distro
}

# 装机面漂移门正例：真包换树后，仓与装机应逐字节相等。判据体复用
# tools/deploy/verify-board-config.sh（此前只有 20 条自检与真板远端，没有一次
# "刚装完的机器"本地正例）；这里只决定跑法与断言，不复制比对逻辑。
verify_install() {
  local out
  out=$(drift_probe 2>&1) \
    || { printf '%s\n' "$out" >&2; die "装机面漂移门判红（真包换树后不应有漂移/缺件）"; }
  printf '%s\n' "$out" | tail -3
  grep -q '结论：装机面.*与仓内一致' <<<"$out" \
    || die "装机面漂移门未给出「与仓内一致」结论"
  echo "::notice title=install-smoke-drift::真包换树后装机面漂移门判一致（仓/装机逐字节）"
}

# 漂移门负例（批222）：在真装机树上改一件 config，门必须判 DRIFT 并点名；还原后
# 必须重回一致。正例只证明"没漂时不误报"，负例才证明"漂了看得见"——两者合起来
# 才是这台门在真装机面上的完整回执。
drift_negative() {
  local target="$ROOT/modules/rr3-sense/config.toml" backup="$WORK/config.toml.bak"
  local out rc=0
  cp "$target" "$backup"
  printf '\n# ci drift probe\n' | as_root tee -a "$target" >/dev/null
  out=$(drift_probe 2>&1) || rc=$?
  as_root install -m644 "$backup" "$target"
  [ "$rc" -eq 1 ] \
    || die "改一件 config 后漂移门未判红（rc=${rc}）：$(tail -2 <<<"$out" | tr '\n' ' ')"
  grep -q '^DRIFT modules/rr3-sense/config.toml' <<<"$out" \
    || die "漂移门未点名被改的 config（DRIFT 行缺席）"
  drift_probe >/dev/null || die "还原 config 后漂移门仍判红"
  echo "::notice title=install-smoke-drift-neg::改一件 config 后漂移门判 DRIFT 并点名，还原后重回一致"
}

# 回滚例：坏 rrd（非 ELF）能通过解包前检查，但起不来——install-bundle 必须回滚。
# 判据：非 0 退出 + 点名回滚 + 装机树仍是上一版 + rrd 恢复 running。
rollback_case() {
  local bad="$WORK/bad.tar.gz" tmp top out rc=0 commit t0 attempt
  commit=$(git rev-parse --short=12 HEAD)
  tmp=$(mktemp -d "$WORK/bad.XXXXXX")
  tar -xzf "$WORK/bundle.tar.gz" -C "$tmp"
  top=$(basename "$(find "$tmp" -mindepth 1 -maxdepth 1 -type d)")
  printf 'not-an-elf\n' >"$tmp/$top/bin/rrd"
  tar -C "$tmp" -czf "$bad" "$top"
  t0=$(now_ms)
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
  ROLLBACK_MS=$(( $(now_ms) - t0 ))
  tt_ceiling rollback "$ROLLBACK_MS" "$ROLLBACK_CEIL_MS"
  attempt=$(grep -o '本次尝试 [0-9]*ms' <<<"$out" | tail -1 || true)
  echo "     回滚证据：rc=${rc}，$(grep '已回滚' <<<"$out" | tail -1)"
  echo "::notice title=install-smoke-rollback::坏包回滚成立 rc=${rc}（树与二进制已回上一版，全模块 running，恢复=${ROLLBACK_MS}ms/上限 ${ROLLBACK_CEIL_MS}ms${attempt:+；$attempt}）"
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
  verify_install
  drift_negative
  rollback_case
  report_timing
  echo "PASS  装机器正例：deb→systemd 全模块 running；真包换树成功且装机面无漂移；坏包回滚成功"
}

main "$@"
