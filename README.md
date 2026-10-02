# Paper Ronny

Project code is [MIT licensed](LICENSE). Fonts, vendored code, dependencies,
and weather data retain their own licenses; see [third-party notices](THIRD_PARTY.md).

This repository contains two implementations side by side:

- [`src/main.rs`](src/main.rs) is the primary Rust/ESP-IDF implementation.
- [`src/main.cpp`](src/main.cpp) is the original Arduino/C++ implementation,
  retained as a working reference for comparison.

The two versions use the same wiring, basic serial command vocabulary, and
weather provider. Settings are stored separately in ESP32 NVS (`paper_ronny`
for Rust, `paper-ronny` for C++); configure each version independently. Their
display implementations remain side by side for comparison. The advanced
refresh bench and cleanup schedule described below are Rust-only.

## Daily-use Rust clock / multi-day soak

Clock mode is the boot default. Routine minute, date, weather, and configuration
updates use FastPartial plus old-image synchronization. A byte-level framebuffer
diff selects the smallest enclosing native window; unchanged pixels/bytes are
retained, and identical frames do not trigger a waveform. Returning from the
bench is also a fast content-aware redraw.

The cleanup schedule follows **local calendar time**, using the configured TZ:

- At each new local hour: fast white blank → current dashboard.
- At 00:00, 06:00, 12:00, 18:00: fast white → black → white → dashboard instead.
- No periodic full waveform by patch count, weather fetch, or midnight change.
  Full refresh is reserved for boot/unknown-state recovery, explicit `refresh`,
  and bench baseline setup. Full recovery retains the proven post-full fast
  blank/redraw conditioning for fine text.

The first valid time arms the hour schedule without an immediate chime. Delayed
hour detection (e.g. a weather request) runs the current hour's cleanup once,
not a backlog. Backward/repeated DST hours don't double-fire. No audio hardware
is configured: "chime" here means the screen's local-hour cleanup sequence.

The redesigned 416×240 landscape page has a large 112-pixel-tall clock, centered
date, separator, and one aligned weather-symbol row: temperature + Celsius or
Fahrenheit glyph, condition glyph, humidity glyph + value (no extra percent
sign), and moon-phase glyph. Weather Icons by
Erik Flowers/Lukas Bischoff are embedded as 64×64 monochrome bitmaps under SIL
OFL 1.1; source, license and regeneration instructions are in
`assets/weather-icons/`. Weather conditions determine the icon, including a moon
for clear night. No city name or unsynchronized 1970 date is displayed.
The moon glyph uses Open-Meteo's `daily=moon_phase` fraction, mapped to the
nearest of eight icons; no local astronomical approximation is used. Two days
are cached so midnight can select the next day's value without waiting for a
weather fetch. Missing/invalid phases or dates without data remain hidden,
as does the moon until time is synchronized. Detailed condition, feels-like,
and wind readings remain available through `status`, not on the compact row.

Wi-Fi reconnects are retried, failed weather fetches retry after 60 seconds, and
last-known data are retained with a `(stale)` label. HTTPS uses ESP-IDF's root
certificate bundle. Weather retry timing uses uptime, but screen cleanup timing
does not. Serial commands remain available between waveform/network operations.

For the soak, leave USB power connected in `clock` mode. `status` or `clock stats`
reports uptime, automatic patch count, hourly-white/six-hour-scrub counts, last
cleanup and free heap. Counters are in RAM and reset on reboot; logs name every
scheduled cleanup. The firmware runs autonomously with the serial monitor closed.
This is the start of the multi-day test, not evidence that artifacts cannot
accumulate. Avoid bench/manual refresh commands during the soak if you want an
uninterrupted view of the production policy.

Host preview and tests (no ESP-IDF required):

```powershell
cargo +stable run --manifest-path scripts/clock-preview/Cargo.toml --target x86_64-pc-windows-msvc --target-dir target/clock-preview-host
cargo +stable test --manifest-path scripts/clock-preview/Cargo.toml --target x86_64-pc-windows-msvc --target-dir target/clock-preview-host
```

