# NOMAD en Rust, M3-A (interfaz: estado compartido, modelo de app, widgets y ventana) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tener la ventana de NOMAD en Rust: tabs de instrumentos, barra de transporte por instrumento, osciloscopio, volumen master y menús de audio/MIDI, sobre el audio y el MIDI reales, con la base (estado compartido, modelo de app y widgets) sobre la que M3-B (panel del Synth y secuenciador) y M3-C (batería, presets, MIDI learn, ayuda) se construyen.

**Architecture:** Tres capas. (1) **Espejos de estado** en `nomad-runtime`: el hilo de audio publica por átomos lo que la interfaz necesita leer (parámetros, patrones, relojes, playhead, notas activas); la interfaz **nunca** lee el motor directamente. (2) **`AppModel`** en `nomad-app`: es el equivalente al `InstrumentManager` + la parte de `App` de Python que no es dibujo; vive en el hilo de la interfaz, es dueño del `ControlHandle` y de la lista de instrumentos, y manda `Command`s. (3) **Dibujo con egui/eframe**: widgets propios (perilla, selector, osciloscopio) cuya lógica pura se prueba contra Python, y `App::ui(&mut self, ctx)` que se puede ejecutar sin ventana en tests.

**Tech Stack:** Rust 1.100, `eframe`/`egui` 0.37, `egui_kittest` 0.37 (dev), lo ya presente. Leer la documentación de las versiones fijadas en `Cargo.lock`: la API de egui cambia entre versiones.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M3). Decisión del autor: **mismo estilo que Python, ventana redimensionable**. Siguen: M3-B (panel del Synth, secuenciador), M3-C (batería, presets, MIDI learn, ayuda).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Python nuevo sólo en `tools/` y `tests/`.
- Estilo visual de `nomad/gui/knob.py`: fondo `#101310`, verde `#55e69a`, gris `#999999`, canal `#29342b`, acento `#b9ffd8`, playhead `#ffffff`. Fuente chica (≈ 8 pt de Tk → 11 px). La ventana arranca en 1366×768 lógicos, es redimensionable (mínimo 1000×600) y se escala con `pixels_per_point`; el diseño no depende de píxeles absolutos.
- La interfaz no bloquea, no toma `Mutex` del audio y no asigna en el hilo de audio. Los mirrors usan sólo átomos (`Ordering::Relaxed`) y no se exige coherencia entre palabras (lectura "rasgada" aceptable, como el osciloscopio de Python).
- El hilo de la interfaz es el **único** dueño de la lista de instrumentos (como Python): ids y nombres (`Synth 02`) salen de contadores propios, las reglas de `set_source` se validan en la interfaz antes de mandar el comando.
- Lógica pura (modelo de perilla, formatos, trigger del osciloscopio, TAP) con tests contra referencias de Python; el dibujo con tests de egui sin GPU (`egui_kittest` o `Context::run`).
- Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (incluidos los `no_alloc` de M2) y los pytest existentes sin regresión. Rust: `export PATH="$HOME/.cargo/bin:$PATH"`. Los tests no pueden exigir pantalla ni audio.

## Review Focus

1. El mirror nunca devuelve basura al leer una instancia recién agregada, recién cerrada o con un id que ya no existe (Task 1).
2. Un REC por pasos hecho en el hilo de audio aparece en el espejo del patrón sin que la interfaz haya mandado nada (Task 1).
3. Cerrar la pestaña seleccionada elige la más cercana ya construida; cerrar la fuente de un seguidor lo pasa a Internal; el último Synth cerrado deja el teclado/MIDI sin objetivo sin romper (Task 2).
4. `KnobModel` con valores `NaN`/inf, drag de 0 píxeles, rueda con `fine` y `format_value` en los bordes (999.9 ms, 1000 ms, 9999 Hz, 10000 Hz, −0 st) coinciden con Python (Task 3).
5. El osciloscopio con buffer vacío, con ruido sin cruces por cero y con `NaN` no entra en pánico y dibuja una línea (Task 3).
6. Cambiar el tamaño de la ventana o `pixels_per_point` no rompe el layout ni los hit-areas de las perillas (Task 4-5).
7. Cerrar la app con audio y MIDI abiertos termina sin colgarse y suelta las teclas sostenidas (Task 5).

