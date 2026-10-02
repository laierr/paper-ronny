# Rust refresh bench reference

[Back to README](../README.md) · [Lab notes](LAB_NOTES.md) · [Clock workload](CLOCK.md)

This reference describes the implemented serial parser and runner in
`src/main.rs`, defaults in `src/config.rs`, and rendering/geometry in
`src/display.rs` and `src/bench_geometry.rs`. The C++ reference does not implement
this bench. All coordinates below are logical landscape coordinates unless
explicitly called native/controller coordinates.

## Control commands

| Command | Effect |
|---|---|
| `rt`, `refresh test` | Enter bench, establish a full baseline, start the current configuration from its first item. |
| `rt 1` … `rt 6`, `refresh test 1` … `refresh test 6` | Load one preset and enter/start it. See retained settings below. |
| `rt all`, `refresh test all` | Run presets 1→6; each stage establishes a baseline. Controls remain available between draws. |
| `rt grid` | Set region to grid and enter/start; other settings remain selected. |
| `rt text` | Set region to textpage, mode fast, sync on, full_every off; enter/start. |
| `rt patterns` | Set region to patterns, mode fast, sync on, full_every off; enter/start. |
| `stop`, `rt stop` | Pause; keep bench mode, sequence position, and image. No effect on clock mode. |
| `run`, `rt run` | Resume an unfinished bench immediately. Restart item/cycle/draw counters if a finite sequence finished. From clock mode, enter the current bench with a full baseline. |
| `clock` | Return to dashboard mode. Does not save settings. |
| `rt show` | Print current bench configuration, not a durable progress report. |
| `rt set` | Pause active bench and open the numbered settings menu. |
| `rt set <key> <value>` | Change a setting in memory. See table and aliases below. |
| `rt defaults` | Restore all bench defaults in memory and reset sequence configuration; does not erase Wi-Fi/clock settings. |
| `rt save` | Save the complete settings object, including Wi-Fi/clock settings, to NVS. |
| `save` | Also saves the complete settings object. |
| `refresh` | Full waveform on current image/mode; bench uses its full_mode/prime/post_full settings. Does not advance sequence. |
| `refresh fast` | Whole-screen FastPartial white → saved frame, synchronized after each phase. |
| `refresh scrub` | Whole-screen FastPartial white → black → white → saved frame, synchronized after each phase. |
| `help` | Print serial command summary. This reference includes aliases/interactions beyond that summary. |
| `status`, `clock stats` | Network/time/weather and clock soak statistics; not bench visual-quality measurements. |
| `reboot` | Restart board. Saved settings reload, but boot mode is clock. |
| `erase` then `erase YES` | Erase the complete saved Rust configuration and reboot. Confirmation is case-sensitive. |

Serial uses 115200 baud and newline-terminated commands. Use lowercase commands
inside the menu. Outside the menu, command matching and setting keys/enum values
are generally case-insensitive.

`display test` is a separate two-second static test-page command, **not** a bench
stage: it displays the page and restores a dashboard image. It does not switch
the current app mode or preserve an active bench image. Use `rt` commands for
controlled bench experiments.

## Menu, editing, persistence, and state

Enter `rt set`, type a setting number (1..19), then its value. Successful edits
return to the setting list. Invalid values leave the value prompt open and do
not change the configuration. `0` exits only at the list; at a value prompt,
zero is a value and is validated normally. `cancel` exits from either prompt.
Closing the menu does not resume a paused bench; use `run`.

Lowercase `run`, `stop`, `rt run`, `rt stop`, `clock`, `refresh`, `refresh fast`,
`refresh scrub`, `rt save`, `rt show`, and `help` exit the menu and execute.
Other commands are not menu escape commands: use `cancel` before entering them.

Any successful setting edit while in bench mode resets item/cycle/draw counters,
cancels a preset suite, and marks the baseline for full reconstruction on the
next running tick. Direct `rt set <key> <value>` **does not pause** a running
bench; use `stop` first if you want several edits applied before restarting.
The menu pauses first. Changes are atomic: rejected edits retain old settings.

Entry with `rt` and shortcuts always creates a baseline. Resuming an unchanged
paused bench does not. `run` after finite completion resets counters but does
not itself force a fresh full baseline. Completed runs stay in bench mode with
the last image visible; no clock/weather redraw replaces it.

Only `save`/`rt save` persist changes. NVS stores configuration, not current
framebuffer, sequence position, running state, or counters. Old saved settings
can differ from defaults. Rust and C++ NVS namespaces are independent.

## All settings

The number column is the exact `rt set` menu order. Defaults below apply after
`rt defaults` or with no saved configuration—not necessarily after a shortcut.

