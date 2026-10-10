#!/usr/bin/env bash
# matrix.sh —— 点验矩阵七列出数入口（0.0.5 批K H3/C12/C11；architecture §12.6 v0.9、
# 0.0.5 方案 §13.2 D4 裁定：执行宿主 = 钉死版本的标准化环境，不是某台机器）。
#
# 定位（§13.2 CI 真源不迁移）：判据的唯一真源仍是 tools/ci/*.sh（入口 check.sh 十步 +
# ipct003/rr-core-xtensa-check/sim-chain 等专用门）；本脚本只是**出数入口**——按 §12.6
# 七列把"本环境实跑了什么、跑出什么"记成一行矩阵，不另立第二套判定逻辑。
#
# 列 → 生产者（本环境能出的证据）：
#   ① 字节契约      tools/ci/ipct003.sh（默认宽容口径跑，矩阵记**实跑 N/4 路** + SKIP 原因，
#                   §12.6：不许写"① 已过"）
#   ② 机制核编译面  tools/ci/rr-core-xtensa-check.sh（工具链缺位 = 脚本自身 SKIP 明说）
#   ③ 供给自证      cargo test -q -p rrd --lib profile::tests——装载门定性：只证
#                   "描述符没撒谎"，**不证 port 正确**（§12.6 v0.9；实测值归 E4）
#   ④ 描述符装载    cargo test -q -p rr-registry --test contract（查册/形状 pin/
#                   未知键点名/缺格 Degraded）
#   ⑤ 安全核生效    遍历 modules/ 含 fn parity_leg 的 crate 逐个 cargo test -q parity_leg
#                   ——E1 形态 = host 数值一致（腿一确定性 + 腿二契约期望值）；
#                   目标上真跑归 E4，不得由本环境冒领
#   ⑥ 生命周期      python3 ≥3.11 预检（sim-chain.sh 的 stdlib tomllib 前提）后：
#                   cargo test -q -p rrd --tests（e2e 全家）+ tools/ci/sim-chain.sh
#   ⑦ 时序达标      **无固定载体**——真值只能来自 E4 真板周期直方图（云 runner 是共享
#                   x86、qemu-user 是指令级模拟，时间都不是硬件时间）。手测回填：
#                   bash tools/ci/matrix.sh --manual 7 --evidence <仓内证据文件>
#
# 环境指纹三件套（§13.2 版本钉三则）：每行产出必带——
#   载体/镜像（容器载体请设 RR3_MATRIX_IMAGE_DIGEST=镜像@digest）+ `rustc -Vv` + 目标三元组。
#   指纹缺失的行判"无载体"不判 PASS；指纹一变，旧绿迹作废标"环境已换代"（换代即重跑），
#   出数后回写 status.md 矩阵行。
#
# --manual <列号> --evidence <仓内证据文件路径>（C12 手测回填契约）：把人工实测写进
#   矩阵行。证据文件必须自带三样——环境指纹（设备/固件版本 + rustc -Vv + 目标三元组）、
#   采样窗口与样本数、原始数据（周期直方图或日志切片）；缺一判"证据不足"不判 PASS。
#   手测行与自动行以 [手测]/[自动] 分标，不混读；手测行**不覆盖自动 FAIL 格**。
#
# 退出码：FAIL → 1；--strict 且存在未跑列（SKIP/无载体计未跑，不判 PASS）→ 2；否则 0。
# --strict 口径 = "七列全部真跑齐才放行"（与 ipct003.sh --strict 同语义）。
# bash 3.2 兼容（macOS 自带 bash）：不用关联数组/mapfile，格用 st<N>/tx<N> 变量 + case。
set -uo pipefail
root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "ERROR: git rev-parse --show-toplevel 失败——请在本仓内执行。" >&2
  exit 1
}
cd "$root" || exit 1
export PATH="$HOME/.cargo/bin:$PATH"

usage() {
  echo "用法: bash tools/ci/matrix.sh [--strict] [--manual <列号> --evidence <仓内证据文件路径>]"
}

strict=0
manual_col=""
manual_evidence=""
while [ $# -gt 0 ]; do
  case "$1" in
    --strict) strict=1; shift ;;
    --manual)
      [ $# -ge 2 ] || { echo "ERROR: --manual 需要列号参数。" >&2; usage >&2; exit 1; }
      manual_col="$2"; shift 2 ;;
    --evidence)
      [ $# -ge 2 ] || { echo "ERROR: --evidence 需要路径参数。" >&2; usage >&2; exit 1; }
      manual_evidence="$2"; shift 2 ;;
    *) echo "ERROR: 未知参数：$1" >&2; usage >&2; exit 1 ;;
  esac
