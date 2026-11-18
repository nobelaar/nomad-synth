# NOMAD en Rust, M1-D (batería, mezclador y topología) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Completar `nomad-core`: la batería (síntesis de los 6 sonidos y la máquina con su secuenciador), el mezclador con nivel/mute/master, y la topología dinámica (agregar y cerrar instancias, relojes seguidores con traspaso). Al final, el motor entero de Rust reproduce el audio de referencia de Python (`tests/data/engine_golden.npy` incluido).

**Architecture:** Mismo método que M1-A/B/C (Python de `nomad/engine/` = especificación; referencias binarias `f64` + JSON). `Engine` (Rust) es dueño de todas las instancias y de sus relojes, y se maneja por métodos directos; la cola de comandos hacia el hilo de audio es del hito M2. Los relojes se guardan en un arreglo del `Engine` y las instancias los referencian por índice (equivale a `eclock`).

**Tech Stack:** igual que M1-A/B/C.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M1, último tramo). Después: M2 (audio y MIDI reales, sin interfaz).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Python nuevo sólo en `tools/` y `tests/`.
- `f64`; tolerancia `|a-b| <= 1e-9 + 1e-9*|b|`; trazas de eventos y estados enteros **exactos**. Excepción única, documentada: si `standard_normal` no puede igualar a numpy bit a bit (Task 2), las baterías con ruido se verifican por espectro y envolvente RMS.
- Sin asignaciones ni bloqueos en `render`, `render_block`, `render_chunk`, `poll`, `advance` y los métodos de comando. La síntesis de un one-shot (`render_one_shot`) **sí** asigna y nunca corre en el hilo de audio: la máquina recibe el resultado con `set_bank`.
- Sin `unsafe` y sin dependencias nuevas en `[dependencies]`.
- Cada `.f64` < 200 KB. Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y el pytest de lo nuevo en verde. Rust: `export PATH="$HOME/.cargo/bin:$PATH"`.

## Review Focus

1. `standard_normal` con las semillas 1 a 6 coincide exacto con numpy en las primeras 10000 muestras y en las 20 primeras muestras de cola de la semilla 3 (Task 2).
2. Los one-shots son finitos, de pico 0.9 y de largo igual al de Python para `tune`/`decay` en los extremos de su rango y con valores no finitos (Task 3).
3. El multiplicador por ganancia de la batería se hace en `f32` como en numpy (`float32 * escalar de Python`): la salida coincide a 1e-9, no sólo "parecida" (Task 4).
4. `stop_all` desvanece en 5 ms sin click y deja a la batería en silencio; reiniciar el reloj no repite pasos (Task 4).
5. Nivel fuera de [0, 1] o no finito, mute con rampa sin click y `master_volume` suavizado: la salida del `Engine` por defecto es idéntica a `engine_golden.npy` (Task 5).
6. Un seguidor no se puede encadenar, no se puede seguir a sí mismo ni a una instancia cerrada, y una fuente con seguidores no puede pasar a seguidor (Task 6).
7. Cerrar la fuente de un seguidor le traspasa `bpm`, `pos` y `running` sin saltos, y el cerrado se silencia con fundido antes de soltarse (Task 6).

---

### Task 1: Verificación completa del audio de referencia de M1-C

**Files:**
- Modify: `tools/ref_synth_seq.py`, `rust/nomad-core/tests/synth_seq.rs`
- Test: `tests/test_ref_synth_seq.py`

**Interfaces:**
- Produces: en `meta` de cada escenario `audio/*` de `synth_seq`, los campos `"sum"` y `"sumsq"` (floats) con la suma y la suma de cuadrados de **todas** las muestras del render completo (no sólo las diezmadas). `synth_seq.rs` los compara con `|a-b| <= 1e-6 + 1e-9*|b|` (son acumulaciones largas).

