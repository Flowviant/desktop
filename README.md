# Flowviant desktop

Windows first: a Tauri 2 tray that bundles the Linux x64 daemon for projects in WSL. Pick `\\wsl$\<distro>\…` or `\\wsl.localhost\<distro>\…`. A Windows folder is declined and pointed at WSL: Flowviant stays Unix (2026-09-28). The tray starts at login by default (in the tray, via `--autostart`); opening the app yourself opens its window. The window and the tray popover are one card (the Machines App design, frames 1c/1d/1e). Connect a project picks the folder, then opens `app.flowviant.com/connect?code=…`, where you pick the project and approve; a failed login shows the daemon's own last words. Each project has an on/off switch on the card: off stops its daemon and keeps it stopped (launch included); on starts it and keeps it running. The app only runs `wsl.exe` against distros that are already running (`wsl --list --running` never boots one), so after `wsl --shutdown` nothing restarts WSL; a project whose distro stopped resumes when WSL does, or on Start. Closing the window hides it; Quit stops every daemon and closes the app — reopening starts the projects that are on. The app's own log (every start, exit, standby, retry, update, switch) is `%LOCALAPPDATA%\com.flowviant.desktop\logs\machines.log` (card › Open logs); each daemon's log is `flowviant logs` in WSL. Release builds are GUI-subsystem and start every `wsl.exe` with `CREATE_NO_WINDOW`, so no console window appears.

## Uninstall

Windows Settings › Apps removes the tray app, stops its WSL daemons, and removes the binaries while keeping the login in `~/.flowviant` for a later install. Inside WSL, run `flowviant uninstall` (`--purge` removes everything). Newer installs remove older copies automatically.

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

The Linux cross-build WORKS (2026-09-24, Arch + nix). What it took, in order:
1. Fetch the MSVC CRT OUTSIDE nix-shell (DNS fails inside it here): `~/.cargo/bin/cargo-xwin env --target x86_64-pc-windows-msvc` from `src-tauri` (cached in `~/.cache/cargo-xwin`).
2. Build inside `nix-shell -p nsis llvmPackages.bintools llvmPackages.clang-unwrapped pkg-config libayatana-appindicator gtk3` with `PATH=$HOME/.cargo/bin:$PATH` and `CL='-imsvc<clang-lib>/lib/clang/16/include'` (the unwrapped clang has no builtin headers; `nix-build '<nixpkgs>' -A llvmPackages.clang-unwrapped.lib`). gtk3 is needed only because the Tauri CLI's appindicator probe runs `pkg-config` on the host.
3. nix's NSIS is 3.06: make a writable NSIS dir (copy of its `share/nsis`, plus `Include/` from Tauri's nsis-3.11 zip, plus `ApplicationID.dll` (NSIS-ApplicationID 1.1, ReleaseUnicode) and `nsis_tauri_utils.dll` in `Plugins/x86-unicode`), then run `NSISDIR=<that dir> makensis installer.nsi` in `target/x86_64-pc-windows-msvc/release/nsis/x64` — the Tauri CLI's own makensis call does not pick the dir up.
Output: `nsis-output.exe` (~30 MB with the bundled daemon). Windows remains the release verification platform.

## Updater signing key

The public key is in `src-tauri/tauri.conf.json`; the private key belongs at `~/.flowviant-desktop-updater.key` outside this repository. Its password is empty. Set `TAURI_SIGNING_PRIVATE_KEY_PATH` to that path for release builds. Keep the key private and backed up. The app checks `https://api.flowviant.com/dl/desktop/latest.json` a minute after it starts and every six hours, downloads a newer installer in the background, and installs it (passive, `/UPDATE`, relaunching with its own arguments) only when no project this computer serves has a working agent; the card also offers a confirmed "Restart now". Publish a build with `sh scripts/release-windows.sh` (signs with this key, uploads the installer, checks the served bytes, then writes the manifest last).
