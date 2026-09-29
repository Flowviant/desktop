#!/bin/sh
# Publish a SIGNED release of Flowviant Machines to the Flowviant download host
# (R2, served at https://api.flowviant.com/dl/desktop/…). The last, optional job
# of .github/workflows/release.yml; it takes the folder that workflow's updater
# job made: Flowviant-Machines-<version>-setup.exe (Authenticode-signed by
# SignPath), its updater signature (.sig) and latest.json.
#   1. upload the installer as desktop-releases/Flowviant-Machines-<version>-setup.exe
#      (flowviant.com/download links it by version),
#   2. check the served bytes are these bytes,
#   3. THEN write desktop-releases/latest.json, the manifest every installed app
#      polls (GET https://api.flowviant.com/dl/desktop/latest.json). Last, so an
#      app is never told about an installer it cannot fetch.
# Needs CLOUDFLARE_API_TOKEN + CLOUDFLARE_ACCOUNT_ID in the environment.
#   sh scripts/publish-r2.sh release
set -eu
cd "$(dirname "$0")/.."
dir=${1:?usage: sh scripts/publish-r2.sh <release folder>}

version=$(bun -p 'require("./src-tauri/tauri.conf.json").version')
name="Flowviant-Machines-$version-setup.exe"
for f in "$dir/$name" "$dir/$name.sig" "$dir/latest.json"; do
  test -f "$f" || { echo "missing $f" >&2; exit 1; }
done
# The manifest names this version, this installer and this signature.
VERSION="$version" NAME="$name" SIG="$(cat "$dir/$name.sig")" bun -e '
const m = require(require("node:path").resolve(process.argv[1]));
const e = process.env;
const w = m.platforms?.["windows-x86_64"];
if (m.version !== e.VERSION || w?.url !== `https://api.flowviant.com/dl/desktop/${e.NAME}` || w?.signature !== e.SIG) {
  console.error("latest.json does not describe this installer"); process.exit(1);
}
' "$dir/latest.json"

wrangler="bunx wrangler@4.100.0"
bucket=flowviant-code-graphs/desktop-releases
$wrangler r2 object put "$bucket/$name" --file "$dir/$name" --content-type application/vnd.microsoft.portable-executable --remote
want=$(sha256sum "$dir/$name" | cut -c1-64)
got=$(curl -fsSL "https://api.flowviant.com/dl/desktop/$name" | sha256sum | cut -c1-64)
[ "$want" = "$got" ] || { echo "served installer does not match ($got vs $want) — manifest NOT updated" >&2; exit 1; }
$wrangler r2 object put "$bucket/latest.json" --file "$dir/latest.json" --content-type application/json --remote
echo "published $name (sha256 $want); installed apps update to $version"
