//! The app's side of the UI languages (the strings and the language switch
//! are `kurrency_core::i18n`): gpui-kit's own strings follow the locale files
//! too, and the tests that check the views and fonts against those files.

/// Lets the `gpui_component.*` keys in our locale files override gpui-kit's
/// built-in strings. Once per process.
pub(crate) fn init() {
    static EXTEND: std::sync::Once = std::sync::Once::new();
    EXTEND.call_once(|| {
        // What `rust_i18n::extend!(gpui_component)` expands to, given the
        // table of the crate that compiles the locale files.
        gpui_kit::component::_rust_i18n_extend(kurrency_core::i18n::backend(), "gpui_component");
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use kurrency_core::i18n::backend;
    use kurrency_core::money;

    use super::*;

    fn keys(locale: &str) -> BTreeSet<String> {
        backend()
            .messages_for_locale(locale)
            .unwrap_or_else(|| panic!("no {locale} locale"))
            .into_iter()
            .map(|(key, _)| key.into_owned())
            .collect()
    }

    #[test]
    fn kit_strings_are_extended() {
        init();
        init();
        use gpui_kit::component as gpui_component;
        assert_eq!(
            gpui_component::_rust_i18n_translate("zh-CN", "Command.empty"),
            "没有匹配的货币"
        );
        assert_eq!(
            gpui_component::_rust_i18n_translate("en", "Command.empty"),
            "No matching currencies"
        );
        // Everything else stays the kit's.
        assert_eq!(
            gpui_component::_rust_i18n_translate("zh-CN", "Input.Copy"),
            "复制"
        );
    }

    /// The views hand no English literal to an element: every label,
    /// tooltip, title and placeholder comes from the locale files.
    #[test]
    fn no_hard_coded_english_in_the_views() {
        const FILES: &[(&str, &str)] = &[
            ("app.rs", include_str!("app.rs")),
            ("app/actions.rs", include_str!("app/actions.rs")),
            ("app/changes.rs", include_str!("app/changes.rs")),
            ("app/commands.rs", include_str!("app/commands.rs")),
            ("app/debug_env.rs", include_str!("app/debug_env.rs")),
            ("app/editing.rs", include_str!("app/editing.rs")),
            ("app/pages.rs", include_str!("app/pages.rs")),
            ("app/prefs.rs", include_str!("app/prefs.rs")),
            ("app/render.rs", include_str!("app/render.rs")),
            ("app/rows.rs", include_str!("app/rows.rs")),
            ("app/window.rs", include_str!("app/window.rs")),
            ("ui/anchor.rs", include_str!("ui/anchor.rs")),
            ("ui/detail.rs", include_str!("ui/detail.rs")),
            ("ui/header.rs", include_str!("ui/header.rs")),
            ("ui/picker.rs", include_str!("ui/picker.rs")),
            ("ui/row.rs", include_str!("ui/row.rs")),
            ("ui/settings.rs", include_str!("ui/settings.rs")),
            ("ui/toast.rs", include_str!("ui/toast.rs")),
        ];
        const CALLS: &[&str] = &[
            ".child(\"",
            ".children([\"",
            ".tooltip(\"",
            ".label(\"",
            ".title(\"",
            ".placeholder(\"",
            "PopupMenuItem::new(\"",
            "Tooltip::new(\"",
            "Notification::warning(\"",
            "Notification::error(\"",
        ];
        // The brand, and currency codes (always Latin).
        let allowed = |text: &str| {
            text == "Kurrency" || (text.len() == 3 && text.chars().all(|c| c.is_ascii_uppercase()))
        };
        let english = |text: &str| {
            let mut run = 0;
            text.chars().any(|c| {
                run = if c.is_ascii_alphabetic() { run + 1 } else { 0 };
                run >= 2
            })
        };
        let mut found = Vec::new();
        for (file, source) in FILES {
            // Windows checkouts may use CRLF; compare on LF.
            let source = source.replace("\r\n", "\n");
            let code = source.split("#[cfg(test)]\nmod tests").next().unwrap();
            for (ix, line) in code.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for call in CALLS {
                    for (at, _) in line.match_indices(call) {
                        let rest = &line[at + call.len()..];
                        let text = rest.split('"').next().unwrap_or_default();
                        if english(text) && !allowed(text) {
                            found.push(format!("{file}:{}: {text:?}", ix + 1));
                        }
                    }
                }
                // Any other literal that reads as prose ("Click for
                // details", "Dark"), except ids, key contexts and logs.
                let plumbing = [
                    "KeyBinding",
                    "debug_selector",
                    "key_context",
                    "const ",
                    "log::",
                    "expect(",
                    ".id(",
                    "tr!(",
                    "cfg!(",
                    "#[",
                ];
                if plumbing.iter().any(|p| line.contains(p)) {
                    continue;
                }
                for text in line.split('"').skip(1).step_by(2) {
                    let words: Vec<&str> = text.split_whitespace().collect();
                    let word = |w: &&str| w.len() > 1 && w.chars().all(|c| c.is_ascii_alphabetic());
                    let capitalised = words.len() == 1
                        && words[0].len() > 2
                        && words[0].starts_with(|c: char| c.is_ascii_uppercase())
                        && words[0][1..].chars().all(|c| c.is_ascii_lowercase());
                    let prose = words.windows(2).any(|pair| pair.iter().all(word));
                    if (prose || capitalised) && !allowed(text) {
                        found.push(format!("{file}:{}: {text:?}", ix + 1));
                    }
                }
            }
        }
        assert!(found.is_empty(), "hard-coded strings: {found:#?}");
    }

    /// The bundled CJK subset covers every character the zh-CN UI can draw:
    /// the locale file and the currency names.
    #[test]
    fn the_cjk_subset_covers_the_chinese_ui() {
        let covered = crate::assets::cjk_coverage();
        let mut texts: Vec<String> = keys("zh-CN")
            .into_iter()
            .map(|key| backend().translate("zh-CN", &key).unwrap().into_owned())
            .collect();
        texts.extend(money::all_zh_names().map(str::to_string));
        let mut missing = BTreeSet::new();
        for text in &texts {
            for ch in text.chars() {
                let cjk = matches!(ch as u32, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF);
                if cjk && !covered.contains(&ch) {
                    missing.insert(ch);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "run scripts/subset-cjk-font.py; missing {missing:?}"
        );
    }
}
