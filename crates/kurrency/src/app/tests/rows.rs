//! The list: promoting, reordering (drag and Alt+arrows), keyboard focus,
//! removing, fitting the window, and the motion states.

use super::*;

#[gpui_kit::test]
fn rows_promote_into_the_anchor(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    // Clicking a row swaps it with the anchor, keeping the equivalence.
    click(cx, "row-JPY");
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    assert_eq!(amount(&app, cx), Some(d("15000")));
    assert_eq!(values(&app, cx), ["90.00", "100.00"]);

    // The amount is rounded to the new anchor's minor units.
    cx.update(|window, cx| app.update(cx, |app, cx| app.set_amount(d("1234"), window, cx)));
    click(cx, "row-EUR");
    assert_eq!(codes(&app, cx), ["EUR", "JPY", "USD"]);
    assert_eq!(amount(&app, cx), Some(d("7.4")));

    // ⇅ swaps with the first row.
    cx.dispatch_action(SwapAnchor);
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    assert_eq!(amount(&app, cx), Some(d("1233")));

    // The change chip opens the history page instead of promoting.
    click(cx, "chip-USD");
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    let view = detail(&app, cx).expect("the chip opens the detail page");
    view.read_with(cx, |view, _| assert_eq!(view.pair(), ("JPY", "USD")));
}

fn drag_slot(app: &Entity<KurrencyApp>, cx: &mut VisualTestContext) -> Option<usize> {
    app.read_with(cx, |app, _| app.drag.as_ref().map(|d| d.slot))
}

/// Presses a row's grip and moves the pointer `dy` px, past the drag
/// threshold. Returns where the pointer is.
fn start_drag(
    cx: &mut VisualTestContext,
    grip: &'static str,
    dy: f32,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    let at = cx
        .debug_bounds(grip)
        .unwrap_or_else(|| panic!("{grip} is not on screen"))
        .center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    let to = at + gpui_kit::point(px(0.), px(dy));
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    draw(cx);
    to
}

fn drag_to(cx: &mut VisualTestContext, to: gpui_kit::Point<gpui_kit::Pixels>) {
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    draw(cx);
}

