# SPDX-License-Identifier: Apache-2.0
# ⑤ 文档门步骤体（空文档 / 相对断链 / 导航漏链 / docs/ 外散落 md；列表打空即门禁失效）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail via run。

check_docs() {
  local list bad nav f d l
  list="$(mktemp)" || return 1; bad="$(mktemp)" || return 1
  git -c core.quotepath=false ls-files '*.md' > "$list" || true
  if [ ! -s "$list" ]; then
    printf 'FAIL  ⑤ md 列表打空（ls-files 无结果属门禁失效，不放行）\n'
    rm -f "$list" "$bad"; return 1
  fi
  while IFS= read -r f; do
    [ -s "$f" ] || printf '空文档  %s\n' "$f" >> "$bad"
    d="$(dirname "$f")"
    while IFS= read -r l; do
      [ -e "$d/$l" ] || printf '断链    %s -> %s\n' "$f" "$l" >> "$bad"
    done < <(grep -ohE '\]\([^)]+\)' "$f" 2>/dev/null \
             | sed 's/^](//; s/)$//; s/#.*//; s/ .*//' \
             | grep -vE '^(https?:|mailto:|/|$)')
  done < "$list"
  # 导航覆盖：把 docs/README.md 里的相对链接按 docs/ 归一成仓根路径后，逐个 README 对号。
  # 本索引住在 docs/ 下，链接里的 `../` 只可能出现在开头，故一次折叠即够。
  nav="$(mktemp)" || return 1
  while IFS= read -r l; do
    printf '%s\n' "docs/$l" | sed 's#^docs/\./##; s#^docs/\.\./##' >> "$nav"
  done < <(grep -ohE '\]\([^)]+\)' docs/README.md 2>/dev/null \
           | sed 's/^](//; s/)$//; s/#.*//; s/ .*//' \
           | grep -vE '^(https?:|mailto:|/|$)')
  if [ ! -s "$nav" ]; then
    printf 'FAIL  ⑤ 导航链接列表打空（docs/README.md 读不到链接属门禁失效，不放行）\n'
    rm -f "$list" "$bad" "$nav"; return 1
  fi
  while IFS= read -r f; do
    case "$f" in
      *README.md) [ "$f" = "docs/README.md" ] || grep -qxF "$f" "$nav" \
                    || printf '导航漏链  %s\n' "$f" >> "$bad" ;;
    esac
  done < "$list"
  # 第四判据（2026-11-25 批46 裁定）：docs/ 之外每目录至多一份 md 且必须是 README.md，
  # 专题文档一律归 docs/design 或 docs/reference。豁免两类：base/os/（Buildroot 外部树
  # 与板卡笔记随外树布局走）；仓根 README_zh.md（开源双语说明）。
  while IFS= read -r f; do
    case "$f" in docs/*|base/os/*) continue ;; esac
    case "$(basename "$f")" in
      README.md) ;;
      README_zh.md) [ "$(dirname "$f")" = "." ] \
                      || printf '散落文档  %s\n' "$f" >> "$bad" ;;
      *) printf '散落文档  %s\n' "$f" >> "$bad" ;;
    esac
  done < "$list"
  if [ -s "$bad" ]; then cat "$bad"; rm -f "$list" "$bad" "$nav"; return 1; fi
  rm -f "$list" "$bad" "$nav"
}

rr3_step_5() {
  run check_docs
}

# 自检（2026-11-24，2026-11-25 批46 补第四判据三例）：夹具仓七例——好档通过 / 空文档红 /
# 断链红 / 导航漏链红 / docs/ 外散落 md 红 / base/os 豁免通过 / 仓根 README_zh 豁免通过。
# 文档门的判据（列表打空、链接归一、README 覆盖）腐化时先在这里红，而不是在
# 真仓上静默放行。调用方见 tools/ci/platform/docs_gate_selftest.sh。
check_docs_selftest() {
  local root td ok=1 d out rc
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" || return 1
  td="$(mktemp -d)" || return 1

  mk_case() {
    d="$td/$1"
    mkdir -p "$d/docs/sub" || return 1
    ( cd "$d" && git init -q . ) || return 1
    printf '# 索引\n\n[a](sub/a.md)\n' > "$d/docs/README.md"
    printf '# A\n' > "$d/docs/sub/a.md"
    printf '%s\n' "$d"
  }
  expect() {
    rc=0
    out="$( ( cd "$1" && . "$root/5.sh" && check_docs ) 2>&1 )" || rc=$?
    rc="${rc:-0}"
    if { [ "$2" = 0 ] && [ "$rc" -eq 0 ]; } \
       || { [ "$2" = 1 ] && [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -qF "$3"; }; then
      echo "PASS  ⑤ 自检：$4"
      rc=0
    else
      echo "FAIL  ⑤ 自检：${4}（rc=${rc}）: $(printf '%s' "$out" | head -2 | tr '\n' ' ')" >&2
      ok=0
      rc=1
    fi
    return "$rc"
  }

  mk_case good >/dev/null && ( cd "$td/good" && git add -A ) && expect "$td/good" 0 "" "好档通过" || ok=0
  mk_case empty >/dev/null && : > "$td/empty/docs/sub/empty.md" \
    && ( cd "$td/empty" && git add -A ) && expect "$td/empty" 1 "空文档" "空文档即红" || ok=0
  mk_case broken >/dev/null && printf '[x](missing.md)\n' > "$td/broken/docs/sub/b.md" \
    && ( cd "$td/broken" && git add -A ) && expect "$td/broken" 1 "断链" "断链即红" || ok=0
  mk_case nav >/dev/null && printf '# Sub\n' > "$td/nav/docs/sub/README.md" \
    && ( cd "$td/nav" && git add -A ) && expect "$td/nav" 1 "导航漏链" "导航漏链即红" || ok=0
  mk_case scatter >/dev/null && mkdir -p "$td/scatter/tools" \
    && printf '# 笔记\n' > "$td/scatter/tools/notes.md" \
    && ( cd "$td/scatter" && git add -A ) && expect "$td/scatter" 1 "散落文档" "docs/ 外散落 md 即红" || ok=0
  mk_case exempt >/dev/null && mkdir -p "$td/exempt/base/os/boards" \
    && printf '# 板卡\n' > "$td/exempt/base/os/boards/pi.md" \
    && ( cd "$td/exempt" && git add -A ) && expect "$td/exempt" 0 "" "base/os 豁免通过" || ok=0
  mk_case zh >/dev/null && printf '# zh\n' > "$td/zh/README_zh.md" \
    && ( cd "$td/zh" && git add -A ) && expect "$td/zh" 0 "" "仓根 README_zh 豁免通过" || ok=0

  rm -rf "$td"
  [ "$ok" -eq 1 ]
}
