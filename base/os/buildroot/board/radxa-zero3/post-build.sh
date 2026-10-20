#!/bin/bash
# post-build.sh —— 目标根文件系统装配（Buildroot post-build，宿主侧执行）。
# 运行环境由 Buildroot 提供：$TARGET_DIR（目标根）、$BINARIES_DIR（镜像产物目录）、
# $HOST_DIR（宿主工具）。档位：脚本就绪；实测=板上（串口出 shell + rrd 自启）。
#
# 五件事（对应 base/os/README.md 的交付物清单）：
#   1. rrd 开机自启（BusyBox init 的 /etc/init.d/S99rrd）；
#   2. /var/run tmpfs（rrd 默认 UDS 路径 /var/run/rock3-ipc.sock 落在易失区，
#      避免残留旧 socket 文件跨重启干扰 bind）；
#   3. data / rr_staging 挂载点（分区标签见 genimage.cfg，rr-ota 守护用例钉死）；
#   4. rrd 与模块产物安装位（交叉编译产物经 $BINARIES_DIR 注入，不在 Buildroot 里编译；
#      manifest 归位 /usr/lib/rockrobot3/modules/<name>/module.toml，entry 改写为装机绝对路径，
#      config 从仓源随装——线上注册门禁拒绝相对 entry，模块开机即读同目录 config）；
#   5. 装载消费真值集装机（profiles/contracts/hardware 三目录随装，增补轮六 基-25——
#      装载必需件缺席的洞此前在 buildroot 装机侧完全不可见，块-15）；
#   6. S99rrd 传 --contracts/--profile 启用装载正门（基-23：出厂轨装载门此前不存在——
#      不传 --profile 时 rrd 走 --manifest 后门、十条校验一个判定位都不跑，L-31）。
#
# 安装位与 deb 快路轨同根 /usr/lib/rockrobot3（rrd.service 头注"两轨同一组安装位"
# 纪律，基-23 由假变真）：这不只是口径美观——rrd 装载链由装配单路径上两级推导
# modules-root（profile.rs），S99rrd 传 --profile /usr/lib/rockrobot3/profiles/distro.toml
# 后 manifests 必然落 /usr/lib/rockrobot3/modules/<名>/module.toml，模块装机位不同根
# 即 spawn 必败。数据面 /var/lib/rr3/staging 不随根动（rr_staging 分区挂载点，
# partition-layout.md 钉死，属分区布局不属安装位）。

set -eu
TARGET="${TARGET_DIR:?post-build.sh 需要 TARGET_DIR（Buildroot 提供）}"

# ---- 4. 产物安装位与目录骨架 -------------------------------------------------
install -d -m 0755 \
	"$TARGET/usr/bin" \
	"$TARGET/usr/lib/rockrobot3/modules" \
	"$TARGET/var/lib/rr3/staging" \
	"$TARGET/data" \
	"$TARGET/extlinux"

# rrd 与模块二进制由 aarch64 交叉编译后放到 $BINARIES_DIR（交付方式见
# patches/README.md 的"产物注入"一节）；缺席只警告不阻断——镜像结构
# 仍应可构建，产物齐装是板上/容器侧收口项。
for bin in rrd rr3-ctl; do
	if [ -f "$BINARIES_DIR/$bin" ]; then
		install -m 0755 "$BINARIES_DIR/$bin" "$TARGET/usr/bin/$bin"
	else
		echo "post-build.sh: 警告：$BINARIES_DIR/$bin 缺席，跳过安装（产物注入未完成）" >&2
	fi
done

# 模块注入：$BINARIES_DIR/rr3-modules/<name>/{bin,manifest.toml}（两轨同源注入格式，
# 快路轨 rr3-pack 读同一格式，两轨不互抄装配）。装机布局对齐装配单语义：
#   /usr/lib/rockrobot3/modules/<name>/{module.toml,<entry>,config.toml}
# 三处与旧版不同（0.0.2 保底轨修复，与快路轨 rr3-pack 同一套裁定）：
#   a. manifest 不再平铺 /usr/lib/rockrobot3/manifest/<name>.toml——模块 rr3_config_load 读
#      manifest 同目录 config.toml，平铺下各模块 config 全部撞同一文件名，且 bin 的
#      相对 entry 解析到 manifest 所在目录必错位；
#   b. entry 改写为装机绝对路径（rr3-pack rr3_manifest_install_rewrite 同语义）：
#      模块开机读装机 manifest 原文上报，线上注册门禁拒绝相对值；
#   c. config 源真值从仓 modules/<name>/config.toml 随装（不是构建产物，不进注入格式；
#      仓根定位可用环境变量 RR3_REPO_ROOT 覆盖，缺省按脚本位置上溯五级）。
# 缺件 / entry 不可装机 = 警告 + 整模块跳过，与上面 rrd/rr3-ctl 缺席同口径：
# 镜像结构仍应可构建，产物齐装是板上收口项；但缺件还装 manifest 的话，rrd 会
# spawn 一个必退出的模块（重启环），故缺件时 manifest 一并不装。
REPO_ROOT="${RR3_REPO_ROOT:-}"
if [ -z "$REPO_ROOT" ]; then
	REPO_ROOT="$(cd "$(dirname "$0")/../../../../.." 2>/dev/null && pwd || true)"
