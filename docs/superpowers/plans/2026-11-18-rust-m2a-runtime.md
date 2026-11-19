# NOMAD en Rust, M2-A (runtime de audio sin interfaz) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hacer que el motor de Rust suene por una salida de audio real sin que el hilo de audio asigne memoria ni se bloquee: cola de comandos, recolección de basura fuera del hilo de audio, telemetría, salida con `cpal`, y herramientas para medir si hay xruns.

**Architecture:** `nomad-core` sigue sin dependencias. Un crate nuevo `nomad-runtime` (librería) contiene `AudioCore` (dueño del `Engine`, vive en el hilo de audio), las colas SPSC (`rtrb`) de comandos y de basura, la telemetría atómica y la salida `cpal`. Un crate nuevo `nomad-app` (binario `nomad`) expone comandos de consola (`devices`, `demo`, `bench`, `soak`). El MIDI es el plan M2-B. La interfaz gráfica es el hito M3.

**Tech Stack:** Rust 1.100, `cpal` 0.18, `rtrb` 0.4 (las APIs cambian entre versiones: leer la documentación de las versiones fijadas en `Cargo.lock`), `std::alloc` para el asignador de conteo en tests. Sin `unsafe` salvo la implementación de `GlobalAlloc` del test.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M2). Cierra los pendientes del reporte de M1-D (asignaciones en el hilo de audio).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. La app de Python no se toca.
- `nomad-core` no gana dependencias en `[dependencies]` (puede ganar `dev-dependencies` solo si hace falta). `cpal` y `rtrb` viven en `nomad-runtime`.
- **El hilo de audio no asigna, no libera y no bloquea**: ni `malloc`/`free`, ni `Mutex`, ni `println!`, ni `Vec::push` que pueda crecer. Se comprueba con un asignador de conteo (Task 1 y 2).
- Un `panic!` en el callback de audio no debe cruzar a `cpal`: se captura (`catch_unwind`), se emite silencio y se cuenta en `errors`.
- `f64` en el motor; la conversión a `f32` ocurre sólo al escribir al dispositivo. Mono duplicado a los canales 0 y 1; el resto de canales en cero; con 1 canal, sólo mono.
- Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y los tests de Python existentes sin regresión (`python -m pytest tests -q --ignore=tests/test_stream_instances.py`). Rust: `export PATH="$HOME/.cargo/bin:$PATH"`.
- Los tests no pueden exigir un dispositivo de audio: los que lo necesitan llevan `#[ignore = "requires an audio device"]`.

## Review Focus

1. Cero asignaciones durante `AudioCore::render` con comandos de todos los tipos, incluyendo agregar y cerrar instancias y reemplazar un sonido de batería (Tasks 1-2).
2. Cola de comandos llena: el productor nunca bloquea, descarta el comando y lo cuenta; el motor sigue estable (Task 2).
3. Valores `NaN`/infinitos/fuera de rango y ids inexistentes en comandos: se tratan como en las llamadas directas al `Engine`, sin `panic` (Task 2).
4. Instancia agregada con el arreglo lleno: se rechaza y la caja vuelve por la cola de basura; no se libera en el hilo de audio (Task 1).
5. Callbacks de tamaños arbitrarios (1, 37, 512, 1000 muestras) dan la misma salida que un único bloque grande (Task 2).
6. Dispositivo con 1, 2 y 6 canales; dispositivo que desaparece a mitad de la ejecución: la app informa y termina sin colgarse (Task 3).
7. La heurística de xruns no cuenta falsos positivos con ráfagas de 2 bloques con marcas de tiempo casi iguales (como JACK) y sí cuenta un retraso real (Task 3).

---

### Task 1: Endurecer el `Engine` para el hilo de audio (sin asignar)

**Files:**
- Modify: `rust/nomad-core/src/engine.rs`, `rust/nomad-core/src/drums.rs`, y lo que el asignador de conteo revele (por ejemplo `synth.rs`, `voice_manager.rs`)
- Test: `rust/nomad-core/tests/no_alloc.rs` (binario propio con `#[global_allocator]`)

