#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
checker="$script_dir/check-api-path-params.sh"
fixtures="$script_dir/fixtures"

run_fixture_pair() {
  local label="$1"
  local safe="$2"
  local anti="$3"

  bash "$checker" "$safe"
  if bash "$checker" "$anti"; then
    echo "expected the $label anti-pattern fixture to fail, but it passed" >&2
    exit 1
  fi
}

run_fixture_pair 'frontend' "$fixtures/frontend-safe/src" "$fixtures/frontend-anti-pattern/src"

echo 'API path-param gate self-test passed (frontend anti-pattern red, resolved/mocks green)'
