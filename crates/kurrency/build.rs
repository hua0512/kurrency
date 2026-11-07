//! Embeds the flag SVGs in the repository's `assets/flags/` as a sorted
//! `(region, bytes)` table and compresses the bundled fonts, so the binary
//! needs no files at runtime, and on Windows compiles the executable's icon
//! and version resource.
//!
//! Each flag is stored gzipped (SVGZ), which resvg reads as it is, and each
//! font raw-deflated, inflated once at startup (`assets::FONTS`): about 1 MB
//! less in the binary.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::{env, fs};

use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder};

/// The fonts in `assets/fonts/` that `assets.rs` embeds.
const FONTS: [&str; 5] = [
    "Inter-Regular.ttf",
    "Inter-Medium.ttf",
    "Inter-SemiBold.ttf",
    "NotoSansSC-Regular.subset.otf",
    "NotoSansSC-Medium.subset.otf",
];

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    // The repository root: `assets/` and `packaging/` are shared with the
    // packaging tools and the docs, so they live there, not in this crate.
    // (`parent()` rather than `..`, so the paths stay plain for rc.exe.)
    let root = Path::new(&manifest_dir)
        .parent()
        .and_then(Path::parent)
        .expect("the crate is crates/kurrency in the repository");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let out_dir = Path::new(&out_dir);
    flags(&root.join("assets").join("flags"), out_dir);
    fonts(&root.join("assets").join("fonts"), out_dir);
    windows_resources(root);
}

/// Writes `OUT_DIR/flags.rs`, the table of `OUT_DIR/flags/<region>.svgz`.
fn flags(flags_dir: &Path, out_dir: &Path) {
    println!("cargo:rerun-if-changed={}", flags_dir.display());
    let svgz_dir = out_dir.join("flags");
    fs::create_dir_all(&svgz_dir).expect("OUT_DIR/flags");

    let mut regions: Vec<String> = fs::read_dir(flags_dir)
        .expect("assets/flags")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".svg").map(str::to_string)
        })
        .collect();
    regions.sort();

    let mut out = String::from(
        "/// Embedded flags (gzipped SVG), sorted by region.\n\
         pub(crate) static FLAGS: &[(&str, &[u8])] = &[\n",
    );
    for region in &regions {
        let svg = fs::read(flags_dir.join(format!("{region}.svg"))).expect("a flag");
        // flate2 writes a zero time into the gzip header, so the bytes only
        // depend on the SVG.
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(&svg).expect("gzip a flag");
        let path = svgz_dir.join(format!("{region}.svgz"));
        fs::write(&path, gz.finish().expect("gzip a flag")).expect("write a .svgz");
        writeln!(
            out,
            "    ({region:?}, include_bytes!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    fs::write(out_dir.join("flags.rs"), out).expect("write flags.rs");
}

/// Writes each of [`FONTS`] raw-deflated to `OUT_DIR/<name>.deflate`.
fn fonts(fonts_dir: &Path, out_dir: &Path) {
    for name in FONTS {
        let path = fonts_dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let font = fs::read(&path).expect("a bundled font");
        let mut deflate = DeflateEncoder::new(Vec::new(), Compression::best());
        deflate.write_all(&font).expect("deflate a font");
        let data = deflate.finish().expect("deflate a font");
        fs::write(out_dir.join(format!("{name}.deflate")), data).expect("write a font");
    }
}

/// Compiles `packaging/windows/kurrency.rc` (the icon and version information)
/// into `kurrency.exe`, given the repository root. `embed-resource` is a build
/// dependency on Windows hosts only, where the resource compiler comes with
/// the MSVC tools.
#[cfg(windows)]
fn windows_resources(root: &Path) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let packaging = root.join("packaging");
    let template = packaging.join("windows").join("kurrency.rc");
    let icon = packaging.join("icons").join("kurrency.ico");
    println!("cargo:rerun-if-changed={}", template.display());
    println!("cargo:rerun-if-changed={}", icon.display());

    let var = |name: &str| env::var(name).unwrap_or_else(|_| panic!("{name}"));
    let rc = fs::read_to_string(&template)
        .expect("packaging/windows/kurrency.rc")
        .replace("@VERSION_MAJOR@", &var("CARGO_PKG_VERSION_MAJOR"))
        .replace("@VERSION_MINOR@", &var("CARGO_PKG_VERSION_MINOR"))
        .replace("@VERSION_PATCH@", &var("CARGO_PKG_VERSION_PATCH"))
        .replace("@VERSION@", &var("CARGO_PKG_VERSION"))
        // Forward slashes need no escaping in an RC string, and both rc.exe
        // and llvm-rc accept them.
        .replace("@ICON@", &icon.display().to_string().replace('\\', "/"));
    let rc_path = Path::new(&var("OUT_DIR")).join("kurrency.rc");
    fs::write(&rc_path, rc).expect("write kurrency.rc");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("compile kurrency.rc");
}

#[cfg(not(windows))]
fn windows_resources(_root: &Path) {}
