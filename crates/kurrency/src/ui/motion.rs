//! The motion pass (DESIGN §4): subtle, under 250ms and interruptible.
//!
//! - digit roll: when an amount changes, only the changed digits roll: the
//!   old one slides out and fades while the new one slides in, upwards when
//!   the value rose and downwards when it fell;
//! - anchor swap: the promoted currency slides up into the card while the
//!   old anchor drops into its row;
//! - add / remove: the row's height and opacity grow or collapse;
//! - copy: a check mark flashes where the row's change chip sits;
//! - reorder: while a row is dragged the others spring apart around its
//!   drop slot, and a moved row springs into its new place.
//!
//! The digit roll is timed here, from the clock, rather than by GPUI's
//! `with_animation`, which keeps an animation's progress in per-element
//! state. That state is dropped whenever the element's id path changes (a
//! row moving to another index, a swap or settle wrapper coming and going)
//! or it is not drawn for a frame, and the roll would then play again. A
//! roll interrupted by the next change carries on from where its digits
//! are, so typing and ↑/↓ key repeat flow instead of restarting.
//!
//! Every other animation goes through `with_animation`, which draws the end
//! state at once when [`App::reduce_motion`](gpui_kit::App::reduce_motion) is
//! set; the transient states (ghost rows, the closing history page) are not
//! created at all then, and digits do not roll.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, ElementId, IntoElement, ParentElement as _,
    Styled as _, div, ease_out_quint, px,
};
use rust_decimal::Decimal;

/// Digit roll.
pub(crate) const ROLL: Duration = Duration::from_millis(240);
/// The copy check mark fading in.
pub(crate) const FADE: Duration = Duration::from_millis(180);
/// Anchor swap.
pub(crate) const SWAP: Duration = Duration::from_millis(220);
/// Row add and remove.
pub(crate) const RESIZE: Duration = Duration::from_millis(200);
/// How long the copy check mark stays.
pub(crate) const COPIED: Duration = Duration::from_millis(1200);
/// Rows moving apart while a row is dragged, and a moved row settling.
pub(crate) const SPRING: Duration = Duration::from_millis(240);

/// How far a rolling digit travels, as a share of the text size.
const ROLL_TRAVEL: f32 = 0.45;
/// The most digits rolling out of one place at once (a quick run of
/// changes); the oldest, nearly gone by then, are dropped.
const MAX_EXITS: usize = 3;

/// A lightly underdamped spring (damping ratio 0.7, about 4% overshoot),
/// settled by `t = 1`.
pub(crate) fn spring(t: f32) -> f32 {
    if t >= 1. {
        return 1.;
    }
    const ZETA: f32 = 0.7;
    const OMEGA: f32 = 7.5;
    let damped = OMEGA * (1. - ZETA * ZETA).sqrt();
    let decay = (-ZETA * OMEGA * t).exp();
    1. - decay * ((damped * t).cos() + ZETA * OMEGA / damped * (damped * t).sin())
}

/// The roll's easing, a cubic ease-out: brisk at first, but without
/// covering most of the way in the first frame as a quintic one does.
fn roll_ease(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// How far through a roll that started at `start` the clock is, 0 to 1.
fn roll_progress(start: Instant, now: Instant) -> f32 {
    (now.saturating_duration_since(start).as_secs_f32() / ROLL.as_secs_f32()).min(1.)
}

/// How a digit is drawn: its vertical offset in units of the travel
/// (positive is down) and its opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pose {
    pub y: f32,
    pub opacity: f32,
}

impl Pose {
    const REST: Self = Self { y: 0., opacity: 1. };
}

/// A place of an amount (or the whole amount) rolling in: from one travel
/// below when the value rose, from above when it fell.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RollIn {
    start: Instant,
    /// 1 when the value rose (digits move up), -1 when it fell.
    dir: f32,
}

impl RollIn {
    fn pose(&self, now: Instant) -> Pose {
        let e = roll_ease(roll_progress(self.start, now));
        Pose {
            y: self.dir * (1. - e),
            opacity: e,
        }
    }

    fn done(&self, now: Instant) -> bool {
        roll_progress(self.start, now) >= 1.
    }
}

