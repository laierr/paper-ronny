# Weather Icons bitmaps

Source: https://github.com/erikflowers/weather-icons
Upstream font: `font/weathericons-regular-webfont.ttf` (Weather Icons 2.0.10).
Designs by Lukas Bischoff; Weather Icons artwork by Erik Flowers.
Licensed under SIL Open Font License 1.1; see OFL.txt. No modified font is
distributed: the .bin files are monochrome raster renderings of condition,
Celsius/Fahrenheit, humidity, and eight moon-phase glyphs.

Rebuild from the repository root with Pillow 11.3.0 in a host-side virtual
environment (the checked-in bitmaps mean this is optional for firmware builds).

Linux / macOS (POSIX shell):

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install Pillow==11.3.0
python scripts/build_weather_icons.py
```

Windows (PowerShell):

```powershell
py -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install Pillow==11.3.0
python scripts/build_weather_icons.py
```

Each bitmap is 64x64, row-major, MSB-first, white=1, black=0. Only the bitmap
files are embedded in firmware; the upstream font is a host-side source asset.
