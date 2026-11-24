# Workspaces

Keep windows grouped on numbered or named virtual workspaces per display, switch between them
instantly, and send windows or whole apps to a workspace, without using macOS Spaces.

## Sub-features

```ts
Switch workspace                                           // ws.switch
├── Control+1..0 (Option+A..Z for named) / spaceman ctl switch <ws> / a workspace in the menu bar menu
│   └── Each Switch to space shortcut is editable in Settings › Shortcuts › Switching; Control+number
│       is the default so Option+number can type a character. An enabled Mission Control shortcut
│       on the same chord is left for macOS.
├── Incoming parked windows revealed -> laid out -> outgoing windows parked off-screen
│   └── Reveal not confirmed by the window server in time -> switch rolled back, old workspace stays
├── Focus -> last-focused tile of the incoming workspace, keyboard focus included
│   (that window raised and made main, then its app brought forward with `AXFrontmost`), unless
│   one of its windows already has focus (e.g. focus follow): that window keeps it
└── Already active -> stays and re-focuses its remembered tile (keeps current focus if it is there)
    ├── Pressed again (`Control+<number>` / `Option+<letter>` / `ctl switch`) while focus is on that workspace -> the
    │   next tile takes focus, wrapping at the end of the tile order; the way to walk the shared
    │   frame in Fullscreen Layout
    ├── Focus on another display's workspace -> only brings focus back; the press after that cycles
    └── Menu bar or HUD click on the active workspace -> never cycles, keeps the current focus

Swipe                                                      // ws.swipe
├── Three-finger swipe left/right -> next/previous workspace in the menu section of the display
│   under the pointer (visible + occupied, in workspace order), stops at either end
└── Suppressed while a pointer button is down or a window drag is active (including macOS
    Accessibility "Three Finger Drag")

Back-and-forth                                             // ws.back-and-forth
├── Option+Tab / ctl back-and-forth -> the workspace focused before this one, on any display
│   (one global focus history; after a restart it starts from the focused display's lane history)
├── Target already on screen -> focus moves there, no display changes, nothing is parked
│   └── Press again -> bounces back to the workspace the focus came from
├── Target on screen but empty -> only shows its HUD; focus history is left alone
└── Target hidden -> its own display switches to it and focuses its remembered tile

Move workspace to another display                          // ws.move-workspace
├── Option+Shift+Tab / ctl move-workspace [next|prev|<display>]
├── Orders displays left-to-right; wraps around for next/prev; persists assignment to config.toml
├── With no visible window focused (an empty workspace), moves the workspace switched to last
├── Reveals & lays out source display's previous workspace -> lays out moved workspace on target
│   -> parks target's displaced workspace (which is only that display's lane history, even when
│   another selector for the same display, such as a UUID, assigns it)
└── Preserves Fullscreen Layout and focus on the moved workspace (focus history untouched); HUD on both displays

Move window without switching                              // ws.move-window
├── Option+Shift+<ws> / ctl move <ws> -> window parked on target, current workspace reflows
├── Target on screen on another display -> tiled there (a floating window is carried over, same
│   size and relative place); focus follows it and Option+Tab goes back
├── Target hidden on another display -> parked off the edge of the display it is on; its recovery
│   frame moves onto the target's display, so showing the workspace brings it up there
└── ctl move-app <bundle-id> <ws>     -> every current window of that app, tiled or floating, in one request

Mouse follows focus ([settings] mouse_follows_focus)       // ws.mouse-follows-focus
├── "off" (default) -> never moves the pointer
├── "display"       -> on explicit keyboard/ctl focus changes (switch, back-and-forth,
│                      move-workspace, focus, move onto an active workspace), if the pointer is not
│                      already on the target display, warps to the center of the focused window (or
│                      the target display when empty)
├── "window"        -> same explicit triggers, warping whenever the pointer is outside the newly
│                      focused window (or empty display)
└── Never warps on passive cycles, focus-follow, Cmd+Tab, mouse clicks, swipes, drags, or while a
    mouse button is held; re-associates mouse movement immediately after warping

Focus follows                                              // ws.focus-follow
├── Focusing a window that lives on a hidden workspace (Dock, Cmd+Tab, notification)
│   -> that workspace becomes active, with focus on that window (not its remembered tile)
│   └── Switch interrupted by that same activation -> retiles every 40 ms for 500 ms and follows it
│       as soon as Accessibility catches up
├── Focusing a window on another display's visible workspace -> no switch; Option+Tab remembers it
└── No focused window reported -> follows the frontmost app only when all its windows, tiled
    or floating, are on one workspace

Where a new window goes (first match wins)                 // ws.adoption
├── ignore rule        -> never managed
├── route rule         -> that workspace (parked if hidden); bundle-only route rules apply to the
│                         app's initial window while secondary windows stay on the active workspace
├── remembered placement (startup window, first 60 s, or the app relaunched within 30 s of
│   quitting) -> remembered workspace (floating if matched by a float rule, dialog/non-resizable
│   without a tile rule, or remembered as floating)
├── float rule / dialog / non-resizable (no tile rule) -> floating on the active workspace under the window's center
└── otherwise          -> tiled on the active workspace of the display under the window's center

Displays                                                    // ws.displays
├── Default: 1-5 main, 6-10 secondary; [[workspaces]] monitor = main | secondary | UUID | name
├── Settings › Workspaces offers every connected display: Main, Secondary, then each further
│   display as its own picker segment and section card (numbered Display 3/4 past three
│   displays); picking one writes its UUID, the same selector Option+Shift+Tab persists, and a
│   saved UUID renders back as that display's name
├── Preferred display unplugged -> its workspaces fall back to main, return on reconnect; its
│   saved assignment keeps a Disconnected displays card with the picker on Not connected
└── Display added or removed -> hidden windows re-parked against the new displays; visible owned
    windows left with their center off every remaining display keep their slots and snap back onto
    their workspace's display

Workspace order ([settings] workspace_order)                // ws.order
├── The order the HUD row, the menu bar menu and three-finger swipes list a display's workspaces
├── Listed workspaces first, in the order given; the rest follow as 1-10, then A-Z
├── Unset or empty -> 1-10, then A-Z, as before
├── Empty workspaces flicked through while the HUD is up stay in the row where the order puts them
├── Only where workspaces are listed: never enables a letter; shortcuts, routes, and saved state
│   are unchanged
├── Unknown or repeated entry -> config rejected
└── Applies on config reload; removing it brings back the default order

Workspace HUD ([settings] hud, on by default)               // ws.hud
├── A switch, a window moved to another workspace, a split-axis or Fullscreen Layout toggle
│   -> a capsule on every display, each with its own row: every workspace on that
│      display holding windows, plus the visible one, in workspace order; a marker on the visible one
├── hud_position = "bottom" (default) -> the capsule rests 60 pt above each display's bottom edge,
│   clear of the Dock; "top" -> 60 pt below its top edge, clear of the menu bar
├── hud_style = "icons" (default) -> each workspace as its number and its apps' icons (one per app,
│   │  tile order); the visible group bright, the others dimmed; nothing changes size
│   └── "layouts" -> each workspace as its layout with an app icon per tile, the visible one larger
│      (others .62×, dimmed); an empty one is a dot; Fullscreen Layout is one tile with the front
│      app's icon and the others smaller beside it
├── No text but workspace numbers, and never a window title; icons are looked up by bundle ID
├── Appears: fades in, scales from .96 and moves 6 pt into place (a bottom capsule rises, a top
│   one drops); stays 1.15 s after the last event; leaves the same way back in about 0.17 s
├── Interactive without stealing focus (OverlayInput::Regions, KeyPolicy::None):
│   ├── Hover over the capsule -> pauses auto-dismiss (and rescues a mid-exit capsule); resumes
│   │   0.6 s after the pointer leaves
│   ├── Click a workspace slot -> switches that display to that workspace without activating
│   └── Right-click (or Ctrl+click) an app icon -> native Float/Tile and Pin/Unpin menu writing
│       through config.toml rules
├── Another switch while it's up, or on its way out -> the same capsule carries on from where it
│   is: the marker's leading edge sets off first and its trailing edge catches up; in Layouts the
│   cells grow and shrink and their neighbours slide. Only the display that switched moves its
│   marker; every capsule holds and leaves together
├── Shown again after hiding -> the marker (and in Layouts the large cell) starts on the
│   workspace the switch left and travels as the capsule fades in
├── A workspace joins or leaves the row -> it fades and scales in place; a moved window's icon
│   leaves one workspace and appears in the other; the marker stays with you
├── The same workspace's layout changes -> its tiles move to their new frames
├── More workspaces than fit -> the row scrolls to keep the visible one in view, its cut edge fades
└── Reduce Motion -> the same changes as a short fade, with nothing travelling or scaling
```

