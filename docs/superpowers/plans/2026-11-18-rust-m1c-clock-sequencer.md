# NOMAD en Rust, M1-C (reloj, rejilla de pasos y secuenciador) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Portar a `nomad-core` el reloj en beats, la rejilla de pasos con swing, el rastreador de pasos y el secuenciador de 16 pasos con acordes y grabación (step y tiempo real), e integrarlo al `Synth`, verificado contra trazas y audio de Python.

**Architecture:** Mismo método que M1-A/B (el Python de `nomad/engine/` es la especificación; referencias binarias `f64` + manifiesto JSON). Los relojes son datos sin dueño: el secuenciador y el rastreador reciben `&Clock` en cada llamada (en Python lo resuelve `eclock`, que es del hito M1-D). El secuenciador no conoce al `Synth`: suena notas a través de un trait `NoteSink` y consulta las teclas en vivo con una referencia a `[bool; 128]`.

**Tech Stack:** igual que M1-A/B.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M1). Sigue: M1-D (batería, mezclador, topología, relojes seguidores).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Python nuevo sólo en `tools/` y `tests/`.
- `f64`; tolerancia `|a-b| <= 1e-9 + 1e-9*|b|` para floats; las trazas de eventos (frames, pasos, notas) se comparan **exactas**.
- Sin asignaciones ni bloqueos en `poll`, `frames_to_event`, `advance`, `on_step`, `record`, `rest`, `release`: los arreglos del secuenciador son de tamaño fijo.
- Sin `unsafe` y sin dependencias nuevas en `[dependencies]`.
- Mismas constantes que Python: `STEPS = 16`, `FAR = 1 << 30` (frames), `EPS = 1e-9` (beats), 6 notas por paso (`EXTRA = 5`), `NONE = 255`.
- Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y el pytest de lo nuevo en verde. Rust: `export PATH="$HOME/.cargo/bin:$PATH"`.

## Review Focus

1. `step_at` justo en un límite de paso (con swing 0, 0.3 y 0.6) y a posiciones `-EPS`, `0`, `+EPS`: el paso correcto y sin repetir (Task 1).
2. Cambiar el swing, la división o el largo con el reloj andando no repite ni salta un paso (`StepTracker`, Task 1).
3. Cambiar de reloj fuente a mitad de una corrida: se une sin reiniciar el patrón y espera al próximo límite (Task 1).
4. Reloj parado: ningún paso dispara, y el secuenciador suelta cualquier nota colgada (Task 2).
5. Una tecla en vivo igual a la nota del secuenciador no se corta al soltar el paso (Task 2).
6. Acorde de 6 notas: la séptima se ignora; transponer fuera de [0, 127] se acota y mantiene los intervalos posibles (Task 2).
7. REC por pasos con varias teclas a la vez graba un acorde en un solo paso y el cursor avanza una vez; REC en vivo reescribe el paso en la vuelta siguiente (Task 3).

---

### Task 1: Reloj, rejilla de pasos y rastreador

**Files:**
- Create: `tools/ref_clock.py`, `rust/nomad-core/src/clock.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_clock.py`, `rust/nomad-core/tests/clock.rs`

**Interfaces:**
- Produces (`nomad_core::clock`): `pub const STEPS: usize = 16; pub const FAR: i64 = 1 << 30; pub const EPS: f64 = 1e-9;`
  - `pub struct Clock { pub sr: f64, pub bpm: f64, pub pos: f64, pub running: bool }`; `Clock::new(sample_rate: f64) -> Clock` (bpm 110); `start(&mut self)` (si ya corre no hace nada; si no, `pos = 0`); `stop(&mut self)` (`running = false`, `pos = 0`); `toggle(&mut self)`; `advance(&mut self, frames: i64)` (sólo si corre: `pos += frames * bpm / (60*sr)`); `frames_to(&self, beat: f64) -> i64` (`FAR` si parado; si no `max(1, ceil((beat-pos)*60*sr/bpm - 1e-6))`).
  - `pub struct StepGrid { pub division: f64, pub length: usize, pub swing: f64 }` con `StepGrid::new(division, length, swing)`, `step_beats(&self, k: i64) -> f64`, `step_start(&self, k: i64) -> f64`, `step_at(&self, pos: f64) -> i64`, `next_boundary(&self, pos: f64) -> f64`, `pattern_step(&self, k: i64) -> usize` (`k mod length`, siempre ≥ 0).
  - `pub struct StepTracker { /* privados */ }`; `StepTracker::new()`; `poll(&mut self, grid: &StepGrid, clock: &Clock, clock_id: u64) -> Option<usize>`; `frames_to_event(&self, grid: &StepGrid, clock: &Clock) -> i64`. `clock_id` identifica al reloj (Python compara identidad de objeto): cambiar de id significa "cambié de fuente".
