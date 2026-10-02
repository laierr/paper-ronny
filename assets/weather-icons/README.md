# Weather Icons bitmaps

Source: https://github.com/erikflowers/weather-icons
Upstream font: `font/weathericons-regular-webfont.ttf` (Weather Icons 2.0.10).
Designs by Lukas Bischoff; Weather Icons artwork by Erik Flowers.
Licensed under SIL Open Font License 1.1; see OFL.txt. No modified font is
distributed: the .bin files are monochrome raster renderings of eight glyphs.

Rebuild with `python scripts/build_weather_icons.py` (Pillow 11.3.0).
Each bitmap is 64x64, row-major, MSB-first, white=1, black=0. Only the bitmap
files are embedded in firmware; the upstream font is a host-side source asset.
