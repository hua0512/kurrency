//! The palette's natural commands (DESIGN §2) and its currency search.
//!
//! - `50 gbp`, `gbp 50`, `€45` → set that currency to the amount (adding it);
//!   a bare number sets the anchor;
//! - `+chf` → add, `-jpy` → remove;
//! - `eur>usd`, `eur/usd` → open the history of that pair;
//! - anything else searches the currencies by code, English or Chinese name.
//!
//! Plain Rust without GPUI.

use rust_decimal::Decimal;

use crate::i18n::tr;
use crate::money;
use crate::paste;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Make `code` the anchor (adding it when missing) with this amount.
    SetAmount {
        code: String,
        amount: Decimal,
    },
    Add(String),
    Remove(String),
    /// The history page of `base` → `quote`.
    History {
        base: String,
        quote: String,
    },
}

/// What a palette query asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub command: Option<Command>,
    /// The text to search the currencies with.
    pub search: String,
}

/// Reads a palette query. `list` is the user's currencies, anchor first.
pub fn parse(query: &str, list: &[String]) -> Query {
    let query = query.trim();
    let resolve = |token: &str| paste::resolve_currency(token, list);

    if let Some(rest) = query.strip_prefix('+') {
        let rest = rest.trim();
        return Query {
            command: resolve(rest).map(Command::Add),
            search: rest.to_string(),
        };
    }
    if let Some(rest) = query.strip_prefix('-')
        && !rest.trim_start().starts_with(|c: char| c.is_ascii_digit())
    {
        let rest = rest.trim();
        return Query {
            command: resolve(rest).map(Command::Remove),
            search: rest.to_string(),
        };
    }
    if let Some((base, quote)) = query.split_once(['>', '/']) {
        let (base, quote) = (base.trim(), quote.trim());
        let command = match (resolve(base), resolve(quote)) {
            (Some(base), Some(quote)) if base != quote => Some(Command::History { base, quote }),
            _ => None,
        };
        return Query {
            command,
            search: if quote.is_empty() { base } else { quote }.to_string(),
        };
    }
    if let Some(price) = paste::parse_price(query, list) {
        let code = price.code.or_else(|| list.first().cloned());
        return Query {
            search: code.clone().unwrap_or_default(),
            command: code.map(|code| Command::SetAmount {
                code,
                amount: price.amount,
            }),
        };
    }
    // `50 g` while typing `50 gbp`: search the letters.
    let search = query
        .trim_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | ',' | ' '))
        .to_string();
    Query {
        command: None,
        search,
    }
}

/// The palette's label for a command (`Set GBP to 50`).
#[cfg(test)]
pub(crate) fn label(command: &Command) -> String {
    label_in(command, money::NumberStyle::COMMA_DOT)
}

/// [`label`] with the amount in a number style (`Set EUR to 45,9`).
pub fn label_in(command: &Command, style: money::NumberStyle) -> String {
    match command {
        Command::SetAmount { code, amount } => {
            let dp = if amount.fract().is_zero() {
                0
            } else {
                amount.normalize().scale()
            };
            tr!(
                "palette.set_amount",
                code = code,
                amount = money::format_amount_in(*amount, dp, style)
            )
        }
        Command::Add(code) => tr!("palette.add", code = code, name = money::display_name(code)),
        Command::Remove(code) => tr!(
            "palette.remove",
            code = code,
            name = money::display_name(code)
        ),
        Command::History { base, quote } => tr!("palette.history", base = base, quote = quote),
    }
}

/// The codes matching `term`, best first: the exact code, codes starting
/// with it, names with a word starting with it, then names containing it.
/// Chinese names count as names. Ties keep the order of `codes`.
///
/// An exact code also brings its relatives (`gbp` → GIP, FKP, SHP, EGP…):
/// right after it, currencies that share both its symbol and its family
/// word ("£", "Pound"), then those that share either, then the ordinary
/// name matches.
pub fn search(term: &str, codes: &[String]) -> Vec<String> {
    let term = term.trim().to_lowercase();
    if term.is_empty() {
        return codes.to_vec();
    }
    let exact = codes.iter().find(|code| code.to_lowercase() == term);
    let family = exact.map(|code| Family::of(code));
    let rank = |code: &str| -> Option<u8> {
        let lower = code.to_lowercase();
        let name = money::currency_name(code).to_lowercase();
        let zh: Vec<&str> = money::zh_names(code).collect();
        let related = family
            .as_ref()
            .map(|family| family.relation(code))
            .unwrap_or_default();
        if lower == term {
            Some(0)
        } else if related == 2 {
            Some(1)
        } else if lower.starts_with(&term) {
            Some(2)
        } else if related == 1 {
            Some(3)
        } else if name.starts_with(&term) || zh.iter().any(|n| n.starts_with(&term)) {
            Some(4)
        } else if name.split([' ', '-']).any(|word| word.starts_with(&term)) {
            Some(5)
        } else if term.chars().count() > 1
            && (name.contains(&term) || zh.iter().any(|n| n.contains(&term)))
        {
            Some(6)
        } else {
            None
        }
    };
    let mut ranked: Vec<(u8, usize, &String)> = codes
        .iter()
        .enumerate()
        .filter_map(|(ix, code)| rank(code).map(|r| (r, ix, code)))
        .collect();
    ranked.sort_unstable();
    ranked
        .into_iter()
        .map(|(_, _, code)| code.clone())
        .collect()
}

