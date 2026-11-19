# NOMAD en Rust, M1-B (voz, gestor de voces, glide y núcleo del Synth) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Portar a `nomad-core` la voz, el gestor de voces (robo, polifonía, glide y legato mono) y el núcleo DSP del Synth (suavizado de parámetros, LFOs, matriz por voz, controles), verificados contra referencias de Python.

**Architecture:** Igual que M1-A: el Python de `nomad/engine/` es la especificación y los tests de Rust comparan contra binarios `f64` que exportan los `tools/ref_*.py`. Los parámetros viajan como un arreglo indexado (`ParamValues`) con constantes de índice, no como diccionario. Las notas y los parámetros entran por métodos directos: la cola de comandos hacia el hilo de audio es del hito M2.

**Tech Stack:** igual que M1-A (infraestructura `refio`/`Blob` ya existente).

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M1). Siguen: M1-C (reloj y secuenciador), M1-D (batería, mezclador, topología).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Python nuevo sólo en `tools/` y `tests/`.
- `f64` en todo el DSP. Tolerancia por muestra: `|a-b| <= 1e-9 + 1e-9*|b|` en primitivas; para voz y Synth completos (filtro con realimentación, renders largos) se admite `1e-7`, y debe quedar anotado en el test con la razón si hace falta aflojarla.
- Sin asignaciones ni bloqueos en `render`, `note_on`, `note_off`, `set_param`, `render_block`: todo scratch se reserva en `new`.
- Sin `unsafe` y sin dependencias nuevas en `[dependencies]` de `nomad-core`.
- Entradas no finitas se tratan como en Python (`_f` de `voice.py`, `_note` de `voice_manager.py`, `clamp` del registro); sin panics en el camino de audio.
- Cada `.f64` < 200 KB. Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, y el pytest de lo nuevo en verde. Rust: `export PATH="$HOME/.cargo/bin:$PATH"`.

## Review Focus

1. Nota fuera de [0, 127] o velocidad `NaN`/fuera de [0, 1]: se ignora la nota / se acota la velocidad (Tasks 2-3).
2. Robo de voz sin click: la discontinuidad máxima entre muestras queda por debajo del umbral del test de Python (`< 0.25`) (Task 3).
3. Bajar la polifonía con un acorde sonando libera las voces sobrantes más viejas; polifonía fuera de [1, max] se acota (Task 3).
4. Glide: 0 ms es idéntico a sin glide; legato mono no reinicia envolventes; un robo sigue desde la afinación actual; `set_glide` con `NaN` o negativo da 0 (Task 3).
5. Parámetros o modulaciones extremas (`cutoff` `NaN`, `pitch` ±1000 semitonos, `amp` −5): salida finita y acotada (Task 2).
6. `panic` lleva todas las voces a silencio sin click y deja el gestor sin notas activas (Tasks 3-4).
7. Cambiar un parámetro suavizado a mitad de un render no produce saltos: el valor sigue la recursión del `Smoother` por bloque (Task 4).

---

### Task 1: Índices de parámetros y tipos comunes

**Files:**
- Modify: `rust/nomad-core/src/params.rs`, `rust/nomad-core/src/lib.rs`
- Create: `rust/nomad-core/src/mods.rs`
- Test: `rust/nomad-core/tests/params_index.rs`

**Interfaces:**
- Consumes: `SYNTH_PARAMS` (M0).
- Produces: `pub mod pid` en `params.rs` con una `pub const <NOMBRE>: usize` por cada parámetro del Synth (nombre en mayúsculas, p. ej. `pid::OSC1_WAVE`, `pid::FILT_ENV_AMT`, `pid::MOD0_SRC`, `pid::GLIDE`, `pid::MASTER_VOLUME`), iguales a la posición en `SYNTH_PARAMS`; `pub type ParamValues = [f64; 63]`; `pub fn default_values() -> ParamValues` (los `default` del registro). `nomad_core::mods::Mods` = `#[derive(Clone, Copy, Default, Debug, PartialEq)] pub struct Mods { pub pitch: f64, pub pitch1: f64, pub pitch2: f64, pub amp: f64, pub cutoff: f64, pub pw1: f64, pub pw2: f64, pub bend: f64 }` (semitonos para `pitch*` y `bend`, igual que `voice.py`).