- [ ] **Step 1: Write the failing test.** En `synth_seq.rs`: `full_render_checksums_match_python` (por escenario, la suma y la suma de cuadrados de lo que renderiza Rust). Python: `test_reference_is_up_to_date` sigue siendo el control de frescura.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-core --test synth_seq`. Expected: FAIL (falta el campo).
- [ ] **Step 3: Implement** el cálculo de `sum`/`sumsq` en el exportador (acumular en `float64` con `math.fsum` para `sum`) y regenerar referencias con `python tools/export_reference.py rust/reference`.
- [ ] **Step 4: Run** pytest + cargo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "test(rust): full-render checksums for synth+sequencer audio"`.

### Task 2: `standard_normal` compatible con numpy

**Files:**
- Create: `tools/ref_normal.py`
- Modify: `rust/nomad-core/src/rng.rs`, `tools/export_reference.py`
- Test: `tests/test_ref_normal.py`, `rust/nomad-core/tests/normal.rs`

**Interfaces:**
- Consumes: `NumpyRng` (M1-A: `next_u64`, `random`).
- Produces: `NumpyRng::standard_normal(&mut self) -> f64` equivalente a `Generator.standard_normal()` de numpy (algoritmo zigurat de 256 capas: `idx = r & 0xff`, `sign = (r >> 8) & 1`, `rabs = (r >> 9) & 0x000fffffffffffff`, `x = rabs * wi[idx]`, aceptación por `ki[idx]`, cola con `ZIGGURAT_NOR_R = 3.6541528853610088` y cuñas con `fi[idx]` usando `random()`). Las tablas `ki`, `wi`, `fi` se **generan en `new`/constante a partir de las fórmulas del zigurat** (`NOR_AREA = 0.00492867323399`, `r` anterior) o se incluyen como `const` si no se reproducen bit a bit; elegir lo que dé coincidencia exacta.
- Python: `ref_normal.write` escribe un blob por semilla, `normal_<seed>.{f64,json}` para `seed ∈ {1, 2, 3, 4, 5, 6}`, con la entrada `"normal"` = primeras **10000** salidas de `default_rng(seed).standard_normal(10000)` (80 KB cada uno, bajo el límite), y `normal_tail.{f64,json}` con `"tail_hits"`: los índices (dentro de 2 millones de muestras de la semilla 3) de las primeras 20 muestras con `|x| > 3.65`, y sus valores, que ejercitan la cola del zigurat.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `normal.rs`: `first_10000_match_numpy_exactly` (seis semillas, comparación **exacta** de bits), `tail_hits_match_numpy` (generar 2 millones con la semilla 3 y comprobar que en los índices exportados los valores son iguales exactos), `mean_and_variance_are_sane` (100000 muestras: media |m| < 0.02, varianza entre 0.97 y 1.03).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test normal`. Expected: FAIL.
- [ ] **Step 3: Implement** `standard_normal`. Si tras un esfuerzo razonable no hay coincidencia exacta, dejar la implementación de mejor esfuerzo, marcar los dos tests exactos con `#[ignore = "…motivo…"]`, **detenerse y reportar** (el resto del plan cambia: Task 3 usaría ruido inyectado y verificación espectral).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): numpy-compatible standard_normal"`.

### Task 3: Síntesis de los one-shots de batería

**Files:**
- Create: `tools/ref_drum_shots.py`, `rust/nomad-core/src/drum_synth.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_drum_shots.py`, `rust/nomad-core/tests/drum_synth.rs`

