# NOMAD en Rust, M3-C (batería, presets, MIDI learn, ayuda y cambio de dispositivo) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cerrar la paridad de la interfaz con la app de Python: vista de la Machine Drum, presets de fábrica y del usuario (mismo formato de archivo), MIDI learn con clic derecho y botón LEARN, modo ayuda con los textos de Python, y cambio de dispositivo de audio y de puerto MIDI en caliente.

**Architecture:** Sobre M3-A/M3-B: la interfaz lee espejos (`InstrumentShared`) y manda `Command`s por `AppModel`. Dos comandos nuevos cargan un patrón entero de una vez (`SeqLoadPattern`, `DrumLoadPattern`) para que un preset se aplique igual que en Python sin asignar en el audio. Los presets y los textos de ayuda son datos portados y verificados contra Python. El cambio de dispositivo devuelve el `AudioCore` desde el stream que se cierra, así el motor y su estado sobreviven.

**Tech Stack:** igual que M3-B.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M3) y los planes M3-A y M3-B. Referencia de comportamiento: `nomad/presets.py`, `nomad/gui/drums_view.py`, `nomad/gui/strip.py` (presets), `nomad/gui/app.py` (`_menu_items`, `_context`, `_toggle_learn`, `_arm`, `_learn_finished`, `_unbind`, modo ayuda, `_cycle_device`, `_cycle_midi`), `nomad/gui/help.py`. Sigue: M4 (instaladores).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Python nuevo sólo en `tools/` y `tests/`.
- Los archivos del usuario son los mismos que usa Python y **no se modifican en tests**: `<config>/presets/<kind>/<slug>.json` y `<config>/midi-map.json`. Los tests usan directorios temporales.
- Validación en el borde para archivos de usuario (presets): tipos, formas de arreglos, rangos, tamaño máximo 64 KB; un preset inválido no cambia nada.
- El hilo de audio sigue sin asignar: los comandos nuevos entran en los tests `no_alloc` (cajas construidas fuera, devueltas por la cola de basura).
- Textos visibles en español, iguales a los de Python donde existan.
- Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y los pytest sin regresión. Rust: `export PATH="$HOME/.cargo/bin:$PATH"`. El disco de `/home` está casi lleno: no compilar en `release` salvo cuando el plan lo pide, y no activar la feature `shots` salvo para las capturas.

## Review Focus

1. Un preset de usuario corrupto, demasiado grande, de otro tipo o con arreglos de forma incorrecta no se lista ni cambia el instrumento; uno con parámetros desconocidos se aplica ignorándolos (Task 1).
2. Aplicar un preset de batería regenera los seis sonidos con su `tune`/`decay` y no toca el BPM; aplicar uno de Synth no toca `master_volume` ni prende el secuenciador (Tasks 1-2).
3. Guardar un preset con el nombre de uno de fábrica (sin distinguir mayúsculas), vacío o sólo con símbolos: error claro, nada escrito (Task 3).
4. LEARN sin haber tocado un control avisa en la línea de estado; un CC aprendido se guarda en el mapa y la perilla deja de estar resaltada; "Quitar MIDI" desvincula y guarda (Task 4).
5. En modo ayuda los controles no reaccionan y el texto aparece tras 150 ms sobre el control; `F1` y `Esc` lo apagan (Task 5).
6. Cambiar de dispositivo de audio no pierde el estado del motor (patrones, parámetros, instrumentos) y un dispositivo que no abre deja un aviso y vuelve al anterior; cambiar de puerto MIDI suelta las teclas sostenidas (Task 6).

---

### Task 1: Presets (núcleo) y comandos de carga de patrón

**Files:**
- Create: `tools/ref_presets.py`, `rust/nomad-app/src/presets.rs`
- Modify: `rust/nomad-runtime/src/command.rs`, `rust/nomad-runtime/src/core.rs`, `rust/nomad-core/src/engine.rs` (métodos para cargar patrones enteros), `tools/export_reference.py`, `rust/nomad-app/src/lib.rs`
- Test: `tests/test_ref_presets.py`, `rust/nomad-app/tests/presets.rs`, y casos nuevos en los `no_alloc` de `nomad-runtime`

