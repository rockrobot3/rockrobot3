#!/usr/bin/env bash
# 版本化部署包的原子安装 / 失败回滚（A10.2）。
#
# 用法：sudo bash tools/deploy/install-bundle.sh <bundle.tar.gz> [安装根]
#   安装根缺省 /usr/lib/rockrobot3，二进制位缺省 /usr/bin（RR3_INSTALL_BIN 覆盖）。
#
# 为什么不是"解包覆盖"：2026-11-08 真板实录是 HEAD rrd + 旧 profile/旧模块的
# 半更新态（装载拒启 + 描述符键不认 → 模块崩溃循环）。本脚本把"整树换位"
# 做成一次 rename：同文件系统内 old→prev、stage→root 两步，窗口只在 rrd 停机
# 期间；任何一步失败都按同一路径回退，不留下半套装机树。
#
# 保留非本包内容：装机树里的 rr3-brain 等未进包的模块以现树为底叠加（stage
# 先整树复制再解包覆盖），换树不会把未打包模块连带删掉。
set -euo pipefail

BUNDLE=${1:-}
ROOT=${2:-/usr/lib/rockrobot3}
BIN=${RR3_INSTALL_BIN:-/usr/bin}

# 从 rrd 单元 ExecStart 取开关值（单元缺失 / 开关未写 = 空）。单元是部署面的
# 单点真值：安装根、profile、socket 都以它为准，脚本不再自持第二份缺省。
# 为什么函数里吞掉 systemctl 的非零：首装时单元可能尚未落地，裸管道在
# pipefail 下会把整条赋值判失败，set -e 当场退出——安装步骤一步都走不到。
unit_arg() {
  { systemctl cat rrd 2>/dev/null || true; } \
    | sed -n "s/^ExecStart=.*$1 \([^ ]*\).*/\1/p" | tail -1
}
SOCK=${RR3_BUS_PATH:-$(unit_arg --socket)}
SOCK=${SOCK:-/var/run/rock3-ipc.sock}

die() { echo "FAIL  $*" >&2; exit 1; }

[ -n "$BUNDLE" ] || die "用法: sudo bash install-bundle.sh <bundle.tar.gz> [安装根]"
[ -f "$BUNDLE" ] || die "部署包不存在: $BUNDLE"
[ "$(id -u)" -eq 0 ] || die "需 root（换装机树 + 重启 rrd）"
command -v systemctl >/dev/null || die "本机无 systemctl，发行版快路轨之外不走本脚本"

# 包顶层目录名 = rockrobot3-<commit>；BUNDLE_VERSION 必须在顶层（A10 打包契约）。
# 清单先落变量再匹配：`tar | grep -q` 的早退会让 tar 吃 SIGPIPE，pipefail 下整
# 脚本会被 141 静默带走（2026-11-09 真板实测）。
list=$(tar -tzf "$BUNDLE")
top=$(printf '%s\n' "$list" | sed -n '1s|/.*||p')
[ -n "$top" ] || die "部署包为空: $BUNDLE"
grep -qx "$top/BUNDLE_VERSION" <<<"$list" \
  || die "部署包缺 $top/BUNDLE_VERSION（非 A10 版本化包）"
grep -qx "$top/bin/rrd" <<<"$list" || die "部署包缺 bin/rrd"

[ "$ROOT" != "/" ] || die "安装根不得为 /"
parent=$(dirname "$ROOT")
stage=$(mktemp -d "$parent/.rockrobot3-new.XXXXXX")
backup="$parent/.rockrobot3-prev"
binbk="$parent/.rockrobot3-bin-prev"
cleanup() { rm -rf "$stage"; }
trap cleanup EXIT

# 二进制另存：旧装机树可能不带 bin/（历史装机只留 /usr/bin），回滚不能指望
# 树里有旧二进制——2026-11-09 真板实测：回滚换回旧树但 /usr/bin/rrd 仍是新版，
# 新 rrd + 旧 profile 直接崩溃循环。备份与树备份同生命周期。
rm -rf "$binbk"
mkdir -p "$binbk"
for b in rrd rr3-ctl; do
  [ -f "$BIN/$b" ] && cp -a "$BIN/$b" "$binbk/$b"
done

# 以现树为底（保留 brain 等未进包模块），再把本包覆盖上去
if [ -d "$ROOT" ]; then
  cp -a "$ROOT/." "$stage/"
else
  mkdir -p "$stage"