/// What makes currencies relatives: the core of their symbol (`£` in `E£`,
/// `$` in `HK$`) and the last word of their name ("Pound", "Dollar").
struct Family {
    code: String,
    symbol: Option<String>,
    word: Option<String>,
}

impl Family {
    fn of(code: &str) -> Self {
        Self {
            code: code.to_string(),
            symbol: core_symbol(code),
            word: family_word(code),
        }
    }

    /// 2 when `code` shares both the symbol and the word, 1 for one of them,
    /// 0 otherwise (and for the code itself).
    fn relation(&self, code: &str) -> u8 {
        if code == self.code {
            return 0;
        }
        let same = |a: &Option<String>, b: Option<String>| a.is_some() && *a == b;
        u8::from(same(&self.symbol, core_symbol(code)))
            + u8::from(same(&self.word, family_word(code)))
    }
}

/// The symbol without the letters that tell its countries apart (`E£` → `£`,
/// `HK$` → `$`); all-letter symbols stay whole (`kr`).
fn core_symbol(code: &str) -> Option<String> {
    let symbol = crate::present::currency_symbol(code)?;
    let core: String = symbol
        .chars()
        .filter(|c| !c.is_alphabetic() && *c != '.')
        .collect();
    let core = if core.is_empty() {
        symbol.trim_end_matches('.').to_lowercase()
    } else {
        core
    };
    (!core.is_empty()).then_some(core)
}

