# Paper Ronny

A serial-controlled e-paper refresh bench for experimenting with and
torture-testing screen update routines. The goal is to expose ghosting, missed
transitions, stale previous-image RAM, and corruption inside or outside updated
regions—not to certify a panel or driver as artifact-free.

The primary implementation is Rust/ESP-IDF, targeting an ESP32-D0WD-V3 and a
WeAct Studio 3.7-inch 240×416 monochrome module (`GDEY037T03`, `UC8253`).
The UI uses a **416×240 landscape canvas**. A clock/weather dashboard is included
as a real-world workload and long-running soak test; it is not the project's
main purpose.

The original [Arduino/C++ implementation](src/main.cpp) remains intact as a
reference for side-by-side comparison. The advanced bench is Rust-only.
Neither Wi-Fi nor weather configuration is needed to run screen tests.

## Documentation

- [Bench reference](docs/BENCH.md): every command, all 19 settings, defaults,
  accepted aliases, preset behavior, test sequences, and example experiments.
- [Lab notes](docs/LAB_NOTES.md): observed symptoms, investigations, workarounds,
  and unresolved questions—not universal hardware guarantees.
- [Clock workload](docs/CLOCK.md): optional network setup, rendering policy,
  local-hour cleanup, weather, and soak counters.
- [Third-party notices](THIRD_PARTY.md): font, dependency, and weather-data licenses.

Project code is [MIT licensed](LICENSE); third-party components retain their
own licenses.

## What the bench can exercise

- Consecutive digits and printable ASCII, large/small glyphs, text strings,
  geometric primitives, and spatially dithered shades.
- Fixed tiles, custom rectangles, deterministic sweeps/pseudorandom placements,
  a six-cell mosaic, and character-precise changes to a packed text page.
- Full-screen pattern and black/white transition sequences.
- Fast versus normal partial/full waveforms, previous-image synchronization,
  post-full RAM priming, and optional white or white/black/white cleanup.
- Finite or indefinite runs, configurable inter-draw gaps, periodic full
  refreshes, and serial pause/resume controls.

Logs report requested logical regions, actual byte-aligned controller windows,
area, and waveform duration. Visual inspection of the physical panel is still
required. Host tests cannot evaluate ghosting or prove a safe refresh cadence.
This is experimental firmware for this particular panel, not a general e-paper
driver or an unattended hardware qualification system.

## Wiring

Both devices use **3.3 V logic**.

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

Rust pin assignments are in `src/main.rs`; C++ assignments are in
`platformio.ini`. Confirm your panel marking before using this firmware on
another 3.7-inch module.

## Build and flash Rust

