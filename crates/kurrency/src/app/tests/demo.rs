//! The `KURRENCY_DEMO` tour, played step by step on the test platform: each
//! part does what it shows, and it ends as it began, so a recording loops.

use super::*;
use crate::app::demo::{Step, TOUR, expand};

/// USD 100 with EUR, JPY, GBP and CNY, as the recording starts.
fn open_tour(cx: &mut TestAppContext) -> (Entity<KurrencyApp>, &mut VisualTestContext) {
    let rates = [
        ("EUR", "0.9"),
        ("JPY", "150"),
        ("GBP", "0.75"),
        ("CNY", "7"),
    ];
    let table = RateTable {
        base: "USD".into(),
        rates: rates.iter().map(|(c, r)| (c.to_string(), d(r))).collect(),
        dates: rates
            .iter()
            .map(|(c, _)| (c.to_string(), "2026-11-06".to_string()))
            .collect(),
        source: Source::Frankfurter,
        fetched_at: kurrency_core::rates::now_secs(),
    };
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
        cx.set_http_client(Arc::new(FixtureHttp {
            offline: true,
            requests: Arc::default(),
        }));
    });
    let mut app = None;
    let settings = Settings {
        theme_mode: settings::ThemeMode::Light,
        ..settings(&["USD", "EUR", "JPY", "GBP", "CNY"], "100")
    };
    let (_root, cx) = cx.add_window_view(|window, cx| {
        let rates = cx.new(|_| RatesStore::from_table(table));
        let view = cx.new(|cx| KurrencyApp::with_state(settings, None, rates, window, cx));
        app = Some(view.clone());
        Root::new(view, window, cx)
    });
    draw(cx);
    (app.unwrap(), cx)
}

#[gpui_kit::test]
fn the_tour_shows_each_feature_and_ends_where_it_began(cx: &mut TestAppContext) {
    let (app, cx) = open_tour(cx);
    let start = app.update(cx, |app, _| {
        app.begin_demo();
        app.demo_start()
    });

    // The anchor and amount after every step, without repeats.
    let mut seen: Vec<(String, Option<Decimal>)> = Vec::new();
    let mut pages = Vec::new();
    for step in TOUR.iter().flat_map(|step| expand(*step)) {
        if let Step::Wait(ms) = step {
            cx.executor().advance_clock(Duration::from_millis(ms));
            draw(cx);
            continue;
        }
        let input = app.update_in(cx, |app, window, cx| app.demo_step(step, window, cx));
        if let Some(input) = input {
            cx.update(|window, cx| input.dispatch(window, cx));
        }
        draw(cx);
        let now = app.read_with(cx, |app, _| (app.anchor().unwrap().to_string(), app.amount));
        if seen.last() != Some(&now) {
            seen.push(now);
        }
        let page = app.read_with(cx, |app, _| match app.page.as_ref().map(|p| &p.view) {
            Some(PageView::Detail(_)) => "history",
            Some(PageView::Settings(_)) => "settings",
            None => "list",
        });
        if pages.last() != Some(&page) {
            pages.push(page);
        }
    }
    settle(cx);

    let milestones = [
        ("USD", "4500"),
        ("EUR", "4050"),
        ("JPY", "1200"),
        ("GBP", "50"),
        ("USD", "100"),
    ];
    let mut rest = seen.iter();
    for (code, amount) in milestones {
        assert!(
            rest.any(|(c, a)| c == code && *a == Some(d(amount))),
            "no {code} {amount} in order in {seen:?}"
        );
    }
    assert_eq!(pages, ["list", "history", "list", "settings", "list"]);

    // As it began: the same rows, amount and theme, nothing hovered, being
    // edited or open.
    assert_eq!(app.read_with(cx, |app, _| app.demo_start()), start);
    assert_eq!(codes(&app, cx), ["USD", "EUR", "JPY", "GBP", "CNY"]);
    app.read_with(cx, |app, _| {
        assert_eq!(app.hovered, None);
        assert!(!app.is_editing());
        assert_eq!(app.prefs.theme_mode, settings::ThemeMode::Light);
    });
    cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
    assert!(!anchor_focused(&app, cx));
    assert_eq!(focused_row(&app, cx), None);
}
