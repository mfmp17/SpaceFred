# Releasing

Spaceman is not release-ready merely because `scripts/build-app.sh` produces a signed bundle. The
script builds locked release binaries, assembles `Spaceman.app`, applies hardened-runtime Developer
ID signing to both executables and the bundle, and verifies the signature. It does not notarize,
staple, or publish an artifact. With `SPACEMAN_RELEASE_UNSIGNED=1` it stops before signing; the
Release workflow builds that way and signs in a separate job.

## Direct Distribution

Spaceman ships outside the Mac App Store as `anomaly.spaceman`, signed with Anomaly's Developer ID
(team `5NZ4Q7NXJ4`) and notarized so Gatekeeper accepts it. App Store Connect and TestFlight are not
part of this path. Releases up to 0.3.0 were `com.kitlangton.spaceman` under a personal Developer ID;
the first update to `anomaly.spaceman` keeps config, workspace state and parked-window recovery (the
paths don't depend on the bundle ID) but macOS asks for Accessibility access again, because that
grant belongs to the old identity.

A release has two halves:

```text
Release workflow (GitHub Actions)              installed Spaceman (src/updater.rs)
build ─► sign + notarize ─► verify ─► publish ─► manifest ─► zip ─► SHA-256 + Developer ID ─► Install & restart
         (Apple keys from AWS)       anoma.ly/release/spaceman

scripts/release-app.sh (release Mac), for builds up to 0.3.2 only
download that zip ─► validate ─► Sparkle appcast ─► R2   (Sparkle key, wrangler)
```

### 1. The Release workflow

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs on `dev`, the default branch:
every push publishes the dev channel, and a dispatch picks a channel:

| Channel | Publishes |
| --- | --- |
| `check` | Nothing. Builds, signs, notarizes and verifies, then pushes and deletes a `v0.0.0-test-<run>` tag. |
| `dev` (also every push that changes more than docs, the website or Markdown) | `0.0.0-dev-<run>` on the dev channel (`https://anoma.ly/release/spaceman/desktop/dev.json`). |
| `stable` | `Cargo.toml`'s version on the stable channel, updates Homebrew (`anomalyco/tap/spaceman`), and tags `v<version>`. Needs `changelog/<version>.md` and no existing tag. |

Its jobs:

- **build-macos** (`macos-26`, no secrets): `SPACEMAN_RELEASE_UNSIGNED=1 scripts/build-app.sh`
  with `SPACEMAN_RELEASE_CHANNEL` and `SPACEMAN_RELEASE_VERSION`, zipped and uploaded with a
  `SHA256SUMS` whose digest the job reports. Building unsigned also stamps
  `SPACEMAN_RELEASE_BUILD=1` into the binaries, which turns the in-app updater on; every other build
  is a source build that never checks.
- **sign-macos** (environment `release`): checks out only `.github/release/`, assumes
  `role/github/spaceman-release` through GitHub OIDC, reads `shared/APPLE_SIGNING_CERTIFICATE` and
  `shared/APPLE_NOTARY_KEY` from AWS Secrets Manager in one step, and with the environment's
  `APPLE_NOTARY_KEY_ID` and `APPLE_NOTARY_ISSUER_ID` signs inside-out, notarizes, staples and writes
  `Spaceman-<version>-macos-arm64.zip` and its `.sha256`. Every Mach-O in the bundle must be one it
  signs.
- **verify-macos** (no secrets): `scripts/validate-release.sh` on the signed app and zip, the team ID
  and designated requirement, Gatekeeper's `Notarized Developer ID`, and a run of the signed CLI.
- **publish** (environment `release`): publishes the zip as target `macos-arm64` (file
  `Spaceman.zip`) through the vendored release client, which checks the served bytes.
- **tag**: `v<version>` on the released commit, only after publishing.
- **homebrew-stage** (`macos-26`, no secrets, `stable` only): fetches the published stable
  manifest, renders `packaging/homebrew/spaceman.rb` via `scripts/release-homebrew.sh`, verifies it
  with `brew style` and `brew audit --cask --strict --online`, and uploads the cask with a digest.
- **homebrew** (environment `release`, `stable` only): checks out only `.github/release/`, verifies
  the staged cask digest, reads `shared/HOMEBREW_TAP_DEPLOY_KEY` via `role/github/spaceman-release`,
  and pushes `Casks/spaceman.rb` to `anomalyco/homebrew-tap` via `.github/release/push-homebrew.sh`.
- **record-brew** (environment `release`, `stable` only): records the `brew` distribution version
  on `anoma.ly/release` via `.github/release/release-service.sh distribution brew <version>`.

`.github/release/` and this workflow are the release boundary: jobs that hold a secret or an OIDC
token run only those scripts, on artifacts they verify by digest and never execute. CODEOWNERS makes
every change under `.github/` need Dax's review.

### 2. In-app updates

A release build checks `https://anoma.ly/release/spaceman/desktop/<channel>.json` 10 seconds after
launch, then hourly (every 15 minutes on dev) and on `Check for Updates...`. The channel is
`[settings] update_channel` (`stable` by default, or `dev`), picked in `Settings > General > Updates`.
When the channel's current release isn't the running build, it downloads the `macos-arm64` target
(only from `https://anoma.ly/release/spaceman/files/`, no redirects), checks its size and SHA-256
against the manifest, unpacks it beside the installed bundle as `.Spaceman-update-<version>-<sha>`,
and requires exactly `Spaceman.app`, bundle ID `anomaly.spaceman`, `codesign --verify --deep --strict`
against `app/update-requirement.txt` (Anomaly's Developer ID, notarized) and `TeamIdentifier=5NZ4Q7NXJ4`.
`Install & restart` (Settings sidebar, `General > Updates`, or the menu) then runs the same exit as
quit: tiling stops and every parked window is restored, and only once restoration fully succeeds
does Spaceman check the signature again, swap the bundles in one `renamex_np(RENAME_SWAP)` and open
the new one after it exits. Quitting (or restarting, or a macOS logout) with an update ready installs
it the same way after the restore, without relaunching; the next launch runs it. The next launch
removes the old bundle.

Manifest polls send the last response's ETag as `If-None-Match`, so an unchanged channel costs a
`304 Not Modified`. An interrupted download keeps its `.partial` file and continues with
`Range: bytes=<have>-` (a `200` starts it over). A copy Spaceman can't replace (a read-only folder or
an App Translocation copy) still checks, and offers the release's zip (`Download`) instead of
staging it.

Versions are never compared. Each Release workflow build is stamped with the channel and version it
publishes (`SPACEMAN_RELEASE_CHANNEL` and `SPACEMAN_RELEASE_VERSION`, passed through
`scripts/build-app.sh` and compiled in; the bundle's `Info.plist` keeps Cargo's version). A build
this updater installed is identified by `updates/installed.json` (the release's SHA-256, tied to the
running `spaceman-agent`'s hash); any other by its stamped version, else the receipt's, else
`CARGO_PKG_VERSION`. When `update_channel` names a channel other than the build's own, the release is
offered as a switch (`Switch to Dev` or `Switch to Stable`) and downloads only once chosen. Builds
from before the stamp count as dev when their version is a dev one, and otherwise never ask.

### 3. The legacy Sparkle appcast

Builds up to 0.3.2 update only through Sparkle, which reads the signed `appcast.xml` at `SUFeedURL` on
R2 (bucket `spaceman-releases`) and not the release service. Until every install runs a build with
the in-app updater, keep publishing each stable release there too, and keep the `SU*` keys in
`app/Info.plist` (a Sparkle client checks them in the update it installs). After a stable run, on the
release Mac (which holds the Sparkle key in Keychain and `wrangler` auth), in a clean worktree of
the released commit:

```sh
./scripts/release-app.sh          # prepare
./scripts/release-app.sh publish
```

`prepare` requires the stable channel's current release to be this version, downloads its
`macos-arm64` zip, checks its size and SHA-256 against the manifest, runs
`scripts/validate-release.sh` and the team check, and signs a local appcast with the Ed25519 key in
Keychain (service `https://sparkle-project.org`, account `ed25519`). It refuses a `CFBundleVersion`
that isn't newer than the published feed's. It never builds or signs the app, so R2 serves the exact
bytes the release service does. Generating the appcast needs the login keychain, so run it from a
LaunchAgent or a logged-in terminal rather than a plain `ssh` session. Test the zip in `dist/`, then
`publish` uploads the versioned zip, checksum and latest alias, compares each public object, and
only then replaces and compares `appcast.xml`. A missing feed is accepted only with
`SPACEMAN_FIRST_RELEASE=1`. Then deploy the website (`bun run deploy` in `website/`).

Before a release, update the version in `Cargo.toml`/`Cargo.lock` and `CFBundleShortVersionString`,
raise `CFBundleVersion`, add `changelog/<version>.md` (frontmatter `date` and `summary`, then
`## Major`, `## Improvements` and `## Fixes`), and update the versioned download links in README,
the setup skill and the website. Published version names are never reused.

## Source Build

```sh
SPACEMAN_CODESIGN_IDENTITY="Developer ID Application: Example (TEAMID)" \
  ./scripts/build-app.sh
codesign --verify --deep --strict target/SpaceFred.app
```

Do not publish `target/SpaceFred.app` as a release based only on those commands.

## Remaining Qualification Gaps

- Decide and validate supported architectures, including universal packaging if promised.
- Validate the declared minimum macOS version and a current macOS version.
- Test first install, Accessibility onboarding, launch-at-login registration and relaunch, shortcuts,
  displays, upgrade, crash recovery, pause/quit restoration, and uninstall on a clean machine.
- Add and verify a concise product demo.
- Recheck the private `_AXUIElementGetWindow` risk and state clearly that the Mac App Store is not
  supported.
- Review `SECURITY.md`, `CONTRIBUTING.md`, license, version, changelog/release notes, and support policy
  before broadening the public preview.

## Release Rules

1. Release from a clean, reviewed commit and retain exact source revision and toolchain metadata.
2. Run all verification in [Development](development.md), both isolated live harnesses, and
   `git diff --check`.
3. Build once for release; notarize and distribute that exact artifact rather than rebuilding it.
4. Verify signature, notarization/stapling, archive contents, executable architecture, plist minimum
   OS, and checksum after packaging.
5. Install the downloadable artifact on a clean supported Mac and execute the recovery/uninstall path.
6. Never ask users to disable SIP or delete parked-window metadata to complete an install or upgrade.
7. Keep source-build instructions separate from artifact installation so users are not told they need
   a Developer ID identity for a future release.