---

### Task 1: Espejos de estado (audio → interfaz)

**Files:**
- Create: `rust/nomad-runtime/src/mirror.rs`
- Modify: `rust/nomad-runtime/src/core.rs`, `rust/nomad-runtime/src/lib.rs`, `rust/nomad-core/src/engine.rs` y los módulos que necesiten accesores de **sólo lectura** (`synth.rs`, `sequencer.rs`, `drums.rs`, `voice_manager.rs`)
- Test: `rust/nomad-runtime/tests/mirror.rs`

**Interfaces:**
- Consumes: `AudioCore`, `Command`, `Engine`, `Instance` (M2).
- Produces (`nomad_runtime::mirror`): `pub struct InstrumentShared` (siempre dentro de `Arc`) con campos atómicos y lectores:
  - común: `id: InstrumentId`, `kind: Kind`, `source: AtomicU32 /* 0 = Internal */`, `clock_running: AtomicBool`, `clock_bpm: AtomicU64` (bits de `f64`), `clock_pos: AtomicU64`, `eff_bpm`, `eff_running`, `eff_pos` (reloj efectivo), `level`, `muted`, `step: AtomicI32` (paso actual del reloj efectivo, −1 si parado);
  - Synth: `params: [AtomicU64; 63]` (valores **objetivo**, bits de `f64`), `seq_on/seq_note/seq_vel: [AtomicU8; 16]`, `seq_extra: [[AtomicU8; 5]; 16]`, `seq_gate`, `seq_division`, `seq_swing` (`AtomicU64`), `seq_length: AtomicU8`, `seq_enabled`, `seq_rec` (`AtomicBool`), `seq_rec_step: AtomicU8`, `active_notes: [AtomicU64; 2]` (bitmask de 128 notas sostenidas), `voices: AtomicU8`;
  - Drums: `values: [AtomicU64; 20]`, `steps: [[AtomicU8; 16]; 6]`, `playing: AtomicBool`.
  - Lectores sin asignar: `fn param(&self, id: usize) -> f64`, `fn seq_snapshot(&self, out: &mut SeqSnapshot)`, `fn drum_snapshot(&self, out: &mut DrumSnapshot)`, `fn active_note(&self, n: u8) -> bool`; structs `SeqSnapshot`/`DrumSnapshot` de tamaño fijo (`Default`).
  - `Instance` gana `pub shared: Arc<InstrumentShared>` creado en `build_instance` (el control guarda un clon antes de enviar `AddInstrument`); `AudioCore` actualiza el espejo de **cada** instancia al final de cada callback (y tras cada comando que cambie estado), con sólo `store`s. Para eso `nomad-core` expone accesores de lectura (`Engine::instances_for_each(&self, f: impl FnMut(&Instance))`, y getters de `Synth::values()`, `StepSequencer` campos públicos de lectura, `DrumMachine::steps/values`, `VoiceManager::fill_active_notes`) sin asignar.
- `Telemetry` suma: `peak: AtomicU32` (último pico de salida, bits de `f32`) y `voices_total: AtomicU32`.

