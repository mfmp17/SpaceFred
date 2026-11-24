# Spaceman Contributor Guide

This file is the current engineering contract. README and `docs/` describe current user behavior;
`PLAN.md` and `docs/RESEARCH.md` are historical records.

## Working Here

- Agents merge their own Spaceman PRs and redeploy (release, publish, install) without asking.
- CI's macOS checks run only on dispatch and tags, so run `scripts/check` locally before merging
  and say so in the PR.
- The installed app follows `dev`: `scripts/spaceman-updater` (LaunchAgent
  `anomaly.spaceman.updater`, set up with `scripts/install-updater`, log in
  `~/.local/share/spaceman/updater.log`) builds a merged `dev` once pushes stop for 3 minutes, or
  after 10 at most, and installs it when the keyboard and mouse have been idle for 2 minutes. You
  don't need to install by hand after a merge.
- Dev loop: `scripts/dev` rebuilds and reinstalls on every save; restart restores every window and
  resumes tiling. The updater puts `dev` back on its next build.
- Release notes are one file per version in `changelog/<version>.md` (frontmatter `date` and
  `summary`, then `## Major`, `## Improvements`, and `## Fixes`).
  The Release workflow (`.github/workflows/release.yml`, on `dev`) builds, signs with Anomaly's
  Developer ID, notarizes and publishes to `https://anoma.ly/release/spaceman` (dev channel on every
  push, stable on dispatch); `.github/` changes require CODEOWNERS review; then
  installed builds update themselves from it (`src/updater.rs`). Until every install has a build
  with that updater, `scripts/release-app.sh` on the release Mac (which holds the Sparkle key and
  `wrangler` auth) still puts each stable zip into the R2 appcast that older (Sparkle) builds read.
  See [docs/releasing.md](docs/releasing.md).
- Bundle ID is `one.frederico.spacefred` (upstream Spaceman is `anomaly.spaceman`). State and config
  paths stay `~/Library/Application Support/Spaceman` and `~/.config/spaceman` so an existing
  Spaceman config carries over. Source builds do not install Anomaly releases.
- Code shape: keep Rust files under 800 non-test lines (1,500 hard cap, 1,000 for test files;
  `scripts/file-size.sh`) and move prefix families (`x.rs` + 2+ `x_*.rs`, or 3+ `x_*.rs`) into `x/`
  (`scripts/folder-shape.sh`).

## Architecture Map

- `agent/`, `src/app.rs`, `src/app/app_loop.rs`, `src/app/app_loop/`, `src/app/worker.rs`,
  `src/app/bridge.rs`: single-process Rune host (`spaceman-agent`), owner-only capped logging, 25 ms
  `NSTimer` foreground pump, action queue, status, restore-before-exit coordination, in-process
  Settings window (`General`, `Layout`, `Rules`, `Workspaces`, `Shortcuts`, `Activity`), first-run onboarding
  tour (`agent/src/onboarding.rs`), non-activating Workspace HUD overlay (`agent/src/hud.rs`),
  Playground (`⇧⌘D` / `--playground`), and the worker boundary.
- `src/app/runtime.rs`, `src/app/cycle.rs`, `src/app/cycle/affinity.rs`, `src/app/switching.rs`,
  `src/app/parking.rs`, `src/app/commands.rs`, `src/app/backend.rs`: the worker-thread
  `WorkspaceRuntime` — observation cycles and affinity, workspace switches, durable
  parking/restoration, focused-window commands, and the native/persistence backend seam. Tests live
  in `src/app/tests.rs` and `src/app/tests/`.
- `src/window_memory.rs`, `src/app/memory.rs`: advisory, title-free per-bundle placement memory and
  relaunch recall. `src/journal.rs`, `src/journal/doctor.rs`, `src/app/journal.rs`: bounded
  arrangement/anomaly journal and the `spaceman doctor` summary.
- `src/app/input.rs`: bounded ordered user input and geometry-based gesture tracking, independent
  of preview-worker availability. Main-thread pointer sampling must not make synchronous AX calls.
- `src/status_item.rs`, `src/login_item.rs`, `src/termination.rs`: main-thread
  menu bar status item, login registration, and asynchronous AppKit termination
  coordination on winit's application delegate.
