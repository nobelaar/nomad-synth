# NOMAD en Rust, M2-B (entrada MIDI) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Portar la entrada MIDI de Python a Rust: parseo de mensajes, el router (teclas sostenidas, pedal, rueda, panic), el mapa de CC con "learn" y su archivo JSON compatible, y la conexión real con `midir`, hasta poder tocar el Synth con un teclado y mover parámetros con perillas, desde la consola.

**Architecture:** Todo en `nomad-runtime` (módulo `midi`), reusando el registro de parámetros de `nomad-core`. Las piezas puras (`parse`, `router`, `map`) se verifican contra referencias exportadas de Python (`nomad/midi/midi_input.py` y `mapping.py` son la especificación). `MidiInput` corre en el hilo de `midir`, mantiene el router y el mapa detrás de un `Mutex` (ese hilo no es de audio) y empuja `Command`s al hilo de audio por una **segunda** cola SPSC, para que dos productores nunca compartan una cola.

**Tech Stack:** Rust 1.100, `midir` 0.11 (leer su documentación), `serde_json`, el resto ya presente. Sin `unsafe`.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M2). Sigue: M3 (interfaz).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Python nuevo sólo en `tools/` y `tests/`.
- El archivo del mapa es el mismo de Python: `<config>/midi-map.json` con `{"version": 1, "bindings": [{"port", "channel", "cc", "param"}]}`; `<config>` = `$XDG_CONFIG_HOME/nomad-synth` o `~/.config/nomad-synth` (como `default_config_dir` de `nomad/main.py`). Debe leer los archivos reales ya existentes (hay uno en `~/.config/nomad-synth/midi-map.json`: **no modificarlo** en tests; los tests usan copias en un directorio temporal).
- El hilo de audio no gana asignaciones ni bloqueos: los `Command` nuevos ya existen (M2-A). Los tests `no_alloc` de M2-A deben seguir en verde.
- Trazas de eventos y estados enteros **exactos**; floats con `|a-b| <= 1e-12 + 1e-12*|b|` (valores `from_norm`).
- Los tests no pueden exigir hardware MIDI: los que necesitan un puerto llevan `#[ignore = "requires ALSA sequencer"]` (el sistema de esta máquina tiene `/dev/snd/seq` y un controlador AKM322, así que se corren a mano).
- Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y los pytest existentes sin regresión. Rust: `export PATH="$HOME/.cargo/bin:$PATH"`.

## Review Focus

1. Bytes truncados, de sistema (`0xF0`, `0xF8`), con bytes de datos ≥ `0x80` o canales altos: devuelven `None`, nunca `panic` (Task 1).
2. `note_on` con velocidad 0 es `note_off`; el pitch bend en `0` y en los extremos (`-8192` y `8191`) da `-1.0` y `1.0` exactos (Task 1).
3. Pedal de sustain: las notas soltadas con pedal abajo se sueltan al levantarlo; volver a tocar una nota pendiente cancela su apagado; CC 123 y CC 120 (Task 2).
4. El mapa: coincidencia exacta de puerto gana a la de reserva (mismo canal y CC en otro puerto); re-aprender un parámetro borra su CC anterior; parámetros discretos se redondean y acotan; un parámetro desconocido en el archivo se descarta (Task 3).
5. Archivo corrupto, versión distinta o forma inválida: mapa vacío con `error`, **sin tocar el archivo**; un guardado que falla conserva el archivo anterior (Task 3).
6. Cola MIDI llena: el callback de `midir` descarta y cuenta, no bloquea; un `panic` en el callback queda contenido; si el puerto desaparece, `reset()` suelta las teclas sostenidas (Task 4).

---

### Task 1: Mensajes MIDI

**Files:**
- Create: `tools/ref_midi_parse.py`, `rust/nomad-runtime/src/midi/mod.rs`, `rust/nomad-runtime/src/midi/parse.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-runtime/src/lib.rs`
- Test: `tests/test_ref_midi_parse.py`, `rust/nomad-runtime/tests/midi_parse.rs`

