# Flowviant Machines (Windows tray)

The Windows app for [Flowviant](https://flowviant.com): a tray app that runs the Flowviant daemon ([Flowviant/cli](https://github.com/Flowviant/cli)) for projects that live in WSL. Built with [Tauri 2](https://v2.tauri.app) and a small React front end. MIT licensed.

Flowviant stays Unix: the daemon is a Linux program, and this app is how a Windows computer runs it. The installer carries the daemon's Linux x64 build and puts it in each WSL distro it serves.

**Download:** [flowviant.com/download](https://flowviant.com/download), or the installer attached to each [GitHub Release](https://github.com/Flowviant/desktop/releases). **[Code signing policy](https://flowviant.com/code-signing)** — free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## What it does

- **Connect a project.** Pick a folder under `\\wsl$\<distro>\…` or `\\wsl.localhost\<distro>\…`, and the app opens `app.flowviant.com/connect?code=…`, where you pick the project and approve. A Windows folder is declined and pointed at WSL. A failed login shows the daemon's own last words.
- **Keep it running.** Each project has an on/off switch on the card: on starts its daemon and keeps it running; off stops it and keeps it stopped, launch included. The app only runs `wsl.exe` against distros that are already running (`wsl --list --running` never boots one), so after `wsl --shutdown` nothing restarts WSL; a project whose distro stopped resumes when WSL does, or on Start.
- **Set up a new computer.** A checklist offers, each only when it is measured missing and one press each: Install WSL (Windows' own `wsl --install`), Open Ubuntu (its first-launch account), Install Claude Code (Anthropic's installer, in the distro), Sign in to Claude.
- **Say what the machine is doing** — which projects it serves, what their agents are doing, a CLI limit, a daemon that is behind — in the window and in the tray popover (one card), with native notifications only for machine events.
- **Update itself** (see *Releases and code signing*), installing only while no project this computer serves has a working agent.

The app does not start at sign-in until you ask. **Start Flowviant when I sign in to Windows**, a switch in the window and the same words as a check item in the tray menu, adds the Windows startup entry (the app then starts in the tray, via `--autostart`) and removes it; it is off on a new install. An install that already starts at sign-in (0.0.15 and older turned it on the first time they ran) keeps it until you turn it off. Opening the app yourself opens its window. Closing the window hides it; Quit stops every daemon and closes the app. Release builds are GUI-subsystem and start every `wsl.exe` with `CREATE_NO_WINDOW`, so no console window appears.

The app's own log (every start, exit, standby, retry, update, switch) is `%LOCALAPPDATA%\com.flowviant.desktop\logs\machines.log` (card › Open logs); each daemon's log is `flowviant logs` in WSL.

### What it sends over the network

On its own, only the update check: a minute after it starts and every six hours it downloads `https://api.flowviant.com/dl/desktop/latest.json`. Everything else follows a press: connecting a project opens `app.flowviant.com` in the browser, and from then on the daemon runs that project against `api.flowviant.com` under the project's credential; the setup checklist downloads WSL (Microsoft) and Claude Code's installer (Anthropic). The [code signing policy](https://flowviant.com/code-signing) has the full statement.

## Uninstall

Windows Settings › Apps removes the app, stops its WSL daemons, and removes the daemon binaries while keeping the login in `~/.flowviant` for a later install. Inside WSL, `flowviant uninstall` (`--purge` removes everything). Newer installs remove older copies automatically.

## Build

You need [Bun](https://bun.sh) 1.3, [Rust](https://www.rust-lang.org/tools/install) (the MSVC toolchain on Windows), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Microsoft C++ Build Tools, WebView2).

```sh
bun install
bun run typecheck
bun run test        # vitest, then the std-only Rust rule modules (plain rustc)
```

The installer carries the daemon's Linux x64 binary. `scripts/prepare-daemon.mjs` copies it into `src-tauri/resources/` from a folder holding a daemon release's `manifest.json` and `flowviant-<version>-linux-x64`, after checking the binary's SHA-256 against the manifest. Point `FLOWVIANT_CLI_DIST` at that folder — a download of the [Flowviant/cli release](https://github.com/Flowviant/cli/releases) at the version in `src-tauri/resources/daemon-version.txt`, or the `dist/` of a daemon checkout after its `scripts/build-binaries.sh` — or keep the daemon checkout beside this one (`../cli/dist`).

On Windows:

```powershell
$env:FLOWVIANT_CLI_DIST = 'C:\path\to\cli-release'
bun run dev      # the app, against the Vite dev server
bun run build    # an unsigned NSIS installer under src-tauri\target\release\bundle\nsis
```

Verify a build on Windows by selecting a WSL Git checkout, completing the browser login, starting and stopping it, checking the Board/Machines/log links, then turning on **Start Flowviant when I sign in to Windows** and restarting Windows to check that it starts in the tray.

<details>
<summary>Compile-checking from Linux</summary>

With [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin), the MSVC Rust target and LLVM (`clang-cl`, `llvm-lib`, `lld-link`): `bun run web:build`, put the daemon in place (`bun run prepare:daemon`), then `cargo xwin check --target x86_64-pc-windows-msvc` in `src-tauri`. A full cross-build also needs NSIS 3.11 with Tauri's plugins; see [Tauri's Windows installer guide](https://v2.tauri.app/distribute/windows-installer/). Releases are never made this way.

</details>

## Releases and code signing

Releases are built only by [`.github/workflows/release.yml`](.github/workflows/release.yml), on GitHub-hosted Windows runners, from a `v*` tag that matches the version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`:

1. the tests run ([`ci.yml`](.github/workflows/ci.yml));
2. the daemon's Linux binary is downloaded from the pinned Flowviant/cli GitHub Release (`src-tauri/resources/daemon-version.txt`), its build attestation checked, and its SHA-256 checked against that release's manifest;
3. the app is built, then **signed through [SignPath](https://about.signpath.io)** with a certificate issued to **SignPath Foundation**; the NSIS installer is built around the signed app, then signed the same way (two signing requests, because an NSIS installer cannot be signed inside; the artifact configurations are copied in [`.signpath/`](.signpath/artifact-configurations));
4. the signed installer is signed again for the in-app updater (minisign; the public key is in `src-tauri/tauri.conf.json`), and `latest.json` is written;
5. the installer, its `.sig`, `latest.json` and `SHA256SUMS` are attached to a GitHub Release;
6. optionally (repository variable `PUBLISH_R2`), [`scripts/publish-r2.sh`](scripts/publish-r2.sh) puts the installer on the Flowviant download host, checks the served bytes, and writes `latest.json` last — the manifest every installed app polls.

The updater's private key is a repository secret and never leaves GitHub Actions. Lose it and installed apps can no longer update themselves. [CODE_SIGNING.md](CODE_SIGNING.md) summarizes the signing policy.

## Font

The interface is set in [Geist](https://vercel.com/font), by Vercel in collaboration with basement.studio, under the [SIL Open Font License 1.1](public/fonts/OFL.txt). The variable font and its licence are in `public/fonts/` and ship inside the app together; Windows' Segoe UI is only the fallback.

## License

MIT — see [LICENSE](LICENSE). The bundled Geist font is under the SIL Open Font License 1.1 ([`public/fonts/OFL.txt`](public/fonts/OFL.txt)).
