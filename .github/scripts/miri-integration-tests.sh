#!/usr/bin/env bash
#
# Parallelizes the integration tests when running with Miri#
#
# Usage: .github/scripts/miri-integration-tests.sh
# Env:   MIRI_TEST_TIMEOUT_SECS (default 300)
#        MIRI_TEST_JOBS (default: nproc)
#        MIRIFLAGS (passed to `cargo miri test`)
set -uo pipefail

manifest_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$manifest_dir"

timeout_secs="${MIRI_TEST_TIMEOUT_SECS:-300}"
jobs="${MIRI_TEST_JOBS:-$(nproc)}"

target_flag() {
  case "$1" in
  lib) echo "--lib" ;;
  *) echo "--test $1" ;;
  esac
}
export -f target_flag

targets=(lib core_integration regression_integration custom_page_sizes_integration statistics_integration)

pairs_file="$(mktemp)"
results_file="$(mktemp)"
trap 'rm -f "$pairs_file" "$results_file"' EXIT

for label in "${targets[@]}"; do
  target="$(target_flag "$label")"
  RUSTFLAGS="--cfg miri" cargo test --quiet $target -- --list 2>/dev/null |
    grep ': test$' | sed 's/: test$//' |
    sed "s/^/${label}\t/" \
      >>"$pairs_file"
done

total=$(wc -l <"$pairs_file")
echo "Running $total tests:"
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
    printf 'PASS\t%s\t%s\n' "$label" "$name"
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

xargs -P "$jobs" -L1 bash -c 'run_one "$@"' _ <"$pairs_file" >>"$results_file"

pass_count=$(grep -c '^PASS' "$results_file" || true)
timeout_lines=$(grep '^TIMEOUT' "$results_file" || true)
fail_lines=$(grep '^FAIL' "$results_file" || true)

echo
echo "$total / $pass_count tests passed."

if [ -n "$timeout_lines" ]; then
  echo "Timed out:"
  echo "$timeout_lines" | awk -F'\t' '{print "  - " $2 " :: " $3}'
fi
if [ -n "$fail_lines" ]; then
  echo "Failed:"
  echo "$fail_lines" | awk -F'\t' '{print "  - " $2 " :: " $3}'
fi

if [ -n "$timeout_lines" ] || [ -n "$fail_lines" ]; then
  exit 1
fi