Hidden windows are **parked**, not minimized: moved past a display's bottom corner so at most a
one-pixel-wide slice of the title bar, which macOS keeps on screen, remains visible, with the original
frame persisted to `parked-windows.json` first.

## How To Get To It (User POV)

```ts
Keyboard (while tiling is enabled)
├── Control+1..0 / Option+A..Z  -> switch; pressed again on the active workspace -> next tile
│                                  (wraps, and is how Fullscreen Layout tiles get focus)
├── Option+` / Option+Shift+`   -> next / previous window on the workspace, floats included
├── Option+Shift+<1..0 | A..Z>  -> move focused window
├── Option+Tab                  -> back-and-forth (last two workspaces focused, any display)
└── Option+Shift+Tab            -> move focused workspace to next display
Trackpad: three-finger swipe left/right -> next/previous workspace (turn macOS's three-finger
          "Swipe between full-screen applications" off or move it to four fingers)
CLI: spaceman ctl switch | back-and-forth | move-workspace | move | move-app
Config: ~/Library/Application Support/Spaceman/config.toml -> [[rules]], [[workspaces]],
        [settings] workspace_order (edits apply when the file changes; new named-workspace shortcuts register on the next app start)
Menu bar: shows the active workspace glyphs; its menu lists each display's visible workspace (checked)
          and occupied ones with their apps' icons and shortcut -> choose one to switch
Mouse: drop a tile on another display -> moves it to that display's active workspace (see tiling.md)
```

