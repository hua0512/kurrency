//! The Settings page (docs/DESIGN.md §7): a full-window page
//! that slides in like the history page. Grouped sections with small
//! uppercase headers; each row has its label on the left and its control on
//! the right. Every change applies at once and is saved.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, actions, div,
    img, px,
};
use kurrency_core::i18n::tr;
use kurrency_core::money::NumberStyle;
use kurrency_core::settings::{
    DecimalPlaces, Language, NumberFormat, RateSource, RefreshInterval, Settings, ThemeId,
    ThemeMode, UpDownColors,
};

use crate::app::{KurrencyApp, RefreshRates};
use crate::rates::store::RatesStore;
use crate::ui::anchor::tabular_numerals;
use crate::ui::flag::mini_flag;
use crate::ui::theme::{CHIP_TINT, SYMBOL_OPACITY, Tokens};

actions!(kurrency_settings, [Back]);

const CONTEXT: &str = "KurrencySettings";

/// The About footer's icon, from the asset source.
pub(crate) const ICON_PATH: &str = "icon.svg";

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Back, Some(CONTEXT))]);
}

/// Asks the list to close the page.
pub(crate) struct CloseSettings;

impl EventEmitter<CloseSettings> for SettingsView {}

pub(crate) struct SettingsView {
    focus_handle: FocusHandle,
    app: WeakEntity<KurrencyApp>,
    rates: Entity<RatesStore>,
    /// The system locale, for "Follow system".
    locale: Option<String>,
    scroll: ScrollHandle,
    _observe_rates: gpui_kit::Subscription,
}