**Interfaces:**
- Consumes: `Engine`, `Instance`, `DrumMachine::set_bank` (M1-D).
- Produces en `nomad_core::engine`:
  - `pub const MAX_INSTANCES: usize = 16;` y el arreglo de instancias pasa a `Vec<Box<Instance>>` con `Vec::with_capacity(MAX_INSTANCES)` creado en `new` (nunca crece).
  - `pub struct Instance` (ahora pública, `Send`) y `pub fn build_instance(sample_rate: f64, max_voices: usize, kind: Kind, id: InstrumentId, name: String) -> Box<Instance>` (asigna: se llama **fuera** del hilo de audio).
  - `Engine::insert_instance(&mut self, inst: Box<Instance>) -> Result<(), Box<Instance>>` (sin asignar; `Err` devuelve la caja si ya hay `MAX_INSTANCES`); `Engine::next_id(&mut self, kind: Kind) -> (InstrumentId, String)` para que el control construya la instancia con el id y nombre que dictan los contadores de Python (`Synth 02`, …); `add_instrument(kind)` se mantiene como atajo (`next_id` + `build_instance` + `insert_instance`) para tests.
  - `Engine::remove_instrument(&mut self, id) -> bool` sigue pasando la instancia a *cerrando* sin liberar; `Engine::drop_closed(&mut self, sink: &mut dyn FnMut(Box<Instance>))` entrega cada instancia cerrada por `sink` (la versión sin parámetro se elimina o queda sólo para tests).
  - `DrumMachine::set_bank(&mut self, track: usize, shot: Vec<f32>) -> Vec<f32>` devuelve el sonido reemplazado (o el que queda sin uso) para que lo libere el llamador, y `Engine::set_drum_bank(&mut self, id, track, shot: Vec<f32>) -> Option<Vec<f32>>`.
  - Ningún método de `render`/comandos (`note_on`, `set_param`, `start`, `set_level`, `set_source`, `render`, …) asigna. Los métodos de consulta que devuelven `Vec` (`active_notes`, `instruments`, `chord`, …) quedan documentados como "sólo fuera del hilo de audio".

- [ ] **Step 1: Write the failing test** `no_alloc.rs`: un `GlobalAlloc` que envuelve a `System` y cuenta `alloc`/`dealloc`/`realloc` **sólo** mientras una bandera `thread_local` está activa; helper `assert_no_alloc(|| ...)`. Tests: `render_is_allocation_free` (Engine por defecto con acorde de 8 notas, secuenciador activo en el Synth, batería corriendo, 2 s en bloques de 37/256/512), `commands_are_allocation_free` (con `assert_no_alloc`: `note_on/off`, `set_param` de todos los ids, `set_control`, `start/stop`, `set_bpm`, `set_mute`, `set_level`, `set_master_volume`, `set_source` y volver a Internal, `set_step`, edición de acordes del secuenciador, `trigger`, `set_drum_value` de `swing`), `insert_and_close_are_allocation_free` (construir la caja **fuera** del `assert_no_alloc`, insertarla dentro; cerrar con `remove_instrument` y soltar con `drop_closed` entregando las cajas a un `Vec` preexistente con capacidad; el `Vec` de destino se reserva antes), `set_bank_hands_back_old_shot` (el `Vec` reemplazado vuelve al llamador y la asignación del nuevo ocurrió fuera), `insert_rejects_when_full` (con `MAX_INSTANCES` instancias la inserción devuelve `Err(caja)`).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-core --test no_alloc`. Expected: FAIL (varias asignaciones y APIs faltantes). Cada fallo de asignación del contador debe mostrar un `Backtrace` para ubicar la causa.
- [ ] **Step 3: Implement** los cambios de arriba y eliminar las asignaciones que el contador revele (preasignar buffers en `new`, `copy` de relojes en vez de `clone` con heap, etc.); documentar con `shortcut:` cualquier límite.
- [ ] **Step 4: Run** `cargo test --workspace` completo (todos los tests de M1 deben seguir verdes sin tocar las referencias) más fmt/clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): allocation-free engine API for the audio thread"`.

### Task 2: Comandos, colas, `AudioCore` y telemetría

**Files:**
- Create: `rust/nomad-runtime/Cargo.toml`, `rust/nomad-runtime/src/lib.rs`, `rust/nomad-runtime/src/command.rs`, `rust/nomad-runtime/src/core.rs`, `rust/nomad-runtime/src/telemetry.rs`
- Modify: `rust/Cargo.toml` (agregar el miembro)
- Test: `rust/nomad-runtime/tests/core.rs`, `rust/nomad-runtime/tests/no_alloc.rs`