/// The amount as it was drawn when a change came, rolling out from there.
/// It is drawn over the new amount and laid out the way the old one was,
/// so nothing jumps sideways when the number grows or shrinks.
#[derive(Debug, Clone, PartialEq)]
struct Exit {
    /// Each character with the pose it had; `None` where it did not change,
    /// so the new amount draws it, and here it only takes up its width.
    chars: Vec<(char, Option<Pose>)>,
    /// How many of `chars` are the integer part.
    int_len: usize,
    roll: RollIn,
}

impl Exit {
    /// Where a character that was at `from` is now: moving one travel the
    /// way the value went and fading out, faster than the new one fades in
    /// so the two do not blur together.
    fn pose(&self, from: Pose, now: Instant) -> Pose {
        let e = roll_ease(roll_progress(self.roll.start, now));
        Pose {
            y: from.y + (-self.roll.dir - from.y) * e,
            opacity: from.opacity * (1. - e) * (1. - e),
        }
    }
}

/// What one amount display shows this frame.
#[derive(Debug, Clone)]
pub(crate) struct Shown {
    /// `anchor` or the row's code.
    pub key: String,
    /// The currency shown: when it changes, the display starts afresh.
    pub code: String,
    /// The characters after the symbol (`1,250.00`).
    pub text: String,
    /// How many of them are the integer part (`1,250`).
    pub int_len: usize,
    pub value: Decimal,
}

/// What one amount display shows, and what of it is rolling.
#[derive(Debug, Clone)]
pub(crate) struct Roll {
    code: String,
    text: Vec<char>,
    int_len: usize,
    value: Decimal,
    /// Places rolling in on their own, counted from the right.
    places: HashMap<usize, RollIn>,
    /// The whole amount rolling in, after a change of its length.
    whole: Option<RollIn>,
    /// Earlier amounts rolling out, oldest first.
    exits: Vec<Exit>,
    /// The frame being drawn.
    now: Instant,
}

impl Roll {
    fn new(shown: Shown, now: Instant) -> Self {
        Self {
            code: shown.code,
            text: shown.text.chars().collect(),
            int_len: shown.int_len,
            value: shown.value,
            places: HashMap::new(),
            whole: None,
            exits: Vec::new(),
            now,
        }
    }

    /// Moves to what the display shows now. When the length is the same,
    /// only the characters that changed roll; otherwise the whole amount
    /// does. Characters still rolling in roll out from where they are.
    fn update(&mut self, shown: Shown, now: Instant, animate: bool) {
        self.now = now;
        self.places.retain(|_, roll| !roll.done(now));
        self.whole = self.whole.filter(|roll| !roll.done(now));
        self.exits.retain(|exit| !exit.roll.done(now));
        let text: Vec<char> = shown.text.chars().collect();
        // Another currency, or the same value written another way (number
        // format), is not a change to roll.
        if !animate || shown.code != self.code || (text != self.text && shown.value == self.value) {
            self.places.clear();
            self.whole = None;
            self.exits.clear();
        } else if text != self.text {
            let roll = RollIn {
                start: now,
                dir: if shown.value > self.value { 1. } else { -1. },
            };
            let same_length = text.len() == self.text.len();
            let chars = (0..self.text.len())
                .map(|ix| {
                    let changed = !same_length || text[ix] != self.text[ix];
                    (self.text[ix], changed.then(|| self.pose(ix)))
                })
                .collect();
            self.exits.push(Exit {
                chars,
                int_len: self.int_len,
                roll,
            });
            let extra = self.exits.len().saturating_sub(MAX_EXITS);
            self.exits.drain(..extra);
            if same_length {
                for (ix, (&new, &old)) in text.iter().zip(&self.text).enumerate() {
                    if new != old {
                        self.places.insert(text.len() - 1 - ix, roll);
                    }
                }
            } else {
                self.places.clear();
                self.whole = Some(roll);
            }
        }
        self.code = shown.code;
        self.text = text;
        self.int_len = shown.int_len;
        self.value = shown.value;
    }

    /// Whether anything is still rolling.
    pub(crate) fn rolling(&self) -> bool {
        !self.places.is_empty() || self.whole.is_some() || !self.exits.is_empty()
    }

    /// How character `ix` of the amount (from the left) is drawn this
    /// frame.
    pub(crate) fn pose(&self, ix: usize) -> Pose {
        let k = self.text.len().saturating_sub(ix + 1);
        self.places
            .get(&k)
            .copied()
            .or(self.whole)
            .map_or(Pose::REST, |roll| roll.pose(self.now))
    }