Named-workspace keys exist only for letters referenced in config; they override built-in shortcuts
on the same letter.

## Existing Checks

- Unit (FakeBackend), `src/app/tests/switching.rs`:
  `switching_reveals_and_lays_out_incoming_beneath_before_hiding_outgoing` (ordering of reveal,
  layout, hide), `switch_keeps_outgoing_visible_when_incoming_reveal_never_arrives`
  (`reliability.rs`, rollback), `moving_to_an_inactive_workspace_does_not_switch`,
  `focusing_a_parked_window_activates_its_workspace`,
  `following_focus_to_a_hidden_workspace_keeps_the_window_that_took_it`,
  `a_follow_cancelled_by_the_activation_it_answers_follows_on_the_next_cycle` and
  `a_follow_that_fails_while_an_activation_cancels_stays_suppressed` (`focus.rs`, notification
  clicks),
  `workspace_back_and_forth_bounces_between_the_last_two_focused_workspaces`,
  `workspace_back_and_forth_on_one_display_switches_like_before`,
  `switching_to_an_unconfigured_named_workspace_is_rejected`.
- Unit (FakeBackend), `src/app/tests/workspace_order.rs`: `settings.workspace_order` orders
  `listed_workspaces` (unlisted workspaces follow in the default order; an entry neither enables a
  letter nor lists an empty workspace), the HUD row and three-finger swipes, and a config reload
  applies and clears it. Unit, `src/config.rs`:
  `workspace_order_accepts_numbers_and_letters_in_the_given_order`,
  `workspace_order_rejects_duplicate_and_unknown_workspaces`, and
  `saving_a_workspace_monitor_keeps_the_workspace_order` (Settings › Workspaces); CLI,
  `src/cli/tests/regressions.rs`: `config_commands_keep_and_show_the_workspace_order`; agent,
  `agent/src/hud/tests.rs`: `visited_empty_workspaces_keep_the_configured_workspace_order` (empty
  workspaces flicked through during one HUD show stay where the order puts them).
