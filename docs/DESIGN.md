# Kurrency design

This is the design reference for Kurrency as built. Code comments cite its sections, for example
`DESIGN §2`. When behaviour changes, update this file in the same change.

Kurrency is a small, calm desktop currency converter that is quick to use, easy to trust at a glance, and quietly
polished. It has no accounts, no API keys and no telemetry, and it works offline.

---

## 1. Layout

The window is compact: 360 px wide, with its height fitted to the content. That height is capped
at the screen, with a minimum of 320×440.

```
┌────────────────────────────────────┐
│ Kurrency                  ● 2h  ⚙  │  header: freshness dot + age, settings
│ ╭────────────────────────────────╮ │
│ │ ▭ USD  US Dollar            ⇅  │ │  anchor card, the only input
│ │  $ 1,250.00                    │ │  36px, decimals dimmed
│ │  = 1,250                       │ │  expression echo while editing
│ ╰────────────────────────────────╯ │
│   10   100   1k   10k              │  quick amounts
│  ▭ EUR   € 1,119.50      ▾0.3%     │  read-only rows, 56px
│  ▭ JPY   ¥ 197,875       ▴0.1%     │  daily change chip
│  ＋ Add            type to search  │
└────────────────────────────────────┘
```

- **Anchor card.** It holds the source currency: clickable flag, code and name (opening the palette
  in source-currency mode), a ⇅ swap with the first row, and the only amount input. While editing,
  the muted currency symbol stays pinned beside the input. Below the amount sits the unit rate
  against the first row when idle (`1 USD = 0.8914 EUR`), morphing into the expression echo
  (`= 4,500`) while editing.
- **Quick amounts.** A subtle segmented strip under the anchor card, scaled automatically to the
  anchor currency's magnitude (`10 / 100 / 1k / 10k` for USD/EUR, `1k / 10k / 100k / 1M` for
  JPY/KRW, `10k / 100k / 1M / 10M` for VND/IDR).
- **Rows.** They're read-only, with right-aligned tabular amounts and a permanent two-line left
  column (code over currency name) so hovering never shifts text vertically. Hovering swaps the
  subtitle to the inverse rate (`1 EUR = 1.1219 USD`) and reveals a one-click copy icon beside the
  amount. Clicking a row promotes it into the anchor, keeping the same value rounded to that
  currency's minor units. Clicking its change chip opens history (§6).
- **Reorder.** Rows reorder by dragging their ⋮⋮ grip (the row lifts and the others spring apart),
  or with Alt+↑/↓. Removing a row is in the context menu (Copy, Move to top, Remove).
- **Bottom toast.** Replaces the "+ Add" row for undo ("JPY removed · Undo") and clipboard price
  suggestions ("Convert ¥1,200 from clipboard? · Convert").
- **Empty state.** It shows "Add a currency to start".

## 2. Input

- **Expressions.** The anchor accepts `+ − × ÷`, percentages (`100+8.5%`, `200-15%`, `50%`),
  parentheses and `k`/`m` suffixes (`1.5k*3`). Enter commits the expression (or shakes the card
  and reverts when invalid); Esc reverts it, and a second Esc leaves the input. ↑/↓ step a plain
  number (Shift multiplies the step by 10). In comma-decimal formats, `,` is the decimal mark.
- **Type anywhere.** With nothing focused, typing a digit, `.`, `,`, `(` or `-` starts editing the
  anchor. Typing a letter opens the palette with that letter already filled in.
- **Smart paste & clipboard hint.** Pasting `€45.99`, `¥1,200`, `1.234,56 €`, `HK$300` or
  `300 hkd` sets that currency as the anchor (adding it if needed) and fills in the amount. An
  ambiguous symbol prefers a currency already in the list. Returning to the window with a new
  currency-tagged price on the clipboard offers a one-click convert toast.
- **Palette (Ctrl+K).** Empty search lists pinned and popular currencies first. It searches by
  code, English name, Chinese name and symbol family, and confirms a listed currency by promoting
  it to the anchor (rather than deleting it). It also takes commands:
  - `50 gbp` sets an amount (with a live `≈ …` conversion preview right in the command row);
  - `+chf` adds a currency and `-jpy` removes one;
  - `eur>usd` opens history.

  A command shows only when it can run.
- **Undo.** Ctrl+Z and Ctrl+Shift+Z cover amounts, anchor changes, adds, removes and reorders,
  50 steps deep.

## 3. Trust and freshness

- **Freshness pill.** The header pill combines freshness status and manual refresh:
  - slate dot: under 12 h old (morphs into a refresh icon on hover);
  - amber dot: stale;
  - hollow ring: offline or updating.

  Next to it is the relative age ("2h"). Its tooltip shows the source, the date, the next refresh
  or the error.
- **Daily change.** Each row's chip compares today's rate with the previous day's, showing a
  muted `0.0%` pill when unchanged or `·` when the previous day is unknown, with hover feedback.
- **Copy.** Copying a row (hover copy button, context menu, or Ctrl+C) puts the raw number on the
  clipboard, and a ✓ flashes in place of the row's chip.

## 4. Motion