impl SettingsView {
    pub(crate) fn new(
        app: WeakEntity<KurrencyApp>,
        rates: Entity<RatesStore>,
        locale: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            _observe_rates: cx.observe(&rates, |_, _, cx| cx.notify()),
            app,
            rates,
            locale,
            scroll: ScrollHandle::new(),
        }
    }

    /// Scrolls the page down by `y` px (debug screenshots).
    pub(crate) fn scroll_by(&self, y: f32) {
        let offset = self.scroll.offset();
        self.scroll
            .set_offset(gpui_kit::point(offset.x, offset.y - px(y)));
    }

    fn back(&mut self, _: &Back, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(CloseSettings);
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Changes a setting from a control's handler: applied at once and saved.
fn change(
    app: &WeakEntity<KurrencyApp>,
    window: &mut Window,
    cx: &mut App,
    f: impl FnOnce(&mut Settings),
) {
    let _ = app.update(cx, |app, cx| app.update_prefs(window, cx, f));
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Tokens::get(cx);
        let prefs = self
            .app
            .upgrade()
            .map(|app| app.read(cx).prefs.clone())
            .unwrap_or_default();
        let app = self.app.clone();

        let header = h_flex()
            .h(px(48.))
            .px_2()
            .gap_1()
            .border_b_1()
            .border_color(t.hairline)
            .child(
                Button::new("settings-back")
                    .ghost()
                    .icon(IconName::ChevronLeft)
                    .tooltip(tr!("settings.back"))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseSettings))),
            )
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr!("settings.title")),
            );

        let appearance = section(
            tr!("settings.appearance"),
            vec![
                row(tr!("settings.theme"), theme_mode(&prefs, &app), &t),
                gallery(&prefs, &app, cx).into_any_element(),
                row(
                    tr!("settings.up_down"),
                    choice(
                        "up-down",
                        prefs.up_down_colors,
                        &[
                            (UpDownColors::ByLanguage, tr!("settings.by_language")),
                            (UpDownColors::GreenUp, tr!("settings.green_up")),
                            (UpDownColors::RedUp, tr!("settings.red_up")),
                        ],
                        &app,
                        |s, v| s.up_down_colors = v,
                    ),
                    &t,
                ),
                row(
                    tr!("settings.reduce_motion"),
                    {
                        let app = app.clone();
                        Switch::new("reduce-motion")
                            .checked(prefs.reduce_motion)
                            .on_click(move |on, window, cx| {
                                let on = *on;
                                change(&app, window, cx, |s| s.reduce_motion = on);
                            })
                            .into_any_element()
                    },
                    &t,
                ),
                row(
                    tr!("settings.language"),
                    // Applied at once: the next frame is in the new language.
                    choice(
                        "language",
                        prefs.language,
                        &[
                            (Language::System, tr!("settings.language_system")),
                            (Language::English, tr!("settings.english")),
                            (Language::ChineseSimplified, tr!("settings.chinese")),
                        ],
                        &app,
                        |s, v| s.language = v,
                    ),
                    &t,
                ),
            ],
            &t,
        );

        let system = NumberFormat::System.style(self.locale.as_deref());
        let system_example = if system == NumberStyle::DOT_COMMA {
            "1.234,56"
        } else if system == NumberStyle::SPACE_COMMA {
            "1 234,56"
        } else {
            "1,234.56"
        };
        let numbers = section(
            tr!("settings.numbers"),
            vec![
                row(
                    tr!("settings.number_format"),
                    choice(
                        "number-format",
                        prefs.number_format,
                        &[
                            (
                                NumberFormat::System,
                                tr!("settings.follow_system", example = system_example),
                            ),
                            (NumberFormat::CommaDot, "1,234.56".into()),
                            (NumberFormat::DotComma, "1.234,56".into()),
                            (NumberFormat::SpaceComma, "1\u{202f}234,56".into()),
                        ],
                        &app,
                        |s, v| s.number_format = v,
                    ),
                    &t,
                ),
                row(
                    tr!("settings.decimal_places"),
                    choice(
                        "decimal-places",
                        prefs.decimal_places,
                        &[
                            (
                                DecimalPlaces::CurrencyDefault,
                                tr!("settings.currency_default"),
                            ),
                            (DecimalPlaces::ExtraTwo, tr!("settings.extra_two")),
                        ],
                        &app,
                        |s, v| s.decimal_places = v,
                    ),
                    &t,
                ),
            ],
            &t,
        );

        let rates = self.rates.read(cx);
        let refreshing = rates.is_refreshing();
        let offset = chrono::Local::now().offset().local_minus_utc() as i64;
        let updated = kurrency_core::rates::last_updated(
            rates.table(),
            offset,
            kurrency_core::rates::now_secs(),
        );
        let source_label = rates.table().source.display_name();
        let last_updated = h_flex()
            .gap_1()
            .child(
                v_flex()
                    .items_end()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_features(tabular_numerals())
                            .child(updated),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.muted)
                            .child(source_label),
                    ),
            )
            .child(if refreshing {
                div()
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().xsmall().color(t.muted))
                    .into_any_element()
            } else {
                Button::new("settings-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .text_color(t.muted)
                    .tooltip(tr!("settings.refresh_now"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(RefreshRates), cx))
                    .into_any_element()
            });
        let rates_section = section(
            tr!("settings.rates"),
            vec![
                row(
                    tr!("settings.source"),
                    choice(
                        "rate-source",
                        prefs.rate_source,
                        &[
                            (RateSource::Auto, tr!("settings.source_auto")),
                            (RateSource::Ecb, tr!("settings.source_ecb")),
                            (RateSource::Frankfurter, tr!("settings.source_frankfurter")),
                        ],
                        &app,
                        |s, v| s.rate_source = v,
                    ),
                    &t,
                ),
                row(
                    tr!("settings.refresh"),
                    choice(
                        "refresh-interval",
                        prefs.refresh_interval,
                        &[
                            (RefreshInterval::Hour, tr!("settings.every_hour")),
                            (RefreshInterval::SixHours, tr!("settings.every_6h")),
                            (RefreshInterval::TwelveHours, tr!("settings.every_12h")),
                            (RefreshInterval::Manual, tr!("settings.manually")),
                        ],
                        &app,
                        |s, v| s.refresh_interval = v,
                    ),
                    &t,
                ),
                row(
                    tr!("settings.last_updated"),
                    last_updated.into_any_element(),
                    &t,
                ),
            ],
            &t,
        );

        v_flex()
            .id("settings")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            // Clicks must not fall through to the list underneath.
            .occlude()
            .child(header)
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    // One unshrinkable column, so the scroll range covers
                    // all of it.
                    .child(
                        v_flex()
                            .flex_shrink_0()
                            .px(px(crate::layout::metrics::PAD_X))
                            .pb_4()
                            .child(appearance)
                            .child(numbers)
                            .child(rates_section)
                            .child(about(&t)),
                    ),
            )
    }
}