**Interfaces:**
- Consumes: `NumpyRng::standard_normal`, `DRUM_PARAMS`, `INSTRUMENTS`.
- Produces (`nomad_core::drum_synth`): `pub fn render_one_shot(kind: usize, sample_rate: f64, tune: f64, decay: f64) -> Vec<f32>` (`kind` indexa `INSTRUMENTS`; `tune`/`decay` se acotan con `DRUM_PARAMS`; equivale a `render_one_shot` de `nomad/engine/drums.py`: duración `_length`, pesos y constantes de `_kick/_snare/_clap/_tom/_hat`, ruido de la semilla `[1,2,3,4,5,6]`, `_noise` con pasa-altos de primer orden, `_band_noise` con `butter(1, [lo, hi], "bandpass")`, `cumsum` de frecuencia para el tono, caída final `linspace` de `0.01·sr`, normalizado a pico 0.9, y conversión final a `f32`). Públicos de apoyo: `pub fn butter1_bandpass(low: f64, high: f64) -> ([f64; 3], [f64; 3])` (frecuencias normalizadas a Nyquist; transformada bilineal con pre-distorsión, como `scipy.signal.butter`) y `pub fn lfilter(b: &[f64], a: &[f64], x: &mut [f64])` (forma directa II transpuesta, condiciones iniciales cero).
- Python: `ref_drum_shots.write` escribe `drum_shots.{f64,json}` (y los blobs extra que haga falta por tamaño): `"butter/<low>_<high>"` (los 6 coeficientes `b`,`a`) para 6 pares de frecuencias normalizadas; `"lfilter/basic"` (entrada de 200 muestras de ruido determinista y la salida con un `b`/`a` dados); `"shot/<kind>/<tune>/<decay>"` para los 6 instrumentos con `(tune, decay)` en `{(0,0.5), (-1,0), (1,1), (0.37,0.81)}` (los largos hacen crecer el archivo: guardar sólo cada muestra par de la salida **y** `sum`, `sumsq`, `len` y el pico en `meta`); `"shot/<kind>/nonfinite"` (`tune=nan`, `decay=inf`) y `"shot/<kind>/sr44100"` (a 44100 Hz).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `drum_synth.rs`: `butter_matches_scipy`, `lfilter_matches_scipy`, `shots_match_python` (largo igual, muestras guardadas con 1e-6 por ser `f32`, `sum`/`sumsq` con tolerancia relativa 1e-6), `shots_are_finite_peak_0_9` (pico `0.9` ± 1e-6 y todo finito, también en `nonfinite`), `deterministic` (dos llamadas iguales dan bytes iguales).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test drum_synth`. Expected: FAIL.
- [ ] **Step 3: Implement** `drum_synth.rs` portando las funciones de `drums.py`; usar `f64` internamente y convertir a `f32` sólo al final.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): synthesized drum one-shots"`.

### Task 4: La máquina de batería

**Files:**
- Create: `tools/ref_drums.py`, `rust/nomad-core/src/drums.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_drums.py`, `rust/nomad-core/tests/drums.rs`