**Interfaces:**
- Produces (`nomad_runtime::midi::parse`): `#[derive(Clone, Debug, PartialEq)] pub enum MidiKind { NoteOn, NoteOff, Cc, Bend }`; `#[derive(Clone, Debug, PartialEq)] pub struct MidiEvent { pub kind: MidiKind, pub note: u8, pub velocity: f64, pub cc: u8, pub value: u8, pub channel: u8, pub bend: f64 }` (sin el nombre de puerto: lo agrega `MidiInput`); `pub fn parse_message(bytes: &[u8]) -> Option<MidiEvent>`. Semántica = `parse_message` de `nomad/midi/midi_input.py` aplicada a `mido.Message.from_bytes(bytes)`: `note_on`/`note_off` (velocidad 0 = apagado, `velocity = vel/127`), `control_change`, `pitchwheel` (`bend = clamp(pitch/8191, -1, 1)` con `pitch = (data2 << 7 | data1) - 8192`), cualquier otro tipo o mensaje malformado → `None`.
- Python: `ref_midi_parse.write` escribe `midi_parse.json` (lista de `{"bytes": [...], "event": null | {"kind","note","velocity","cc","value","channel","bend"}}`) para ~60 mensajes: note on/off en canales 0, 7 y 15 con velocidades 0, 1, 64, 127; CC 1, 64, 120, 123 y otros; pitch bend en `0x0000`, `0x2000`, `0x3FFF`, `0x1FFF`; y casos que Python devuelve `None`: reloj `[0xF8]`, `[0xFE]`, sysex `[0xF0, 0x7E, 0xF7]`, program change, aftertouch, `[0x90, 60]` (truncado), `[0x90, 128, 64]`, `[]`.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date` (regenerar y comparar). Rust `midi_parse.rs`: `matches_python` (todos los casos, eventos iguales; floats con tolerancia), `never_panics_on_any_short_input` (todas las secuencias de 0 a 3 bytes con valores en `{0x00, 0x7F, 0x80, 0x90, 0xB0, 0xE0, 0xF0, 0xF8, 0xFF}`).
- [ ] **Step 2: Run** `python -m pytest tests/test_ref_midi_parse.py` y `cd rust && cargo test -p nomad-runtime --test midi_parse`. Expected: FAIL.
- [ ] **Step 3: Implement** `parse_message`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): MIDI message parsing"`.

### Task 2: Router

**Files:**
- Create: `tools/ref_midi_router.py`, `rust/nomad-runtime/src/midi/router.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-runtime/src/midi/mod.rs`
- Test: `tests/test_ref_midi_router.py`, `rust/nomad-runtime/tests/midi_router.rs`

**Interfaces:**
- Consumes: `MidiEvent` (Task 1), `Control` (`nomad-core`).
- Produces (`nomad_runtime::midi::router`): `pub trait RouterSink { fn note_on(&mut self, note: u8, velocity: f64); fn note_off(&mut self, note: u8); fn set_control(&mut self, ctl: Control, value: f64); fn panic(&mut self); fn other_cc(&mut self, cc: u8, value: u8, channel: u8); }` y `pub struct MidiRouter { /* held, pending: bitsets de 128, sustain: bool */ }` con `MidiRouter::new()`, `handle(&mut self, e: &MidiEvent, sink: &mut impl RouterSink)`, `reset(&mut self, sink: &mut impl RouterSink)`. Semántica = `MidiRouter` de `nomad/midi/midi_input.py` (CC 1 → `ModWheel` con `value/127`; CC 64 pedal con umbral 64 y vaciado de pendientes al soltar; CC 123 suelta las teclas físicas sostenidas tras vaciar pendientes; CC 120 limpia todo y llama `panic`; bend → `Control::Bend`; el resto de CC va a `other_cc`; las notas se sueltan en orden ascendente como `sorted`). Sin asignaciones por evento.
- Python: `ref_midi_router.write` escribe `midi_router.json`: escenarios como lista de eventos `{"kind", "note", "velocity", "cc", "value", "channel", "bend"}` más `("reset",)`; la salida es la **traza** de llamadas a un motor falso: `["note_on", n, vel]`, `["note_off", n]`, `["control", "bend"|"modwheel", valor]`, `["panic"]`, `["cc", cc, valor, canal]`. Casos: `"basic_chord"`, `"vel0_is_off"`, `"sustain_hold_release"` (notas con pedal, soltar el pedal), `"retap_cancels_pending"`, `"cc123_all_notes_off"`, `"cc120_panic"`, `"modwheel_bend"`, `"other_cc_forwarded"`, `"reset_releases_everything"` (teclas + pedal abajo y `reset`), `"sustain_threshold"` (valores 63 y 64).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `midi_router.rs` con un `RecordingSink`: `traces_match_python` (todas, **exactas**), `no_allocation_per_event` (con el asignador de conteo de M2-A en un binario propio o reusando el helper si es compartible: 0 asignaciones en un lote de 10000 eventos mezclados), `reset_is_idempotent`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-runtime --test midi_router`. Expected: FAIL.
- [ ] **Step 3: Implement** `MidiRouter`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): MIDI router with sustain and panic"`.

### Task 3: Mapa de CC y "learn"

**Files:**
- Create: `tools/ref_midi_map.py`, `rust/nomad-runtime/src/midi/map.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-runtime/src/midi/mod.rs`, `rust/nomad-runtime/Cargo.toml` (agregar `serde_json` como dependencia normal)
- Test: `tests/test_ref_midi_map.py`, `rust/nomad-runtime/tests/midi_map.rs`