- Python: `ref_clock.write` escribe `clock.{f64,json}`. (a) `"grid/<swing>/<div>"` para `swing ∈ {0.0, 0.3, 0.6}`, `div ∈ {0.25, 1.0}`: para 200 posiciones `pos` (incluye cada `step_start(k)` para k=0..24 y `±1e-10`, `±1e-12` alrededor de cinco límites) guarda `[step_at, next_boundary]`; con `step_beats` y `step_start` de k=0..32 en `meta` o como entrada aparte. (b) Escenarios de rastreador, cada uno una lista de comandos `("start",)`, `("stop",)`, `("adv", frames)`, `("bpm", x)`, `("swing", s)`, `("div", d)`, `("len", L)`, `("clock", id)` (cambia el reloj que se pasa al rastreador, usando dos relojes distintos con estado propio), `("poll",)`; cada `poll` guarda `(paso o -1, frames_to_event)`: `"track/basic"` (120 bpm, 3000 frames por `adv`, 40 polls), `"track/restart"` (start, run, stop, start), `"track/bpm_change"`, `"track/swing_drag"` (swing de 0 a 0.5 en 10 pasos con el reloj corriendo), `"track/div_change"`, `"track/join_midrun"` (dos relojes: el segundo arrancado y adelantado; `("clock", 2)` a mitad), `"track/handover_on_boundary"` (el cambio ocurre exactamente sobre un límite), `"track/len5"` (largo 5), `"track/stopped"` (polls con el reloj parado).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `clock.rs`: `grid_matches_python` (todos los `grid/*`; enteros exactos, floats con tolerancia), `tracker_scenarios_match_python` (todos los `track/*`: pasos y `frames_to_event` **exactos**), `clock_basics` (parado: `frames_to == FAR`; arrancado `frames_to(1.0)` a 120 bpm y 48 kHz es 24000; `stop` vuelve `pos` a 0; `start` con el reloj ya corriendo no reinicia `pos`), `pattern_step_is_never_negative` (`k` negativo), `no_repeated_step_on_swing_drag` (de `track/swing_drag`: no hay dos pasos consecutivos iguales seguidos de un poll sin avance de reloj).
- [ ] **Step 2: Run** `python -m pytest tests/test_ref_clock.py` y `cd rust && cargo test -p nomad-core --test clock`. Expected: FAIL.
- [ ] **Step 3: Implement** portando `Clock`, `StepGrid` y `StepTracker` de `nomad/engine/clock.py` literalmente (el comportamiento de `_joined`, `last`, `_last_pos` y el chequeo `restarted`/`fresh` es el contrato).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): beat clock, step grid and step tracker"`.

### Task 2: Secuenciador de 16 pasos con acordes

**Files:**
- Create: `tools/ref_sequencer.py`, `rust/nomad-core/src/sequencer.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`, `rust/nomad-core/src/voice_manager.rs` (sólo para implementar `NoteSink`)
- Test: `tests/test_ref_sequencer.py`, `rust/nomad-core/tests/sequencer.rs`