- `src/updater.rs`, `src/updater/`: the in-app updater (release-service protocol, HTTPS transport,
  bundle staging, Developer ID verification, swap and relaunch after restoration).
- `src/hotkeys.rs`, `src/keyboard_layout.rs`: Carbon global shortcut adapters.
- `src/macos.rs`, `src/macos/`: Accessibility/Core Graphics observation and writes, display
  discovery, native event monitor, and native handle cache.
- `src/observation.rs`, `src/model.rs`, `src/reconcile.rs`: immutable observations, stable logical slots,
  and conservative physical-window reconciliation.
- `src/workspace.rs`, `src/runtime.rs`, `src/layout_tree.rs`, `src/layout_tree/`, `src/navigation.rs`:
  ten default numbered workspaces, opt-in named workspaces A-Z, ownership, nested weighted layouts,
  directional operations, and persistence representation.
- `src/config.rs`, `src/serde_map.rs`: strict version-1 TOML parsing, first-match window disposition,
  state-path selection, and deterministic persisted-map serialization.
- `src/control.rs`, `src/main.rs`: mode-`0600` local Unix control protocol, CLI, dry-run/live probes,
  fixture replay, and HUD.
- `src/motion.rs`, `src/macos/display_link.rs`, `src/macos/presenter.rs`: the motion engine's
  foundation, not yet wired in: a closed-form camera spring, gesture velocity history and
  per-display column bounds (pure, cross-platform); a paused-until-started main-thread
  `CADisplayLink`; and the interactive presenter, which writes position-only frames through
  per-app lanes that coalesce to the newest frame and hold `AXEnhancedUserInterface` off for a
  motion.
- `src/geometry.rs`, `src/live.rs`: frame math, off-screen target selection, live observation
  completion, placement filtering, and CLI probe helpers.
- `src/permission_guide.rs`, `native/permission_guide.m`: Accessibility onboarding adapter.
- `.github/workflows/release.yml`, `.github/release/`: the CI release (build, Developer ID sign,
  notarize, verify, publish to the Anomaly release service, tag).
- `scripts/`: icon generation, verified Sparkle setup, signed bundle assembly/install, notarized
  release preparation/validation/publication, and destructive live acceptance harnesses.

## Product Invariants

- A logical slot is product identity; a physical `AXWindow` binding is replaceable evidence.
- Native-tab replacement preserves slot, order, weight, nested layout, workspace, Fullscreen Layout,
  and active-tile intent.
- Incomplete or timed-out observations never prove absence. Ambiguous native-tab evidence preserves
  bindings during its bounded settling window; only repeated complete observations may retire an
  unresolved binding under the tested reconciliation policy.
- One accepted observation produces one coherent model revision; native writes converge toward it.
- Rules are ordered and first-match. Named workspaces use the same workspace identity and ordered
  route schema as numbered workspaces; do not introduce an orthogonal app-space schema.
- Automatic tiling starts when Accessibility permission and hotkeys are available. Documentation must
  not claim an unconditional paused startup.

## Recovery And Persistence Invariants

- Persist a window's original frame before moving it off-screen.
- Remove parked metadata only after restoration succeeds and the removal is durably persisted. Roll
  metadata back in memory if persistence fails.
- Pause, quit, restart-for-install, hotkey failure, and worker failure must favor restoration. Never
  recommend deleting `parked-windows.json` as the first recovery step.
- `workspace-state.json` must validate before restoration. Invalid config/state may be ignored so
  parked recovery can proceed; malformed parked state must not be silently discarded.
- Persist workspace/layout mutations atomically. Keep backward migration only for actual persisted
  formats already emitted.
- Never change state paths or schemas without migration/recovery tests and synchronized docs.

## Privacy And Security Invariants

- Accessibility is used only for window observation, focus, movement, resizing, and required input/
  display context. Never require SIP to be disabled.
- Window titles may be used for matching in memory but must never enter persistence, serialized
  snapshots, debug output, HUD output, fixtures, logs, or control responses.
