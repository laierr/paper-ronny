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
refresh test            run all six 0..9 partial-refresh experiments
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
erase                    erase saved settings after confirmation
reboot                   restart the ESP32
```

Commands and detailed validation errors appear only on serial. After Wi-Fi is
configured, the firmware reconnects automatically, synchronizes time with NTP,
fetches current conditions from Open-Meteo, and refreshes the clock once per
minute. Weather is refreshed every 15 minutes by default.

## Notes

- Weather requests use Open-Meteo and need no API key. This demo disables TLS
  certificate validation to avoid maintaining a root CA on the device; add CA
  validation before using the networking code in a security-sensitive product.
- The Rust clock uses a byte-aligned hardware window for minute updates. It
  performs a full cleanup after 10 clock patches and whenever date, weather, or
  configuration content changes.
- If the module is a different 3.7-inch revision, confirm the panel marking.
  This project specifically selects `GxEPD2_370_GDEY037T03`.