| # | Key | Accepted values / default | Meaning |
|---|---|---|---|
| 1 | `region` | `preset`, `custom`, `sweep`, `random`, `grid`, `textpage`, `patterns`; default `preset` | Placement/workload strategy. `text` aliases `textpage`. |
| 2 | `rect` | Four unsigned integers `x y width height`; initial stored values `8 24 192 200` | Set geometry atomically and select `custom`. Commas and/or whitespace separate values. Rectangle must fit. |
| 3 | `x` | 0..415; default 8 | Stored custom left edge; does not select custom automatically. |
| 4 | `y` | 0..239; default 24 | Stored custom top edge; does not select custom automatically. |
| 5 | `width` | 1..416; default 192 | Custom/sweep/random width; must fit with x in custom mode. |
| 6 | `height` | 1..240; default 200 | Custom/sweep/random height; must fit with y in custom mode. |
| 7 | `content` | `digits`, `all`, `text`, `shapes`, `shades`, `mixed`; default `digits` | Generic patch content. Key alias `glyphs`. Specialized regions override it. |
| 8 | `size` | `large`, `small`; default `large` | Glyph/font scale; also chooses the preset tile. Does not resize a custom rectangle. |
| 9 | `time` | Integer 0..60 seconds; default 1 | Gap **after** a draw completes. Zero removes intentional gap, not waveform/loop overhead. |
| 10 | `cycles` | Integer 0..100000; default 1 | Complete sequence repetitions. 0 means unlimited; aliases `forever`, `infinite`, `inf`. |
| 11 | `mode` | `fast`, `normal`; default `fast` | Direct update waveform: FastPartial or Partial. Overridden by blank-before/full cleanup. |
| 12 | `sync` | `on`, `off`; default `on` | After direct partial redraw, copy updated patch into previous-image RAM. Key alias `baseline`. |
| 13 | `full_every` | Integer 0..65535; default 0/off | Full refresh every N target draws, starting at draw N. 0 disables periodic full refresh, not entry/recovery full refreshes. Key alias `full`. |
| 14 | `full_mode` | `fast`, `normal`; default `fast` | FastFull versus Full for bench baseline, scheduled full refresh, and manual `refresh` in bench mode. |
| 15 | `prime` | `old`, `both`, `none`; default `old` | RAM writes after full waveform: old plane only, current plus old, or neither. Full pre-waveform writes still seed both planes. |
| 16 | `grid_update` | `cell`, `screen`; default `cell` | Update one grid cell per draw, or all cells in one whole-screen transfer. `all` aliases `screen`. Only meaningful for grid. |
| 17 | `text_chars` | Integer 1..69; default 8 | Characters replaced per textpage draw; clamped to available columns. Only meaningful for textpage. |
| 18 | `blank` | `off`, `on`, `scrub`; default `off` | Direct redraw, whole-screen white→target, or whole-screen white→black→white→target. All blank phases force FastPartial and synchronization. |
| 19 | `post_full` | `on`, `off`; default `on` | After every bench full waveform, apply whole-screen fast white blank/redraw conditioning. Turn off to isolate raw full-to-partial/priming behavior. |

Additional accepted value aliases:

- `size`: `l` = large, `s` = small.
- `mode`: `f`, `fastpartial` = fast; `n`, `partial` = normal.
- `full_mode`: `f` = fast, `n` = normal.
- `sync`: `yes`, `true`, `1` = on; `no`, `false`, `0` = off.
- `content`/`glyphs`: `d`, `digit` = digits; `a` = all.
- `full_every`/`full`: `off`, `never`, `none` = 0.
- `blank` and `post_full`: `true`, `1` = on; `false`, `0` = off.
  Unlike `sync`, these two do not accept `yes`/`no`.

There is no setting for arbitrary supplied text, random seed, pattern pitch,
individual grid-cell dimensions, or arbitrary per-draw waveform sequences.
Those are currently fixed in source.

## Geometry and regions

Origin is top left; canvas is 416 wide × 240 high. Custom rectangles require
`x + width <= 416` and `y + height <= 240`. Use `rect` when multiple dimensions
must change together; one-field edits are validated against the other fields.
All stored geometry is validated even when its fields are ignored by a region.

| Region | Placement and baseline |
|---|---|
| `preset` | Large tile `(8,24,192,200)` or small tile `(232,64,176,112)`, chosen by size; reference test page. Stored x/y/width/height ignored. |
| `custom` | Fixed stored rectangle; white page with reference marks. |
| `sweep` | Stored width/height, moving through 5 columns × 3 rows including canvas edges/center. Position advances per draw, not per cycle; x/y ignored. |
| `random` | Stored width/height, deterministic pseudorandom position derived from draw count. Repeatable after counter reset; x/y ignored. |
| `grid` | Six fixed cells shown below; initial content at step 0. Generic size/content and stored geometry ignored. |
| `textpage` | Packed monospace page; deterministic row/column spans replaced. Size/text_chars select patch geometry. Stored geometry/content/grid_update ignored. |
| `patterns` | Whole 416×240 canvas; initial white baseline. Stored geometry, size, content, grid_update, and text_chars ignored. |

