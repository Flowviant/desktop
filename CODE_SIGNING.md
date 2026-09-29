# Code signing policy

The full policy is published at **[flowviant.com/code-signing](https://flowviant.com/code-signing)**. This is its summary for this repository.

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## What is signed

- The Windows installer, `Flowviant-Machines-<version>-setup.exe`.
- The app it installs, `flowviant-desktop.exe`.

Windows names the publisher of both as SignPath Foundation. The uninstaller the installer writes is not signed. The daemon's Linux binary the installer carries is not a Windows program and is not signed with this certificate; it comes from the [Flowviant/cli](https://github.com/Flowviant/cli) GitHub Release, and the build verifies its attestation and SHA-256 before bundling it.

## How

Only [`.github/workflows/release.yml`](.github/workflows/release.yml) makes a release: on GitHub-hosted runners, from a `v*` tag of this repository. It submits the app, then the installer built around it, to SignPath with SignPath's GitHub action. SignPath verifies that each artifact came from this repository's workflow run (origin verification) and signs only after an approver approves the request. The SignPath artifact configurations are copied in [`.signpath/artifact-configurations/`](.signpath/artifact-configurations). Nothing built on a personal computer is signed.

## Team

| Role | Members |
| --- | --- |
| Committers (authors) | [William](https://github.com/wayleem) |
| Reviewers | [William](https://github.com/wayleem) |
| Approvers | [William](https://github.com/wayleem) |

Committers change the source without another review; a change from anyone else is reviewed by a reviewer before it is merged. Approvers approve each signing request. Every team member uses multi-factor authentication for GitHub and for SignPath.

## Privacy

On its own, the app makes one request: the update check (`https://api.flowviant.com/dl/desktop/latest.json`, a minute after start and every six hours), which carries nothing about you or your projects. Everything else follows a press of yours. See the [policy page](https://flowviant.com/code-signing#privacy) and the [Flowviant Privacy Policy](https://flowviant.com/privacy).
