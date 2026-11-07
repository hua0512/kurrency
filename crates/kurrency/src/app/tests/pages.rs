//! The history and settings pages, and switching the language live.

use super::*;

#[gpui_kit::test]
fn detail_opens_from_the_keyboard_and_esc_restores_focus(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    cx.simulate_keystrokes("down");
    draw(cx);
    cx.simulate_keystrokes(&keys("ctrl-enter"));
    cx.run_until_parked();
    let view = detail(&app, cx).expect("Ctrl+Enter opens the detail page");
    view.read_with(cx, |view, _| {
        assert_eq!(view.pair(), ("USD", "EUR"));
        assert_eq!(view.range(), history::Range::D30);
        // No network and no cache.
        assert!(!view.is_loading());
        assert_eq!(view.error(), Some(&HistoryError::Offline));
        assert!(view.stats().is_none());
    });

    // Type-anywhere stays out of the detail page.
    draw(cx);
    cx.simulate_keystrokes("5");
    assert_eq!(amount(&app, cx), Some(d("100")));

    // Esc goes back with focus on EUR again; the page slides out.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(detail(&app, cx).is_none());
    assert_eq!(focused_row(&app, cx).as_deref(), Some("EUR"));
    app.read_with(cx, |app, _| assert!(app.closing.is_some()));
    // The page sliding out is inert: the row under it takes the click.
    click(cx, "row-JPY");
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(codes(&app, cx), ["USD", "EUR", "JPY"]);
    settle(cx);
    app.read_with(cx, |app, _| assert!(app.closing.is_none()));

    // Ctrl+Right in the anchor compares it with the first row.
    click(cx, "anchor-amount");
    cx.simulate_keystrokes(&keys("ctrl-right"));
    let view = detail(&app, cx).expect("Ctrl+Right opens the detail page");
    view.read_with(cx, |view, _| assert_eq!(view.pair(), ("EUR", "USD")));

    // The chip's action opens a row's page.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.close_page(window, cx));
        window.dispatch_action(Box::new(OpenRowDetail { code: "JPY".into() }), cx);
    });
    let view = detail(&app, cx).expect("the chip action opens the detail page");
    view.read_with(cx, |view, _| assert_eq!(view.pair(), ("USD", "JPY")));
}

#[gpui_kit::test]
fn detail_loads_history_and_reuses_it(cx: &mut TestAppContext) {
    let requests = Arc::new(AtomicUsize::new(0));
    let (app, cx) = open_app(cx, Some(requests.clone()));
    let count = || requests.load(Ordering::SeqCst);

    cx.update(|window, cx| app.update(cx, |app, cx| app.open_detail("EUR", window, cx)));
    cx.run_until_parked();
    let view = detail(&app, cx).unwrap();
    view.read_with(cx, |view, _| {
        assert!(view.error().is_none());
        let stats = view.stats().expect("stats for 30D");
        assert_eq!(stats.high.rate, d("0.909"));
        assert_eq!(stats.low.rate, d("0.9"));
        assert!(view.chart_len() >= 30);
    });
    assert_eq!(count(), 1);

    // A shorter range is already cached and its tail is fresh.
    view.update(cx, |view, cx| view.set_range(history::Range::D7, cx));
    cx.run_until_parked();
    assert_eq!(count(), 1);
    view.read_with(cx, |view, _| assert!(view.chart_len() <= 8));

    // A longer one fetches only the missing head, and is downsampled.
    view.update(cx, |view, cx| view.set_range(history::Range::Y1, cx));
    cx.run_until_parked();
    assert_eq!(count(), 2);
    view.read_with(cx, |view, _| {
        assert_eq!(view.chart_len(), history::CHART_POINTS);
    });

    // Flipping loads the other direction.
    view.update(cx, ui::detail::DetailView::flip);
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.pair(), ("EUR", "USD"));
        assert!(view.stats().unwrap().low.rate >= d("1.1"));
    });
    assert_eq!(count(), 3);
}

