//! The palette, smart paste, and undo and redo.

use super::*;

#[gpui_kit::test]
fn palette_opens_and_adds_a_currency(cx: &mut TestAppContext) {
    let (app, cx) = open_with(cx, None, settings(&["USD"], "10"));

    cx.simulate_keystrokes(&keys("ctrl-k"));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(window.has_active_dialog(cx));
    });
    app.read_with(cx, |app, cx| {
        assert_eq!(app.picker.codes(), ["EUR", "JPY", "USD"]);
        assert_eq!(app.picker.query(cx), "");
    });

    // A name finds the currency; Enter adds it and closes the palette.
    cx.simulate_input("yen");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
    assert_eq!(codes(&app, cx), ["USD", "JPY"]);
    assert_eq!(values(&app, cx), ["1,500"]);
}

fn paste(cx: &mut VisualTestContext, text: &str) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    cx.simulate_keystrokes(&keys("ctrl-v"));
    draw(cx);
}

#[gpui_kit::test]
fn smart_paste_sets_the_currency_and_amount(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    // A price makes its currency the anchor with that amount.
    paste(cx, "€45.99");
    assert_eq!(codes(&app, cx), ["EUR", "USD", "JPY"]);
    assert_eq!(amount(&app, cx), Some(d("45.99")));
    paste(cx, "¥1,200");
    assert_eq!(codes(&app, cx), ["JPY", "USD", "EUR"]);
    assert_eq!(amount(&app, cx), Some(d("1200")));

    // A bare number sets the anchor amount.
    paste(cx, "1.234,5");
    assert_eq!(codes(&app, cx)[0], "JPY");
    assert_eq!(amount(&app, cx), Some(d("1234.5")));

    // Currencies without a rate, and text, are ignored.
    paste(cx, "300 hkd");
    paste(cx, "hello");
    assert_eq!(codes(&app, cx), ["JPY", "USD", "EUR"]);
    assert_eq!(amount(&app, cx), Some(d("1234.5")));

    // One undo step per paste.
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(amount(&app, cx), Some(d("1200")));
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(codes(&app, cx), ["EUR", "USD", "JPY"]);
    assert_eq!(amount(&app, cx), Some(d("45.99")));
}

#[gpui_kit::test]
fn smart_paste_adds_a_missing_currency(cx: &mut TestAppContext) {
    let (app, cx) = open_with(cx, None, settings(&["USD", "EUR"], "10"));
    paste(cx, "JPY 500");
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    assert_eq!(amount(&app, cx), Some(d("500")));
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(codes(&app, cx), ["USD", "EUR"]);
    assert_eq!(amount(&app, cx), Some(d("10")));
}

#[gpui_kit::test]
fn smart_paste_in_the_anchor_input(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    // All selected (as on focus): the price takes over.
    click(cx, "anchor-amount");
    paste(cx, "45,99 EUR");
    assert_eq!(codes(&app, cx)[0], "EUR");
    assert_eq!(amount(&app, cx), Some(d("45.99")));

    // A partial selection pastes text as usual.
    click(cx, "anchor-amount");
    cx.simulate_keystrokes("end");
    paste(cx, "€5");
    assert_eq!(codes(&app, cx)[0], "EUR");
    assert_eq!(input_text(&app, cx), "45.99€5");
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(amount(&app, cx), Some(d("45.99")));

    // So does a bare number, keeping the input's own undo.
    click(cx, "anchor-amount");
    paste(cx, "77");
    assert_eq!(input_text(&app, cx), "77");
    assert_eq!(amount(&app, cx), Some(d("77")));
}

#[gpui_kit::test]
fn undo_and_redo_list_and_amount_changes(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx, None);

    // An amount commit.
    click(cx, "anchor-amount");
    cx.simulate_input("250");
    cx.simulate_keystrokes("enter escape escape");
    draw(cx);
    assert_eq!(amount(&app, cx), Some(d("250")));

    // Inside the input, Ctrl+Z is the input's text undo.
    click(cx, "anchor-amount");
    cx.simulate_input("9");
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(amount(&app, cx), Some(d("250")));
    assert_eq!(codes(&app, cx), ["USD", "EUR", "JPY"]);
    cx.simulate_keystrokes("escape escape");
    draw(cx);

    // A promotion, a move and a removal.
    click(cx, "row-JPY");
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    cx.update(|_, cx| app.update(cx, |app, cx| app.move_to_top("USD", cx)));
    assert_eq!(codes(&app, cx), ["JPY", "USD", "EUR"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.remove_currency("EUR", window, cx)));
    draw(cx);
    assert_eq!(codes(&app, cx), ["JPY", "USD"]);
    assert!(
        app.read_with(cx, |app, _| app.toast.is_some()),
        "the removal toast"
    );

    // The toast's Undo puts the row back and closes the toast.
    click(cx, "toast-undo");
    assert_eq!(codes(&app, cx), ["JPY", "USD", "EUR"]);
    assert!(app.read_with(cx, |app, _| app.toast.is_none()));
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(codes(&app, cx), ["USD", "EUR", "JPY"]);
    assert_eq!(amount(&app, cx), Some(d("250")));
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(amount(&app, cx), Some(d("100")));
    // Nothing older.
    cx.simulate_keystrokes(&keys("ctrl-z"));
    assert_eq!(amount(&app, cx), Some(d("100")));

    cx.simulate_keystrokes(&keys("ctrl-shift-z ctrl-shift-z"));
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    assert_eq!(amount(&app, cx), Some(d("37500")));
    // A new change drops the redo steps.
    cx.dispatch_action(SwapAnchor);
    cx.simulate_keystrokes(&keys("ctrl-shift-z"));
    assert_eq!(codes(&app, cx), ["EUR", "JPY", "USD"]);
}

#[gpui_kit::test]
fn palette_commands(cx: &mut TestAppContext) {
    let (app, cx) = open_with(cx, None, settings(&["USD", "EUR"], "10"));
    let command = |query: &str, cx: &mut VisualTestContext| {
        app.read_with(cx, |app, cx| app.palette_command(query, cx))
    };
    // Only commands that can run: rates exist, adds are new, removals
    // are listed.
    assert_eq!(
        command("50 jpy", cx),
        Some(Command::SetAmount {
            code: "JPY".into(),
            amount: d("50")
        })
    );
    assert_eq!(command("50 hkd", cx), None);
    assert_eq!(command("+jpy", cx), Some(Command::Add("JPY".into())));
    assert_eq!(command("+eur", cx), None);
    assert_eq!(command("-jpy", cx), None);
    assert_eq!(command("-eur", cx), Some(Command::Remove("EUR".into())));

    // Typed in the palette and run with Enter.
    cx.simulate_keystrokes(&keys("ctrl-k"));
    draw(cx);
    cx.simulate_input("jpy 50");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(codes(&app, cx), ["JPY", "EUR", "USD"]);
    assert_eq!(amount(&app, cx), Some(d("50")));
    cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));

    // A plain query toggles the currency it finds.
    cx.simulate_keystrokes(&keys("ctrl-k"));
    draw(cx);
    cx.simulate_input("euro");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(codes(&app, cx), ["JPY", "USD"]);

    // eur>usd opens that history.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let command = app.palette_command("usd>jpy", cx).unwrap();
            app.run_command(command, window, cx);
        });
    });
    let view = detail(&app, cx).expect("the history page");
    view.read_with(cx, |view, _| assert_eq!(view.pair(), ("USD", "JPY")));
}
