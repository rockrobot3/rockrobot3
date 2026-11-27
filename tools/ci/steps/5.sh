# SPDX-License-Identifier: Apache-2.0
# ⑤ 文档门步骤体（空文档 / 相对断链 / 导航漏链 / 散落 md：docs/ 根只留三件、docs/ 外只留 README；
# 列表打空即门禁失效）。由 check.sh 在启动段 source；函数读写入口的全局 fail via run。
#
# 2026-11-27 批134：check_docs（62 行）与 check_docs_selftest（52 行）按判据拆组——
# 链接/导航/散落三组各一函数，自检夹具与断言提到文件作用域（可被两组自检共用）。

# —— 文档门三组判据（fail 面写入调用方给的 bad 文件）——

# 空文档 + 相对断链：每个追踪 md 的 `](目标)` 逐个按所在目录解析，排除外链与锚点。
docs_links() {
  local list="$1" bad="$2" f d l
  while IFS= read -r f; do
    [ -s "$f" ] || printf '空文档  %s\n' "$f" >> "$bad"
    d="$(dirname "$f")"
    while IFS= read -r l; do
      [ -e "$d/$l" ] || printf '断链    %s -> %s\n' "$f" "$l" >> "$bad"
    done < <(grep -ohE '\]\([^)]+\)' "$f" 2>/dev/null \
             | sed 's/^](//; s/)$//; s/#.*//; s/ .*//' \
             | grep -vE '^(https?:|mailto:|/|$)')
  done < "$list"
}

# 导航覆盖：把 docs/README.md 里的相对链接按 docs/ 归一成仓根路径后，逐个 README 对号。
# 本索引住在 docs/ 下，链接里的 `../` 只可能出现在开头，故一次折叠即够。
docs_nav() {
  local list="$1" bad="$2" nav f l
  nav="$(mktemp)" || return 1
  while IFS= read -r l; do
    printf '%s\n' "docs/$l" | sed 's#^docs/\./##; s#^docs/\.\./##' >> "$nav"
  done < <(grep -ohE '\]\([^)]+\)' docs/README.md 2>/dev/null \
           | sed 's/^](//; s/)$//; s/#.*//; s/ .*//' \
           | grep -vE '^(https?:|mailto:|/|$)')
  if [ ! -s "$nav" ]; then
    printf 'FAIL  ⑤ 导航链接列表打空（docs/README.md 读不到链接属门禁失效，不放行）\n'
    rm -f "$nav"
    return 1
  fi
  while IFS= read -r f; do
    case "$f" in
      *README.md) [ "$f" = "docs/README.md" ] || grep -qxF "$f" "$nav" \
                    || printf '导航漏链  %s\n' "$f" >> "$bad" ;;
    esac
  done < "$list"
  rm -f "$nav"
}

