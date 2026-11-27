//! Frame-by-frame checks of the Try it step: the real view, typed at as a window would be,
//! stepped at 120 Hz on a virtual clock and read back as presented rects. The step's beats
//! (a key coming back up, a tick) run on that same clock.

use rune::{Env, Mods, PhysicalInput, PhysicalKeyCode, Runtime, motion_gate::Run};

use super::{
    Step,
    demo::{Demo, LESSONS, Mode},
    stage,
    try_it::{Card, Play, TICK_BEAT},
};
use crate::Ui;

const HZ: f64 = 120.0;

struct Tour {
    run: Run<Ui>,
}

impl Tour {
    /// The Try it step, drawn and at rest, with the keyboard on its desktop.
    fn on_try_it() -> Self {
        let mut tour = Self::on(Step::TryIt);
        // As arriving from the arrangement does, and without a pointer over the step.
        tour.run.rt.update(|_, tx| tx.focus(stage::KEY, false));
        tour.run.step();
        tour
    }

    fn on(step: Step) -> Self {
        let mut ui = crate::playground::blank_fixture();
        super::tour_scene(&mut ui, step, true, false);
        let mut rt = Runtime::new(ui, crate::view::view, (920.0, 660.0), Env::new(true, 2.0));
        rt.poll = Some(|ui, _, tx| super::poll(ui, tx));
        let mut run = Run::new(rt, HZ);
        run.idle(0.5);
        run.step();
        Self { run }
    }

    fn click(&mut self, key: &'static str) -> usize {
        self.run.act(key, |rt| Run::tap(rt, key));
        self.run.rec.frames - 1
    }

    fn press(&mut self, key: PhysicalKeyCode, mods: Mods) -> usize {
        self.run.act("key", |rt| {
            rt.physical_input(PhysicalInput::Key {
                key,
                mods,
                pressed: true,
                repeat: false,
            });
        });
        self.run.rec.frames - 1
    }

    fn play(&mut self) -> Play {
        let mut play = Play::default();
        self.run.rt.update_if(|ui, _| {
            play = ui.tour.as_ref().unwrap().play;
            false
        });
        play
    }

    fn demo(&mut self) -> Demo {
        self.play().demo
    }

    fn step(&mut self) -> Step {
        let mut step = Step::Welcome;
        self.run.rt.update_if(|ui, _| {
            step = ui.tour.as_ref().unwrap().flow.step();
            false
        });
        step
    }

    fn track(&self, path: &str) -> &rune::motion_gate::Track {
        let suffix = format!("/{path}");
        self.run
            .rec
            .tracks
            .iter()
            .rfind(|track| track.path == path || track.path.ends_with(&suffix))
            .unwrap_or_else(|| panic!("nothing drawn at {path}"))
    }

    /// The presented rect of the element whose path ends in `path`, on each frame from `from`.
    fn rects(&self, path: &str, from: usize) -> Vec<[f32; 4]> {
        self.track(path)
            .frames
            .iter()
            .filter(|(frame, _)| *frame >= from)
            .map(|(_, snap)| snap.rect)
            .collect()
    }

    /// How opaque that element is on each frame from `from`.
    fn opacities(&self, path: &str, from: usize) -> Vec<f32> {
        self.track(path)
            .frames
            .iter()
            .filter(|(frame, _)| *frame >= from)
            .map(|(_, snap)| snap.op)
            .collect()
    }

    /// How opaque that element is now.
    fn opacity(&self, path: &str) -> f32 {
        self.track(path).frames.last().unwrap().1.op
    }

    /// Whether anything is drawn at `path` on the newest frame.
    fn shows(&self, path: &str) -> bool {
        let suffix = format!("/{path}");
        let newest = self.run.rec.frames - 1;
        self.run.rec.tracks.iter().any(|track| {
            (track.path == path || track.path.ends_with(&suffix))
                && track
                    .frames
                    .last()
                    .is_some_and(|(frame, _)| *frame == newest)
        })
    }

    fn findings(&self) -> Vec<String> {
        self.run.finish().iter().map(ToString::to_string).collect()
    }
}

fn option() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

fn option_shift() -> Mods {
    Mods {
        shift: true,
        ..option()
    }
}

/// Every step between two frames is smaller than `most`, so nothing jumped.
fn glides(xs: &[f32], most: f32) -> bool {
    xs.windows(2).all(|pair| (pair[1] - pair[0]).abs() < most)
}