**Interfaces:**
- Consumes: `Engine`, `Instance`, `Control`, `Kind`, `InstrumentId` (`nomad-core`).
- Produces (`nomad_runtime`):
  - `pub enum Command` con una variante por operación que hoy la interfaz de Python hace sobre el motor: `NoteOn { note: u8, velocity: f32 }`, `NoteOff { note: u8 }`, `Panic`, `SetParam { id: InstrumentId, param: u16, value: f64 }` (parámetro del Synth por índice `pid`), `SetControl { ctl: Control, value: f64 }`, `SetMaster(f64)`, `SetTargetSynth(InstrumentId)`, `Start(InstrumentId)`, `Stop(InstrumentId)`, `SetBpm { id, bpm: f64 }`, `SetMute { id, muted: bool }`, `SetLevel { id, level: f64 }`, `SetSource { id, src: Option<InstrumentId> }`, `SeqSetStep { id, step: u8, on: bool, note: Option<u8>, vel: Option<u8> }`, `SeqAddNote { id, step: u8, note: u8 }`, `SeqTranspose { id, step: u8, delta: i16 }`, `SeqClear(InstrumentId)`, `SeqGate { id, gate: f64 }`, `SeqDivision { id, division: f64 }`, `SeqLength { id, length: u8 }`, `SeqSwing { id, swing: f64 }`, `SeqEnabled { id, enabled: bool }`, `SeqArm { id, on: bool }`, `SeqRest(InstrumentId)`, `DrumSetStep { id, track: u8, step: u8, level: u8 }`, `DrumClear(InstrumentId)`, `DrumTrigger { id, track: u8, velocity: f64 }`, `DrumValue { id, param: u16, value: f64 }`, `DrumSetBank { id, track: u8, shot: Box<Vec<f32>> }`, `AddInstrument(Box<Instance>)`, `RemoveInstrument(InstrumentId)`, `DropClosed`. `Command: Send`.
  - `pub enum Garbage { Instance(Box<Instance>), Shot(Box<Vec<f32>>) }`.
  - `pub struct Telemetry` (en `Arc`), con campos atómicos: `callbacks: AtomicU64`, `xruns: AtomicU64`, `errors: AtomicU64`, `dropped_commands: AtomicU64`, `render_ns_last: AtomicU64`, `render_ns_max: AtomicU64`, `late_us_max: AtomicU64`, y un anillo SPSC del osciloscopio (`ScopeRing`, f32 mono, 2 s a la frecuencia de muestreo) con `write(&[f32])` (audio) y `window(&self, n: usize, out: &mut [f32]) -> usize` (lector).
  - `pub fn create(engine: Engine, sample_rate: f64) -> (AudioCore, ControlHandle)` donde `ControlHandle { pub fn send(&mut self, cmd: Command) -> bool /* false = cola llena, descartado y contado */; pub fn reap(&mut self) -> usize /* suelta la basura; devuelve cuántos */; pub fn telemetry(&self) -> Arc<Telemetry> }` (capacidad de comandos 4096, de basura 256; `ControlHandle: Send`) y `AudioCore { pub fn render_stereo(&mut self, out: &mut [f32], channels: usize) }` que: vacía todos los comandos, llama `Engine::render` en un buffer `f64` preasignado (para tamaños mayores se procesa en tandas de hasta 4096 muestras), escribe en `out` (intercalado, `channels` ≥ 1: mono duplicado a canales 0 y 1), mide el tiempo de `render` (`Instant`) y alimenta el osciloscopio y los contadores. `AudioCore: Send` y todas sus asignaciones ocurren en `create`.
  - Los comandos con valores inválidos se aplican con las mismas reglas de saneamiento que las llamadas directas al `Engine`; un id inexistente se ignora. `AddInstrument` con el arreglo lleno devuelve la caja por la cola de basura; `RemoveInstrument`/`DropClosed` mandan las instancias cerradas por la cola de basura; `DrumSetBank` manda el sonido reemplazado por la cola de basura. Si la cola de basura está llena, el objeto se **retiene** en el audio core (en un `Vec` de capacidad fija) y se reintenta en el siguiente callback; nunca se libera ahí.