- [ ] **Step 1: Write the failing tests** en `mirror.rs`: `params_mirror_follows_set_param` (tras `SetParam` y un render el espejo da el valor acotado), `pattern_mirror_follows_edit_commands` (`SeqSetStep`, `SeqAddNote`, `DrumSetStep`), `rec_written_pattern_reaches_mirror` (con `SeqArm`, un `NoteOn`/`NoteOff` por el `MidiSender`: el espejo muestra el paso grabado y `seq_rec_step` avanzó), `clock_and_follower_mirror` (un Synth que sigue a la batería muestra `eff_bpm` de la fuente y `source` ≠ 0), `playhead_step_advances_when_running`, `active_notes_mirror` (tocar 60 y 64: máscara correcta; soltar: se limpia tras el release), `add_and_remove_instance_lifecycle` (el `Arc` agregado queda legible; tras `RemoveInstrument`/`DropClosed` sigue siendo seguro de leer), `mirror_updates_do_not_allocate` (en el binario con asignador de conteo de M2: 0 asignaciones durante `render_stereo` con espejos), y los `no_alloc` de M2 siguen en verde.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-runtime --test mirror`. Expected: FAIL.
- [ ] **Step 3: Implement** `mirror.rs`, la publicación en `AudioCore` y los accesores de lectura.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): audio-to-UI state mirrors"`.

### Task 2: `AppModel` (el manager del lado de la interfaz)

**Files:**
- Create: `rust/nomad-app/src/model.rs`, `rust/nomad-app/src/tap.rs`
- Modify: `rust/nomad-app/src/lib.rs`
- Test: `rust/nomad-app/tests/model.rs`

**Interfaces:**
- Consumes: `ControlHandle`, `Command`, `InstrumentShared`, `build_instance`, `Engine::next_id` lógica (contadores propios), `MidiInput::reset`.
- Produces (`nomad_app::model`): `pub struct InstrumentView { pub id: InstrumentId, pub kind: Kind, pub name: String, pub shared: Arc<InstrumentShared> }`; `pub struct AppModel`; `AppModel::new(control: ControlHandle, sample_rate: f64, initial: Vec<InstrumentView> /* Synth 01 y Machine Drum 01 que ya trae el Engine */) -> AppModel`; `instruments(&self) -> &[InstrumentView]`; `selected(&self) -> Option<InstrumentId>`; `target_synth(&self) -> Option<InstrumentId>`; `add_instrument(&mut self, kind: Kind) -> Option<InstrumentId>` (construye con `build_instance` fuera del audio y envía `AddInstrument`; `None` si `MAX_INSTANCES`; ids y nombres `"<Etiqueta> NN"` con contador por tipo, igual que `InstrumentManager.add`); `select(&mut self, id)` (si es Synth y no es el objetivo, envía `SetTargetSynth`, llama al gancho `on_retarget` para que el llamador haga `MidiInput::reset`); `close_instrument(&mut self, id)` (reglas de `App.close_instance`: reasigna el objetivo a otro Synth si hace falta, elige la pestaña más cercana ya lista, manda `RemoveInstrument` y, pasados 100 ms de reloj de pared medidos con `Instant` en `tick()`, `DropClosed`); `set_source(&mut self, id, src: Option<InstrumentId>) -> bool` (valida con las reglas de `InstrumentManager.set_source` y sólo entonces manda `SetSource`); `cycle_source(&mut self, id) -> bool` (lógica de `InstanceStrip.cycle_source`); `source_options(&self, id) -> Vec<Option<InstrumentId>>`; `toggle_play(&mut self, id)`, `set_bpm(id, bpm)`, `toggle_mute(id)` (la **intención** de mute vive en el modelo), `set_level(id, f64)`, `set_master(f64)`; `tick(&mut self)` (cada fotograma: `control.reap()` y el cierre diferido); `tap_bpm(&mut self, id, now: Instant)` usando `TapTempo`.
- `nomad_app::tap::TapTempo`: `new()`, `tap(&mut self, now: Instant) -> Option<f64>` (promedio de los últimos 4 intervalos como `InstanceStrip.tap`, se reinicia con una pausa > 2 s, resultado redondeado y acotado a [20, 300]).

