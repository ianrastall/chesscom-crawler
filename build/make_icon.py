"""Draws the app and tray icon (a light pawn with a dark outline) into src/ChesscomCrawler.App/Assets/app.ico."""
from pathlib import Path
from PIL import Image, ImageDraw

S = 1024  # supersampled canvas
OUT = Path(__file__).resolve().parents[1] / "src" / "ChesscomCrawler.App" / "Assets" / "app.ico"


def draw(size: int) -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    light, dark = (232, 228, 220, 255), (28, 27, 25, 255)
    outline = max(28, S // 22)

    def pawn(fill, grow):
        g = grow
        d.ellipse((S * 0.37 - g, S * 0.10 - g, S * 0.63 + g, S * 0.36 + g), fill=fill)          # head
        d.polygon([(S * 0.40 - g, S * 0.40 - g), (S * 0.60 + g, S * 0.40 - g),
                   (S * 0.69 + g, S * 0.74 + g), (S * 0.31 - g, S * 0.74 + g)], fill=fill)          # body
        d.rounded_rectangle((S * 0.31 - g, S * 0.35 - g, S * 0.69 + g, S * 0.44 + g), radius=S * 0.03, fill=fill)  # collar
        d.rounded_rectangle((S * 0.20 - g, S * 0.72 - g, S * 0.80 + g, S * 0.90 + g), radius=S * 0.05, fill=fill)  # base

    pawn(dark, outline)
    pawn(light, 0)
    return img.resize((size, size), Image.LANCZOS)


sizes = [16, 20, 24, 32, 40, 48, 64, 256]
images = [draw(s) for s in sizes]
images[-1].save(OUT, format="ICO", sizes=[(s, s) for s in sizes], append_images=images[:-1])
print(OUT)