Moving tests leave previously drawn tiles in place. Each selected rectangle is
cleared and redrawn, but pixels outside it are preserved—even if a previous tile
partly overlapped it. Glyphs are clipped to their rectangles, not auto-fit.

Rotation maps logical Y to native controller X, whose window is byte-aligned.
The transferred native window can therefore be larger than the requested
rectangle. Expanded edge pixels are copied from the framebuffer unchanged.
Native window logs use inclusive endpoints; canvas/native coordinates are not
interchangeable.

## Content and cycle lengths

For preset/custom/sweep/random regions:

| Content | One cycle | Rendering |
|---|---:|---|
| `digits` | 10 draws | Consecutive seven-segment 0..9, not isolated one-shot glyphs. |
| `all` | 94 draws | Printable ASCII `!` through `~` (33..126), excluding space. |
| `text` | 8 draws | Fixed changing sample strings; not character-precise page patches. |
| `shapes` | 6 draws | Filled/outlined rectangle, circle, triangle with diagonal. |
| `shades` | 17 draws | 0..100% black coverage in 1/16 increments, ordered 4×4 dither. Not native grayscale. |
| `mixed` | 376 draws | Interleaved digits, ASCII, shapes, shades; each substream wraps at its own length. |

Preset digits use 100×160 large or 56×90 small glyphs; generic patch digits use
100×160 large or 20×32 small. Preset ASCII uses FONT_10X20 scaled 7× large or
4× small; generic ASCII uses 7× large or 1× small. Text strings use unscaled
10×20 large or 6×10 small fonts. Shapes/dither use region geometry, not size.

The eight sample strings, in order, are `Paper Ronny`, `WiFi connected`,
`0123456789`, `The quick brown fox`, `BLACK -> WHITE`, `thin strokes / dots`,
`Time 12:34:56`, and `Weather 27 C`. They are test data, not live device status.

Specialized cycle lengths: grid cell mode 564 draws (94 rounds of six cells),
grid screen mode 94 draws, textpage 94 draws, patterns 14 draws. A draw is one
target operation—not one waveform: blank/scrub/post-full can add waveforms.
Draw count continues across cycles; sequence item resets each cycle. `full_every`
counts target draws, including across cycles, not intermediate cleanup phases.
Preset-suite transitions reset counters for each stage.

### Grid

| Cell | Rectangle (x y width height) | Content |
|---|---|---|
| 1 | 4 4 128 232 | Large seven-segment digit |
| 2 | 136 4 276 44 | Large-font sample strings |
| 3 | 136 52 132 132 | Geometric primitives |
| 4 | 272 52 140 132 | Dither coverage |
| 5 | 136 188 132 48 | Small-font sample strings |
| 6 | 272 188 140 48 | Black → text → white → text density transitions |

Cell mode rotates through cells 1..6; screen mode advances all six together and
transfers the whole framebuffer. Screen mode with fast waveform is whole-screen
**FastPartial**, not FastFull. Baseline uses step 0; updates begin at step 1.

### Packed text

Small font yields 69 columns × 24 rows of 6×10 cells; large yields 41 × 12 of
10×20 cells. Each draw replaces only `text_chars` consecutive character cells.
For large font, a requested 69 is clamped to 41. Right-edge remainder stays white.
Rows/columns in logs are zero-based. Patch position derives from total draw
count; replacement characters cycle through printable ASCII. This is different
from `content text`, which clears its whole chosen rectangle each draw.

### Full-screen patterns

Each cycle uses these 14 targets, with a 16-pixel line pitch:

1. Black vertical lines on white.
2. Black horizontal lines on white.
3. Black grid on white.
4. Solid black.
5. White grid on black.
6. Black grid on white (`invert 1`).
7. White grid on black (`invert 2`).
8. Solid white.
9. 50% checkerboard dither.
10. Solid black over dither.
11. Solid white after black.
12. Sparse circle/diagonals on white.
13. Solid black over shapes.
14. Solid white.

Inversion stages are fixed target images, not an invert-current-buffer command.
There is no repeated-identical-frame second pass. The next cycle begins directly
with vertical lines; it does not full-refresh unless full_every requires it.

## Presets and shortcuts: what is retained

| Preset | Size | Partial mode | Sync | Gap | Sequence |
|---|---|---|---|---:|---|
| 1 | large | fast | on | 1 s | 0..9 |
| 2 | small | fast | on | 5 s | 0..9 |
| 3 | large | normal | on | 1 s | 0..9 |
| 4 | small | normal | on | 5 s | 0..9 |
| 5 | large | fast | off | 1 s | 0..9 |
| 6 | large | normal | off | 3 s | 0..9 |