/// The last word of the English name: "Pound", "Dollar", "Franc".
fn family_word(code: &str) -> Option<String> {
    let name = money::currency_name(code);
    if name == code {
        return None;
    }
    name.rsplit(' ').next().map(str::to_lowercase)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn list() -> Vec<String> {
        ["USD", "EUR", "JPY"]
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn command(query: &str) -> Option<Command> {
        parse(query, &list()).command
    }

    fn set(code: &str, amount: &str) -> Option<Command> {
        Some(Command::SetAmount {
            code: code.into(),
            amount: d(amount),
        })
    }

    #[test]
    fn set_amounts() {
        assert_eq!(command("50 gbp"), set("GBP", "50"));
        assert_eq!(command("gbp 50"), set("GBP", "50"));
        assert_eq!(command("GBP50"), set("GBP", "50"));
        assert_eq!(command("€45,99"), set("EUR", "45.99"));
        assert_eq!(command("1.5k chf"), set("CHF", "1500"));
        // A bare number sets the anchor.
        assert_eq!(command("75"), set("USD", "75"));
        assert_eq!(parse("50 gbp", &list()).search, "GBP");
    }

    #[test]
    fn add_and_remove() {
        assert_eq!(command("+chf"), Some(Command::Add("CHF".into())));
        assert_eq!(command("+ CHF"), Some(Command::Add("CHF".into())));
        assert_eq!(command("-jpy"), Some(Command::Remove("JPY".into())));
        assert_eq!(command("- jpy"), Some(Command::Remove("JPY".into())));
        assert_eq!(command("+ch"), None);
        assert_eq!(parse("+ch", &list()).search, "ch");
        // "-5" is not a removal (and not a price either).
        assert_eq!(command("-5"), None);
    }

    #[test]
    fn history_pairs() {
        let pair = |base: &str, quote: &str| {
            Some(Command::History {
                base: base.into(),
                quote: quote.into(),
            })
        };
        assert_eq!(command("eur>usd"), pair("EUR", "USD"));
        assert_eq!(command("EUR/USD"), pair("EUR", "USD"));
        assert_eq!(command("gbp > jpy"), pair("GBP", "JPY"));
        assert_eq!(command("eur>eur"), None);
        assert_eq!(command("eur>"), None);
        assert_eq!(parse("eur>u", &list()).search, "u");
        assert_eq!(parse("eur>", &list()).search, "eur");
    }

    #[test]
    fn plain_queries_search() {
        let q = parse("swiss", &list());
        assert_eq!(q.command, None);
        assert_eq!(q.search, "swiss");
        assert_eq!(parse("50 g", &list()).search, "g");
        assert_eq!(
            parse("", &list()),
            Query {
                command: None,
                search: String::new()
            }
        );
    }

    #[test]
    fn labels() {
        assert_eq!(label(&set("GBP", "50").unwrap()), "Set GBP to 50");
        assert_eq!(label(&set("JPY", "1200").unwrap()), "Set JPY to 1,200");
        assert_eq!(label(&set("EUR", "45.9").unwrap()), "Set EUR to 45.9");
        assert_eq!(
            label_in(
                &set("JPY", "1200.5").unwrap(),
                money::NumberStyle::DOT_COMMA
            ),
            "Set JPY to 1.200,5"
        );
        assert_eq!(label(&Command::Add("CHF".into())), "Add CHF · Swiss Franc");
        assert_eq!(
            label(&Command::Remove("JPY".into())),
            "Remove JPY · Japanese Yen"
        );
        assert_eq!(
            label(&Command::History {
                base: "EUR".into(),
                quote: "USD".into()
            }),
            "EUR → USD history"
        );
    }

    #[test]
    fn chinese_labels() {
        use crate::i18n::{Lang, with_lang};
        with_lang(Lang::ZhCn, || {
            assert_eq!(label(&set("GBP", "50").unwrap()), "将 GBP 设为 50");
            assert_eq!(label(&Command::Add("CHF".into())), "添加 CHF · 瑞士法郎");
            assert_eq!(label(&Command::Remove("JPY".into())), "移除 JPY · 日元");
            assert_eq!(
                label(&Command::History {
                    base: "EUR".into(),
                    quote: "USD".into()
                }),
                "查看 EUR → USD 走势"
            );
        });
    }

    #[test]
    fn search_matches_both_languages_in_either_ui() {
        use crate::i18n::{Lang, with_lang};
        let codes: Vec<String> = ["CHF", "HKD", "ISK", "USD"]
            .iter()
            .map(ToString::to_string)
            .collect();
        for lang in [Lang::En, Lang::ZhCn] {
            with_lang(lang, || {
                assert_eq!(search("swiss", &codes), ["CHF"]);
                assert_eq!(search("瑞士", &codes), ["CHF"]);
                // CLDR's name and the everyday one.
                assert_eq!(search("港元", &codes), ["HKD"]);
                assert_eq!(search("港币", &codes), ["HKD"]);
                // A CLDR-only name.
                assert_eq!(search("冰岛", &codes), ["ISK"]);
            });
        }
    }

    #[test]
    fn search_ranks_codes_then_names() {
        let codes: Vec<String> = ["AUD", "CHF", "CNY", "GBP", "GIP", "HKD", "USD"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(search("", &codes), codes);
        // The exact code, then its relatives: GIP shares "£" and "Pound".
        assert_eq!(search("gbp", &codes), ["GBP", "GIP"]);
        assert_eq!(search("G", &codes), ["GBP", "GIP"]);
        // Code prefix first, then names: "Pound" in GBP and GIP.
        assert_eq!(search("pound", &codes), ["GBP", "GIP"]);
        assert_eq!(search("dollar", &codes), ["AUD", "HKD", "USD"]);
        assert_eq!(search("us", &codes), ["USD", "AUD"]);
        // Chinese names.
        assert_eq!(search("港币", &codes), ["HKD"]);
        assert_eq!(search("人民", &codes), ["CNY"]);
        assert_eq!(search("zzz", &codes), Vec::<String>::new());
    }

    #[test]
    fn an_exact_code_brings_its_family() {
        let codes: Vec<String> = [
            "AUD", "CHF", "EGP", "EUR", "FKP", "GBP", "GIP", "JPY", "LBP", "SHP", "SSP", "SYP",
            "USD",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let found = search("gbp", &codes);
        assert_eq!(found[0], "GBP");
        // Every pound, nothing else.
        let mut rest = found[1..].to_vec();
        rest.sort();
        assert_eq!(
            rest,
            ["EGP", "FKP", "GIP", "LBP", "SHP", "SSP", "SYP"],
            "{found:?}"
        );
        // "£" and "Pound" together rank above "Pound" alone.
        let pos = |code: &str| found.iter().position(|c| c == code).unwrap();
        for both in ["FKP", "GIP", "SHP"] {
            assert!(pos(both) < pos("LBP"), "{found:?}");
        }
        // "usd" brings the other dollars; a plain search does not.
        assert_eq!(search("usd", &codes), ["USD", "AUD"]);
        assert_eq!(search("u", &codes), ["USD"]);
    }
}