- [ ] **Step 1: Write the failing tests.** `core.rs`: `matches_direct_engine_bit_exact` (los mismos comandos y renders por `AudioCore` y directo en un `Engine` dan `out as f32` idéntico en tamaños de callback `[1, 37, 512, 1000]`), `command_order_is_preserved`, `full_queue_drops_and_counts` (4096+N envíos sin render: `send` devuelve `false` N veces, `dropped_commands == N` y el audio no se desestabiliza), `invalid_values_do_not_panic` (NaN, ±inf, `u8` fuera de rango como pasos, ids inexistentes), `stereo_and_channel_layouts` (1, 2 y 6 canales), `garbage_returns_removed_instances_and_old_shots` (`reap` > 0 y el sonido viejo llega intacto), `add_when_full_returns_box_via_garbage`, `panic_in_render_is_contained` (un `Command` de prueba `#[cfg(test)]` que provoque `panic!` produce silencio y suma `errors`). `no_alloc.rs` (binario con asignador de conteo, igual que en Task 1): `audio_core_render_is_allocation_free` con una lista de comandos de **todas** las variantes aplicada durante el render (la caja de `AddInstrument` y el `Vec` de `DrumSetBank` se construyen antes).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-runtime`. Expected: FAIL.
- [ ] **Step 3: Implement** el crate (`rtrb` para ambas colas). `core.rs`: `catch_unwind(AssertUnwindSafe(...))` alrededor de cada render; la telemetría con `Ordering::Relaxed`.
- [ ] **Step 4: Run** todo (+ fmt/clippy, `cargo test --workspace`). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): audio runtime with command queue, garbage queue and telemetry"`.

### Task 3: Salida de audio con `cpal` y la app `nomad devices|demo`

**Files:**
- Create: `rust/nomad-runtime/src/output.rs`, `rust/nomad-app/Cargo.toml`, `rust/nomad-app/src/main.rs`
- Modify: `rust/Cargo.toml`, `rust/nomad-runtime/src/lib.rs`
- Test: `rust/nomad-runtime/tests/output.rs`

**Interfaces:**
- Consumes: `AudioCore`, `ControlHandle`, `Telemetry` (Task 2).
- Produces (`nomad_runtime::output`): `pub struct DeviceInfo { pub index: usize, pub name: String, pub host: String, pub channels: u16, pub default_sample_rate: u32 }`; `pub fn list_output_devices() -> Vec<DeviceInfo>` (sin panics si no hay dispositivos: lista vacía); `pub fn pick_device(devices: &[DeviceInfo], default_index: Option<usize>) -> Result<usize, String>` (regla de `nomad/audio/stream.py`: un dispositivo del host JACK con ≥ 2 canales; si no, el predeterminado del sistema; si no, `Err` con el mensaje "No audio output device available. Check that PipeWire/JACK or ALSA exposes an output."); `pub struct LateTracker` con `new(sample_rate: f64)` y `observe(&mut self, t_seconds: f64, frames: usize) -> LateEvent` (`LateEvent { pub xrun: bool, pub late_ms: f64 }`) que porta la heurística de `AudioStream.callback`: referencia acumulada, `LATE_FACTOR = 1.5`, re-anclaje en cada xrun o cuando llega temprano; `pub struct AudioOutput` con `AudioOutput::start(core: AudioCore, device: Option<usize>, sample_rate: u32, block: u32) -> Result<AudioOutput, String>` (pide 48000 Hz y bloque 512; si el dispositivo no ofrece 48000 usa su frecuencia y lo informa con `pub fn sample_rate(&self) -> u32` — el `Engine` se construye **antes** con la frecuencia elegida: exponer `pub fn negotiate(device: Option<usize>, sample_rate: u32) -> Result<(usize, u32), String>`), `stop(&mut self)` y un callback de error de `cpal` que marca `errors` y un `AtomicBool device_lost`.
- App (`rust/nomad-app`, binario `nomad`, sin dependencias de CLI pesadas: parsear `std::env::args`): `nomad devices` (lista), `nomad demo [--device N] [--seconds S]` (arma un `Engine` por defecto, aplica el preset "Pad de acordes" y arranca el secuenciador del Synth y la batería —la receta del preset se escribe en la app con los valores de `nomad/presets.py` `FACTORY["synth"][0]` y `FACTORY["drums"][0]`—, reproduce S segundos e imprime una línea por segundo con `callbacks`, `xruns`, `errors`, `render_ns_max` y al final un resumen; sale con código 1 si hubo errores o el dispositivo se perdió).

