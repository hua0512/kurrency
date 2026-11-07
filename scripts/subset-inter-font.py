#!/usr/bin/env python3
"""Rebuilds the bundled Inter faces: assets/fonts/Inter-{Regular,Medium,SemiBold}.ttf.

The full static faces of Inter 4.1 (rsms/inter, SIL OFL 1.1) have about 2,850
characters; Kurrency draws a few hundred of them. This keeps:

- Basic Latin, Latin-1 and Latin Extended-A, ƒ and the Romanian comma-below
  letters, and the spacing accents,
- general punctuation (spaces, dashes, quotes, ...), super- and subscripts,
  currency symbols, letterlike symbols (№, ™), arrows, the math operators,
  the macOS key symbols (⌘ ⌥ ⌃ ⇧ ⌫ ⎋ ⏎, gpui-kit's Kbd), geometric shapes
  (the change chips' ▲▼) and the miscellaneous symbols and dingbats (✓),
  as far as Inter has them,
- the characters of the ISO 4217 currency symbols and English currency
  names that Inter can draw outside those ranges (CURRENCY_CHARS),
- every character of locales/*.yml.

Every OpenType layout feature (tnum, kern, case, ...) and the TrueType hinting
stay: Linux draws with swash, which hints, and Windows with DirectWrite.
Greek and Cyrillic words would fall back to a system font.

Usage (from the repository root):

    python3 -I scripts/subset-inter-font.py

The release zip is downloaded into a fresh temporary directory and checked
against a pinned SHA-256 sum. Subsetting uses fontTools' pyftsubset: from
`python3 -m fontTools.subset` when fontTools is installed, else through
`uvx --from fonttools==FONTTOOLS pyftsubset` (https://docs.astral.sh/uv/).
The same inputs and fontTools version give the same bytes.

The app's font tests (crates/kurrency/src/assets.rs) fail when the UI needs a
character the subset lacks, or when a currency symbol that full Inter draws
would fall back to its ISO code: re-run this script after editing the
locales, and widen the ranges below if a test asks for it.
"""

import hashlib
import pathlib
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

FONTTOOLS = "4.68.0"
INTER_VERSION = "4.1"
INTER_ZIP = (
    "https://github.com/rsms/inter/releases/download/"
    f"v{INTER_VERSION}/Inter-{INTER_VERSION}.zip"
)
INTER_ZIP_SHA256 = "9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e"
# The static faces in the zip, and where they go.
FACES = ["Inter-Regular.ttf", "Inter-Medium.ttf", "Inter-SemiBold.ttf"]
FACES_DIR = "extras/ttf/"

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOCALES = ROOT / "locales"
OUT_DIR = ROOT / "assets" / "fonts"

RANGES = [
    "U+0020-007E",  # Basic Latin
    "U+00A0-017F",  # Latin-1, Latin Extended-A
    "U+0192,U+0218-021B",  # ƒ, Romanian ș ț
    "U+02BB-02BC,U+02C6-02C7,U+02D8-02DD",  # modifier letters, accents
    "U+2000-206F",  # general punctuation
    "U+2070-209F",  # super- and subscripts
    "U+20A0-20CF",  # currency symbols
    "U+2100-214F",  # letterlike symbols
    "U+2190-21FF",  # arrows
    "U+2200-22FF",  # math operators
    "U+2300-23FF",  # miscellaneous technical (key symbols)
    "U+25A0-25FF",  # geometric shapes
    "U+2600-27BF",  # miscellaneous symbols, dingbats
    "U+FEFF,U+FFFD",
]

# The characters outside RANGES that Inter has, of the currency symbols
# (iso_currency's table: лв., ден, дин, ฿ and KGS's С) and of the English
# currency names (Vietnamese Đồng).
CURRENCY_CHARS = "Свдеилн฿ồ"


def characters() -> str:
    chars = set(CURRENCY_CHARS)
    for path in sorted(LOCALES.glob("*.yml")):
        chars.update(path.read_text(encoding="utf-8"))
    chars = {c for c in chars if c.isprintable()}
    return "".join(sorted(chars))


def fetch(directory: pathlib.Path) -> zipfile.ZipFile:
    with urllib.request.urlopen(INTER_ZIP) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != INTER_ZIP_SHA256:
        sys.exit(f"{INTER_ZIP}: sha256 {digest}, expected {INTER_ZIP_SHA256}")
    path = directory / f"Inter-{INTER_VERSION}.zip"
    path.write_bytes(data)
    return zipfile.ZipFile(path)


def pyftsubset() -> list[str]:
    try:
        import fontTools  # noqa: F401

        return [sys.executable, "-I", "-m", "fontTools.subset"]
    except ImportError:
        pass
    if shutil.which("uvx"):
        return ["uvx", "--from", f"fonttools=={FONTTOOLS}", "pyftsubset"]
    sys.exit("needs fontTools: pip install fonttools, or install uv")


def main() -> None:
    text = characters()
    with tempfile.TemporaryDirectory(prefix="kurrency-inter-") as tmp:
        tmp = pathlib.Path(tmp)
        chars_file = tmp / "chars.txt"
        chars_file.write_text(text, encoding="utf-8")
        archive = fetch(tmp)
        for face in FACES:
            source = tmp / face
            source.write_bytes(archive.read(FACES_DIR + face))
            out = OUT_DIR / face
            subprocess.run(
                pyftsubset()
                + [
                    str(source),
                    f"--unicodes={','.join(RANGES)}",
                    f"--text-file={chars_file}",
                    f"--output-file={out}",
                    # Every layout feature, the hinting (kept by default) and
                    # every name record (copyright, licence, family, ...).
                    "--layout-features=*",
                    "--name-IDs=*",
                    "--name-languages=*",
                ],
                check=True,
            )
            print(f"{out.relative_to(ROOT)}: {out.stat().st_size} bytes")
    print(f"{len(text)} characters besides the ranges")


if __name__ == "__main__":
    main()