- [ ] **Step 1: Write the failing test** `params_index.rs`: `every_pid_matches_its_name` (para las 63 constantes: `SYNTH_PARAMS[pid::X].name == "x"` en minúsculas; la lista de pares `(const, nombre)` se escribe en el test), `values_array_has_63_entries`, `default_values_match_registry`, `mods_default_is_zero`.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-core --test params_index`. Expected: FAIL (no compila).
- [ ] **Step 3: Implement** `pid`, `ParamValues`, `default_values` y `Mods`; `pub mod mods;` en `lib.rs`.
- [ ] **Step 4: Run** el test + fmt/clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): parameter index constants and Mods"`.

### Task 2: Voz

**Files:**
- Create: `tools/ref_voice.py`, `rust/nomad-core/src/voice.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_voice.py`, `rust/nomad-core/tests/voice.rs`

**Interfaces:**
- Consumes: `Oscillator`, `Envelope`, `Ladder4Pole` (M1-A), `ParamValues`, `pid`, `Mods`.
- Produces (`nomad_core::voice`): `pub const STEAL_FADE_SEC: f64 = 0.005;` `pub struct Voice { pub note: i32, pub age: u64, pub velocity: f64, pub env2_level: f64, /* privados */ }`; `Voice::new(sample_rate: f64) -> Voice`; `is_active(&self) -> bool`; `note_on(&mut self, note: i32, velocity: f64, start: Option<f64>)`; `set_target(&mut self, note: i32)` (legato: no toca envolventes); `note_off(&mut self)`; `begin_steal(&mut self, note: i32, velocity: f64)`; `render(&mut self, out: &mut [f64], p: &ParamValues, mods: &Mods)` (**suma** en `out`). Semántica = `nomad/engine/voice.py` incluida la parte de glide (`_cur`, `_pitch_note`, `RC` de ~3 constantes de tiempo, `p["glide"]`), el robo con fundido de 5 ms, el reenganche tras el fundido, el sync de osc2 con la fase de osc1 por bloque, el filtro en granos de 16 (resonancia·`K_MAX` > 3.2) o 64, el suavizado log2 del cutoff (`_CUTOFF_TAU = 0.003`) y `nan_to_num` a 0 al final. Bloques de hasta `MAX_BLOCK`; la primera muestra de la voz y su estado inicial deben coincidir con Python.
- Python: `ref_voice.write` escribe `voice.{f64,json}`. Cada escenario guarda `params` (overrides sobre los defaults de `REGISTRY`) y una lista de comandos `("on", note, vel[, start])`, `("off",)`, `("steal", note, vel)`, `("target", note)`, `("run", n)` ejecutados con bloques de 16 (`block` en meta) y concatena las salidas. Escenarios: `"basic/saw"`, `"basic/sine_tri_sq"` (osc1 y osc2 en ondas distintas), `"sync"` (`osc2_sync=1`, osc2 a +7 semitonos), `"free_mode"` (`osc1_mode=1`, `osc1_hz=330`), `"filter_env"` (`filt_env_amt=0.8`, resonancia 0.6), `"mods"` (mods `pitch=0.5`, `pitch1=-1.2`, `cutoff=0.3`, `pw1=0.2`, `amp=-0.3`, `bend=2.0`), `"mods_extreme"` (`pitch=1000`, `cutoff=nan`, `amp=-5`), `"steal"` (on 60, run 2000, steal 72, run 1500), `"retrigger"` (on, run, on sobre voz activa vía `begin_steal`), `"glide/fresh"` (`glide=200`, on 72 con start 60), `"glide/steal"` (`glide=300`: on 60, run, steal 72), `"glide/legato"` (`glide=100`: on 60, run 1500, target 67, run 3000), `"vel_extremes"` (velocidad `nan`, `-1`, `7`, tres escenarios cortos).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `voice.rs`: `scenarios_match_python` (todos, tolerancia 1e-7 documentada), `silent_until_note_on`, `idle_after_release` (`is_active()` es `false` tras `note_off` y suficientes bloques), `extreme_mods_stay_finite_and_bounded` (`|x| < 10`), `velocity_is_clamped` (vel 7 y 1.0 dan la misma salida; `nan` da silencio o la de 0), `glide_zero_equals_no_start` (con `glide=0` el `start` no cambia la salida).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test voice`. Expected: FAIL.
- [ ] **Step 3: Implement** `Voice` portando `voice.py`. Los buffers por bloque se reservan en `new` (`MAX_BLOCK`).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): synth voice with glide"`.

