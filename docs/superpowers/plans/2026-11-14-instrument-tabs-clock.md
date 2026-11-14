# Instrument Tabs & Shared Clock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Host several independent Synth / Machine Drum instances behind a tab bar, mixed into one master, with a shared `Clock` that instances can follow.

**Architecture:** `Clock` (beats, float64) replaces `Transport`; instruments derive steps from `clock.pos` through a stateless `StepGrid`. `Engine` becomes a mixer over an immutable `Topology` adopted through a single FIFO command queue; `InstrumentManager` (GUI thread) is the only writer of the graph and publishes snapshots. GUI gets one persistent panel per instance and a `TabBar`.

**Tech Stack:** Python 3.14, numpy, scipy, tkinter, pytest (existing). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-14-instrument-tabs-clock-design.md` (approved with four precisions). **Prerequisite:** branch `fix/separate-play` (independent PLAY) is committed and merged to `main` before Stage 1; Stage 1 replaces what that branch changed in `engine.py`/`transport.py`.

## Global Constraints

- Audio callback: no allocations, no blocking, never raises (`Engine.errors`/`inst.errors` count swallowed events). Freed objects return to the GUI thread (graveyard), never dropped in the callback.
- **One writer per field** (spec table): class A audio-owned, class B GUI-published atomic stores, class C commands through the FIFO. No new GUI writes to audio-owned fields.
- Clocks advance **exactly once per render chunk**, only clocks of Internal instances ("roots"). Followers never own time.
- Followers: one-to-many allowed; chains rejected; source switch is immediate (jump / repeated or skipped step accepted and documented).
- `master_volume` keeps its meaning (global mix volume, `Engine.set_param/snapshot`, `--volume`, MIDI-learn names). `level` is new, per instance, 0..1, default 1.0.
- GUI never imports from `nomad.audio`/`midi` internals beyond what it does today; `nomad.engine` imports nothing from `gui`/`midi`.
- Window must fit 1366×768 (`tests/test_layout.py`); 16 steps per pattern, positions in float beats, sr 48000.
- After every task: `.venv/bin/python -m pytest -q` is fully green. After every **stage** (task) that touches audio, also run the real-stream check: `.venv/bin/python -m pytest tests/test_stream.py -s -q` (real device; expects `errors == 0` and `xruns <= 1`).

## Review Focus

- Closing a master with followers while playing: followers continue from the exact `pos` and BPM, no restart (Task 3).
- Add / close / switch source repeatedly during a live stream: no xrun burst, no exception, no hung voices (Task 8).
- Closing the selected tab, the first tab, or the last Synth: selection falls back sensibly; MIDI notes held on the closed tab are released (Task 7).
- Muting during sustained notes then unmuting: no click; `level` default 1.0 leaves the single-synth output bit-identical to today (Task 2).
- Editing BPM or pressing PLAY on a follower is ignored/disabled; a follower of a stopped master is silent and releases its notes (Task 4, 7).

## File Structure

- Create `nomad/engine/clock.py` (`Clock`, `StepGrid`), `instance.py` (`InstrumentBase`), `topology.py` (`Topology`), `manager.py` (`InstrumentManager`), `registry.py` (`INSTRUMENT_TYPES`), `nomad/gui/synth_panel.py`, `nomad/gui/tabbar.py`, `tools/stream_stress.py`.
- Modify `nomad/engine/{engine,instrument,sequencer,drums}.py`, delete `transport.py` (replaced), `nomad/gui/{app,seq_view,drums_view,help}.py`, `nomad/main.py`, `nomad/midi/midi_input.py` (router target), README.

---

### Task 1 (Stage 1): Clock in beats replaces Transport

**Files:** Create `nomad/engine/clock.py`; modify `drums.py`, `sequencer.py`, `instrument.py`, `engine.py`; delete `transport.py`; Test `tests/test_clock.py` (port of `tests/test_transport.py`), existing `test_drums.py`, `test_sequencer.py`, `test_engine_transport.py`.

**Interfaces:**
- Produces: `Clock(sample_rate)`: `bpm: float = 110.0`, `pos: float` (beats), `running: bool`, `start()` (pos=0, running), `stop()` (pos=0, not running), `toggle()`, `advance(frames: int)` (`pos += frames*bpm/(60*sr)` if running), `frames_to(beat: float) -> int` (>=1, `1<<30` if not running).
- Produces: `StepGrid(division: float = 0.25, length: int = 16, swing: float = 0.0)`: `step_at(pos) -> int` (absolute step index, closed form: pair = floor(pos/(2*div)); even step lasts `div*(1+swing)`, odd `div*(1-swing)`), `next_boundary(pos) -> float` (beat of the next step start), `pattern_step(abs_index) -> int` (`% length`).
- Each step-driven instrument (drums, sequencer) exposes `poll()` (fires `on_step` for every abs step newer than its `_last`, tolerance `1e-9` beats; `_last = -1` after the clock restarts or `pos` goes backwards; joining mid-run sets `_last` to the current step without firing) and `frames_to_event()`.
- Until Task 2 the `Engine` keeps two roots (`self.clock` for drums, `self.seq_clock` for synth sequencers) and drives them as `Transport.run` did: at each chunk start `poll()`, chunk = min(remaining, `frames_to_event`s, 256-frame slices), render, `advance` each root once.

- [ ] **Step 1: Write failing tests** `tests/test_clock.py`: `test_step_grid_even_odd_lengths` (swing 0.5 → step 0 lasts 0.375 beat, step 1 lasts 0.125), `test_clock_advance_120bpm` (6000 frames = 0.25 beat), `test_bpm_jump_mid_step_does_not_skip` (port: 60 BPM, advance 11040, set `bpm=180`, advance 384 → `step_at(pos) == 1`), `test_swing_jump_mid_step_does_not_skip`, `test_frames_to_next_boundary_exact` (`frames_to(grid.next_boundary(0.0)) == 6000` at 120 BPM), `test_stopped_clock_frames_to_is_far`, `test_pattern_step_wraps_at_length` (`length=12`: abs 13 → 1).
- [ ] **Step 2:** `pytest tests/test_clock.py -v` → FAIL (module missing).
- [ ] **Step 3:** Implement `clock.py`; switch `DrumMachine` (`clock` instead of `transport`, `playing/position` properties from `clock.running` and `grid.pattern_step(grid.step_at(clock.pos))`, standalone `DrumMachine(sr, clock=None)` creates its own `Clock`, `set_value("bpm"|"swing")` writes `clock.bpm` / `grid.swing`) and `StepSequencer` to the `poll()`/`frames_to_event()` protocol; `Engine` loop as in Interfaces.
- [ ] **Step 4:** `pytest -q` → all green (the ported jump tests in `test_drums.py` are the safety net, unmodified); `pytest tests/test_stream.py -s -q` → `errors == 0`.
- [ ] **Step 5:** commit `refactor(engine): beat-based Clock and StepGrid replace Transport`.

### Task 2 (Stage 2): Instances, topology, command FIFO, mute / level / master

**Files:** Create `instance.py`, `topology.py`; modify `engine.py`, `instrument.py`, `drums.py`; Test `tests/test_instance.py`, `tests/test_engine_mixer.py`; existing engine/main/stream tests unchanged.

**Interfaces:**
- `InstrumentBase`: `id: int`, `kind: str`, `name: str`, `clock: Clock` (own, always allocated), `source: InstrumentBase | None` (GUI-published attribute, read by audio only through the `Topology`), `buf: np.ndarray` (256, preallocated), audio-owned `_gain: float`, `_gain_target: float`; property `effective_clock` (`source.clock` or own). Abstract: `drain()`, `render_block(b, n)`, `poll()`, `frames_to_event() -> int`, `advance(n)`, `panic()`, `on_stop()`.
- `Topology(insts: tuple)` immutable: `.insts`, `.roots` (tuple of own clocks of instances with `source is None`), built by `Topology.build(insts)`.
- `Engine`: `_q: SimpleQueue` accepting `("topo", Topology, handovers)`, `("clock", clock, "start"|"stop"|"bpm", value)`, `("mute", inst, bool)`, `("level", inst, float)`, `("panic",)`; `self.topology`; grave queue `_grave` returning the old topology to the GUI thread; `Engine.master_volume` handled by `Engine.set_param("master_volume", v)` (clamped, smoothed per slice as today) and **removed from `SynthInstrument`'s parameters**; `Engine.snapshot()` merges it; `Engine.synth`, `.vm`, `.drums`, `.set_param`, `.note_on/off`, `.panic` keep working (default instances created at construction).
- Per-instance gain: target = `level * (0 if muted else 1)`; ramped linearly per 256-frame slice on the audio thread (~5 ms), applied to `inst.buf` before summing into the mix.
- `DrumMachine.trigger`, `stop_all` and clock start/stop route through commands (class C); `DrumMachine` standalone keeps its direct API for its own tests.
- Handover on adoption (audio thread): for each `(dst_clock, src_clock)` copy `bpm`, `pos`, `running`.

- [ ] **Step 1: Write failing tests:** `test_default_level_is_bit_identical_to_before` (render a fixed sequence through `Engine` with `level` default and compare to a recorded golden array from the pre-change code: record it first with `git stash`/previous commit), `test_mute_ramps_without_click` (sustained note, `mute` → `np.abs(np.diff(y)).max()` below a click threshold at the ramp), `test_unmute_restores_level`, `test_mute_keeps_clock_running` (muted drums still advance `clock.pos`), `test_master_volume_still_global_and_unchanged` (existing `test_master_volume_jump_has_no_click` passes), `test_each_root_clock_advances_once_per_render` (counter wrapper on `Clock.advance`: exactly one call per chunk per root, zero for followers), `test_gui_thread_never_writes_audio_fields` (static check: `Engine` public methods only enqueue; assert queue size grows and instance fields unchanged before `render`), `test_panic_goes_through_queue`.
- [ ] **Step 2:** run the new tests → FAIL.
- [ ] **Step 3:** Implement per Interfaces; `Engine.render` mixes `topology.insts` (not special-casing drums); remove `Engine.seq_clock`.
- [ ] **Step 4:** `pytest -q` → green; real-stream check → `errors == 0`, `xruns <= 1`.
- [ ] **Step 5:** commit `refactor(engine): instances, topology snapshot and command FIFO`.

### Task 3 (Stage 3): InstrumentManager, registry, clock source rules

**Files:** Create `manager.py`, `registry.py`; Test `tests/test_manager.py`, `tests/test_manager_stress.py`.

**Interfaces:**
- `INSTRUMENT_TYPES: dict[str, InstrumentType]` with `InstrumentType(kind: str, label: str, create: Callable[[float, int, str], InstrumentBase])`; `"synth"` → label `"Synth"`, `"drums"` → label `"Machine Drum"`.
- `InstrumentManager(engine)`: `instances -> tuple[InstrumentBase, ...]` (GUI authority), `add(kind: str) -> InstrumentBase` (names `Synth 01`, `Synth 02`, `Machine Drum 01`…, ids monotonic, never reused), `remove(inst)`, `set_source(inst, src_or_None) -> bool`, `start(inst)/stop(inst)/set_bpm(inst, v)` (ignored and return `False` for followers), `set_mute(inst, bool)`, `set_level(inst, v)`, `collect()` (drains the engine graveyard on the GUI thread).
- `set_source` rejects: `src is inst`; `src` not live or not Internal; `inst` has followers and `src` is not None. Allows many followers per source, and re-pointing a follower to another Internal source.
- `remove(inst)`: followers become Internal with a handover `(follower.clock, inst.clock)` (copies `bpm/pos/running` on the audio thread); the instance gets `panic()` and is dropped from the topology ~100 ms later via a second publish (GUI `after`, injected as a callable for tests). `remove` of the last instance is allowed. A follower going Internal (`src=None`) also gets a handover from its old source.

- [ ] **Step 1: Write failing tests:** `test_add_names_and_ids`, `test_two_followers_of_one_master_allowed`, `test_chain_rejected` (follower as source; master that has followers cannot follow), `test_self_and_dead_source_rejected`, `test_follower_play_and_bpm_ignored`, `test_remove_master_followers_keep_position_and_bpm` (render until `master.clock.pos ≈ 3.2`, remove master, render one chunk → each follower's own clock `pos`/`bpm`/`running` equal the master's at removal time, patterns not restarted: sequencer step index continues), `test_follower_goes_internal_with_handover`, `test_source_switch_is_immediate_next_render` (documented jump allowed: assert the follower's `effective_clock` is the new source on the very next `render`), `test_removed_instance_not_dropped_in_audio_thread` (after `render`, the old topology sits in the graveyard, not freed: `weakref` still alive until `collect()`), `test_stress_random_ops_with_real_classes` (port of the scratchpad model `clock_sim.py` onto real `Engine` + manager: audio thread renders random sizes while the GUI thread issues random add/remove/set_source/start/stop/bpm/mute for 5 s: no exception, `engine.errors == 0`, every root advanced once per render, no follower references a clock that no root advances).
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** Implement. `registry.py` imports only engine modules.
- [ ] **Step 4:** `pytest -q` → green; real-stream check green.
- [ ] **Step 5:** commit `feat(engine): InstrumentManager with shared clock sources`.

### Task 4 (Stage 4): Follower semantics in instruments

**Files:** Modify `instrument.py`, `sequencer.py`, `drums.py`, `engine.py`; Test `tests/test_follow.py`.

**Interfaces:** an instrument's `poll()`/`frames_to_event()` read `effective_clock` only; `running` of that clock gates events; when the effective clock stops, `on_stop()` releases notes (sequencer) and drum voices fade (5 ms); when a follower switches to a clock whose `pos` is behind/ahead, `_last` is recomputed (no fire for the current step, next boundary fires).

- [ ] **Step 1: Write failing tests:** `test_follower_steps_align_with_master` (master 16-step drums, follower sequencer with notes on every step: `note_on` frames equal the drum `on_step` frames over 3 bars at 128 BPM), `test_bpm_change_on_master_reaches_follower` (set 128 BPM at 120 → follower interval matches within 1 frame), `test_different_length_patterns_stay_aligned` (follower `length=12` realigns with master every `lcm` steps: step indices computed from `pos`), `test_master_stop_stops_followers_and_releases_notes`, `test_follower_of_stopped_master_is_silent`, `test_join_mid_run_does_not_fire_current_step`, `test_mute_is_independent_of_follow`.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** Implement per Interfaces (division/length/swing stay per instrument, defaults 0.25 / 16 / 0.0, no UI).
- [ ] **Step 4:** `pytest -q` green; real-stream check green.
- [ ] **Step 5:** commit `feat(engine): instruments follow a shared clock`.

### Task 5 (Stage 5): Extract SynthPanel from App

**Files:** Create `nomad/gui/synth_panel.py`; modify `app.py`, `seq_view.py`, `help.py`; Test `tests/test_synth_panel.py`, existing `test_layout.py`, `test_help.py`, `test_main.py`, `test_scope_view.py`.

**Interfaces:** `SynthPanel(parent, instrument: SynthInstrument, manager, hooks)` owns a `widgets: dict[str, Knob|Selector]` for its params (LAYOUT sections incl. `polyphony`, `bend_range`; **not** `master_volume`), its `SeqView`, a small strip with `level` knob and `MUTE` button, `Clock Source` selector and BPM knob (built in Task 7), `push_snapshot()`, `show(name, value)`, `apply_param(name, value)`. `App.widgets` and `App.apply_param` stay as aliases to the selected panel so existing tests pass; the `master_volume` knob stays in the right-hand global section and talks to `Engine`.

- [ ] **Step 1: Write failing tests:** `test_panel_widgets_bound_to_its_own_instrument` (two `SynthInstrument`s, two panels: turning a knob in A changes only A's snapshot), `test_master_volume_not_in_panels`, `test_app_widgets_alias_selected_panel`, `test_panel_help_registered` (every panel widget has help).
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** Move `_section/_make/_user/_show/_push_snapshot/apply_param` into `SynthPanel`; App builds one panel for the default Synth, shown in place of today's `synth_view`.
- [ ] **Step 4:** `pytest -q` green including layout (≤1366×768); real-stream check green.
- [ ] **Step 5:** commit `refactor(gui): per-instance SynthPanel`.

### Task 6 (Stage 6): TabBar, "+" chooser, close, selection

**Files:** Create `nomad/gui/tabbar.py`; modify `app.py`, `midi_input.py` (router target), `help.py`; Test `tests/test_tabbar.py`.

**Interfaces:** `TabBar(parent, manager, on_select, on_new_kind_menu, on_close)`: tabs show `inst.name`, a `MASTER` badge when `manager` reports followers, `⇄ <source name>` when following, the selected tab highlighted, and a `×` close button; `+` opens a `tk.Menu` built from `INSTRUMENT_TYPES` (`Synth`, `Machine Drum`). Tab bar **replaces** the SYNTH/DRUMS header buttons (no extra row, no extra width). `App.select(inst)` shows that instance's persistent panel (`grid_remove` the others) and does not touch the engine. MIDI router and on-screen keyboard target the selected synth: switching or closing releases notes held on the previous target (`MidiRouter.reset()`), no hung voices.

- [ ] **Step 1: Write failing tests:** `test_plus_menu_lists_registered_types`, `test_new_tab_creates_instance_and_selects_it`, `test_switching_tabs_keeps_panel_state` (move a knob, switch away and back: value kept; engine untouched: queue size unchanged by switching), `test_close_selected_falls_back_to_neighbour`, `test_close_last_tab_allowed_and_plus_still_works`, `test_midi_goes_to_selected_synth`, `test_held_notes_released_on_tab_switch`, `test_tab_badges_master_and_follower`, `test_header_fits_1366` (tab bar plus existing header widgets ≤1366 px).
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** Implement. Panels are built lazily with `root.after` one section per tick (GIL held by Tk otherwise), shown only when complete; a `Machine Drum` tab wraps the existing `DrumView` for its `DrumMachine`.
- [ ] **Step 4:** `pytest -q` green; real-stream check green.
- [ ] **Step 5:** commit `feat(gui): instrument tab bar with create, select and close`.

### Task 7 (Stage 7): Clock Source UI, mute / level strip, locks

**Files:** Modify `synth_panel.py`, `drums_view.py` (per-instance strip), `tabbar.py`, `help.py`; Test `tests/test_clock_ui.py`.

**Interfaces:** per tab: `CLOCK: [Internal ▾ | <other Internal instance names>]` (options regenerate on tab add/close), BPM knob (locked, showing the source BPM, while following), PLAY button disabled while following, `MUTE` button and `LEVEL` knob always enabled. Selector calls `manager.set_source`; a rejected change reverts the selector and sets the status text. Help texts for the new controls in the same Spanish style.

- [ ] **Step 1: Write failing tests:** `test_clock_menu_offers_only_internal_other_instances`, `test_following_locks_bpm_and_play`, `test_follower_shows_master_bpm`, `test_going_internal_unlocks_and_keeps_bpm`, `test_closing_master_updates_follower_ui_to_internal`, `test_mute_and_level_buttons_call_manager`, `test_help_for_new_controls`.
- [ ] **Step 2:** run → FAIL.
- [ ] **Step 3:** Implement; GUI keeps its own `muted` intent for painting and never reads audio gain.
- [ ] **Step 4:** `pytest -q` green (layout still ≤1366×768); real-stream check green.
- [ ] **Step 5:** commit `feat(gui): clock source selector, locks, mute and level`.

### Task 8 (Stage 8): Real-stream validation, docs

**Files:** Create `tools/stream_stress.py`; modify `README.md`; Test `tests/test_stream_instances.py` (real device, auto-skipped without one).

**Interfaces:** `tools/stream_stress.py --seconds N`: builds `Engine` + `InstrumentManager`, opens the real `AudioStream`, plays a repeating sequence on a master Synth and a Machine Drum following it, and from the GUI-side thread adds/removes instances, switches sources, mutes and changes BPM at a fixed pace; prints `callbacks`, `xruns`, `errors`, `max_late_ms`, panel-build time and exits non-zero if `errors > 0` or `xruns` exceeds 1.

- [ ] **Step 0 (product vision: "five instruments open at once"):** extend the tool with `--instances 5` (3 Synths with the sequencer on + 2 Machine Drums, 8 voices held on one Synth) and record `max_late_ms` and xruns; if it xruns, report it as a finding for the user (CPU budget), do not hide it.
- [ ] **Step 1: Write failing test** `test_instance_churn_on_real_stream` (skips when `list_output_devices()` is empty): 5 s of add/remove/switch/mute with `errors == 0` and `xruns <= 1`.
- [ ] **Step 2:** run → FAIL (tool missing).
- [ ] **Step 3:** Implement the tool; run it manually for 30 s and record the numbers in the commit message; also a GUI run: create a Synth tab while a sequence plays and record xruns and the panel build time (this is the measurement the spec lists as pending).
- [ ] **Step 4:** `pytest -q` green; README gets "Pestañas e instancias" and "Reloj compartido" sections (source change is immediate and may jump or repeat a step; MIDI/keyboard follow the selected tab; `master_volume` vs `level`).
- [ ] **Step 5:** commit `docs,test: instance churn validation on the real stream`.

---

## Self-Review notes

- Spec coverage: tabs / create / close / select (T6), independent state (T3, T5), simultaneous mixing (T2), shared clock by reference, one-to-many, no chains, handover, immediate switch documented (T3, T4, T8), MUTE audio-thread ramp and ownership policy (T2), `master_volume` unchanged + `level` new (T2, T5), existing class-C violations fixed (T2), staged real-stream validation (every task + T8). Out of scope per spec: chains, quantized switch, MIDI clock/Link, pattern-length UI, instance cap.
- Type consistency: `InstrumentBase`, `Topology.build`, `InstrumentManager.set_source -> bool`, `Clock.frames_to`, `StepGrid.next_boundary` are used with the same signatures in later tasks.
- Known limit: `poll()` tolerance `1e-9` beats; float64 beat accumulation drifts ~1e-16 per op (negligible per session, unmeasured over hours).