- [ ] **Step 1: Write the failing tests** en `model.rs` (con un `AudioCore` avanzado a mano en el test, sin dispositivo): `add_names_and_ids` (`Synth 02`, `Machine Drum 02`, `Synth 03`), `add_respects_max_instances`, `close_selected_picks_nearest_ready_tab`, `close_source_hands_followers_back_to_internal`, `source_rules` (cadena, auto-seguimiento, fuente con seguidores, fuente cerrada: `false` sin comando), `cycle_source_matches_python_order` (Internal → cada otro Internal → Internal), `select_synth_sends_retarget_and_calls_hook`, `closing_last_synth_clears_target_without_panic`, `deferred_drop_after_100ms` (con un reloj inyectable en `tick`), `follower_play_is_noop`. `tap.rs`: `tap_averages_last_four_intervals`, `tap_resets_after_two_seconds`, `tap_clamps_to_20_300` (tabla fija de intervalos contra el cálculo de `InstanceStrip.tap`).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test model`. Expected: FAIL.
- [ ] **Step 3: Implement** `model.rs` y `tap.rs` portando `manager.py`, `app.py` (`close_instance`, `select`, `_retarget`) y `strip.py` (`cycle_source`, `tap`).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): app model (instrument manager for the UI thread)"`.

### Task 3: Lógica de widgets contra Python

**Files:**
- Create: `tools/ref_gui_logic.py`, `rust/nomad-app/src/widgets/mod.rs`, `rust/nomad-app/src/widgets/knob_model.rs`, `rust/nomad-app/src/widgets/scope_math.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-app/src/lib.rs`
- Test: `tests/test_ref_gui_logic.py`, `rust/nomad-app/tests/widgets_logic.rs`

**Interfaces:**
- Consumes: `ParamSpec`, `SYNTH_PARAMS`, `DRUM_PARAMS`.
- Produces: `KnobModel::new(spec: &'static ParamSpec, value: f64)` con `norm()`, `value()`, `set_norm(f64) -> Result<(), ()>` (no finito → `Err`), `set_value(f64) -> Result<(), ()>`, `set_from_drag(&mut self, pixels: f64, fine: bool)`, `set_from_wheel(&mut self, delta: i32, fine: bool)`, `reset()`, `format_value() -> String`, constantes `DRAG_PX = 200.0`, `FINE = 10.0`, `WHEEL_STEP = 0.02`; mismas fórmulas y formatos que `nomad/gui/knob.py` (`ms`, `Hz`, `st`/`ct`, `voices`, otras unidades con `{:g}`, y porcentaje con signo si `lo < 0`; `format_value` de Python usa `f"{v:.1f}"`, `{:g}` etc.: se replica el redondeo de Python, **no** el de `format!` por defecto de Rust; ojo con `round()` bancario de Python para `:.0f` vs `round-half-to-even`). `scope_math::find_trigger(buf: &[f32]) -> usize` (último cruce ascendente por cero; `NaN` cuenta como 0) y `scope_math::to_points(seg: &[f32], width: usize, height: usize, out: &mut Vec<(f32, f32)>)` (decimación mín/máx idéntica a `to_points` de `scope_view.py`).
- Python: `ref_gui_logic.write` escribe `gui_logic.json` con: (a) `format_value` de `KnobModel` para cada parámetro del Synth y de la batería a valores `lo`, `default`, `hi`, `lo + (hi-lo)·0.37` y a los bordes de formato (`99.95`, `100`, `999.9`, `1000` ms; `9999`, `10000` Hz; `-0.4`, `0.4` st; `0.005`); (b) secuencias de `set_from_drag`/`set_from_wheel` (con y sin `fine`) y el `norm`/`value` resultantes; (c) `find_trigger` y `to_points` para 12 señales fijas (senos a distintas frecuencias, ruido determinista, vacío, constante, con `NaN`).

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `widgets_logic.rs`: `format_matches_python` (cadenas **exactas** para todos los casos), `drag_wheel_reset_match_python` (1e-12), `non_finite_is_rejected`, `trigger_matches_python` (enteros exactos), `points_match_python` (1e-6 por ser `f32`, mismo largo), `scope_handles_empty_and_nan`.
- [ ] **Step 2: Run** pytest + `cd rust && cargo test -p nomad-app --test widgets_logic`. Expected: FAIL.
- [ ] **Step 3: Implement** `knob_model.rs` y `scope_math.rs`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): knob model and scope math, verified against Python"`.

