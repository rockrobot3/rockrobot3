#!/bin/bash
# buildroot-assembly-smoke.sh —— 保底轨装配冒烟（本机可跑，无 Docker / 无 Buildroot）。
# 构造最小 TARGET_DIR / BINARIES_DIR，以仓内真实 manifest 与 config 为源跑 post-build.sh，
# 断言 0.0.2 保底轨修复的四点：manifest 归位 modules/<name>/module.toml、entry 装机绝对化
# （除 entry 行外字节零差异）、bin 按 entry 落位、config 从仓源随装；另断言坏模块
# （缺 bin / 无 entry 行）只警告不阻断、旧平铺安装位不再存在、S99rrd glob 跟上。
set -eu -o pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
POST_BUILD="$REPO_ROOT/base/os/buildroot/board/radxa-zero3/post-build.sh"

work="$(mktemp -d /tmp/rr3-assembly-smoke.XXXXXX)"
trap 'rm -rf "$work"' EXIT
TARGET="$work/target"
BINARIES="$work/binaries"
mkdir -p "$TARGET/etc/init.d" "$BINARIES/rr3-modules/rr3-sense" "$BINARIES/rr3-modules/rr3-ghost" \
	"$BINARIES/rr3-modules/rr3-noentry"

# 好模块：仓内 rr3-sense 真 manifest + 真 config 源 + 假 bin
cp "$REPO_ROOT/modules/rr3-sense/module.toml" "$BINARIES/rr3-modules/rr3-sense/manifest.toml"
printf '#!/bin/sh\nexit 0\n' > "$BINARIES/rr3-modules/rr3-sense/bin"
chmod +x "$BINARIES/rr3-modules/rr3-sense/bin"
# 坏模块一：有 manifest 无 bin（警告 + 整模块跳过）
cp "$REPO_ROOT/modules/rr3-sense/module.toml" "$BINARIES/rr3-modules/rr3-ghost/manifest.toml"
# 坏模块二：manifest 无 entry 行（警告 + 整模块跳过）
printf '[manifest]\nname = "rr3-noentry"\n' > "$BINARIES/rr3-modules/rr3-noentry/manifest.toml"
printf '#!/bin/sh\n' > "$BINARIES/rr3-modules/rr3-noentry/bin"
chmod +x "$BINARIES/rr3-modules/rr3-noentry/bin"

# 不传 RR3_REPO_ROOT：顺带验证 post-build.sh 的仓根五级上溯缺省路径
out="$work/out.log"
TARGET_DIR="$TARGET" BINARIES_DIR="$BINARIES" bash "$POST_BUILD" 2>&1 | tee "$out"

fail=0
ok() { echo "  ✔ $1"; }
bad() { echo "  ✘ $1"; fail=1; }

echo "断言：装机布局"
[ -f "$TARGET/usr/lib/rr3/modules/rr3-sense/module.toml" ] &&
	ok "manifest 归位 modules/rr3-sense/module.toml" || bad "module.toml 未归位"
[ ! -e "$TARGET/usr/lib/rr3/manifest" ] &&
	ok "旧平铺安装位 /usr/lib/rr3/manifest 不存在" || bad "/usr/lib/rr3/manifest 仍存在"
[ -d "$TARGET/usr/bin" ] && [ -d "$TARGET/var/lib/rr3/staging" ] &&
	[ -d "$TARGET/data" ] && [ -d "$TARGET/extlinux" ] &&
	ok "目录骨架（usr/bin、staging、data、extlinux）" || bad "目录骨架缺失"

echo "断言：entry 绝对化（除 entry 行外字节零差异）"
src="$REPO_ROOT/modules/rr3-sense/module.toml"
dst="$TARGET/usr/lib/rr3/modules/rr3-sense/module.toml"
entry_rel="$(sed -n 's/^entry[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$src" | head -n 1)"
want="/usr/lib/rr3/modules/rr3-sense/$entry_rel"
if grep -q "^entry[[:space:]]*=[[:space:]]*\"$want\"$" "$dst"; then
	ok "entry 已改写为装机绝对路径 $want"
else
	bad "entry 未绝对化（期望 $want）"
fi
if diff <(grep -v '^entry[[:space:]]*=' "$src") \
	<(grep -v '^entry[[:space:]]*=' "$dst") >/dev/null; then
	ok "除 entry 行外与仓源逐字节一致"
else
	bad "entry 以外的内容被改动"
fi
[ -x "$TARGET/usr/lib/rr3/modules/rr3-sense/$entry_rel" ] &&
	ok "bin 按 entry 落位且可执行" || bad "bin 未落 entry 名"

echo "断言：config 随装"
if [ -f "$TARGET/usr/lib/rr3/modules/rr3-sense/config.toml" ] &&
	diff "$REPO_ROOT/modules/rr3-sense/config.toml" \
		"$TARGET/usr/lib/rr3/modules/rr3-sense/config.toml" >/dev/null; then
	ok "config 与仓源 modules/rr3-sense/config.toml 一致"
else
	bad "config 未随装或与仓源不一致"
fi

echo "断言：S99rrd"
[ -x "$TARGET/etc/init.d/S99rrd" ] &&
	grep -q 'for m in /usr/lib/rr3/modules/\*/module.toml' "$TARGET/etc/init.d/S99rrd" &&
	ok "S99rrd 按 modules/*/module.toml 收 manifest" || bad "S99rrd glob 未跟上"
[ -f "$TARGET/etc/fstab" ] && ok "fstab 落位" || bad "fstab 缺失"

echo "断言：坏模块只警告不阻断"
grep -q "rr3-ghost 缺 bin" "$out" && [ ! -e "$TARGET/usr/lib/rr3/modules/rr3-ghost" ] &&
	ok "缺 bin 模块：警告且整模块未装" || bad "缺 bin 模块处置不对"
grep -q "rr3-noentry 的 manifest 有 0 条 entry 行" "$out" &&
	[ ! -e "$TARGET/usr/lib/rr3/modules/rr3-noentry" ] &&
	ok "无 entry 行模块：警告且整模块未装" || bad "无 entry 行模块处置不对"

if [ "$fail" -ne 0 ]; then
	echo "装配冒烟：红（见上 ✘）"
	exit 1
fi
echo "装配冒烟：全绿"
