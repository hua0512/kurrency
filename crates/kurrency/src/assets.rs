//! The app's asset source: gpui-kit's icons plus the bundled flags
//! (`assets/flags/`, lipis/flag-icons 4x3, MIT), all compiled into the binary,
//! and the bundled Inter faces with their CJK fallback (`assets/fonts/`, SIL
//! OFL 1.1), with which characters they can draw.
//!
//! `build.rs` compresses both: the flags are gzipped SVG (SVGZ), which
//! GPUI's `img()` hands to resvg as it is, and the fonts are deflated and
//! inflated here, once, when the text system loads them at startup.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io::Read as _;
use std::sync::{LazyLock, OnceLock};

use gpui_kit::{AssetSource, Result, SharedString};

include!(concat!(env!("OUT_DIR"), "/flags.rs"));

/// Embeds a file from the repository's top-level `assets/` directory (which
/// the packaging and the docs use too), by its path in there.
macro_rules! embed {
    (bytes: $path:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/", $path))
    };
    (str: $path:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/", $path))
    };
}
pub(crate) use embed;

/// Embeds a font from `assets/fonts/` as `build.rs` deflated it.
macro_rules! deflated_font {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".deflate"))
    };
}

/// Inflates a font that `build.rs` deflated.
fn inflate(data: &[u8]) -> Vec<u8> {
    let mut font = Vec::new();
    flate2::read::DeflateDecoder::new(data)
        .read_to_end(&mut font)
        .expect("a deflated font");
    font
}

/// Inter 4.1 (rsms/inter, SIL OFL 1.1, see `assets/fonts/OFL.txt`): the
/// static faces for the typography table's weights 400 (and 450), 500 and
/// 600. Static faces, because GPUI draws a variable font at one weight.
/// `scripts/subset-inter-font.py` cuts them down to Latin, the symbols and
/// every character of the locales, keeping the hinting and layout features.
pub(crate) static FONTS: LazyLock<[Vec<u8>; 3]> = LazyLock::new(|| {
    [
        inflate(deflated_font!("Inter-Regular.ttf")),
        inflate(deflated_font!("Inter-Medium.ttf")),
        inflate(deflated_font!("Inter-SemiBold.ttf")),
    ]
});

/// The family name of [`FONTS`].
pub(crate) const FONT_FAMILY: &str = "Inter";

/// Noto Sans SC 2.004 (SIL OFL 1.1, see `assets/fonts/NotoSansSC-OFL.txt`),
/// cut down by `scripts/subset-cjk-font.py` to the characters the Chinese UI
/// uses: the fallback for Inter, which has no CJK glyphs, so the app never
/// depends on system fonts. Regular and Medium; weight 600 draws Medium.
pub(crate) static CJK_FONTS: LazyLock<[Vec<u8>; 2]> = LazyLock::new(|| {
    [
        inflate(deflated_font!("NotoSansSC-Regular.subset.otf")),
        inflate(deflated_font!("NotoSansSC-Medium.subset.otf")),
    ]
});

/// The family name of [`CJK_FONTS`].
pub(crate) const CJK_FONT_FAMILY: &str = "Noto Sans SC";

/// Inter, falling back to [`CJK_FONT_FAMILY`] for the characters it lacks.
pub(crate) fn font_fallbacks() -> gpui_kit::FontFallbacks {
    gpui_kit::FontFallbacks::from_fonts(vec![CJK_FONT_FAMILY.to_string()])
}

/// Sets [`font_fallbacks`] on an element and its children. Views drawn
/// outside the app's root (the palette dialog) set it too; kit tooltips and
/// menus use the text system's own fallback, which finds the same faces.
pub(crate) fn with_cjk_fallback<E: gpui_kit::Styled>(mut element: E) -> E {
    element.text_style().font_fallbacks = Some(font_fallbacks());
    element
}

/// The characters [`CJK_FONTS`] can draw (every face has the same set).
#[cfg(test)]
pub(crate) fn cjk_coverage() -> BTreeSet<char> {
    coverage(&CJK_FONTS[0])
}

/// Whether the bundled fonts draw `ch`: every Inter face, or every face of
/// the CJK fallback, has a glyph for it. Anything else would come from a
/// system font, which may not be there. Read from the fonts' character maps
/// once, on first use.
pub(crate) fn has_glyph(ch: char) -> bool {
    static DRAWABLE: OnceLock<BTreeSet<char>> = OnceLock::new();
    DRAWABLE
        .get_or_init(|| {
            let all = |faces: &[Vec<u8>]| {
                faces
                    .iter()
                    .map(|face| coverage(face))
                    .reduce(|a, b| &a & &b)
                    .unwrap_or_default()
            };
            let mut chars = all(&*FONTS);
            chars.extend(all(&*CJK_FONTS));
            chars
        })
        .contains(&ch)
}