fi
tar -xzf "$BUNDLE" -C "$stage" --strip-components=1
for f in bin/rrd bin/rr3-ctl contracts/topics.toml; do
  [ -f "$stage/$f" ] || die "解包后缺 $f（包内容与 A10 契约不符）"
done

# 期望模块集：从 rrd 单元命令行取 --profile，读其 [enabled_slots] 键（模块名）
profile=$(unit_arg --profile)
expected=""
if [ -n "$profile" ] && [ -f "$profile" ]; then
  expected=$(awk '/^\[enabled_slots\]/{f=1;next} /^\[/{f=0} f && /^[A-Za-z0-9_-]+ *=/ {sub(/ *=.*/, ""); print}' "$profile")
fi

rollback() {
  echo "FAIL  新树启动验证未过，回滚" >&2
  systemctl stop rrd >/dev/null 2>&1 || true
  rm -f "$BIN/.rrd.new" "$BIN/.rr3-ctl.new"
  rm -rf "$ROOT"
  if [ -d "$backup" ]; then
    mv "$backup" "$ROOT"
    for b in rrd rr3-ctl; do
      if [ -f "$binbk/$b" ]; then
        install -m755 "$binbk/$b" "$BIN/$b"
      elif [ -f "$ROOT/bin/$b" ]; then
        install -m755 "$ROOT/bin/$b" "$BIN/$b"
      fi
    done
    systemctl reset-failed rrd >/dev/null 2>&1 || true
    systemctl start rrd >/dev/null 2>&1 || true
    die "已回滚到上一版（该树在 $ROOT，二进制备份在 $binbk）"
  fi
  die "首次安装失败且无上一版可回退（新树已删除，请查 journalctl -u rrd）"
}

echo "安装 $top → $ROOT（rrd 停机窗口内整树换位）"
systemctl stop rrd
# Restart=always 会在超时被 SIGKILL 后自动拉起；停稳确认必须在换树之前，
# 否则"新树已就位、旧进程还在跑"的假窗口会把健康探针引到旧进程上。
for _ in $(seq 1 40); do
  systemctl is-active --quiet rrd || break
  sleep 1
done
systemctl is-active --quiet rrd && die "rrd 40s 内未停稳，放弃安装（机树未改动）"
# mktemp 建的是 0700：换位后整棵装机树会继承该权限，非 root 工具连
# profiles/contracts 都读不到。换位前显式归一为 0755（安装根是共享只读面）。
chmod 0755 "$stage"
if [ -d "$ROOT" ]; then
  rm -rf "$backup"
  mv "$ROOT" "$backup"
fi
if ! mv "$stage" "$ROOT"; then
  # 第二次换位失败：立即把旧树放回，避免"有备份但无装机树"的裸奔窗口
  if [ -d "$backup" ]; then
    mv "$backup" "$ROOT"
  fi
  die "新树换位失败（已恢复上一版树；/usr/bin 二进制未改动）"
fi
trap - EXIT

# 二进制替换同样在回滚保护内：这里失败若直接退出，会留下"新树 + 旧 rrd"
# 的半更新态，正是本脚本要消灭的故障面。
for b in rrd rr3-ctl; do
  install -m755 "$ROOT/bin/$b" "$BIN/.$b.new" || rollback
  mv -f "$BIN/.$b.new" "$BIN/$b" || rollback
done
# 先前的崩溃循环会把 StartLimit 打满（Failed to start: start request repeated
# too quickly）；不清账的话新树第一次 start 就被拒，健康窗口还没开始就回滚。
systemctl reset-failed rrd >/dev/null 2>&1 || true
systemctl start rrd || rollback

# 健康探针：30s 内 rrd 应答 status，且 enabled_slots 的每个模块都 running
ok=0
for _ in $(seq 1 30); do
  if out=$("$BIN/rr3-ctl" --bus-path "$SOCK" status 2>/dev/null); then
    ok=1
    for m in $expected; do
      grep -qE "^$m[[:space:]]" <<<"$out" || { ok=0; break; }
    done
    grep -q 'failed' <<<"$out" && ok=0
    [ "$ok" -eq 1 ] && break
  fi
  sleep 1
done
[ "$ok" -eq 1 ] || rollback

echo "PASS  新树已就位：$(sed -n 's/^commit=//p' "$ROOT/BUNDLE_VERSION")"
echo "      模块：$(printf '%s ' $expected)"
echo "      上一版保留在 $backup（人工回滚：停 rrd → 换回该树与 bin/ 二进制）"