/// A section: a small uppercase muted title over a card of rows.
fn section(title: String, rows: Vec<AnyElement>, t: &Tokens) -> impl IntoElement {
    let count = rows.len();
    v_flex()
        .child(
            div()
                .pt(px(20.))
                .pb(px(7.))
                .pl(px(4.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.muted)
                .child(title.to_uppercase()),
        )
        .child(
            v_flex()
                .rounded(px(12.))
                .border_1()
                .border_color(t.hairline)
                .bg(t.card)
                .children(rows.into_iter().enumerate().map(|(ix, row)| {
                    v_flex()
                        .child(row)
                        // Hairlines between rows, inset like the list's.
                        .when(ix + 1 < count, |el| {
                            el.child(div().mx(px(14.)).h(px(1.)).bg(t.hairline))
                        })
                })),
        )
}

/// Label left, control right.
fn row(label: String, control: AnyElement, t: &Tokens) -> AnyElement {
    h_flex()
        .min_h(px(46.))
        .pl(px(14.))
        .pr(px(10.))
        .py(px(4.))
        .gap_3()
        .justify_between()
        .child(div().text_size(px(14.)).text_color(t.text).child(label))
        .child(control)
        .into_any_element()
}

/// A select: the current option with a caret, opening a menu of all of
/// them (gpui-kit's own setting dropdowns work the same way).
fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    current: T,
    options: &[(T, String)],
    app: &WeakEntity<KurrencyApp>,
    set: fn(&mut Settings, T),
) -> AnyElement {
    let label = options
        .iter()
        .find(|(value, _)| *value == current)
        .map(|(_, label)| label.clone())
        .unwrap_or_default();
    let options = options.to_vec();
    let app = app.clone();
    Button::new(id)
        .ghost()
        .small()
        // The ghost padding lines the caret up with the switch's edge.
        .mr(px(-6.))
        .label(label)
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            options.iter().fold(menu, |menu, (value, label)| {
                let value = *value;
                let app = app.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(value == current)
                        .on_click(move |_, window, cx| change(&app, window, cx, |s| set(s, value))),
                )
            })
        })
        .into_any_element()
}

/// System / Light / Dark.
fn theme_mode(prefs: &Settings, app: &WeakEntity<KurrencyApp>) -> AnyElement {
    const MODES: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];
    let label = |mode: ThemeMode| match mode {
        ThemeMode::System => tr!("settings.mode_system"),
        ThemeMode::Light => tr!("settings.mode_light"),
        ThemeMode::Dark => tr!("settings.mode_dark"),
    };
    let selected = MODES
        .iter()
        .position(|mode| *mode == prefs.theme_mode)
        .unwrap_or(0);
    let app = app.clone();
    TabBar::new("theme-mode")
        .segmented()
        .small()
        .w(px(196.))
        .selected_index(selected)
        .children(
            MODES
                .iter()
                .map(|mode| Tab::new().label(label(*mode)).flex_1()),
        )
        .on_click(move |ix, window, cx| {
            if let Some(mode) = MODES.get(*ix) {
                let mode = *mode;
                change(&app, window, cx, |s| s.theme_mode = mode);
            }
        })
        .into_any_element()
}

const CARD_W: f32 = 140.;
const PREVIEW_H: f32 = 127.;

/// The theme gallery: a live miniature of each theme, scrolling sideways.
/// Clicking a card picks it for its mode (light or dark).
fn gallery(prefs: &Settings, app: &WeakEntity<KurrencyApp>, cx: &App) -> impl IntoElement {
    let t = Tokens::get(cx);
    let active = if cx_is_dark(cx) {
        prefs.theme_dark
    } else {
        prefs.theme_light
    };
    div()
        .id("theme-gallery")
        .debug_selector(|| "theme-gallery".into())
        .w_full()
        .overflow_x_scroll()
        .child(
            h_flex()
                .px(px(12.))
                .pt(px(2.))
                .pb(px(12.))
                .gap(px(10.))
                .children(ThemeId::ALL.into_iter().map(|id| {
                    let chosen = id == prefs.theme_light || id == prefs.theme_dark;
                    let app_entity = app.clone();
                    v_flex()
                        .id(SharedString::from(format!("theme-{}", id.label())))
                        .debug_selector(move || format!("theme-{}", id.label().to_lowercase()))
                        .w(px(CARD_W))
                        .flex_shrink_0()
                        .gap(px(6.))
                        .cursor_pointer()
                        .child(
                            div()
                                .p(px(2.))
                                .rounded(px(12.))
                                .border_2()
                                .border_color(if id == active {
                                    t.accent
                                } else {
                                    gpui_kit::transparent_black()
                                })
                                .child(preview(id)),
                        )
                        .child(
                            h_flex()
                                .px(px(4.))
                                .gap_1()
                                .text_size(px(12.))
                                .child(div().font_weight(FontWeight::MEDIUM).child(theme_label(id)))
                                .child(div().text_color(t.muted).child(if id.is_dark() {
                                    tr!("settings.mode_dark")
                                } else {
                                    tr!("settings.mode_light")
                                }))
                                .when(chosen, |el| {
                                    el.child(div().flex_1()).child(
                                        gpui_kit::component::Icon::new(IconName::Check)
                                            .size(px(14.))
                                            .text_color(t.accent),
                                    )
                                }),
                        )
                        .on_click(move |_, window, cx| {
                            change(&app_entity, window, cx, |s| {
                                if id.is_dark() {
                                    s.theme_dark = id;
                                } else {
                                    s.theme_light = id;
                                }
                            });
                        })
                })),
        )
}

fn cx_is_dark(cx: &App) -> bool {
    Tokens::get(cx).dark
}