- [ ] **Step 1: Write the failing tests.** `output.rs`: `pick_device_prefers_jack_stereo`, `pick_device_falls_back_to_default`, `pick_device_errors_without_devices` (mensaje exacto), `late_tracker_ignores_burst_of_two` (mismas marcas de tiempo cercanas en pares: 0 xruns), `late_tracker_counts_real_delay` (un retraso de 3 bloques: 1 xrun y `late_ms` ≈ 3 bloques), `late_tracker_reanchors_after_xrun`, `list_devices_does_not_panic`; un test `#[ignore = "requires an audio device"]` `plays_one_second_on_default_device` (abre el predeterminado, 1 s, `errors == 0`).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-runtime --test output`. Expected: FAIL.
- [ ] **Step 3: Implement** `output.rs` con `cpal` (formato de salida `f32`; si el dispositivo sólo ofrece otro formato, convertir en el callback sin asignar; `buffer_size` fijo en `block` si el dispositivo lo permite) y `main.rs`.
- [ ] **Step 4: Run** todo + fmt/clippy + `cargo run -p nomad-app -- devices` (anotar la salida) y, **si hay dispositivo**, `cargo run -p nomad-app -- demo --seconds 5` (anotar la salida; no bloquea el commit si el entorno no tiene audio, pero se reporta).
- [ ] **Step 5: Commit** `git commit -m "feat(rust): cpal audio output and nomad devices/demo"`.

### Task 4: Medición: `nomad bench` y `nomad soak`

**Files:**
- Create: `rust/nomad-app/src/bench.rs`
- Modify: `rust/nomad-app/src/main.rs`
- Test: `rust/nomad-app/tests/bench.rs`

**Interfaces:**
- Consumes: `Engine`, `AudioCore`, `AudioOutput`, `Telemetry`.
- Produces: `pub fn run_bench(voices: usize, synths: usize, seconds: f64, block: usize) -> BenchReport` con `BenchReport { pub blocks: usize, pub mean_pct: f64, pub p50_pct: f64, pub p99_pct: f64, pub max_pct: f64, pub over_budget: usize }` (porcentaje del presupuesto `block / sample_rate` por bloque medido con `Instant`; renderiza **sin** dispositivo, offline, con `synths` Synths de `voices` voces cada uno tocando acordes sostenidos, una batería con el patrón por defecto y el secuenciador del primer Synth activo; descarta los 50 primeros bloques de calentamiento). `nomad bench [--voices 8] [--synths 2] [--seconds 10] [--block 512]` imprime el informe y sale con código 1 si `max_pct > 100`. `nomad soak [--minutes 10] [--device N]` reproduce en vivo la misma escena durante M minutos e imprime cada 10 s `callbacks`, `xruns`, `errors`, `render_ns_max`, `late_us_max`; al final sale con código 0 sólo si `xruns == 0` y `errors == 0`.

- [ ] **Step 1: Write the failing tests.** `bench.rs`: `bench_runs_and_reports` (`run_bench(8, 1, 1.0, 512)` devuelve `blocks > 50`, porcentajes finitos y `p50 <= p99 <= max`), `bench_scales_with_voices` (con `voices` 1 vs 8 en 1 s, la media de 8 es mayor que la de 1), `bench_arg_parsing` (flags y valores por defecto, errores de uso).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app`. Expected: FAIL.
- [ ] **Step 3: Implement** `bench.rs` y los subcomandos. El conteo de voces por Synth se logra con acordes de `voices` notas sostenidas y `polyphony = voices`.
- [ ] **Step 4: Run** todo + `cargo build --release -p nomad-app && ./target/release/nomad bench --seconds 10` y anotar el informe en el mensaje del commit y en el reporte de entrega (comparar con el Python medido: 8 voces + batería usaban ~61 % de media con numba y picos de 4 a 30 veces el presupuesto en esta máquina).
- [ ] **Step 5: Commit** `git commit -m "feat(rust): nomad bench and soak measurement tools"`.
