#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
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

# 意外退出必须可归因（批274）：`set -e` 下的裸命令失败不会经过 die/rollback，板侧只
# 看到退出码；先打失败命令与所在行，再让 shell 继续退出（与 install-smoke 批245 同款）。
trap 'echo "FAIL  意外退出：${BASH_COMMAND}（行 ${LINENO}；装机树 ${ROOT}）" >&2' ERR

# 毫秒时钟（GNU date；本脚本只在 Linux systemd 宿主跑）。四段用时随成功/回滚行打印，
# 真板排障先看这一行：慢在停机、换树、二进制还是启动探针，不用猜。
now_ms() { date +%s%3N; }

[ -n "$BUNDLE" ] || die "用法: sudo bash install-bundle.sh <bundle.tar.gz> [安装根]"
[ -f "$BUNDLE" ] || die "部署包不存在: $BUNDLE"
[ "$(id -u)" -eq 0 ] || die "需 root（换装机树 + 重启 rrd）"
command -v systemctl >/dev/null || die "本机无 systemctl，发行版快路轨之外不走本脚本"
# base 层前置（2026-11-27 批161/164）：bundle 不含也不装 unit，而停机窗口、profile
# 真值与健康探针全部锚在已装 unit 上——unit 缺失时下面的 systemctl stop 在 set -e 下
# 以 "Unit rrd.service not loaded" 中止，既走不到换树，也说不清缺的是哪一层。故在
# 动树之前 fail-closed，把"先装 deb"这一步点名到原因（批161 的 WARN 续行已被此取代：
# 它承诺的"继续"从未成立）。
if [ -z "$(unit_arg --socket)" ] && [ -z "$(unit_arg --profile)" ]; then
  die "未找到已装 rrd.service（bundle 不装 unit）——先按 README 装机节装 deb 铺 base 层，再换树"
fi

# 包顶层目录名 = rockrobot3-<commit>；BUNDLE_VERSION 必须在顶层（A10 打包契约）。
# 清单先落变量再匹配：`tar | grep -q` 的早退会让 tar 吃 SIGPIPE，pipefail 下整
# 脚本会被 141 静默带走（2026-11-09 真板实测）。
list=$(tar -tzf "$BUNDLE")
top=$(printf '%s\n' "$list" | sed -n '1s|/.*||p')
[ -n "$top" ] || die "部署包为空: $BUNDLE"
grep -qx "$top/BUNDLE_VERSION" <<<"$list" \
  || die "部署包缺 $top/BUNDLE_VERSION（非 A10 版本化包）"
grep -qx "$top/bin/rrd" <<<"$list" || die "部署包缺 bin/rrd"

# 包内模块结构预检（批309）：缺 bin/manifest 的包在停 rrd **之前**拒掉——坏包不该
# 换来整机停机与一次回滚，也不该"换上去才发现少件"。
bmods=$(printf '%s\n' "$list" | sed -n "s#^$top/modules/\(rr3-[A-Za-z0-9_-]*\)/.*#\1#p" | sort -u)
[ -n "$bmods" ] || die "包内没有任何模块（modules/* 为空，拒绝换树）"
for m in $bmods; do
  grep -qx "$top/modules/$m/module.toml" <<<"$list" \
    || die "包内缺 modules/$m/module.toml（模块结构不完整，未停 rrd）"
  grep -qx "$top/modules/$m/$m" <<<"$list" \
    || die "包内缺 modules/$m/$m（模块二进制缺失，未停 rrd）"
done

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
for b in rrd rr3-ctl rr3-ota-ctl; do
  [ -f "$BIN/$b" ] && cp -a "$BIN/$b" "$binbk/$b"
done

# 以现树为底（保留 brain 等未进包模块），再把本包覆盖上去
if [ -d "$ROOT" ]; then
  cp -a "$ROOT/." "$stage/"
else
  mkdir -p "$stage"
fi
tar -xzf "$BUNDLE" -C "$stage" --strip-components=1
for f in bin/rrd bin/rr3-ctl bin/rr3-ota-ctl contracts/topics.toml; do
  [ -f "$stage/$f" ] || die "解包后缺 ${f}（包内容与 A10 契约不符）"
done

