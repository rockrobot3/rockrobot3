# SPDX-License-Identifier: Apache-2.0
# ⑨-y 跨平台可编面：Windows 红 crate 基线 + ESP 专用工程协议头字段漂移。
# 由 check.sh 在启动段 source；函数只读写 check.sh 的全局 fail/root。
# Windows 基线与 ESP 协议头两项各成一函数（gy_windows / gy_header），
# 临时件与 fail 汇总留在 rr3_step_9y。

# Windows 红 crate 基线：target 未装即 fail-closed；装了就跑 cross check 取红 crate 数。
gy_windows() {
  if rustup target list --installed 2>/dev/null | grep -q '^x86_64-pc-windows-msvc$'; then
    gy_red=0
    if ! cargo check --locked --manifest-path base/runtime/Cargo.toml \
           --target x86_64-pc-windows-msvc --keep-going >"$gy_log" 2>&1; then
      gy_red="$(grep -oE 'could not compile `[a-z0-9_-]+`' "$gy_log" | sort -u | wc -l | tr -d ' ')"
    fi
    gy_b="$(grep -vE '^[[:space:]]*(#|$)' tools/ci/baseline/gate-win-baseline.txt 2>/dev/null | sed -n '1p' | tr -d '[:space:]')"
    case "$gy_b" in
      ''|*[!0-9]*)
        printf 'FAIL  ⑨-y 基线文件形状坏: tools/ci/baseline/gate-win-baseline.txt（首个非注释行须为纯整数，实得"%s"）——fail-closed，不放行\n' "${gy_b:-<缺文件或空>}"
        gate_y_fail=1
        ;;
      *)
        if [ "$gy_red" -gt "$gy_b" ]; then
          printf 'FAIL  ⑨-y Windows 红 crate 回涨: %s > 基线 %s（"只降不升"被破——新红 crate 必有一笔未归因，先归因再动基线；红名单见上 cargo 输出）\n' \
            "$gy_red" "$gy_b"
          grep -oE 'could not compile `[a-z0-9_-]+`' "$gy_log" | sort -u | sed 's/^/      红 crate: /'
          gate_y_fail=1
        elif [ "$gy_red" -lt "$gy_b" ]; then
          printf 'PASS  ⑨-y Windows 红 crate %s < 基线 %s（**红账下降 %s：须同批下调 tools/ci/baseline/gate-win-baseline.txt**；转绿终点 = 基-33 拆修全落 count=0）\n' \
            "$gy_red" "$gy_b" "$((gy_b - gy_red))"
        else
          printf 'PASS  ⑨-y Windows 红 crate %s = 基线 %s（只降不升在账；基-33 转绿终点已至（批N19 结案），本门自此防回涨）\n' \
            "$gy_red" "$gy_b"
        fi
        ;;
    esac
  else
    printf 'FAIL  ⑨-y Windows target 未安装（rustup target add x86_64-pc-windows-msvc）——fail-closed 不设 SKIP（能跑的机器越少，漂移越没人发现）\n'
    gate_y_fail=1
  fi
}

# —— ⑨-y 附加判据：ESP 专用工程不进门禁 ④/⑥，宿主编译看不见协议头字段漂移
#    （0.0.8 静态审计实锤：payload_crc32 进场后 esp32-uart-echo 的 Rr3Header
#    字面量未跟，xtensa 固件 E0063 编不过）。按构造点计数 fail-closed：
#    剥注释后"构造数（扣除 -> Rr3Header 返回型）"必须 ≤ payload_crc32 字段数。
#    工具链不在位的机器也能跑，是这些工程唯一不依赖 esp 环境的漂移防线。
gy_header() {
  local f h r c need hdr_bad
  hdr_bad=0
  while IFS= read -r f; do
    [ -f "$f" ] || continue
    h=$(sed 's://.*$::' "$f" | grep -c 'Rr3Header[[:space:]]*{')
    r=$(sed 's://.*$::' "$f" | grep -c -- '->[[:space:]]*Rr3Header')
    c=$(sed 's://.*$::' "$f" | grep -c 'payload_crc32')
    need=$((h - r))
    if [ "$need" -gt 0 ] && [ "$c" -lt "$need" ]; then
      printf 'FAIL  ⑨-y 协议头字段漂移 %s：构造 %s 处、payload_crc32 %s 处（缺字段即 xtensa 编不过）\n' \
        "$f" "$need" "$c"
      hdr_bad=1
    fi
  done < <(git -c core.quotepath=false ls-files 'tools/*/src/*.rs' | grep -E '(esp|espidf)')
  if [ "$hdr_bad" -ne 0 ]; then
    fail=1
  else
    printf 'PASS  ⑨-y 协议头字段齐全（ESP 专用工程 Rr3Header 构造均带 payload_crc32）\n'
  fi
}

rr3_step_9y() {
  gate_y_fail=0
  gy_log="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  gy_windows
  rm -f "$gy_log"
  if [ "$gate_y_fail" -ne 0 ]; then
    fail=1
  fi
  gy_header
}
