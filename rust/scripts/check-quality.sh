#!/usr/bin/env bash
set -Eeuo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MAX_FILE_LINES=250
failures=0

while IFS= read -r -d '' file; do
  lines="$(wc -l < "$file")"
  if [ "$lines" -gt "$MAX_FILE_LINES" ]; then
    printf 'FAIL: %s has %d lines (max %d)\n' "${file#"$ROOT_DIR/"}" "$lines" "$MAX_FILE_LINES" >&2
    failures=$((failures + 1))
  fi
done < <(find crates -type f -name '*.rs' -print0)

if [ "$failures" -gt 0 ]; then
  exit 1
fi

echo "File line limits OK (all .rs files <= $MAX_FILE_LINES lines)."
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