### Task 4: Tema y widgets de egui

**Files:**
- Create: `rust/nomad-app/src/theme.rs`, `rust/nomad-app/src/widgets/knob.rs`, `rust/nomad-app/src/widgets/selector.rs`, `rust/nomad-app/src/widgets/scope.rs`, `rust/nomad-app/examples/gallery.rs`
- Modify: `rust/nomad-app/Cargo.toml` (`eframe`, `egui`; `egui_kittest` como `dev-dependency`), `rust/nomad-app/src/widgets/mod.rs`
- Test: `rust/nomad-app/tests/widgets_ui.rs`

**Interfaces:**
- Produces: `theme::apply(ctx: &egui::Context)` (colores del estilo de Python, `Visuals::dark()` ajustado, bordes planos, fuentes chicas, sin animaciones de hover exageradas); constantes `BG, GREEN, GREY, TROUGH, ACCENT, PLAYHEAD: Color32`.
  - `widgets::knob::knob(ui: &mut egui::Ui, model: &mut KnobModel, label: &str, locked: bool) -> KnobResponse` con `KnobResponse { pub changed: bool, pub reset: bool, pub context: bool /* clic derecho o medio */, pub hovered: bool, pub response: egui::Response }`: arco de 270° (de abajo-izquierda en sentido horario), canal `TROUGH` de 5 px, relleno `GREEN` hasta `norm`, aguja, etiqueta arriba y valor formateado abajo, tamaño 64×84 puntos lógicos; arrastre vertical (`DRAG_PX`), `shift` = fino, rueda (`WHEEL_STEP`), doble clic = valor por defecto (siempre notifica), clic derecho/medio = `context`, `locked` muestra pero no reacciona.
  - `widgets::selector::selector(ui, labels: &[&str], index: usize) -> Option<usize>` (fila de botones planos, activo en `GREEN` sobre `BG`; devuelve el índice sólo al hacer clic) y `selector_with_context(..) -> (Option<usize>, bool)`.
  - `widgets::scope::scope(ui, buffer: &ScopeRing, time_ms: &mut f64, sample_rate: f64)` con botones 5/10/20/50 ms y la línea verde con trigger (`find_trigger` sobre las primeras `n` de `2n` muestras, `to_points`), cuadrícula de 3 divisiones.
  - `examples/gallery.rs`: una ventana con todos los widgets (para mirarlos a mano).
- Los widgets no hablan con el motor: devuelven cambios y el llamador los envía.

