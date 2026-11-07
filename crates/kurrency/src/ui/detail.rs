//! The history detail page (docs/DESIGN.md §6): one row's currency against the
//! current source currency, with the current rate, the change over the range,
//! an area chart, and the range's high, low and average.

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::AreaChart;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, actions, div, linear_color_stop,
    linear_gradient, px,
};
use kurrency_core::i18n::tr;
use kurrency_core::rates::history::{self, CHART_POINTS, Date, History, Range, Stats};
use kurrency_core::rates::now_secs;
use rust_decimal::Decimal;

use crate::rates::history_provider::{HistoryError, HistoryProvider, SOURCE_LABEL};
use crate::rates::store::RatesStore;
use crate::ui::anchor::tabular_numerals;

actions!(kurrency_detail, [Back]);

const CONTEXT: &str = "KurrencyDetail";

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

// Lucide icons (ISC) outside gpui-kit's default embedded set, inlined so
// the app's asset source stays as it is.
const ARROW_LEFT_RIGHT_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3 4 7l4 4"/><path d="M4 7h16"/><path d="m16 21 4-4-4-4"/><path d="M20 17H4"/></svg>"#;

/// ⇄, the flip-pair button.
fn flip_icon() -> Icon {
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
    text: SharedString,
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
    /// "1 {base} = x {quote}". The base is the source currency.
    base: String,
    quote: String,
    range: Range,
    rates: Entity<RatesStore>,
    provider: Option<HistoryProvider>,
    /// Whether a rise is drawn in green (else red).
    green_up: bool,
    /// The decimal separator of Settings → Number format.
    decimal: char,
    history: Option<History>,
    derived: Derived,
    loading: bool,
    error: Option<HistoryError>,
    /// The in-flight load. Replacing or dropping it (leaving the page)
    /// cancels the request.
    _load: Option<Task<()>>,
}

