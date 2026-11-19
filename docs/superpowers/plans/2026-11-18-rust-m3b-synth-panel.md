# NOMAD en Rust, M3-B (panel del Synth, secuenciador y teclado) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Que la pestaña de un Synth muestre y controle todo lo que hoy muestra la de Python: secciones de parámetros, matriz de modulación plegable, secuenciador con acordes y REC, columna VOICE, y teclado en pantalla; con un layout que se adapta al ancho de la ventana (incluido un gestor de ventanas en mosaico que la deja angosta).

**Architecture:** Sobre M3-A. La interfaz lee los **espejos** (`InstrumentShared`) y manda `Command`s por `AppModel`; no toca el motor. Los valores de los parámetros tienen un sincronizador (`ParamSync`) que reconcilia "lo que el usuario acaba de mover" con "lo que el audio publicó" (para no perder arrastres entre fotogramas ni pisar cambios que vienen del MIDI). Los datos de layout (secciones, etiquetas, selectores) son datos puros verificados contra Python.

**Tech Stack:** igual que M3-A (egui/eframe/egui_kittest 0.37).

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M3) y `docs/superpowers/plans/2026-11-18-rust-m3a-gui-shell.md`. Referencia de comportamiento y aspecto: `nomad/gui/synth_panel.py`, `seq_view.py`, `keyboard.py`, `app.py` (`_menu_items`, `_reset`). Sigue: M3-C (batería, presets, MIDI learn, ayuda).

**Branch:** `feature/rust-m0`, un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Python nuevo sólo en `tools/` y `tests/`.
- Misma paleta y fuentes que M3-A. El diseño es **adaptable**: ancho lógico mínimo útil 640 (los gestores de ventanas en mosaico ignoran el mínimo de la ventana); a anchos menores aparece desplazamiento vertical y las secciones se reacomodan en filas (`horizontal_wrapped`/rejilla que cambia de columnas), sin cortar controles esenciales ni texto de estado.
- La interfaz sólo escribe por `Command`; sólo lee espejos. Ninguna llamada bloquea.
- Textos visibles en español, como en Python ("MODULACIÓN", "en uso", etc.); los nombres de parámetros son los del registro.
- Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (incluidos `no_alloc` y `mirror` de M2/M3-A) y los pytest existentes sin regresión. Rust: `export PATH="$HOME/.cargo/bin:$PATH"`. Los tests no pueden exigir pantalla ni audio.

## Review Focus

1. Arrastrar una perilla durante varios fotogramas seguidos no pierde incrementos aunque el espejo todavía no haya publicado el valor, y un cambio que viene del MIDI aparece en la perilla cuando el usuario no la está tocando (Task 1).
2. `polyphony` se muestra con `+1` de desfase en el selector (índice 0 = 1 voz); `osc1_sync` está oculto; los selectores de ondas del LFO tienen 5 opciones y los de oscilador 4 (Task 1).
3. Paso apagado con acorde guardado: muestra `·`; con acorde `C3` + 2 notas más: `C3` y debajo `+2`; los pasos más allá de LEN se ven apagados aunque estén encendidos (Task 3).
4. `shift+clic` en un paso apagado lo enciende **con acento**; `shift+clic` sobre un acento lo apaga; rueda con `shift` transpone una octava (Task 3).
5. REC: el botón muestra `REC·STEP` con el reloj parado y `REC·LIVE` con el reloj andando; `REST` sólo está habilitado en REC con el reloj parado; el cursor de REC se ve como playhead cuando no hay reloj (Task 3).
6. Teclado: arrastrar de una tecla a otra suelta la anterior y toca la nueva; al perder el foco o salir del área se suelta; las teclas negras ganan donde se superponen (Task 4).
7. A 640, 1000 y 1366 puntos de ancho el panel del Synth no recorta controles y no hay solapamientos (Task 5).

---

### Task 1: Datos de layout y sincronizador de parámetros

**Files:**
- Create: `tools/ref_synth_layout.py`, `rust/nomad-app/src/synth_layout.rs`, `rust/nomad-app/src/param_sync.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-app/src/lib.rs`
- Test: `tests/test_ref_synth_layout.py`, `rust/nomad-app/tests/synth_layout.rs`, `rust/nomad-app/tests/param_sync.rs`

