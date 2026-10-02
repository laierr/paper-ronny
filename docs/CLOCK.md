# Clock/weather workload (Rust)

[Back to README](../README.md) · [Bench reference](BENCH.md)

The dashboard is a real-world test workload for the refresh routines, not a
claim of production reliability. Clock mode is currently the boot default.
`clock` returns from the bench; `rt` enters it. Network setup is optional for
bench use.

## Serial configuration

```text
set ssid My WiFi
set password my secret password
set latitude 32.0853
set longitude 34.7818
set timezone IST-2IDT,M3.4.4/26,M10.5.0
set units metric
set weather_minutes 15
save
connect
sync
weather
clock
```

`set` changes memory; `save` persists the complete configuration in NVS.
`show` prints settings with the password hidden. Latitude is -90..90, longitude
-180..180, units are `metric|imperial`, and weather interval is 5..180 minutes.
The example coordinates and POSIX TZ describe Tel Aviv; replace them for your
location. `set location <label>` is retained but no city label is rendered.
Rust and C++ configurations are separate; these instructions describe Rust.

## Page and data

The landscape page contains a large clock, date, separator, temperature with
C/F glyph, weather-condition glyph, humidity glyph/value, and moon-phase glyph.
Time/date are withheld until synchronized rather than showing 1970. Weather
can appear before time sync; the moon needs a valid local date.

Open-Meteo supplies current conditions and `daily=moon_phase`. Two days of moon
fractions are cached and matched to the clock's local date, then mapped to eight
icons. Missing/invalid phases are hidden; no local lunar approximation is used.
Detailed conditions, feels-like temperature, wind, and cached moon values are
available through `status`. Weather Icons attribution is in
[the assets directory](../assets/weather-icons/README.md).

Weather data by [Open-Meteo](https://open-meteo.com/), under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). API access is subject
to [the provider's terms](https://open-meteo.com/en/terms).

## Rendering and cleanup policy

Routine minute/date/weather/configuration changes use FastPartial, synchronized
previous-image RAM, and the smallest enclosing byte-level dirty window.
Unchanged frames trigger no waveform. Returning from the bench uses this path.

- New local hour: fast white → dashboard.
- Local 00:00, 06:00, 12:00, 18:00: fast white → black → white → dashboard instead.
- Full refresh: boot/unknown-state recovery, explicit `refresh`, and bench entry
  or configured bench baseline/full-cleanup operations—not minute counts.
- Clock full refreshes always use fast white blank/redraw conditioning afterward.

The schedule follows local calendar hours, not uptime. The first valid time arms
it without immediate cleanup. Delayed detection cleans the current hour once,
not every missed hour. Backward/repeated hours do not double-fire. Log messages
call this an hourly chime; there is no audio output.

Wi-Fi retries every 30 seconds in clock mode. Weather defaults to a 15-minute
interval; failures retain last-known values, mark them stale, and retry after
60 seconds. Rust HTTPS uses ESP-IDF's root certificate bundle. Bench mode defers
automatic weather fetches/Wi-Fi reconnect attempts and suppresses dashboard
redraws; manual network commands can still block while they execute.

## Soak procedure

Leave the board powered in `clock` mode and avoid manual refresh/bench commands
if testing the automatic policy alone. Serial may be disconnected; firmware
continues autonomously. `status` or `clock stats` reports uptime, automatic patch
count, hourly-white/six-hour-scrub counts, last cleanup, and free heap, plus
network/time/weather state. Counters reset on reboot and are not durable logs.

Record photographs and serial counters at intervals, including after hourly
cleanup. The policy is an experiment; see [lab notes](LAB_NOTES.md) for observed
results and limitations. Completion of a waveform is not proof of visual quality.
