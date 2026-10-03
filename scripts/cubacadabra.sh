#!/bin/sh
# Run the canonical native CLI from a sibling checkout or an installed binary.
set -eu

if [ -n "${CUBACADABRA_CLI:-}" ]; then
  exec "$CUBACADABRA_CLI" "$@"
fi

tools_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if ! command -v cargo >/dev/null 2>&1; then
  echo "Install Rust (cargo), or set CUBACADABRA_CLI to the native cubacadabra executable." >&2
  exit 1
fi
exec cargo run --quiet --locked --manifest-path "$tools_dir/Cargo.toml" --bin cubacadabra -- "$@"
