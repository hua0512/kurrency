//! Editing the anchor: expressions, commit and revert, type-anywhere, the
//! quick amounts and stepping.

use super::*;

#[gpui_kit::test]
fn anchor_edits_convert_commit_and_revert(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);
    assert_eq!(values(&app, cx), ["90.00", "15,000"]);

    // Clicking the formatted amount edits the raw number, all selected.
    click(cx, "anchor-amount");
    assert!(anchor_focused(&app, cx));
    assert_eq!(input_text(&app, cx), "100");

    // An expression converts live, with the echo under it.
    cx.simulate_input("1.5k*2");
    assert_eq!(amount(&app, cx), Some(d("3000")));
    assert_eq!(echo(&app, cx).as_deref(), Some("= 3,000"));
    assert_eq!(values(&app, cx), ["2,700.00", "450,000"]);

    // Enter replaces the expression with its result and keeps editing.
    cx.simulate_keystrokes("enter");
    assert_eq!(input_text(&app, cx), "3000");
    assert_eq!(echo(&app, cx), None);
    assert!(anchor_focused(&app, cx));

    // An invalid expression shows a dash; the rows keep the last valid
    // value, which was "3" one keystroke earlier.
    cx.simulate_keystrokes(&keys("ctrl-a"));
    cx.simulate_input("3*");
    assert_eq!(echo(&app, cx).as_deref(), Some("—"));
    assert_eq!(values(&app, cx), ["2.70", "450"]);

    // Esc puts back the committed value; a second Esc leaves the input.
    cx.simulate_keystrokes("escape");
    assert_eq!(amount(&app, cx), Some(d("3000")));
    assert_eq!(input_text(&app, cx), "3000");
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!anchor_focused(&app, cx));
    app.read_with(cx, |app, _| assert!(!app.is_editing()));
}

#[gpui_kit::test]
fn comma_starts_an_amount_in_comma_decimal_formats(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);
    // The default format: a comma types nothing.
    cx.simulate_keystrokes(",");
    draw(cx);
    assert!(!anchor_focused(&app, cx));

    // "Follow system" in a German locale reads `1.234,56`.
    app.update(cx, |app, _| {
        app.prefs.number_format = settings::NumberFormat::System;
        app.fmt.style = app.prefs.number_format.style(Some("de-DE"));
    });
    cx.simulate_keystrokes(",");
    draw(cx);
    assert!(anchor_focused(&app, cx));
    assert_eq!(input_text(&app, cx), ",");
    cx.simulate_input("5*3");
    assert_eq!(amount(&app, cx), Some(d("1.5")));
    assert_eq!(echo(&app, cx).as_deref(), Some("= 1,50"));
    cx.simulate_keystrokes(&keys("ctrl-a"));
    cx.simulate_input("12.5*2");
    assert_eq!(amount(&app, cx), Some(d("25")), "a point works too");
    cx.simulate_keystrokes("enter escape");
    draw(cx);
    assert!(!anchor_focused(&app, cx));

    // `1 234,56` too.
    app.update(cx, |app, _| {
        app.fmt.style = settings::NumberFormat::SpaceComma.style(None);
    });
    cx.simulate_keystrokes(",");
    draw(cx);
    cx.simulate_input("25");
    assert_eq!(amount(&app, cx), Some(d("0.25")));
}

#[gpui_kit::test]
fn type_anywhere_edits_or_searches(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    // A digit with nothing focused starts editing the anchor.
    cx.simulate_keystrokes("7");
    draw(cx);
    assert!(anchor_focused(&app, cx));
    assert_eq!(input_text(&app, cx), "7");
    assert_eq!(amount(&app, cx), Some(d("7")));
    cx.simulate_input("*3");
    assert_eq!(amount(&app, cx), Some(d("21")));
    assert_eq!(echo(&app, cx).as_deref(), Some("= 21"));

    // Esc goes back to 100; Esc again leaves; "(" starts a new edit.
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(amount(&app, cx), Some(d("100")));
    cx.simulate_keystrokes("(");
    draw(cx);
    assert_eq!(input_text(&app, cx), "(");
    cx.simulate_keystrokes("escape escape");
    draw(cx);

    // From a focused row too, a letter opens the palette with it.
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(focused_row(&app, cx).as_deref(), Some("EUR"));
    cx.simulate_keystrokes("j");
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.has_active_dialog(cx));
    });
    app.read_with(cx, |app, cx| assert_eq!(app.picker.query(cx), "j"));
    assert!(!anchor_focused(&app, cx));
    assert_eq!(amount(&app, cx), Some(d("100")));
}

#[gpui_kit::test]
fn quick_amounts_and_stepping(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    click(cx, "quick-1k");
    assert_eq!(amount(&app, cx), Some(d("1000")));
    assert_eq!(values(&app, cx), ["900.00", "150,000"]);

    // ↑/↓ step a plain number, Shift by ten.
    click(cx, "anchor-amount");
    cx.simulate_keystrokes("up");
    assert_eq!(amount(&app, cx), Some(d("1001")));
    assert_eq!(input_text(&app, cx), "1001");
    cx.simulate_keystrokes("shift-up shift-up down");
    assert_eq!(amount(&app, cx), Some(d("1020")));

    // A chip click ends the edit and sets the amount.
    click(cx, "quick-10");
    assert_eq!(amount(&app, cx), Some(d("10")));
    app.read_with(cx, |app, _| assert!(!app.is_editing()));

    // On an expression, ↓ leaves for the list instead (committing it).
    click(cx, "anchor-amount");
    assert_eq!(input_text(&app, cx), "10");
    cx.simulate_input("2k");
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(amount(&app, cx), Some(d("2000")));
    assert_eq!(focused_row(&app, cx).as_deref(), Some("EUR"));
}