# 期望模块集：从 rrd 单元命令行取 --profile，读其 [enabled_slots] 键（模块名）
profile=$(unit_arg --profile)
expected=""
if [ -n "$profile" ] && [ -f "$profile" ]; then
  expected=$(awk '/^\[enabled_slots\]/{f=1;next} /^\[/{f=0} f && /^[A-Za-z0-9_-]+ *=/ {sub(/ *=.*/, ""); print}' "$profile")
fi

# 必需件预检（批311）：与解包后断言同源，但前移到停 rrd 之前——缺件不该换来停机。
for f in bin/rrd bin/rr3-ctl bin/rr3-ota-ctl contracts/topics.toml contracts/slots.toml; do
  grep -qx "$top/$f" <<<"$list" || die "包内缺 ${f}（A10 必需件，未停 rrd）"
done
if [ -n "$profile" ]; then
  grep -qx "$top/profiles/${profile##*/}" <<<"$list" \
    || die "包内缺 profiles/${profile##*/}（unit 的 --profile 不在包内，未停 rrd）"
fi

# 回滚带归因（批270）：板侧运维通常只读最后一行"已回滚"，不带原因就还得翻整段日志。
rollback() {
  local why="${1:-未知原因}"
  echo "FAIL  新树启动验证未过（${why}），回滚" >&2
  systemctl stop rrd >/dev/null 2>&1 || true
  rm -f "$BIN/.rrd.new" "$BIN/.rr3-ctl.new" "$BIN/.rr3-ota-ctl.new"
  rm -rf "$ROOT"
  if [ -d "$backup" ]; then
    mv "$backup" "$ROOT"
    for b in rrd rr3-ctl rr3-ota-ctl; do
      if [ -f "$binbk/$b" ]; then
        install -m755 "$binbk/$b" "$BIN/$b"
      elif [ -f "$ROOT/bin/$b" ]; then
        install -m755 "$ROOT/bin/$b" "$BIN/$b"
      fi
    done
    systemctl reset-failed rrd >/dev/null 2>&1 || true
    systemctl start rrd >/dev/null 2>&1 || true
    # 回滚后确认旧版真的在跑（批316）：树换回来不等于服务恢复——旧树也可能因部署面
    # 变化起不来；只报"已回滚"会让板侧误以为服务在跑。3s 内未 active 就把这半句点名。
    live=0
    for _ in $(seq 1 30); do
      systemctl is-active --quiet rrd && { live=1; break; }
      sleep 0.1
    done
    if [ "$live" -eq 1 ]; then
      die "已回滚到上一版（原因：${why}；rrd 已重新 active；该树在 ${ROOT}，二进制备份在 ${binbk}；本次尝试 $(( $(now_ms) - T_STOP ))ms）"
    fi
    die "已回滚到上一版（原因：${why}；但 rrd 未恢复 active——旧树也未起来，查 journalctl -u rrd；该树在 ${ROOT}，二进制备份在 ${binbk}；本次尝试 $(( $(now_ms) - T_STOP ))ms）"
  fi
  die "首次安装失败且无上一版可回退（原因：${why}；新树已删除，本次尝试 $(( $(now_ms) - T_STOP ))ms，请查 journalctl -u rrd）"
}

echo "安装 $top → ${ROOT}（rrd 停机窗口内整树换位）"
T_STOP=$(now_ms)
systemctl stop rrd
# Restart=always 会在超时被 SIGKILL 后自动拉起；停稳确认必须在换树之前，
# 否则"新树已就位、旧进程还在跑"的假窗口会把健康探针引到旧进程上。
for _ in $(seq 1 40); do
  systemctl is-active --quiet rrd || break
  sleep 1
done
systemctl is-active --quiet rrd && die "rrd 40s 内未停稳，放弃安装（机树未改动）"
STOP_MS=$(( $(now_ms) - T_STOP ))
# mktemp 建的是 0700：换位后整棵装机树会继承该权限，非 root 工具连
# profiles/contracts 都读不到。换位前显式归一为 0755（安装根是共享只读面）。
chmod 0755 "$stage"
T_SWAP=$(now_ms)
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
SWAP_MS=$(( $(now_ms) - T_SWAP ))

# 二进制替换同样在回滚保护内：这里失败若直接退出，会留下"新树 + 旧 rrd"
# 的半更新态，正是本脚本要消灭的故障面。
T_BIN=$(now_ms)
for b in rrd rr3-ctl rr3-ota-ctl; do
  install -m755 "$ROOT/bin/$b" "$BIN/.$b.new" || rollback "二进制 ${b} 安装失败"
  mv -f "$BIN/.$b.new" "$BIN/$b" || rollback "二进制 ${b} 换位失败"