**Interfaces:**
- Produces (`nomad_app::synth_layout`): `pub struct Section { pub key: &'static str, pub title: &'static str, pub params: &'static [usize] /* pid */ }`; `pub static SECTIONS: &[Section]` (osc1, osc2, mixer, filter, amp_env, filt_env, lfo1, lfo2, matrix, global —sin `master_volume`—, en el orden de `LAYOUT` de `nomad/gui/synth_panel.py`, con `osc1_sync` excluido por `HIDDEN`); `pub enum WidgetKind { Knob, Selector }`; `pub fn widget_kind(pid: usize) -> WidgetKind` (`polyphony` y los que terminan en `_wave`, `_mode`, `_sync`, `_src`, `_dst`); `pub fn selector_labels(pid: usize) -> &'static [&'static str]` (`polyphony`: `"1".."8"`; `*_wave`: `SIN TRI SAW SQR` y `S&H` para LFO; `_mode`: `KEY FREE`; `_sync`: `OFF SYNC`; `_src`: `- LFO1 LFO2 MW VEL ENV2`; `_dst`: `- PIT P1 P2 AMP CUT PW1 PW2`); `pub fn selector_offset(pid: usize) -> i64` (1 para `polyphony`, 0 el resto); `pub fn short_label(pid: usize) -> &'static str` (regla de `_short`: `master_volume` → `volume`, `bend_range` → `bend`, `_level` → `osc1`/`osc2`, `modN_amt` → `amt N+1`, y el resto el nombre sin prefijo de sección).
- Produces (`nomad_app::param_sync`): `pub struct ParamSync { /* valor local, instante del último envío, último valor enviado */ }`; `ParamSync::new(initial: f64)`; `value(&self) -> f64`; `user_set(&mut self, v: f64, now: Instant)` (fija el valor local y marca "enviado"); `observe(&mut self, mirror: f64, now: Instant)` (adopta el valor del espejo si pasaron más de 150 ms desde el último `user_set` **y** el espejo difiere del local en más de `1e-9` relativo; mientras el usuario está dentro de esa ventana, el valor local manda); `needs_send(&self) -> bool`/`mark_sent(&mut self)`. Un `SynthParams` (arreglo de 63 `ParamSync`) con `fn ui_value(&mut self, pid, mirror, now) -> f64`.
- Python: `ref_synth_layout.write` escribe `synth_layout.json`: `sections` (clave, título, lista de nombres de parámetro en orden), `kinds` (`knob`/`selector` por nombre, para los 63), `labels` (lista de etiquetas por cada parámetro selector), `offsets` y `short` (etiqueta corta por parámetro visible), usando `LAYOUT`, `HIDDEN`, `widget_kind`, `_labels`, `_short` de `synth_panel.py`.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `synth_layout.rs`: `sections_match_python` (mismas secciones y el mismo orden de parámetros, convertidos a nombres), `kinds_labels_offsets_short_match_python` (todos los visibles; **exactos**), `hidden_and_master_are_absent` (`osc1_sync`, `master_volume`). `param_sync.rs`: `drag_across_frames_accumulates` (cinco `user_set` consecutivos con espejo viejo: el valor local final es el último, ningún `observe` lo pisa), `mirror_wins_after_window` (a +200 ms, un `observe` con valor distinto lo adopta), `mirror_ignored_inside_window` (a +50 ms no), `equal_mirror_is_noop`, `non_finite_mirror_is_ignored`.
- [ ] **Step 2: Run** pytest + `cd rust && cargo test -p nomad-app --test synth_layout --test param_sync`. Expected: FAIL.
- [ ] **Step 3: Implement** los dos módulos.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): synth layout data and parameter sync"`.

### Task 2: Secciones de parámetros, matriz plegable y VOICE

**Files:**
- Create: `rust/nomad-app/src/synth_panel.rs`
- Modify: `rust/nomad-app/src/model.rs` (`pub fn send(&mut self, cmd: Command) -> bool`), `rust/nomad-app/src/lib.rs`
- Test: `rust/nomad-app/tests/synth_panel.rs`