#[test]
fn a_switch_slides_the_workspaces_and_the_hud_marker_stretches_across() {
    let mut tour = Tour::on_try_it();
    let strip_rest = tour.rects("strip", 0)[0];
    let small = tour.rects("hws1", 0)[0];
    // The HUD is up from the start and stays up.
    assert!(tour.opacity("hud") > 0.99);

    let from = tour.press(PhysicalKeyCode::Digit2, option());
    assert_eq!(tour.demo().workspace(), 1);
    tour.run.play(1.0);
    let least = tour.opacities("hud", from).into_iter().fold(1.0, f32::min);
    assert!(least > 0.99, "the HUD faded to {least}");
    // Workspace 2's screen grew to full size as it became the one showing.
    let grown = *tour.rects("hws1", from).last().unwrap();
    assert!(grown[2] > small[2] * 1.4, "{grown:?} from {small:?}");

    // The strip travels one screen to the left, and never more than a few points a frame.
    let strip: Vec<f32> = tour
        .rects("strip", from)
        .iter()
        .map(|rect| rect[0])
        .collect();
    let travelled = strip_rest[0] - strip[strip.len() - 1];
    assert!(travelled > 400.0, "{travelled}");
    assert!(glides(&strip, 30.0), "{strip:?}");
    let there = *tour.rects("marker", from).last().unwrap();

    // Back again: the HUD's marker returns to the first screen, wider on the way than at
    // either end.
    let from = tour.press(PhysicalKeyCode::Digit1, option());
    tour.run.play(1.0);
    let marker = tour.rects("marker", from);
    let back = marker[marker.len() - 1];
    assert!(back[0] < there[0] - 20.0, "{back:?} from {there:?}");
    assert!((back[2] - there[2]).abs() < 0.5);
    let widest = marker.iter().map(|rect| rect[2]).fold(0.0, f32::max);
    assert!(widest > there[2] + 2.0, "{widest} against {}", there[2]);
    // Once the next lesson has come up, everything has settled and the HUD is still there.
    tour.run.play(1.5);
    assert!(tour.opacity("hud") > 0.99);
    assert_eq!(tour.findings(), Vec::<String>::new());
}

#[test]
fn only_the_keyboard_makes_a_move() {
    let mut tour = Tour::on(Step::TryIt);
    // Before anything has focus, a chord goes nowhere.
    tour.press(PhysicalKeyCode::Digit2, option());
    assert_eq!(tour.demo(), Demo::default());
    // The prompt is not a button: clicking it, its keys or its progress does nothing.
    for key in ["prompt", "tray", "progress"] {
        tour.click(key);
        assert_eq!(tour.demo(), Demo::default(), "{key}");
    }

    // Clicking the display gives it the keyboard, and the chord lands.
    tour.click(stage::KEY);
    assert_eq!(tour.demo(), Demo::default());
    tour.press(PhysicalKeyCode::Digit2, option());
    assert_eq!(tour.demo().workspace(), 1);
    tour.press(PhysicalKeyCode::Digit1, option());
    assert_eq!(tour.demo().workspace(), 0);
    tour.run.play(1.0);
    // A held key's repeats and its release make no second move.
    tour.press(PhysicalKeyCode::Space, option_shift());
    assert_eq!(tour.demo().mode(0), Mode::Floated);
    tour.run.act("repeat", |rt| {
        rt.physical_input(PhysicalInput::Key {
            key: PhysicalKeyCode::Space,
            mods: option_shift(),
            pressed: true,
            repeat: true,
        });
        rt.physical_input(PhysicalInput::Key {
            key: PhysicalKeyCode::Space,
            mods: option_shift(),
            pressed: false,
            repeat: false,
        });
    });
    assert_eq!(tour.demo().mode(0), Mode::Floated);
    // Option-Shift-2 moves a window in the real thing; it is not one of the four.
    tour.press(PhysicalKeyCode::Digit2, option_shift());
    assert_eq!(tour.demo().workspace(), 0);
    assert_eq!(tour.demo().learned(), 3);
}

#[test]
fn arriving_from_the_arrangement_gives_the_stage_the_keyboard() {
    let mut tour = Tour::on(Step::Arrange);
    tour.click("tour-continue");
    tour.run.play(0.8);
    assert_eq!(tour.step(), Step::TryIt);
    // No click on the step yet, and its shortcut already lands.
    tour.press(PhysicalKeyCode::Digit2, option());
    assert_eq!(tour.demo().workspace(), 1);
}