    /// The earlier amounts rolling out, oldest first: for each, its
    /// characters (`None` where hidden) and how many are the integer part.
    pub(crate) fn exits(&self) -> impl Iterator<Item = (Vec<(char, Option<Pose>)>, usize)> {
        self.exits.iter().map(|exit| {
            let chars = exit
                .chars
                .iter()
                .map(|&(ch, from)| (ch, from.map(|from| exit.pose(from, self.now))))
                .collect();
            (chars, exit.int_len)
        })
    }
}

/// A row collapsing after its removal.
#[derive(Debug, Clone)]
pub(crate) struct Leaving {
    pub code: String,
    /// Its position among the list rows (1-based, as in `rows`).
    pub ix: usize,
    pub version: usize,
}

/// The promoted and demoted currencies of the last anchor swap.
#[derive(Debug, Clone)]
pub(crate) struct Swap {
    pub promoted: String,
    pub demoted: String,
    pub version: usize,
}

#[derive(Default)]
pub(crate) struct Motion {
    next: usize,
    /// Per amount display: `anchor` or a row's code.
    rolls: HashMap<String, Roll>,
    /// Rows being added, by code.
    pub entering: HashMap<String, usize>,
    pub leaving: Vec<Leaving>,
    pub swap: Option<Swap>,
    /// The row whose value was just copied.
    pub copied: Option<(String, usize)>,
    /// Rows sliding into their new place after a reorder: each code with
    /// the vertical offset (px) it starts from.
    pub settle: Option<(Vec<(String, f32)>, usize)>,
}

impl Motion {
    /// A fresh version for a new animation (never 0).
    pub(crate) fn bump(&mut self) -> usize {
        self.next += 1;
        self.next
    }

    /// Records what each display shows at `now`, starting rolls where the
    /// value changed (unless `animate` is off). Displays that are gone
    /// forget their state, so they do not roll when they come back. Returns
    /// whether a roll is still playing, which needs another frame.
    pub(crate) fn update_rolls(
        &mut self,
        shown: impl IntoIterator<Item = Shown>,
        now: Instant,
        animate: bool,
    ) -> bool {
        let mut rolls = HashMap::new();
        for shown in shown {
            let key = shown.key.clone();
            let roll = match self.rolls.remove(&key) {
                Some(mut roll) => {
                    roll.update(shown, now, animate);
                    roll
                }
                None => Roll::new(shown, now),
            };
            rolls.insert(key, roll);
        }
        self.rolls = rolls;
        self.rolls.values().any(Roll::rolling)
    }

    pub(crate) fn roll(&self, key: &str) -> Option<&Roll> {
        self.rolls.get(key)
    }

    /// Drops one-shot states whose animation has finished, so a row that
    /// moves later does not replay it.
    pub(crate) fn expire(&mut self, version: usize) {
        self.entering.retain(|_, v| *v != version);
        self.leaving.retain(|l| l.version != version);
        if self.swap.as_ref().is_some_and(|s| s.version == version) {
            self.swap = None;
        }
        if self.copied.as_ref().is_some_and(|(_, v)| *v == version) {
            self.copied = None;
        }
        if self.settle.as_ref().is_some_and(|(_, v)| *v == version) {
            self.settle = None;
        }
    }

    /// The offset a row settles from after a reorder, with the animation's
    /// version.
    pub(crate) fn settling(&self, code: &str) -> Option<(f32, usize)> {
        let (rows, version) = self.settle.as_ref()?;
        rows.iter()
            .find(|(c, _)| c == code)
            .map(|(_, from)| (*from, *version))
    }
}

/// One character of an amount drawn `size` px high, where it is this
/// frame.
fn glyph(ch: char, pose: Pose, size: f32) -> gpui_kit::Div {
    let el = div().child(ch.to_string());
    if pose == Pose::REST {
        return el;
    }
    el.relative()
        .top(px(pose.y * size * ROLL_TRAVEL))
        .opacity(pose.opacity)
}

/// Character `ix` of an amount drawn `size` px high, rolling in if it is.
pub(crate) fn roll_char(ch: char, ix: usize, roll: Option<&Roll>, size: f32) -> AnyElement {
    let pose = roll.map_or(Pose::REST, |roll| roll.pose(ix));
    glyph(ch, pose, size).into_any_element()
}

