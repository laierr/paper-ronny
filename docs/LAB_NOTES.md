# E-paper refresh lab notes

[Back to README](../README.md) · [Bench reference](BENCH.md) · [Clock workload](CLOCK.md)

These notes separate physical observations from implemented behavior and working
hypotheses. They summarize development testing reported by the operator on one
WeAct 3.7-inch GDEY037T03/UC8253 module with an ESP32-D0WD-V3. They are not a
controlled qualification report, proof of lifetime safety, or a guarantee for
other panels, firmware versions, temperatures, or power conditions.

Documentation snapshot: 2026-10-02. There is no complete timestamped image/log
dataset for the earlier experiments, and the multi-day clock soak has not been
documented as completed.

## Reference behavior and initial symptoms

The retained C++/GxEPD2 implementation provided a useful comparison: the operator
reported clean, seamless changes to a chunky minute digit with no noticeable
ghosting, residue, or flicker. The original Rust path did not match that behavior.

Early Rust symptoms included retained test-page imagery, substantial ghosting,
faint or apparently white-on-white first draws after blanking/full reset, and
corruption in subsequently affected pixels. Some partial probes appeared to show
only even digits or redraw the preceding digit. Sequential 0..9 tests were added
because a single glyph did not expose the transition dependence.

These observations motivated explicit old/current image-plane handling and
tests that keep controls responsive rather than blocking through an entire run.
The clock's old unsynchronized date/time behavior was also removed, but network
and layout fixes are separate from proving display-waveform correctness.

## Full-to-partial path investigation

The implemented full path was changed to write both planes through an explicit
full-sized RAM window, trigger FastFull outside partial-window mode, then restore
the window and prime old RAM. The panel/controller reinitialization is handled
through epdsi. GxEPD2's behavior was used as the comparison, not evidence that all
remaining physical symptoms were explained.

After improvements, the operator reported that fast synchronized large-region
updates could run hundreds of redraws without significant added noise/ghosting.
However, the first fine-text draw after full reset could still appear faint,
with artifacts on subsequent partial changes. That distinction matters: fixing
RAM/window sequencing did not establish a complete root cause for the fine-text
full-reset transition.

### Current workaround

`refresh fast` (whole-screen white → saved frame, FastPartial with old-plane
synchronization at each phase) was reported to fix the faint-text symptom.
Full refreshes now finish with that conditioning by default.

For diagnosis, `post_full off` removes conditioning; `prime old|both|none` and
`full_mode fast|normal` remain available. With conditioning enabled, later
fast writes re-synchronize planes and can conceal differences between raw
priming probes. Keep blank off when isolating this transition too.

The precise physical/controller cause of the remaining post-full faint-text
behavior is **not proven**. Do not describe this workaround as a resolved hardware
root cause.

## Fast redraw area and content experiments

Testing expanded from two fixed tiles to arbitrary rectangles, deterministic
moving windows, six differently shaped grid cells, packed monospace text, and
whole-screen target patterns. This helps distinguish corruption of the selected
pixels from damage to neighboring content or loss of previous-image state.

The operator reported that whole-screen fast redraw worked very well. Slight
ghosting was more evident when drawing over regions previously containing many
black pixels. Solid black over existing shapes was problematic, with black over
dither identified as a particularly useful stress case.

These are content-dependent observations on the tested panel—not a numerical
safe-area threshold. No measured maximum safe fast-update percentage, cadence,
or lifetime refresh count has been established. The bench reports actual
transferred area so future comparisons can record it.

### Cleanup comparisons

- White blank → target was observed to improve problematic redraws, including
  faint text after full reset.
- A rapid white → black → white sequence was reported to clean residual ghosting
  more effectively; the bench adds target restoration as the fourth phase.
- Redrawing an identical frame gave only a small improvement in the operator's
  testing. The repeated-identical-frame second pass was removed from the pattern
  sequence rather than retained as a default cleanup strategy.
- Frequent full waveforms did not appear necessary for the tested fast/synced
  workloads. This is a rationale for experimentation, not a manufacturer-backed
  recommendation to eliminate periodic full recovery everywhere.

The current 14-target pattern sequence explicitly probes black over dither and
black over sparse shapes. Compare blank off/on/scrub with full_every off so a
periodic full waveform does not confound the cleanup comparison. All run entries
still establish a full baseline, which should be noted in results.

## Packed-text testing correction

The first text approach cleared a large rectangle for each replacement, deleting
too much existing text to be useful as a character-isolation test. The textpage
workload now fills the screen with monospace text and replaces selected whole
character cells. Controller byte alignment preserves expanded neighboring
pixels from the framebuffer.

Host checks confirm neighboring pixels are unchanged in software. This does not
prove that the physical panel leaves them unchanged during a waveform; compare
photographs around patch boundaries as well as the target cells.

## Clock as a long-running workload

The optional dashboard exercises content-aware patches during routine minute,
date, and weather changes. Its experimental cleanup schedule uses white→frame
at local-hour boundaries and white→black→white→frame every sixth local hour.
Full refresh is reserved for explicit/recovery/baseline paths instead of a
fixed partial-count budget. This lets the app accumulate realistic transitions
without hiding them behind frequent full resets.

The purpose of the soak is to learn whether artifacts accumulate over days;
there is no recorded completed multi-day result here. Counters are RAM-only and
reset on reboot. Manual cleanup or bench entry interrupts the policy-only
experiment and should be recorded as such.

## Recording the next experiment

Record at least:

- Panel marking/revision, board, wiring/power source, and firmware commit.
- Ambient conditions if known; do not assume temperature-independent behavior.
- Full `rt show` output after entering the workload (shortcuts change settings).
- Whether the baseline used post_full conditioning and which prime/full mode.
- Draw labels/counts, cycle, gap, logical/native window, and waveform timings.
- Photos before/after the suspect transition, including neighboring pixels.
- Any stop/run, refresh, reset, network command, or settings edit during the run.
- Whether the observed result is repeatable from a fresh baseline and whether
  it survives a change of glyph/content/area.

Compare one factor at a time. Intentionally unsynchronized probes may produce
bad images by design; waveform completion is not a pass condition. Use full
`refresh` when the visible image no longer matches the software framebuffer,
then re-enter the workload for a clearly identified baseline.

## Open questions

- Why does raw full-reset → fine-text partial rendering sometimes look faint?
- How sensitive are ghosting/cleanup results to temperature, power, and panel
  revision?
- Does content-aware fast redraw plus scheduled cleanup remain acceptable over
  several days without manual intervention?
- Which transition/content/area combinations benefit from white-only cleanup
  versus white/black/white, and at what waveform/time cost?
- Can future image/log captures quantify residue and outside-window changes
  rather than relying only on visual descriptions?
