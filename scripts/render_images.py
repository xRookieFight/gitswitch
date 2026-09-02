#!/usr/bin/env python3
"""Turn the JSON frames produced by `cargo run --example screenshots` into the
PNG screenshots and the animated GIF used in the README.

Usage:
    cargo run --example screenshots
    python3 scripts/render_images.py
"""

from __future__ import annotations

import json
import pathlib
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
FRAMES = ROOT / "docs" / "frames"
IMAGES = ROOT / "docs" / "images"

FONT_CANDIDATES = [
    "/usr/share/fonts/google-noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Regular.ttf",
    "/usr/share/fonts/liberation-mono-fonts/LiberationMono-Regular.ttf",
    "/usr/share/fonts/gdouros-symbola/Symbola.ttf",
    "/usr/share/fonts/urw-base35/NimbusMonoPS-Regular.otf",
]
BOLD_CANDIDATES = [
    "/usr/share/fonts/google-noto/NotoSansMono-Bold.ttf",
    "/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Bold.ttf",
    "/usr/share/fonts/liberation-mono-fonts/LiberationMono-Bold.ttf",
] + FONT_CANDIDATES

FONT_SIZE = 17
PAD = 22
TITLEBAR = 34
RADIUS = 12
BACKGROUND = (26, 27, 38)
CHROME = (18, 19, 28)
DEFAULT_FG = (202, 211, 245)
PAGE = (13, 14, 20)

# The animation and the still shots that end up in the README.
GIF_SEQUENCE = [
    ("03-accounts", 1700),
    ("05-selected", 900),
    ("06-switching", 500),
    ("07-switched", 2000),
    ("04-help", 1900),
    ("08-confirm", 1500),
]


def load_fonts(paths: list[str], size: int) -> list[ImageFont.FreeTypeFont]:
    fonts = []
    for path in paths:
        if pathlib.Path(path).exists():
            try:
                fonts.append(ImageFont.truetype(path, size))
            except OSError:
                continue
    if not fonts:
        sys.exit("no usable monospace font found")
    return fonts


def mask_bytes(font: ImageFont.FreeTypeFont, char: str) -> bytes:
    """Rasterised glyph, used to detect a missing (.notdef) character."""
    mask = font.getmask(char)
    return bytes(bytearray(mask))


class FontSet:
    """A primary font plus fallbacks, so box drawing and arrows always render."""

    def __init__(self, paths: list[str], size: int) -> None:
        self.fonts = load_fonts(paths, size)
        self._missing = [mask_bytes(font, "￾") for font in self.fonts]
        self._cache: dict[str, ImageFont.FreeTypeFont] = {}

    def pick(self, char: str) -> ImageFont.FreeTypeFont:
        if char in self._cache:
            return self._cache[char]
        chosen = self.fonts[0]
        for font, missing in zip(self.fonts, self._missing):
            try:
                mask = mask_bytes(font, char)
            except OSError:
                continue
            if mask and mask != missing:
                chosen = font
                break
        self._cache[char] = chosen
        return chosen


# Box drawing is painted with real lines instead of glyphs so borders always
# join up, whatever the font on the machine happens to cover.
BOX_PARTS = {
    "\u2500": "lr",
    "\u2501": "lr",
    "\u2502": "ud",
    "\u2503": "ud",
    "\u250c": "rd",
    "\u2510": "ld",
    "\u2514": "ru",
    "\u2518": "lu",
    "\u251c": "udr",
    "\u2524": "udl",
    "\u252c": "lrd",
    "\u2534": "lru",
    "\u253c": "lrud",
    "\u256d": "rd",
    "\u256e": "ld",
    "\u256f": "lu",
    "\u2570": "ru",
}


def draw_box(draw: ImageDraw.ImageDraw, char: str, box: tuple[int, int, int, int], color) -> None:
    x, y, w, h = box
    cx, cy = x + w // 2, y + h // 2
    parts = BOX_PARTS[char]
    if "l" in parts:
        draw.line([(x, cy), (cx, cy)], fill=color)
    if "r" in parts:
        draw.line([(cx, cy), (x + w, cy)], fill=color)
    if "u" in parts:
        draw.line([(cx, y), (cx, cy)], fill=color)
    if "d" in parts:
        draw.line([(cx, cy), (cx, y + h)], fill=color)