/// The characters a font has a glyph for.
fn coverage(font: &[u8]) -> BTreeSet<char> {
    let face = ttf_parser::Face::parse(font, 0).expect("a bundled font");
    let mut chars = BTreeSet::new();
    for table in face.tables().cmap.expect("cmap").subtables {
        if table.is_unicode() {
            table.codepoints(|c| {
                if let Some(ch) = char::from_u32(c)
                    && table.glyph_index(c).is_some_and(|g| g.0 != 0)
                {
                    chars.insert(ch);
                }
            });
        }
    }
    chars
}

/// Asset paths of the flags: `flags/<region>.svg`.
const FLAGS_PREFIX: &str = "flags/";

/// The app icon (`assets/icon.svg`), shown in Settings → About.
const ICON: (&str, &[u8]) = (crate::ui::settings::ICON_PATH, embed!(bytes: "icon.svg"));

pub(crate) struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path.strip_prefix(FLAGS_PREFIX) {
            let data = name.strip_suffix(".svg").and_then(flag_svg);
            return Ok(data.map(Cow::Borrowed));
        }
        if path == ICON.0 {
            return Ok(Some(Cow::Borrowed(ICON.1)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut list = gpui_kit::assets::Assets.list(path)?;
        list.extend(
            FLAGS
                .iter()
                .map(|(region, _)| format!("{FLAGS_PREFIX}{region}.svg"))
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        Ok(list)
    }
}

/// The embedded SVG of a region's flag (`"us"`, `"eu"`).
fn flag_svg(region: &str) -> Option<&'static [u8]> {
    FLAGS
        .binary_search_by(|(name, _)| (*name).cmp(region))
        .ok()
        .map(|ix| FLAGS[ix].1)
}

/// The asset path of the currency's flag, or `None` when it has no bundled
/// flag and shows the neutral badge.
pub(crate) fn flag_path(code: &str) -> Option<SharedString> {
    let region = kurrency_core::money::flag_region(code)?;
    flag_svg(&region).map(|_| format!("{FLAGS_PREFIX}{region}.svg").into())
}

#[cfg(test)]
mod tests {
    use kurrency_core::{money, present};

    use super::*;

    /// A flag's SVG text, from its embedded SVGZ.
    fn gunzip(data: &[u8]) -> Vec<u8> {
        assert!(data.starts_with(&[0x1f, 0x8b]), "not gzipped");
        let mut svg = Vec::new();
        flate2::read::GzDecoder::new(data)
            .read_to_end(&mut svg)
            .unwrap();
        svg
    }

    /// Every code a row or the palette can hold: rows keep any three
    /// uppercase letters from settings, the palette lists ISO codes.
    fn all_codes() -> impl Iterator<Item = String> {
        let letters = || b'A'..=b'Z';
        letters().flat_map(move |a| {
            letters().flat_map(move |b| {
                letters().map(move |c| String::from_utf8(vec![a, b, c]).unwrap())
            })
        })
    }

    #[test]
    fn every_code_has_a_flag_or_the_fallback() {
        for code in all_codes() {
            match flag_path(&code) {
                Some(path) => {
                    let data = Assets.load(&path).unwrap().expect("embedded flag");
                    assert!(gunzip(&data).starts_with(b"<svg"), "{path}");
                }
                None => {
                    let badge = money::flag_badge(&code);
                    assert!(badge == "¤" || badge == code[..2], "{code}: {badge}");
                }
            }
        }
    }

    #[test]
    fn listed_currencies_have_real_flags() {
        // Supranational currencies with no shared flag show the badge.
        let badged = ["XAF", "XOF", "XCD", "XPF"];
        let mut flags = 0;
        for code in all_codes().filter(|code| money::is_regular_currency(code)) {
            if badged.contains(&code.as_str()) {
                assert_eq!(flag_path(&code), None, "{code}");
                assert_eq!(money::flag_badge(&code), "¤");
            } else {
                assert!(flag_path(&code).is_some(), "{code} has no flag");
                flags += 1;
            }
        }
        assert!(flags > 140, "{flags}");
    }

    #[test]
    fn every_embedded_flag_is_used_and_sized() {
        assert!(FLAGS.is_sorted_by_key(|(name, _)| *name));
        let used: std::collections::HashSet<String> = all_codes()
            .filter_map(|code| money::flag_region(&code))
            .collect();
        for (region, data) in FLAGS {
            assert!(used.contains(*region), "{region}.svg is never shown");
            let text = String::from_utf8(gunzip(data)).unwrap();
            assert!(text.contains("viewBox=\"0 0 640 480\""), "{region}");
            assert!(
                text.starts_with("<svg width=\"40\" height=\"30\""),
                "{region}"
            );
        }
        assert!(Assets.load("flags/zz.svg").unwrap().is_none());
        let icon = Assets
            .load(crate::ui::settings::ICON_PATH)
            .unwrap()
            .unwrap();
        assert!(icon.starts_with(b"<svg"));
        // Everything else is gpui-kit's icons.
        let check = gpui_kit::assets::IconName::Check.path();
        assert!(Assets.load(&check).unwrap().is_some());
    }

    /// `img()` passes bytes that are not a raster format to GPUI's SVG
    /// renderer, which reads SVGZ (resvg's `Tree::from_data`): every flag
    /// draws at its 4:3 size, as it did uncompressed.
    #[gpui_kit::test]
    fn every_flag_renders_from_svgz(cx: &mut gpui_kit::TestAppContext) {
        let renderer = cx.update(|cx| cx.svg_renderer());
        for (region, _) in FLAGS {
            let data = Assets
                .load(&format!("{FLAGS_PREFIX}{region}.svg"))
                .unwrap()
                .unwrap();
            let image = renderer
                .render_single_frame(&data, 1.0)
                .unwrap_or_else(|err| panic!("{region}: {err}"));
            let size = image.size(0);
            assert_eq!(size.width.0 * 3, size.height.0 * 4, "{region}: {size:?}");
            // Not blank: some pixel is opaque.
            assert!(
                image.as_bytes(0).unwrap().chunks(4).any(|px| px[3] == 255),
                "{region}"
            );
        }
    }

    /// The family name the text system matches a face by: the typographic
    /// family, else the legacy one.
    fn family(face: &ttf_parser::Face) -> Option<String> {
        let name = |id: u16| {
            face.names()
                .into_iter()
                .find(|n| n.name_id == id && n.is_unicode())
                .and_then(|n| n.to_string())
        };
        name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| name(ttf_parser::name_id::FAMILY))
    }

    /// Characters outside the locales that the UI draws with Inter: the
    /// digits and separators of every number format, the change chips'
    /// marks (`ui::row::render_chip`), the arithmetic operators, the
    /// buttons' arrows, and gpui-kit's macOS key symbols (`Kbd`).
    const UI_SYMBOLS: &str = "0123456789.,'=-%\u{a0}\u{2009}\u{202f}▲▼·—–…•×÷−+*/()≈≥\
                              ↑↓←→↵⌘⌥⌃⇧⌫⎋⏎";

    #[test]
    fn the_bundled_fonts_are_static_inter_faces() {
        // What the English UI draws: every locale string, every English
        // currency name and the symbols above.
        let mut needed: BTreeSet<char> = kurrency_core::i18n::backend()
            .messages_for_locale("en")
            .unwrap()
            .into_iter()
            .flat_map(|(_, text)| text.chars().collect::<Vec<_>>())
            .chain(UI_SYMBOLS.chars())
            .chain(
                all_codes().flat_map(|code| money::display_name(&code).chars().collect::<Vec<_>>()),
            )
            .collect();
        needed.retain(|c| !c.is_control() && *c != ' ');
        let fallback = cjk_coverage();
        for data in FONTS.iter() {
            // A static TrueType font (not a collection, WOFF or the variable
            // face, which GPUI draws at one weight).
            assert_eq!(data[..4], [0, 1, 0, 0]);
            let face = ttf_parser::Face::parse(data, 0).unwrap();
            assert!(!face.is_variable());
            assert_eq!(family(&face).as_deref(), Some(FONT_FAMILY));
            // Tabular numerals for the amounts.
            let tnum = ttf_parser::Tag::from_bytes(b"tnum");
            let features = face.tables().gsub.expect("GSUB").features;
            assert!(features.into_iter().any(|f| f.tag == tnum));
            // Every character, in Inter or its bundled fallback.
            let covered = coverage(data);
            let missing: Vec<&char> = needed
                .iter()
                .filter(|c| !covered.contains(c) && !fallback.contains(c))
                .collect();
            assert!(missing.is_empty(), "no glyph for {missing:?}");
        }
    }

    /// The faces are `scripts/subset-inter-font.py`'s subset: small, the
    /// same characters in every weight (so a weight change never swaps the
    /// font), all of Latin-1 and Latin Extended-A, and still hinted and
    /// kerned, like the full faces.
    #[test]
    fn the_inter_faces_are_the_hinted_latin_subset() {
        // Not the soft hyphen, which is never drawn, or the deprecated ŉ,
        // which Inter does not have.
        let latin = (0xa0..=0x17f_u32)
            .filter_map(char::from_u32)
            .filter(|c| !matches!(c, '\u{ad}' | 'ŉ'));
        let latin: BTreeSet<char> = (' '..='~').chain(latin).collect();
        let regular = coverage(&FONTS[0]);
        for data in FONTS.iter() {
            assert!(data.len() < 200_000, "{} bytes", data.len());
            let covered = coverage(data);
            assert_eq!(covered, regular);
            let missing: Vec<&char> = latin.difference(&covered).collect();
            assert!(missing.is_empty(), "no glyph for {missing:?}");
            // Greek and Cyrillic words are cut; their letters in currency
            // symbols (лв, ден) are kept.
            assert!(!covered.contains(&'Ж') && !covered.contains(&'Ω'));
            assert!("лвден฿".chars().all(|c| covered.contains(&c)));

            let face = ttf_parser::Face::parse(data, 0).unwrap();
            let raw = face.raw_face();
            for table in [b"fpgm", b"prep", b"cvt ", b"gasp"] {
                let tag = ttf_parser::Tag::from_bytes(table);
                assert!(raw.table(tag).is_some(), "no {tag} table: unhinted");
            }
            let features = |table: Option<ttf_parser::opentype_layout::LayoutTable<'_>>| {
                let table = table.expect("a layout table");
                table
                    .features
                    .into_iter()
                    .map(|f| f.tag)
                    .collect::<BTreeSet<_>>()
            };
            let gpos = features(face.tables().gpos);
            assert!(gpos.contains(&ttf_parser::Tag::from_bytes(b"kern")));
            let gsub = features(face.tables().gsub);
            for feature in [b"tnum", b"case", b"calt"] {
                assert!(gsub.contains(&ttf_parser::Tag::from_bytes(feature)));
            }
        }
    }

    /// The app never needs a system font for a currency symbol: each one is
    /// drawn from the bundled faces, or the ISO code stands in for it.
    #[test]
    fn every_symbol_is_drawn_or_falls_back_to_the_code() {
        // Per face, as the text system sees them, independent of `has_glyph`.
        let all = |faces: &[Vec<u8>]| faces.iter().map(|f| coverage(f)).collect::<Vec<_>>();
        let (inter, cjk) = (all(&*FONTS), all(&*CJK_FONTS));
        let drawn = |ch: char| {
            inter.iter().all(|set| set.contains(&ch)) || cjk.iter().all(|set| set.contains(&ch))
        };
        let mut fallbacks = Vec::new();
        for code in all_codes() {
            let Some(symbol) = present::display_symbol(&code, |_| true) else {
                assert_eq!(present::display_symbol(&code, has_glyph), None);
                continue;
            };
            let shown = present::display_symbol(&code, has_glyph).unwrap();
            assert!(shown.chars().all(drawn), "{code}: {shown:?} has no glyph");
            if symbol.chars().all(drawn) {
                assert_eq!(shown, symbol, "{code} fell back needlessly");
            } else {
                assert_eq!(shown, code, "{code}: {symbol:?}");
                fallbacks.push(code);
            }
        }
        // The Arabic-script symbols, and Armenian, Bengali, Georgian, Khmer
        // and a combining mark (KGS): the same list as with the full Inter
        // faces, so the subset dropped no symbol that Inter can draw.
        assert_eq!(
            fallbacks,
            [
                "AED", "AFN", "AMD", "BDT", "BHD", "DZD", "GEL", "IQD", "IRR", "KGS", "KHR", "KWD",
                "LYD", "MAD", "OMR", "QAR", "SAR", "TND", "YER"
            ]
        );
        // The common ones keep their symbols.
        for (code, symbol) in [
            ("EUR", "€"),
            ("GBP", "£"),
            ("JPY", "¥"),
            ("INR", "₹"),
            ("KRW", "₩"),
        ] {
            assert_eq!(
                present::display_symbol(code, has_glyph).as_deref(),
                Some(symbol)
            );
        }
    }

    #[test]
    fn the_cjk_fallback_is_a_small_noto_sans_sc_subset() {
        for data in CJK_FONTS.iter() {
            // OpenType with CFF outlines, static (no variations).
            assert_eq!(data[..4], *b"OTTO");
            let face = ttf_parser::Face::parse(data, 0).unwrap();
            assert!(!face.is_variable());
            // One family for both weights, the name the fallback asks for.
            assert_eq!(family(&face).as_deref(), Some(CJK_FONT_FAMILY));
            assert!(data.len() < 200_000, "{} bytes", data.len());
            // Both weights cover the same characters.
            assert_eq!(coverage(data), cjk_coverage());
        }
    }
}
