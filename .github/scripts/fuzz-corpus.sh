#!/usr/bin/env bash
#
# Syncs the fuzzing corpus with the `fuzz-corpus` draft release.
#
# The release is kept as a *draft* on purpose: the repository has immutable
# releases enabled, so a published release's assets could never be replaced.
# Never publish it. If it is published by accident, delete it and run `init`
# with a new FUZZ_CORPUS_TAG (an immutable release's tag cannot be reused).
#
# Each fuzz target is stored as its own `<target>.tar.gz` asset, holding the
# `<target>/` directory of `fuzz/corpus`. libFuzzer names corpus files by the
# hash of their contents, so unpacking one corpus over another is a union.
#
# Usage: .github/scripts/fuzz-corpus.sh init
#        .github/scripts/fuzz-corpus.sh pull [TARGET...]
#        .github/scripts/fuzz-corpus.sh push [TARGET...]
#
#   init  Create the draft release (once per repository).
#   pull  Download the stored corpus and merge it into fuzz/corpus/<target>.
#   push  Pull, minimize the merged corpus with `cargo fuzz cmin`, and upload
#         it, replacing the stored one. Inputs found by anyone who pushed since
#         your last pull are kept, not overwritten.
#
# With no TARGET, every target from `cargo fuzz list` is used.
#
# Env:   FUZZ_CORPUS_TAG (default: fuzz-corpus)
#        FUZZ_TOOLCHAIN  (default: nightly) toolchain for `cargo fuzz`
#        SPACEWASM_TARGET (optional) target triple passed to `cargo fuzz cmin`
#        GH_TOKEN / GH_REPO are honored by `gh` as usual. Reading a draft
#        release needs write access to the repository.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

tag="${FUZZ_CORPUS_TAG:-fuzz-corpus}"
toolchain="${FUZZ_TOOLCHAIN:-nightly}"
corpus_dir="fuzz/corpus"

# Corpus inputs are plain bytes, so archives carry no file metadata. On macOS,
# this keeps tar from adding `._*` AppleDouble files (which would unpack as
# bogus corpus entries) and `com.apple.*` xattr headers (which GNU tar warns
# about).
export COPYFILE_DISABLE=1

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

usage() {
  sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed -e '$d' -e 's/^# \{0,1\}//'
  exit 2
}

# Fails unless the release exists and is still a draft.
check_release() {
  local draft
  if ! draft=$(gh release view "$tag" --json isDraft --jq .isDraft 2>/dev/null); then
    echo "error: release '$tag' not found (or not visible to this token)." >&2
    echo "Run '$0 init' to create it. Draft releases are only visible with write access." >&2
    exit 1
  fi
  if [ "$draft" != "true" ]; then
    echo "error: release '$tag' is published, so its assets are immutable." >&2
    echo "Delete it and re-run init with a new FUZZ_CORPUS_TAG." >&2
    exit 1
  fi
}

# Number of inputs in fuzz/corpus/<target> (0 if it does not exist yet).
count() {
  if [ -d "$corpus_dir/$1" ]; then
    find "$corpus_dir/$1" -type f | wc -l | tr -d ' '
  else
    echo 0
  fi
}

targets() {
  if [ "$#" -gt 0 ]; then
    printf '%s\n' "$@"
  else
    cargo "+$toolchain" fuzz list
  fi
}

cmd_init() {
  if gh release view "$tag" >/dev/null 2>&1; then
    check_release
    echo "Draft release '$tag' already exists."
    return
  fi
  gh release create "$tag" --draft \
    --title "Fuzzing corpus (CI only, do not publish)" \
    --notes "Minimized libFuzzer corpus used by CI, one \`<target>.tar.gz\` per fuzz target. Managed by \`.github/scripts/fuzz-corpus.sh\`.

**Keep this a draft.** This repository uses immutable releases: once published, these assets can no longer be updated."
  echo "Created draft release '$tag'."
}

# Merges the stored corpus for each target into fuzz/corpus/<target>.
# A target with no stored corpus yet is skipped.
pull_targets() {
  local target
  mkdir -p "$corpus_dir"
  for target in "$@"; do
    if ! gh release download "$tag" --pattern "$target.tar.gz" --dir "$tmp" --clobber 2>/dev/null; then
      echo "$target: no stored corpus, skipping."
      continue
    fi
    local before
    before=$(count "$target")
    tar -xzf "$tmp/$target.tar.gz" -C "$corpus_dir" --exclude '._*'
    echo "$target: pulled, $before -> $(count "$target") inputs."
  done
}

cmd_pull() {
  check_release
  local list
  list=$(targets "$@")
  # shellcheck disable=SC2086
  pull_targets $list
}

cmd_push() {
  check_release
  local list target
  list=$(targets "$@")
  # shellcheck disable=SC2086
  pull_targets $list
  for target in $list; do
    if [ "$(count "$target")" -eq 0 ]; then
      echo "$target: empty corpus, skipping."
      continue
    fi
    echo "$target: minimizing..."
    cargo "+$toolchain" fuzz cmin "$target" ${SPACEWASM_TARGET:+--target "$SPACEWASM_TARGET"} >"$tmp/$target.cmin.log" 2>&1 || {
      tail -20 "$tmp/$target.cmin.log" >&2
      echo "error: cargo fuzz cmin $target failed." >&2
      exit 1
    }
    tar --no-xattrs -czf "$tmp/$target.tar.gz" -C "$corpus_dir" "$target"
    gh release upload "$tag" "$tmp/$target.tar.gz" --clobber
    echo "$target: pushed $(count "$target") inputs" \
      "($(du -h "$tmp/$target.tar.gz" | cut -f1 | tr -d ' ') compressed)."
  done
}

[ "$#" -ge 1 ] || usage
cmd="$1"
shift
case "$cmd" in
  init) cmd_init ;;
  pull) cmd_pull "$@" ;;
  push) cmd_push "$@" ;;
  *) usage ;;
esac
