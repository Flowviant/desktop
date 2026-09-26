#!/bin/sh
# Publish a built Windows installer of Flowviant Machines:
#   1. sign it with the updater key (the pubkey is in src-tauri/tauri.conf.json),
#   2. upload it to R2 as Flowviant-Machines-<version>-setup.exe (flowviant.com/download
#      links it; apps/marketing/src/lib/site.ts names the version),
#   3. check the served bytes are these bytes,
#   4. THEN write desktop-releases/latest.json, the manifest every installed app polls
#      (GET https://api.flowviant.com/dl/desktop/latest.json). Last, so an app is never
#      told about an installer it cannot fetch.
#
# Build first (see README: cargo-xwin, then makensis by hand). Needs
# CLOUDFLARE_API_TOKEN + CLOUDFLARE_ACCOUNT_ID in the environment.
#   RELEASE_NOTES="…" sh scripts/release-windows.sh
set -eu
cd "$(dirname "$0")/.."

version=$(bun -p 'require("./src-tauri/tauri.conf.json").version')
target=${CARGO_TARGET_DIR:-src-tauri/target}
nsis="$target/x86_64-pc-windows-msvc/release/nsis/x64"
grep -q "!define VERSION \"$version\"" "$nsis/installer.nsi" || { echo "the built installer is not $version — rebuild" >&2; exit 1; }
name="Flowviant-Machines-$version-setup.exe"
out=$(mktemp -d)
cp "$nsis/nsis-output.exe" "$out/$name"

key=${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.flowviant-desktop-updater.key}
bunx tauri signer sign -f "$key" -p "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" "$out/$name" >/dev/null

VERSION="$version" NAME="$name" SIG="$(cat "$out/$name.sig")" NOTES="${RELEASE_NOTES:-}" bun -e '
const fs = require("node:fs");
const e = process.env;
fs.writeFileSync(process.argv[1], JSON.stringify({
  version: e.VERSION,
  notes: e.NOTES,
  pub_date: new Date().toISOString(),
  platforms: { "windows-x86_64": { signature: e.SIG, url: `https://api.flowviant.com/dl/desktop/${e.NAME}` } },
}, null, 2) + "\n");
' "$out/latest.json"

bucket=flowviant-code-graphs/desktop-releases
(cd ../api && bunx wrangler r2 object put "$bucket/$name" --file "$out/$name" --content-type application/vnd.microsoft.portable-executable --remote)
want=$(sha256sum "$out/$name" | cut -c1-64)
got=$(curl -fsSL "https://api.flowviant.com/dl/desktop/$name" | sha256sum | cut -c1-64)
[ "$want" = "$got" ] || { echo "served installer does not match ($got vs $want) — manifest NOT updated" >&2; exit 1; }
(cd ../api && bunx wrangler r2 object put "$bucket/latest.json" --file "$out/latest.json" --content-type application/json --remote)
echo "published $name (sha256 $want); installed apps update to $version"
