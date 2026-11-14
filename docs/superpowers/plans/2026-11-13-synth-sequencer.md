# Synth Sequencer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A 16-step note sequencer owned by each Synth instance, driven by a global sample-accurate Transport shared with the Drum Machine.

**Architecture:** Extract the drum phase clock into `Transport`; extract the synth voice/param state out of `Engine` into `SynthInstrument`; `Engine` becomes a host that runs `Transport.run(...)` and sums instruments. `StepSequencer` is an event source that calls the owning synth's `VoiceManager` directly from the audio thread.

**Tech Stack:** Python, numpy, scipy, tkinter, pytest (existing). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-13-synth-sequencer-design.md`

## Global Constraints

- Audio callback: no allocations, no blocking, never raises (`Engine.errors` counts swallowed events).
- GUI never imports from the audio layer beyond `nomad.engine`; `nomad.engine` imports nothing from `gui`/`midi`.
- Sequencer emits note events only; it never writes audio.
- Existing public API keeps its signature: `Engine.note_on/note_off/set_param/set_control/panic/snapshot/render`, attributes `Engine.vm`, `Engine.drums`, `Engine.errors`, and `DrumMachine.{values,steps,playing,position,start,stop,toggle,stop_all,set_value,render(out, n)}`.
- Sample rate 48000, 16 steps, step = 16th note = `sr*60/bpm/4` frames; swing: even steps `*(1+swing)`, odd `*(1-swing)`.
- Whole existing suite (`pytest -q`) is green at the end of every task.

## Review Focus

- BPM/swing jump mid-step while a sequence plays: no skipped or doubled note (Task 1, 5).
- Live-held key equals the sequencer's note: sequencer gate-off must not cut the live note (Task 5).
- SEQ toggled off, transport stopped, or panic while a sequenced note sounds: no hung voice (Task 5).
- Render sizes that are not multiples of 256 (1, 37, 700): note fires at the exact frame (Task 4, 5).
- Empty pattern, pattern edited during play, note out of 0..127: silent / clamped, no exception (Task 5).

## File Structure

- Create `nomad/engine/transport.py` — clock + `run` driver.
- Create `nomad/engine/instrument.py` — `SynthInstrument` (voices, params, LFO, matrix, command queue, sequencer slot).
- Create `nomad/engine/sequencer.py` — `StepSequencer`.
- Create `nomad/gui/seq_view.py` — SEQ toggle + step editor.
- Modify `nomad/engine/engine.py` (host), `nomad/engine/drums.py` (use Transport), `nomad/gui/app.py`, `nomad/gui/help.py`, `README.md`.

---

### Task 1: Transport

**Files:**
- Create: `nomad/engine/transport.py`
- Test: `tests/test_transport.py`

**Interfaces:**
- Produces: `class Transport(sample_rate: float)` with attributes `bpm: float = 110.0`, `swing: float = 0.0`, `playing: bool`, `position: int` (0..15), `phase: float`; methods `start()`, `stop()`, `toggle()`, `frames_to_step() -> int` (>=1 when playing, `1<<30` when stopped), `run(n: int, render_span, on_step, limit=None) -> None`, `step_frames(step: int) -> float`.
- `run` contract: `render_span(start: int, chunk: int)` is called for consecutive spans covering `0..n`; `on_step(position: int)` is called when a step boundary is reached (after the span that ends on it) and once with `0` before the first span after `start()`; `limit() -> int` (optional) caps each span at that many frames (>=1). Phase is a fraction of the current step so bpm/swing jumps mid-step never skip a step.

- [ ] **Step 1: Write failing tests** in `tests/test_transport.py`: `test_step_frames_at_120bpm` (`step_frames(0) == 6000` at sr 48000), `test_swing_alternates` (swing 0.5 → even 9000, odd 3000), `test_run_fires_step0_on_start_then_every_6000_frames` (render_span records spans; on_step positions `[0, 1, 2]` after 12001 frames; spans never cross a 6000 boundary), `test_stopped_run_is_one_span_no_events`, `test_bpm_jump_mid_step_does_not_skip` (port of `test_drums.py::test_bpm_jump_mid_step_does_not_skip_steps`: 60 BPM, run 11040, set `bpm=180`, run 384 → `position == 1`), `test_swing_jump_mid_step_does_not_skip_steps` (port), `test_limit_caps_spans` (limit returns 100 → no span longer than 100).
- [ ] **Step 2:** `pytest tests/test_transport.py -v` → FAIL (module missing).
- [ ] **Step 3: Implement `Transport`** by moving the algorithm of `DrumMachine._duration` and the loop in `DrumMachine.render` (`nomad/engine/drums.py:294-374`) verbatim: `_pending_start`, `phase += chunk/dur`, wrap at `1 - 1e-9`, `position = (position+1) % STEPS`. Import `STEPS` from `drums`? No — define `STEPS = 16` in `transport.py` and have `drums.py` import it in Task 2 (keep `drums.STEPS` re-exported).
- [ ] **Step 4:** run the new tests → PASS; `pytest -q` still green.
- [ ] **Step 5:** `git add nomad/engine/transport.py tests/test_transport.py && git commit -m "feat(engine): extract sample-accurate Transport clock"`

### Task 2: DrumMachine runs on Transport

**Files:**
- Modify: `nomad/engine/drums.py` (`DrumMachine.__init__`, `start/stop/toggle/stop_all`, `_duration`, `render`, `set_value`)
- Test: `tests/test_drums.py` (existing, unchanged) + `tests/test_drums_shared_clock.py`

**Interfaces:**
- Consumes: `Transport` (Task 1).
- Produces: `DrumMachine(sample_rate, transport: Transport | None = None)`; `.transport`; `.position`/`.playing` become read-only properties of the transport; `.on_step(position: int)` (fires hits, replaces `_fire` use); `.render_chunk(out, start, chunk)` (public name of `_render_chunk`); `.render(out, n)` standalone = `self.transport.run(n, self.render_chunk_into(out), self.on_step)`; `set_value("bpm"|"swing")` writes both `values[...]` and `transport.bpm/swing`. Engine drives the shared transport instead (Task 4); in that mode `render` is not called.

- [ ] **Step 1: Write failing test** `test_two_drum_machines_share_one_transport`: two `DrumMachine(SR, transport=t)`; `t.start()`; render both through `t.run`; both fire step 0 on the same frame (hat steps set in both; the sums are equal). Also `test_dm_owns_transport_by_default` (`dm.transport is not None`).
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: Implement** per Interfaces. Delete `_phase`, `_pending_start`, `_duration` from `DrumMachine`. `stop_all()` = `transport.stop()` + `_fade_left = _fade_len`. Keep `FADE_MS`, regen throttling untouched.
- [ ] **Step 4:** `pytest tests/test_drums.py tests/test_drums_view.py tests/test_drums_shared_clock.py -v` → all PASS (the bpm/swing-jump tests are the safety net; do not edit them).
- [ ] **Step 5:** commit `refactor(drums): drive the step clock from Transport`.

### Task 3: Extract SynthInstrument from Engine

**Files:**
- Create: `nomad/engine/instrument.py`
- Modify: `nomad/engine/engine.py`
- Test: `tests/test_instrument.py`; existing `tests/test_engine.py`, `tests/test_main.py` unchanged.

**Interfaces:**
- Produces: `class SynthInstrument(sample_rate: float, max_voices: int = 8, name: str = "Synth 01")` owning everything the audio path of `Engine` owned for the synth: `vm`, `matrix`, `lfos`, `_values/_p/_ctl/_smoothers/_csm/_slots/_src/_mods`, command queue `_q`, `errors`; methods `note_on/note_off/set_param/set_control/panic/snapshot` (same semantics as today), `drain()`, `render_block(b: np.ndarray, n: int) -> None` (the per-control-block body of `Engine.render` that adds voices into `b`), `_resolve`. Also `self._live = bytearray(128)` marking keys held via note_on/note_off events (used in Task 5), and `self.sequencer = None` placeholder.
- `Engine(sample_rate, max_voices)` creates `self.synth = SynthInstrument(...)`, `self.drums`, `self.instruments = [self.synth]`; keeps `CONTROL_BLOCK`, `PITCH_SEMITONES` exports; delegates caller-thread API to `self.synth`; exposes aliases `vm`, `matrix`, `lfos`, `_p`, `_mods`, `_resolve`, `errors` (property returning `synth.errors`) so existing tests pass unmodified.

- [ ] **Step 1: Write failing test** `tests/test_instrument.py::test_two_instruments_are_independent`: two `SynthInstrument`s; `a.set_param("master_volume", ...)` / `a.note_on(60, 1)`; render both; `b` stays silent and `b.snapshot()["active_notes"] == []`; and `test_instrument_param_isolation` (`a.set_param("cutoff", x)` does not change `b.snapshot()["cutoff"]`).
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: Move code** from `engine.py` into `instrument.py` without behavior change (docstring invariants about smoothing/modulation move with it). `Engine.render` becomes: drain, loop control blocks calling `synth.render_block`, then drums, master gain ramp, `tanh` — still the old loop for now (Transport integration is Task 4).
- [ ] **Step 4:** `pytest -q` → all PASS, including untouched `test_engine.py`, `test_main.py`, `test_stream.py`.
- [ ] **Step 5:** commit `refactor(engine): extract SynthInstrument, Engine becomes host`.

### Task 4: Engine drives the shared Transport

**Files:**
- Modify: `nomad/engine/engine.py`, `nomad/engine/instrument.py`
- Test: `tests/test_engine_transport.py`

**Interfaces:**
- Consumes: Transport (T1), DrumMachine with injected transport (T2), SynthInstrument (T3).
- Produces: `Engine.transport` (one `Transport`, passed to `DrumMachine(sr, transport=...)`); `Engine.play()/stop_transport()/toggle_transport()` delegating to the transport; `Engine.render(out)` = drain queues, then `transport.run(total, self._span, self._on_step, limit=self._limit)` where `_span(start, chunk)` renders the chunk in `CONTROL_BLOCK` slices (synth `render_block`, `drums.render_chunk`, master ramp + `tanh`), `_on_step(pos)` calls `drums.on_step(pos)` and `inst.on_step(pos)` for each instrument; `SynthInstrument.on_step(pos)`, `.frames_to_event() -> int` (`1<<30` for now), `.advance(n)` (no-op for now), `.on_stop()` (no-op for now). Engine detects `playing` True→False between renders and calls `on_stop()` on instruments. `Engine.panic()` queues synth panic and calls `drums.stop_all()`.

- [ ] **Step 1: Write failing tests** `test_drums_and_transport_start_together` (`e.drums.toggle()` → `e.transport.playing`; first non-silent drum frame of step 0 at frame 0 when render size is 37), `test_render_sizes_not_multiple_of_256_keep_step_timing` (render `[1, 37, 700, 5262]` summing to 6000 → `transport.position == 1`, equal to rendering `[6000]`), `test_panic_stops_transport_and_drums`.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: Implement** per Interfaces. Preserve the master gain ramp per slice and the `tanh` limiter. Keep preallocated scratch only.
- [ ] **Step 4:** `pytest -q` → all PASS (drum + engine suites unchanged).
- [ ] **Step 5:** commit `feat(engine): host drives shared Transport, sample-accurate step splits`.

### Task 5: StepSequencer

**Files:**
- Create: `nomad/engine/sequencer.py`
- Modify: `nomad/engine/instrument.py`, `nomad/engine/engine.py`
- Test: `tests/test_sequencer.py`

**Interfaces:**
- Consumes: `SynthInstrument.vm`, `._live` (T3), Transport callbacks (T4).
- Produces: `class StepSequencer(synth: SynthInstrument, sample_rate: float, transport: Transport)` with arrays `on: uint8[16]`, `note: uint8[16]` (default 60), `vel: uint8[16]` (default 100; accent 127), attributes `enabled: bool = False`, `gate: float = 0.5` (0.05..1.0, clamped, NaN → 0.5), and methods `set_step(step: int, on: bool, note: int = None, vel: int = None)` (clamps note 0..127, vel 1..127, step 0..15), `clear()`, `on_step(pos)`, `frames_to_off() -> int`, `advance(n)`, `release()`. Behavior: on step with `enabled` and `on[pos]`: release the previous sequencer note, `vm.note_on(note, vel/127)`, arm gate-off after `gate * transport.step_frames(pos)` frames (at least 1); `advance(n)` counts down and calls `_off()`; `_off()` skips `vm.note_off` when `synth._live[note]` is set; `release()` always offs the held sequencer note. `SynthInstrument.sequencer` is a `StepSequencer`; the instrument's `on_step/frames_to_event/advance/on_stop` delegate to it; `panic` and `enabled=False` (via property setter calling `release()`) release it.

- [ ] **Step 1: Write failing tests**, 48 kHz, 120 BPM (6000 frames/step): `test_note_on_at_exact_frame` (step 0 `on`, note 64; render `[1, 37, 5962]`; voice for 64 active after first call), `test_gate_off_after_gate_fraction` (gate 0.5 → `vm.active_notes()` empty after 3000 frames, note released 3000 frames after step start; render split `[2999, 1]` shows held then released), `test_two_steps_retrigger_legato_gate_1`, `test_empty_pattern_is_silent`, `test_disabled_sequencer_is_silent_and_releases` (toggle `enabled=False` mid-note → `active_notes() == []` after render), `test_stop_releases_note`, `test_panic_releases_note`, `test_live_key_not_cut_by_gate_off` (live `note_on(64)` via `synth.note_on` while seq plays 64 → still in `active_notes()` after the gate-off), `test_midi_and_sequence_play_together` (live 48 held + seq 64 → both active), `test_bpm_jump_mid_step_no_skipped_or_double_note` (count `note_on` calls over 4 steps with a mid-step `bpm` jump), `test_out_of_range_inputs` (`set_step(99, ...)`, `note=300`, `gate=nan` → clamped, no exception), `test_no_allocation_in_step_path` (tracemalloc snapshot diff around 1000 `on_step`/`advance` calls shows no growth beyond a small constant).
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: Implement** per Interfaces; wire `Engine._limit` to `min(inst.frames_to_event())`, and `Engine._span` to call `inst.advance(chunk)` after rendering each chunk.
- [ ] **Step 4:** `pytest -q` → all PASS.
- [ ] **Step 5:** commit `feat(engine): per-synth StepSequencer driven by Transport`.

### Task 6: GUI — SEQ toggle and step editor

**Files:**
- Create: `nomad/gui/seq_view.py`
- Modify: `nomad/gui/app.py` (`_build`: place `SeqView` in `synth_view` below the matrix, `_poll`: `seq_view.refresh()`), `nomad/gui/help.py`, `README.md`
- Test: `tests/test_seq_view.py`, `tests/test_help.py` (existing, must stay green)

**Interfaces:**
- Consumes: `engine.synth.sequencer`, `engine.transport`, `engine.toggle_transport()`.
- Produces: `class SeqView(parent, sequencer, transport, register_help=None)` with `toggle_enabled()`, `toggle_step(step: int, accent: bool = False)`, `set_note(step: int, delta: int)`, `refresh()`; widgets: `SEQ` button (on/off), `PLAY/STOP` button (transport), `GATE` control, 16 step buttons (label = note name when on; playhead highlight like `DrumView`), mouse wheel over a step = ±1 semitone (shift = ±12), shift+click = accent (vel 127). Help keys `seq_toggle`, `seq_steps`, `seq_gate` added to `help.py` with texts in the same language/style as the drum help entries.

- [ ] **Step 1: Write failing tests** (pattern of `tests/test_drums_view.py`): `test_toggle_step_writes_sequencer`, `test_wheel_changes_note_and_clamps_0_127`, `test_accent_sets_vel_127`, `test_seq_button_toggles_enabled`, `test_play_button_toggles_shared_transport`, `test_refresh_highlights_playhead_only_while_playing`.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3: Implement** `SeqView` and wire it in `app.py`; no direct audio access beyond sequencer/transport attribute stores.
- [ ] **Step 4:** `pytest -q` → all PASS (including `test_help.py` coverage of new keys and `test_layout.py`).
- [ ] **Step 5:** manual check: `python -m nomad.main`, enable SEQ, add steps, PLAY; play keys while it runs; start DRUMS PLAY — one transport. Commit `feat(gui): SEQ toggle and step editor in the synth panel`.

---

## Self-Review notes

- Spec coverage: Transport (T1/T2/T4), SynthInstrument (T3), sequencer events through the same VoiceManager (T5), MIDI+seq simultaneously (T5 test), global PLAY (T4/T6), panic/stop (T4/T5), GUI toggle in-panel (T6), compatibility (T3 keeps old tests unmodified). Out of scope per spec: dynamic tabs, second synth in UI, chords, persistence.
- Call made beyond the spec: gate is one value per sequencer (not per step); per-step is note + velocity.
- Known limit: `DrumMachine.values["bpm"]` mirrors `Transport.bpm`; only the drum knobs write it today.