def render(frame: dict, regular: FontSet, bold: FontSet, title: str) -> Image.Image:
    cell_w = round(regular.fonts[0].getlength("M"))
    ascent, descent = regular.fonts[0].getmetrics()
    cell_h = ascent + descent + 2

    cols, rows = frame["width"], frame["height"]
    inner_w, inner_h = cols * cell_w, rows * cell_h
    win_w = inner_w + PAD * 2
    win_h = inner_h + PAD * 2 + TITLEBAR

    window = Image.new("RGB", (win_w, win_h), BACKGROUND)
    draw = ImageDraw.Draw(window)
    draw.rectangle([0, 0, win_w, TITLEBAR], fill=CHROME)
    for index, color in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        x = 18 + index * 20
        draw.ellipse([x, TITLEBAR // 2 - 6, x + 12, TITLEBAR // 2 + 6], fill=color)
    label_font = bold.pick("g")
    draw.text(
        (win_w // 2, TITLEBAR // 2),
        title,
        font=label_font,
        fill=(146, 156, 186),
        anchor="mm",
    )

    cells = frame["cells"]
    for index, cell in enumerate(cells):
        col, row = index % cols, index // cols
        x = PAD + col * cell_w
        y = TITLEBAR + PAD + row * cell_h

        if cell["bg"]:
            draw.rectangle([x, y, x + cell_w, y + cell_h], fill=tuple(cell["bg"]))

        char = cell["c"]
        if not char.strip():
            continue

        color = tuple(cell["fg"]) if cell["fg"] else DEFAULT_FG
        if char in BOX_PARTS:
            draw_box(draw, char, (x, y, cell_w, cell_h), color)
            continue

        fonts = bold if cell["bold"] else regular
        draw.text(
            (x, y + 1),
            char,
            font=fonts.pick(char),
            fill=color,
        )

    return rounded(window)


def rounded(window: Image.Image) -> Image.Image:
    """Rounds the window corners and drops it on a padded page with a shadow."""
    mask = Image.new("L", window.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, window.width - 1, window.height - 1], RADIUS, fill=255
    )

    margin = 26
    page = Image.new("RGB", (window.width + margin * 2, window.height + margin * 2), PAGE)

    shadow = Image.new("L", page.size, 0)
    ImageDraw.Draw(shadow).rounded_rectangle(
        [margin, margin + 6, margin + window.width, margin + window.height + 6],
        RADIUS,
        fill=110,
    )
    shadow = shadow.filter(ImageFilter.GaussianBlur(12))
    page.paste(Image.new("RGB", page.size, (0, 0, 0)), (0, 0), shadow)

    page.paste(window, (margin, margin), mask)
    return page


def main() -> None:
    frames = sorted(FRAMES.glob("*.json"))
    if not frames:
        sys.exit("no frames found - run `cargo run --example screenshots` first")

    IMAGES.mkdir(parents=True, exist_ok=True)
    regular = FontSet(FONT_CANDIDATES, FONT_SIZE)
    bold = FontSet(BOLD_CANDIDATES, FONT_SIZE)

    rendered: dict[str, Image.Image] = {}
    for path in frames:
        frame = json.loads(path.read_text())
        image = render(frame, regular, bold, "gitswitch")
        rendered[path.stem] = image
        out = IMAGES / f"{path.stem}.png"
        image.save(out, optimize=True)
        print(f"wrote {out.relative_to(ROOT)}")

    sequence = [(rendered[name], ms) for name, ms in GIF_SEQUENCE if name in rendered]
    if sequence:
        images = [image.quantize(colors=200, method=Image.Quantize.MEDIANCUT) for image, _ in sequence]
        gif = IMAGES / "demo.gif"
        images[0].save(
            gif,
            save_all=True,
            append_images=images[1:],
            duration=[ms for _, ms in sequence],
            loop=0,
            optimize=True,
        )
        print(f"wrote {gif.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