**Interfaces:**
- Consumes: `Clock`, `StepGrid`, `StepTracker` (Task 1), `VoiceManager`.
- Produces (`nomad_core::sequencer`): `pub const EXTRA: usize = 5; pub const NONE: u8 = 255;` `pub trait NoteSink { fn note_on(&mut self, note: i64, velocity: f64); fn note_off(&mut self, note: i64); }` (implementado por `VoiceManager`); `pub struct StepSequencer { pub on: [u8; 16], pub note: [u8; 16], pub vel: [u8; 16], pub extra: [[u8; EXTRA]; 16], pub grid: StepGrid, pub enabled: bool, pub rec: bool, pub rec_step: usize, /* privados: tracker, gate, held, left, rec_down, rt_last */ }`; `StepSequencer::new(sample_rate: f64) -> StepSequencer` (grid `0.25`, 16, 0.0; `note` en 60, `vel` en 100, `gate` 0.5); `gate(&self) -> f64` y `set_gate(&mut self, v: f64)` (acota a [0.05, 1.0], no finito → 0.5); `set_step(&mut self, step: usize, on: bool, note: Option<i64>, vel: Option<i64>)` (acota nota a [0, 127] y vel a [1, 127]; `on = false` borra `extra`); `chord(&self, step: usize) -> Vec<i32>` (sólo tests/GUI); `add_note(&mut self, step: usize, n: i64)`; `transpose(&mut self, step: usize, delta: i64)`; `clear(&mut self)`; `position(&self, clock: &Clock) -> usize`; `poll(&mut self, clock: &Clock, clock_id: u64, sink: &mut impl NoteSink, live: &[bool; 128])`; `frames_to_event(&self, clock: &Clock) -> i64`; `advance(&mut self, n: i64, sink: &mut impl NoteSink, live: &[bool; 128])`; `release(&mut self, sink: &mut impl NoteSink, live: &[bool; 128])`; `on_step(&mut self, pos: usize, clock: &Clock, sink: &mut impl NoteSink, live: &[bool; 128])`. Semántica = `sequencer.py` (acordes hasta 6 notas con `NONE` como vacío, `_off` que no suelta teclas en vivo, `_left` por `gate * step_beats * 60 * sr / bpm`, `frames_to_event` = mínimo entre el apagado pendiente y el rastreador cuando `enabled`).
- Python: `ref_sequencer.write` escribe `sequencer.{f64,json}`. Los escenarios corren el lazo de planificación de `Engine.render` sobre **un** `SynthInstrument` (sin audio): por cada chunk, `inst.poll()`, `chunk = min(restante, inst.frames_to_event())`, `inst.advance(chunk)`, `inst.clock.advance(chunk)`; el exportador envuelve `vm.note_on/note_off` para registrar `(frame_absoluto, "on"|"off", nota, velocidad_int)` y `live` se modifica con `inst.note_on/note_off` + `drain()` cuando el escenario lo indica. Cada escenario guarda la traza como 4 columnas (frame, tipo 1/0, nota, vel) en `.f64`. Escenarios: `"seq/mono_16"` (patrón de 6 notas, 120 bpm, 2 compases), `"seq/chord_bar"` (`division=4.0`, `length=4`, `gate=1.0`, acordes de 3 notas), `"seq/gate_short"` (gate 0.1), `"seq/swing"` (swing 0.4), `"seq/disabled_then_enabled"`, `"seq/stop_releases"` (se para el reloj con una nota sonando), `"seq/live_key_survives"` (la tecla 64 en vivo coincide con la nota del paso), `"seq/len5_vel"` (largo 5, acentos), `"seq/transpose_clear"` (transpose y clear a mitad), `"seq/max_chord"` (7 intentos de `add_note`: la séptima se ignora).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `sequencer.rs` con un `Recorder` que implementa `NoteSink` y apunta `(frame, on/off, nota, vel)`: `traces_match_python` (todas, **exactas**), `chord_api` (`set_step`, `add_note` con duplicados y con la séptima nota, `chord()`; `set_step(.., on=false)` limpia los extras), `transpose_clamps` (acorde `[60, 127]` +12 queda `[60, 127]`; `[0, 40]` −5 queda `[0, 40]`), `gate_sanitised` (`NaN`, 0, 5), `position_follows_clock`, `stopped_clock_releases_hanging_note`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test sequencer`. Expected: FAIL.
- [ ] **Step 3: Implement** `StepSequencer` portando la parte de reproducción de `nomad/engine/sequencer.py` (sin `record*`, que es la Task 3); `impl NoteSink for VoiceManager` delegando en `note_on`/`note_off`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): 16-step chord sequencer playback"`.

### Task 3: Grabación (REC por pasos y en vivo)

**Files:**
- Modify: `tools/ref_sequencer.py`, `rust/nomad-core/src/sequencer.rs`
- Test: `tests/test_ref_sequencer.py`, `rust/nomad-core/tests/sequencer_rec.rs`

**Interfaces:**
- Consumes: `StepSequencer` (Task 2), `Clock`.
- Produces en `StepSequencer`: `record(&mut self, note: i64, vel: i64, clock: &Clock)`; `record_off(&mut self, clock: &Clock)`; `arm(&mut self, on: bool)` (pone `rec`, `rec_step = 0`, limpia `rec_down` y `rt_last`); `rest(&mut self, clock: &Clock)`. Semántica = `record`, `record_off`, `arm`, `rest` de `sequencer.py` (reloj corriendo: tiempo real con el paso más cercano por `step_start`, `rt_last` agrupa notas del mismo paso y reescribe en la vuelta siguiente; parado: paso por cursor, la primera tecla reescribe, las siguientes suman al acorde, el cursor avanza cuando se sueltan todas; `rest` sólo con reloj parado y sin teclas abajo).
- Python: escenarios de comandos `("arm", bool)`, `("rec", note, vel)`, `("recoff",)`, `("rest",)`, `("clock", "start"|"stop")`, `("adv", frames)`; guardan el estado final del patrón (`on`, `note`, `vel`, `extra` aplanados, `rec_step`) tras cada comando en el `.f64` y los comandos en `meta`. Casos: `"rec/step_notes"` (4 notas sueltas), `"rec/step_chord"` (3 teclas a la vez y soltarlas), `"rec/step_rest"` (nota, rest, nota), `"rec/step_wrap"` (17 notas), `"rec/step_eighth_seventh"` (7 teclas a la vez: la séptima se ignora), `"rec/live_quantize"` (reloj corriendo, notas a distintas posiciones incluyendo `x.49` y `x.51` de un paso), `"rec/live_chord"` (dos notas en el mismo paso), `"rec/live_next_pass"` (misma posición una vuelta después reescribe), `"rec/clock_starts_with_keys_down"`, `"rec/rest_ignored_when_running"`.