**Interfaces:**
- Consumes: `SYNTH_PARAMS`, `pid`, `find` (`nomad-core`); `MidiEvent`.
- Produces (`nomad_runtime::midi::map`): `pub const VERSION: i64 = 1;` `pub struct MidiMap { pub error: Option<String>, /* privados: path, bindings: Vec<((String, u8, u8), String)> en orden de inserción, armed: Option<String> */ }`; `MidiMap::new(path: Option<PathBuf>) -> MidiMap`; `bindings(&self) -> &[((String, u8, u8), String)]`; `learning(&self) -> Option<&str>`; `arm(&mut self, param: Option<&str>) -> Result<(), UnknownParam>` (nombre fuera de `SYNTH_PARAMS` → `Err`); `unbind(&mut self, param: &str)`; `handle(&mut self, port: &str, e: &MidiEvent) -> Option<(usize /* pid */, f64)>` (coincidencia exacta `(port, canal, cc)`, luego reserva por `(canal, cc)` en cualquier puerto; con `armed`, enlaza ese CC, borra los enlaces previos del parámetro, desarma y devuelve el valor; `value = from_norm(param, e.value/127)`; parámetros discretos —`polyphony` o nombres que terminan en `_wave`, `_mode`, `_sync`, `_src`, `_dst`— se redondean a entero y se acotan a `[lo, hi]`; eventos que no son CC → `None` sin desarmar); `save(&self) -> std::io::Result<()>` (sin `path` no hace nada; crea el directorio; escribe `{"version": 1, "bindings": [{"port","channel","cc","param"}]}` con sangría de 2 espacios, de forma atómica con archivo temporal + `rename`, y conserva el archivo anterior si falla); `load(&mut self)` (nunca falla: archivo inexistente → vacío sin error; JSON corrupto, versión distinta o forma inválida → vacío con `error = Some("<path>: <motivo>")` **sin tocar el archivo**; entradas inválidas —puerto no string, canal fuera de 0..=15, cc fuera de 0..=127, parámetro desconocido— se descartan); `pub fn default_config_dir() -> PathBuf`. El orden de inserción replica el de un `dict` de Python (asignar a una clave existente conserva su posición; la clave nueva va al final).
- Python: `ref_midi_map.write` escribe `midi_map.json` con escenarios de operaciones `("arm", p)`, `("event", port, canal, cc, valor)`, `("unbind", p)`, `("save",)`, `("load",)` sobre un `MidiMap(path)` de un directorio temporal, guardando por operación el resultado `[param, valor] | null` y los `bindings` resultantes en orden; más una carpeta `rust/reference/midi_map_files/` con los archivos que Python **escribió** (`saved_after_learn.json`) y casos inválidos escritos a mano (`corrupt.json`, `wrong_version.json`, `bindings_not_list.json`, `bad_entries.json` con entradas buenas y malas mezcladas, `unknown_param.json`) más una **copia de la forma** del archivo real del autor (sólo estructura: puerto genérico `"TEST:TEST MIDI 1 24:0"`, no los datos reales). Casos: `"learn_binds_next_cc"`, `"learn_disarms"`, `"fallback_other_port"`, `"exact_beats_fallback"`, `"relearn_drops_old"`, `"discrete_rounded"` (`osc1_wave`, `polyphony`, `lfo1_wave`), `"unknown_cc_none"`, `"non_cc_keeps_armed"`, `"unbind"`, `"continuous_exp"` (`filt_cutoff` y `amp_attack`, curva exponencial).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date` (incluye los archivos generados). Rust `midi_map.rs` con directorios temporales (`std::env::temp_dir()` + nombre único; sin dependencias nuevas): `scenarios_match_python` (resultados con 1e-12, `bindings` en orden **exacto**), `loads_python_written_file` (los de `midi_map_files/`), `bad_files_give_empty_map_with_error_and_untouched_file` (el contenido y la fecha de modificación del archivo no cambian), `bad_entries_are_dropped_good_kept`, `save_roundtrips_and_matches_python_format` (guardar y comparar el JSON parseado con el `saved_after_learn.json` de Python, mismos pares clave/valor y orden), `save_creates_directory_and_leaves_no_tmp`, `save_failure_keeps_old_file` (ruta cuyo directorio padre es de sólo lectura, o archivo destino que es un directorio: el original sigue intacto), `path_none_is_noop`, `arm_unknown_param_errors`, `default_config_dir_honors_xdg` (con `XDG_CONFIG_HOME` seteado, en un test aislado en su propio binario o con un mutex para no pisar otras pruebas).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-runtime --test midi_map`. Expected: FAIL.
- [ ] **Step 3: Implement** `MidiMap` portando `nomad/midi/mapping.py`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): MIDI CC map with learn, JSON-compatible"`.

### Task 4: Entrada real con `midir` y `nomad midi`