impl DetailView {
    pub(crate) fn new(
        base: String,
        quote: String,
        rates: Entity<RatesStore>,
        provider: Option<HistoryProvider>,
        green_up: bool,
        decimal: char,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            base,
            quote,
            range: Range::default(),
            rates,
            provider,
            green_up,
            decimal,
            history: None,
            derived: Derived::default(),
            loading: false,
            error: None,
            _load: None,
        };
        this.load(cx);
        this
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
            self.load(cx);
        }
    }

    pub(crate) fn flip(&mut self, cx: &mut Context<Self>) {
        std::mem::swap(&mut self.base, &mut self.quote);
        self.history = None;
        self.derived = Derived::default();
        self.load(cx);
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
                text: history::format_rate_in(p.rate, dp, self.decimal).into(),
            })
            .collect();
        self.derived = Derived { stats, chart, dp };
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

    fn change_color(&self, change: Decimal, cx: &App) -> Hsla {
        let theme = cx.theme();
        if change.is_zero() {
            theme.muted_foreground
        } else if change.is_sign_positive() == self.green_up {
            theme.success
        } else {
            theme.danger
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .h(px(48.))
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("detail-back")
                    .ghost()
                    .icon(IconName::ChevronLeft)
                    .tooltip(tr!("detail.back"))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseDetail))),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{} / {}", self.base, self.quote)),
            )
            .when(self.loading && !self.derived.chart.is_empty(), |el| {
                el.child(Spinner::new().xsmall().color(theme.muted_foreground))
            })
            .child(
                Button::new("detail-flip")
                    .ghost()
                    .icon(flip_icon())
                    .tooltip(tr!("detail.flip"))
                    .on_click(cx.listener(|this, _, _, cx| this.flip(cx))),
            )
    }

    fn render_hero(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let rate = self.current_rate(cx);
        let dp = match &self.derived.stats {
            Some(_) => self.derived.dp,
            None => rate.map(history::rate_decimals).unwrap_or(4),
        };
        let figure = match rate {
            Some(rate) => format!(
                "{} {}",
                history::format_rate_in(rate, dp, self.decimal),
                self.quote
            ),
            None => format!("— {}", self.quote),
        };
        v_flex()
            .gap_1()
            .font_features(tabular_numerals())
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .child(tr!("detail.one_unit", code = self.base)),
            )
            .child(
                div()
                    .text_size(px(32.))
                    .line_height(px(40.))
                    .font_weight(FontWeight::NORMAL)
                    .child(figure),
            )
            .map(|el| match &self.derived.stats {
                Some(stats) => {
                    let arrow = if stats.change.is_sign_negative() && !stats.change.is_zero() {
                        "▼"
                    } else if stats.change.is_zero() {
                        "•"
                    } else {
                        "▲"
                    };
                    let pct = stats.change_pct.abs().round_dp(2);
                    let sign = if stats.change_pct.is_sign_negative() && !pct.is_zero() {
                        "−"
                    } else if pct.is_zero() {
                        ""
                    } else {
                        "+"
                    };
                    el.child(
                        div()
                            .text_size(px(13.))
                            .text_color(self.change_color(stats.change, cx))
                            .child(format!(
                                "{arrow} {} ({sign}{}%) · {}",
                                history::format_rate_in(
                                    stats.change.abs(),
                                    self.derived.dp,
                                    self.decimal
                                ),
                                history::format_rate_in(pct, 2, self.decimal),
                                self.range.label()
                            )),
                    )
                }
                None if self.loading => el.child(Skeleton::new().w(px(160.)).h(px(14.))),
                None => el.child(div().h(px(18.))),
            })
    }

    fn render_chart(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
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
            let line = theme.foreground.opacity(0.85);
            AreaChart::new(points.clone())
                .id("detail-chart")
                .x(|p: &ChartPoint| p.label.clone())
                .y(|p: &ChartPoint| p.value)
                .stroke(line)
                .fill(linear_gradient(
                    0.,
                    linear_color_stop(theme.foreground.opacity(0.22), 1.),
                    linear_color_stop(theme.foreground.opacity(0.), 0.),
                ))
                .linear()
                .x_axis(false)
                .grid(false)
                .y_domain(min - pad, max + pad)
                .y_padding(8., 4.)
                .appear_key((self.base.clone(), self.quote.clone(), self.range))
                .tooltip_content(|p: &ChartPoint, _, cx| {
                    div()
                        .text_size(px(12.))
                        .font_features(tabular_numerals())
                        .text_color(cx.theme().popover_foreground)
                        .child(format!("{} · {}", p.label, p.text))
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
        div().w_full().h(px(180.)).child(body)
    }

    fn render_ranges(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = Range::ALL
            .iter()
            .position(|r| *r == self.range)
            .unwrap_or(1);
        TabBar::new("detail-range")
            .segmented()
            .w_full()
            .selected_index(selected)
            .children(
                Range::ALL
                    .iter()
                    .map(|r| Tab::new().label(r.label()).flex_1()),
            )
            .on_click(cx.listener(|this, ix: &usize, _, cx| {
                if let Some(range) = Range::ALL.get(*ix) {
                    this.set_range(*range, cx);
                }
            }))
    }

    fn render_stats(&self, cx: &App) -> Option<impl IntoElement> {
        let theme = cx.theme();
        let stats = self.derived.stats.as_ref()?;
        let dp = self.derived.dp;
        let with_year = self.range.spans_years();
        let row = |label: String, value: Decimal, date: Option<Date>, last: bool| {
            h_flex()
                .h(px(40.))
                .text_size(px(14.))
                .when(!last, |el| el.border_b_1().border_color(theme.border))
                .child(
                    div()
                        .w(px(96.))
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::MEDIUM)
                        .child(history::format_rate_in(value, dp, self.decimal)),
                )
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .when_some(date, |el, d| el.child(d.short(with_year))),
                )
        };
        Some(
            v_flex()
                .font_features(tabular_numerals())
                .child(row(
                    tr!("detail.high"),
                    stats.high.rate,
                    Some(stats.high.date),
                    false,
                ))
                .child(row(
                    tr!("detail.low"),
                    stats.low.rate,
                    Some(stats.low.date),
                    false,
                ))
                .child(row(tr!("detail.average"), stats.average, None, true)),
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
        let (bg, fg) = (theme.background, theme.foreground);
        v_flex()
            .id("detail")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .size_full()
            .bg(bg)
            .text_color(fg)
            // Clicks must not fall through to the list underneath.
            .occlude()
            .child(self.render_header(cx))
            .child(
                v_flex()
                    .id("detail-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .gap_4()
                    .child(self.render_hero(cx))
                    .child(self.render_chart(cx))
                    .child(self.render_ranges(cx))
                    .children(self.render_stats(cx)),
            )
            .child(self.render_footer(cx))
    }
}

/// The page slide-in: 180ms ease-out from the right (docs/DESIGN.md §4).
pub(crate) const SLIDE_DURATION: Duration = Duration::from_millis(180);