**Interfaces:**
- Consumes: `SECTIONS`, `widget_kind`, `selector_labels`, `ParamSync`, `knob`, `selector`, `AppModel`, `InstrumentShared`, `KnobModel`.
- Produces (`nomad_app::synth_panel`): `pub struct SynthPanelState { pub matrix_open: bool, params: SynthParams, /* uno por instancia de Synth */ }`; `pub fn synth_sections_ui(ui: &mut egui::Ui, state: &mut SynthPanelState, inst: &InstrumentView, model: &mut AppModel, now: Instant)`: dibuja las secciones `osc1 osc2 filter mixer / amp_env filt_env lfo1 lfo2` (cada una con su título en verde y sus controles: selectores arriba, perillas debajo), el encabezado de la matriz `MODULACIÓN ▸/▾  (N en uso)` (N = slots con `src`, `dst` y `amt` distintos de cero, leídos del espejo) que despliega los 8 slots (cada uno: selector `src` y `dst` y perilla `amt N`), y devuelve cambios como `Command::SetParam { id, param, value }` por `model.send`. `pub fn voice_ui(ui, state, inst, model, now)` dibuja la sección VOICE (`polyphony`, `bend`, `glide`) para la columna derecha. Menú contextual de cada control (clic derecho/medio): `Valor por defecto` (manda el default del registro) y `MIDI learn` **deshabilitado** (lo activa M3-C). Cada instancia de Synth tiene su propio `SynthPanelState` (guardado por `InstrumentId`).

- [ ] **Step 1: Write the failing tests** (egui_kittest/`Context::run` con un `AudioCore` avanzado a mano y espejos reales): `all_visible_params_have_a_control` (el árbol de accesibilidad contiene un control por cada parámetro de `SECTIONS`, con la etiqueta corta), `knob_drag_sets_param_through_command` (arrastrar la perilla `filt_cutoff` 40 px cambia el espejo tras un render al valor que calcula `KnobModel`), `selector_click_sets_param_with_offset` (`polyphony` clic en "4" → `SetParam(polyphony, 4.0)`), `matrix_collapsed_by_default_and_counts_active` (con `mod0_src=1, mod0_dst=5, mod0_amt=0.3` el encabezado dice `1 en uso`), `matrix_toggle_shows_slots`, `context_default_resets_param`, `each_synth_has_its_own_panel_state` (dos Synths: abrir la matriz en uno no la abre en el otro), `mirror_change_from_outside_updates_knob` (un `SetParam` mandado por otro camino se ve en la perilla tras 200 ms simulados).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test synth_panel`. Expected: FAIL.
- [ ] **Step 3: Implement** `synth_panel.rs` portando `SynthPanel` (`section`, `_build_matrix`, `_update_mod_label`, `toggle_matrix`, `make`).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): synth parameter sections, collapsible matrix and VOICE"`.

### Task 3: Secuenciador (vista y edición)

**Files:**
- Create: `tools/ref_seq_view.py`, `rust/nomad-app/src/seq_view.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-app/src/lib.rs`
- Test: `tests/test_ref_seq_view.py`, `rust/nomad-app/tests/seq_view.rs`

