# SPDX-License-Identifier: Apache-2.0
# ⑧ 件级判据三门（门 ⑧ 步骤体：⑧-a 两腿对拍计数 / ⑧-b 依赖闭包 grep /
#    ⑧-c 注入面校验；含 C10 stub 反例自证；全 fail-closed）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail，并调用入口的共享助手 mod_lang。
# 拆分结构（前缀与文件既有变量 gate_fail/c10_*/atom_*/closure_* 及函数 parity_gate 同源）：
#   parity_slot_* / parity_leg_* —— ⑧-a 单格查表、判定与两腿计数；parity_gate 遍历全仓。
#   c10_fixture_* / c10_selftest —— C10 反例夹具与自证判定（自证跑的是同一 parity_gate）。
#   closure_gate —— ⑧-b 依赖闭包 grep 门；injection_gate —— ⑧-c 注入面校验门。
# 编排与 gate_fail→fail 汇总留在 rr3_step_8。

# 取某格在 slots.toml 的 status（段边界内取引号串；形状畸形归 atom.schema 门，此处不猜）。
parity_slot_status() {
  awk -v want="$2" '
    /^\[\[slot\]\]/  { in_s = 1; cur = "" }
    in_s && $1 == "name"   { v = $3; gsub(/[",]/, "", v); cur = v }
    in_s && $1 == "status" { v = $3; gsub(/[",]/, "", v); if (cur == want) { print v; exit } }
  ' "$1"
}

# 取某格的 tier_scope 档名列表（第二轴 L-2 扩读，空格分隔）。
parity_slot_tier() {
  awk -v want="$2" '
    /^\[\[slot\]\]/  { in_s = 1; cur = "" }
    in_s && $1 == "name"       { v = $3; gsub(/[",]/, "", v); cur = v }
    in_s && $1 == "tier_scope" {
      if (cur == want) {
        v = $0; out = ""
        while (match(v, /"[^"]+"/)) { out = out substr(v, RSTART + 1, RLENGTH - 2) " "; v = substr(v, RSTART + RLENGTH) }
        print out; exit
      }
    }
  ' "$1"
}

# 数一个件的两腿：扫描 mech.rs + 运行机制实体（composite/dispatch/rpc）**及其测试子目录**，
# 统计 fn parity_leg1_<件名>_* / fn parity_leg2_<件名>_* 命中数。输出 "<leg1> <leg2>"。
# 结构化拆分后机制实体的单测可能搬进 `<实体>/tests/`，门按内容执法、不按文件名执法。
parity_leg_counts() {
  local mod_dir="$1" atom_dir="$2" atom_name="$3" leg1=0 leg2=0 f
  local mech_files=("$atom_dir/mech.rs" "$mod_dir/src/composite.rs" "$mod_dir/src/dispatch.rs" "$mod_dir/src/rpc.rs")
  local mech_dirs=("$mod_dir/src/composite" "$mod_dir/src/dispatch" "$mod_dir/src/rpc")
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    leg1=$((leg1 + $(grep -cE "fn parity_leg1_${atom_name}_[a-z0-9_]+" "$f" || true)))
    leg2=$((leg2 + $(grep -cE "fn parity_leg2_${atom_name}_[a-z0-9_]+" "$f" || true)))
  done < <(printf '%s\n' "${mech_files[@]}" | while IFS= read -r f; do [ -f "$f" ] && printf '%s\n' "$f"; done)
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    leg1=$((leg1 + $(grep -cE "fn parity_leg1_${atom_name}_[a-z0-9_]+" "$f" || true)))
    leg2=$((leg2 + $(grep -cE "fn parity_leg2_${atom_name}_[a-z0-9_]+" "$f" || true)))
  done < <(for d in "${mech_dirs[@]}"; do [ -d "$d" ] && find "$d" -name '*.rs' -print; done)
  printf '%s %s\n' "$leg1" "$leg2"
}

# 两腿达标判定：打印该格两腿判词；返回 0=两腿齐、1=不齐（只有腿一记未达标）。
parity_leg_verdict() {
  local atom_name="$1" slot="$2" lang="$3" tier_scope="$4" legs="$5"
  local leg1="${legs%% *}" leg2="${legs##* }"
  if [ "$leg1" -ge 1 ] && [ "$leg2" -ge 1 ]; then
    printf 'PASS  ⑧-a %s→%s: 两腿齐（leg1=%s leg2=%s，扫描 mech.rs + composite/dispatch/rpc(.rs|/)；lang=%s tier_scope=%s）\n' "$atom_name" "$slot" "$leg1" "$leg2" "$lang" "$tier_scope"
    return 0
  fi
  printf 'FAIL  ⑧-a %s→%s: normal 格两腿不齐（leg1=%s leg2=%s）——只有腿一记未达标（§4.0.3 判据②；lang=%s tier_scope=%s）\n' "$atom_name" "$slot" "$leg1" "$leg2" "$lang" "$tier_scope"
  return 1
}

# 单格判定：status/tier_scope/lang 三轴 + 两腿计数。打印该格判词；返回 0=过、1=有 FAIL。
parity_slot_check() {
  local slots_toml="$1" mod_dir="$2" atom_dir="$3" atom_name="$4" slot="$5"
  local status tier_scope t lang rc=0 legs
  status="$(parity_slot_status "$slots_toml" "$slot")"
  if [ -z "$status" ]; then
    printf 'FAIL  ⑧-a %s→%s: 提供格在 slots.toml 无 status 记录（格表缺失即红，C9 门只读这一处）\n' "$atom_name" "$slot"
    return 1
  fi
  # 第二轴 tier_scope（L-2 扩读）：非空 + 档名在 host|rtos|bare 白名单内，缺/非法即红。
  tier_scope="$(parity_slot_tier "$slots_toml" "$slot")"
  if [ -z "$tier_scope" ]; then
    printf 'FAIL  ⑧-a %s→%s: 提供格在 slots.toml 缺 tier_scope 登记（两轴其一缺失即红，L-2 扩读）\n' "$atom_name" "$slot"
    return 1
  fi
  for t in $tier_scope; do
    case "$t" in
      host|rtos|bare) ;;
      *) printf 'FAIL  ⑧-a %s→%s: tier_scope 含非法档 "%s"（白名单 host|rtos|bare）\n' "$atom_name" "$slot" "$t"; rc=1 ;;
    esac
  done
  if [ "$status" = "retired" ]; then
    printf 'PASS  ⑧-a %s→%s: retired 退出分母（号位保留、义务解除；tier_scope=%s）\n' "$atom_name" "$slot" "$tier_scope"
    return $rc
  fi
  if [ "$status" = "placeholder" ]; then
    printf 'PASS  ⑧-a %s→%s: placeholder 记不适用（占位 ≠ 通过，登记填实触发条件；tier_scope=%s）\n' "$atom_name" "$slot" "$tier_scope"
    return $rc
  fi
  # 件形只认 rust（python 语言面随 ADR-0016 裁定 3 退役，批338）；判据 = module.toml
  # 数据位，不是 `[ ! -f mech.rs ]` 启发式。
  lang="$(mod_lang "$mod_dir/module.toml")"
  if [ "$lang" != "rust" ]; then
    printf 'FAIL  ⑧-a %s→%s: module.toml lang=%s（白名单 rust，缺/非法即 fail-closed）\n' "$atom_name" "$slot" "${lang:-<空>}"
    return 1
  fi
  # 件级精确归属：对拍函数名须带本件名前缀（fn parity_leg1_<件名>_*）——
  # 模块级粗扫会让同模块异件互充数（battery 没写、charger 写了也能过）。
  legs="$(parity_leg_counts "$mod_dir" "$atom_dir" "$atom_name")"
  parity_leg_verdict "$atom_name" "$slot" "$lang" "$tier_scope" "$legs" || rc=1
  return $rc
}

# 两腿对拍计数门（参数：<slots.toml> <modules根目录>；抽成函数供 C10 fixture 复用——
# 自证跑的必须是同一份逻辑，另抄一份就失去了自证意义）。返回 0=全过、
# 1=有 FAIL、2=门自身失效（枚举打空等）。
parity_gate() {
  local slots_toml="$1" mods_root="$2" atom_tomls rc=0
  atom_tomls="$(find "$mods_root" -path '*/atoms/*/atom.toml' -not -path '*/target/*' 2>/dev/null | sort)"
  if [ -z "$atom_tomls" ]; then
    echo "ERROR: parity_gate atom.toml 枚举打空（门禁失效，不放行）" >&2
    return 2
  fi
  local atom_toml atom_dir atom_name mod_dir slot
  while IFS= read -r atom_toml; do
    atom_dir="$(dirname "$atom_toml")"
    atom_name="$(basename "$atom_dir")"
    mod_dir="$(dirname "$(dirname "$atom_dir")")"
    # [provides] 段内的格名（段边界内取引号串；形状畸形归 atom.schema 门，此处不猜）。
    for slot in $(awk '/^\[provides\]/{p=1;next} /^\[/{p=0} p' "$atom_toml" \
                    | grep -oE '"[a-z][a-z0-9.]*"' | tr -d '"'); do
      parity_slot_check "$slots_toml" "$mod_dir" "$atom_dir" "$atom_name" "$slot" || rc=1
    done
  done <<< "$atom_tomls"
  return $rc
}

# C10 反例夹具（契约面与件面）：slots.toml 一格 normal + module.toml lang=rust +
# atom.toml 提供该格。件形判据是 module.toml `lang`（L-2）：fixture 须给数据位，
# 否则会先被 lang 门（而非对拍计数）拦下——自证要证明的是"对拍计数拦得住空件"。
c10_fixture_tree() {
  local c10_dir="$1"
  mkdir -p "$c10_dir/contracts" "$c10_dir/modules/rr3-stub/atoms/stub" "$c10_dir/modules/rr3-stub/src"
  cat > "$c10_dir/contracts/slots.toml" <<'EOF'
schema_version = 2
[[slot]]
name = "stub.slot"
layer = "body"
status = "normal"
tier_scope = ["host", "rtos", "bare"]
EOF
  cat > "$c10_dir/modules/rr3-stub/module.toml" <<'EOF'
[manifest]
name = "rr3-stub"
lang = "rust"
EOF
  cat > "$c10_dir/modules/rr3-stub/atoms/stub/atom.toml" <<'EOF'
schema_version = 1

[atom]
name = "stub"
version = "0.0.1"

[provides]
slots = ["stub.slot"]

[uses]
slots = []
EOF
}

# C10 反例夹具（机制面）：恒输出 0 的件 + 只有腿一的对拍，并置空机制实体文件。
c10_fixture_mech() {
  local c10_dir="$1"
  cat > "$c10_dir/modules/rr3-stub/atoms/stub/mech.rs" <<'EOF'
//! stub 反例件（C10 fixture）：恒输出 0 的机制 + 只有腿一的对拍。
fn rr3_stub_body() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn parity_leg1_stub_is_deterministic() {
        assert_eq!(super::rr3_stub_body(), 0);
    }
}
EOF
  : > "$c10_dir/modules/rr3-stub/src/composite.rs"
}

# ⑧-a C10 反例自证（先于正仓跑：门先证明自己拦得住空件）。判据不止"退非零"：
# 必须是**对拍计数**（两腿不齐）拦下的，不能被 lang/tier_scope/status 等旁门替它挡刀。
c10_selftest() {
  local c10_dir c10_out c10_rc
  c10_dir="$(mktemp -d)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
  c10_fixture_tree "$c10_dir"
  c10_fixture_mech "$c10_dir"
  c10_out="$(parity_gate "$c10_dir/contracts/slots.toml" "$c10_dir/modules" 2>&1)" && c10_rc=0 || c10_rc=$?
  if [ "$c10_rc" -eq 0 ] || ! printf '%s' "$c10_out" | grep -q 'normal 格两腿不齐'; then
    printf 'FAIL  ⑧-a C10 反例自证: stub 件（恒输出0+只有腿一）未被**对拍计数**拦下（rc=%s）——门对空件发奖即门禁失效\n' "$c10_rc"
    printf '%s\n' "$c10_out"
    gate_fail=1
  else
    printf 'PASS  ⑧-a C10 反例自证: stub 反例被拦（恒输出0+只有腿一 → 对拍计数记未达标，门红）\n'
  fi
  rm -rf "$c10_dir"
}

# ⑧-b 件级依赖闭包 grep 门（L0/L1，C7 分级）：行注释先剥、#[cfg(test)] 块整块剥；
# 命中 L0=std::（含声明与全限定内联）或 L1=+serde_json 即红。
closure_gate() {
  local atom_rs=0 f closure_hits
  for f in modules/*/atoms/*/*.rs; do [ -f "$f" ] && atom_rs=$((atom_rs + 1)); done
  if [ "$atom_rs" -eq 0 ]; then
    printf 'FAIL  ⑧-b atom 源文件枚举打空（门禁失效，不放行）\n'
    gate_fail=1
  else
    closure_hits="$(mktemp)" || { echo "ERROR: mktemp 失败。" >&2; exit 1; }
    awk '
      { line = $0; sub(/\/\/.*$/, "", line) }
      strip {
        n = gsub(/{/, "{", line); m = gsub(/}/, "}", line); depth += n - m
        if (depth <= 0 && n + m > 0) strip = 0
        next
      }
      line ~ /^[[:space:]]*#\[cfg\(test\)\]/ { strip = 1; depth = 0; next }
      line ~ /std::|extern crate std|use serde_json|serde_json::/ { print FILENAME ":" FNR ":" $0 }
    ' modules/*/atoms/*/*.rs >> "$closure_hits"
    if [ -s "$closure_hits" ]; then
      cat "$closure_hits"
      printf 'FAIL  ⑧-b 依赖闭包命中（L0=std::（含声明与全限定内联）；L1=+serde_json；非 test 面，见上行号）\n'
      gate_fail=1
    else
      printf 'PASS  ⑧-b 件级依赖闭包（%s 份 atom 源非 test 面，L0/L1 禁词零命中）\n' "$atom_rs"
    fi
    rm -f "$closure_hits"
  fi
}

# ⑧-c 注入面校验门（显式过滤器封改名/删除 fail-open；run_tests 内部已置全局 fail）。
injection_gate() {
  run_tests sh -c 'cd base/runtime && cargo test --locked -p rr-registry --test contract injection'
}

# ⑧ 步骤编排：自证 → 正仓对拍 → 依赖闭包 → 汇总 gate_fail → 注入面校验。
rr3_step_8() {
  gate_fail=0
  c10_selftest
  parity_gate contracts/slots.toml modules || gate_fail=1
  closure_gate
  if [ "$gate_fail" -ne 0 ]; then
    fail=1
  fi
  injection_gate
}