- [ ] **Step 1: Write the failing tests** `widgets_ui.rs` con `egui_kittest` (o `egui::Context::run` con `RawInput` si kittest no alcanza): `knob_drag_up_increases_value` (simula arrastre de 40 px hacia arriba: `norm` sube `40/200`), `knob_shift_is_fine`, `knob_wheel_steps`, `knob_double_click_resets_and_reports`, `knob_context_click_reports`, `locked_knob_ignores_input`, `selector_click_returns_index_only_on_click`, `scope_buttons_change_time`, `theme_sets_background` (el color de fondo del panel es `#101310`), `widgets_survive_scale_changes` (`pixels_per_point` 1.0, 1.5 y 2.0: sin pánico y con rectángulos no vacíos).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test widgets_ui`. Expected: FAIL.
- [ ] **Step 3: Implement** el tema y los widgets. Verificar que las fuentes por defecto de egui dibujan los símbolos que el diseño usa (`▾ ▸ · ⇄ ● ×`); si alguno no se ve, reemplazarlo por un equivalente que sí.
- [ ] **Step 4: Run** todo (+ fmt/clippy) y `cargo build --example gallery -p nomad-app` (que compile; abrirla es manual).
- [ ] **Step 5: Commit** `git commit -m "feat(rust): egui theme, knob, selector and scope widgets"`.

### Task 5: Ventana, pestañas, barra de transporte y menús

**Files:**
- Create: `rust/nomad-app/src/ui.rs`, `rust/nomad-app/src/header.rs`, `rust/nomad-app/src/transport.rs`
- Modify: `rust/nomad-app/src/main.rs` (subcomando `nomad` sin argumentos o `nomad gui`), `rust/nomad-app/src/lib.rs`
- Test: `rust/nomad-app/tests/ui_smoke.rs`

**Interfaces:**
- Consumes: `AppModel`, `InstrumentShared`, widgets, `AudioOutput`, `MidiInput`, `Telemetry`.
- Produces: `pub struct NomadApp` (implementa `eframe::App`) con `NomadApp::new(model: AppModel, output: Option<AudioOutput>, midi: Option<MidiInput>, midi_map: Arc<Mutex<MidiMap>>, telemetry: Arc<Telemetry>) -> NomadApp` y `fn ui(&mut self, ctx: &egui::Context)` (todo el dibujo, ejecutable sin ventana). Layout (lógico, escalable): **cabecera** (título "NOMAD SYNTH", `TabBar`: una pestaña por instrumento con `×` y `⇄ Fuente`/`●` como en `refresh_tabs`, botón `+` con menú Synth/Machine Drum, menú de dispositivo de audio y de puerto MIDI —listas y selección con `list_output_devices`/`list_input_ports`—, `VOICES n`, `XRUNS n`, `LEARN` y `?` deshabilitados hasta M3-C, línea de estado); **barra de transporte** del instrumento seleccionado (PLAY/STOP grande, BPM editable con arrastre y teclado más `TAP`, `CLOCK: …` que cicla fuentes, `PRESET ▾` y `SAVE` deshabilitados hasta M3-C, `MUTE` y nivel); **columna derecha** con el osciloscopio y el volumen master; **centro**: marcador de posición "Panel del Synth (M3-B)" / "Machine Drum (M3-C)". Todo lee de los espejos y manda `Command`s por el `AppModel`; un seguidor muestra el BPM de su fuente con los controles bloqueados. `main`: arma `Engine` por defecto, `create`, abre audio y MIDI como `demo --midi` (si fallan, la ventana abre igual y lo informa en la línea de estado), inicia `eframe` y al cerrar detiene audio y MIDI y hace `MidiInput::reset`.

- [ ] **Step 1: Write the failing tests** `ui_smoke.rs` (sin ventana, `Context::run`/`egui_kittest`, con un `AudioCore` avanzado a mano): `first_frame_draws_header_transport_and_scope` (aparecen "NOMAD SYNTH", "PLAY", "BPM", "MUTE", "Synth 01", "Machine Drum 01"), `add_tab_via_plus_menu`, `close_tab_updates_selection`, `play_button_starts_selected_instrument` (el clic envía `Start` y, tras un render, el espejo muestra `clock_running`), `bpm_edit_sets_clock`, `follower_controls_are_locked` (tras `SetSource`), `device_menu_lists_ports_without_panic`, `layout_survives_resize_and_scale` (anchos 1000, 1366, 1920 y `pixels_per_point` 1.0/2.0), `closing_stops_audio_and_releases_keys` (llamar `on_exit` sin colgarse).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test ui_smoke`. Expected: FAIL.
- [ ] **Step 3: Implement** la ventana, la cabecera y el transporte; `main` con `eframe::run_native`.
- [ ] **Step 4: Run** `cd rust && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` y los pytest. Luego **a mano**: `cargo run --release -p nomad-app` (si hay pantalla: abrir 5 s con `timeout`, sacar una captura con `import -window root` o similar y anotar la ruta; si no hay pantalla, decirlo).
- [ ] **Step 5: Commit** `git commit -m "feat(rust): NOMAD window with tabs, transport bar, scope and device menus"`.
