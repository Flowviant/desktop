#!/bin/sh
# THE TRAY'S STD-ONLY RUST TESTS, without building the app (2026-09-26).
#
# The rule modules below use nothing but std, so plain `rustc --test` builds
# and runs their #[cfg(test)] cases (and the source pins they carry: the
# notification policy's announcer pin, the supervisor's one-home bans, the
# drain ban) on a box with no Tauri toolchain. `bun run test` here
# chains this after vitest, and CI runs it as its own step — so CI MUST have
# rustc on PATH (ubuntu-latest ships it; the workflow prints its version).
#
# Each module is given to rustc by ABSOLUTE path: `file!()` is then absolute,
# so source_pins.rs's src/ walk finds the folder from any working directory.
#   sh scripts/test-rust.sh
set -eu
src=$(cd "$(dirname "$0")/../src-tauri/src" && pwd)
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
for module in updates notifications wsl daemon_output supervisor_rules setup_rules; do
  rustc --edition 2021 --test -A dead_code "$src/$module.rs" -o "$out/$module"
  "$out/$module" --quiet
done
