//! `KURRENCY_DEMO=1` (debug builds only): plays a scripted tour of the app
//! for the README's animated demo (`scripts/record-demo.sh` records it).
//!
//! Every step goes through the app's real input paths: keystrokes are
//! dispatched to the window as if typed (type-anywhere, the amount input,
//! Ctrl+K and the palette, Enter, Esc), the pointer is a synthetic mouse that
//! hovers and clicks rows and the history chart, and the paste is smart
//! paste's own parser fed a fixed text instead of the clipboard. So every
//! animation plays as it would for a person.
//!
//! The tour ends where it began (the same rows, amount and theme, nothing
//! hovered or focused), so a recording of it loops without a seam. The
//! settings page covers the list while the starting state is put back.
//!
//! Compiled into debug builds only (`#[cfg(debug_assertions)]` on the
//! module): the demo, like the fixture it is recorded with
//! (`KURRENCY_CACHE_DIR`, `KURRENCY_NOW`), is a debug-build tool, and
//! release binaries carry none of it.

use std::time::{Duration, Instant};

use gpui_kit::{
    App, AsyncWindowContext, Context, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PlatformInput, Point, WeakEntity, Window, point, px,
};
use kurrency_core::settings::ThemeMode;
use rust_decimal::Decimal;

use super::KurrencyApp;
use super::actions::shortcut;
use crate::layout::metrics;

/// The pause between typed characters.
const TYPE_MS: u64 = 90;
/// How often the pointer moves while it glides.
const GLIDE_STEP_MS: u64 = 16;

/// Where the synthetic pointer goes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Target {
    /// The middle of list row `n` (1-based; 0 is the anchor).
    Row(usize),
    /// A point across the history chart, `0.` at its left edge and `1.` at
    /// its right.
    Chart(f32),
    /// Off the window, so nothing is hovered.
    Away,
}

/// One step of the tour.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Step {
    Wait(u64),
    /// A keystroke, `ctrl-k` style (`Ctrl` becomes `Cmd` on macOS).
    Key(&'static str),
    /// Types the text one character at a time.
    Type(&'static str),
    /// Moves the pointer there at once.
    Point(Target),
    /// Moves the pointer there smoothly over the milliseconds.
    Glide(Target, u64),
    /// A glide's pointer at `t` (0 to 1) of the way: 0 starts it from
    /// where the pointer is.
    Toward(Target, f32),
    /// Presses and releases the left button where the pointer is.
    Click,
    /// Pastes the text as a price (smart paste).
    Paste(&'static str),
    /// Opens the history page of a listed currency.
    History(&'static str),
    /// Settings → Theme: System, Light or Dark.
    Theme(ThemeMode),
    /// Puts back the rows, amount and theme the tour started with.
    Restore,
}

use Step::*;
use Target::*;

/// The tour, about 15s with its pauses. The recording keeps ~0.6s of the
/// first and last pauses, which show the same frame, for a seamless loop.
pub(crate) const TOUR: &[Step] = &[
    Wait(900),
    // Maths in the amount: 1.5k*3 = 4,500, every row rolling to it.
    Type("1.5k*3"),
    Wait(650),
    Key("enter"),
    Wait(750),
    // Hover the first row (its inverse rate), then click it into the card.
    Point(Row(1)),
    Wait(700),
    Click,
    Wait(350),
    Point(Away),
    Wait(900),
    // A price pasted from anywhere: ¥1,200 makes JPY the anchor.
    Paste("¥1,200"),
    Wait(1100),
    // The palette's natural commands.
    Key("ctrl-k"),
    Wait(350),
    Type("50 gbp"),
    Wait(800),
    Key("enter"),
    Wait(900),
    // The pound's history, scrubbed with the pointer.
    History("GBP"),
    Wait(650),
    Point(Chart(0.25)),
    Glide(Chart(0.8), 1200),
    Wait(700),
    Point(Away),
    Key("escape"),
    Wait(650),
    // Settings: Paper to Graphite and back. The page hides the list while
    // it is put back as it was at the start.
    Key("ctrl-,"),
    Wait(300),
    Restore,
    Wait(500),
    Theme(ThemeMode::Dark),
    Wait(1300),
    Theme(ThemeMode::Light),
    Wait(700),
    Key("escape"),
    Wait(1200),
];

/// What [`Step::Restore`] puts back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Start {
    pub codes: Vec<String>,
    pub amount: Option<Decimal>,
    pub theme_mode: ThemeMode,
}

/// The tour's progress.
pub(crate) struct Demo {
    start: Start,
    pointer: Point<f32>,
    /// Where the current glide began.
    glide_from: Point<f32>,
}

/// Input for the window, dispatched outside the view's update so that its
/// handlers can update the view.
pub(crate) enum Input {
    Key(Keystroke),
    Mouse(Vec<PlatformInput>),
}

impl Input {
    pub(crate) fn dispatch(self, window: &mut Window, cx: &mut App) {
        match self {
            Self::Key(keystroke) => {
                window.dispatch_keystroke(keystroke, cx);
            }
            Self::Mouse(events) => {
                for event in events {
                    window.dispatch_event(event, cx);
                }
            }
        }
    }

    /// The pointer moving to `at` (logical px).
    pub(crate) fn move_to(at: Point<f32>) -> Self {
        Self::Mouse(vec![PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(at.x), px(at.y)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        })])
    }
}