- Unit (FakeBackend), `src/app/tests/switch_repeat.rs`: a repeat `SwitchWorkspace` walks a
  3-tile workspace A -> B -> C -> A in split and Fullscreen Layout (remembered focus and HUD
  included), warps the pointer onto each next tile under `mouse_follows_focus`, steps on inside
  the focus settle window while Accessibility still reports the old window; with focus on another
  display's workspace the first press only re-focuses and the next one cycles; a float hands focus
  to the remembered tile; a menu-bar/HUD `SelectWorkspace` on the active workspace never cycles;
  a single-tile workspace stays put.
- Unit (FakeBackend), `src/app/tests/cross_display.rs`: moving a window to a workspace on
  another display, on screen (tiled, floating, focus history) or hidden (parked on its own display
  even when a write off the other display's edge is refused, shown on the target display later,
  left where it was when it cannot be parked), dragged floats parking off their actual display on
  switch or move (issue 148) and relocating from a third display onto the target display (issue
  152), and `move_app_moves_every_managed_window_of_the_app` (issue 121) and
  `move_app_moves_floating_windows_too` (issue 136). Unit, `src/macos/apply.rs`: a refused size write
  before a move no longer stops the move; focusing raises the window before bringing its app
  forward, and either `AXFrontmost` or AppKit activation brings the app forward (issue 92).
- Unit (FakeBackend), `src/app/tests/focus.rs`:
  `a_workspace_switch_that_cannot_focus_keeps_the_focus_restore_pending`,
  `a_workspace_switch_that_focuses_settles_the_focus_restore`,
  `reselecting_the_active_split_workspace_keeps_the_focused_tile` (issue 120),
  `leaving_with_a_float_focused_keeps_the_remembered_tile` (`float.rs`, issue 138), and the `mouse_follows_focus_*`
  tests covering `off` / `display` / `window`, empty-display centering, same-display and
  in-window skips, `move-workspace` / `move`, passive `cycle()` focus-follow suppression, and
  `mouse_down` guarding (plus `warp_reassociates_immediately_after_warping_and_skips_while_mouse_is_down`
  in `src/macos/apply.rs`). Whether keys really land in the incoming app on macOS 14 and later,
  and live `CGWarpMouseCursorPosition` behavior on hardware displays, are not covered by a test.
- Unit (FakeBackend), `src/app/tests/move_workspace.rs`:
  `move_workspace_to_occupied_display_promotes_source_previous_and_parks_displaced`,
  `option_tab_after_moving_the_focused_workspace_never_reveals_the_displaced_one` (issue 76),
  `workspace_back_and_forth_switches_a_hidden_workspace_on_another_display`,
  `cycle_records_a_click_on_another_display_for_option_tab`,
  `move_workspace_to_empty_display_and_three_display_wrap_around`,
  `move_workspace_single_display_is_a_noop`,
  `move_workspace_preserves_fullscreen_layout_and_focused_tile`,
  `move_workspace_persisted_assignment_survives_config_reload`,
  `move_workspace_replaces_unplugged_assignment_and_rolls_back_on_error`;
  `src/app/tests/empty_workspace_move.rs` (an empty workspace on a non-main display, issue 201);
  `src/app/tests/dual_selector_move.rs` (a display with a UUID and a `secondary` selector, issue 202).
- Unit (FakeBackend), `src/app/tests/reconnect_unsettled.rs`: a tile whose display disconnects
  before its layout settles returns to its tile on reconnect, and a refusing app stays frozen (issue 203).
- Unit (FakeBackend), `src/app/tests/tear_regressions.rs` (sequences found by model-based testing):
  `switch_after_moving_a_window_there_survives_async_activation`,
  `frontmost_app_with_a_visible_floating_window_does_not_switch_away`,
  `reconnecting_a_display_keeps_parked_windows_hidden_and_owned`,
  `a_failed_re_park_after_a_display_change_is_retried`,
  `a_display_change_leaves_a_held_up_parked_window_alone`,
  `a_display_that_grows_at_the_bottom_re_parks_its_held_up_windows`,
  `a_failed_re_park_survives_a_second_display_change`,
  `re_parking_a_grown_window_hides_all_of_it`,
  `switching_away_parks_a_window_that_overflows_the_display_edge`,
  `laying_out_a_parked_window_retires_its_stale_recovery_frame`.
