# Tiling

Arrange a workspace's windows as non-overlapping tiles in nested horizontal/vertical splits, and
navigate, reorder, nest, resize, and balance them from the keyboard or the pointer.

## Sub-features

```ts
Automatic tiling                                        // tile.auto
├── New managed window -> follows settings.layout ("dwindle" default, or "columns")
│   ├── dwindle -> splits the focused (or last) tile along its longer side (landscape: 1 | (2 / (3 | 4)); portrait: 1 / (2 | (3 / 4)))
│   ├── columns -> joins the last focused tile’s split as a sibling, or the root without a surviving focus
│   └── Creation or activation before the first visible frame -> briefly recheck for the foreground window
├── Window closes, moves away, or floats -> its slot retires, siblings reflow
│   ├── In dwindle -> auto-chosen splits inside the space that grew re-orient outermost first to their new longer side (order and weights kept; user-arranged splits and legacy splits without a recorded origin keep their direction, and when such a split, root or nested, is left with one tile the next new tile joins it in that direction, #507, #611)
│   ├── Close and open seen together in dwindle -> the new tile splits the survivor by the space it has after the close, not its earlier minimum-size fit (#599)
│   ├── Last app window closes but its surface lingers -> siblings reflow when the app confirms closure
│   ├── Window closes while parked on a hidden workspace (app still running) -> forgotten once the window server stops listing it, so the workspace no longer counts as occupied for swipes and workspace steps (#553)
│   └── App keeps its ordered-out window alive -> slot settles for two seconds
└── Native tab replaces its window -> same slot, order, weight, nesting
    ├── Any app (Finder, Safari, Terminal...) -> when the evidence is unambiguous (one window out, one in, focus moved to it, at the tile's frame or, for an app that caps its window size, where the old tab was)
    │   ├── Under a route rule with windows = "all" -> the incoming tab still keeps the slot on its tile's workspace, not the rule's
    │   │   └── Caught mid-switch (old tab still listed, or focus not there yet) at the tile's frame -> held up to a second for the evidence instead of routed, then routed as a new window (#706), unless the tile's window is still there and focused: the app's own Move Tab to New Window, so the held window keeps the tile and the moved-out tab is routed (#756)
    │   ├── A snapshot mid-switch with no focused window -> the app's last focused tab still counts as the outgoing one, so the incoming tab keeps the slot (#718)
    │   ├── Right after a pointer resize is released -> the incoming tab keeps the resized tile's side and width (#624)
    │   ├── Selected while the tile's title-bar drag is still held -> the release commits the drop and the incoming tab, where the drag left the group, keeps the tile and its split sizes (#750)
    │   └── Old tab ordered out before the new one is on screen -> while another tab of the group is listed at the tile's corner, the tile waits up to 300 ms for it, so it still keeps side and width (#707), also when the new tab is on screen but not yet listed by Accessibility (#719), and when the app has other tiles, such as a second tab group focus passes through first (#749); a close with no other window of the app at that corner does not wait
    ├── Selected while Pause shows its tile's hidden workspace -> on Resume the tab now shown, the one window of the app at the group's corner, takes the tile and parks with its workspace, so showing that workspace is never blocked by the tab left behind; a window parked before that cycle (another Space, or closed but kept alive) keeps its tile and recovery frame (#753)
    ├── Ghostty -> ambiguous evidence also holds the slot for a bounded settle
    └── In a floating tab group -> the incoming tab, the one new window of the app where the outgoing tab was, takes its float (workspace, manual or automatic, pin), so the group stays floating where it was and parks with its workspace; caught mid-switch with the old tab still listed at that frame, it waits up to a second before it is adopted as a new window (#747); right after a desktop switch, a window of the same app at the float's frame on the arriving desktop neither takes the float nor waits for it (#769)

Focus                                                   // tile.focus
├── Option+H/J/K/L / ctl focus <dir> -> nearest tile in that direction, crossing displays
│   ├── [labs] recent_focus -> of several tiles bordering the focused one on that side, the most
│   │   recently focused (in-memory, per tile slot; floats don't count), else the nearest;
│   │   only the part of the edge a window really covers counts, so a size-capped window
│   │   doesn't border across the gap it leaves (#648)
│   ├── Into another display's workspace -> the window it left (tile or float) is remembered, so
│   │   Option+Tab back refocuses it (#639)
│   └── In Fullscreen Layout -> previous/next tile in stable order (see fullscreen-layout.md)
├── Option+` / ctl focus next        -> next window: tiles in stable order, then floats, wrapping
│   ├── Option+Shift+` / ctl focus prev -> the same ring, backward
│   ├── From a pin whose home is hidden -> the ring of the workspace its display shows, entered
│   │   at its first window (prev: its last); the pin's home stays hidden (#592)
│   └── Within ~0.7 s after macOS reports a desktop change, or ~1.5 s after three or four fingers
│       slide sideways on the trackpad and lift (before macOS reports it) -> waits for the layout
│       hold, retiles, then cycles the arriving desktop's windows (#770, #777)
├── For 250 ms after a focus command, a report of a window it left (or of no window at all) still
│   counts as its target: quick presses, numbered moves, Option+Tab, Dwindle's next split and focus
│   follow all start from the window just focused
└── ctl debug and spaceman hud print the targets these keys go to, from the same resolver

Reorder                                                 // tile.reorder
├── Option+Shift+H/J/K/L / ctl move-direction <dir> -> swap/move within the tree
│   ├── At the workspace edge -> moves to the adjacent display's active workspace, even an empty one
│   │   └── Focus goes with it, and Mouse follows focus brings the pointer (drag drops never warp)
│   └── Perpendicular move    -> re-orients or creates a mixed split (falls through to the adjacent display if the saved split would not change)
└── Pointer drag (non-destructive previews against drag-start layout; committed only on mouse-up)
    ├── Display left/right edge band -> full-height half-width column on that side (includes outer gap, armed once the pointer has been outside the band; lone tile unchanged; saved layout edited even while temporarily stacked)
    ├── Portrait display top/bottom edge band ([labs] portrait_edge_rows) -> full-width half-height row on that edge (same rules; includes the menu bar and Dock strips; side bands win in the corners, the strips' corners included, and a side band's strip ends do not arm it)
    ├── Over a sibling     -> live-previewed swap, applied on mouse-up
    ├── Near a tile edge   -> nested split (in the saved layout, even while tiles are temporarily stacked)
    └── Onto another display -> moved to that display's active workspace (restoring siblings left behind)

Size                                                    // tile.size
├── Option+- / Option+= / ctl resize shrink|grow -> 50 px at the immediate split
│   └── Clamped at every minimum, even one a window first reveals during the resize
├── Pointer resize       -> split weights follow both edges of a corner drag, preview during the gesture
│   └── On release         -> every tile is written exactly to its target, even a point or two off
├── ctl resize width|height <points> -> resize the focused window to that width/height at the
│   nearest split on that axis; siblings give or take in proportion; clamped at minimums; lone tile unchanged
├── ctl balance          -> equalize the focused tile's immediate split (no default hotkey)
└── Window refuses its tile size (app minimum) -> its tile grows to fit and siblings give up
    the space, every moved seam written exactly (#629); if necessary, try stacking an adjacent pair

Axis                                                    // tile.axis
└── Option+/ / ctl layout -> flip the root split axis (also exits Fullscreen Layout)

Spacing                                                 // tile.spacing
├── settings.gaps.inner              -> exactly that many points between neighboring tiles, any nesting
├── settings.gaps.top/bottom/left/right -> outer padding; gives way above a 320×240 tiled area
├── settings.gaps.outer              -> padding of each of those edges the file leaves out; an edge
│   with its own key keeps it, even at 0
├── settings.single_window_max_width -> a lone tile (one tiled window, or Fullscreen Layout) centered
│   at that width when the padded area is wider
└── settings.single_window_max_height -> with [labs] single_window_height, that lone tile also
    centered at that height when the padded area is taller (both axes at once with both caps)
```