The original ESP32 is an Xtensa target. Install Rust/rustup, then the Espressif
Rust toolchain. See [espup's host prerequisites and setup instructions](https://github.com/esp-rs/espup#requirements)
for your OS. The project has been exercised on Windows; the Unix instructions
below follow upstream tooling but have not been tested on this laptop.

### Linux / macOS (POSIX shell)

Run from the repository root:

```sh
cargo install espup ldproxy espflash
espup install
. "$HOME/export-esp.sh"
cargo build --release --locked
espflash flash --baud 921600 --monitor --monitor-baud 115200 \
  target/xtensa-esp32-espidf/release/paper-ronny
```

Source the export file in every new shell (or use the path supplied to
`espup install --export-file`); it configures the toolchain environment, including
libclang. Omitting `--port` lets espflash detect/select a board. To select one
explicitly, add `--port /dev/ttyUSB0` (example Linux port) or
`--port /dev/cu.usbserial-XXXX` (example macOS port); use your actual device path.
On Linux, ensure your user has permission to access that serial device according
to your distribution's rules.

### Windows (PowerShell)

Windows also needs the C++ workload from Visual Studio Build Tools for
Microsoft's linker.

```powershell
cargo install espup ldproxy espflash
espup install
$env:LIBCLANG_PATH = "$env:USERPROFILE\.rustup\toolchains\esp\xtensa-esp32-elf-clang\esp-clang\bin"
cargo build --release --locked
espflash flash --port COM14 --baud 921600 --monitor --monitor-baud 115200 target\xtensa-esp32-espidf\release\paper-ronny
```

The explicit `LIBCLANG_PATH` line is a fallback for the current PowerShell session
if espup's Windows environment setup is not yet visible; adjust it for a custom
`RUSTUP_HOME`. Replace `COM14` with your board's port
(`espflash board-info` can detect it). The ELF executable above is the input
to espflash; no separate manually assembled binary is required.

### Reconnect to serial (either host)

Press Ctrl+R in the monitor if the board remains in the bootloader after flashing.
Ctrl+C releases serial without stopping the firmware. To reconnect:

```sh
espflash monitor
# Or explicitly: espflash monitor --port /dev/ttyUSB0
```

```powershell
espflash monitor --port COM14
```

Cargo uses the `esp-idf-sys` PlatformIO backend internally; do not invoke
`pio run` to build Rust. A compatibility-patched crate is vendored in
`third_party/esp-idf-sys`.

## Start a bench run

Serial is **115200 baud**, with newline-terminated commands. Boot currently
starts in clock mode, even without network setup. Enter `rt` to switch to
the bench:

```text
rt defaults
rt set cycles forever
rt
stop
rt set
```

`rt set` pauses testing and opens the numbered settings menu. Choose a setting,
enter its value, then use `0` at the menu list to exit and `run` to resume.
`rt show` prints the active configuration; `rt save` persists it.
For a more varied workload:

```text
rt set cycles forever
rt grid
stop
rt text
stop
rt patterns
```

Each entry establishes a full-refresh baseline. `stop` retains the test image;
`clock` returns to the dashboard. Clock redraws do not interfere with bench
mode, including while paused or after a finite run completes.
See the [bench reference](docs/BENCH.md) before comparing waveform settings:
some shortcuts and cleanup options intentionally override other settings.

## C++ reference

```sh
pio run
pio run -t upload
pio device monitor
```

Rust and C++ store configuration independently in NVS (`paper_ronny` versus
`paper-ronny`). Their basic serial setup vocabulary is similar, but Rust bench
commands are not supported by the reference firmware.

## Host checks

No ESP-IDF is required for these geometry, policy, and rendering checks. Run from
the repository root and explicitly select the **host** target: the root Cargo
configuration otherwise selects the ESP32 target, even with `+stable`.

Linux / macOS (derive the installed stable compiler's native target):

```sh
host_target="$(rustc +stable -vV | sed -n 's/^host: //p')"
cargo +stable test --locked --manifest-path scripts/clock-preview/Cargo.toml \
  --target "$host_target" --target-dir target/clock-preview-host
cargo +stable run --locked --manifest-path scripts/clock-preview/Cargo.toml \
  --target "$host_target" --target-dir target/clock-preview-host
```

Windows (PowerShell, x86-64 MSVC):

```powershell
cargo +stable test --locked --manifest-path scripts/clock-preview/Cargo.toml --target x86_64-pc-windows-msvc --target-dir target/clock-preview-host
cargo +stable run --locked --manifest-path scripts/clock-preview/Cargo.toml --target x86_64-pc-windows-msvc --target-dir target/clock-preview-host
```

The preview uses actual display code and writes `target/clock-preview.pbm`.
Checks cover window geometry, text-patch isolation, minute-update isolation,
cleanup boundaries, and moon-icon selection. Physical waveform quality is not
tested here.

## Security and operating limits

Rust weather HTTPS validates certificates. The retained C++ demo uses
`setInsecure()` and does **not** authenticate its weather server.
Wi-Fi passwords are stored on the board in unencrypted NVS; serial setup has
no authentication or encryption. Do not publish credential-bearing serial
transcripts or NVS dumps.

Unbounded runs and deliberately unsynchronized/incorrect waveform probes are
available for investigation. The firmware does not enforce a manufacturer-rated
refresh budget or automatically assess panel safety. Monitor experiments and
use `stop`, `refresh`, or reset when needed.