impl KurrencyApp {
    /// Starts the tour after `delay` (time to float the window and start
    /// recording). Prints `kurrency-demo: start` and `kurrency-demo: end` to
    /// stderr, for the recording script.
    pub(super) fn start_demo(
        &mut self,
        delay: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let started = this.update(cx, |this, _| this.begin_demo());
            if started.is_err() {
                return;
            }
            eprintln!("kurrency-demo: start");
            for step in TOUR {
                // A glide follows the clock, however long each move takes.
                if let Glide(target, ms) = *step {
                    let started = Instant::now();
                    let total = Duration::from_millis(ms).as_secs_f32();
                    let mut t = 0.;
                    loop {
                        if !run(&this, cx, Toward(target, t)) {
                            return;
                        }
                        if t >= 1. {
                            break;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(GLIDE_STEP_MS))
                            .await;
                        t = (started.elapsed().as_secs_f32() / total).min(1.);
                    }
                    continue;
                }
                for step in expand(*step) {
                    match step {
                        Wait(ms) => {
                            cx.background_executor()
                                .timer(Duration::from_millis(ms))
                                .await;
                        }
                        step => {
                            if !run(&this, cx, step) {
                                return;
                            }
                        }
                    }
                }
            }
            eprintln!("kurrency-demo: end");
            let _ = this.update(cx, |this, _| this.demo = None);
        })
        .detach();
    }

    /// Remembers the state the tour returns to; the pointer starts off
    /// the window.
    pub(crate) fn begin_demo(&mut self) {
        self.demo = Some(Demo {
            start: self.demo_start(),
            pointer: AWAY,
            glide_from: AWAY,
        });
    }

    /// The state the tour returns to.
    pub(crate) fn demo_start(&self) -> Start {
        Start {
            codes: self.rows.iter().map(|row| row.code.clone()).collect(),
            amount: self.amount,
            theme_mode: self.prefs.theme_mode,
        }
    }

    /// Runs one step, or returns the input it sends to the window.
    /// [`Step::Wait`], [`Step::Type`] and [`Step::Glide`] do nothing here:
    /// [`expand`] (or the tour's clock, for a glide) turns them into single
    /// steps.
    pub(crate) fn demo_step(
        &mut self,
        step: Step,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Input> {
        let input = match step {
            Wait(_) | Type(_) | Glide(..) => None,
            Key(key) => {
                let keystroke = match key.chars().count() {
                    1 => Keystroke {
                        key: key.to_string(),
                        key_char: Some(key.to_string()),
                        modifiers: Modifiers::default(),
                    },
                    _ => match Keystroke::parse(shortcut_of(key)) {
                        Ok(keystroke) => keystroke,
                        Err(err) => {
                            log::warn!("demo: bad keystroke {key}: {err:?}");
                            return None;
                        }
                    },
                };
                Some(Input::Key(keystroke))
            }
            Point(target) => {
                let at = self.demo_point(target, window);
                Some(self.demo_move(at))
            }
            Toward(target, t) => {
                let to = self.demo_point(target, window);
                let demo = self.demo.as_mut()?;
                if t == 0. {
                    demo.glide_from = demo.pointer;
                }
                let from = demo.glide_from;
                let t = ease_in_out(t);
                let at = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
                Some(self.demo_move(at))
            }
            Click => {
                let at = self.demo.as_ref().map_or(AWAY, |demo| demo.pointer);
                let position = point(px(at.x), px(at.y));
                let down = MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                };
                let up = MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                };
                Some(Input::Mouse(vec![
                    PlatformInput::MouseDown(down),
                    PlatformInput::MouseUp(up),
                ]))
            }
            Paste(text) => {
                self.paste_text(text, false, window, cx);
                None
            }
            History(code) => {
                self.open_detail(code, window, cx);
                None
            }
            Theme(mode) => {
                self.update_prefs(window, cx, |s| s.theme_mode = mode);
                None
            }
            Restore => {
                self.demo_restore(window, cx);
                None
            }
        };
        cx.notify();
        input
    }

    /// Puts the rows, amount and theme back as they were when the tour
    /// started, without animating (the settings page covers the list).
    fn demo_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = self.demo.as_ref().map(|demo| demo.start.clone()) else {
            return;
        };
        let mut rows = std::mem::take(&mut self.rows);
        for code in &start.codes {
            if let Some(ix) = rows.iter().position(|row| &row.code == code) {
                self.rows.push(rows.remove(ix));
            }
        }
        self.rows.append(&mut rows);
        self.amount = start.amount;
        self.echo = None;
        self.hovered = None;
        self.motion = Default::default();
        self.save_settings();
        self.update_prefs(window, cx, |s| s.theme_mode = start.theme_mode);
    }

    /// Moves the pointer to `at`, remembering it for the next click.
    fn demo_move(&mut self, at: Point<f32>) -> Input {
        if let Some(demo) = &mut self.demo {
            demo.pointer = at;
        }
        Input::move_to(at)
    }

    /// The window point of a target, in logical px.
    pub(crate) fn demo_point(&self, target: Target, window: &Window) -> Point<f32> {
        use metrics::*;
        let width = f32::from(window.viewport_size().width);
        match target {
            Row(n) => {
                let list_top = HEADER + 2. * RING + CARD + CHIPS_GAP + CHIPS + LIST_GAP;
                let n = n.max(1) as f32;
                point(width / 2., list_top + (n - 1.) * ROW + ROW / 2.)
            }
            Chart(x) => {
                // The history page under the header: its own 48px header,
                // 16px of padding, the hero (84px) and a 16px gap. At the
                // window's natural height the chart shrinks to about 55px.
                let top = HEADER + 48. + 16. + 84. + 16.;
                let (left, right) = (PAD_X, width - PAD_X);
                point(left + (right - left) * x, top + 26.)
            }
            Away => AWAY,
        }
    }
}