- [ ] **Step 1: Write the failing tests.** Python `test_reference_is_up_to_date`. Rust `sequencer_rec.rs`: `rec_scenarios_match_python` (estado del patrón **exacto** tras cada comando), `arm_resets_cursor_and_counters`, `rest_is_ignored_while_running_or_keys_down`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test sequencer_rec`. Expected: FAIL.
- [ ] **Step 3: Implement** `record`, `record_off`, `arm` y `rest` portando `sequencer.py`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): sequencer step and real-time recording"`.

### Task 4: El secuenciador dentro del Synth

**Files:**
- Create: `tools/ref_synth_seq.py`
- Modify: `rust/nomad-core/src/synth.rs`, `tools/export_reference.py`, `rust/nomad-core/tests/common/mod.rs`
- Test: `tests/test_ref_synth_seq.py`, `rust/nomad-core/tests/synth_seq.rs`

**Interfaces:**
- Consumes: `Synth` (M1-B), `StepSequencer`, `Clock`.
- Produces en `Synth`: campo `pub sequencer: StepSequencer`; `pub fn note_on(&mut self, note: i64, velocity: f64, clock: &Clock)` y `pub fn note_off(&mut self, note: i64, clock: &Clock)` (la firma anterior sin reloj se reemplaza: si `sequencer.rec` llaman a `record(note, (velocity*127) as i64, clock)` / `record_off(clock)` **además** de tocar la nota y marcar `live`); `pub fn arm_rec(&mut self, on: bool)`, `pub fn rest(&mut self, clock: &Clock)`; `pub fn poll_sequencer(&mut self, clock: &Clock, clock_id: u64)`; `pub fn frames_to_event(&self, clock: &Clock) -> i64`; `pub fn advance_sequencer(&mut self, n: i64)`; `panic` ahora también hace `sequencer.arm(false)` y `release`. Se agrega en `tests/common/mod.rs` un `run_single_instrument(synth: &mut Synth, clock: &mut Clock, frames: usize, out: &mut [f64], events: &mut dyn FnMut(usize, &mut Synth, &mut Clock))` que replica el lazo de `Engine.render` para **una** instancia Internal: `poll_sequencer`, `chunk = min(restante, frames_to_event)`, bloques de `CONTROL_BLOCK`, `advance_sequencer(chunk)`, `clock.advance(chunk)`; `events` se invoca antes de cada iteración con el frame absoluto, para inyectar notas y comandos. (En M1-D ese lazo pasa al mezclador.)
- Python: `ref_synth_seq.write` escribe `synth_seq.{f64,json}`: audio del lazo de planificación de `Engine.render` con una sola `SynthInstrument` y su reloj propio, bloques de `render` de 256 muestras (llamando a `inst.render_block`). Escenarios: `"audio/pad_progression"` (preset "Pad de acordes" de `nomad.presets.FACTORY["synth"][0]`, 6 s a 100 bpm), `"audio/bass_glide"` (preset "Bajo" con glide, 4 s), `"audio/rec_then_play"` (REC por pasos de 4 acordes vía `note_on/note_off`, luego PLAY: 3 s), `"audio/live_key_over_seq"` (una tecla en vivo sobre la secuencia).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `synth_seq.rs`: `audio_scenarios_match_python` (1e-9), `rec_through_synth_note_on_writes_pattern` (con `arm_rec(true)`, `note_on`+`note_off` de tres teclas simultáneas escribe un acorde de 3 en el paso 0), `panic_disarms_and_releases`, `live_key_is_not_cut_by_sequencer` (tecla en vivo igual a la del paso sigue activa tras el apagado del paso).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test synth_seq`. Expected: FAIL.
- [ ] **Step 3: Implement** la integración en `synth.rs` (ajustar los llamadores de `note_on`/`note_off` de los tests previos de `synth.rs` y `voice_manager.rs` para la nueva firma con `&Clock`) y el helper de `tests/common/mod.rs`. Cargar los presets de fábrica en Rust no es necesario: el exportador guarda en `meta` los valores de parámetros y el patrón del preset, y el test los aplica con `set_param`/`set_step`/`add_note`.
- [ ] **Step 4: Run** `cd rust && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` y `python -m pytest tests -q --ignore=tests/test_stream_instances.py`. Expected: todo PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): sequencer wired into the synth core"`.