#[test]
fn a_learned_lesson_wears_its_tick_for_a_beat_and_then_the_next_is_asked_for() {
    let mut tour = Tour::on_try_it();
    let asking = |index| Card::Lesson {
        index,
        learned: false,
    };
    assert_eq!(tour.play().card(), asking(0));
    assert!(tour.shows("lesson0") && !tour.shows("tick"));

    // The wrong shortcut still moves the desktop, and the same lesson stays up.
    tour.press(PhysicalKeyCode::Space, option_shift());
    assert_eq!(tour.demo().mode(0), Mode::Floated);
    assert_eq!(tour.play().card(), asking(0));
    tour.press(PhysicalKeyCode::Space, option_shift());
    tour.run.play(0.6);

    // The one asked for: its tick and its progress mark show while the beat lasts.
    tour.press(PhysicalKeyCode::Digit2, option());
    assert_eq!(
        tour.play().card(),
        Card::Lesson {
            index: 0,
            learned: true
        }
    );
    tour.run.play(TICK_BEAT - 0.1);
    assert!(tour.shows("tick") && tour.shows("mark0/done"));
    // Then the next lesson rises into its place, by itself, and the step stays put.
    tour.run.play(0.9);
    assert_eq!(tour.play().card(), asking(1));
    assert!(tour.shows("lesson1") && !tour.shows("lesson0") && !tour.shows("tick"));
    assert_eq!(tour.step(), Step::TryIt);
    tour.run.play(1.0);
    assert_eq!(tour.findings(), Vec::<String>::new());
}

#[test]
fn floating_lifts_the_window_off_the_grid_while_the_other_springs_wide() {
    let mut tour = Tour::on_try_it();
    let safari_rest = tour.rects("w-com.apple.Safari", 0)[0];
    let terminal_rest = tour.rects("w-com.apple.Terminal", 0)[0];

    let from = tour.press(PhysicalKeyCode::Space, option_shift());
    tour.run.play(1.0);
    let safari = tour.rects("w-com.apple.Safari", from);
    let terminal = tour.rects("w-com.apple.Terminal", from);
    let lifted = safari[safari.len() - 1];
    let wide = terminal[terminal.len() - 1];
    // Off the grid: inset from where the two tiles reached, and off the top.
    assert!(lifted[0] > safari_rest[0] + 20.0 && lifted[1] > safari_rest[1] + 20.0);
    assert!(lifted[0] + lifted[2] < terminal_rest[0] + terminal_rest[2] - 20.0);
    // The other takes both tiles' room, starting where the first one did.
    assert!((wide[0] - safari_rest[0]).abs() < 0.5, "{wide:?}");
    assert!(wide[2] > 2.0 * terminal_rest[2], "{wide:?}");
    let widths: Vec<f32> = terminal.iter().map(|rect| rect[2]).collect();
    assert!(glides(&widths, 30.0), "{widths:?}");

    // Again, and both are back on the grid.
    let from = tour.press(PhysicalKeyCode::Space, option_shift());
    tour.run.play(1.0);
    let safari = tour.rects("w-com.apple.Safari", from);
    let terminal = tour.rects("w-com.apple.Terminal", from);
    let settled = |rect: [f32; 4], rest: [f32; 4]| {
        rect.iter()
            .zip(rest)
            .all(|(now, rest)| (now - rest).abs() < 0.5)
    };
    assert!(settled(safari[safari.len() - 1], safari_rest));
    assert!(settled(terminal[terminal.len() - 1], terminal_rest));
    assert_eq!(tour.findings(), Vec::<String>::new());
}