**Interfaces:**
- Produces en `nomad-runtime::Command`: `SeqLoadPattern { id: InstrumentId, pattern: Box<SeqPattern> }` con `pub struct SeqPattern { pub on: [u8; 16], pub note: [u8; 16], pub vel: [u8; 16], pub extra: [[u8; 5]; 16], pub gate: f64, pub division: f64, pub length: u8, pub swing: f64 }` (la caja vuelve por la cola de basura) y `DrumLoadPattern { id, steps: [[u8; 16]; 6] }`; en `Engine` los métodos sin asignar que los aplican (mismo efecto que el `apply` de `nomad/presets.py`: `division` al valor más cercano de `{0.25, 0.5, 1, 2, 4}`, `swing` acotado a [0, 0.6], `enabled` **no** cambia).
- Produces (`nomad_app::presets`): `pub enum PresetKind { Synth, Drums }`; `pub struct Preset { pub kind: PresetKind, pub name: String, pub json: serde_json::Value /* validado */ }`; `pub fn check(kind: PresetKind, data: &serde_json::Value) -> Result<(), String>` (las mismas reglas de `check` de Python, mensajes en inglés iguales: `"not a synth preset"`, `"missing params/seq"`, `"bad param value"`, `"bad sequence"`, `"bad pattern"`); `pub fn capture(inst: &InstrumentView, name: &str) -> serde_json::Value` (desde el espejo: parámetros del Synth sin `master_volume` o de la batería sin `bpm`, más la secuencia/patrón, con las mismas claves que `capture` de Python); `pub fn apply_commands(inst: &InstrumentView, preset: &serde_json::Value, sample_rate: f64) -> Result<Vec<Command>, String>` (valida primero; Synth: un `SetParam` por cada parámetro conocido + `SeqLoadPattern`; batería: un `DrumValue` por valor conocido + `DrumLoadPattern` + un `DrumSetBank` por pista con `render_one_shot` del `tune`/`decay` nuevos, renderizado **aquí**, fuera del audio); `pub fn factory(kind) -> Vec<serde_json::Value>` (los 4 de Synth y 3 de batería de `FACTORY`, **idénticos** a los de Python); `pub fn slug(name: &str) -> String`; `pub struct PresetStore { dir: Option<PathBuf> }` con `new(dir)`, `can_save()`, `list(kind) -> Vec<(String, Origin)>` (`Origin::{Factory, User}`; fábrica primero, después los del usuario válidos en orden de archivo), `get(kind, name) -> Result<serde_json::Value, String>`, `save(kind, name, data) -> Result<String, String>` (nombre limpio; rechaza vacío y nombres de fábrica sin distinguir mayúsculas; escritura atómica con archivo temporal y `rename`; formato de archivo **byte a byte** igual al de Python: `json.dumps(indent=1)` sin salto final). El directorio por defecto es `<config>/presets`.
- Python: `ref_presets.write` escribe `presets.json` con `FACTORY` completo, resultados de `check` para ~15 presets válidos e inválidos (aceptado/mensaje), `slug` de ~12 nombres, y una carpeta `rust/reference/preset_files/` con archivos guardados por `PresetStore.save` de Python (uno de Synth con acordes, uno de batería) y casos inválidos escritos a mano.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `presets.rs`: `factory_matches_python` (JSON igual), `check_matches_python`, `slug_matches_python`, `store_lists_factory_then_valid_user_files`, `store_rejects_reserved_empty_and_symbol_names`, `save_is_byte_identical_to_python`, `save_is_atomic_and_leaves_no_tmp`, `corrupt_or_oversize_files_are_skipped`, `capture_apply_roundtrip_through_engine` (aplicar "Pad de acordes" a un Synth con un `AudioCore` avanzado a mano y capturar desde el espejo da el mismo JSON de parámetros y secuencia), `drum_apply_regenerates_shots_and_keeps_bpm`, `apply_rejects_invalid_and_sends_nothing`. En `nomad-runtime/tests/no_alloc.rs`: los dos comandos nuevos dentro del render sin asignaciones.
- [ ] **Step 2: Run** pytest + `cd rust && cargo test -p nomad-app --test presets` y `cargo test -p nomad-runtime --test no_alloc`. Expected: FAIL.
- [ ] **Step 3: Implement** los comandos, los métodos del `Engine` y `presets.rs`, portando `nomad/presets.py`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): presets core, factory presets and pattern-load commands"`.

