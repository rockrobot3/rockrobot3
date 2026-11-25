#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# 发布点执法（机制件）：tag 触发的发布必须同时满足两条——
#   ① tag 与仓根 VERSION 同源：`v<VERSION>`，预发布只许再带 `-rc<N>` 后缀；
#   ② tag 指向的提交是 release 提交：标题以 `release(v<VERSION>)` 起头。
# VERSION 只能在 release 提交内变更，"随手升 VERSION 再打 tag"在此被拦下。
# 非 tag 事件（workflow_dispatch / 分支推送）不做版本判定：手输标签原样给、分支给 edge——
# 那两条路径不发 Release。
#
# 用法（在仓根执行；向标准输出打两行 `tag=` / `version=`，供 GITHUB_OUTPUT）：
#   bash tools/ci/release-tag.sh <事件名> <ref> <ref 名> <手输标签>
# 前三参取 GITHUB_EVENT_NAME / GITHUB_REF / GITHUB_REF_NAME，第四参是 workflow_dispatch
# 的 inputs.tag（其余事件传空）。任一判据不成立即非 0 退出，不打输出。
rr3_release_tag() {
  local event="$1" ref="$2" ref_name="$3" dispatch_tag="${4:-}" ver tag base subject
  ver="$(tr -d '[:space:]' < VERSION)"
  if [[ ! "$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    printf 'ERROR: VERSION 不是 X.Y.Z：%s\n' "$ver" >&2
    return 1
  fi
  if [ "$event" = "workflow_dispatch" ]; then
    tag="$dispatch_tag"
  elif [[ "$ref" == refs/tags/* ]]; then
    tag="$ref_name"
    base="${tag%-rc*}"
    subject="$(git log -1 --format=%s)"
    if [ "$base" != "v$ver" ]; then
      printf '::error title=version::tag %s 与 VERSION %s 不同源（VERSION 只能在 release(vX.Y.Z) 提交内变更，随后打同名 tag）\n' \
        "$tag" "$ver" >&2
      return 1
    fi
    if [[ "$subject" != "release(v$ver)"* ]]; then
      printf '::error title=version::tag %s 未指向 release 提交（HEAD 标题：%s；须以 release(v%s) 起头）\n' \
        "$tag" "$subject" "$ver" >&2
      return 1
    fi
  else
    tag=edge
  fi
  printf 'tag=%s\nversion=%s\n' "$tag" "$ver"
}

# 自检夹具仓：只含 VERSION 与一两个提交，git 身份只对本命令生效。
rr3_release_fixture() {
  local dir="$1"
  mkdir "$dir" || return 1
  git -C "$dir" init -q || return 1
  printf '0.0.8\n' >"$dir/VERSION" || return 1
  git -C "$dir" add VERSION || return 1
  git -C "$dir" -c user.name=selftest -c user.email=selftest@invalid commit -q -m 'release(v0.0.8)：自检基线' || return 1
}

# 逐例判定：<期望 ok|bad> <例名> <四参…>；输出只打一行 PASS/FAIL。
rr3_release_case() {
  local want="$1" name="$2" out
  shift 2
  if out="$(rr3_release_tag "$@" 2>/dev/null)"; then
    if [ "$want" = ok ] && printf '%s\n' "$out" | grep -q '^version=0.0.8$'; then
      printf 'PASS  %s\n' "$name"; return 0
    fi
    printf 'FAIL  %s（应拒却放行：%s）\n' "$name" "$out"; return 1
  fi
  if [ "$want" = bad ]; then printf 'PASS  %s\n' "$name"; return 0; fi
  printf 'FAIL  %s（应放行却拒绝：%s）\n' "$name" "$out"
  return 1
}

rr3_release_selftest() {
  local tmp ok=0
  tmp="$(mktemp -d)" || return 1
  (
    cd "$tmp" || exit 1
    rr3_release_fixture "$tmp/repo" || exit 1
    cd "$tmp/repo" || exit 1
    rr3_release_case ok  '正式 tag 指向 release 提交' push refs/tags/v0.0.8 v0.0.8 '' || ok=1
    rr3_release_case bad 'tag 与 VERSION 不同源'       push refs/tags/v0.0.9 v0.0.9 '' || ok=1
    rr3_release_case ok  'rc tag 仍指向 release 提交'  push refs/tags/v0.0.8-rc1 v0.0.8-rc1 '' || ok=1
    rr3_release_case ok  '手输标签原样给'              workflow_dispatch refs/heads/dev/hubs-01 dev/hubs-01 1.2.3 || ok=1
    rr3_release_case ok  '分支推送给 edge'            push refs/heads/dev/hubs-01 dev/hubs-01 '' || ok=1
    git -c user.name=selftest -c user.email=selftest@invalid commit -q --allow-empty -m 'chore: 随手提交' || exit 1
    rr3_release_case bad 'tag 未指向 release 提交'    push refs/tags/v0.0.8 v0.0.8 '' || ok=1
    exit "$ok"
  )
  local rc=$?
  rm -rf "$tmp"
  return "$rc"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  if [ "${1:-}" = "--self-test" ]; then
    rr3_release_selftest
    exit $?
  fi
  rr3_release_tag "$@"
fi