fn settings_open(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> bool {
    app.read_with(cx, |app, _| {
        matches!(
            app.page.as_ref().map(|p| &p.view),
            Some(PageView::Settings(_))
        )
    })
}

#[gpui_kit::test]
fn settings_page_applies_and_saves(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kurrency-app-settings-{}", std::process::id()));
    let path = dir.join("settings.json");
    let (app, cx) = open_saving(
        cx,
        None,
        settings(&["USD", "EUR", "JPY"], "100"),
        Some(path.clone()),
    );

    // Ctrl+, opens the page; type-anywhere stays out of it.
    cx.simulate_keystrokes(&keys("ctrl-,"));
    settle(cx);
    assert!(settings_open(&app, cx));
    cx.simulate_keystrokes("5");
    assert_eq!(amount(&app, cx), Some(d("100")));

    // A gallery card sets the slot of its own mode.
    click(cx, "theme-ink");
    click(cx, "theme-slate");
    app.read_with(cx, |app, _| {
        assert_eq!(app.prefs.theme_dark, settings::ThemeId::Ink);
        assert_eq!(app.prefs.theme_light, settings::ThemeId::Slate);
    });

    // Number format and decimal places apply at once.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.update_prefs(window, cx, |s| {
                s.number_format = settings::NumberFormat::DotComma;
                s.decimal_places = settings::DecimalPlaces::ExtraTwo;
            });
        });
    });
    let shown = |code: &'static str, cx: &mut VisualTestContext| {
        app.read_with(cx, |app, cx| {
            app.fmt.amount(code, app.value_of(code, cx).unwrap())
        })
    };
    assert_eq!(shown("EUR", cx), "90,0000");
    assert_eq!(shown("JPY", cx), "15.000,00");
    let saved = std::fs::read_to_string(&path).unwrap();
    for value in ["\"dot_comma\"", "\"extra_two\"", "\"ink\"", "\"slate\""] {
        assert!(saved.contains(value), "{value} in {saved}");
    }

    // Esc goes back to the list; the page slides out.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!settings_open(&app, cx));
    app.read_with(cx, |app, _| assert!(app.closing.is_some()));
    settle(cx);

    // The anchor reads and writes the chosen format.
    click(cx, "anchor-amount");
    assert_eq!(input_text(&app, cx), "100");
    cx.simulate_input("12,5*2");
    assert_eq!(amount(&app, cx), Some(d("25")));
    assert_eq!(echo(&app, cx).as_deref(), Some("= 25"));
    cx.simulate_keystrokes("enter");
    assert_eq!(input_text(&app, cx), "25");
    cx.simulate_keystrokes("escape");
    draw(cx);
    click(cx, "anchor-amount");
    cx.simulate_input("1,25");
    cx.simulate_keystrokes("enter");
    assert_eq!(amount(&app, cx), Some(d("1.25")));
    assert_eq!(input_text(&app, cx), "1,25");
    assert_eq!(
        app.read_with(cx, |app, cx| app.raw_value("JPY", cx))
            .as_deref(),
        Some("187,5")
    );

    // The settings round-trip through the file.
    let loaded = settings::load(Some(&path), None);
    assert_eq!(loaded.number_format, settings::NumberFormat::DotComma);
    assert_eq!(loaded.amount.as_deref(), Some("1.25"));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn the_language_applies_live(cx: &mut TestAppContext) {
    use kurrency_core::i18n::{Lang, lang};
    use settings::{Language, UpDownColors};
    let (app, cx) = open_app(cx, None);
    let set = |cx: &mut VisualTestContext, f: fn(&mut Settings)| {
        cx.update(|window, cx| app.update(cx, |app, cx| app.update_prefs(window, cx, f)));
        draw(cx);
    };
    assert_eq!(lang(), Lang::En);
    assert!(app.read_with(cx, |app, _| app.green_up()));

    // Chinese at once, with red for up (by language), on every page.
    set(cx, |s| s.language = Language::ChineseSimplified);
    assert_eq!(lang(), Lang::ZhCn);
    assert!(!app.read_with(cx, |app, _| app.green_up()));
    cx.update(|window, cx| app.update(cx, |app, cx| app.remove_currency("JPY", window, cx)));
    draw(cx);
    let toast = app.read_with(cx, |app, _| app.toast.clone().unwrap().message);
    assert_eq!(toast.as_ref(), "已移除 JPY");
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_settings_page(window, cx)));
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_detail("EUR", window, cx)));
    draw(cx);

    // An explicit colour choice wins over the language.
    set(cx, |s| s.up_down_colors = UpDownColors::GreenUp);
    assert!(app.read_with(cx, |app, _| app.green_up()));

    // Back to English; "System" follows the system locale.
    set(cx, |s| {
        s.language = Language::English;
        s.up_down_colors = UpDownColors::ByLanguage;
    });
    assert_eq!(lang(), Lang::En);
    assert!(app.read_with(cx, |app, _| app.green_up()));
    app.update(cx, |app, _| app.locale = Some("zh-CN".into()));
    set(cx, |s| s.language = Language::System);
    assert_eq!(lang(), Lang::ZhCn);
    app.update(cx, |app, _| app.locale = Some("zh-TW".into()));
    set(cx, |s| s.language = Language::English);
    set(cx, |s| s.language = Language::System);
    assert_eq!(lang(), Lang::En);
}