#[test]
fn fullscreen_fills_the_screen_and_leaves_a_tab_of_the_window_behind() {
    let mut tour = Tour::on_try_it();
    let safari_rest = tour.rects("w-com.apple.Safari", 0)[0];
    let terminal_rest = tour.rects("w-com.apple.Terminal", 0)[0];

    let from = tour.press(PhysicalKeyCode::KeyF, option_shift());
    assert_eq!(tour.demo().mode(0), Mode::Fullscreen);
    tour.run.play(1.0);
    let front = *tour.rects("w-com.apple.Safari", from).last().unwrap();
    let tab = *tour.rects("w-com.apple.Terminal", from).last().unwrap();
    assert!(front[2] > 2.0 * safari_rest[2], "{front:?}");
    assert!(tab[1] < front[1] && tab[3] < 20.0, "{tab:?} over {front:?}");
    assert!(tab[0] > front[0] && tab[0] + tab[2] < front[0] + front[2]);

    let from = tour.press(PhysicalKeyCode::KeyF, option_shift());
    tour.run.play(1.0);
    let back = *tour.rects("w-com.apple.Terminal", from).last().unwrap();
    assert!((back[2] - terminal_rest[2]).abs() < 0.5 && (back[3] - terminal_rest[3]).abs() < 0.5);
    assert_eq!(tour.findings(), Vec::<String>::new());
}

#[test]
fn the_last_lesson_brings_the_seal_and_the_desktop_stays_free_to_play_with() {
    let mut tour = Tour::on_try_it();
    let presses = [
        (PhysicalKeyCode::Digit2, option()),
        (PhysicalKeyCode::Digit1, option()),
        (PhysicalKeyCode::Space, option_shift()),
        (PhysicalKeyCode::Space, option_shift()),
        (PhysicalKeyCode::KeyF, option_shift()),
        (PhysicalKeyCode::Digit1, option()),
        (PhysicalKeyCode::KeyF, option_shift()),
    ];
    for (index, (key, mods)) in presses.into_iter().enumerate() {
        assert_eq!(tour.demo().learned(), index);
        tour.press(key, mods);
        tour.run.play(1.2);
    }
    assert!(tour.demo().complete());
    assert_eq!(tour.demo().learned(), LESSONS.len());
    // The keys' place is taken by the seal, every mark is filled, and nothing moved on.
    assert_eq!(tour.play().card(), Card::Ready);
    tour.run.play(1.0);
    assert!(tour.shows("seal/disc/check") && !tour.shows("tray"));
    assert!((0..LESSONS.len()).all(|index| tour.shows(&format!("mark{index}/done"))));
    assert_eq!(tour.step(), Step::TryIt);

    // Every shortcut still plays, and the prompt stays as it is.
    tour.press(PhysicalKeyCode::Digit2, option());
    assert_eq!(tour.demo().workspace(), 1);
    tour.press(PhysicalKeyCode::Space, option_shift());
    assert_eq!(tour.demo().mode(1), Mode::Floated);
    tour.run.play(2.2);
    assert_eq!(tour.play().card(), Card::Ready);
    assert_eq!(tour.step(), Step::TryIt);
    assert_eq!(tour.findings(), Vec::<String>::new());
}

#[test]
fn another_base_modifier_changes_the_keys_asked_for_on_the_spot() {
    let mut tour = Tour::on_try_it();
    let control_option = Mods {
        ctrl: true,
        ..option()
    };
    tour.run.act("modifier", |rt| {
        rt.update_if(|ui, _| {
            super::set_modifier(ui, crate::panes::shortcuts::PRESETS[1].1);
            true
        });
    });
    tour.run.play(0.6);
    // The old chord is no longer a move; the new one is, and it is the lesson asked for.
    tour.press(PhysicalKeyCode::Digit2, option());
    assert_eq!(tour.demo(), Demo::default());
    tour.press(PhysicalKeyCode::Digit2, control_option);
    assert_eq!(tour.demo().workspace(), 1);
    assert_eq!(tour.demo().learned(), 1);
}