### Task 3: Gestor de voces

**Files:**
- Create: `tools/ref_voice_manager.py`, `rust/nomad-core/src/voice_manager.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_voice_manager.py`, `rust/nomad-core/tests/voice_manager.rs`

**Interfaces:**
- Consumes: `Voice`, `ParamValues`, `Mods`.
- Produces (`nomad_core::voice_manager`): `pub struct VoiceManager { /* privados */ }`; `VoiceManager::new(sample_rate: f64, max_voices: usize) -> VoiceManager` (≥ 1); `set_polyphony(&mut self, n: i64)`; `set_glide(&mut self, ms: f64)` (`NaN`, `inf`, ≤ 0 → 0); `note_on(&mut self, note: i64, velocity: f64)` (nota fuera de 0..=127 se ignora); `note_off(&mut self, note: i64)`; `active_notes(&self) -> Vec<i32>` (**sólo para tests y GUI**, asigna; la versión sin asignar es `fill_active_notes(&self, out: &mut [i32; 128]) -> usize`); `panic(&mut self)`; `render<F: FnMut(&Voice, &Mods) -> Mods>(&mut self, out: &mut [f64], p: &ParamValues, mods: &Mods, resolve: F)`; `pub glide: f64` y `pub polyphony: usize` de sólo lectura. Semántica = `voice_manager.py`: reutiliza la voz de la misma nota (robo), voz libre mientras haya menos activas que `polyphony`, si no roba la más vieja en release o, si no hay, la más vieja sostenida; sellos de edad; polifonía bajada libera las más viejas sostenidas sobrantes; legato mono (`polyphony == 1`, `glide > 0`, voz sostenida) con la regla de última nota, sin pila de notas (`shortcut:` en un comentario), `_last` como inicio del glide de una voz fresca.
- Python: `ref_voice_manager.write` escribe `voice_manager.{f64,json}` con escenarios de eventos entre bloques de 16 muestras: lista `("on", n, v)`, `("off", n)`, `("poly", k)`, `("glide", ms)`, `("panic",)`, `("run", frames)`; guarda la salida concatenada, y en `meta` los `active_notes()` ordenados capturados tras cada `run` (`checks`). Escenarios: `"chord8"` (8 notas), `"steal_oldest"` (9ª nota), `"steal_releasing"` (la 9ª roba una en release antes que una sostenida), `"poly1"` (acorde con polifonía 1), `"poly_lower"` (acorde de 4 y bajar a 2), `"glide_poly"` (glide 150, acorde sobre una nota previa liberada), `"glide_mono_legato"` (poly 1, glide 100: 60, run, 67, run, off 67, run), `"panic"`, `"invalid_notes"` (128, -1 ignorados; 60 válida), `"resolve"` (con un `resolve` que fija `amp=-0.5` para todas las voces).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `voice_manager.rs`: `scenarios_match_python` (salida con 1e-7 y `active_notes` iguales en cada checkpoint), `steal_has_no_step_discontinuity` (réplica del test de Python: `max |diff| < 0.25`), `lowering_polyphony_releases_extras`, `polyphony_is_clamped` (0 → 1, 99 → max), `glide_sanitised` (`set_glide(f64::NAN)`, `-5.0`, `inf` dan 0), `panic_silences_everything` (tras `panic` y 1000 bloques, `active_notes()` vacío y la salida es ceros), `fill_active_notes_matches_active_notes` (misma lista ordenada tras un acorde con robo).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test voice_manager`. Expected: FAIL.
- [ ] **Step 3: Implement** `VoiceManager` portando `voice_manager.py` más el glide/legato. Las voces viven en un `Vec<Voice>` creado en `new`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): voice manager with steal, polyphony, glide and legato"`.