done
if { [ -n "$manual_col" ] && [ -z "$manual_evidence" ]; } ||
   { [ -z "$manual_col" ] && [ -n "$manual_evidence" ]; }; then
  echo "ERROR: --manual <列号> 与 --evidence <路径> 必须成对给出。" >&2
  usage >&2
  exit 1
fi
case "$manual_col" in
  "") ;;
  1|2|3|4|5|6|7) ;;
  *) echo "ERROR: --manual 列号须为 1..7（§12.6 七列），得：'$manual_col'。" >&2; exit 1 ;;
esac

scratch="$(mktemp -d)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
trap 'rm -rf "$scratch"' EXIT
trap 'rm -rf "$scratch"; exit 130' INT TERM

# —— 环境指纹三件套 ——
has_toolchain=1
command -v cargo >/dev/null 2>&1 || has_toolchain=0
command -v rustc >/dev/null 2>&1 || has_toolchain=0
mirror="${RR3_MATRIX_IMAGE_DIGEST:-}"
if [ -n "$mirror" ]; then
  carrier="容器镜像 $mirror"
else
  carrier="本机直跑（无容器镜像；容器载体设 RR3_MATRIX_IMAGE_DIGEST=镜像@digest 补齐本件）"
fi
os_line="$(uname -srm 2>/dev/null || echo unknown)"
if [ "$has_toolchain" -eq 1 ]; then
  rustc_ver="$(rustc -Vv 2>/dev/null | sed -n '1p')"
  host_triple="$(rustc -vV 2>/dev/null | awk '/^host:/{print $2}')"
else
  rustc_ver="(rustc/cargo 缺位)"
  host_triple="(无)"
fi

echo "=== 环境指纹（三件套，§13.2 版本钉三则）==="
echo "载体/镜像: $carrier"
echo "宿主:      $os_line"
echo "rustc -Vv: $rustc_ver"
echo "目标三元组: $host_triple"
echo

# —— run_tests 语义的 cargo 测试助手（同 check.sh 口径：退出码 + "running N tests" N≥1
#    + "test result: ok. N passed" N≥1，封过滤器打空/测试集丢失的 fail-open）。
#    输出落 $scratch/last.out 供 FAIL 时点名；通过返 0，否则返 1。
cargo_test_ok() {
  local dir="$1"; shift
  if ( cd "$dir" && cargo test -q "$@" ) >"$scratch/last.out" 2>&1; then
    grep -qE '^running [1-9][0-9]* tests?' "$scratch/last.out" &&
      grep -qE '^test result: ok\. [1-9][0-9]* passed' "$scratch/last.out" && return 0
    return 1
  fi
  return 1
}

# —— 七格（st=状态 pass|fail|skip|manual；tx=出数文本）——
st1=skip tx1="未出数"; st2=skip tx2="未出数"
st3=skip tx3="未出数"; st4=skip tx4="未出数"
st5=skip tx5="未出数"; st6=skip tx6="未出数"
st7=skip tx7="未出数"

# ① 字节契约：ipct003.sh 默认宽容口径，矩阵只如实记"实跑 N/4 路"。
echo "--- ① 字节契约（tools/ci/ipct003.sh，记实跑 N/4 路）---"
if [ "$has_toolchain" -eq 0 ]; then
  tx1="无载体（rustc/cargo 缺位）"
else
  if bash tools/ci/ipct003.sh >"$scratch/c1.out" 2>&1; then
    p1="$(grep -c '^PASS' "$scratch/c1.out" || true)"
    s1="$(grep -c '^SKIP' "$scratch/c1.out" || true)"
    if [ "${s1:-0}" -eq 0 ]; then
      st1=pass
      tx1="实跑 4/4 路全对拍（宿主 dump + aarch64 编译期 + arm64 容器 + xtensa 编译期）"
    else
      st1=skip
      reason1="$(grep '^SKIP' "$scratch/c1.out" | sed 's/^SKIP  //; s/——.*//; s/（.*//' | tr '\n' '；' | sed 's/；$//')"
      tx1="实跑 ${p1:-0}/4 路，SKIP ${s1:-0} 路（${reason1}）——--strict 口径计未跑（§12.6：不许写\"① 已过\"）"
    fi
  else
    st1=fail
    tx1="ipct003.sh 失败（对拍破裂或运行错误）"
    sed -n '1,30p' "$scratch/c1.out"
  fi