The preview renders actual display code to `target/clock-preview.pbm` and verifies
minute changes preserve date/weather, and single-character updates preserve all
neighboring pixels. Unit tests cover dirty windows and local cleanup boundaries.

## Rust build (primary)

The Rust build uses ESP-IDF and the `esp-idf-sys` PlatformIO backend. The
original ESP32-D0WD-V3 is an Xtensa target, so install the Espressif Rust
toolchain first:

```powershell
cargo install espup ldproxy espflash
espup install
$env:LIBCLANG_PATH = "$env:USERPROFILE\.rustup\toolchains\esp\xtensa-esp32-elf-clang\esp-clang\bin"
cargo build --release
espflash flash --port COM14 --baud 921600 --monitor --monitor-baud 115200 target\xtensa-esp32-espidf\release\paper-ronny
espflash monitor --port COM14
```

The host also needs the C++ workload from Visual Studio Build Tools on Windows;
`espup` and several Cargo build dependencies require Microsoft's linker. The
`LIBCLANG_PATH` example assumes the default Rust installation directory; adjust
it if using a custom `RUSTUP_HOME`. Set it in each new PowerShell session.
Replace `COM14` with your board's port (`espflash board-info` can detect it).
Press Ctrl+R in the monitor if the board is still in the bootloader after flashing;
Ctrl+C closes the monitor without stopping the running firmware.

This build enables the PlatformIO backend internally through Cargo; you do not
run `pio run` for Rust. A patched `esp-idf-sys` crate is vendored for compatibility
with the PlatformIO metadata interface; see [third-party notices](THIRD_PARTY.md).
This configuration has been exercised on Windows, not validated on every host.

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
changed in `platformio.ini` for C++; Rust pin assignments are in `src/main.rs`.

## Serial setup (both implementations)

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

### Fixed-grid refresh bench (Rust)

`rt grid` starts a fixed six-cell mosaic. Unlike random tests, each cell retains
its location and has its own sequence, with different window aspect ratios:

| Cell | Landscape rectangle (x y width height) | Content |
|---|---|---|
| 1 | 4 4 128 232 | Large seven-segment digit |
| 2 | 136 4 276 44 | Changing large-font text strings |
| 3 | 136 52 132 132 | Filled/outlined geometry |
| 4 | 272 52 140 132 | Dither coverage 0..100% black |
| 5 | 136 188 132 48 | Changing small-font text strings |
| 6 | 272 188 140 48 | Solid black → sparse text → white → text |

The cell windows are rectangular (controller requirement); artwork includes
circles, triangles and other shapes. Byte-rounded gutter pixels are preserved.
Grid mode uses these fixed content roles rather than the global `content` and
`size` settings. Initial content is drawn during baseline setup; subsequent
updates start with step 1. A cycle is 94 rounds: 564 cell updates, or 94
whole-screen updates. `cycles forever` repeats indefinitely.

```text
rt set mode fast
rt set sync on
rt set full_every off
rt set time 1
rt set cycles forever
rt set grid_update cell
rt grid
```

`grid_update cell` updates one cell per draw, round-robin. For the whole-screen
experiment, stop and switch to `rt set grid_update screen`, then `run`:
all six cells advance together and the entire framebuffer is sent in one
partial waveform. With `mode fast`, this is **FastPartial over the whole
screen**, not the flashing FastFull waveform. Menu item 16 selects this option.
Changing bench settings resets the baseline on the next run, using a full
waveform; `full_every off` disables periodic full cleanup after that.

Outside grid mode, `rt set content text` tests strings in any arbitrary region;
`size` selects 10x20 or 6x10 font pixels.

### Experimental fast cleanup

`refresh fast` works in either clock or bench mode, running two full-screen
**FastPartial** waveforms: blank white, synchronize the blank previous-image
plane, then restore the complete saved framebuffer and synchronize it again.
It preserves the framebuffer, bench sequence, and running/paused state, and
logs the duration of both phases. This probes whether an intermediate white
image reduces black-to-white residue without a full waveform. It is not a
recovery operation for an unknown/corrupted physical state: it assumes the
framebuffer matches the visible image. Plain `refresh` retains the full-waveform
recovery path. Clock mode also uses this blank/redraw sequence for hourly cleanup.