**Interfaces:**
- Consumes: `InstrumentShared::seq_snapshot`, `SeqSnapshot`, `Command::Seq*`, `AppModel`.
- Produces (`nomad_app::seq_view`): lógica pura — `pub fn note_name(n: i32) -> String` (`C#4`; `_NAMES[n % 12] + (n/12 - 1)` con el módulo/división de Python para negativos), `pub fn step_label(snap: &SeqSnapshot, step: usize) -> (String, String)` (línea de arriba: nota raíz si el paso está encendido, `·` si no; línea de abajo: `+N` si encendido con `N` extras, vacío si no), `pub fn step_style(snap, step, playhead: i32) -> StepStyle { fill: Fill /* Off | On | Accent */, dimmed: bool /* paso ≥ LEN */, playhead: bool }`, `pub const DIVS: [(f64, &str); 5] = [(0.25,"1/16"),(0.5,"1/8"),(1.0,"1/4"),(2.0,"1/2"),(4.0,"1 bar")]`, `pub fn next_division(cur: f64) -> f64` (el más cercano de `DIVS` y el siguiente), `pub fn next_length(cur: u8) -> u8` (`cur % 16 + 1`), `pub fn click_command(snap, step, accent: bool) -> Command` (reglas de `toggle_step`: sin acento, alterna encendido y al encender `vel = 100`; con acento, `set_step(step, !(on && vel == 127), vel = 127)`), `pub fn wheel_command(step, notches: i32, octave: bool) -> Command` (`SeqTranspose { delta: ±1 o ±12 }`), `pub fn rec_label(rec: bool, playing: bool) -> &'static str` (`REC`, `REC·STEP`, `REC·LIVE`) y `pub fn rest_enabled(rec: bool, playing: bool) -> bool`. UI — `pub fn seq_view_ui(ui, inst: &InstrumentView, model: &mut AppModel, rec_intent: &mut bool)`: fila de controles agrupados (`SEQ REC REST` | `DIV LEN` | `GATE` deslizador 5-100 % | `CLEAR`), 16 botones de paso de dos líneas (separación extra cada 4), borde grueso `PLAYHEAD` en el paso actual (si el reloj corre: `shared.step`; si no y REC está armado: `seq_rec_step`), pasos ≥ LEN en `TROUGH`; interacciones: clic, `shift+clic`, rueda (con `shift` octava).
- Python: `ref_seq_view.write` escribe `seq_view.json` con: `note_name` para n en 0..127 y algunos negativos/fuera de rango; `step_label`/`step_style` para patrones de prueba (apagado, encendido, con 1 a 5 extras, acento, más allá de LEN) calculados con las reglas de `SeqView.refresh`; `next_division` y `next_length` para toda la tabla; `rec_label`/`rest_enabled` para las 4 combinaciones; y `click_command` como la lista de `(on, vel)` resultante de `toggle_step` en cada estado inicial.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `seq_view.rs`: `pure_logic_matches_python` (todo exacto), `click_shift_click_rules`, `wheel_transposes_whole_chord_via_command`, `rec_flow_updates_labels` (REC armado → `REC·STEP`; con el reloj andando → `REC·LIVE`; `REST` habilitado sólo en el primero), `ui_click_sends_seq_set_step` (egui_kittest: clic en el botón 3 → tras un render el espejo tiene el paso 3 encendido), `ui_shift_click_accent`, `playhead_follows_mirror_step`, `rec_written_step_shows_chord_label` (grabar un acorde de 3 teclas por el `MidiSender` y ver `C3` / `+2`), `gate_slider_sends_gate`, `div_len_buttons_cycle`.
- [ ] **Step 2: Run** pytest + `cd rust && cargo test -p nomad-app --test seq_view`. Expected: FAIL.
- [ ] **Step 3: Implement** `seq_view.rs` portando `SeqView`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): step sequencer view with chords and REC"`.

### Task 4: Teclado en pantalla

**Files:**
- Create: `tools/ref_keyboard.py`, `rust/nomad-app/src/keyboard.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-app/src/lib.rs`
- Test: `tests/test_ref_keyboard.py`, `rust/nomad-app/tests/keyboard.rs`

**Interfaces:**
- Produces (`nomad_app::keyboard`): constantes `WHITE_PC`, `BLACK_AFTER`, `BLACK_W = 0.6`, `BLACK_H = 0.6`, `VELOCITY = 0.8`, `BASE_NOTE = 36`; `pub fn note_at(x: f64, y: Option<f64>, width: f64, height: f64, octaves: usize, base: u8) -> Option<u8>` (idéntica a `note_at` de `keyboard.py`: las negras ganan donde se superponen, `y = None` es "abajo" = blanca); `pub struct KeyboardState { held: Option<u8> }` con `press_or_move(&mut self, note: Option<u8>) -> (Option<u8> /* note_off */, Option<u8> /* note_on */)` y `release(&mut self) -> Option<u8>`; `pub fn keyboard_ui(ui, state: &mut KeyboardState, lit: &dyn Fn(u8) -> bool, model: &mut AppModel)` (6 octavas desde `BASE_NOTE`, alto 70 puntos, ancho disponible; blancas `#d8d8d8`, negras `#1c1c1c`, iluminadas en `GREEN`; manda `Command::NoteOn { note, velocity: 0.8 }` / `NoteOff`; suelta al salir del área, al soltar el botón y cuando la ventana pierde el foco).
- Python: `ref_keyboard.write` escribe `keyboard.json` con `note_at` evaluado en una rejilla de puntos (x en 0..width cada 7 px, y en `[None, 0, 20, 41, 42, 60, 83]`) para `octaves ∈ {1, 2, 6}`, `base ∈ {36, 48}` y `width ∈ {350, 1350}`, `height 84`, más `None` fuera de los bordes.