# 散落面：第四判据（2026-11-25 批46）docs/ 之外每目录至多一份 md 且必须是 README.md；
# 第五判据（2026-11-27 批59）docs/ 根只允许索引与实况三件。豁免两类：base/os/
# （Buildroot 外部树与板卡笔记随外树布局走）、仓根 README_zh.md（开源双语说明）。
docs_scatter() {
  local list="$1" bad="$2" f
  while IFS= read -r f; do
    case "$f" in
      docs/*/*) continue ;;
      docs/*)
        case "$(basename "$f")" in
          README.md|status.md|status-history.md) ;;
          *) printf '散落文档  %s\n' "$f" >> "$bad" ;;
        esac
        continue ;;
      base/os/*) continue ;;
    esac
    case "$(basename "$f")" in
      README.md) ;;
      README_zh.md) [ "$(dirname "$f")" = "." ] \
                      || printf '散落文档  %s\n' "$f" >> "$bad" ;;
      *) printf '散落文档  %s\n' "$f" >> "$bad" ;;
    esac
  done < "$list"
}

check_docs() {
  local list bad rc=0
  list="$(mktemp)" || return 1; bad="$(mktemp)" || return 1
  git -c core.quotepath=false ls-files '*.md' > "$list" || true
  if [ ! -s "$list" ]; then
    printf 'FAIL  ⑤ md 列表打空（ls-files 无结果属门禁失效，不放行）\n'
    rm -f "$list" "$bad"; return 1
  fi
  docs_links "$list" "$bad"
  docs_nav "$list" "$bad" || { rm -f "$list" "$bad"; return 1; }
  docs_scatter "$list" "$bad"
  if [ -s "$bad" ]; then cat "$bad"; rc=1; fi
  rm -f "$list" "$bad"
  return "$rc"
}

rr3_step_5() {
  run check_docs
}

# —— 自检（夹具仓，判据腐化先在这里红）——
# 自检夹具：建一个最小 git 仓（docs/README.md + docs/sub/a.md），回显目录。
rr3_docs_case() {
  local td="$1" name="$2" d
  d="$td/$name"
  mkdir -p "$d/docs/sub" || return 1
  ( cd "$d" && git init -q . ) || return 1
  printf '# 索引\n\n[a](sub/a.md)\n' > "$d/docs/README.md"
  printf '# A\n' > "$d/docs/sub/a.md"
  printf '%s\n' "$d"
}

# 跑一次 check_docs 并对结论/点名断言：<root> <目录> <期望rc> <点名子串> <标签>。
rr3_docs_expect() {
  local root="$1" dir="$2" want="$3" needle="$4" label="$5" rc out
  rc=0
  out="$( ( cd "$dir" && . "$root/5.sh" && check_docs ) 2>&1 )" || rc=$?
  rc="${rc:-0}"
  if { [ "$want" = 0 ] && [ "$rc" -eq 0 ]; } \
     || { [ "$want" = 1 ] && [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -qF "$needle"; }; then
    echo "PASS  ⑤ 自检：${label}"
    return 0
  fi
  echo "FAIL  ⑤ 自检：${label}（rc=${rc}）: $(printf '%s' "$out" | head -2 | tr '\n' ' ')" >&2
  return 1
}

# 核心三判据六例：好档 / 空文档 / 断链 / 导航漏链 / docs 外散落 / docs 根散落。
docs_selftest_core() {
  local root="$1" td="$2" ok=1
  rr3_docs_case "$td" good >/dev/null && ( cd "$td/good" && git add -A ) \
    && rr3_docs_expect "$root" "$td/good" 0 "" "好档通过" || ok=0
  rr3_docs_case "$td" empty >/dev/null && : > "$td/empty/docs/sub/empty.md" \
    && ( cd "$td/empty" && git add -A ) \
    && rr3_docs_expect "$root" "$td/empty" 1 "空文档" "空文档即红" || ok=0
  rr3_docs_case "$td" broken >/dev/null && printf '[x](missing.md)\n' > "$td/broken/docs/sub/b.md" \
    && ( cd "$td/broken" && git add -A ) \
    && rr3_docs_expect "$root" "$td/broken" 1 "断链" "断链即红" || ok=0
  rr3_docs_case "$td" nav >/dev/null && printf '# Sub\n' > "$td/nav/docs/sub/README.md" \
    && ( cd "$td/nav" && git add -A ) \
    && rr3_docs_expect "$root" "$td/nav" 1 "导航漏链" "导航漏链即红" || ok=0
  rr3_docs_case "$td" scatter >/dev/null && mkdir -p "$td/scatter/tools" \
    && printf '# 笔记\n' > "$td/scatter/tools/notes.md" \
    && ( cd "$td/scatter" && git add -A ) \
    && rr3_docs_expect "$root" "$td/scatter" 1 "散落文档" "docs/ 外散落 md 即红" || ok=0
  rr3_docs_case "$td" droot >/dev/null && printf '# 笔记\n' > "$td/droot/docs/notes.md" \
    && ( cd "$td/droot" && git add -A ) \
    && rr3_docs_expect "$root" "$td/droot" 1 "散落文档" "docs/ 根散落 md 即红" || ok=0
  [ "$ok" -eq 1 ]
}

# 豁免三例：docs/status.md / base/os 板卡笔记 / 仓根 README_zh.md 必须放行。
docs_selftest_exempt() {
  local root="$1" td="$2" ok=1
  rr3_docs_case "$td" dstat >/dev/null && printf '# 实况\n' > "$td/dstat/docs/status.md" \
    && ( cd "$td/dstat" && git add -A ) \
    && rr3_docs_expect "$root" "$td/dstat" 0 "" "docs/status.md 豁免通过" || ok=0
  rr3_docs_case "$td" exempt >/dev/null && mkdir -p "$td/exempt/base/os/boards" \
    && printf '# 板卡\n' > "$td/exempt/base/os/boards/pi.md" \
    && ( cd "$td/exempt" && git add -A ) \
    && rr3_docs_expect "$root" "$td/exempt" 0 "" "base/os 豁免通过" || ok=0
  rr3_docs_case "$td" zh >/dev/null && printf '# zh\n' > "$td/zh/README_zh.md" \
    && ( cd "$td/zh" && git add -A ) \
    && rr3_docs_expect "$root" "$td/zh" 0 "" "仓根 README_zh 豁免通过" || ok=0
  [ "$ok" -eq 1 ]
}

# 自检总入口：九例（核心六 + 豁免三），任一红即红。调用方见
# tools/ci/platform/docs_gate_selftest.sh。
check_docs_selftest() {
  local root td ok=1
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || return 1
  td="$(mktemp -d)" || return 1
  docs_selftest_core "$root" "$td" || ok=0
  docs_selftest_exempt "$root" "$td" || ok=0
  rm -rf "$td"
  [ "$ok" -eq 1 ]
}
