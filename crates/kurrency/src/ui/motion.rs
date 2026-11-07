//! The motion pass (DESIGN §4): subtle, under 250ms and interruptible.
//!
//! - digit roll: when an amount changes, only the changed digits roll in;
//! - anchor swap: the promoted currency slides up into the card while the
//!   old anchor drops into its row;
//! - add / remove: the row's height and opacity grow or collapse;
//! - copy: a check mark flashes where the row's change chip sits;
//! - reorder: while a row is dragged the others spring apart around its
//!   drop slot, and a moved row springs into its new place.
//!
//! Every animation goes through GPUI's `with_animation`, which draws the end
//! state at once when [`App::reduce_motion`](gpui_kit::App::reduce_motion) is
//! set; the transient states (ghost rows, the closing history page) are not
//! created at all then.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, ElementId, IntoElement, ParentElement as _,
    Styled as _, div, ease_out_quint, px,
};

/// Digit roll.
pub(crate) const ROLL: Duration = Duration::from_millis(180);
/// Anchor swap.
pub(crate) const SWAP: Duration = Duration::from_millis(220);
/// Row add and remove.
pub(crate) const RESIZE: Duration = Duration::from_millis(200);
/// How long the copy check mark stays.
pub(crate) const COPIED: Duration = Duration::from_millis(1200);
/// Rows moving apart while a row is dragged, and a moved row settling.
pub(crate) const SPRING: Duration = Duration::from_millis(240);

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

/// What one amount display showed, and what it showed before its last change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Roll {
    pub text: String,
    pub previous: String,
    /// Bumped on every change, which restarts the roll; 0 never rolls.
    pub version: usize,
}

impl Roll {
    /// Whether the character `ix` of [`Self::text`] differs from the one in
    /// the same place before the change, counting from the right so the
    /// decimal point stays aligned.
    pub(crate) fn changed(&self, ix: usize) -> bool {
        if self.version == 0 {
            return false;
        }
        let from_right = self.text.chars().count() - 1 - ix;
        self.text.chars().rev().nth(from_right) != self.previous.chars().rev().nth(from_right)
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

    /// Records what each display shows this frame; displays that are gone
    /// forget their state, so they do not roll when they come back.
    pub(crate) fn update_rolls(&mut self, shown: impl IntoIterator<Item = (String, String)>) {
        let mut rolls = HashMap::new();
        for (key, text) in shown {
            let roll = match self.rolls.remove(&key) {
                Some(roll) if roll.text == text => roll,
                Some(roll) => {
                    self.next += 1;
                    Roll {
                        previous: roll.text,
                        text,
                        version: self.next,
                    }
                }
                None => Roll {
                    previous: text.clone(),
                    text,
                    version: 0,
                },
            };
            rolls.insert(key, roll);
        }
        self.rolls = rolls;
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

/// One character of an amount; a changed one rolls down into place while it
/// fades in.
pub(crate) fn roll_char(
    ch: char,
    key: &str,
    ix: usize,
    roll: Option<&Roll>,
    size: f32,
) -> AnyElement {
    let el = div().relative().child(ch.to_string());
    match roll.filter(|roll| roll.changed(ix)) {
        Some(roll) => el
            .with_animation(
                ElementId::NamedInteger(format!("roll-{key}-{ix}").into(), roll.version as u64),
                Animation::new(ROLL).with_easing(ease_out_quint()),
                move |el, t| el.top(px(-(1. - t) * size * 0.4)).opacity(0.15 + 0.85 * t),
            )
            .into_any_element(),
        None => el.into_any_element(),
    }
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
            Animation::new(ROLL).with_easing(ease_out_quint()),
            gpui_kit::Styled::opacity,
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn only_changed_digits_roll() {
        let mut m = Motion::default();
        m.update_rolls(shown(&[("EUR", "89.17")]));
        let roll = m.roll("EUR").unwrap();
        assert!(
            (0..5).all(|ix| !roll.changed(ix)),
            "the first frame rolls nothing"
        );

        m.update_rolls(shown(&[("EUR", "89.47")]));
        let roll = m.roll("EUR").unwrap().clone();
        let changed: Vec<usize> = (0..5).filter(|&ix| roll.changed(ix)).collect();
        assert_eq!(changed, [3]);

        // Unchanged frames keep the version, so the roll keeps playing.
        m.update_rolls(shown(&[("EUR", "89.47")]));
        assert_eq!(m.roll("EUR").unwrap().version, roll.version);

        // Aligned from the right: a new leading digit is the only change.
        m.update_rolls(shown(&[("EUR", "189.47")]));
        let roll = m.roll("EUR").unwrap();
        let changed: Vec<usize> = (0..6).filter(|&ix| roll.changed(ix)).collect();
        assert_eq!(changed, [0]);
    }

    #[test]
    fn displays_that_disappear_forget() {
        let mut m = Motion::default();
        m.update_rolls(shown(&[("anchor", "100.00")]));
        m.update_rolls(shown(&[]));
        // Shown again, it starts afresh instead of rolling from 100.00.
        m.update_rolls(shown(&[("anchor", "250.00")]));
        let roll = m.roll("anchor").unwrap();
        assert!((0..6).all(|ix| !roll.changed(ix)));
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