/// A miniature of the main window in a theme: the anchor card and two rows.
fn preview(id: ThemeId) -> impl IntoElement {
    let p = Tokens::of(id);
    let amount = |symbol: &'static str, int: &'static str, frac: &'static str, size: f32| {
        h_flex()
            .items_baseline()
            .text_size(px(size))
            .font_features(tabular_numerals())
            .whitespace_nowrap()
            .child(
                div()
                    .opacity(SYMBOL_OPACITY)
                    .mr(px(size * 0.25))
                    .child(symbol),
            )
            .child(int)
            .when(!frac.is_empty(), |el| {
                el.child(div().opacity(p.decimals).child(frac))
            })
    };
    let mini_row = |code: &'static str, symbol, int, frac, up: bool, last: bool| {
        let color = if up { p.green } else { p.red };
        v_flex()
            .child(
                h_flex()
                    .h(px(19.))
                    .px(px(3.))
                    .gap(px(4.))
                    .child(mini_flag(code, 10., p.hairline))
                    .child(
                        div()
                            .w(px(18.))
                            .flex_shrink_0()
                            .text_size(px(7.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(code),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(amount(symbol, int, frac, 9.)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(3.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(color.opacity(CHIP_TINT * 1.5))
                            .text_color(color)
                            .text_size(px(6.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(if up { "▲ 0.1%" } else { "▼ 0.3%" }),
                    ),
            )
            .when(!last, |el| {
                el.child(div().ml(px(25.)).mr(px(3.)).h(px(1.)).bg(p.hairline))
            })
    };
    // The title bar with the freshness dot, in the accent.
    let header = h_flex()
        .h(px(9.))
        .px(px(2.))
        .text_size(px(7.))
        .font_weight(FontWeight::SEMIBOLD)
        .child("Kurrency")
        .child(div().flex_1())
        .child(div().size(px(4.)).rounded_full().bg(p.accent));
    // The quick-amount chips, "100" selected as the anchor card shows it.
    let chips = h_flex()
        .gap(px(3.))
        .children(["10", "100", "1k", "10k"].map(|label| {
            let selected = label == "100";
            div()
                .flex_1()
                .h(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .border_color(if selected { p.accent } else { p.hairline })
                .when(selected && p.accent_fill > 0., |el| {
                    el.bg(p.accent.opacity(p.accent_fill))
                })
                .text_size(px(5.5))
                .text_color(if selected { p.accent } else { p.text })
                .child(label)
        }));
    v_flex()
        .w_full()
        .h(px(PREVIEW_H))
        .p(px(6.))
        .gap(px(3.))
        .rounded(px(9.))
        .overflow_hidden()
        .bg(p.bg)
        .text_color(p.text)
        .border_1()
        .border_color(p.hairline)
        .child(header)
        .child(
            v_flex()
                .px(px(6.))
                .pt(px(5.))
                .pb(px(4.))
                .gap(px(1.))
                .rounded(px(6.))
                .border_1()
                .border_color(if p.accent_fill > 0. {
                    p.accent.opacity(p.accent_fill * 2.)
                } else {
                    p.hairline
                })
                .bg(p.card)
                .child(
                    h_flex()
                        .gap(px(4.))
                        .child(mini_flag("USD", 10., p.hairline))
                        .child(
                            div()
                                .text_size(px(7.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("USD"),
                        )
                        .child(
                            div()
                                .text_size(px(6.))
                                .text_color(p.muted)
                                .child(kurrency_core::money::display_name("USD")),
                        ),
                )
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(amount("$", "100", ".00", 16.)),
                ),
        )
        .child(chips)
        .child(mini_row("EUR", "€", "89", ".17", false, false))
        .child(mini_row("JPY", "¥", "15,818", "", true, true))
}

/// The About footer.
fn about(t: &Tokens) -> impl IntoElement {
    v_flex()
        .pt(px(28.))
        .pb(px(8.))
        .gap(px(4.))
        .items_center()
        .text_size(px(11.))
        .text_color(t.muted)
        .child(img(ICON_PATH).size(px(36.)).mb(px(6.)))
        .child(div().text_size(px(12.)).text_color(t.text).child(tr!(
            "settings.about_version",
            version = env!("CARGO_PKG_VERSION")
        )))
        .child(tr!("settings.about_rates"))
        .child(tr!("settings.about_flags"))
        .child(tr!("settings.about_fonts"))
        .child(tr!("settings.about_names"))
}

/// A theme's name in the UI language (`Paper` → `纸白`).
fn theme_label(id: ThemeId) -> String {
    match id {
        ThemeId::Paper => tr!("settings.theme_paper"),
        ThemeId::Graphite => tr!("settings.theme_graphite"),
        ThemeId::Ink => tr!("settings.theme_ink"),
        ThemeId::Slate => tr!("settings.theme_slate"),
    }
}