## How To Get To It (User POV)

```ts
Keyboard: Option+H/J/K/L, Option+Shift+H/J/K/L, Option+`, Option+-, Option+=, Option+/
Pointer:  drag a managed window (reorder/nest/move display), drag its edge (resize)
CLI:      spaceman ctl focus | move-direction | resize shrink|grow|width|height | balance | layout | retile
Shortcut: [shortcuts.bindings] can bind any of those, e.g. "ctrl+alt+w" = "resize width 1200"
Settings: Settings › Layout -> New windows (Dwindle | Columns), Window gaps (Customize separately: Between windows, Screen edges, Top/Bottom/Left/Right edge), Wide displays: Single-window width (and, with Labs › Windows › Single-window height, Single-window height)
Config:   spaceman config set layout dwindle|columns; spaceman config set gaps.inner|gaps.outer|gaps.top|gaps.bottom|gaps.left|gaps.right|single_window_max_width|single_window_max_height <points>
```

Input arriving while the worker is busy queues in order (up to 64); queued input is dropped on
pause, quit, or hotkey loss. A `spaceman ctl` command is admitted to the queue before it replies,
so a full queue fails that command with `Input queue full; command was not queued` (#712).

## Existing Checks

- Unit (FakeBackend), `src/app/tests/layout.rs`: `focus_navigation_follows_linear_tile_order`,
  `focus_navigation_crosses_to_the_adjacent_display`,
  `workspace_focus_cycle_wraps_through_tile_order_in_every_layout`,
  `keyboard_resize_applies_and_persists_the_focused_layout`,
  `balance_applies_and_persists_the_focused_parent_split`,
  `resize_to_applies_and_persists_the_focused_window_width`,
  `resize_to_requires_leaving_fullscreen_layout`,
  `reorder_moves_to_the_adjacent_display_at_a_tile_boundary`,
  `reorder_moves_to_an_empty_workspace_on_the_adjacent_display` (issue 119),
  `perpendicular_reorder_creates_a_mixed_split_for_three_windows`,
  `layout_toggle_repartitions_and_persists_the_focused_workspace`.
- Engine and layout tree: `dwindle_landscape_splits_one_through_four_windows`,
  `dwindle_portrait_splits_vertically_first_then_alternates`,
  `dwindle_splits_focused_anchor_and_collapses_on_close`,
  `dwindle_keeps_a_flipped_root_axis_when_one_tile_is_left_and_another_joins`,
  `dwindle_ignores_a_stale_fit_when_a_tile_closes_as_another_opens`,
  `columns_mode_preserves_flat_sibling_insertion`, `new_window_joins_the_focused_nested_split`,
  `dwindle_splits_the_focused_tile_along_its_longer_side`, and
  `corner_drag_moves_both_nested_boundaries` cover Dwindle/Columns insertion and diagonal resizing;
  `src/app/tests/dwindle.rs` covers live landscape/portrait Dwindle tiling, close/move/float
  re-orientation of auto-chosen splits (3 and 4 windows), preservation of manual `Option+/` /
  drop / move splits across restarts, untouched sibling splits and weights, legacy state loading,
  live config reload between Dwindle and Columns, cross-workspace moves, axis toggle, and Fullscreen Layout;
  `resize_to_*` in `src/layout_tree/tests.rs` cover the nearest split on each axis, proportional
  sharing, recursive minimums, and lone tiles.
- Unit (FakeBackend), `src/app/tests/three_display.rs` and `src/navigation.rs`: three-display
  directional focus from an empty workspace (parked window reported at corner/held-up or no focus,
  #355) and from top/bottom tiles of tall portrait side displays that do not overlap the middle
  display (#356), plus dropping a dragged tile over another display's menu bar (#358).
  `tall_portrait_move_reaches_full_requested_height` (#428): a window moved onto a taller portrait
  display whose size macOS fit to the display it left is written again on the next cycle instead
  of being frozen as refused; `a_window_short_of_its_tile_on_its_own_display_still_freezes` keeps
  the freeze for a window that refuses its frame where it is. `src/macos/apply.rs`
  `a_window_from_another_display_that_lands_short_is_sized_again` covers the native follow-up size
  write; the live clamp itself is not covered.
- Unit, `src/navigation.rs` and `src/runtime.rs`: `[labs] recent_focus` picks the most recently
  focused bordering tile and never a further one (`the_most_recent_bordering_tile_wins`,
  `recency_never_reaches_past_a_nearer_tile`), a short shared edge of a few points still borders
  while a 1-point corner miss does not (`a_short_shared_edge_still_borders`, #525), a window
  capped short of its tile does not border across the gap
  (`a_window_capped_short_of_its_tile_does_not_border_across_the_gap`, #648) while one capped
  along the direction still borders (`a_window_capped_along_the_direction_still_borders`), and the
  per-slot order ignores floats (`focus_recency_orders_tiles_and_ignores_floats`) and drops closed
  tiles, so a reused slot ID starts unranked (`a_reused_slot_id_starts_unranked`).
- Unit (FakeBackend), `src/app/tests/recent_focus.rs`: with `1 | (2 / 3)`, left and back from 3
  returns to 3 with the Lab on (also when the second press comes before the new focus is
  reported) and lands on 2 with it off, and lands on 2 when 1's window is capped to 2's height
  (`a_window_capped_short_of_its_tile_borders_only_what_it_reaches`, #648); the `ctl debug`
  targets match the keys. No live test
  covers it yet.
- Unit (FakeBackend), `src/app/tests/gestures.rs` and `src/app/tests/drag_path.rs`:
  non-destructive drag previews (path-independent side drops, fast cross-display drops, cancel
  restoration), display outer left/right edge column drop bands (full edge sweep, outer gaps,
  band arming after leaving the band, temporary stacks, Fullscreen Layout), portrait top/bottom
  row drop bands in `src/app/tests/drag_rows.rs`, edge-nesting preview,
  cross-display drop, a layout toggle while the button is held surviving the release (#524), a held cross-display drop whose destination display switched
  workspaces landing on the one shown at release (#550) or whose destination workspace moved to another
  display (#605); `startup.rs` covers gesture classification and `src/app/input.rs` covers
  queued final gestures, pointer bounds tracking while busy, and edge drags grabbed from outside
  the window (in a gap or over the neighbor), which `src/macos/tests.rs` hit-tests by resize-border
  reach.
- Unit (FakeBackend), `src/app/tests/reported/`: pointer resize previews leave the saved split alone
  until mouse-up and pausing discards them (#387), a window that settles a point or two short of
  its tile after release is written back to the seam (#506), a drag over a resized temporary stack keeps its
  sizes (#388), pausing a held drag restores the previewed siblings (#389), a release keeps the target
  its preview showed across a config reload (#390), returning to an edge band's last preview point
  after leaving it while the worker was busy docks the edge column (#495), releasing a held swap
  preview after a numbered move sent the tile elsewhere keeps that move (#528), a Fullscreen Layout tile
  and a tile moved to another display without a gesture come back (#392), a Fullscreen Layout tile
  let go a point or two from its capped frame is written back exactly (#613), a drag begun in
  Fullscreen Layout stays ignored when the mode is left right after it starts (#619), a float or new window
  over a Dock strip stays on that display (#393, #394), a new window matching a `route` rule over a
  menu bar strip is routed, the topmost display's included (#498, #586), while a new floating window centered between displays is left alone (#730), a lost mouse-up sample restores the previews (#395), a width cap
  does not hide an adjacent display from directional focus and moves (#396), an emptied named
  workspace keeps its split direction across a restart (#402), absolute and keyboard resizes keep
  the 50-point floor past gaps and every known minimum (#407, #408, #409), reach an absolute width
  that fits on the first try even when a sibling first reveals its minimum during it (#492),
  give the same seams to a grow or shrink whether a sibling's minimum is known or first revealed
  by it (#607), and
  write a 1–2 point change exactly (#412), Dwindle splits the tile it shows (#410), and a retile
  learns every minimum a row reveals (#413) and writes the refit's seams exactly, even ones that
  moved only a point or two (#629). `src/app/input.rs` classifies a title-bar drag whose
  window changes size as it lands as a drop (#429).
- Unit (FakeBackend), `src/app/tests/reconciliation.rs`:
  `early_open_event_rechecks_until_the_new_window_can_split_its_sibling`,
  `closed_window_its_app_keeps_ordered_out_gives_up_its_tile` (an app closes its window by ordering it out),
  `workspace_whose_windows_all_leave_the_screen_keeps_its_tiles` (another Space is showing).
  `src/app/tests/ordered_out.rs`:
  `confirmed_last_window_close_reflows_the_sibling_in_the_same_cycle`,
  `uncertain_close_requests_one_early_retry`,
  `recheck_reflows_when_the_ordered_out_surface_disappears`,
  `a_possible_tab_replacement_or_incomplete_snapshot_keeps_its_tile`,
  `hidden_or_minimized_window_reflows_the_sibling_in_the_same_cycle`,
  `an_app_keeps_its_tiles_while_one_of_its_windows_is_still_settling`,
  `a_parked_window_keeps_its_tile_through_the_settle`.
- Unit (FakeBackend), `src/app/tests/minimums.rs`: a window that refuses its tile width gets a wider
  tile (middle, last, several at once, and observation-cycle cases); a three-window row that cannot fit stacks two neighbors, and a wider display restores the requested row; Fullscreen Layout ignores minimums. `src/app/tests/stacked_resize.rs`: a resize (issue 197), swap, or drag-nest (with or without a preview) while a pair is stacked never saves the temporary stack; a drag-nest through a fit that only reweights keeps the requested weights; a perpendicular move through a stack saves A above `B | C` (even with an adjacent display above), while an outward move that would recreate the same saved split crosses to an empty or occupied adjacent display (#373); and repeated previews held on a stacked tile keep evaluating against the gesture-start layout. `src/layout_tree.rs`:
  `fitting_minimums_reaches_into_nested_splits_and_leaves_impossible_splits_alone`.
- Window classification (`src/macos/classify.rs`), `src/macos/tests.rs`:
  `primary_windows_without_a_working_fullscreen_button_still_tile` (FaceTime, Phone, News, Podcasts,
  Games, Shortcuts, System Settings, windows showing a sheet),
  `settings_windows_modals_and_accessory_panels_still_float`, and
  `open_save_panels_and_button_bar_dialogs_float` (AppKit Open/Save panels, `AXCancelButton`, and
  buttonless `AXDefaultButton` dialogs).
- `tile` rule: `src/app/tests/rules.rs`
  `a_tile_rule_tiles_a_window_that_looks_like_a_dialog_until_toggled_to_float`; `src/config.rs`
  `tile_rule_parses_and_rejects_a_workspace`; `src/cli/tests.rs` `rules_add_tile_writes_a_tile_rule`;
  `agent/src/rules/tests.rs` `a_tile_rule_reads_back_and_the_hud_float_replaces_it_in_place`.
- Spacing, `src/spacing/tests.rs`: outer padding and its 320×240 floor; one tile, two columns and a
  nested three-tile split with inner gaps and asymmetric padding (no overlaps, exact point totals);
  equal weights give equal windows; Fullscreen Layout's capped frame; directional focus across gaps;
  a pointer resize, keyboard resize, `resize width` and balance through gaps.
  `src/app/tests/spacing.rs` covers the frames the runtime applies, `outer` with one edge at zero
  included, and a 2-point gap set during a held drag still reaching the seam after layout toggles
  made while the button was held (#614). `src/config.rs` and `src/cli/tests.rs` cover parsing,
  `outer`'s fallback and its explicit edges, range errors and `spaceman config`;
  `agent/src/rules/tests.rs` covers Settings › Layout saves that keep comments, keep `outer` and
  its explicit zeros, and restore the file.
- `src/reconcile/tests.rs` `finder_tab_replacement_keeps_its_slot_and_place_in_the_row`: a
  non-Ghostty tab change keeps the tile's slot and order.
- `src/app/tests/reported/float_native_tab.rs`: the incoming tab of a floated group keeps the
  float, also when it is listed beside the outgoing tab first, and a window that only shares a
  float's frame is adopted after the wait (#747);
  `a_window_on_the_arriving_desktop_at_a_floats_frame_does_not_take_the_float`: right after a
  desktop switch, a same-app window at a departed float's frame stays a tile (#769).
- `src/app/tests/reported/paused_tab_switch.rs`: a tab selected while Pause showed its hidden
  group takes the group's tile on Resume, whether or not Resume parked the old tab first, and
  workspace 2 then opens with all its windows; two new windows at the group's corner claim
  nothing, and a window parked before that cycle (on another Space) keeps its tile and recovery
  frame (#753).
- Fixture replay: `cargo run -- replay fixtures/ghostty-tab-replacement.json` (reconciliation of a
  native-tab replacement, without the runtime).
- Live harness (not run): `scripts/live-ghostty-tabs.sh` for native-tab slot preservation.
- Live gauntlet, `scripts/gauntlet.sh`: three windows tile the display with no overlap or gap and
  match `ctl debug`; directional focus, repeat switch and `focus next` move real macOS focus; the
  rest re-fill after a minimize or hide (500 ms budget from when the window leaves the screen);
  float and un-float re-tile.
- Observed (2026-11-24, gauntlet in a macOS 27 VM): the rest re-fill about 40 ms after the window
  leaves the screen, after a minimize (`AXWindowMiniaturized`) and after a hide (Cmd+H, through
  `NSWorkspaceDidHideApplicationNotification`; before that a hide waited for the 1 s fallback
  refresh, about 730 ms). `src/macos/events.rs`: `hiding_an_app_refreshes_on_the_next_tick`.

## Gotchas

- For about 2.7 s after macOS reports a desktop switch (the 700 ms slide settle plus the 2 s
  arrival window; 3.5 s after a trackpad swipe's fingers lift), a floating tab group's float is
  not handed to its incoming tab and the incoming tab is not held: a window that disappears then
  is taken to be on the desktop just left (#769, as in #431). A real tab switch in a floating
  group in that window loses its float, and the incoming tab is likely adopted as a tile.
- Settings › Rules shows a `tile` rule only for a built-in float, as `· tiled` on its chip under
  `Floating by default`. Tiling is the default for every other app, so the map has nowhere to
  draw its tile rule: edit those with `spaceman rules` or `config.toml`. The Workspace HUD's
  `Float`, `Pin` and `Always open` items replace an app's tile rule.
- Tests check requested frames; apps with minimum sizes may refuse them. Spaceman grows such a
  window's tile to the size it kept, taking the space from its siblings in proportion to what each
  can spare, and logs a `minimum size:` line in `agent.log`. Minimums are learned per window, only
  from refusals visible right after the write, and kept until the window closes or Spaceman
  restarts. Apps that clamp
  later (Electron aspect ratios) are still recorded as constrained instead of fought every cycle.
  A window the app moves as well as resizes right after a write is not clamping: its tile is
  written again, and only a window that keeps doing so is left where it settles (#723).
  If the requested split cannot fit, Spaceman tries grouping one adjacent pair of root children
  on the other axis. It checks both width and height and keeps window order. Automatic fits
  are temporary; the requested layout returns when it fits again. A resize or balance while a
  pair is stacked changes only the stack shown, never the saved split. If no pair solves the shortage,
  windows may overlap. A sustained overlap involving a constrained tile appears in `spaceman doctor`
  as `constrained_overlap`.
- A swap, a drag-nest, a display-edge column drop, or a perpendicular move while tiles are
  temporarily stacked edits the saved layout; the stack is then recomputed from it, so the result
  shown can differ from the stack the tile was dropped on or moved across. When a perpendicular move
  would recreate the same saved split (such as moving the first tile left or last tile right out of
  a temporarily stacked row), it crosses to the adjacent display instead. Dropping A on the top
  edge of C in `| A | B/C |`, stacked only to fit, saves `| B | A/C |`. Drag previews evaluate
  non-destructively against the layout at drag start and commit only on mouse-up, so repeated
  previews never compound into intermediate swaps.
- Some apps (many SwiftUI apps) close a window by ordering it out; the window server keeps
  listing it. When a sibling stays visible and the app had one tile on this workspace, an empty
  Accessibility window list can confirm the close immediately, unless the window server still
  lists another window of the app, held by no workspace, at the tile's top-left corner, as it
  does a native tab group's next tab (#707). The check then waits up to 300 ms for that tab to
  take the tile over; an older window the app keeps ordered out at that same corner delays a
  close the same way. The same 300 ms wait applies while the app's next tab is already on
  screen but Accessibility lists neither tab yet (#719), unless the app's focused window is
  already a new one there. An inconclusive check gets one retry after 75 ms. A surface that stays
  ordered out then follows the usual two-second settling period. Minimizing an app's only window
  (Command-M) or hiding the app (Command-H) skips that period: macOS confirms it, so the siblings
  reflow at once, and the window comes back as a new tile at the end of the layout. While nothing on the display is observed (another macOS Space
  is showing), tiles are kept, except a window macOS reports minimized or hidden, such as a
  workspace's only window. A window minimized while its workspace is hidden stays minimized when
  you switch back; it also comes back as a new tile.
- `balance` and `resize width|height` have no default hotkey; bind them in
  `[shortcuts.bindings]` or run them from the CLI.
- With gaps, a drop that lands in the gap between two tiles misses both and leaves the layout as it
  was. The Workspace HUD's layout previews stay schematic and ignore spacing.
- `settings.gaps.outer` is a config shorthand only: the runtime and `config show` get four resolved
  edges, and Settings never writes it. A file that spells out all four edges ignores it.
- A window that enters macOS native fullscreen (for example browser video) loses its slot after two
  seconds and returns as a new tile at the end of the layout
  ([known defect](fullscreen-layout.md#gotchas)).
- Floating windows (rules, manual Toggle Float, modal windows, AppKit Open/Save panels,
  cancel-button or buttonless default-button dialogs, dialogs with a close button but
  neither a fullscreen button nor an enabled zoom button, or non-resizable windows) are owned by a
  workspace (and can be moved between workspaces)
  but are never tiled or resized. A window that already holds a tile keeps it while it temporarily
  reports non-resizable (such as while showing a window-modal close or save sheet); only a `float`
  rule or Toggle Float releases a tile.
- New windows open on the active workspace of the display under the window's center unless matched by
  an explicit `route` rule or startup placement recall.
