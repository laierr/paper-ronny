# Paper Ronny

This repository contains two implementations side by side:

- [`src/main.rs`](src/main.rs) is the primary Rust/ESP-IDF implementation.
- [`src/main.cpp`](src/main.cpp) is the original Arduino/C++ implementation,
  retained as a working reference for comparison.

The two versions use the same wiring, serial command vocabulary, NVS settings,
and weather provider. Their display implementations remain side by side for
comparison.

## Rust build (primary)

The Rust build uses ESP-IDF and the `esp-idf-sys` PlatformIO backend. The
original ESP32-D0WD-V3 is an Xtensa target, so install the Espressif Rust
toolchain first:

```powershell
cargo install espup ldproxy
espup install
. $HOME\export-esp.ps1
cargo build --release
espflash flash --port COM14 --baud 921600 --monitor --monitor-baud 115200 target\xtensa-esp32-espidf\release\paper-ronny
espflash monitor --port COM14
```

The host also needs the C++ workload from Visual Studio Build Tools on Windows;
`espup` and several Cargo build dependencies require Microsoft's linker.

## C++ reference build

The existing PlatformIO environment remains available unchanged:

```sh
pio run
pio run -t upload
pio device monitor
```

A serial-configured Wi-Fi clock and weather dashboard for an ESP32-D0WD-V3
development board and the WeAct Studio 3.7-inch, 240x416 monochrome e-paper
module (`GDEY037T03`, `UC8253`). The display is driven in landscape mode.

## Wiring

Both the ESP32 and the WeAct module use **3.3 V logic**.

| WeAct pin | ESP32 pin | Notes |
|---|---:|---|
| VCC | 3V3 | Do not use 5 V |
| GND | GND | Common ground |
| SDA / DIN | GPIO 23 | SPI MOSI |
| SCL / CLK | GPIO 18 | SPI clock |
| CS | GPIO 5 | Chip select |
| D/C | GPIO 17 | Data/command |
| RES / RST | GPIO 16 | Reset |
| BUSY | GPIO 4 | Busy input |

These values match the executable definitions in WeAct's ESP32 example. Its
nearby comment reverses D/C and reset, so follow the table above. Pins can be
changed in `platformio.ini` without editing source code.

## Build, flash, and monitor

Install PlatformIO, connect the ESP32, then run:

```sh
pio run
pio run -t upload
pio device monitor
```

The serial port is 115200 baud with a newline line ending. Type `help` to list
all commands. A first-time setup might look like:

```text
set ssid My WiFi
set password my secret password
set latitude 32.0853
set longitude 34.7818
set location Tel Aviv
set timezone IST-2IDT,M3.4.4/26,M10.5.0
save
connect
sync
weather
refresh
```