### Task 2: Vista de la Machine Drum

**Files:**
- Create: `rust/nomad-app/src/drums_view.rs`
- Modify: `rust/nomad-app/src/ui.rs`, `rust/nomad-app/src/lib.rs`
- Test: `rust/nomad-app/tests/drums_view.rs`

**Interfaces:**
- Consumes: `InstrumentShared::drum_snapshot`, `knob`, `KnobModel` con `DRUM_PARAMS`, `ParamSync` (sobre los 20 valores), `Command::{DrumSetStep, DrumTrigger, DrumValue, DrumSetBank, DrumClear}`, `render_one_shot`.
- Produces (`nomad_app::drums_view`): `pub struct DrumViewState` (sincronizadores de los 20 valores y un regenerador con el mismo enfriamiento que Python: `REGEN_COOLDOWN_S = 0.06`, que junta cambios de `tune`/`decay` y manda un `DrumSetBank` por pista tras el enfriamiento); `pub fn drums_view_ui(ui, state, inst, model, now)`: arriba la perilla `SWING` y `CLEAR`; una fila por instrumento (`KICK SNARE CLAP TOM CHAT OHAT`): botón con el nombre (dispara `DrumTrigger` con velocidad 1.0), perillas `tune`, `decay`, `level` (etiquetas cortas de `_SHORT`), y 16 pasos (nivel 0 `TROUGH`, 1 `GREEN`, 2 `ACCENT`; clic alterna 0↔1, `shift+clic` alterna 0↔2, igual que `toggle_step`; borde `PLAYHEAD` en el paso actual del reloj efectivo); menú contextual `Valor por defecto`. El layout se adapta como el del Synth (a < 1000 de ancho las perillas y los pasos van en dos renglones por instrumento). La pestaña de la batería no muestra el teclado.

- [ ] **Step 1: Write the failing tests** (egui_kittest con un `AudioCore` avanzado a mano): `rows_and_controls_exist`, `click_and_shift_click_follow_python_rules`, `name_button_triggers` (tras un render hay señal en la salida), `tune_change_regenerates_after_cooldown` (con reloj inyectable: antes de 60 ms no hay `DrumSetBank`, después sí, y el espejo de valores cambió), `swing_and_clear`, `playhead_follows_mirror`, `follower_drum_shows_source_tempo` (el BPM está en la barra de M3-A; aquí sólo se verifica que la batería seguidora no muestra controles de tempo propios).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test drums_view`. Expected: FAIL.
- [ ] **Step 3: Implement** `drums_view.rs` portando `DrumView` (`_build_top`, `_build_row`, `toggle_step`, `refresh`, `flush`).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): Machine Drum view"`.

### Task 3: Presets en la interfaz

**Files:**
- Modify: `rust/nomad-app/src/transport.rs`, `rust/nomad-app/src/ui.rs`
- Test: `rust/nomad-app/tests/presets_ui.rs`

**Interfaces:**
- Consumes: `PresetStore`, `capture`, `apply_commands`, `AppModel::send`.
- Produces: en la barra de transporte, `PRESET ⏷` (menú con los de fábrica, un separador y los del usuario, armado al abrirse para ver los recién guardados) y `SAVE` (diálogo modal de egui con un campo de nombre y Guardar/Cancelar); al elegir uno se aplican sus comandos y el botón muestra el nombre (recortado a 14 caracteres + `⏷`); los errores van a la línea de estado con el prefijo `PRESET:` y al guardar `PRESET guardado: <nombre>`, como en `strip.py`. `NomadApp` arma el `PresetStore` con `<config>/presets`.