**Files:**
- Create: `rust/nomad-runtime/src/midi/input.rs`
- Modify: `rust/nomad-runtime/src/core.rs` (segunda cola, telemetría), `rust/nomad-runtime/src/lib.rs`, `rust/nomad-runtime/src/command.rs` (sin variantes nuevas salvo que haga falta), `rust/nomad-runtime/Cargo.toml` (`midir`), `rust/nomad-app/src/main.rs`
- Test: `rust/nomad-runtime/tests/midi_input.rs`

**Interfaces:**
- Consumes: `parse_message`, `MidiRouter`, `MidiMap`, `Command`, `AudioCore`.
- Produces: `create(engine, sample_rate) -> (AudioCore, ControlHandle)` **sin cambiar su firma**; nuevo `ControlHandle::midi_sender(&mut self) -> Option<MidiSender>` (se entrega una sola vez: segunda cola SPSC de capacidad 1024 creada en `create`, que `AudioCore` vacía en cada callback tras los comandos de control) con `MidiSender::send(&mut self, cmd: Command) -> bool` (descarta y cuenta en `Telemetry.midi_dropped: AtomicU64` si está llena). `Telemetry` gana `target_synth: AtomicU32` (id del Synth objetivo, actualizado por el audio al aplicar `SetTargetSynth`, inicial = id del primer Synth) y `midi_events: AtomicU64`, `midi_errors: AtomicU64`.
- `nomad_runtime::midi::input`: `pub fn list_input_ports() -> Vec<String>` (nombres, sin panics si no hay ALSA); `pub struct MidiInput` con `MidiInput::open(port: Option<&str> /* subcadena del nombre o None = el primero que no sea "Midi Through" */, sender: MidiSender, map: Arc<Mutex<MidiMap>>, telemetry: Arc<Telemetry>) -> Result<MidiInput, String>`, `reset(&self)` (bajo el `Mutex` del router suelta teclas y pedal por el `sender`), `port_name(&self) -> &str`, `is_alive(&self) -> bool` (falso si el puerto desapareció o el callback registró un error irrecuperable). El callback de `midir`: `catch_unwind`; parsea; si es nota, pitch bend o CC de rueda/pedal/panic usa el router con un `RouterSink` que manda `Command::NoteOn/NoteOff/SetControl/Panic`; para el resto de CC consulta `MidiMap::handle(port, &event)` y, si devuelve `(pid, valor)`, manda `Command::SetParam { id: target_synth, param: pid, value }`. El `MidiMap` queda compartido (`Arc<Mutex<..>>`) para que M3 haga *learn*.
- App: `nomad midi [--port TEXTO]` (lista puertos; si hay uno, imprime cada evento recibido con su traza hacia el motor, 20 s o hasta Ctrl-C) y `nomad demo --midi [TEXTO] [--midi-map RUTA]` (suma la entrada MIDI al demo: teclado toca el Synth y las perillas mapeadas mueven parámetros; por defecto el mapa es `<config>/midi-map.json`, y **sólo se lee**: `demo` nunca guarda).

- [ ] **Step 1: Write the failing tests.** `midi_input.rs`: `list_ports_does_not_panic`, `sender_drops_and_counts_when_full` (sin ALSA: sobre `create` directo, 1024+N envíos), `router_sink_maps_events_to_commands` (con un `RouterSink` real conectado a un `MidiSender` y un `AudioCore` en un hilo de prueba: nota → `Engine` toca y `active` lo refleja tras un render), `cc_map_sets_param_on_target_synth` (mapa con `21 → filt_cutoff`, evento CC 21 valor 127: tras render, `Synth::values()[pid::FILT_CUTOFF]` ≈ `hi`; con `SetTargetSynth` a un segundo Synth, el parámetro cambia **en ese**), `callback_panic_is_contained`. Tests con ALSA, `#[ignore = "requires ALSA sequencer"]`: `virtual_port_roundtrip` (crea un puerto virtual con `midir::MidiOutput::create_virtual`, abre `MidiInput` sobre él, manda `[0x90, 60, 100]` y comprueba que llega un `NoteOn`), `reset_releases_held_keys`.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-runtime --test midi_input`. Expected: FAIL.
- [ ] **Step 3: Implement** `input.rs`, la segunda cola y los campos de telemetría, y los subcomandos. Los tests `no_alloc` de M2-A deben seguir verdes con la segunda cola.
- [ ] **Step 4: Run** todo + fmt/clippy + a mano: `cargo run -p nomad-app -- midi` con el AKM322 conectado (anotar la salida real) y `cargo run -p nomad-app -- demo --midi --seconds 10` tocando el controlador (anotar `xruns`).
- [ ] **Step 5: Commit** `git commit -m "feat(rust): midir input and nomad midi/demo --midi"`.