#[test]
fn the_prompt_and_its_keys_stay_on_screen_from_one_lesson_to_the_next() {
    let mut tour = Tour::on_try_it();
    let option_at = tour.rects("keycap-option", 0)[0];
    let action_at = tour.rects("keycap-action", 0)[0];
    let tray_at = tour.rects("tray", 0)[0];
    // Option-2, Option-1, then Option-Shift-Space: Shift joins between Option and the last
    // key, whose legend rolls from 1 to Space.
    let from = tour.press(PhysicalKeyCode::Digit2, option());
    tour.run.play(1.2);
    tour.press(PhysicalKeyCode::Digit1, option());
    tour.run.play(1.2);
    assert_eq!(
        tour.play().card(),
        Card::Lesson {
            index: 2,
            learned: false
        }
    );
    for path in [
        "prompt",
        "title",
        "tray",
        "keycap-option",
        "keycap-action",
        "progress",
    ] {
        let least = tour.opacities(path, from).into_iter().fold(1.0, f32::min);
        assert!(least > 0.99, "{path} faded to {least}");
    }
    assert!(tour.shows("lesson2") && !tour.shows("lesson1") && !tour.shows("lesson0"));
    assert!(tour.shows("keycap-shift") && tour.shows("legend-Space"));
    assert!(!tour.shows("legend-1") && !tour.shows("legend-2"));

    // The tray's right edge never moved, flush with the card's inset, while it widened to the
    // left; Option slid along to make room for Shift, and the last key grew for Space.
    let right = tray_at[0] + tray_at[2];
    let tray = tour.rects("tray", from);
    assert!(
        tray.iter()
            .all(|rect| (rect[0] + rect[2] - right).abs() < 0.5),
        "{tray:?}"
    );
    let prompt = tour.rects("prompt", 0)[0];
    let inset = prompt[0] + prompt[2] - right;
    assert!((inset - super::prompt::INSET).abs() < 0.5, "{inset}");
    let option = tour.rects("keycap-option", from);
    let xs: Vec<f32> = option.iter().map(|rect| rect[0]).collect();
    assert!(xs[xs.len() - 1] < option_at[0] - 10.0, "{xs:?}");
    assert!(glides(&xs, 6.0), "{xs:?}");
    let action = tour.rects("keycap-action", from);
    let last = action[action.len() - 1];
    assert!(last[2] > action_at[2] + 10.0, "{last:?}");
    assert!(
        (last[0] + last[2] - action_at[0] - action_at[2]).abs() < 0.5,
        "{last:?}"
    );
    // And the two marks filled.
    assert!(tour.shows("mark0/done") && tour.shows("mark1/done") && !tour.shows("mark2/done"));
    // The next lesson came up at the end of the tick's beat; let it settle.
    tour.run.play(1.0);
    assert_eq!(tour.findings(), Vec::<String>::new());
}

#[test]
fn workspace_ones_number_again_brings_the_window_behind_forward() {
    let mut tour = Tour::on_try_it();
    for (key, mods) in [
        (PhysicalKeyCode::Digit2, option()),
        (PhysicalKeyCode::Digit1, option()),
        (PhysicalKeyCode::Space, option_shift()),
        (PhysicalKeyCode::Space, option_shift()),
        (PhysicalKeyCode::KeyF, option_shift()),
    ] {
        tour.press(key, mods);
        tour.run.play(1.2);
    }
    let front = *tour.rects("w-com.apple.Safari", 0).last().unwrap();
    let tab = *tour.rects("w-com.apple.Terminal", 0).last().unwrap();
    assert_eq!(
        tour.play().card(),
        Card::Lesson {
            index: 5,
            learned: false
        }
    );

    let from = tour.press(PhysicalKeyCode::Digit1, option());
    assert_eq!(tour.demo().desk(0).focus, 1);
    assert_eq!(tour.demo().learned(), 6);
    tour.run.play(1.0);
    // The two trade places: Terminal fills the screen, and Safari is the tab behind it.
    let terminal = tour.rects("w-com.apple.Terminal", from);
    let safari = tour.rects("w-com.apple.Safari", from);
    let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.5);
    assert!(near(terminal[terminal.len() - 1], front), "{terminal:?}");
    assert!(near(safari[safari.len() - 1], tab), "{safari:?}");
    let heights: Vec<f32> = terminal.iter().map(|rect| rect[3]).collect();
    assert!(glides(&heights, 30.0), "{heights:?}");
    // The menu bar names the app in front, and the HUD has it in front too: its icon large,
    // Safari's small beside it.
    assert!(tour.shows("menus-com.apple.Terminal") && !tour.shows("menus-com.apple.Safari"));
    let front_icon = *tour
        .rects("hws0/i-com.apple.Terminal", from)
        .last()
        .unwrap();
    let behind_icon = *tour.rects("hws0/i-com.apple.Safari", from).last().unwrap();
    assert!(
        front_icon[2] > behind_icon[2] * 1.4,
        "{front_icon:?} {behind_icon:?}"
    );
    assert!(front_icon[0] < behind_icon[0]);
    assert!(tour.opacity("hws0/i-com.apple.Safari") < 0.6);
    tour.run.play(1.0);
    assert_eq!(tour.findings(), Vec::<String>::new());
}