All motion is subtle, under 250 ms and interruptible:
- digits roll when they change;
- the promoted row slides into the anchor;
- the anchor card shakes lightly when committing an invalid expression;
- rows fade in and out when added or removed;
- pages slide in and out;
- the freshness dot pulses once when an update starts.

`reduce_motion` in Settings turns all of it off. GPUI can't read the OS setting.

## 5. Visual system

- **Brand.** Premium and minimal: near-monochrome, with one restrained accent (Kurrency Slate)
  used for focus and selection.
- **Typography.** Text uses the bundled Inter, with a Noto Sans SC subset (`Kurrency Sans SC`)
  as the CJK fallback. On Windows, where GPUI can only fall back to system fonts, the first
  installed system CJK font in `assets::CJK_FALLBACKS` draws instead. Amounts use tabular
  numerals: 36px in the anchor and 18px in rows. An amount too long for its space (a large
  IRR or VND figure) shrinks to fit, down to half that size, and is never cut off mid-number.
  Decimals are at 55% opacity (75% in Ink) and currency symbols are muted.
- **Shapes and sizes.**
  - Cards have radius 12 and chips radius 6.
  - Flags are 20×15 with radius 2.5 and a hairline ring.
  - Rows are 56px, with hairlines inset to the text.
- **Themes.** All four meet WCAG AA contrast for text, and at least 3:1 for muted text.
  - **Paper (light):** the default light theme.
  - **Slate (light):** a cool blue-grey tint.
  - **Graphite (dark):** the default dark theme.
  - **Ink (dark):** high-contrast true black.
- **Up/down colours.** They follow the UI language: green for up in English, red for up in
  Chinese. Settings can override this.
- **Identity.** The icon is a graphite tile with an off-white "K" whose arms are opposing
  arrows. The source is `assets/icon.svg`, and `scripts/gen-icons.sh` generates the
  platform icons from it.

## 6. History page

- **Opening it.** It slides in from the change chip, from Ctrl+Enter or from the palette, and
  shows the row's currency against the anchor. It reuses the single top TitleBar with back, both
  pair flags (`USD / EUR`, meaning 1 USD = x EUR), and ⇄ to flip the pair.
- **Content.**
  - The current rate, with the change over the selected range on the line above it and the user's
    current anchor amount converted underneath.
  - A trend-coloured area chart with subtle High/Low reference lines and ranges 7D / 30D / 1Y / 5Y /
    MAX. Scrubbing updates the top readout (including "your amount then" and difference vs today)
    without a floating bubble obscuring the curve; dragging across two dates measures the span's
    change. The chart takes the height the page has left, from 100px to 260px.
  - A range position bar showing where today's rate sits between the range's Low and High, above
    High, Low (with dates) and Average side by side.
- **Data.** History is cached per pair, and only missing days are fetched. Offline with no cache,
  the page says "History unavailable offline".

## 7. Settings

The settings page slides in from the gear or from Ctrl+, and has three sections:
- **Appearance:**
  - the theme gallery, and System / Light / Dark;
  - up/down colours;
  - reduce motion;
  - language (System / English / 简体中文).
- **Numbers:**
  - number format (follow system, 1,234.56, 1.234,56 or 1 234,56);
  - decimal places (ISO minor units, or 2 extra).
- **Rates:**
  - source: Auto, ECB official or Frankfurter blended;
  - refresh every 1h / 6h / 12h, or manually;
  - last updated, with a refresh button.

An About footer gives the version and credits. Settings apply live. The window size is remembered;
so is its position, except on Wayland, which can't report it.

## 8. Data

- **Providers.** Auto tries them in order:
  - Frankfurter v2;
  - fawazahmed0/exchange-api, from its Cloudflare mirror then its jsDelivr mirror;
  - the ECB daily XML.

  None of them needs a key.
- **Rates.** These are daily mid-market reference rates, not what a bank charges.
- **Caching.**
  - The cache loads straight away at startup, and a background refresh runs according to the
    refresh interval.
  - A failed refresh keeps the old table.
  - The first run offline uses a rates snapshot bundled at build time.
- **Self-contained.** Fonts, flags and icons are embedded in the binary. Users never need to
  install anything. (GPUI on Windows can only fall back to installed fonts, so the Chinese UI
  needs a system CJK font there, which a Chinese Windows has. See §5.) Symbols the bundled
  fonts can't draw fall back to the ISO code (`AED 1,250.00`).

## 9. Keyboard

| Key | Action |
|---|---|
| Digits / letters | Edit the amount / search currencies |
| ↑ ↓ | Move between rows (in the anchor: step the amount) |
| Enter | Commit the expression · promote the focused row |
| Esc | Revert the edit · close a page or the palette |
| Ctrl+K | Palette |
| Ctrl+Enter, Ctrl+→ | Open the focused row's history |
| Alt+↑ / Alt+↓ | Move the focused row |
| Ctrl+C | Copy the focused row's value |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Ctrl+R | Refresh rates |
| Ctrl+, | Settings |

On macOS, Cmd replaces Ctrl.

## Not yet built

- Traditional Chinese.
- Always-on-top window pin (pending runtime window-level support in GPUI).