/// Runs one step of the tour; false once the window is gone.
fn run(this: &WeakEntity<KurrencyApp>, cx: &mut AsyncWindowContext, step: Step) -> bool {
    let Ok(input) = this.update_in(cx, |this, window, cx| this.demo_step(step, window, cx)) else {
        return false;
    };
    match input {
        Some(input) => cx.update(|window, cx| input.dispatch(window, cx)).is_ok(),
        None => true,
    }
}

/// Off the window.
const AWAY: Point<f32> = Point { x: -100., y: -100. };

/// Slow at both ends, like a hand moving a mouse.
fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// `chart:0.6`, `row:2` or `away` (`KURRENCY_POINTER`).
pub(crate) fn parse_target(text: &str) -> Option<Target> {
    let (kind, arg) = text.split_once(':').unwrap_or((text, ""));
    match kind {
        "chart" => arg.parse().ok().map(Chart),
        "row" => arg.parse().ok().map(Row),
        "away" => Some(Away),
        _ => None,
    }
}

/// `ctrl-k`, `ctrl-,` with Cmd on macOS; other keys as they are.
fn shortcut_of(key: &'static str) -> &'static str {
    match key.strip_prefix("ctrl-") {
        Some(rest) => shortcut(rest),
        None => key,
    }
}

/// Splits typing and gliding into single steps with their pauses. (The
/// live tour paces a glide by the clock instead, see `start_demo`.)
pub(crate) fn expand(step: Step) -> Vec<Step> {
    match step {
        Type(text) => text
            .char_indices()
            .flat_map(|(ix, ch)| {
                let key = &text[ix..ix + ch.len_utf8()];
                let key = if key == " " { "space" } else { key };
                [Key(key), Wait(TYPE_MS)]
            })
            .collect(),
        Glide(target, ms) => {
            let n = (ms / GLIDE_STEP_MS).max(1);
            (0..=n)
                .flat_map(|k| [Toward(target, k as f32 / n as f32), Wait(GLIDE_STEP_MS)])
                .collect()
        }
        step => vec![step],
    }
}