- [ ] **Step 1: Write the failing tests** `presets_ui.rs` (con un `PresetStore` en un directorio temporal): `menu_lists_factory_then_user`, `selecting_factory_preset_applies_it` (tras un render el espejo tiene los parámetros del preset y el botón muestra el nombre), `save_dialog_writes_file_and_lists_it`, `save_reserved_name_shows_error`, `drum_preset_from_menu`, `invalid_user_file_is_not_listed`.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test presets_ui`. Expected: FAIL.
- [ ] **Step 3: Implement** el menú, el diálogo y su integración.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): preset menu and save dialog"`.

### Task 4: MIDI learn

**Files:**
- Create: `rust/nomad-app/src/learn.rs`
- Modify: `rust/nomad-app/src/header.rs`, `rust/nomad-app/src/synth_panel.rs`, `rust/nomad-app/src/ui.rs`
- Test: `rust/nomad-app/tests/learn.rs`

**Interfaces:**
- Consumes: `Arc<Mutex<MidiMap>>` (el que comparte `MidiInput`), menú contextual de M3-B.
- Produces (`nomad_app::learn`): `pub struct LearnState { pub last_touched: Option<usize /* pid */>, pub armed_by_us: Option<usize> }` con `toggle_learn(&mut self, map: &Mutex<MidiMap>) -> String` (mensajes de estado de `_toggle_learn`/`_arm`: `"LEARN: touch a control first"`, `"LEARN: <param> — move a CC"`), `arm(&mut self, map, pid: Option<usize>) -> String`, `poll(&mut self, map) -> Option<String>` (cada fotograma: si estaba armado y el mapa ya no aprende, guarda el mapa y devuelve `"LEARN: <param> bound"` o `"LEARN: could not save map (<error>)"`), `unbind(&mut self, map, pid) -> String` (`"MIDI: <param> desvinculado"`). En la interfaz: botón `LEARN` de la cabecera habilitado (resaltado mientras está armado), la perilla armada se dibuja resaltada (etiqueta y valor en `GREEN`, como `highlight`), y el menú contextual de cada control del Synth muestra `MIDI learn` / `Cancelar learn` y, si el parámetro tiene un CC, `Quitar MIDI` (los de la batería sólo tienen `Valor por defecto`, como en Python). Tocar cualquier control actualiza `last_touched`.

- [ ] **Step 1: Write the failing tests** `learn.rs` (mapa en un directorio temporal y un `MidiHandler::feed` de M2-B para simular el CC): `learn_without_touch_reports`, `context_menu_learn_then_cc_binds_and_saves` (el archivo del mapa temporal contiene el binding), `learn_button_uses_last_touched`, `cancel_learn`, `unbind_removes_and_saves`, `armed_knob_is_highlighted`, `drum_knobs_have_no_learn_entry`.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test learn`. Expected: FAIL.
- [ ] **Step 3: Implement** `learn.rs` y su integración.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): MIDI learn from the UI"`.

### Task 5: Modo ayuda

**Files:**
- Create: `tools/ref_help.py`, `rust/nomad-app/src/help.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-app/src/ui.rs` y los módulos de widgets/paneles para registrar la clave de ayuda de cada control
- Test: `tests/test_ref_help.py`, `rust/nomad-app/tests/help.rs`

