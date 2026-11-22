//! The history detail page (docs/DESIGN.md §6): one pair's rate history with
//! a trend-coloured area chart, unobscured Apple-Stocks top readout on hover
//! (including "your amount then"), drag-to-measure across any two dates, and
//! a range position bar alongside High, Low and Average.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::chart::AreaChart;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, actions, canvas, div, hsla,
    linear_color_stop, linear_gradient, px, relative,
};
use kurrency_core::i18n::tr;
use kurrency_core::money;
use kurrency_core::present;
use kurrency_core::rates::history::{self, CHART_POINTS, Date, History, Range, Stats};
use kurrency_core::rates::now_secs;
use rust_decimal::Decimal;

use crate::assets;
use crate::rates::history_provider::{HistoryError, HistoryProvider, SOURCE_LABEL};
use crate::rates::store::RatesStore;
use crate::ui::anchor::tabular_numerals;
use crate::ui::segmented;
use crate::ui::theme::Tokens;

actions!(kurrency_detail, [Back]);

const CONTEXT: &str = "KurrencyDetail";

/// Vertical headroom passed to `AreaChart::y_padding`.
const CHART_PAD_TOP: f32 = 10.;
const CHART_PAD_BOTTOM: f32 = 6.;

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Back, Some(CONTEXT)),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-left"
            } else {
                "ctrl-left"
            },
            Back,
            Some(CONTEXT),
        ),
    ]);
}

// Lucide "arrow-left-right" (ISC).
const ARROW_LEFT_RIGHT_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3 4 7l4 4"/><path d="M4 7h16"/><path d="m16 21 4-4-4-4"/><path d="M20 17H4"/></svg>"#;

/// ⇄, the flip-pair icon shared with the unified header.
pub(crate) fn flip_icon() -> Icon {
    Icon::default().data(ARROW_LEFT_RIGHT_SVG)
}

/// Asks the list to close the page and restore focus.
pub(crate) struct CloseDetail;

impl EventEmitter<CloseDetail> for DetailView {}

/// One drawn point of the chart.
#[derive(Clone)]
struct ChartPoint {
    label: SharedString,
    value: f64,
    rate: Decimal,
}

/// What the range shows once a load finishes.
#[derive(Default)]
struct Derived {
    stats: Option<Stats>,
    chart: Vec<ChartPoint>,
    /// Decimal places for this pair's rates.
    dp: u32,
}

pub(crate) struct DetailView {
    focus_handle: FocusHandle,
    /// "1 {base} = x {quote}".
    base: String,
    quote: String,
    /// The user's anchor currency & amount, for "your amount then" readouts.
    anchor_code: String,
    anchor_amount: Option<Decimal>,
    fmt: money::Format,
    range: Range,
    rates: Entity<RatesStore>,
    provider: Option<HistoryProvider>,
    /// Whether a rise is drawn in green (else red).
    green_up: bool,
    history: Option<History>,
    derived: Derived,
    loading: bool,
    error: Option<HistoryError>,
    /// Chart bounds captured each frame for scrub & drag-to-measure hit math.
    chart_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Currently scrubbed point index.
    hover_ix: Option<usize>,
    /// Drag-to-measure anchor and current end index.
    drag_start: Option<usize>,
    drag_end: Option<usize>,
    /// The in-flight load. Replacing or dropping it cancels the request.
    _load: Option<Task<()>>,
}