Each preset sets region=preset, content=digits, cycles=1, plus the table's
size/mode/sync/time. It **retains** full_every, full_mode, prime, blank,
post_full, stored geometry, grid_update, and text_chars. Thus blank on can mask
the intended unsynced probe in presets 5/6. Use `rt defaults` first for a known
comparison and `rt show` after entry. `rt all` likewise retains these controls.

`rt grid` changes region only. `rt text` and `rt patterns` additionally force
mode=fast, sync=on, full_every=off; they retain gap, cycles, blank, full_mode,
prime, post_full, and other settings. To test normal/unsynced text or patterns,
set the region explicitly and then use `rt`, or stop/edit/run after entry.

## Waveform precedence and important interactions

Each target draw chooses one path, in this order:

1. If full_every divides the target draw count, use full_mode/prime/post_full.
2. Otherwise, if blank is on/scrub, use whole-screen FastPartial cleanup phases
   and target redraw, synchronized after every phase.
3. Otherwise, transfer the selected byte-aligned window with mode and sync.

Consequences:

- `blank on` overrides `mode normal`, `sync off`, and the physical update area:
  even a one-character logical patch causes two whole-screen waveforms.
- `blank scrub` similarly causes four whole-screen waveforms. Neither option
  changes unaffected framebuffer pixels in the restored target.
- `full_every off` does not suppress full entry/configuration baselines or
  manual `refresh`.
- `post_full on` adds white→frame FastPartial conditioning after full refresh.
  It re-synchronizes RAM and may mask the effect of prime none/both/old probes.
  Use post_full off and blank off when isolating priming problems.
- `prime none` does not mean no RAM seeding at all: both planes are written
  before the full waveform; only post-waveform priming is omitted.
- Normal partial or sync off are diagnostic probes, not endorsed reliable modes.
- `refresh fast`/`refresh scrub` always use whole-screen fast synchronized
  phases, independent of bench mode/sync/blank. They preserve the saved target
  and run/pause state, and restart the inter-draw gap after completion.
- Manual `refresh` preserves run/pause state but schedules the next running
  draw immediately. It uses the current bench full-refresh options.

Fast cleanup assumes the software framebuffer represents the visible image.
If a probe corrupted the physical state, that assumption may be false; plain
`refresh` is the full-waveform recovery tool. Cleanup helpers seed previous-image
RAM from the saved previous framebuffer, not the newly rendered target.

## Timing, controls, and logs

`time` starts after the complete target operation (including all cleanup phases).
The runner polls roughly every 200 ms; gap 0 is not a fixed-rate or zero-latency
loop. Waveforms are synchronous. `stop` is processed between operations, not
mid-waveform; it does not wait for the configured gap. Manual network commands
can also block controls while their requests execute.

Bench mode suppresses automatic clock/time/weather-triggered **screen redraws**,
even when stopped/completed. Automatic weather fetching and Wi-Fi reconnect
attempts are deferred; SNTP can still run/retry. Manual network commands do not
convert bench mode into clock mode.

Logs include cycle number, target label/index, requested rectangle, native
inclusive window, transferred pixel area/percentage, and waveform duration.
For blank/full paths, the initially logged region is the logical target region;
the later WHOLE-screen/full-cleanup labels describe the actual waveform area.
Host tests validate geometry/pixel preservation but cannot observe panel residue.

## Example experiments

### Moving unaligned patch

```text
stop
rt defaults
rt set rect 13 7 160 120
rt set region random
rt set content mixed
rt set cycles forever
rt set time 1
rt
```

Defaults already select fast, sync on, blank off, and full_every off. For a sweep,
stop, change region to sweep, then run. For fixed whole-screen generic content,
use `rt set rect 0 0 416 240` and `rt`.

### Character isolation

```text
stop
rt defaults
rt set size small
rt set text_chars 1
rt set cycles forever
rt text
```

Photograph neighboring text before/after a patch; check outside-window pixels,
not just the replacement string.

### Direct versus white blank versus scrub

```text
stop
rt defaults
rt patterns
rt set blank on
rt patterns
rt set blank scrub
rt patterns
```

Each default one-cycle run finishes with white. Inspect intermediate targets too,
especially black over dither. All three runs start with a fresh full baseline.
Keep full_every off so periodic full refresh does not confound the comparison.

### Raw full-to-partial transition

```text
stop
rt defaults
rt set content all
rt set size small
rt set post_full off
rt set full_every 10
rt set prime old
rt
```

Repeat with prime both/none and full_mode normal, changing one factor at a time.
Compare post_full on as a separate workaround test. Record complete settings,
panel identity, photographs, and logs; see [lab notes](LAB_NOTES.md).