done
BIN_MS=$(( $(now_ms) - T_BIN ))
# 先前的崩溃循环会把 StartLimit 打满（Failed to start: start request repeated
# too quickly）；不清账的话新树第一次 start 就被拒，健康窗口还没开始就回滚。
T_BOOT=$(now_ms)
systemctl reset-failed rrd >/dev/null 2>&1 || true
# 崩溃重启计数基线（批255）：坏 rrd 起不来时 unit 在 Restart=always 下会反复拉起，
# 而 rr3-ctl 永远不应答——旧实现要空等满 30s 才回滚（实测 rollback≈32.6s）。
# 读 NRestarts（reset-failed 后归零）增量：涨了就是新树在崩，立即回滚并点名次数，
# 不再用固定窗口替一个已经确定的结论计时。
base_nr=$(systemctl show -p NRestarts --value rrd 2>/dev/null || echo 0)
base_nr=${base_nr#NRestarts=}
case $base_nr in (''|*[!0-9]*) base_nr=0 ;; esac
systemctl start rrd || rollback "systemctl start rrd 失败"

# 健康探针：30s 内 rrd 应答 status，且 enabled_slots 的每个模块都 running
# （批318：此前只查"名字出现 + 无 failed"，degraded/未起齐也会判过——与注释承诺
# 不一致；未 running 的模块名进回滚原因，板侧不用再翻 status）；
# 期间 NRestarts 增量即崩溃循环，提前结束窗口（省下的 30s 是排障时间的直接收益）。
# 0.2s 粒度（批319）：1s 粒度把就绪时刻向上取整，实测启动探针多等数百 ms；窗口仍
# 30s（150 拍），轮询只读 status，零副作用。
ok=0
crash=""
miss=""
for _ in $(seq 1 150); do
  nr=$(systemctl show -p NRestarts --value rrd 2>/dev/null || echo "$base_nr")
  nr=${nr#NRestarts=}
  case $nr in (''|*[!0-9]*) nr=$base_nr ;; esac
  if [ "$nr" -gt "$base_nr" ]; then
    crash=$((nr - base_nr))
    break
  fi
  if out=$("$BIN/rr3-ctl" --bus-path "$SOCK" status 2>/dev/null); then
    ok=1
    miss=""
    for m in $expected; do
      awk -v m="$m" '$1==m && $3=="running"{f=1} END{exit !f}' <<<"$out" \
        || miss="$miss $m"
    done
    [ -n "$miss" ] && ok=0
    grep -q 'failed' <<<"$out" && ok=0
    [ "$ok" -eq 1 ] && break
  fi
  sleep 0.2
done
if [ -n "$crash" ]; then
  rollback "rrd 崩溃重启 ${crash} 次（启动探针 $(( $(now_ms) - T_BOOT ))ms 内即判）"
fi
[ "$ok" -eq 1 ] || {
  why="30s 内未应答 status 或模块未全 running"
  [ -n "$miss" ] && why="${why}（未 running：${miss# }）"
  rollback "$why"
}
BOOT_MS=$(( $(now_ms) - T_BOOT ))

echo "PASS  新树已就位：$(sed -n 's/^commit=//p' "$ROOT/BUNDLE_VERSION")（停机 ${STOP_MS}ms / 换树 ${SWAP_MS}ms / 二进制 ${BIN_MS}ms / 启动探针 ${BOOT_MS}ms）"
# 诚实降级（2026-11-27 批164）：expected 取不到时旧文案打成"模块："空白，读起来像
# "零模块也验过了"。逐模块断言是本次探针的主体，缺它就必须说清只验到了哪一层。
if [ -n "$expected" ]; then
  echo "      模块：$(printf '%s ' $expected)"
elif [ -n "$profile" ]; then
  echo "      模块：未核对（unit 的 --profile 指向 ${profile}，该文件不存在或无可读槽位）"
else
  echo "      模块：未核对（unit 的 ExecStart 未声明 --profile，探针只验 rrd 应答）"
fi
echo "      上一版保留在 ${backup}（人工回滚：停 rrd → 换回该树与 bin/ 二进制）"
