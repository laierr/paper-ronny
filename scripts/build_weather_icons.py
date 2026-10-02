"""Rasterize upstream OFL Weather Icons; run with Pillow installed (host only)."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
NAMES = ['sun', 'moon', 'partly', 'cloud', 'fog', 'rain', 'snow', 'storm']
CODES = [0xf00d, 0xf02e, 0xf002, 0xf013, 0xf014, 0xf019, 0xf01b, 0xf01e]
NAMES += ['celsius', 'fahrenheit', 'humidity', 'phase_new', 'phase_waxing_crescent',
          'phase_first_quarter', 'phase_waxing_gibbous', 'phase_full',
          'phase_waning_gibbous', 'phase_third_quarter', 'phase_waning_crescent']
CODES += [0xf03c, 0xf045, 0xf07a, 0xf095, 0xf098, 0xf09c, 0xf09f,
          0xf0a3, 0xf0a6, 0xf0aa, 0xf0ad]
font_path = ROOT / 'assets/weather-icons/weathericons-regular-webfont.ttf'
sheet = Image.new('1', (64 * len(CODES), 64), 1)
for index, (name, code) in enumerate(zip(NAMES, CODES)):
    font = ImageFont.truetype(str(font_path), 64)
    glyph = chr(code)
    bounds = font.getbbox(glyph)
    width, height = bounds[2] - bounds[0], bounds[3] - bounds[1]
    font = ImageFont.truetype(str(font_path), int(64 * 56 / max(width, height)))
    bounds = font.getbbox(glyph)
    width, height = bounds[2] - bounds[0], bounds[3] - bounds[1]
    image = Image.new('L', (64, 64), 255)
    ImageDraw.Draw(image).text(((64-width)//2-bounds[0], (64-height)//2-bounds[1]), glyph, font=font, fill=0)
    mono = image.point(lambda v: 255 if v >= 128 else 0, mode='1')
    (ROOT / f'assets/weather-icons/{name}.bin').write_bytes(mono.tobytes())
    sheet.paste(mono, (index*64, 0))
(ROOT / 'target').mkdir(exist_ok=True)
sheet.save(ROOT / 'target/weather-icons-preview.png')
