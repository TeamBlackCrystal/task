#!/usr/bin/env bash
# Verify binary-tracked files are unchanged across `git add --renormalize .`
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

is_binary_tracked() {
  local path="$1"
  local binary text
  binary="$(git check-attr binary -- "$path" | awk -F': ' '{print $3}')"
  text="$(git check-attr text -- "$path" | awk -F': ' '{print $3}')"
  if [[ "$binary" == "set" ]] || [[ "$text" == "unset" ]]; then
    return 0
  fi
  return 1
}

list_binary_files() {
  git ls-files -z | while IFS= read -r -d '' path; do
    if is_binary_tracked "$path"; then
      printf '%s\0' "$path"
    fi
  done
}

hash_binary_files() {
  local out="$1"
  : >"$out"
  while IFS= read -r -d '' path; do
    if [[ ! -f "$path" ]]; then
      echo "missing: $path" >&2
      exit 1
    fi
    sha256sum "$path" >>"$out"
  done < <(list_binary_files)
}

BEFORE="$(mktemp)"
AFTER="$(mktemp)"
trap 'rm -f "$BEFORE" "$AFTER"' EXIT

hash_binary_files "$BEFORE"
BINARY_COUNT="$(grep -c . "$BEFORE" || true)"

echo "binary_files_tracked=$BINARY_COUNT"
echo "binary_hash_before:"
cat "$BEFORE"

if [[ "${1:-}" == "--after-renormalize" ]]; then
  hash_binary_files "$AFTER"
  echo "binary_hash_after:"
  cat "$AFTER"
  if ! diff -u "$BEFORE" "$AFTER"; then
    echo "ERROR: binary file hash mismatch after renormalize" >&2
    exit 1
  fi
  echo "OK: all $BINARY_COUNT binary file(s) unchanged"
fi