/// The earlier amounts rolling out, each laid over the amount's digits the
/// way it was drawn: its integer part, then its fraction at `decimals`
/// opacity. Goes in a relative container that starts where the digits do.
pub(crate) fn roll_exits(roll: Option<&Roll>, size: f32, decimals: f32) -> Vec<AnyElement> {
    let Some(roll) = roll else {
        return Vec::new();
    };
    roll.exits()
        .map(|(chars, int_len)| {
            let mut glyphs = chars.into_iter().map(|(ch, pose)| match pose {
                Some(pose) => glyph(ch, pose, size),
                // Unchanged: only its width, so the rest line up.
                None => div().invisible().child(ch.to_string()),
            });
            let integer: Vec<_> = glyphs.by_ref().take(int_len).collect();
            let fraction: Vec<_> = glyphs.collect();
            h_flex()
                .absolute()
                .top_0()
                .left_0()
                .child(h_flex().children(integer))
                .when(!fraction.is_empty(), |el| {
                    el.child(h_flex().opacity(decimals).children(fraction))
                })
                .into_any_element()
        })
        .collect()
}

/// Grows a new row from nothing, or collapses a removed one.
pub(crate) fn resize(
    el: impl IntoElement,
    id: impl Into<ElementId>,
    height: f32,
    growing: bool,
) -> AnyElement {
    div()
        .overflow_hidden()
        .child(el)
        .with_animation(
            id,
            Animation::new(RESIZE).with_easing(ease_out_quint()),
            move |el, t| {
                let t = if growing { t } else { 1. - t };
                el.h(px(height * t)).opacity(t)
            },
        )
        .into_any_element()
}

/// Slides an element `from` px vertically into place while it fades in
/// (the anchor swap).
pub(crate) fn slide_in(el: impl IntoElement, id: impl Into<ElementId>, from: f32) -> AnyElement {
    div()
        .relative()
        .child(el)
        .with_animation(
            id,
            Animation::new(SWAP).with_easing(ease_out_quint()),
            move |el, t| el.top(px(from * (1. - t))).opacity(t),
        )
        .into_any_element()
}

/// Springs an element `from` px vertically into place (a reordered row).
pub(crate) fn settle(el: impl IntoElement, id: impl Into<ElementId>, from: f32) -> AnyElement {
    div()
        .relative()
        .child(el)
        .with_animation(
            id,
            Animation::new(SPRING).with_easing(spring),
            move |el, t| el.top(px(from * (1. - t))),
        )
        .into_any_element()
}

/// The gap a dragged row would drop into, springing open (or shut, where
/// the row was a moment ago) so the rows around it move apart.
pub(crate) fn gap(
    el: impl IntoElement,
    id: impl Into<ElementId>,
    height: f32,
    opening: bool,
) -> AnyElement {
    div()
        .overflow_hidden()
        .child(el)
        .with_animation(
            id,
            Animation::new(SPRING).with_easing(spring),
            move |el, t| {
                let t = if opening { t } else { 1. - t };
                el.h(px((height * t).max(0.)))
            },
        )
        .into_any_element()
}