`set` changes the in-memory configuration. `save` persists it in ESP32 NVS.
`show` displays the active settings (the password is never printed). Use a
[POSIX TZ string](https://github.com/nayarsystems/posix_tz_db/blob/master/zones.csv)
for `timezone`; the example above is Israel time with daylight saving rules.

Other useful commands:

```text
status                 connection, time, and weather status
set units metric       metric or imperial
set weather_minutes 15 automatic weather refresh interval (5..180)
display test            show a test page briefly, then restore the dashboard
refresh test            enter bench mode and run configured test (same as rt)
refresh test 1          rerun stage 1 (1..6) and leave digit 9 displayed
rt                      run the configurable refresh test
rt show                 show its current parameters
rt set time 1           seconds between draws (0..60)
rt set size large       large or small hardware window
rt set mode fast        fast or normal partial waveform
rt set sync on          synchronize the previous-image plane after each draw
rt set cycles forever   sequence cycles, or a finite number
rt set glyphs digits    digits or all printable ASCII glyphs
rt set full_every 20    full cleanup every N draws, or off
rt save                 persist the refresh-test parameters
rt all                  run all six presets with responsive controls
stop                    pause test, keeping the bench image visible
run                     resume, or restart a completed sequence
clock                   return to the clock demo
refresh                 fully redraw the current mode/image
rt set full_mode fast   fast (GxEPD2 default) or normal full waveform
rt set prime old        post-full RAM seeding: old, both, or none
erase                    erase saved settings after confirmation
reboot                   restart the ESP32
```

Commands and detailed validation errors appear only on serial. After Wi-Fi is
configured, the firmware reconnects automatically, synchronizes time with NTP,
fetches current conditions from Open-Meteo, and refreshes the clock once per
minute. Weather is refreshed every 15 minutes by default.

Bench mode suppresses clock, time-sync, and weather-triggered display updates,
including after a finite test finishes. Automatic weather fetching is deferred
until clock mode resumes. Serial commands run between individual panel updates;
`stop` responds after the current waveform completes, without waiting for the
configured inter-draw delay. Settings persist only when saved.

The full-to-partial transition now follows the local GxEPD2 driver: write both
planes through an explicit RAM window, trigger FastFull outside partial mode,
reset panel settings (handled by epdsi), then prime the old plane. The previous
Rust sequence used normal Full and omitted that reset. To isolate the remaining
physical symptom, use `rt set full_every 10` and compare `full_mode fast` versus
`normal`, keeping `mode fast` and `sync on`; compare `prime old` versus `both`
separately. Logs include actual waveform duration. These are diagnostic controls;
the full-to-partial fix has now been visually confirmed by the user.

## Advanced Rust refresh bench

`rt set` opens a numbered settings menu and pauses an active bench. Enter a
setting number, then its value when prompted. It returns to the setting list
after each successful edit. `0` at the list or `cancel` exits; zero remains a
valid setting value. `run` resumes; `rt save` persists settings. Direct
`rt set <key> <value>` commands still work.

Regions use logical landscape coordinates: **416 wide by 240 high**, origin at
top left. `region preset` retains the original two tiles. `rect x y w h` sets
all four fields atomically and selects `custom`; `region sweep` scans a 5x3
grid including edges and center, while `region random` uses repeatable
pseudorandom positions with the configured width/height. Moving tests leave
previous tiles in place deliberately, exposing corruption outside the window.
Non-preset baselines include reference grid marks.

Content choices are `digits`, `all` (94 printable ASCII glyphs), `shapes`
(filled/outlined rectangle, circle, triangle and diagonal), `shades`
(17 black-coverage levels using 4x4 ordered dithering), and `mixed` (interleaves
the four types). `size large|small` selects glyph scale, independent of custom
region dimensions; glyphs are clipped rather than resized to fit. The panel
is monochrome: shades are spatial dithering, not native grayscale.

Example: test unaligned, moving 160x120 patches without periodic cleanup:

```text
rt set rect 13 7 160 120
rt set region random
rt set content mixed
rt set size large
rt set mode fast
rt set sync on
rt set cycles forever
rt set time 1
rt set full_every off
rt
```

For a full-screen fast-partial stress test use `rt set rect 0 0 416 240`.
To test small text use `rt set content all` and `rt set size small`.
Each draw logs content/index, requested rectangle, actual byte-aligned native
window, pixel count, percentage of panel area, and waveform duration. Expanded
edge pixels are copied from the framebuffer unchanged. Repeat at different
sizes and gaps, watching for ghosting, missed transitions, or changes outside
the window; the firmware does not automatically declare a region safe.

Geometry checks can run on the host without ESP-IDF:

```sh
rustc +stable --test src/bench_geometry.rs -o target/bench-geometry-tests.exe
target/bench-geometry-tests.exe
```

## Notes

- Weather requests use Open-Meteo and need no API key. This demo disables TLS
  certificate validation to avoid maintaining a root CA on the device; add CA
  validation before using the networking code in a security-sensitive product.
- The Rust clock uses a byte-aligned hardware window for minute updates. It
  performs a full cleanup after 10 clock patches and whenever date, weather, or
  configuration content changes.
- If the module is a different 3.7-inch revision, confirm the panel marking.
  This project specifically selects `GxEPD2_370_GDEY037T03`.
