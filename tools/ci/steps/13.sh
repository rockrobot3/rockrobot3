# ⑬ 仓面卫生门（门 13 步骤体：追踪面本机路径 / 构建目录 / 未入册源码 /
#    shell 展开截断；扫描器错误按 fail-closed 计红）。
# 由 check.sh 在启动段 source；函数读写入口的全局 fail 与 root。

rr3_step_13() {
  local self="${BASH_SOURCE[0]}"
  case "$self" in "$root"/*) self="${self#"$root"/}" ;; esac
  hyg_fail=0
  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    hy_path="$(git grep -IE -n \
      -e '/Users/[A-Za-z0-9._-]+/' \
      -e 'trae_projects' \
      -e 'C:\\\\Users\\\\' \
      -- . ':(exclude)'"$self" ':(exclude)docs/process/conventions.md' \
          ':(exclude)docs/process/conventions-receipts.md' \
      2>&1)"
    hy_rc=$?
    case "$hy_rc" in
      0)
        printf 'FAIL  ⑬ 追踪文件含本机开发路径（规则 7-b：不得暴露本地路径）:\n%s\n' \
          "$(printf '%s\n' "$hy_path" | head -5)"
        hyg_fail=1
        ;;
      1)
        printf 'PASS  ⑬ 追踪文件无本机开发路径（/Users/<name>/、trae_projects、C:\\Users\\）\n'
        ;;
      *)
        # 工具错误不得当"无命中"放行（fail-closed：门自己坏掉也是红）
        printf 'FAIL  ⑬ 本机路径扫描器执行失败（git grep 退出码 %s）:\n%s\n' \
          "$hy_rc" "$(printf '%s\n' "$hy_path" | head -3)"
        hyg_fail=1
        ;;
    esac
    hy_dir="$(git ls-files | grep -E '(^|/)(target|build|node_modules)/' | head -5)"
    if [ -n "$hy_dir" ]; then
      printf 'FAIL  ⑬ 构建目录被追踪（源码区严禁构建产物）:\n%s\n' "$hy_dir"
      hyg_fail=1
    else
      printf 'PASS  ⑬ 追踪面无构建目录（target/build/node_modules）\n'
    fi
    # 未入册源码门（B8 审计：⑫ 度量只读 `git ls-files`，新源码没 `git add` 就
    # 不进度量、也不进构建——白名单策略下它连 `git status` 都不显眼，属静默漏面。
    # 这里只点源码类扩展名，忽略目录（.trae/ 等）不误报）。
    hy_new="$(git status --porcelain --untracked-files=normal -- \
      '*.rs' '*.toml' '*.py' '*.html' '*.sh' 2>&1)"
    hy_rc=$?
    if [ "$hy_rc" -ne 0 ]; then
      printf 'FAIL  ⑬ 未入册源码扫描器执行失败（git status 退出码 %s）:\n%s\n' \
        "$hy_rc" "$(printf '%s\n' "$hy_new" | head -3)"
      hyg_fail=1
    else
      hy_new="$(printf '%s\n' "$hy_new" | grep '^??' | head -5)"
      if [ -n "$hy_new" ]; then
        printf 'FAIL  ⑬ 源码未入册（度量/构建会漏，先 git add）:\n%s\n' "$hy_new"
        hyg_fail=1
      else
        printf 'PASS  ⑬ 源码面无非入册新件（rs/toml/py/html/sh）\n'
      fi
    fi
    # shell 展开截断门（2026-11-15 实测补齐）：`$name` 紧跟非 ASCII 字节时，bash 3.2
    # （macOS 默认 /bin/bash）在 C locale 下把多字节首字节并进变量名，`set -u` 当场
    # unbound——stage-image.sh 末行 `$out（arch=…` 即在开发机退 1，而 CI 的 bash 5
    # 不复现（本地红、CI 绿，比反向更难查）。判据：可执行行（整行注释跳过）里出现
    # `$标识符` 紧跟非 ASCII 即红；写法统一 `${标识符}`，语义完全不变。
    hy_sh="$(git -c core.quotepath=false ls-files '*.sh' | while IFS= read -r f; do
      LC_ALL=C awk -v F="$f" '
        { line=$0; sub(/^[ \t]+/,"",line); if (line ~ /^#/) next }
        match($0, /\$[A-Za-z_][A-Za-z0-9_]*/) {
          nxt = substr($0, RSTART+RLENGTH, 1);
          if (nxt != "" && nxt !~ /^[ -~]$/) print F ": " $0;
        }' "$f" 2>/dev/null
    done | head -5)"
    if [ -n "$hy_sh" ]; then
      printf 'FAIL  ⑬ shell 变量展开后紧跟非 ASCII（bash 3.2 下 unbound，须写 ${name}）:\n%s\n' "$hy_sh"
      hyg_fail=1
    else
      printf 'PASS  ⑬ shell 展开截断（$name 后接 ASCII 或写成 ${name}）\n'
    fi
  else
    printf 'SKIP  ⑬ 非 git 工作树——仓面卫生门不执行\n'
  fi
  if [ "$hyg_fail" -ne 0 ]; then
    fail=1
  fi
}