fi
if [ -d "$BINARIES_DIR/rr3-modules" ]; then
	for mod_dir in "$BINARIES_DIR"/rr3-modules/*/; do
		name="$(basename "$mod_dir")"
		mod_dst="$TARGET/usr/lib/rockrobot3/modules/$name"
		if [ ! -f "$mod_dir/manifest.toml" ]; then
			echo "post-build.sh: 警告：模块 $name 缺 manifest.toml，整模块跳过" >&2
			continue
		fi
		if [ ! -f "$mod_dir/bin" ]; then
			echo "post-build.sh: 警告：模块 $name 缺 bin，整模块跳过（缺 bin 还装 manifest 必重启环）" >&2
			continue
		fi
		# entry 行必须恰一条（rr3-pack 同门）：多行语义不明，零行不可装配
		entry_n="$(grep -c '^entry[[:space:]]*=' "$mod_dir/manifest.toml" || true)"
		if [ "$entry_n" != "1" ]; then
			echo "post-build.sh: 警告：模块 $name 的 manifest 有 $entry_n 条 entry 行（须恰一条），整模块跳过" >&2
			continue
		fi
		entry_rel="$(sed -n 's/^entry[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$mod_dir/manifest.toml" | head -n 1)"
		if [ -z "$entry_rel" ]; then
			echo "post-build.sh: 警告：模块 $name 的 entry 行不可解析（须双引号值），整模块跳过" >&2
			continue
		fi
		# entry 必须可装机：仓相对、不含 . / .. 段；且不含 sed 分隔符 |（改写安全）
		case "$entry_rel" in
			/*|.|..|./*|../*|*/.|*/..|*/./*|*/../*|*\|*)
				echo "post-build.sh: 警告：模块 $name entry 不可装机（$entry_rel：绝对路径 / . 与 .. 段 / 含 |），整模块跳过" >&2
				continue
				;;
		esac
		entry_dir="$(dirname "$entry_rel")"
		if [ "$entry_dir" = "." ]; then
			install -d "$mod_dst"
		else
			install -d "$mod_dst/$entry_dir"
		fi
		# bin 按入口落位：注入格式的固定名 bin 装到 entry 指向的装机相对路径
		install -m 0755 "$mod_dir/bin" "$mod_dst/$entry_rel"
		# entry 绝对化：仅引号内的值换成装机绝对路径，其余字节原样
		sed "s|entry\([[:space:]]*=[[:space:]]*\)\"[^\"]*\"|entry\1\"/usr/lib/rockrobot3/modules/$name/$entry_rel\"|" \
			"$mod_dir/manifest.toml" > "$mod_dst/module.toml.tmp"
		chmod 0644 "$mod_dst/module.toml.tmp"
		mv "$mod_dst/module.toml.tmp" "$mod_dst/module.toml"
		# 改写复核：装机 manifest 的 entry 必须恰为新绝对路径（防同型错位回归）
		check="$(sed -n 's/^entry[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$mod_dst/module.toml" | head -n 1)"
		if [ "$check" != "/usr/lib/rockrobot3/modules/$name/$entry_rel" ]; then
			echo "post-build.sh: 警告：模块 $name entry 改写复核不一致（得 $check），整模块跳过" >&2
			rm -rf "$mod_dst"
			continue
		fi
		if [ -n "$REPO_ROOT" ] && [ -f "$REPO_ROOT/modules/$name/config.toml" ]; then
			install -m 0644 "$REPO_ROOT/modules/$name/config.toml" "$mod_dst/config.toml"
		else
			echo "post-build.sh: 警告：模块 $name 的 config.toml 未随装（RR3_REPO_ROOT=$REPO_ROOT 下 modules/$name/config.toml 缺席）——模块开机即读 manifest 同目录 config，缺件必重启环" >&2
		fi
	done
fi

# ---- 5. 装载消费真值集装机（增补轮六 基-25／实-12，块-15 修法 c：buildroot 轨此前
# 连一册契约都不装——装载必需件缺席的洞在装机侧完全不可见）----
# 装机位与快路轨同根（/usr/lib/rockrobot3，rrd.service 头注"两轨同一组安装位"纪律，
# 基-23 兑现）；
# 清单真值 = rrd rr3_runtime_truth_set（基-25：三处消费方判等的唯一清单，装机面整目录
# 随装、不抄第二份文件清单——多带装载不读的契约件无害，逐册判等是 rr3-pack 打包面
# 与 distro-smoke 断言面的事）。缺件 = 警告不阻断（镜像构建面），与上面 rrd/rr3-ctl
# 产物缺席同口径：镜像结构仍应可构建；但 S99rrd 已传 --contracts/--profile 启用消费
# （基-23，本脚本第 6 件），真值集缺席的镜像板上 rrd 装载期缺册即拒 boot——
# fail-closed 点名，这正是装载门在出厂轨存在后的预期行为。
if [ -n "$REPO_ROOT" ]; then
	for rel in profiles contracts hardware; do
		if [ -d "$REPO_ROOT/$rel" ]; then
			mkdir -p "$TARGET/usr/lib/rockrobot3/$rel"
			cp -R "$REPO_ROOT/$rel/." "$TARGET/usr/lib/rockrobot3/$rel/"
		else
			echo "post-build.sh: 警告：$REPO_ROOT/$rel 缺席，装载真值集未随装（S99rrd 已启用装载正门，此镜像板上 rrd 装载期缺册即拒 boot）" >&2
		fi
	done
else
	echo "post-build.sh: 警告：RR3_REPO_ROOT 未设且仓根定位失败，装载真值集未随装（基-25）" >&2
fi

# ---- 2. fstab：/var/run tmpfs + data / staging 挂载 --------------------------
cat > "$TARGET/etc/fstab" <<'EOF'
# <fs>                  <mountpoint>    <type>  <opts>                  <dump> <pass>
proc                    /proc           proc    defaults                0      0
devpts                  /dev/pts        devpts  defaults,gid=5,mode=620 0      0
tmpfs                   /dev/shm        tmpfs   mode=0777               0      0
tmpfs                   /tmp            tmpfs   mode=1777               0      0
tmpfs                   /var/run        tmpfs   mode=0755               0      0
tmpfs                   /var/log        tmpfs   mode=0755               0      0
# 用户数据：升级不动（base/ota/partition-layout.md 分区表）
LABEL=rr_data           /data           ext4    defaults                0      1
# 模块级更新暂存区（ADR-0007 决策二），rrd 拉包 → 校验 → 落位
LABEL=rr_staging        /var/lib/rr3/staging ext4 defaults              0      2
EOF

# ---- 1. rrd 开机自启（BusyBox init）-----------------------------------------
cat > "$TARGET/etc/init.d/S99rrd" <<'EOF'
#!/bin/sh
# rockrobot3 守护进程自启（post-build.sh 生成）。最后一位 S99：
# 等所有内核驱动与基础服务就位后再起 rrd。
case "$1" in
	start)
		[ -x /usr/bin/rrd ] || exit 0
		# 装载正门（基-23）：与发行版快路轨 rrd.service 的 ExecStart 三参数同形——
		# 两轨注入约定同源，装配链（装载/查册/缺格判定/manifest 解析）在 bind 前完成，
		# 缺册/缺格 fail-closed 点名。不再逐个 --manifest（与 --profile 同给 rrd 在
		# parse 处即拒）：manifests 由装配单按 modules-root（/usr/lib/rockrobot3，
		# 装配单路径上两级）推导，模块装机布局见本脚本第 4 件。
		# 机型档选型：distro.toml = RK3566 参考机 host 档（chip=rk3566，与 rrd.service
		# 缺省锚点同源）；机型专属装配单归真板档（M1）另立后此处随改。
		start-stop-daemon -S -b -m -p /var/run/rrd.pid \
			-x /usr/bin/rrd -- --socket /var/run/rock3-ipc.sock \
			--contracts /usr/lib/rockrobot3/contracts \
			--profile /usr/lib/rockrobot3/profiles/distro.toml
		;;
	stop)
		start-stop-daemon -K -p /var/run/rrd.pid
		;;
	restart)
		"$0" stop
		"$0" start
		;;
	*)
		echo "Usage: $0 {start|stop|restart}"
		exit 1
		;;
esac
EOF
chmod 0755 "$TARGET/etc/init.d/S99rrd"

echo "post-build.sh: 目标根装配完成（fstab + S99rrd 装载正门 + 产物安装位）"