### Packed text and full-screen pattern tests

`rt text` fills the screen with readable monospace text, then replaces small
character spans on different rows. `rt set size small` uses 69 columns × 24 rows
of 6×10 cells; `large` uses 41 columns × 12 rows of 10×20 cells. Only the selected
cells are erased and rewritten. Controller byte rounding preserves neighboring
characters and spacing. `rt set text_chars 1` exercises single-character writes;
the default is 8 characters. Counts longer than a row are clamped to its width.
Rows/columns are zero-based in serial logs; unfilled right-edge pixels remain
white. This is a separate page bench, not the old single-string tile test.

`rt patterns` selects fast/synced full-screen updates and disables periodic full
cleanup. Its first eight targets are: vertical lines → horizontal lines → grid
→ black → white grid on black → invert → invert → solid white. Extra stages
probe 50% dither → black → white, then sparse shapes → black → white.
Each cycle has 14 draws. The repeated-frame pass was removed after panel testing
showed little benefit. Lines have 16-pixel pitch; inversion stages use fixed
target images. Logs name each target.

For a direct-vs-blanking comparison:

```text
rt set time 1
rt set cycles 1
rt set blank off
rt patterns
rt set blank on
rt patterns
rt set blank scrub
rt patterns
```

`blank on` uses fast whole-screen white blanking plus a fast whole-screen target
redraw for every step, syncing both intermediate states. The old baseline comes
from the framebuffer **before** rendering the next target. This option overrides
the partial `mode` and `sync` probes; even a tiny text patch then causes two
whole-screen waveforms, while retaining all untouched framebuffer pixels. With
`blank off`, normal configured window/mode/sync behavior applies. A nonzero
`full_every` still takes precedence, so keep it off for this comparison.
Menu items 17 and 18 control `text_chars` and `blank` respectively. `rt text` and
`rt patterns` preserve the selected blank option, gap and cycle count. Entry or
configuration restart still establishes a full baseline; subsequent
cycles do not reset unless explicitly configured. Physical ghosting must be
assessed on the panel; waveform completion alone is not a quality verdict.

`rt set blank scrub` performs fast white → black → white → target for every draw,
synchronizing the previous-image RAM after each phase. This costs four waveforms
but tests the cleanup found more effective on the physical panel. `refresh scrub`
runs it once and restores the current framebuffer without advancing the bench.
Both manual cleanup commands preserve run/pause state. `blank on` retains the
two-waveform white → target method; `blank off` uses direct redraw.

Full refreshes now finish with fast white blanking and restoration by default:
this applies the user's observed workaround for faint fine text after full
reset. The underlying physical/controller cause is not yet proven. Bench menu
item 19 (`rt set post_full off`) disables this conditioning for A/B diagnosis;
it also allows the old `prime` probes to be tested without subsequent automatic
old-plane synchronization. Clock/demo full refreshes always condition the image.

- Weather requests use Open-Meteo and need no API key. Rust HTTPS validates the
  server using ESP-IDF's certificate bundle.
- The retained C++ demo calls `setInsecure()` and does **not** authenticate the
  weather server. Treat it as a reference/demo, not a secure production client.
- Wi-Fi credentials are stored on the board in NVS, not in the repository.
  NVS encryption is not enabled; serial access can change settings, and the
  serial setup channel is not encrypted. Do not publish NVS dumps or serial
  transcripts containing passwords.
- Weather data: [Open-Meteo](https://open-meteo.com/), licensed under
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Review the provider's
  [API terms](https://open-meteo.com/en/terms) before commercial deployment.
- The Rust clock uses content-aware byte-aligned hardware windows, with local
  hourly fast cleanup and six-hour fast scrub as described above.
- If the module is a different 3.7-inch revision, confirm the panel marking.
  This project specifically selects `GxEPD2_370_GDEY037T03`.