/// Fades an element in quickly (the copy check mark).
pub(crate) fn fade_in(el: impl IntoElement, id: impl Into<ElementId>) -> AnyElement {
    div()
        .child(el)
        .with_animation(
            id,
            Animation::new(FADE).with_easing(ease_out_quint()),
            gpui_kit::Styled::opacity,
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;

    const MS: Duration = Duration::from_millis(1);
    /// Where a digit coming up into place starts.
    const BELOW: Pose = Pose { y: 1., opacity: 0. };

    fn shown(key: &str, code: &str, text: &str) -> Shown {
        Shown {
            key: key.into(),
            code: code.into(),
            text: text.into(),
            int_len: text.find('.').unwrap_or(text.len()),
            value: Decimal::from_str(&text.replace(',', "")).unwrap(),
        }
    }

    /// Shows `text` for EUR at `now`; whether anything is rolling.
    fn show(m: &mut Motion, text: &str, now: Instant) -> bool {
        m.update_rolls([shown("EUR", "EUR", text)], now, true)
    }

    fn eur(m: &Motion) -> &Roll {
        m.roll("EUR").unwrap()
    }

    /// The characters of EUR's amount rolling in.
    fn rolling_in(m: &Motion) -> Vec<usize> {
        let roll = eur(m);
        (0..roll.text.len())
            .filter(|&ix| roll.pose(ix) != Pose::REST)
            .collect()
    }

    /// The visible characters of each amount rolling out, with their poses.
    fn rolling_out(m: &Motion) -> Vec<Vec<(usize, char, Pose)>> {
        eur(m)
            .exits()
            .map(|(chars, _)| {
                chars
                    .into_iter()
                    .enumerate()
                    .filter_map(|(ix, (ch, pose))| Some((ix, ch, pose?)))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn only_changed_digits_roll_once() {
        let mut m = Motion::default();
        let t0 = Instant::now();
        assert!(!show(&mut m, "89.17", t0), "the first frame rolls nothing");

        // The 4 comes up from below; the 1 it replaces rolls out.
        assert!(show(&mut m, "89.47", t0));
        assert_eq!(rolling_in(&m), [3]);
        assert_eq!(eur(&m).pose(3), BELOW);
        assert_eq!(rolling_out(&m), [vec![(3, '1', Pose::REST)]]);

        // It plays once, however often the amount is drawn after.
        assert!(show(&mut m, "89.47", t0 + 100 * MS));
        assert!(!show(&mut m, "89.47", t0 + ROLL));
        assert!(!show(&mut m, "89.47", t0 + ROLL + 500 * MS));
    }

    #[test]
    fn an_interrupted_roll_carries_on_from_where_it_is() {
        let mut m = Motion::default();
        let t0 = Instant::now();
        show(&mut m, "89.17", t0);
        show(&mut m, "89.48", t0);

        // 60ms in, the next change replaces the 4 but leaves the 8 alone.
        let t1 = t0 + 60 * MS;
        show(&mut m, "89.48", t1);
        let (four, eight) = (eur(&m).pose(3), eur(&m).pose(4));
        show(&mut m, "89.78", t1);
        assert_eq!(eur(&m).pose(3), BELOW);
        // The 8 carries on instead of snapping into place, and the 4 rolls
        // out from exactly where it was.
        assert_eq!(eur(&m).pose(4), eight);
        assert_eq!(rolling_out(&m).last().unwrap(), &[(3, '4', four)]);
    }

    #[test]
    fn a_change_of_length_rolls_the_whole_amount() {
        let mut m = Motion::default();
        let t0 = Instant::now();
        show(&mut m, "89.14", t0);
        show(&mut m, "0.89", t0);
        // Every character comes in together, from above (the value fell)...
        assert_eq!(rolling_in(&m), [0, 1, 2, 3]);
        assert!((0..4).all(|ix| eur(&m).pose(ix)
            == Pose {
                y: -1.,
                opacity: 0.
            }));
        // ...while all of the old amount leaves, laid out as it was.
        let out = rolling_out(&m);
        let old: String = out[0].iter().map(|&(_, ch, _)| ch).collect();
        assert_eq!(old, "89.14");
    }

    #[test]
    fn another_currency_does_not_roll() {
        let mut m = Motion::default();
        let t0 = Instant::now();
        m.update_rolls([shown("anchor", "USD", "100.00")], t0, true);
        // The anchor becomes EUR (a promotion): its amount slides in with
        // the card instead.
        assert!(!m.update_rolls([shown("anchor", "EUR", "89.17")], t0, true));
        // Nor does anything with reduced motion.
        assert!(!m.update_rolls([shown("anchor", "EUR", "90.00")], t0, false));
    }

    #[test]
    fn displays_that_disappear_forget() {
        let mut m = Motion::default();
        let t0 = Instant::now();
        m.update_rolls([shown("anchor", "USD", "100.00")], t0, true);
        m.update_rolls([], t0, true);
        // Shown again, it starts afresh instead of rolling from 100.00.
        assert!(!m.update_rolls([shown("anchor", "USD", "250.00")], t0, true));
    }

    #[test]
    fn the_spring_settles_with_a_small_overshoot() {
        assert_eq!(spring(0.), 0.);
        assert_eq!(spring(1.), 1.);
        let samples: Vec<f32> = (0..=100).map(|i| spring(i as f32 / 100.)).collect();
        let peak = samples.iter().copied().fold(0., f32::max);
        assert!(peak > 1. && peak < 1.06, "overshoot {peak}");
        assert!((spring(0.95) - 1.).abs() < 0.02);
    }
}
