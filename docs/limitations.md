# Known Limitations

Spaceman is an experimental public preview, not a broadly qualified release.

- The downloadable preview is currently arm64-only; x86_64 and universal packaging are not validated.
- The declared macOS 14 minimum lacks a documented validation matrix across supported macOS releases.
- Clean-machine install, permission, upgrade, recovery, and uninstall have not been release-qualified.
- The app depends on private `_AXUIElementGetWindow` behavior and cannot target the Mac App Store.
- Accessibility behavior varies by application; only standard, movable, on-screen windows can be
  workspace-owned. Nonstandard or immovable panels remain global.
- Native-tab identity has no perfect public identifier. Ambiguous transitions intentionally defer
  replacement and may need another cycle.
- After a reboot or application relaunch, remembered placements restore each window's workspace and
  floating state, not its tile position, weight, or nesting within the layout. When windows have no
  title or share the same title, windows of a multi-window application that reopen with identical
  frames are matched in creation order and may swap workspaces.
- Configuration supports ordered exact-match `float`/`tile`/`route`/`ignore` rules and display assignment. It
  intentionally does not support orthogonal named app spaces or regex title matching.
- Configuration changes to rules, display assignments, and `[settings]` apply automatically; newly
  referenced named workspace letters register their global shortcuts on the next app start. `ctl restart`
  exits safely but does not relaunch.
- Spaceman assumes one macOS Space per display. Windows on another Space are adopted into the active
  workspace when that Space is shown, and tiles whose windows are ordered out while it is shown may
  be released. Floats and pins on a Space that isn't shown keep floating until it is shown again,
  even when their app has a window on the Space being shown (#768). Layout waits about 0.7 s after
  each Space slide so the two Spaces' windows are not tiled together. A trackpad swipe shows the
  arriving Space's windows before macOS reports the change, which comes about a second after the
  fingers lift, so layout also waits once three or four fingers slide sideways on the trackpad and
  for about 1.5 s after they lift (#513, #572), even if a resting thumb lifts or a palm brushes the
  trackpad mid-swipe (#677); Spaceman's own workspace swipe delays layout the same way, while
  resting fingers do not. A Space change made from the keyboard (Control+arrow) is held only from
  when macOS reports it. A move, focus, resize, layout, float or pin shortcut, or the matching
  `spaceman ctl` command (`move`, `focus`, `move-direction`, `resize`, `balance`, `layout`, ...),
  sent during that 0.7 s, or within about 1.5 s after three or four fingers slide sideways on the
  trackpad (whether or not macOS reports a change), waits for it to end and then acts on the
  arriving desktop's windows (#770, #777). One sent between Control+arrow and the macOS report is
  not held and still acts on the desktop being left.
  Accessibility cannot reach a parked window on a Space that isn't showing, so pausing or quitting
  there waits, with its recovery entry kept, until that Space is shown.
- Hidden workspaces' windows are parked just past a display's bottom corner. With Mission Control's
  **Group windows by application** off (the macOS default), Mission Control lays windows out by
  position, so parked windows can shrink every thumbnail in the overview (#555). Turn it on in
  System Settings › Desktop & Dock › Mission Control, or run
  `defaults write com.apple.dock expose-group-apps -bool true && killall Dock`.
- `secondary` selects the first non-main display; display ordering and duplicate names may surprise.
- Global shortcuts may collide with other applications or fail on keyboard layouts that cannot
  produce required characters.
- Settings covers Accessibility, Launch at login, Workspace HUD, placement rules for an app's every
  window or for the windows a `title_contains` filter picks out, overrides for the built-in
  floats, workspace display assignments, keyboard shortcuts, and Activity diagnostics; exact
  `title` rules, `tile` rules for other apps and `settings.workspace_order` are edited in
  `config.toml`. There is no telemetry or analytics.
- The repository still lacks a concise product demo.

Use the isolated live harnesses in [Development](development.md) before changing window-management,
workspace, display, or recovery behavior.
