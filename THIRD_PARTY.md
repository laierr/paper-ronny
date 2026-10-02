# Third-party notices

Third-party licenses are separate from any license selected for this project's
own code. This repository does not relicense dependencies.

## Weather Icons

The original Weather Icons 2.0.10 font and rasterized glyphs are in
`assets/weather-icons/`. Artwork by Erik Flowers and Lukas Bischoff, distributed
under SIL Open Font License 1.1. Source, regeneration instructions, and the
license are included in that directory. No modified font is distributed.

## Vendored esp-idf-sys

`third_party/esp-idf-sys/` contains esp-idf-sys 0.37.2 from
<https://github.com/esp-rs/esp-idf-sys>, licensed under MIT OR Apache-2.0.
Both license files and original upstream metadata are preserved there.
The local compatibility change in `build/pio.rs` reads the optional
`pio_framework_dir` field and returns a descriptive error when it is absent.

## Firmware dependencies

Rust dependencies and exact versions are recorded in `Cargo.lock`; their own
license metadata can be inspected with `cargo metadata --locked`. ESP-IDF and
its components retain their upstream licenses.

The original C++ reference uses GxEPD2 (GPL-3.0), Adafruit GFX (BSD), and
ArduinoJson (MIT), fetched by PlatformIO. Their source/license files are supplied
by the dependency manager. In particular, distributing a compiled C++ firmware
requires observing the GPL obligations of GxEPD2; a permissive license on this
project's own code would not remove those obligations. No compiled firmware or
PlatformIO dependency cache is included in this repository.

## Weather data

Weather data by <https://open-meteo.com/>, under CC BY 4.0. API access is subject
to the provider's terms: <https://open-meteo.com/en/terms>.