- Unit, `agent/src/panes/workspaces.rs`: the display picker with two displays (Main | Secondary
  kept, section captions name the real displays), three displays (two identically named externals
  told apart as `Secondary` and `Studio Display 2`, picking the third writes its UUID, a saved
  UUID renders as its label, a saved UUID for Main resolves to Main), one display (Secondary still
  offered), an unplugged display (an extra `Not connected` segment, its `config.toml` entry
  kept), and four displays (numbered segments `Display 3`/`Display 4`, names kept in the
  headings). Section cards and segments themselves are seen in the Playground
  (`Workspaces/workspaces-three-displays`, `Workspaces/workspaces-four-displays`,
  `Workspaces/workspaces-unplugged`), not by a test.
- Unit, `src/macos/displays.rs`: `selector_for` (`main`/`secondary`/UUID) and `ordered` (left to
  right), shared with `Option+Shift+Tab`.
- Unit (FakeBackend), `src/app/tests/switching.rs`:
  `status_menu_lists_each_displays_visible_and_occupied_workspaces` (menu rows per display, checks,
  app order). The AppKit rows themselves (icons, tooltip of app names) are not covered.
- Unit: `swipes_step_through_the_occupied_workspaces_of_the_display_under_the_pointer`
  (`src/app/tests/switching.rs`), the recognizer in `src/swipe.rs` (fires once per swipe; ignores
  short, vertical and four-finger swipes), pointer-button / three-finger-drag suppression in
  `src/macos/swipes.rs` and `src/app/input.rs` (issue 151), and visible stranded-window recovery
  after display disconnect in `src/app/tests/display_disconnect.rs` (issue 150). Live CGEventTap
  callback delivery in `src/macos/swipes.rs` is not covered.
- Unit (FakeBackend), `src/app/tests/reconciliation.rs`: adoption order
  (`bundle_route_applies_to_initial_window_while_secondary_windows_stay_on_active_workspace`,
  `title_route_rule_routes_matching_secondary_window`,
  `ignore_rule_releases_the_window_and_never_adopts_it`,
  `floating_rule_owns_window_without_adding_it_to_layout`,
  `new_window_opens_on_active_workspace_even_when_app_has_window_on_inactive_workspace`), display fallback
  (`missing_preferred_display_falls_back_without_double_visibility`,
  `active_preferred_workspace_falls_back_and_returns_after_hotplug`).
- Unit (FakeBackend), `src/app/tests/parking.rs`: parking without minimizing, re-parking windows that
  drift back, honoring windows that keep un-parking (parking exemption, working tree), and keeping
  slots when Accessibility briefly cannot find a hidden window.
- Unit, `src/geometry.rs`: `parking_leaves_a_single_corner_pixel_on_screen`,
  `a_window_held_above_its_parking_corner_is_still_parked`,
  `a_window_held_up_by_its_title_bar_stays_off_a_shorter_neighbor`,
  `a_window_held_up_stays_off_a_neighbors_menu_bar`,
  `a_side_dock_moves_parking_to_the_other_corner`, and
  `a_display_below_falls_back_to_the_edge_strip_not_its_corner`: parking targets a bottom corner,
  accepts macOS holding the window up by its title bar, and keeps every held-up position off other
  displays and off menu bars and Docks; when a display below blocks both corners it parks along its
  own display's side edge rather than another display's corner. How far macOS holds it up is not
  covered.
- Unit (FakeBackend), `src/app/tests/reconciliation.rs`:
  `an_app_that_quits_while_hidden_releases_its_tiles_and_recovery_metadata` (an app that quits
  while its windows are parked frees their tiles without the workspace being shown).
- Unit: `src/hotkeys/delivery.rs` pairs Option presses seen by the key tap with Carbon deliveries
  in either order and reports a press still undelivered after 750 ms. The tap
  (`src/macos/keys.rs`) and the re-registration in `src/app/app_loop/hotkey_health.rs` are not
  covered.
- Live harness (not run): `scripts/live-workspaces.sh` switches 6/7/8 by real hotkeys, moves without
  switching, rapid-switch coalescing, and hard-kill recovery. Needs two displays and AeroSpace.
- Live gauntlet, `scripts/gauntlet.sh`: `ctl switch` parks every window to at most a one-point strip,
  records each original frame in `parked-windows.json`, restores the exact frames on the way back and
  empties the file; `ctl move` parks the window with its frame recorded.