**Interfaces:**
- Produces (`nomad_app::help`): `pub fn help_for(key: &str) -> Option<(&'static str, &'static str)>` con **todos** los textos de `nomad/gui/help.py` (parámetros por nombre, combinaciones `osc1_*`, `lfo1_*`, `amp_*`/`filt_*`, elementos como `tab_new`, `seq_rec`, `drum_kick`, `preset_menu`, `inst_bpm`, etc.), generados desde la referencia (incrustados con `include_str!` de un JSON o como tabla en Rust; lo que sea más simple de mantener); `pub struct HelpState { pub on: bool, hover: Option<(String, Instant)>, shown: Option<String> }` con `toggle()`, `hover(key, now)`, `visible(now) -> Option<&str>` (aparece tras `HELP_DELAY_MS = 150`). En la interfaz: botón `?` de la cabecera, `F1` y `Esc` (sólo apaga); con el modo activo, los controles **no** reaccionan a clics, arrastres ni rueda; el cursor es de ayuda; la línea de estado dice `AYUDA: pasá el mouse por un control (F1 / Esc para salir)`; al pasar sobre un control aparece un recuadro con título en verde negrita y cuerpo en `#dddddd` con ancho de envoltura 330, al costado del control (o del otro lado si no entra), y queda hasta el siguiente.
- Python: `ref_help.write` escribe `help.json` con `help_for(key)` para el conjunto completo de claves (todas las de `REGISTRY` visibles, las de `DRUM_REGISTRY`, y todas las de `_ELEMENTS`/`_FIXED`).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `help.rs`: `texts_match_python` (título y cuerpo **exactos** para todas las claves), `every_visible_control_has_help` (cada control registrado en la interfaz tiene texto), `help_mode_blocks_controls` (con el modo activo un arrastre sobre una perilla no manda `SetParam`), `tooltip_after_delay`, `f1_and_esc`.
- [ ] **Step 2: Run** pytest + `cd rust && cargo test -p nomad-app --test help`. Expected: FAIL.
- [ ] **Step 3: Implement** `help.rs` y el registro de claves en los widgets.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): help mode with the Python texts"`.

### Task 6: Cambio de dispositivo de audio y de puerto MIDI en caliente

**Files:**
- Modify: `rust/nomad-runtime/src/output.rs`, `rust/nomad-runtime/src/midi/input.rs`, `rust/nomad-app/src/header.rs`, `rust/nomad-app/src/ui.rs`
- Test: `rust/nomad-runtime/tests/output.rs`, `rust/nomad-runtime/tests/midi_input.rs`, `rust/nomad-app/tests/devices_ui.rs`

**Interfaces:**
- Produces: `AudioOutput::close(self) -> AudioCore` (devuelve el núcleo con todo su estado: el callback guarda el `AudioCore` en un `Arc<Mutex<Option<AudioCore>>>` y lo toma con `try_lock`; si no consigue el lock emite silencio ese bloque —`shortcut: try_lock en el callback, sólo hay contención durante un cambio de dispositivo`—; `close` pausa el stream, lo suelta y saca el núcleo) y `AudioOutput::start` recibe el mismo `AudioCore`; si el dispositivo nuevo no abre, el núcleo vuelve en el `Err` para reintentar con el anterior. `MidiInput::close(self) -> MidiSender` (suelta las teclas y devuelve el emisor de un solo uso para abrir otro puerto). En la interfaz, los menús `AUDIO ⏷` y `MIDI ⏷` cambian en caliente: audio cierra, reabre en el elegido (o vuelve al anterior con aviso en la línea de estado) y conserva el motor; MIDI hace `reset`, cierra y abre el nuevo puerto con el mismo mapa.

- [ ] **Step 1: Write the failing tests.** `output.rs`: `core_survives_close` (con un `AudioCore` envuelto sin dispositivo: el handoff devuelve el mismo núcleo con el patrón intacto), y `#[ignore = "requires an audio device"]` `switch_device_keeps_engine_state`. `midi_input.rs`: `#[ignore = "requires ALSA sequencer"]` `close_returns_sender_and_releases_keys`. `devices_ui.rs`: `device_menu_switch_keeps_patterns` (con un dispositivo falso inyectable en la capa de la app: el cambio conserva los instrumentos y patrones), `failed_switch_restores_previous_and_reports`.
- [ ] **Step 2: Run** los tests nuevos. Expected: FAIL.
- [ ] **Step 3: Implement** el handoff del núcleo, el cierre de MIDI y los menús.
- [ ] **Step 4: Run** `cd rust && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` y los pytest. **A mano**, con pantalla: abrir la ventana, cargar un preset en cada instrumento, cambiar de dispositivo de audio y de puerto MIDI y sacar una captura **recortada a la ventana** de la pestaña de la batería y del menú de presets; pasar las rutas.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): hot switching of audio device and MIDI port"`.