impl DetailView {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        base: String,
        quote: String,
        anchor_code: String,
        anchor_amount: Option<Decimal>,
        fmt: money::Format,
        rates: Entity<RatesStore>,
        provider: Option<HistoryProvider>,
        green_up: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            base,
            quote,
            anchor_code,
            anchor_amount,
            fmt,
            range: Range::default(),
            rates,
            provider,
            green_up,
            history: None,
            derived: Derived::default(),
            loading: false,
            error: None,
            chart_bounds: Rc::new(Cell::new(Bounds::default())),
            hover_ix: None,
            drag_start: None,
            drag_end: None,
            _load: None,
        };
        this.load(cx);
        this
    }

    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    pub(crate) fn quote(&self) -> &str {
        &self.quote
    }

    pub(crate) fn is_busy_with_chart(&self) -> bool {
        self.loading && !self.derived.chart.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn pair(&self) -> (&str, &str) {
        (&self.base, &self.quote)
    }

    #[cfg(test)]
    pub(crate) fn range(&self) -> Range {
        self.range
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> Option<&Stats> {
        self.derived.stats.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn chart_len(&self) -> usize {
        self.derived.chart.len()
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&HistoryError> {
        self.error.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(crate) fn set_range(&mut self, range: Range, cx: &mut Context<Self>) {
        if self.range != range {
            self.range = range;
            self.clear_scrub();
            self.load(cx);
        }
    }

    pub(crate) fn flip(&mut self, cx: &mut Context<Self>) {
        std::mem::swap(&mut self.base, &mut self.quote);
        self.history = None;
        self.derived = Derived::default();
        self.clear_scrub();
        self.load(cx);
    }

    fn clear_scrub(&mut self) {
        self.hover_ix = None;
        self.drag_start = None;
        self.drag_end = None;
    }

    fn today() -> Date {
        Date::from_unix_secs(now_secs())
    }

    /// Shows what the cache has for the range at once, then fetches the
    /// missing days in the background.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.rederive();
        let Some(provider) = self.provider.clone() else {
            self.loading = false;
            self.error = Some(HistoryError::Offline);
            cx.notify();
            return;
        };
        self.loading = true;
        cx.notify();

        let (base, quote) = (self.base.clone(), self.quote.clone());
        let from = self.range.start(Self::today());
        let existing = self
            .history
            .clone()
            .filter(|h| h.base == base && h.quote == quote);
        self._load = Some(cx.spawn(async move |this, cx| {
            let history = match existing {
                Some(history) => history,
                None => {
                    let provider = provider.clone();
                    let cached = cx
                        .background_spawn(async move { provider.cached(&base, &quote) })
                        .await;
                    let shown = cached.clone();
                    if this
                        .update(cx, |this, cx| {
                            this.history = Some(shown);
                            this.rederive();
                            cx.notify();
                        })
                        .is_err()
                    {
                        return;
                    }
                    cached
                }
            };
            let (history, error) = cx
                .background_spawn(async move { provider.update(history, from, now_secs()).await })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.history = Some(history);
                this.error = error;
                this.loading = false;
                this.rederive();
                this._load = None;
                cx.notify();
            });
        }));
    }

    fn rederive(&mut self) {
        let Some(history) = self
            .history
            .as_ref()
            .filter(|h| h.base == self.base && h.quote == self.quote)
        else {
            self.derived = Derived::default();
            return;
        };
        let slice = history.since(self.range.start(Self::today()));
        let stats = history::stats(slice);
        let dp = stats
            .as_ref()
            .map(|s| history::rate_decimals(s.last))
            .unwrap_or(4);
        let with_year = self.range.spans_years();
        let chart = history::downsample(slice, CHART_POINTS)
            .into_iter()
            .map(|p| ChartPoint {
                label: p.date.short(with_year).into(),
                value: p.rate.try_into().unwrap_or(0.),
                rate: p.rate,
            })
            .collect();
        self.derived = Derived { stats, chart, dp };
        if self
            .hover_ix
            .is_some_and(|ix| ix >= self.derived.chart.len())
        {
            self.clear_scrub();
        }
    }

    fn back(&mut self, _: &Back, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(CloseDetail);
    }

    /// The current rate: the newest history point, else the live table.
    fn current_rate(&self, cx: &App) -> Option<Decimal> {
        self.derived.stats.as_ref().map(|s| s.last).or_else(|| {
            self.rates
                .read(cx)
                .table()
                .convert(Decimal::ONE, &self.base, &self.quote)
        })
    }

    /// Active drag-to-measure span `(start, end)` in chronological order, when
    /// the user dragged across at least two distinct points.
    fn drag_span(&self) -> Option<(usize, usize)> {
        let (a, b) = (self.drag_start?, self.drag_end?);
        if a == b || a >= self.derived.chart.len() || b >= self.derived.chart.len() {
            return None;
        }
        Some((a.min(b), a.max(b)))
    }

    /// Maps a window pointer x-coordinate to the nearest point index in the chart.
    fn index_for_x(&self, x: Pixels) -> Option<usize> {
        let len = self.derived.chart.len();
        if len < 2 {
            return None;
        }
        let bounds = self.chart_bounds.get();
        let width = f32::from(bounds.size.width);
        if width <= 1. {
            return None;
        }
        let rel = ((f32::from(x) - f32::from(bounds.origin.x)) / width).clamp(0., 1.);
        Some(((rel * (len - 1) as f32).round() as usize).min(len - 1))
    }

    fn change_color(&self, change: Decimal, cx: &App) -> Hsla {
        let t = Tokens::get(cx);
        if change.is_zero() {
            t.accent
        } else if change.is_sign_positive() == self.green_up {
            t.green
        } else {
            t.red
        }
    }

    fn trend_color(&self, cx: &App) -> Hsla {
        if let Some((a, b)) = self.drag_span() {
            let delta = self.derived.chart[b].rate - self.derived.chart[a].rate;
            return self.change_color(delta, cx);
        }
        match &self.derived.stats {
            Some(stats) => self.change_color(stats.change, cx),
            None => Tokens::get(cx).accent,
        }
    }

    /// Formats `amount` in `code` with its symbol (`$100`, `€ 89.20`).
    fn format_money(&self, code: &str, value: Decimal) -> String {
        let parts = present::amount_parts(code, value, &self.fmt, assets::has_glyph);
        match parts.symbol {
            Some(sym) => format!("{sym}{}{}", parts.integer, parts.fraction),
            None => format!("{}{}", parts.integer, parts.fraction),
        }
    }

    /// Computes "your amount" line for `rate` (`historical`: past tense with
    /// optional comparison against `compare_rate`; idle: present `from = to`),
    /// when the user's anchor currency is part of this pair.
    fn your_amount_line(
        &self,
        rate: Decimal,
        compare_rate: Option<Decimal>,
        historical: bool,
    ) -> Option<String> {
        let amount = self.anchor_amount?;
        if amount.is_zero() || rate.is_zero() {
            return None;
        }
        let (target_code, converted, delta) = if self.base == self.anchor_code {
            let now = amount * rate;
            let diff = compare_rate.map(|cmp| now - amount * cmp);
            (&self.quote, now, diff)
        } else if self.quote == self.anchor_code {
            let now = amount / rate;
            let diff = compare_rate
                .filter(|c| !c.is_zero())
                .map(|cmp| now - amount / cmp);
            (&self.base, now, diff)
        } else {
            return None;
        };
        let from = self.format_money(&self.anchor_code, amount);
        let to = self.format_money(target_code, converted);
        if !historical {
            return Some(tr!("detail.your_amount_now", from = from, to = to));
        }
        match delta {
            Some(diff) if !money::round_to(diff, self.fmt.dp(target_code)).is_zero() => {
                let sign = if diff.is_sign_positive() { "+" } else { "−" };
                let delta_str = format!("{sign}{}", self.format_money(target_code, diff.abs()));
                Some(tr!(
                    "detail.your_amount_delta",
                    from = from,
                    to = to,
                    delta = delta_str
                ))
            }
            _ => Some(tr!("detail.your_amount", from = from, to = to)),
        }
    }

    fn format_delta(
        &self,
        change: Decimal,
        pct: Decimal,
        suffix: Option<String>,
        cx: &App,
    ) -> gpui_kit::AnyElement {
        let arrow = if change.is_sign_negative() && !change.is_zero() {
            "▼"
        } else if change.is_zero() {
            "•"
        } else {
            "▲"
        };
        let pct_abs = pct.abs().round_dp(2);
        let sign = if pct.is_sign_negative() && !pct_abs.is_zero() {
            "−"
        } else if pct_abs.is_zero() {
            ""
        } else {
            "+"
        };
        let dec = self.fmt.style.decimal;
        let core = format!(
            "{arrow} {} ({sign}{}%)",
            history::format_rate_in(change.abs(), self.derived.dp, dec),
            history::format_rate_in(pct_abs, 2, dec),
        );
        let text = match suffix {
            Some(s) => format!("{core} · {s}"),
            None => core,
        };
        div()
            .text_color(self.change_color(change, cx))
            .whitespace_nowrap()
            .child(text)
            .into_any_element()
    }

    fn render_hero(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let t = Tokens::get(cx);
        let dec = self.fmt.style.decimal;
        let live_rate = self.current_rate(cx);
        let dp = match &self.derived.stats {
            Some(_) => self.derived.dp,
            None => live_rate.map(history::rate_decimals).unwrap_or(4),
        };

        let (left_label, right_badge, display_rate, amount_note) = if let Some((a, b)) =
            self.drag_span()
        {
            let start = &self.derived.chart[a];
            let end = &self.derived.chart[b];
            let delta = end.rate - start.rate;
            let pct = if start.rate.is_zero() {
                Decimal::ZERO
            } else {
                (delta * Decimal::ONE_HUNDRED / start.rate).round_dp(2)
            };
            (
                format!("{} → {}", start.label, end.label),
                self.format_delta(delta, pct, None, cx),
                Some(end.rate),
                self.your_amount_line(end.rate, Some(start.rate), true),
            )
        } else if let Some(ix) = self.hover_ix
            && let Some(p) = self.derived.chart.get(ix)
        {
            let first = self.derived.chart.first().map(|f| f.rate);
            let badge = match first {
                Some(f) if !f.is_zero() => {
                    let delta = p.rate - f;
                    let pct = (delta * Decimal::ONE_HUNDRED / f).round_dp(2);
                    self.format_delta(delta, pct, None, cx)
                }
                _ => div().into_any_element(),
            };
            (
                format!("{} · {}", p.label, tr!("detail.one_unit", code = self.base)),
                badge,
                Some(p.rate),
                self.your_amount_line(p.rate, live_rate, true),
            )
        } else {
            let badge = match &self.derived.stats {
                Some(stats) => {
                    self.format_delta(stats.change, stats.change_pct, Some(self.range.label()), cx)
                }
                None if self.loading => Skeleton::new().w(px(140.)).h(px(14.)).into_any_element(),
                None => div().into_any_element(),
            };
            (
                tr!("detail.one_unit", code = self.base),
                badge,
                live_rate,
                live_rate.and_then(|r| self.your_amount_line(r, None, false)),
            )
        };

        let rate_str = match display_rate {
            Some(rate) => history::format_rate_in(rate, dp, dec),
            None => "—".to_string(),
        };
        let is_scrubbing = self.hover_ix.is_some() || self.drag_span().is_some();

        v_flex()
            .font_features(tabular_numerals())
            .child(
                h_flex()
                    .h(px(18.))
                    .gap_2()
                    .justify_between()
                    .text_size(px(12.5))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .whitespace_nowrap()
                            .truncate()
                            .child(left_label),
                    )
                    .child(right_badge),
            )
            .child(
                h_flex()
                    .items_baseline()
                    .gap_1p5()
                    .h(px(38.))
                    .child(
                        div()
                            .text_size(px(30.))
                            .line_height(px(38.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(rate_str),
                    )
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.muted_foreground)
                            .child(self.quote.clone()),
                    ),
            )
            // Always reserve the 16px personal-amount slot so hovering never
            // shifts the chart below.
            .child(
                div()
                    .h(px(16.))
                    .text_size(px(11.5))
                    .text_color(if is_scrubbing { t.accent } else { t.muted })
                    .truncate()
                    .children(amount_note),
            )
    }

    fn render_chart(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let t = Tokens::get(cx);
        let points = &self.derived.chart;
        let body = if points.len() >= 2 {
            let (min, max) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                (lo.min(p.value), hi.max(p.value))
            });
            let pad = if max > min {
                0.
            } else {
                min.abs() * 0.01 + 1e-9
            };
            let lo = min - pad;
            let hi = max + pad;
            let trend = self.trend_color(cx);

            let chart = AreaChart::new(points.clone())
                .id("detail-chart")
                .interactive(false)
                .x(|p: &ChartPoint| p.label.clone())
                .y(|p: &ChartPoint| p.value)
                .stroke(trend)
                .fill(linear_gradient(
                    0.,
                    linear_color_stop(trend.opacity(0.24), 1.),
                    linear_color_stop(trend.opacity(0.), 0.),
                ))
                .linear()
                .x_axis(false)
                .grid(false)
                .reference_line(max)
                .reference_line(min)
                .y_domain(lo, hi)
                .y_padding(CHART_PAD_TOP, CHART_PAD_BOTTOM)
                .appear_key((self.base.clone(), self.quote.clone(), self.range));

            let dec = self.fmt.style.decimal;
            let dp = self.derived.dp;
            let high_label = self
                .derived
                .stats
                .as_ref()
                .map(|s| history::format_rate_in(s.high.rate, dp, dec));
            let low_label = self
                .derived
                .stats
                .as_ref()
                .map(|s| history::format_rate_in(s.low.rate, dp, dec));

            let bounds_cell = self.chart_bounds.clone();
            let bounds = self.chart_bounds.get();
            let chart_h = f32::from(bounds.size.height);
            let n = points.len();
            let x_frac = |ix: usize| ix as f32 / (n - 1).max(1) as f32;
            let y_px = |value: f64| -> f32 {
                if chart_h <= CHART_PAD_TOP + CHART_PAD_BOTTOM || hi <= lo {
                    return chart_h / 2.;
                }
                let plot_h = chart_h - CHART_PAD_TOP - CHART_PAD_BOTTOM;
                let norm = ((value - lo) / (hi - lo)).clamp(0., 1.) as f32;
                CHART_PAD_TOP + plot_h * (1. - norm)
            };

            let dot = |ix: usize| {
                let p = &points[ix];
                let xf = x_frac(ix);
                let yp = y_px(p.value);
                div().absolute().left(relative(xf)).top(px(yp)).child(
                    div()
                        .relative()
                        .left(px(-4.5))
                        .top(px(-4.5))
                        .size(px(9.))
                        .rounded_full()
                        .bg(trend)
                        .border_2()
                        .border_color(t.bg),
                )
            };

            let drag_span = self.drag_span();
            let hover_ix = self.hover_ix;

            div()
                .id("detail-chart-overlay")
                .relative()
                .size_full()
                .cursor_crosshair()
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if !*hovered {
                        this.clear_scrub();
                        cx.notify();
                    }
                }))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    let ix = this.index_for_x(event.position.x);
                    let mut changed = this.hover_ix != ix;
                    this.hover_ix = ix;
                    if this.drag_start.is_some() {
                        if event.pressed_button == Some(MouseButton::Left) {
                            if this.drag_end != ix {
                                this.drag_end = ix;
                                changed = true;
                            }
                        } else {
                            this.drag_start = None;
                            this.drag_end = None;
                            changed = true;
                        }
                    }
                    if changed {
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        let ix = this.index_for_x(event.position.x);
                        this.hover_ix = ix;
                        this.drag_start = ix;
                        this.drag_end = ix;
                        cx.notify();
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _, cx| {
                        if this.drag_start.is_some() {
                            this.drag_start = None;
                            this.drag_end = None;
                            cx.notify();
                        }
                    }),
                )
                .child(chart)
                // Capture actual painted bounds for exact pointer-to-point math.
                .child(
                    canvas(
                        move |b, _, _| {
                            bounds_cell.set(b);
                        },
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                // Subtle muted High / Low edge markers.
                .when_some(high_label, |el, label| {
                    el.child(
                        div()
                            .absolute()
                            .right_0()
                            .top(px(0.))
                            .text_size(px(10.))
                            .text_color(t.muted.opacity(0.65))
                            .font_features(tabular_numerals())
                            .child(label),
                    )
                })
                .when_some(low_label, |el, label| {
                    el.child(
                        div()
                            .absolute()
                            .right_0()
                            .bottom(px(2.))
                            .text_size(px(10.))
                            .text_color(t.muted.opacity(0.65))
                            .font_features(tabular_numerals())
                            .child(label),
                    )
                })
                // Drag-to-measure shaded range band + dual hairlines.
                .when_some(drag_span, |el, (a, b)| {
                    let xa = x_frac(a);
                    let xb = x_frac(b);
                    el.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(relative(xa))
                            .w(relative(xb - xa))
                            .bg(trend.opacity(0.11))
                            .border_l_1()
                            .border_r_1()
                            .border_color(trend.opacity(0.45)),
                    )
                    .child(dot(a))
                    .child(dot(b))
                })
                // Single-point crosshair + glowing dot (no floating bubble).
                .when(drag_span.is_none(), |el| {
                    el.when_some(hover_ix.filter(|&ix| ix < n), |el, ix| {
                        let xf = x_frac(ix);
                        el.child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(relative(xf))
                                .w(px(1.))
                                .border_l_1()
                                .border_dashed()
                                .border_color(trend.opacity(0.55)),
                        )
                        .child(dot(ix))
                    })
                })
                .into_any_element()
        } else if self.loading {
            Skeleton::new()
                .size_full()
                .rounded(px(8.))
                .into_any_element()
        } else {
            let message = match (&self.error, points.is_empty()) {
                (Some(HistoryError::Offline), _) => tr!("detail.offline"),
                (Some(HistoryError::Failed(_)), _) => tr!("detail.failed"),
                _ => tr!("detail.empty"),
            };
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .text_color(theme.muted_foreground)
                .child(message)
                .into_any_element()
        };

        div()
            .w_full()
            .flex_1()
            .min_h(px(CHART_MIN_H))
            .max_h(px(CHART_MAX_H))
            .child(body)
    }

    fn render_ranges(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Tokens::get(cx);
        segmented::bar(&t)
            .w_full()
            .children(Range::ALL.iter().map(|range| {
                let range = *range;
                segmented::segment(
                    SharedString::from(format!("detail-range-{}", range.label())),
                    range == self.range,
                    &t,
                )
                .flex_1()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(range.label())
                .on_click(cx.listener(move |this, _, _, cx| this.set_range(range, cx)))
            }))
    }

    fn render_stats(&self, cx: &App) -> Option<impl IntoElement> {
        let theme = cx.theme();
        let t = Tokens::get(cx);
        let stats = self.derived.stats.as_ref()?;
        let dp = self.derived.dp;
        let dec = self.fmt.style.decimal;
        let with_year = self.range.spans_years();
        let trend = self.trend_color(cx);

        // Where today's rate sits between the range's low and high (0..=1).
        let span = stats.high.rate - stats.low.rate;
        let frac: f32 = if span.is_zero() {
            0.5
        } else {
            ((stats.last - stats.low.rate) / span)
                .try_into()
                .unwrap_or(0.5_f32)
                .clamp(0., 1.)
        };
        let pct = (frac * 100.).round() as u32;
        let track_bg = if t.dark {
            hsla(0., 0., 1., 0.08)
        } else {
            hsla(0., 0., 0., 0.08)
        };

        let range_bar = v_flex()
            .gap(px(6.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(9.))
            .border_1()
            .border_color(t.hairline)
            .bg(t.card)
            .child(
                h_flex()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(t.muted)
                    .child(tr!("detail.low"))
                    .child(
                        div()
                            .text_color(t.text)
                            .font_weight(FontWeight::MEDIUM)
                            .child(tr!(
                                "detail.range_position",
                                pct = pct,
                                range = self.range.label()
                            )),
                    )
                    .child(tr!("detail.high")),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.))
                    .rounded_full()
                    .bg(track_bg)
                    .child(
                        div()
                            .h_full()
                            .w(relative(frac))
                            .rounded_full()
                            .bg(trend.opacity(0.65)),
                    )
                    .child(
                        div().absolute().left(relative(frac)).top(px(-2.5)).child(
                            div()
                                .relative()
                                .left(px(-4.5))
                                .size(px(9.))
                                .rounded_full()
                                .bg(t.text)
                                .border_2()
                                .border_color(trend),
                        ),
                    ),
            );

        let column = |label: String, value: Decimal, date: Option<Date>| {
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(14.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(history::format_rate_in(value, dp, dec)),
                )
                .child(
                    div()
                        .h(px(15.))
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .when_some(date, |el, d| el.child(d.short(with_year))),
                )
        };

        Some(
            v_flex()
                .flex_shrink_0()
                .gap(px(10.))
                .font_features(tabular_numerals())
                .child(range_bar)
                .child(
                    h_flex()
                        .items_start()
                        .gap_3()
                        .child(column(
                            tr!("detail.high"),
                            stats.high.rate,
                            Some(stats.high.date),
                        ))
                        .child(column(
                            tr!("detail.low"),
                            stats.low.rate,
                            Some(stats.low.date),
                        ))
                        .child(column(tr!("detail.average"), stats.average, None)),
                ),
        )
    }

    fn render_footer(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let note = match &self.error {
            Some(HistoryError::Offline) if self.derived.stats.is_some() => {
                tr!("detail.note_offline")
            }
            Some(HistoryError::Failed(_)) if self.derived.stats.is_some() => {
                tr!("detail.note_failed")
            }
            _ => String::new(),
        };
        h_flex()
            .w_full()
            .h(px(36.))
            .px_4()
            .border_t_1()
            .border_color(theme.border)
            .text_size(px(12.))
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .truncate()
                    .child(tr!("detail.footer", source = SOURCE_LABEL, note = note)),
            )
    }
}

impl Focusable for DetailView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DetailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border) = (theme.background, theme.foreground, theme.border);
        v_flex()
            .id("detail")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .size_full()
            .bg(bg)
            .text_color(fg)
            .border_t_1()
            .border_color(border)
            // Clicks must not fall through to the list underneath.
            .occlude()
            .child(
                v_flex()
                    .id("detail-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .gap_3()
                    .child(div().flex_shrink_0().child(self.render_hero(cx)))
                    .child(self.render_chart(cx))
                    .child(div().flex_shrink_0().child(self.render_ranges(cx)))
                    .children(self.render_stats(cx)),
            )
            .child(self.render_footer(cx))
    }
}

/// The shortest the history chart gets before the page scrolls.
const CHART_MIN_H: f32 = 100.;
/// The tallest the history chart grows in a tall window.
const CHART_MAX_H: f32 = 260.;

/// The page slide-in: 180ms ease-out from the right (docs/DESIGN.md §4).
pub(crate) const SLIDE_DURATION: Duration = Duration::from_millis(180);