- Observed (2026-11-24, gauntlet): after `ctl move`, the moved (now parked) window keeps keyboard
  focus and its app stays frontmost. Spaceman's in-memory focus suppression stops it following
  that focus, but a relaunch in that state follows it to the target workspace.

- Unit, `src/app/tests/dispatch.rs` and `src/app/tests/switching.rs`: the HUD event's rows per
  display, the visible workspace on each, and `from` only on the display that switched.
- Unit, `agent/src/hud/tests.rs`: when the HUD shows, holds, leaves and comes back, a fresh show
  starting on the workspace the switch left, the Layouts row's width, the Icons apps, which
  panel each display keeps as displays come and go, and the top/bottom geometry (the panel's
  edge, the capsule's 60 pt inset, and the 6 pt entry offset's direction). `agent/src/hud/motion_tests.rs` steps the
  overlay's own view at 120 Hz on a virtual clock, for both styles, and reads back each frame's
  presented rects, opacity and scale: a burst of switches, a hide and a show pass Rune's motion
  gate (no pop, jump, cut fade or overshoot); the marker goes one way per switch, its leading
  edge first, and a switch mid-flight carries it on without slowing; shown again, it starts where
  the switch left; the other display's row stays at rest; a moved window's icon changes groups;
  the widest row fits the panel with the marker in view; Reduce Motion moves nothing. Not
  covered: the panels themselves (ordering in and out, the first show's fade, moving between
  displays, a display unplugged while one is up) and how the motion looks on a display.
  Multi-display has not been seen on real hardware.

## Gotchas

- Unit tests model frames, not z-order or paint timing: "outgoing parked after incoming laid out"
  does not prove nothing flashes. See the stacked-window flash in
  [Fullscreen Layout](fullscreen-layout.md#gotchas).
- Some apps (Electron, Ghostty) move their own windows back on screen; Spaceman re-parks, then after
  repeated undoing honors the reappearance and re-homes the window to the visible workspace. In the
  working tree, a window that does so twice in five minutes stays tiled but is no longer hidden
  across switches (`parking_exempt`).
- Focus-follow switches workspaces as a side effect of any app activation, including ones the user
  did not intend (an app raising itself). Each `focus follow` line in `agent.log` records recent
  input timing and whether Command was held, so Command-Tab can be told apart from self-activation.
- Since macOS 14, AppKit activation from a background app is cooperative and ignored, so a switch
  that only called it showed the incoming window while keys stayed in the app focus left (issue
  92). Spaceman sets `AXFrontmost` on the incoming app instead. When focusing still fails,
  `agent.log` gets `switch focus failed: workspace <ws> window <id>` (never a title).
- macOS keeps a parked window's title bar on screen, so a one-pixel-wide slice of it stays at the
  display's bottom corner: about 32 px tall for a plain title bar, 66 px with a unified toolbar.
- Some apps (Arc) answer `AXFrontmost` with success and still stay behind the app focus left.
  After focusing, Spaceman polls the frontmost app every 10 ms and repeats the focus every 50 ms,
  at most three times (200 ms bound).
- Panels and non-standard windows are never parked, so they show on every workspace; an empty
  workspace exposes whatever unmanaged windows sit behind managed ones.
- macOS can stop delivering Carbon hotkeys to a running agent (seen twice on Nov 9, 2026, cause
  unknown). A listen-only key tap sees an Option shortcut Carbon didn't take; after 750 ms the agent
  logs `hotkey: <name> pressed but macOS did not deliver it`, registers every shortcut again (at
  most every 5 s) and runs the missed action. Each delivered press logs `hotkey: <name>`.
- Config edits (rules, display assignments, and HUD settings) reload automatically before the next
  command; new named-workspace hotkeys register on the next app start, and `ctl restart` exits
  without relaunching.
- The HUD's first appearance after launch only fades in: its panel has drawn nothing yet for the
  capsule to rise from. Every later one enters with the full motion.
- Try the HUD's motion without switching workspaces in the Playground (`⇧⌘D`, HUD > hud-switch):
  both styles at once. `N`/`P` step a workspace, `R` and `B` switch several quickly, `M` moves
  a window, `L` changes the layout, `H` hides it and shows the next workspace.