### Task 4: Núcleo del Synth (parámetros suavizados, LFOs, matriz, controles)

**Files:**
- Create: `tools/ref_synth.py`, `rust/nomad-core/src/synth.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_synth.py`, `rust/nomad-core/tests/synth.rs`

**Interfaces:**
- Consumes: `VoiceManager`, `Lfo`, `ModMatrix`, `Sources`, `Smoother`, `SYNTH_PARAMS`, `pid`, `Mods`.
- Produces (`nomad_core::synth`): `pub const CONTROL_BLOCK: usize = 256; pub const PITCH_SEMITONES: f64 = 12.0;` `#[derive(Clone, Copy, PartialEq, Debug)] pub enum Control { Bend, ModWheel }`; `pub struct Synth { /* privados */ }`; `Synth::new(sample_rate: f64, max_voices: usize) -> Synth` (LFOs con semillas 1 y 2); `note_on(&mut self, note: i64, velocity: f64)`; `note_off(&mut self, note: i64)`; `set_param(&mut self, id: usize, value: f64)` (clamp del registro; `pid::MASTER_VOLUME` y ids ≥ 63 se ignoran); `set_control(&mut self, ctl: Control, value: f64)` (acota a [-1, 1], `NaN` → 0); `panic(&mut self)`; `render_block(&mut self, out: &mut [f64])` con `out.len() <= CONTROL_BLOCK`, que **suma** las voces; `values(&self) -> &ParamValues` (los valores objetivo); `is_live(&self, note: usize) -> bool` (teclas sostenidas por `note_on`/`note_off`, no por el secuenciador). Semántica = `SynthInstrument` de `nomad/engine/instrument.py`: parámetros en el dominio normalizado con `Smoother` (constante `smooth_ms` del registro) para los de `_SMOOTHED` y asignación inmediata para el resto (`polyphony` además llama a `set_polyphony`, `glide` a `set_glide`, `modN_*` configuran la matriz), controles con suavizado de 10 ms, los dos LFOs avanzan por bloque, `modwheel` y `bend·bend_range` por bloque, y la resolución de la matriz por voz con `PITCH_SEMITONES`.
- Python: `ref_synth.write` escribe `synth.{f64,json}` y además `synth_sets.json` con las listas `smoothed` (nombres de `_SMOOTHED`) e `immediate` (el resto sin `master_volume`). Escenarios con `SynthInstrument(48000)` sin motor ni secuenciador: se llama a sus métodos públicos y, entre bloques, `drain()` y `render_block(b, n)` con `n` en `[256, 256, 100, 256, ...]`. Casos: `"pad"` (polyphony 6, un acorde de 4 notas, ataque lento, LFO1 triángulo a 0.25 Hz → cutoff por la matriz, 20000 muestras), `"param_sweep"` (barrido de `filt_cutoff` de 200 a 8000 con un `set_param` por bloque), `"controls"` (bend 1.0 con `bend_range` 12 y modwheel 1.0 ruteado a pitch), `"glide_mono"` (poly 1, glide 80, tres notas), `"lfo_sh"` (LFO2 S&H a 8 Hz → pw1, osc1 cuadrada), `"clamps"` (`set_param` con valores fuera de rango y `nan`).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `synth.rs`: `scenarios_match_python` (1e-7), `smoothed_set_matches_python` (los `smoothed`/`immediate` de `synth_sets.json` coinciden con los que usa `Synth` — exponer `pub fn is_smoothed(id: usize) -> bool`), `set_param_clamps_and_ignores_master`, `set_control_clamps`, `render_adds_into_buffer`, `is_live_tracks_note_on_off` (y `panic` lo limpia), `panic_goes_silent`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test synth`. Expected: FAIL.
- [ ] **Step 3: Implement** `Synth` portando `render_block`, `_handle`, `_resolve` y el constructor de `instrument.py`.
- [ ] **Step 4: Run** `cd rust && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` y `python -m pytest tests -q --ignore=tests/test_stream_instances.py`. Expected: todo PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): synth DSP core"`.