**Interfaces:**
- Consumes: `drum_synth::render_one_shot`, `Clock`, `StepGrid`, `StepTracker`, `DRUM_PARAMS`.
- Produces (`nomad_core::drums`): `pub const ACCENT_GAIN: f64 = 1.0; pub const NORMAL_GAIN: f64 = 0.6; pub const FADE_MS: f64 = 5.0;` `pub struct DrumMachine { pub steps: [[u8; 16]; 6], pub grid: StepGrid, pub values: [f64; 20], /* privados: bank, active, fade, tracker, scratch */ }`; `DrumMachine::new(sample_rate: f64) -> DrumMachine` (síntesis de los 6 sonidos con los valores por defecto, patrón por defecto de `_default_pattern`, `grid.swing` y bpm tomados de `values`; **asigna**, se crea fuera del hilo de audio); `set_step(&mut self, track: usize, step: usize, level: u8)` (acota a 0..=2); `clear_pattern(&mut self)`; `set_value(&mut self, id: usize, value: f64) -> ValueEffect` con `pub enum ValueEffect { None, Regenerate(usize) /* track a re-sintetizar */, Bpm(f64) }` (aplica el clamp del registro; `swing` actualiza `grid.swing` directo; `tune`/`decay` devuelven `Regenerate(track)`; `bpm` devuelve `Bpm(v)` para que el llamador lo mande al reloj); `set_bank(&mut self, track: usize, shot: Vec<f32>)` (intercambio del sonido, sin asignar: recibe el `Vec` ya hecho); `trigger(&mut self, track: usize, velocity: f64)`; `stop_all(&mut self, clock: &mut Clock)` (`clock.stop()` y fundido de 5 ms); `position(&self, clock: &Clock) -> usize`; `poll(&mut self, clock: &Clock, clock_id: u64)`; `frames_to_event(&self, clock: &Clock) -> i64`; `render_chunk(&mut self, out: &mut [f64])` (**suma**); `playing(&self, clock: &Clock) -> bool`. La mezcla multiplica `bank[f32] * (ganancia como f32)` en `f32` y luego suma en `f64` (equivale a numpy con escalar de Python). El control de frecuencia de regeneración (`REGEN_COOLDOWN_S`, conjunto `dirty`, `flush`) **no** se porta aquí: es del lado de la interfaz (M3).
- Python: `ref_drums.write` escribe `drums.{f64,json}`: salida de `DrumMachine.render(out, n)` (el conductor autónomo del propio archivo, reloj propio) en bloques de 512, con los comandos `("start",)`, `("trigger", track, vel)`, `("set", nombre, valor)`, `("step", track, step, level)`, `("clear",)`, `("stop_all",)`, `("render", n)`; guarda la salida concatenada diezmada 1 de 20 más `sum`/`sumsq` completos en `meta`. Escenarios: `"drums/default_2bars"` (120 bpm, 2 compases), `"drums/accents"` (patrón con acentos en 3 pistas), `"drums/swing"` (swing 0.4), `"drums/tune_decay"` (cambio de `kick_tune`/`snare_decay` a mitad: la regeneración se hace en el exportador con `render_one_shot` y el escenario registra el momento del `set_bank`), `"drums/stop_all"` (parar con sonidos a medias), `"drums/trigger_manual"` (trigger sin reloj), `"drums/levels"` (`*_level` distintos y `velocity` no finita).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `drums.rs` con un mini conductor `tests/common` equivalente a `DrumMachine.render` (poll, `chunk = min(restante, frames_to_event)`, `render_chunk`, `clock.advance`): `scenarios_match_python` (1e-9 en las muestras guardadas, checksums con 1e-6), `default_pattern_matches_python` (los 6×16 pasos), `stop_all_fades_to_silence` (tras `stop_all` y 600 muestras la salida es ceros y el reloj parado), `set_value_effects` (`kick_tune` → `Regenerate(0)`, `bpm` → `Bpm`, `swing` cambia `grid.swing`, valores no finitos → default), `step_levels_clamped`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test drums`. Expected: FAIL.
- [ ] **Step 3: Implement** `DrumMachine` portando `DrumMachine` de `nomad/engine/drums.py` (`on_step`, `_trigger`, `render_chunk`, fundido de `stop_all`).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): drum machine runtime"`.

### Task 5: Mezclador (`Engine`) con nivel, mute y master

**Files:**
- Create: `tools/ref_engine.py`, `rust/nomad-core/src/engine.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`, `rust/nomad-core/tests/common/mod.rs`
- Test: `tests/test_ref_engine.py`, `rust/nomad-core/tests/engine.rs`

