# ⑨-w 依赖面禁令（门 9w 步骤体：单元直依 rr-hal-api/rr-bus-api、use 面白名单冻结、
#    白名单条目数基线钉数、板侧禁实现 crate；基-35，全 fail-closed）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail。

rr3_step_9w() {
  local self="${BASH_SOURCE[0]}"
  dep_fail=0
  dep_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  # 1) rr-hal-api 必依（逐单元点名；枚举打空即门禁失效，与 ④/⑥ 同口径）
  unit_manifests="$(find modules -maxdepth 2 -name Cargo.toml -not -path '*/target/*' | sort)"
  if [ -z "$unit_manifests" ]; then
    printf 'FAIL  ⑨-w 单元 Cargo.toml 枚举打空（门禁失效，不放行）\n'
    dep_fail=1
  fi
  for c in $unit_manifests; do
    if grep -q 'rr-hal-api' "$c"; then
      printf 'PASS  ⑨-w %s 直依 rr-hal-api（契约依赖面在 Cargo 清单可证）\n' "$c"
    else
      printf 'FAIL  ⑨-w %s 未声明 rr-hal-api——单元契约符号须直依能力面 crate，不经实现 crate re-export（基-35）\n' "$c"
      dep_fail=1
    fi
  done
  # 2) use 面白名单（modules/** 全部 .rs；行注释先剥；白名单外逐行点名）
  unit_rs=0
  while IFS= read -r -d '' f; do
    unit_rs=$((unit_rs + 1))
    sed 's://.*$::' "$f" \
      | grep -nE 'rr_hal::' \
      | grep -vE 'rr_hal::(backend::|sim|provision_store|Rr3PcmStream)' \
      | sed "s|^|$f:|" >> "$dep_hits"
  done < <(find modules -name '*.rs' -not -path '*/target/*' -print0 | sort -z)
  if [ "$unit_rs" -eq 0 ]; then
    printf 'FAIL  ⑨-w 单元 .rs 枚举打空（门禁失效，不放行）\n'
    dep_fail=1
  elif [ -s "$dep_hits" ]; then
    cat "$dep_hits"
    printf 'FAIL  ⑨-w 依赖面白名单外命中（契约符号须走 rr_hal_api；白名单=backend 装配/sim/provision_store/Rr3PcmStream 四类冻结，扩名单须登记，见上行号）\n'
    dep_fail=1
  else
    printf 'PASS  ⑨-w 单元 use 面白名单（%s 份 .rs 全扫，rr_hal:: 引用皆装配/承载白名单四类）\n' "$unit_rs"
  fi
  rm -f "$dep_hits"
  # 3) 板侧禁实现 crate（rr-hal/rr-bus/rrd；用 `名 =` 形状匹配依赖键，不误伤 rr-hal-api）
  board_dep="$(grep -E '^[[:space:]]*(rr-hal|rr-bus|rrd)[[:space:]]*=' tools/esp32-periph-base/Cargo.toml 2>/dev/null || true)"
  if [ -n "$board_dep" ]; then
    printf '%s\n' "$board_dep"
    printf 'FAIL  ⑨-w 板侧 Cargo 出现实现 crate 依赖（rr-hal/rr-bus/rrd，architecture §14 全量口径）\n'
    dep_fail=1
  else
    printf 'PASS  ⑨-w 板侧零实现 crate 依赖（esp32-periph-base，rr-hal/rr-bus/rrd 三件钉住）\n'
  fi
  # 4) 承载面必依 rr-bus-api 且禁列 rr-bus（依赖键 = 形状匹配，同判据 3 口径，不误伤
  #    注释与 rr-bus-api 自身；枚举复用判据 1 的 ${unit_manifests}）
  for c in $unit_manifests; do
    if grep -qE '^[[:space:]]*rr-bus-api[[:space:]]*=' "$c" \
       && ! grep -qE '^[[:space:]]*rr-bus[[:space:]]*=' "$c"; then
      printf 'PASS  ⑨-w %s 承载面直依 rr-bus-api（不绑 rr-bus 实现层，基-35 窄化②）\n' "$c"
    else
      printf 'FAIL  ⑨-w %s 承载面依赖违例——须直依 rr-bus-api 且不得列 rr-bus（基-35 窄化②/批N9 件一）\n' "$c"
      dep_fail=1
    fi
  done
  # 5) 单元 use 面 rr_bus:: 零命中（rr_bus_api:: 不含子串 rr_bus:: 不误伤；rrd/tools 的
  #    re-export 路径不在扫描面——它们消费 rr-bus 是工具层既有 import，豁免同判据 3 注）
  bus_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  while IFS= read -r -d '' f; do
    sed 's://.*$::' "$f" \
      | grep -nE 'rr_bus::' \
      | sed "s|^|$f:|" >> "$bus_hits"
  done < <(find modules -name '*.rs' -not -path '*/target/*' -print0 | sort -z)
  if [ -s "$bus_hits" ]; then
    cat "$bus_hits"
    printf 'FAIL  ⑨-w 单元 use 面命中 rr_bus::——承载客户端走 rr_bus_api，语义常量直引 rr_core（基-35 窄化②，见上行号）\n'
    dep_fail=1
  else
    printf 'PASS  ⑨-w 单元 use 面零 rr_bus::（%s 份 .rs 全扫，承载面已契约化）\n' "$unit_rs"
  fi
  rm -f "$bus_hits"
  # 6) 白名单条目数基线钉数（判据 2) 白名单正则交替项数对基线；基-35 窄化①/§25.8 #1）。
  #    条目数从本文件判据 2) 的白名单行提取（单真值点自执法面自身导出，不抄第二份——
  #    同 ⑨-n 键名单从代码常量导出口径）；提取打空即门禁失效。
  wl_line="$(grep -m1 -oE "grep -vE 'rr_hal::\([^)]*\)'" "$self" || true)"
  wl_count="$(printf '%s' "$wl_line" | sed -n 's/.*rr_hal::(\([^)]*\)).*/\1/p' | awk -F'|' '{print NF}')"
  if [ -z "$wl_count" ] || [ "$wl_count" -eq 0 ] 2>/dev/null; then
    printf 'FAIL  ⑨-w 白名单条目数提取打空（判据 2) 白名单行不在或形状漂移）——门禁失效，不放行\n'
    dep_fail=1
  else
    gate_w_base="tools/ci/gate-w-baseline.txt"
    if [ ! -r "$gate_w_base" ]; then
      printf 'FAIL  ⑨-w 基线文件不可读: %s（裁定配套 fail-closed——白名单条目基线缺位即红，不放行）\n' "$gate_w_base"
      dep_fail=1
    else
      wl_baseline="$(grep -vE '^[[:space:]]*(#|$)' "$gate_w_base" | head -1 | tr -d '[:space:]')"
      case "$wl_baseline" in
        ''|*[!0-9]*)
          printf 'FAIL  ⑨-w 基线文件形状坏: %s（首个非注释行非纯整数: "%s"）——fail-closed，不放行\n' "$gate_w_base" "${wl_baseline:-<空>}"
          dep_fail=1
          ;;
        *)
          if [ "$wl_count" -gt "$wl_baseline" ]; then
            printf 'FAIL  ⑨-w 白名单条目 %s > 基线 %s（条目数只降不升违例——扩白名单须同批上调 %s，diff 即登记；基-35 窄化①/§25.8 #1）\n' \
              "$wl_count" "$wl_baseline" "$gate_w_base"
            dep_fail=1
          elif [ "$wl_count" -lt "$wl_baseline" ]; then
            printf 'PASS  ⑨-w 白名单条目 %s ≤ 基线 %s（**条目收紧 %s 类：须同批下调 tools/ci/gate-w-baseline.txt**）\n' \
              "$wl_count" "$wl_baseline" "$((wl_baseline - wl_count))"
          else
            printf 'PASS  ⑨-w 白名单条目 %s = 基线 %s（四类冻结设计态：backend 装配/sim/provision_store/Rr3PcmStream，只降不升在账）\n' \
              "$wl_count" "$wl_baseline"
          fi
          ;;
      esac
    fi
  fi
  if [ "$dep_fail" -ne 0 ]; then
    fail=1
  fi
}