- No telemetry, network access, or update checks without explicit product and security review.
  The updater (`src/updater.rs`) is the one reviewed network path: release builds only, the
  release service's manifest and `files/` only, nothing about the install beyond the User-Agent.
- Keep `control.sock` and `control.lock` at mode `0600`, bound under the user's Application Support
  directory, with bounded requests and single-agent locking.
- Treat `_AXUIElementGetWindow` as private, optional, dynamically resolved compatibility risk. Do not
  expand private API usage casually or claim Mac App Store support.

## Thread And FFI Invariants

- AppKit/status UI and APIs requiring it stay on the main thread.
- Accessibility observer run-loop objects stay on the dedicated AX event thread.
- Window-management work and its thread-local AX handle cache stay on the worker thread. The one
  exception is the interactive presenter's lane threads (`src/macos/presenter.rs`): each lane opens,
  uses and drops its own apps' AX handles, and only window IDs and points cross into it.
- Native handles (`AXUIElement`, `AXObserver`, Core Foundation run-loop values, Objective-C objects)
  never enter logical models, persisted state, or cross-thread messages.
- Native callbacks only invalidate/request observation; they do not mutate the logical model.
- Keep unsafe FFI narrow, typed, dynamically checked where applicable, and covered by safe Rust
  boundaries. Do not relax crate-wide unsafe policy to simplify an adapter.

## Live-Test Safety

- Disable AeroSpace and every other window manager before directly launching a process that applies
  frames. The repository live harnesses instead require AeroSpace initially, capture its baseline, and
  own the disable/restore transition.
- Prefer fixture replay and dry-run commands. Use `watch-window` or disposable Ghostty processes for
  scoped live tests.
- Read live harnesses before running them. Confirm expected displays, preserve baseline workspace and
  focus, and verify cleanup. Never experiment on unrelated user windows.
- Live harness state must use the internal `SPACEMAN_APPLICATION_SUPPORT` override and must never read
  or mutate the user's normal workspace or parked-window state.
- Preserve `~/Library/Application Support/Spaceman/parked-windows.json` until pause/quit restoration is
  confirmed. If a test crashes, reopen the same app and recover before cleaning state.

## Verification

```sh
scripts/check            # fmt, file-size, folder-shape, metadata, clippy, tests, audit, CA roots, replay, config, website
scripts/check --no-web   # the same without the website
```

Run `./scripts/live-ghostty-tabs.sh` and `./scripts/live-workspaces.sh` only when the change reaches
their live behavior, AeroSpace is initially enabled, and the other safety conditions above are
satisfied.

`scripts/gauntlet.sh` (docs/development.md › Live Gauntlet) is the end-to-end check for changes to
tiling, switching, parking, focus, reflow, float, the Scratchpad, or recovery. Run it on a disposable
macOS VM or a test Mac with `--host`, never on a Mac someone is using, and quote its receipt summary
in the PR.

## Documentation Synchronization

Update README and linked docs in the same change when menu labels, shortcuts, CLI commands, config
schema, environment variables, state paths/schema, privacy output, supported windows/displays,
packaging, startup, restoration, or uninstall behavior changes. Verify every command and label against
source. Keep historical documents visibly marked and never rewrite them as current truth.

`docs/features/` maps user-visible capabilities to entry points, existing checks, and known gaps.
Consult it before changing user-facing behavior, and update the affected feature file when a shortcut,
CLI command, rule semantic, restart/recovery outcome, or covering test changes.

## Release Rules

- A Developer ID-signed source build is not a public release artifact.
- Do not claim release readiness without notarization/stapling, a downloadable checksummed artifact,
  architecture and minimum-OS validation, and clean-machine install/upgrade/recovery/uninstall tests.
- Build once from a clean reviewed revision, notarize and distribute that exact artifact, and verify
  signature, ticket, architecture, plist, archive contents, and checksum after packaging.
- Keep the real app icon and add a concise demo before calling the preview broadly release-ready.
- Never tell artifact users they need a Developer ID identity. A local source build accepts
  Developer ID Application or Apple Development (`scripts/build-app.sh`); releases require Developer ID.
- Never ship instructions to disable SIP, erase recovery metadata before restoration, or imply Mac App
  Store support.