#[gpui_kit::test]
fn rows_reorder_by_dragging(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kurrency-app-drag-{}", std::process::id()));
    let path = dir.join("settings.json");
    let (app, cx) = open_saving(
        cx,
        None,
        settings(&["USD", "EUR", "JPY", "GBP"], "100"),
        Some(path.clone()),
    );
    settle(cx);
    let row = layout::metrics::ROW;

    // Lifting GBP: its slot opens where it was.
    let at = start_drag(cx, "grip-GBP", -6.);
    assert_eq!(drag_slot(&app, cx), Some(2));
    app.read_with(cx, |app, _| assert!(app.hovered.is_none()));
    // Up two rows, the others move apart around slot 0.
    drag_to(cx, at - gpui_kit::point(px(0.), px(row)));
    assert_eq!(drag_slot(&app, cx), Some(1));
    let top = at - gpui_kit::point(px(0.), px(2. * row + 20.));
    drag_to(cx, top);
    assert_eq!(drag_slot(&app, cx), Some(0));
    app.read_with(cx, |app, _| {
        assert_eq!(app.drag.as_ref().unwrap().previous, Some(1));
    });
    // Far past the ends it stays in the list.
    drag_to(cx, top - gpui_kit::point(px(0.), px(400.)));
    assert_eq!(drag_slot(&app, cx), Some(0));
    drag_to(cx, top);

    // Letting go moves it there, without promoting it or opening history.
    cx.simulate_mouse_up(top, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert_eq!(codes(&app, cx), ["USD", "GBP", "EUR", "JPY"]);
    assert_eq!(amount(&app, cx), Some(d("100")));
    assert!(detail(&app, cx).is_none());
    app.read_with(cx, |app, _| assert!(app.drag.is_none()));
    let saved = settings::load(Some(&path), None);
    assert_eq!(saved.currencies, ["USD", "GBP", "EUR", "JPY"]);
    settle(cx);

    // A drag let go over a change chip neither opens history nor
    // promotes: EUR goes back to its own slot.
    let at = start_drag(cx, "grip-EUR", 4.);
    assert_eq!(drag_slot(&app, cx), Some(1));
    let chip = cx.debug_bounds("chip-JPY").expect("JPY's chip").center();
    drag_to(cx, gpui_kit::point(chip.x, at.y + px(4.)));
    cx.simulate_mouse_up(
        gpui_kit::point(chip.x, at.y + px(4.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    draw(cx);
    assert_eq!(codes(&app, cx), ["USD", "GBP", "EUR", "JPY"]);
    assert!(detail(&app, cx).is_none());
    settle(cx);

    // A plain click on the grip (the flag) still promotes.
    click(cx, "grip-JPY");
    assert_eq!(codes(&app, cx), ["JPY", "GBP", "EUR", "USD"]);

    // The drop was one undo step.
    cx.dispatch_action(UndoChange);
    assert_eq!(codes(&app, cx), ["USD", "GBP", "EUR", "JPY"]);
    cx.dispatch_action(UndoChange);
    assert_eq!(codes(&app, cx), ["USD", "EUR", "JPY", "GBP"]);
    cx.dispatch_action(RedoChange);
    assert_eq!(codes(&app, cx), ["USD", "GBP", "EUR", "JPY"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn alt_arrows_move_the_focused_row(cx: &mut TestAppContext) {
    let (app, cx) = open_with(cx, None, settings(&["USD", "EUR", "JPY", "GBP"], "100"));
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(focused_row(&app, cx).as_deref(), Some("EUR"));

    cx.simulate_keystrokes("alt-down");
    draw(cx);
    assert_eq!(codes(&app, cx), ["USD", "JPY", "EUR", "GBP"]);
    assert_eq!(focused_row(&app, cx).as_deref(), Some("EUR"));
    cx.simulate_keystrokes("alt-down alt-down");
    draw(cx);
    assert_eq!(
        codes(&app, cx),
        ["USD", "JPY", "GBP", "EUR"],
        "stops at the end"
    );
    cx.simulate_keystrokes("alt-up alt-up alt-up alt-up");
    draw(cx);
    assert_eq!(
        codes(&app, cx),
        ["USD", "EUR", "JPY", "GBP"],
        "never into the anchor"
    );
    assert_eq!(focused_row(&app, cx).as_deref(), Some("EUR"));
    assert_eq!(amount(&app, cx), Some(d("100")));

    // Each move is one undo step.
    cx.dispatch_action(UndoChange);
    assert_eq!(codes(&app, cx), ["USD", "JPY", "EUR", "GBP"]);
}

#[gpui_kit::test]
fn keyboard_moves_between_rows(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(focused_row(&app, cx).as_deref(), Some("EUR"));
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(focused_row(&app, cx).as_deref(), Some("JPY"));

    // Ctrl+C copies the focused row's raw number.
    cx.simulate_keystrokes(&keys("ctrl-c"));
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("15000"));

    // Enter promotes it; focus stays on the slot (now USD).
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    assert_eq!(focused_row(&app, cx).as_deref(), Some("USD"));

    // ↑ from the first row reaches the anchor input.
    cx.simulate_keystrokes("up up");
    draw(cx);
    assert!(anchor_focused(&app, cx));
    assert_eq!(input_text(&app, cx), "15000");
}

#[gpui_kit::test]
fn removing_the_anchor_and_fitting_the_window(cx: &mut TestAppContext) {
    let (app, cx) = open_with(cx, None, settings(&["USD", "EUR"], "10"));
    let height =
        |cx: &mut VisualTestContext| cx.update(|window, _| f32::from(window.bounds().size.height));
    assert_eq!(height(cx), layout::fit_height(2, None));

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            for code in ["JPY", "GBP", "CNY", "CHF"] {
                app.add_currency(code, window, cx);
            }
        });
    });
    draw(cx);
    assert_eq!(height(cx), layout::fit_height(6, None));
    assert!(height(cx) > layout::metrics::MIN_HEIGHT);

    // Removing the anchor makes the next row the anchor, same value.
    cx.update(|window, cx| app.update(cx, |app, cx| app.remove_currency("USD", window, cx)));
    draw(cx);
    assert_eq!(codes(&app, cx)[0], "EUR");
    assert_eq!(amount(&app, cx), Some(d("9")));
    assert_eq!(height(cx), layout::fit_height(5, None));
}

#[gpui_kit::test]
fn motion_states_play_once(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    // Adding grows the row; removing leaves a collapsing ghost.
    cx.update(|window, cx| app.update(cx, |app, cx| app.add_currency("CNY", window, cx)));
    app.read_with(cx, |app, _| {
        assert!(app.motion.entering.contains_key("CNY"));
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.remove_currency("EUR", window, cx)));
    app.read_with(cx, |app, _| {
        assert_eq!(app.motion.leaving.len(), 1);
        assert_eq!(app.motion.leaving[0].ix, 1);
    });
    // Promoting swaps; copying flashes a check instead of a toast.
    click(cx, "row-JPY");
    app.read_with(cx, |app, _| {
        let swap = app.motion.swap.as_ref().expect("a swap");
        assert_eq!(
            (swap.promoted.as_str(), swap.demoted.as_str()),
            ("JPY", "USD")
        );
    });
    cx.simulate_keystrokes(&keys("ctrl-c"));
    app.read_with(cx, |app, _| {
        assert_eq!(
            app.motion.copied.as_ref().map(|c| c.0.as_str()),
            Some("USD")
        );
    });
    settle(cx);
    app.read_with(cx, |app, _| {
        assert!(app.motion.entering.is_empty());
        assert!(app.motion.leaving.is_empty());
        assert!(app.motion.swap.is_none());
        assert!(app.motion.copied.is_none());
    });

    // Reduced motion leaves no transient states behind.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.update(|window, cx| app.update(cx, |app, cx| app.remove_currency("CNY", window, cx)));
    app.read_with(cx, |app, _| assert!(app.motion.leaving.is_empty()));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_detail("USD", window, cx);
            app.close_page(window, cx);
        });
    });
    app.read_with(cx, |app, _| assert!(app.closing.is_none()));
}