- [ ] **Step 1: Write the failing tests.** Python: `test_reference_is_up_to_date`. Rust `keyboard.rs`: `note_at_matches_python` (exacto en toda la rejilla), `drag_moves_note_off_then_on`, `release_and_focus_loss_send_note_off`, `black_keys_win_overlap`, `ui_press_sends_note_on_and_release_note_off` (egui_kittest; el Synth objetivo suena tras un render y se apaga al soltar), `lit_keys_come_from_mirror_active_notes`.
- [ ] **Step 2: Run** pytest + `cd rust && cargo test -p nomad-app --test keyboard`. Expected: FAIL.
- [ ] **Step 3: Implement** `keyboard.rs` portando `Keyboard`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): on-screen keyboard"`.

### Task 5: Integración y layout adaptable

**Files:**
- Modify: `rust/nomad-app/src/ui.rs`, `rust/nomad-app/src/header.rs`, `rust/nomad-app/src/transport.rs`
- Test: `rust/nomad-app/tests/ui_synth.rs`

**Interfaces:**
- Consumes: todo lo anterior y `NomadApp` (M3-A).
- Produces: en `NomadApp::ui`, cuando la pestaña seleccionada es un Synth: el centro muestra, de arriba abajo, el secuenciador, las secciones y la matriz; la columna derecha (osciloscopio, MASTER, VOICE); y el teclado abajo (sólo para Synth, oculto para la batería). Reglas adaptables: a un ancho disponible `< 1000` la columna derecha pasa **debajo** del centro; las secciones usan una rejilla de 4 columnas a ≥ 1000, de 2 a ≥ 640 y de 1 por debajo; todo el centro va en un `ScrollArea` vertical; la cabecera usa `horizontal_wrapped` para que `VOICES`/`XRUNS` y la línea de estado no se corten; el teclado se escala al ancho. El estado de cada panel (`SynthPanelState`, `KeyboardState`, intención de REC) vive en `NomadApp` por `InstrumentId` y se limpia al cerrar la pestaña.

- [ ] **Step 1: Write the failing tests** `ui_synth.rs`: `synth_tab_shows_sequencer_sections_keyboard_and_voice`, `drum_tab_hides_keyboard_and_synth_panel` (placeholder de M3-C), `layout_at_640_1000_1366` (en cada ancho no hay panics, los controles esenciales —PLAY, BPM, perilla `filt_cutoff`, los 16 pasos, el teclado— existen en el árbol de accesibilidad y los rectángulos de pasos no se solapan), `header_wraps_without_clipping_status` (ancho 660: `XRUNS` y la línea de estado están dentro del rectángulo de la ventana), `closing_a_tab_drops_its_panel_state`, `switching_tabs_keeps_each_synth_state`.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-app --test ui_synth`. Expected: FAIL.
- [ ] **Step 3: Implement** la integración y el layout adaptable.
- [ ] **Step 4: Run** `cd rust && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` y los pytest. **A mano:** con pantalla, abrir la ventana 5 s con `timeout`, cargar el escenario de `demo` (acorde) y sacar capturas **recortadas a la ventana** a 660, 1000 y 1366 puntos de ancho, guardarlas en el scratchpad y pasar las rutas (no capturar pantalla completa: puede haber contenido privado).
- [ ] **Step 5: Commit** `git commit -m "feat(rust): synth tab integration with adaptive layout"`.
