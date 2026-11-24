# Spaceman 🪐

Spaceman is an experimental macOS tiling window manager with a soft spot for windows that refuse to sit still. It is built around stable logical windows. A native window, including a selected Ghostty native tab, can change its physical Accessibility identity without creating a new tile. Spaceman keeps the logical slot and replaces its physical binding when the evidence is unambiguous. Same seat. New body. No drama. ✨

[Website](https://anoma.ly/spaceman) · [Download for Apple silicon](https://anoma.ly/spaceman/download/macos-arm64)

> [!WARNING]
> Disable AeroSpace, yabai, Amethyst, and every other window manager before first opening Spaceman
> or installing it from source. Two managers can repeatedly move the same windows. Keep the parked
> window state until Spaceman has successfully paused or quit and restored every window.

## What it does 🪟

Spaceman currently:

- starts automatic tiling when Accessibility permission and its global hotkeys are available;
- provides ten default numbered workspaces plus opt-in named workspaces A-Z, with 1-5 on the main
  display and 6-10 on the secondary display by default;
- assigns workspaces to `main`, `secondary`, a display UUID, or an exact display name, falling back
  to the main display when a preferred display disconnects;
- persists active workspaces, nested split layouts, tile order and weights, and parked-window recovery
  metadata across Spaceman restarts, and remembers each application's window placements by bundle
  identifier so reboots and startup recovery return windows to their workspaces;
- journals window arrangements and model/screen disagreements locally, summarized in Settings (`Activity`)
  and by `spaceman doctor`;
- shows a non-activating Workspace HUD capsule on each display on workspace switches,
  cross-workspace moves, and layout toggles (`[settings] hud = true`): a row of that display's
  workspaces, drawn as numbers and app icons by default or (`hud_style = "layouts"`) as their layouts with app
  icons, with a marker that travels to the one you are on. It sits above the bottom edge by default
  or below the top one (`hud_position = "top"`), follows quick switches without restarting, and
  honors Reduce Motion. `workspace_order` sets the order the HUD, menu bar menu, and three-finger
  swipes list workspaces in, such as the keyboard row `Q W E R T Y`;
- can register its signed app bundle with macOS to launch automatically at login;
- supports directional focus and movement across displays, drag-to-reorder/nest, mouse and keyboard
  resizing, balancing, and a workspace-wide Fullscreen Layout;
- keeps movable non-resizable and rule-floated windows with their logical workspace without tiling
  or resizing them;
- exposes a local CLI over a user-only Unix socket.

Spaceman uses Accessibility APIs to observe, focus, move, and resize eligible windows. It requires no
SIP change. Read [Security and privacy](docs/security-and-privacy.md),
[Compatibility](docs/compatibility.md), and [Limitations](docs/limitations.md) before trying it.

## Install the preview 📦

The current preview is a Developer ID-signed, notarized arm64 build for Apple silicon Macs. It remains
experimental; read [Known limitations](docs/limitations.md) before installing it.

1. Disable every other window manager. For AeroSpace: `aerospace enable off`.
2. Download, install and open the latest build:

   ```sh
   curl -fL https://anoma.ly/spaceman/download/macos-arm64 -o /tmp/Spaceman.zip \
     && ditto -x -k /tmp/Spaceman.zip /Applications && open /Applications/Spaceman.app
   ```

3. Follow the setup tour: grant Accessibility access, then press `Start Tiling`.
4. Optionally link the bundled CLI: `mkdir -p "$HOME/.local/bin" && ln -sf "/Applications/Spaceman.app/Contents/MacOS/spaceman" "$HOME/.local/bin/spaceman"`.

To check the download first, get
[Spaceman for Apple silicon](https://anoma.ly/spaceman/download/macos-arm64)
and its [SHA-256 checksum](https://anoma.ly/spaceman/download/macos-arm64.sha256),
run `shasum -a 256 -c Spaceman.zip.sha256`, then unzip it and move `Spaceman.app`
to `/Applications`.

Spaceman updates itself from Anomaly's release service: when an update is ready, choose
`Install & Restart` in Settings or the menu, or just quit and it installs for the next launch.
Spaceman restores every window first.
Builds after 0.3.0 are signed by Anomaly with the bundle ID `anomaly.spaceman`, so macOS asks for
Accessibility access once more after that update; your config, workspaces and parked-window recovery
carry over. If a manual replacement is necessary, pause and quit the running version and confirm that every parked
window has returned before replacing its bundle.

## Install from source 🛠️

Source builds are distinct from the notarized preview artifact. Sign a local build with your
`Developer ID Application` identity, or with `Apple Development` if that is what the keychain has.
Public releases stay on Developer ID.

Requirements: macOS, Xcode command-line tools, Rust 1.100.0, and one of those signing identities.

From an existing repository checkout, first disable every other window manager, then run:

```sh
./scripts/install-app.sh
```

Set `SPACEMAN_CODESIGN_IDENTITY` if auto-detection does not choose the desired identity. The script
builds and signs `target/Spaceman.app`, installs it at `/Applications/Spaceman.app`, links the CLI to
`~/.local/bin/spaceman`, and opens the app.

Ensure `~/.local/bin` is on `PATH`. For zsh:

```sh
printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$HOME/.zshrc"
exec zsh
```

Artifact users do not need a Developer ID identity. See [Releasing](docs/releasing.md) for the
maintainer workflow.

## First open 👋

1. Disable every other window manager. For AeroSpace: `aerospace enable off`.
2. Open Spaceman: `open "/Applications/Spaceman.app"`.
3. On a fresh install, Spaceman opens its three-step setup tour when Accessibility access is missing.
   Choose `Allow` and grant Spaceman access in **System Settings > Privacy & Security** (the
   Accessibility list, named System & App Protection Override on macOS 27.3 and later), optionally
   enable `Launch at Login`, then press `Start Tiling`.
4. Confirm operation with `spaceman ctl status`.

Automatic tiling starts when the setup tour finishes. A fresh install holds it while the tour is
open, so granting Accessibility never rearranges your windows behind the setup window. Without
Accessibility permission, the menu reports the permission error and tiling remains paused. While
tiling, the menu lists each display's workspaces (the visible one checked, plus any holding windows,
with their apps' icons); choosing one switches to it. The menu actions are `Setup Tour…` (while
Accessibility is missing), `Pause Tiling` (or `Resume Tiling` while paused), `Workspace HUD`,
`Launch at Login`, `Settings...` (`⌘,`), `Check for Updates...`, and `Quit Spaceman` (`⌘Q`). Settings
includes `General`, `Layout` (window gaps, screen-edge padding, and a single-window width for wide
displays, over a live preview), `Rules`, `Workspaces`, `Shortcuts`, and `Activity` (title-free arrangement
diagnostics and process vitals). If macOS requires approval, the login item action opens **System
Settings > General > Login Items & Extensions**.

## Everyday use ⌨️

Core controls:

| Keys | Action |
| --- | --- |
| `Control+1` ... `Control+9`, `Control+0` | Switch to workspace 1 ... 10 |
| `Option+Shift+1` ... `Option+Shift+0` | Move the focused window to workspace 1 ... 10 |
| `Option+A` ... `Option+Z` | Switch to a configured named workspace |
| `Option+Shift+A` ... `Option+Shift+Z` | Move the focused window to a configured named workspace |
| `Option+Tab` | Focus the workspace focused before this one, on any display |
| `Option+Shift+Tab` | Move the focused workspace to the next display |
| Option-backtick | Focus the next window in the workspace, tiled or floating |
| Option-Shift-backtick | Focus the previous window in the workspace |
| `Option+H/J/K/L` | Focus left/down/up/right |
| `Option+Shift+H/J/K/L` | Move the focused tile left/down/up/right |
| `Option+/` | Toggle the active layout's root split axis |
| `Option+-`, `Option+=` | Shrink/grow the focused tile by 50 pixels |
| `Option+Shift+F` | Toggle Fullscreen Layout without native fullscreen |
| `Option+Shift+Space` | Float the focused window above the tiles, or tile it again |
| Three-finger swipe left/right | Next/previous occupied workspace on the display under the pointer |

### Can't type @? We got you. 🌍

On a Portuguese macOS layout, `@` is Option+2. Spaceman used to grab Option+number for workspace
switching, which meant the `@` key never got a word in. Switch to space now defaults to
**Control+number**, never Option+number, so the character keys stay yours.

Those ten shortcuts are editable in **Settings › Shortcuts › Switching**. Click a row, press any
chord (or ⌫ for none). Esc cancels. Each choice is saved in `config.toml` and registered live.
**Reset to Defaults** puts them back on Control+number. If a chord types a character on the active
keyboard layout, the row says so. Spaceman never changes macOS Mission Control shortcuts. It only
looks at which of those are enabled, and when one already uses a chord, Spaceman leaves that key
alone and the row tells you.

The swipe shares three fingers with macOS's "Swipe between full-screen applications"; set that to
four fingers or off in Trackpad settings so only one of them moves.

Every shortcut can be rebound in `config.toml`'s `[shortcuts]` table, for example to match a Linux
tiling setup; see [Configuration › Shortcuts](docs/configuration.md#shortcuts). While the base
modifier is Option (the default), Switch to space stays on Control. Choosing another base modifier
moves those shortcuts with the rest of the built-ins.

Character shortcuts follow the active macOS keyboard layout. Workspace digits remain the physical
ANSI number row, and Tab remains physical Tab. See [Controls](docs/controls.md) for drag, resize,
balance, CLI, and socket behavior.

New windows join the most recently focused tile’s split. When an app closes its last window,
neighboring tiles can expand as soon as the app confirms the close. When apps refuse to fit side by side,
Spaceman can temporarily stack adjacent tiles and restore the requested split when space allows.

Commands and completed gestures wait in arrival order when the worker is busy; repeated resize or
focus commands are not collapsed into one. Workspace switches keep the outgoing windows visible if
incoming parked windows cannot finish restoring. Parking targets are checked against every attached
display, including middle displays in three-display arrangements.

```sh
spaceman ctl status
spaceman ctl switch 2
spaceman ctl move 7
spaceman ctl layout
spaceman ctl float
spaceman ctl balance
spaceman ctl pause
spaceman ctl resume
```

## Configure 🧩

Spaceman reads `~/Library/Application Support/Spaceman/config.toml` at startup and reloads rule,
display, and HUD (on/off, style, and screen edge) edits automatically when the file changes. See the exact current schema and
[config.example.toml](config.example.toml) in [Configuration](docs/configuration.md).

```sh
spaceman config-check "$HOME/Library/Application Support/Spaceman/config.toml"
spaceman displays
```

## Recover or remove 🛟

Use `spaceman ctl pause` or `spaceman ctl quit` and wait for `spaceman ctl status` or process exit to
confirm that restoration completed **before** removing the app or state. Do not delete
`parked-windows.json` as an early troubleshooting step: it records where off-screen parked windows
must return.

After a crash, recorded parked windows missing from otherwise valid workspace state are restored
before automatic reconciliation resumes. Recovery metadata stays intact until restoration and its
durable removal succeed.

Follow [Recovery](docs/recovery.md) for stuck or crashed processes and safe uninstall instructions.

## Documentation 📚

- [Configuration](docs/configuration.md)
- [Controls](docs/controls.md)
- [Recovery and uninstall](docs/recovery.md)
- [Security and privacy](docs/security-and-privacy.md)
- [Compatibility](docs/compatibility.md)
- [Known limitations](docs/limitations.md)
- [Development](docs/development.md)
- [Releasing](docs/releasing.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Historical implementation plan](PLAN.md) and [historical research](docs/RESEARCH.md)

## Status

Public experimental preview. The arm64 artifact is signed, notarized, stapled, checksummed, and served
through Anomaly's release service, which the app's own updater checks. Broad release readiness is not proven: minimum-OS runtime validation,
clean-machine install/upgrade/recovery/uninstall qualification, wider architecture support, and a
concise product demo remain outstanding.

## License

MIT
