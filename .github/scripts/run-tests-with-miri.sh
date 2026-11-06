#!/usr/bin/env bash
#
# Parallelizes the integration tests when running with Miri#
#
# Usage: .github/scripts/run-tests-with-miri.sh
# Env:   MIRI_TEST_TIMEOUT_SECS (default 300)
#        MIRI_TEST_JOBS (default: nproc)
#        MIRI_TEST_TARGETS (space-separated. default: lib core_integration
#                           regression_integration custom_page_sizes_integration)
#        MIRIFLAGS (passed to `cargo miri test`)
set -uo pipefail

manifest_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$manifest_dir"

cargo miri setup

timeout_secs="${MIRI_TEST_TIMEOUT_SECS:-300}"
jobs="${MIRI_TEST_JOBS:-$(nproc)}"

target_flag() {
  case "$1" in
  lib) echo "--lib" ;;
  *) echo "--test $1" ;;
  esac
}
export -f target_flag

# shellcheck disable=SC2206
targets=(${MIRI_TEST_TARGETS:-lib core_integration regression_integration custom_page_sizes_integration})

pairs_file="$(mktemp)"
results_file="$(mktemp)"
skipped_file="$(mktemp)"
trap 'rm -f "$pairs_file" "$results_file" "$skipped_file"' EXIT

list_tests() {
  RUSTFLAGS="--cfg miri" cargo test --quiet $2 -- --list $1 2>&1
}

for label in "${targets[@]}"; do
  target="$(target_flag "$label")"

  if ! listing="$(list_tests '' "$target")"; then
    echo "!!! could not enumerate tests for target '${label}'" >&2
    echo "$listing" >&2
    exit 1
  fi
  if ! ignored_listing="$(list_tests '--ignored' "$target")"; then
    echo "!!! could not enumerate ignored tests for target '${label}'" >&2
    echo "$ignored_listing" >&2
    exit 1
  fi

  extract_names() { printf '%s\n' "$1" | grep ': test$' | sed 's/: test$//' || true; }

  names="$(extract_names "$listing")"
  ignored="$(extract_names "$ignored_listing")"

  if [ -z "$names" ]; then
    echo "!!! target '${label}' enumerated zero tests" >&2
    echo "    (stale MIRI_TEST_TARGETS entry?)" >&2
    exit 1
  fi

  if [ -n "$ignored" ]; then
    printf '%s\n' "$ignored" | sed "s/^/${label}\t/" >>"$skipped_file"
    # Keep only names absent from the ignored set.
    names="$(comm -23 <(printf '%s\n' "$names" | sort) <(printf '%s\n' "$ignored" | sort))"
  fi

  if [ -n "$names" ]; then
    printf '%s\n' "$names" | sed "s/^/${label}\t/" >>"$pairs_file"
  fi
done

total=$(wc -l <"$pairs_file")
skipped=$(wc -l <"$skipped_file")
echo "Running $total tests ($skipped ignored under Miri):"
echo "Executors: $jobs"
echo "Timeout: ${timeout_secs}s"

export TIMEOUT_SECS="$timeout_secs"

run_one() {
  local label="$1" name="$2"
  local target
  target="$(target_flag "$label")"

  local out status
  out="$(timeout "$TIMEOUT_SECS" cargo miri test --features miri-soft-floats $target -- --exact "$name" 2>&1)"
  status=$?

  if [ "$status" -eq 0 ]; then
    # `--exact` with a name libtest cannot match is a *successful* run of zero
    # tests, so a zero exit status alone does not distinguish "passed" from
    # "never ran". libtest prints "running 0 tests" in exactly that case; a
    # test that is present but ignored still prints "running 1 test".
    if printf '%s\n' "$out" | grep -q '^running 0 tests$'; then
      printf 'NOTRUN\t%s\t%s\n' "$label" "$name"
      echo "!!! DID NOT RUN: $label :: $name" >&2
      echo "$out" >&2
    else
      printf 'PASS\t%s\t%s\n' "$label" "$name"
    fi
  elif [ "$status" -eq 124 ]; then
    printf 'TIMEOUT\t%s\t%s\n' "$label" "$name"
    echo "!!! TIMED OUT after ${TIMEOUT_SECS}s: $label :: $name" >&2
  else
    printf 'FAIL\t%s\t%s\n' "$label" "$name"
    echo "!!! FAILED: $label :: $name" >&2
    echo "$out" >&2
  fi
}
export -f run_one

xargs_status=0
xargs -P "$jobs" -L1 bash -c 'run_one "$@"' _ <"$pairs_file" >>"$results_file" || xargs_status=$?

pass_count=$(grep -c '^PASS' "$results_file" || true)
timeout_lines=$(grep '^TIMEOUT' "$results_file" || true)
fail_lines=$(grep '^FAIL' "$results_file" || true)
notrun_lines=$(grep '^NOTRUN' "$results_file" || true)
# Every enumerated test must have produced a line above. A worker that dies
# before it prints one — an OOM kill, a cancelled job, xargs failing to fork —
# is otherwise counted as neither a pass nor a failure.
accounted=$(wc -l <"$results_file")
unaccounted=$(comm -23 <(sort "$pairs_file") <(cut -f2,3 "$results_file" | sort))

echo
echo "$pass_count / $total tests passed."

if [ "$skipped" -gt 0 ]; then
  echo "Ignored under Miri ($skipped, not run):"
  awk -F'\t' '{print "  - " $1 " :: " $2}' "$skipped_file"
fi
if [ -n "$timeout_lines" ]; then
  echo "Timed out:"
  echo "$timeout_lines" | awk -F'\t' '{print "  - " $2 " :: " $3}'
fi
if [ -n "$fail_lines" ]; then
  echo "Failed:"
  echo "$fail_lines" | awk -F'\t' '{print "  - " $2 " :: " $3}'
fi
if [ -n "$notrun_lines" ]; then
  echo "Did not run (filter matched no test):"
  echo "$notrun_lines" | awk -F'\t' '{print "  - " $2 " :: " $3}'
fi
if [ -n "$unaccounted" ]; then
  echo "No result reported ($((total - accounted)) of $total):"
  echo "$unaccounted" | awk -F'\t' '{print "  - " $1 " :: " $2}'
fi
if [ "$xargs_status" -ne 0 ]; then
  echo "!!! xargs exited $xargs_status" >&2
fi

if [ -n "$timeout_lines" ] || [ -n "$fail_lines" ] || [ -n "$notrun_lines" ] ||
  [ -n "$unaccounted" ] || [ "$xargs_status" -ne 0 ]; then
  exit 1
fi
