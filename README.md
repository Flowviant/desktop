# Flowviant desktop

Windows first: a Tauri 2 tray that bundles the Linux x64 daemon for projects in WSL. Pick `\\wsl$\<distro>\…` or `\\wsl.localhost\<distro>\…`. A Windows folder is declined until a native Windows daemon exists. The tray starts at login by default. Closing the window hides it; Quit stops the daemons this app started on this box.

## Build the bundled daemon

In the CLI checkout, build the release binary with the same version used by the server:

```sh
cd /path/to/cli
scripts/build-binaries.sh
cat dist/manifest.json
```

In this repository, `apps/desktop/scripts/prepare-daemon.mjs` copies and verifies `linux-x64` from the CLI release manifest into `src-tauri/resources/flowviant-linux-x64`. It looks for sibling `desk-cli/dist` or `cli/dist`; set `FLOWVIANT_CLI_DIST` if the checkout is elsewhere. The binary is ignored by Git; release builds must run the copy step. The installed WSL binary self-updates at its idle boundary.

## Windows development and NSIS release

Install [Rust with the MSVC toolchain](https://v2.tauri.app/start/prerequisites/), [Bun](https://bun.sh/docs/installation), Microsoft C++ Build Tools, and [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) on Windows. Enable WSL and install a distro with Git. Then:

```powershell
cd C:\path\to\flowviant
bun install
$env:FLOWVIANT_CLI_DIST = 'C:\path\to\cli\dist'
bun run desktop:dev
bun run desktop:build
```

The NSIS installer is under `apps/desktop/src-tauri/target/release/bundle/nsis`. It is unsigned. Verify on Windows by selecting a WSL Git checkout, completing browser login, starting and stopping it, checking Board/Machines/log links, then restarting Windows to check autostart. Use a second box with the same project credential to verify the displaced notification and no restart loop. Verify a CLI limit and an applied daemon update to check those two notifications.

For a Linux cross-build attempt, follow [Tauri's Windows installer guide](https://v2.tauri.app/distribute/windows-installer/): install NSIS, LLVM/lld, the MSVC Rust target, and `cargo-xwin`, then run `tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc` from `apps/desktop` after the daemon copy step. Windows is the release verification platform even if cross compilation succeeds.

The Linux cross-build attempted on 2026-09-24 reached `cargo-xwin` but could not fetch the Microsoft CRT CAB: `download.visualstudio.microsoft.com` failed DNS resolution after four attempts. Build the NSIS package on Windows with the PowerShell commands above.

## Updater signing key

The public key is in `src-tauri/tauri.conf.json`; the private key belongs at `~/.flowviant-desktop-updater.key` outside this repository. This run generated it at `/home/w/.claude/jobs/90720d07/tmp/flowviant-desktop-updater.key` because the home directory was read-only in the build sandbox. Move it to the home path before producing a signed updater package. Set `TAURI_SIGNING_PRIVATE_KEY_PATH` to that path for release builds. Keep the key private and backed up. The configured endpoint, `https://api.flowviant.com/desktop/latest.json`, needs a later Worker route before automatic desktop updates can be served.