fi
printf '状态: %s ｜ %s\n\n' "$st1" "$tx1"

# ② 机制核编译面：xtensa 三查（SKIP 语义脚本内定，矩阵照抄）。
echo "--- ② 机制核编译面（tools/ci/rr-core-xtensa-check.sh）---"
if [ "$has_toolchain" -eq 0 ]; then
  tx2="无载体（rustc/cargo 缺位）"
else
  if bash tools/ci/rr-core-xtensa-check.sh >"$scratch/c2.out" 2>&1; then
    if grep -q '^SKIP' "$scratch/c2.out"; then
      tx2="无载体（$(grep '^SKIP' "$scratch/c2.out" | sed 's/^SKIP  //; s/——.*//' | sed -n '1p')）"
    else
      st2=pass
      tx2="tripwire + xtensa-esp32-none-elf 编译 + 宿主对拍三查全绿（bare 档编译面）"
    fi
  else
    st2=fail
    tx2="机制核面编译/tripwire/宿主对拍失败（工具链在位）"
    sed -n '1,30p' "$scratch/c2.out"
  fi
fi
printf '状态: %s ｜ %s\n\n' "$st2" "$tx2"

# ③ 供给自证：装载门（§12.6 v0.9 定性——只证描述符没撒谎，不证 port 正确）。
echo "--- ③ 供给自证（装载门：rrd profile::tests）---"
if [ "$has_toolchain" -eq 0 ]; then
  tx3="无载体（rustc/cargo 缺位）"
else
  if cargo_test_ok base/runtime -p rrd --lib profile::tests; then
    st3=pass
    # cargo -q 吞逐测行（test xxx ... ok 不出），条数从 result 行提取
    n3="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$scratch/last.out" | tail -n 1)"
    tx3="装载门全绿（rrd profile::tests ${n3:-?} 条）——装载门定性：不证 port 正确，实测值归 E4"
  else
    st3=fail
    tx3="rrd profile::tests 失败或 0 通过（过滤器打空属门禁失效）"
    sed -n '1,30p' "$scratch/last.out"
  fi
fi
printf '状态: %s ｜ %s\n\n' "$st3" "$tx3"

# ④ 描述符装载：rr-registry contract（查册/形状 pin/未知键点名/缺格 Degraded）。
echo "--- ④ 描述符装载（rr-registry contract）---"
if [ "$has_toolchain" -eq 0 ]; then
  tx4="无载体（rustc/cargo 缺位）"
else
  if cargo_test_ok base/runtime -p rr-registry --test contract; then
    st4=pass
    n4="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$scratch/last.out" | tail -n 1)"
    tx4="契约测试全绿（rr-registry contract ${n4:-?} 条：schema/注册表形状 pin/装配校验/L-10/注入面）"
  else
    st4=fail
    tx4="rr-registry contract 失败或 0 通过"
    sed -n '1,30p' "$scratch/last.out"
  fi
fi
printf '状态: %s ｜ %s\n\n' "$st4" "$tx4"

# ⑤ 安全核生效：件级两腿对拍逐 crate（E1 形态 = host 数值一致；目标真跑归 E4）。
echo "--- ⑤ 安全核生效（件级两腿对拍，逐 crate）---"
if [ "$has_toolchain" -eq 0 ]; then
  tx5="无载体（rustc/cargo 缺位）"