**Interfaces:**
- Consumes: `Synth`, `DrumMachine`, `Clock`, `Smoother`, `SYNTH_PARAMS` (`master_volume`).
- Produces (`nomad_core::engine`): `pub type InstrumentId = u32; #[derive(Clone, Copy, PartialEq, Debug)] pub enum Kind { Synth, Drums }`; `pub struct Engine { /* privados */ }`; `Engine::new(sample_rate: f64, max_voices: usize) -> Engine` (crea `Synth 01` con id 1 y `Machine Drum 01` con id 2, ambos Internal, nivel 1.0, y la mezcla maestra al valor por defecto de `master_volume`); `render(&mut self, out: &mut [f64])` (replica `Engine.render` de `nomad/engine/engine.py`: lazo `poll`/`min frames_to_event`/`_span` en bloques de `CONTROL_BLOCK`, rampa de ganancia por instancia hacia `gain_target`, suma, suavizado y rampa del master, `tanh`, y `advance` de cada instancia y de cada reloj raíz una vez por chunk; `out` se **escribe**, no se suma); comandos: `start(&mut self, id: InstrumentId) -> bool`, `stop(id) -> bool`, `set_bpm(id, bpm: f64) -> bool` (acota a [20, 300], no finito se ignora), `set_mute(id, muted: bool)`, `set_level(id, level: f64)` (acota a [0, 1], no finito → 1.0), `set_master_volume(&mut self, v: f64)`, `note_on(&mut self, note: i64, velocity: f64)` y `note_off(&mut self, note: i64)` hacia el Synth objetivo (por defecto `Synth 01`), `set_target_synth(&mut self, id) -> bool`; acceso: `synth_mut(&mut self, id) -> Option<&mut Synth>`, `drums_mut(&mut self, id) -> Option<&mut DrumMachine>`, `clock(&self, id) -> Option<&Clock>` (el reloj propio), `effective_clock(&self, id) -> Option<&Clock>`. `start`/`stop`/`set_bpm` devuelven `false` si la instancia es seguidora o no existe (Task 6 las hace seguidoras).
- Python: `ref_engine.write` escribe `engine.{f64,json}`: `"golden"` = `tests/data/engine_golden.npy` convertido a `f64` (8633 muestras) con la receta de `test_default_level_is_bit_identical_to_before` descrita en `meta`; y escenarios de operaciones (lista de `("note_on", n, v)`, `("note_off", n)`, `("start", id)`, `("stop", id)`, `("bpm", id, x)`, `("mute", id, b)`, `("level", id, x)`, `("master", v)`, `("param", nombre, valor)` para el Synth, `("render", n)`) sobre `Engine(48000)` + `InstrumentManager`: `"mix/levels"`, `"mix/mute_ramp"`, `"mix/master"`, `"mix/level_nonfinite"`, `"mix/bpm_clamp"`; cada `render` guarda 1 de cada 20 muestras y `sum`/`sumsq` en `meta`, y el estado `(pos, bpm, running)` de cada reloj propio tras cada `render`.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `engine.rs`: `default_engine_matches_golden` (la receta del `meta` de `golden`, **1e-9** muestra a muestra: es el test central de M1), `scenarios_match_python` (muestras diezmadas, checksums y relojes), `mute_ramps_without_click_and_keeps_clock_running`, `level_clamps`, `commands_fail_for_unknown_id`, `each_root_clock_advances_once_per_chunk` (réplica del test de Python con `render` en bloques de 37 y de 512).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test engine`. Expected: FAIL.
- [ ] **Step 3: Implement** `Engine` con las dos instancias por defecto (arreglo de instancias como `enum Instance { Synth(..), Drums(..) }` con campos comunes: nombre, id, reloj propio, `level`, `muted`, `igain`; la topología dinámica llega en Task 6, por ahora el arreglo tiene siempre dos elementos y `source = None`).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): engine mixer with levels, mute and master"`.

### Task 6: Topología dinámica y relojes seguidores

**Files:**
- Create: `tools/ref_topology.py`
- Modify: `rust/nomad-core/src/engine.rs`, `tools/export_reference.py`, `rust/nomad-core/tests/common/mod.rs`
- Test: `tests/test_ref_topology.py`, `rust/nomad-core/tests/topology.rs`

