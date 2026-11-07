#!/usr/bin/env python3
"""Rebuilds the bundled CJK fallback font: assets/fonts/NotoSansSC-*.subset.otf.

Inter has no CJK glyphs and Kurrency never relies on system fonts, so the
Chinese UI draws from Noto Sans SC (SIL OFL 1.1), cut down to the characters
the app can show:

- every character in locales/zh-CN.yml,
- every Simplified Chinese currency name
  (crates/kurrency-core/src/generated/cldr_zh.rs),
- gpui-kit's own zh-CN strings that appear in Kurrency (KIT_STRINGS),
- ASCII, and the common CJK and full-width punctuation.

Usage (from the repository root):

    python3 -I scripts/subset-cjk-font.py

The fonts are downloaded into a fresh temporary directory and checked against
pinned SHA-256 sums. Subsetting uses fontTools' pyftsubset: from
`python3 -m fontTools.subset` when fontTools is installed, else through
`uvx --from fonttools==FONTTOOLS pyftsubset` (https://docs.astral.sh/uv/).
The same inputs and fontTools version give the same bytes.

The app's `i18n::tests::the_cjk_subset_covers_the_chinese_ui` (in
crates/kurrency/src/i18n.rs) fails when a string
needs a character the subset lacks: re-run this script after editing the
locale or the currency names.
"""

import hashlib
import pathlib
import shutil
import subprocess
import sys
import tempfile
import urllib.request

FONTTOOLS = "4.68.0"
NOTO_TAG = "Sans2.004"
BASE = (
    "https://raw.githubusercontent.com/notofonts/noto-cjk/"
    f"{NOTO_TAG}/Sans/SubsetOTF/SC/"
)
FONTS = {
    "NotoSansSC-Regular.otf": "faa6c9df652116dde789d351359f3d7e5d2285a2b2a1f04a2d7244df706d5ea9",
    "NotoSansSC-Medium.otf": "7633f5a016d4dd95e685a69633d818aabc4644c4b08e26bd35b1b30c45ed5dda",
}

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCES = [
    ROOT / "locales" / "zh-CN.yml",
    ROOT / "crates" / "kurrency-core" / "src" / "generated" / "cldr_zh.rs",
]
OUT_DIR = ROOT / "assets" / "fonts"

# gpui-kit's zh-CN strings that Kurrency can show (its locales/ui.yml):
# the input's context menu, the palette's placeholder and empty state, and
# dialog buttons.
KIT_STRINGS = "剪切 复制 粘贴 全选 清除 输入命令或搜索... 未找到结果。暂无数据 确定 取消 关闭"

PUNCTUATION = (
    "，。、；：？！…—～·“”‘’（）《》〈〉「」『』【】〔〕"
    "０１２３４５６７８９％＋－／＝"
)


def characters() -> str:
    chars = {chr(c) for c in range(0x20, 0x7F)}
    for path in SOURCES:
        chars.update(path.read_text(encoding="utf-8"))
    chars.update(KIT_STRINGS)
    chars.update(PUNCTUATION)
    chars = {c for c in chars if c.isprintable()}
    return "".join(sorted(chars))


def fetch(name: str, sha256: str, directory: pathlib.Path) -> pathlib.Path:
    with urllib.request.urlopen(BASE + name) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != sha256:
        sys.exit(f"{name}: sha256 {digest}, expected {sha256}")
    path = directory / name
    path.write_bytes(data)
    return path


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
    with tempfile.TemporaryDirectory(prefix="kurrency-font-") as tmp:
        tmp = pathlib.Path(tmp)
        chars_file = tmp / "chars.txt"
        chars_file.write_text(text, encoding="utf-8")
        for name, sha256 in FONTS.items():
            source = fetch(name, sha256, tmp)
            out = OUT_DIR / name.replace(".otf", ".subset.otf")
            subprocess.run(
                pyftsubset()
                + [
                    str(source),
                    f"--text-file={chars_file}",
                    f"--output-file={out}",
                    # Kerning and the usual shaping features; no hinting,
                    # which GPUI does not use.
                    "--layout-features=kern,liga,locl,ccmp,mark,mkmk,halt,palt",
                    "--no-hinting",
                    "--desubroutinize",
                    # Keep the copyright, licence, version and family records.
                    "--name-IDs=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17",
                    "--name-languages=*",
                ],
                check=True,
            )
            print(f"{out.relative_to(ROOT)}: {out.stat().st_size} bytes")
    print(f"{len(text)} characters")


if __name__ == "__main__":
    main()