else
  parity_mods=""
  for d in modules/*/; do
    [ -f "${d}Cargo.toml" ] || continue
    if grep -rqs 'fn parity_leg' "${d}atoms" "${d}src" 2>/dev/null; then
      parity_mods="$parity_mods $(basename "$d")"
    fi
  done
  if [ -z "$parity_mods" ]; then
    st5=fail
    tx5="枚举打空：modules/ 无任何含 fn parity_leg 的 crate（门禁失效，不放行）"
  else
    st5=pass
    bad5=""
    for m in $parity_mods; do
      if cargo_test_ok "modules/$m" parity_leg; then
        n5="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$scratch/last.out" | tail -n 1)"
        printf 'PASS  %s：两腿对拍 %s 条全绿\n' "$m" "${n5:-?}"
      else
        st5=fail
        bad5="$bad5 $m"
        printf 'FAIL  %s：parity_leg 失败或 0 通过\n' "$m"
        sed -n '1,20p' "$scratch/last.out"
      fi
    done
    if [ "$st5" = "pass" ]; then
      tx5="逐 crate 两腿对拍全绿（${parity_mods# }）——确定性腿 + 契约期望腿 host 数值一致；目标上真跑归 E4"
    else
      tx5="对拍不齐 crate:${bad5}（只有腿一记未达标，§4.0.3）"
    fi
  fi
fi
printf '状态: %s ｜ %s\n\n' "$st5" "$tx5"

# ⑥ 生命周期：rrd e2e 全家 + sim-chain 六真进程闭环（python3 ≥3.11 是 sim 链前提）。
echo "--- ⑥ 生命周期（rrd e2e + tools/ci/sim-chain.sh；python3 ≥3.11 前提）---"
py_ok=0
py_ver=""
if command -v python3 >/dev/null 2>&1; then
  py_ver="$(python3 -V 2>&1 | awk '{print $2}')"
  if python3 -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)' 2>/dev/null; then
    py_ok=1
  fi
fi
if [ "$has_toolchain" -eq 0 ]; then
  tx6="无载体（rustc/cargo 缺位）"
elif [ "$py_ok" -eq 0 ]; then
  tx6="无载体（python3 ${py_ver:-缺位} < 3.11——sim-chain.sh 的 stdlib tomllib 前提；触发条件：E1 ubuntu 载体或本机升 python ≥3.11）"
else
  if cargo_test_ok base/runtime -p rrd --tests; then
    if bash tools/ci/sim-chain.sh >"$scratch/c6.out" 2>&1; then
      st6=pass
      tx6="rrd e2e 全家 + rr3-sim --chain 六真进程 mock 闭环全绿（注册→心跳→分发→节拍→断开→监督重启）"
    else
      st6=fail
      tx6="sim-chain.sh 失败（rrd e2e 半边已绿，闭环链红）"
      sed -n '1,30p' "$scratch/c6.out"
    fi
  else
    st6=fail
    tx6="rrd tests（e2e 全家）失败或 0 通过"
    sed -n '1,30p' "$scratch/last.out"
  fi
fi
printf '状态: %s ｜ %s\n\n' "$st6" "$tx6"

# ⑦ 时序达标：E4 真板专属，本环境固定"无载体 + 触发条件"（§13.2：不得由标准环境冒领）。
echo "--- ⑦ 时序达标（E4 真板专属，本环境固定无载体）---"
tx7="无载体（E4 真板专属——云 runner 共享 x86 / qemu-user 指令级模拟，时间非硬件时间，不得冒领）；触发条件：0.0.6 树莓派真板批 / 0.0.7 轻承载档真板（周期直方图）；手测回填：bash tools/ci/matrix.sh --manual 7 --evidence <仓内证据文件>"
printf '状态: %s ｜ %s\n\n' "$st7" "$tx7"

# —— [手测] 回填（C12 契约：证据三样齐才收，不覆盖自动 FAIL 格）——
if [ -n "$manual_col" ]; then
  echo "--- [手测] 回填：列$manual_col ← $manual_evidence ---"
  ev="$manual_evidence"
  case "$ev" in /*) ;; *) ev="$root/$ev" ;; esac
  reasons=""
  if [ ! -f "$ev" ]; then
    reasons="文件不存在"
  elif [ ! -s "$ev" ]; then
    reasons="文件为空"
  else
    case "$ev" in
      "$root"/*) ;;
      *) reasons="不在仓内（--evidence 须为仓内证据文件，随仓提交留痕）；" ;;
    esac
    grep -q '环境指纹' "$ev" || reasons="${reasons}缺[环境指纹]；"
    grep -q '采样窗口' "$ev" || reasons="${reasons}缺[采样窗口]；"
    grep -q '样本数' "$ev" || reasons="${reasons}缺[样本数]；"
    grep -q '原始数据' "$ev" || reasons="${reasons}缺[原始数据]；"
    grep -Eq '(x86_64|aarch64|arm64e?|armv[67]|riscv(32|64)|xtensa)[a-z0-9_]*-[a-z0-9_]+-(linux|darwin|none|espidf|android|freebsd)' "$ev" \
      || reasons="${reasons}缺[目标三元组]；"
  fi
  case "$manual_col" in
    1) _st="$st1"; _tx="$tx1" ;; 2) _st="$st2"; _tx="$tx2" ;;
    3) _st="$st3"; _tx="$tx3" ;; 4) _st="$st4"; _tx="$tx4" ;;
    5) _st="$st5"; _tx="$tx5" ;; 6) _st="$st6"; _tx="$tx6" ;;
    7) _st="$st7"; _tx="$tx7" ;;
  esac
  if [ -z "$reasons" ]; then
    case "$_st" in
      fail)
        echo "WARN  [手测] 证据在册但自动格 FAIL——手测行不覆盖自动 FAIL 格（先修自动项）。" >&2
        _st="fail"
        _tx="$_tx ｜ [手测] 证据在册（${manual_evidence}）但不覆盖 FAIL 格" ;;
      skip)
        _st="manual"
        _tx="$_tx ｜ [手测] 证据在册（${manual_evidence}）——三样齐（环境指纹/采样窗口与样本数/原始数据）" ;;
      *)
        _tx="$_tx ｜ [手测] 另有证据在册（${manual_evidence}）" ;;
    esac
  else
    echo "WARN  [手测] 证据不足：${reasons}（缺一判证据不足，不判 PASS——C12 契约）" >&2
    _tx="$_tx ｜ [手测] 证据不足（${manual_evidence}）：${reasons}"
  fi
  case "$manual_col" in
    1) st1="$_st"; tx1="$_tx" ;; 2) st2="$_st"; tx2="$_tx" ;;
    3) st3="$_st"; tx3="$_tx" ;; 4) st4="$_st"; tx4="$_tx" ;;
    5) st5="$_st"; tx5="$_tx" ;; 6) st6="$_st"; tx6="$_tx" ;;
    7) st7="$_st"; tx7="$_tx" ;;
  esac
  echo
fi

# —— 矩阵行 + 汇总 ——
render() {
  case "$1" in
    pass)   printf 'PASS' ;;
    fail)   printf 'FAIL' ;;
    skip)   printf '未跑（SKIP/无载体）' ;;
    manual) printf 'PASS[手测回填]' ;;
    *)      printf '??' ;;
  esac
}
echo "=== 点验矩阵（本环境一行；行指纹 = 顶部环境三件套；[自动]行与 [手测] 注记分标）==="
echo
echo "| 列 | 检查项 | 本环境出数 | 状态 |"
echo "|---|---|---|---|"
printf '| ① | 字节契约 | %s | [自动] %s |\n' "$tx1" "$(render "$st1")"
printf '| ② | 机制核编译面 | %s | [自动] %s |\n' "$tx2" "$(render "$st2")"
printf '| ③ | 供给自证（装载门） | %s | [自动] %s |\n' "$tx3" "$(render "$st3")"
printf '| ④ | 描述符装载（装载门） | %s | [自动] %s |\n' "$tx4" "$(render "$st4")"
printf '| ⑤ | 安全核生效 | %s | [自动] %s |\n' "$tx5" "$(render "$st5")"
printf '| ⑥ | 生命周期 | %s | [自动] %s |\n' "$tx6" "$(render "$st6")"
printf '| ⑦ | 时序达标 | %s | [自动] %s |\n' "$tx7" "$(render "$st7")"
echo
pass_n=0; fail_n=0; skip_n=0; manual_n=0
for s in "$st1" "$st2" "$st3" "$st4" "$st5" "$st6" "$st7"; do
  case "$s" in
    pass)   pass_n=$((pass_n + 1)) ;;
    fail)   fail_n=$((fail_n + 1)) ;;
    skip)   skip_n=$((skip_n + 1)) ;;
    manual) manual_n=$((manual_n + 1)) ;;
  esac
done
echo "=== 汇总 ==="
echo "行指纹: $carrier ｜ $rustc_ver ｜ ${host_triple}（回写 status.md 矩阵行时随行带走；指纹一变旧绿迹作废）"
echo "PASS=$pass_n  PASS[手测]=$manual_n  未跑=$skip_n  FAIL=$fail_n"
if [ "$fail_n" -gt 0 ]; then
  echo "存在 FAIL 列——矩阵出数失败（先修红项，手测行不覆盖 FAIL 格）"
  exit 1
fi
if [ "$strict" -eq 1 ] && [ "$skip_n" -gt 0 ]; then
  echo "--strict：$skip_n 列未跑（SKIP/无载体计未跑，不判 PASS）——矩阵未完整出数，不放行"
  exit 2
fi
if [ "$strict" -eq 1 ]; then
  echo "--strict：七列全部出数，矩阵完整，放行"
else
  echo "默认口径：如实出数（未跑列以无载体/SKIP 记入矩阵，不冒领 PASS）"
fi
exit 0