**Interfaces:**
- Consumes: `Engine` (Task 5).
- Produces en `Engine` (semántica de `InstrumentManager` + `Topology` de `nomad/engine/manager.py` y `topology.py`): `add_instrument(&mut self, kind: Kind) -> InstrumentId` (id siguiente; nombre `"<Etiqueta> NN"` con contador por tipo: etiquetas `"Synth"` y `"Machine Drum"`, formato `{:02}`); `instruments(&self) -> Vec<(InstrumentId, Kind, String)>` en orden de mezcla (sólo tests/GUI, asigna); `source_of(&self, id) -> Option<InstrumentId>`; `followers_of(&self, id) -> Vec<InstrumentId>`; `set_source(&mut self, id, src: Option<InstrumentId>) -> bool` con las reglas: falso si `id` no existe o `src == id`; falso si `src` no existe, ya es seguidor (cadena) o `id` tiene seguidores; verdadero sin cambios si ya es esa fuente; al volver a Internal se copian `bpm`, `pos`, `running` del reloj de la fuente anterior al reloj propio (traspaso); `remove_instrument(&mut self, id) -> bool` (sus seguidores pasan a Internal con traspaso; la instancia pasa a *cerrando*: se silencia con `close_now` —Synth: `panic` + `clock.stop()`; Drums: `stop_all`— **después** del traspaso, y sigue renderizando su fundido hasta `drop_closed`); `drop_closed(&mut self)` (suelta las cerradas; equivale al temporizador de 100 ms de la interfaz); `effective_clock(id)` devuelve el reloj de la fuente si la hay. `render` avanza una vez por chunk **sólo** los relojes raíz (instancias Internal, incluidas las cerrando).
- Python: `ref_topology.write` escribe `topology.{f64,json}`: escenarios de operaciones sobre `Engine(48000)` + `InstrumentManager(engine, schedule=cb)` donde `cb` guarda las funciones para ejecutarlas con la operación `("drop_closed",)`: `("add", "synth"|"drums")`, `("source", id, src|null)` con el booleano de retorno guardado en `meta`, `("remove", id)`, `("start", id)`, `("bpm", id, x)`, `("note_on", n, v)`, `("seq_on", id)` (activa el secuenciador del Synth con un patrón fijo de 16 notas), `("render", n)`, `("drop_closed",)`. Casos: `"topo/follow_master"` (el Synth sigue a la batería a 128 bpm 3 compases; réplica de `test_follower_steps_align_with_master`), `"topo/bpm_change_reaches_follower"`, `"topo/different_lengths"` (largo 12 en el seguidor), `"topo/close_master_handover"` (cerrar la fuente a mitad: el seguidor sigue sin saltos), `"topo/go_internal_keeps_tempo"`, `"topo/rules"` (cadena rechazada, auto-seguimiento, fuente con seguidores, fuente inexistente: los booleanos esperados en `meta`), `"topo/add_names_ids"` (`Synth 02`, `Machine Drum 02`, `Synth 03`: nombres e ids en `meta`), `"topo/closing_fade"` (cerrar un Synth con notas y renderizar antes y después de `drop_closed`). Cada `render` guarda 1 de cada 20 muestras y `sum`/`sumsq`, y tras cada operación el estado `(pos, bpm, running)` de cada reloj y la lista `(id, nombre)` de instancias.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `topology.rs` con un intérprete de operaciones compartido en `tests/common/mod.rs`: `scenarios_match_python` (audio con 1e-9, checksums con 1e-6, **estado de relojes con 1e-9, enteros y booleanos exactos**, nombres e ids exactos), `chain_rejected`, `follower_cannot_start_or_set_bpm`, `source_with_followers_cannot_follow`, `remove_hands_over_before_silencing` (el seguidor conserva `pos` y `running` del maestro cerrado), `closing_instance_renders_until_dropped`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test topology`. Expected: FAIL.
- [ ] **Step 3: Implement** el grafo dinámico sobre el arreglo de instancias de Task 5, portando `InstrumentManager.add/set_source/remove/_drop` y `Topology` (`roots`, `eff`) literalmente, con la `drop_closed` explícita. La asignación de memoria sólo ocurre en `add_instrument`/`remove_instrument` (fuera del hilo de audio en el diseño final).
- [ ] **Step 4: Run** `cd rust && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` y `python -m pytest tests -q --ignore=tests/test_stream_instances.py`. Expected: todo PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): dynamic topology and follower clocks"`.
