# Drum Machine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Agregar a NOMAD una drum machine de 16 pasos con seis instrumentos sintetizados, mezclada con las voces del sintetizador y accesible como segunda vista del panel.

**Architecture:** Un módulo nuevo `nomad/engine/drums.py` contiene las recetas de síntesis (one-shots pre-renderizados), el banco de sonidos, el reloj del secuenciador y la mezcla. `Engine` lo instancia y suma su buffer antes de la ganancia master, así que `master_volume`, el `tanh` y el scope siguen siendo únicos. La GUI suma una vista `DrumView` que se intercambia con la del sintetizador dentro de la columna izquierda; la columna derecha (scope + global) no se mueve.

**Tech Stack:** Python 3.14, numpy, tkinter, pytest. Sin dependencias nuevas.

**Spec:** `docs/superpowers/specs/2026-11-13-drum-machine-design.md`

## Global Constraints

- El motor (`nomad/engine`) no importa GUI ni MIDI.
- El callback de audio no reserva memoria ni levanta excepciones: `DrumMachine.render` usa buffers preasignados y sólo `np.multiply`/`np.add` con `out=`.
- Sólo el hilo de Tk toca widgets. `DrumMachine` se escribe desde la GUI con stores atómicos (floats, `uint8` de la matriz de pasos y referencias de arrays) y el hilo de audio los lee; no hay cola porque el orden no importa y el peor caso es un bloque de latencia (corrección a §8.1 del spec, que hablaba de cola).
- Con la batería parada y sin one-shots activos, la salida del motor es idéntica a la de `main`.
- Sin dependencias nuevas; 1366×768.
- Commits firmados: siempre `git commit -S -m "..."`. Nunca sin `-S`.
- Rama de trabajo: `feat/drum-machine` desde `main` en `/home/imn0p/synth-lab`.
- Código en inglés; textos de ayuda de la GUI en español, como los existentes.
- Correr tests con `./.venv/bin/python -m pytest`.

## Review Focus

- Cambiar `bpm` o `swing` mientras corre no debe repetir ni saltear el paso en curso (Task 2).
- PLAY dos veces o PLAY con el transporte ya andando no debe disparar el paso 0 dos veces (Task 2).
- Mover `tune`/`decay` mientras un golpe suena no debe clickear ni leer fuera del array nuevo (Tasks 1 y 2).
- `panic()` con un hat abierto sonando debe callar todo en 5 ms (Task 2 y 3).
- Un paso que cae justo en el límite de un bloque debe dispararse una vez, con cualquier tamaño de callback (Task 2).

---

### Task 1: Recetas de síntesis y registro de parámetros

**Files:**
- Create: `nomad/engine/drums.py`
- Test: `tests/test_drums.py`

**Interfaces:**
- Consumes: `ParamSpec` de `nomad/engine/params.py`.
- Produces:
  - `INSTRUMENTS: tuple[str, ...] = ("kick", "snare", "clap", "tom", "chat", "ohat")`
  - `STEPS: int = 16`
  - `DRUM_REGISTRY: dict[str, ParamSpec]` con 20 entradas: `{track}_{tune,decay,level}` + `bpm` + `swing`.
  - `render_one_shot(kind: str, sr: float = 48000.0, tune: float = 0.0, decay: float = 0.5) -> np.ndarray` (float32, mono, normalizado a 0.9, determinístico).

- [ ] **Step 1: Escribir los tests que fallan**

```python
# tests/test_drums.py
import numpy as np
import pytest

from nomad.engine.drums import DRUM_REGISTRY, INSTRUMENTS, STEPS, render_one_shot

SR = 48000.0


@pytest.mark.parametrize("kind", INSTRUMENTS)
def test_one_shot_is_a_decaying_sound(kind):
    y = render_one_shot(kind, SR)
    assert y.dtype == np.float32 and y.ndim == 1
    assert 0.03 * SR < len(y) <= 1.5 * SR
    assert np.abs(y).max() > 0.1
    assert np.abs(y[-200:]).max() < 0.02 * np.abs(y).max()


def test_one_shot_is_deterministic():
    assert np.array_equal(render_one_shot("snare", SR), render_one_shot("snare", SR))


def test_tune_raises_kick_pitch_and_decay_lengthens_it():
    base = render_one_shot("kick", SR, tune=0.0, decay=0.5)
    higher = render_one_shot("kick", SR, tune=0.8, decay=0.5)
    longer = render_one_shot("kick", SR, tune=0.0, decay=0.9)
    peak = lambda y: int(np.argmax(np.abs(np.fft.rfft(y))))
    assert peak(higher) > peak(base)
    loud = lambda y: np.count_nonzero(np.abs(y) > 0.01 * np.abs(y).max())
    assert loud(longer) > loud(base)


def test_bad_arguments_are_clamped_or_rejected():
    render_one_shot("kick", SR, tune=99.0, decay=float("nan"))
    with pytest.raises(ValueError):
        render_one_shot("cowbell", SR)


def test_registry_has_the_twenty_drum_params():
    expected = {f"{t}_{s}" for t in INSTRUMENTS for s in ("tune", "decay", "level")}
    assert set(DRUM_REGISTRY) == expected | {"bpm", "swing"}
    assert DRUM_REGISTRY["bpm"].lo == 60 and DRUM_REGISTRY["bpm"].hi == 180
    assert DRUM_REGISTRY["swing"].hi == pytest.approx(0.6)
    assert DRUM_REGISTRY["kick_level"].default == pytest.approx(0.6)
    assert DRUM_REGISTRY["ohat_level"].default == pytest.approx(0.3)
    assert STEPS == 16
```

- [ ] **Step 2: Correr los tests y ver que fallan**

Run: `./.venv/bin/python -m pytest tests/test_drums.py -q`
Expected: FAIL con `ModuleNotFoundError: No module named 'nomad.engine.drums'`.

- [ ] **Step 3: Implementar `nomad/engine/drums.py` (sólo síntesis y registro)**

Constantes y helpers requeridos:

- `_SPECS`: tabla por instrumento con frecuencia base, factor de caída de pitch, `tau_base`, `tau_span` y semilla de ruido fija (`kick:1, snare:2, clap:3, tom:4, chat:5, ohat:6`).
- `_wrap(value, spec)`: clamp + `nan` → default, sin depender de `params.clamp` (que sólo conoce `REGISTRY`).
- `_env(t, tau) = np.exp(-t / tau)`; `_hp_noise(rng, n, cutoff_hz, sr)` con un high-pass de un polo (resta de la media móvil o equivalente).
- Largo del one-shot: `min(int(1.5 * sr), int(8 * tau_max) + int(0.02 * sr))`; se recorta a lo que pida la receta.
- Recetas: `kick` seno con pitch `base * (2 ** (tune * 0.5))` y caída exponencial desde 2.6× + click de ruido de 2 ms; `snare` dos senos (180 y 330 Hz escalados por tune) + ruido high-pass; `clap` ruido high-pass con 4 bursts de 1 ms separados 10 ms y cola; `tom` seno con caída de pitch 1.6×; `chat`/`ohat` ruido high-pass con `tau` corto/largo.
- Decaimiento: `tau = tau_base + tau_span * decay`.
- Normalización final: `y = (y / max|y|) * 0.9`; devolver `y.astype(np.float32)`.
- `DRUM_REGISTRY` con `tune` (−1..1, default 0), `decay` (0..1, default 0.5), `level` (0..1, defaults 0.6/0.5/0.45/0.5/0.35/0.3 para kick/snare/clap/tom/chat/ohat), `bpm` (60..180, default 110, unidad `"BPM"`), `swing` (0..0.6, default 0).

- [ ] **Step 4: Correr los tests y ver que pasan**

Run: `./.venv/bin/python -m pytest tests/test_drums.py -q`
Expected: PASS (5 tests + 6 parametrizados).

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/drums.py tests/test_drums.py
git commit -S -m "feat(drums): synthesized one-shots and drum parameter registry"
```

---

### Task 2: Secuenciador y mezcla (`DrumMachine`)

**Files:**
- Modify: `nomad/engine/drums.py`
- Test: `tests/test_drums.py`

**Interfaces:**
- Consumes: `render_one_shot`, `DRUM_REGISTRY`, `INSTRUMENTS`, `STEPS` de Task 1.
- Produces:

```python
class DrumMachine:
    def __init__(self, sample_rate: float = 48000.0) -> None: ...
    sr: float
    values: dict[str, float]      # caller-side, readonly para la GUI
    steps: np.ndarray             # (6, 16) uint8: 0 off, 1 normal, 2 acento
    playing: bool
    position: int                 # 0..15, escritura del hilo de audio
    errors: int
    def set_step(self, track: str, step: int, level: int) -> None: ...
    def clear_pattern(self) -> None: ...
    def set_value(self, name: str, value: float) -> None: ...
    def trigger(self, track: str, velocity: float = 1.0) -> None: ...
    def start(self) -> None: ...
    def stop(self) -> None: ...
    def toggle(self) -> None: ...
    def stop_all(self) -> None: ...   # stop + fade de 5 ms
    def render(self, out: np.ndarray, n: int) -> None: ...
```

- [ ] **Step 1: Escribir los tests que fallan**

```python
from nomad.engine.drums import DrumMachine


def make_dm(bpm=120.0):
    dm = DrumMachine(SR)
    dm.clear_pattern()
    dm.set_value("bpm", bpm)
    dm.set_step("chat", 0, 1)
    return dm


def render(dm, sizes):
    chunks = []
    for n in sizes:
        b = np.zeros(n)
        dm.render(b, n)
        chunks.append(b)
    return np.concatenate(chunks)


def test_default_pattern_makes_sound_when_playing():
    dm = DrumMachine(SR)
    assert dm.steps[0, 0] == 2 and dm.steps[0, 4] == 1 and dm.steps[1, 4] == 1
    dm.start()
    out = render(dm, [256] * 8)
    assert np.abs(out).max() > 0.01


def test_position_advances_exactly_at_the_step_boundary():
    dm = make_dm(120.0)          # 16th a 120 BPM = 6000 frames
    dm.start()
    render(dm, [5999])
    assert dm.position == 0
    render(dm, [1])
    assert dm.position == 1


def test_block_size_does_not_change_the_groove():
    a = render(make_dm(), [16384])
    b = render(make_dm(), [256] * 64)
    c = render(make_dm(), [37, 1, 500, 128, 1000, 3000, 2000, 9718])
    assert np.allclose(a, b, atol=1e-9)
    assert np.allclose(a, c, atol=1e-9)


def test_accent_is_louder_than_a_normal_step():
    quiet, loud = make_dm(), make_dm()
    quiet.set_step("chat", 0, 1)
    loud.set_step("chat", 0, 2)
    for dm in (quiet, loud):
        dm.start()
    assert np.abs(render(loud, [4000])).max() > 1.4 * np.abs(render(quiet, [4000])).max()


def test_retrigger_restarts_the_hit():
    dm = make_dm()
    dm.trigger("kick")
    render(dm, [3000])
    tail = np.abs(render(dm, [4000])).max()
    dm.trigger("kick")
    assert np.abs(render(dm, [4000])).max() > 2 * tail


def test_tone_change_while_the_hit_is_playing_is_safe():
    dm = DrumMachine(SR)
    dm.trigger("kick")
    render(dm, [256])
    dm.set_value("kick_decay", 0.1)
    dm.set_value("kick_decay", 0.9)   # array nuevo, offset viejo más grande
    out = render(dm, [4096])
    assert np.isfinite(out).all()


def test_start_is_not_retriggered_and_stop_returns_to_step_zero():
    dm = make_dm()
    dm.start()
    dm.start()
    render(dm, [6000])
    assert dm.position == 1
    dm.stop()
    assert dm.position == 0 and not dm.playing
    dm.start()
    assert dm.position == 0


def test_panic_cuts_everything_within_five_ms():
    dm = DrumMachine(SR)
    dm.set_step("ohat", 0, 2)
    dm.start()
    render(dm, [480])
    dm.stop_all()
    tail = render(dm, [480])
    assert not dm.playing and dm.position == 0
    assert np.abs(tail[240:]).max() < 1e-3


def test_bpm_and_swing_apply_without_retriggering():
    dm = make_dm(120.0)
    dm.start()
    render(dm, [3000])
    dm.set_value("bpm", 180.0)
    render(dm, [3000])            # el paso 0 no se repite
    assert dm.position == 1
    dm.set_value("swing", 0.6)
    render(dm, [2000])
    assert dm.position == 2       # 180 BPM = 4000 frames por paso, swing par = 6400


def test_values_are_clamped_and_unknown_names_raise():
    dm = DrumMachine(SR)
    dm.set_value("bpm", 9999.0)
    dm.set_value("swing", float("nan"))
    assert dm.values["bpm"] == 180.0
    assert dm.values["swing"] == 0.0
    with pytest.raises(KeyError):
        dm.set_value("cowbell_level", 1.0)
    with pytest.raises(IndexError):
        dm.set_step("kick", 16, 1)


def test_render_does_not_allocate(monkeypatch):
    dm = DrumMachine(SR)
    dm.start()
    out = np.zeros(256)
    dm.render(out, 256)
    calls = {"n": 0}
    for name in ("empty", "zeros", "ones", "full", "arange", "linspace"):
        real = getattr(np, name)
        def counted(*a, __real=real, **k):
            calls["n"] += 1
            return __real(*a, **k)
        monkeypatch.setattr(np, name, counted)
    for _ in range(40):
        dm.render(out, 256)
    assert calls["n"] == 0
```

- [ ] **Step 2: Correr y ver que fallan**

Run: `./.venv/bin/python -m pytest tests/test_drums.py -q`
Expected: FAIL con `ImportError: cannot import name 'DrumMachine'`.

- [ ] **Step 3: Implementar `DrumMachine`**

Detalles que los tests no fijan:

- Patrón inicial: kick en 0/4/8/12 (acento en 0), snare en 4/12, chat en los pasos pares.
- Reloj: `_elapsed` en frames; `duration(step) = (sr*60/bpm/4) * (1 + swing)` para pasos pares y `* (1 - swing)` para impares. En cada bloque, avanzar en tramos que terminan en el límite de paso; al cruzar, `_elapsed -= duration`, `position = (position+1) % STEPS` y disparar. `start()` marca `_pending_start`: el primer `render` dispara el paso 0 en la primera muestra. `bpm` se lee fresco en cada tramo (re-temporiza el resto del paso sin re-disparar).
- Mezcla: `_active` por instrumento `[array | None, offset, gain]`; `_mix` y `_tmp` preasignados de `STEPS`-independiente tamaño `CONTROL_BLOCK` (crecer sólo si `n` los supera), `_ramp = np.arange(256)` creado en `__init__`.
- `render(out, n)`: si no hay transporte ni golpes activos ni fade, volver sin tocar `out`. Si hay fade, multiplicar el aporte de la batería por una rampa lineal de 5 ms antes de sumarlo.
- `set_value`: valida con el `ParamSpec` de `DRUM_REGISTRY`; para `{track}_tune`/`{track}_decay` regenera el array en el hilo que llama y reemplaza la referencia del banco; para `level`/`bpm`/`swing` actualiza `values`.
- `trigger` con `velocity` recortada a 0..1 y multiplicada por el `level` del instrumento.
- `set_step` valida `track in INSTRUMENTS` y `0 <= step < STEPS`, escribe `uint8` 0/1/2.
- Si la regeneración de un one-shot supera ~5 ms en la medición manual, limitarla a una cada 50 ms
  (siempre aplicando el último valor al soltar la perilla).

- [ ] **Step 4: Correr y ver que pasan**

Run: `./.venv/bin/python -m pytest tests/test_drums.py -q`
Expected: PASS (16 tests).

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/drums.py tests/test_drums.py
git commit -S -m "feat(drums): 16-step sequencer, swing clock and allocation-free mix"
```

---

### Task 3: Integración con `Engine`

**Files:**
- Modify: `nomad/engine/engine.py` (constructor y `render`; rama `panic` de `_handle`)
- Test: `tests/test_drums.py`

**Interfaces:**
- Consumes: `DrumMachine` de Task 2.
- Produces: `Engine.drums: DrumMachine`; `Engine.render` mezcla la batería después de las voces y antes de la ganancia master; `Engine.panic()` llama `drums.stop_all()`.

- [ ] **Step 1: Escribir los tests que fallan**

```python
from nomad.engine.engine import Engine


def test_drums_stopped_leaves_engine_output_identical():
    a, b = Engine(SR), Engine(SR)
    for e in (a, b):
        e.note_on(60, 0.8)
    assert np.array_equal(render_engine(a, 8), render_engine(b, 8))


def test_drums_are_audible_without_any_note():
    e = Engine(SR)
    e.drums.start()
    out = render_engine(e, 16)
    assert np.abs(out).max() > 0.01


def test_panic_silences_the_drums():
    e = Engine(SR)
    e.drums.start()
    render_engine(e, 4)
    e.panic()
    tail = render_engine(e, 4)
    assert np.abs(tail[240:]).max() < 1e-3
    assert not e.drums.playing


def render_engine(engine, blocks):
    chunks = []
    for _ in range(blocks):
        b = np.zeros(256)
        engine.render(b)
        chunks.append(b)
    return np.concatenate(chunks)
```

- [ ] **Step 2: Correr y ver que fallan**

Run: `./.venv/bin/python -m pytest tests/test_drums.py -q -k "engine or audible or panic"`
Expected: FAIL con `AttributeError: 'Engine' object has no attribute 'drums'`.

- [ ] **Step 3: Modificar `nomad/engine/engine.py`**

- `from nomad.engine.drums import DrumMachine`; en `__init__`, `self.drums = DrumMachine(self.sr)` y documentar la mezcla en el docstring del módulo.
- En `render`, después de `self.vm.render(b, p, self._mods, self._resolve)` y antes del cálculo de `g0/g1`: `self.drums.render(b, n)`.
- En `_handle`, rama `kind == 4` (panic): `self.vm.panic(); self.drums.stop_all()`.
- Sin cambios en `snapshot()` ni en `note_on/note_off`.

- [ ] **Step 4: Correr toda la suite**

Run: `./.venv/bin/python -m pytest -q`
Expected: PASS (los 38 existentes + los nuevos; `test_render_does_not_allocate` sigue pasando).

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/engine.py tests/test_drums.py
git commit -S -m "feat(engine): mix drum machine into the master bus and panic it"
```

---

### Task 4: Vista DRUMS (`DrumView`)

**Files:**
- Create: `nomad/gui/drums_view.py`
- Test: `tests/test_drums_view.py`

**Interfaces:**
- Consumes: `DrumMachine`, `DRUM_REGISTRY`, `INSTRUMENTS`, `STEPS`; `Knob`, `KnobModel` y la paleta `BG, GREEN, GREY, TROUGH` de `nomad/gui/knob.py`.
- Produces:

```python
ACCENT = "#b9ffd8"
PLAYHEAD = "#ffffff"

class DrumView(tk.Frame):
    def __init__(self, parent, drums: DrumMachine,
                 register_help=None, on_context=None) -> None: ...
    knobs: dict[str, Knob]          # "kick_level", "bpm", ...
    def toggle_step(self, track: str, step: int, accent: bool = False) -> None: ...
    def set_value(self, name: str, value: float) -> None: ...
    def reset(self, name: str) -> None: ...
    def refresh(self) -> None: ...   # lee drums.position/steps/values
    def toggle_transport(self) -> None: ...
    def clear(self) -> None: ...
```

- [ ] **Step 1: Escribir los tests que fallan**

```python
# tests/test_drums_view.py
import tkinter as tk

import pytest

from nomad.engine.drums import DrumMachine
from nomad.gui.drums_view import PLAYHEAD, DrumView


@pytest.fixture
def view():
    try:
        root = tk.Tk()
    except tk.TclError as e:
        if "display" not in str(e):
            raise
        pytest.skip("no display")
    dm = DrumMachine(48000)
    v = DrumView(root, dm)
    root.update()
    yield v, dm
    root.destroy()


def test_click_toggles_and_shift_accents(view):
    v, dm = view
    v.toggle_step("kick", 0, accent=False)
    assert dm.steps[0, 0] == 0          # el patrón inicial lo tenía prendido
    v.toggle_step("kick", 0, accent=False)
    assert dm.steps[0, 0] == 1
    v.toggle_step("kick", 1, accent=True)
    assert dm.steps[0, 1] == 2
    v.toggle_step("kick", 1, accent=True)
    assert dm.steps[0, 1] == 0


def test_refresh_marks_playhead(view):
    v, dm = view
    dm.start()
    v.refresh()
    assert v._steps[0][0].cget("highlightbackground") == PLAYHEAD


def test_knobs_write_through_to_the_machine(view):
    v, dm = view
    v.knobs["kick_level"].on_change(0.2)
    v.knobs["bpm"].on_change(140.0)
    assert dm.values["kick_level"] == pytest.approx(0.2)
    assert dm.values["bpm"] == pytest.approx(140.0)


def test_reset_returns_a_knob_to_its_default(view):
    v, dm = view
    v.set_value("kick_level", 0.1)
    v.reset("kick_level")
    assert dm.values["kick_level"] == pytest.approx(0.6)


def test_transport_and_clear(view):
    v, dm = view
    v.toggle_transport()
    assert dm.playing
    v.toggle_transport()
    assert not dm.playing
    v.clear()
    assert not dm.steps.any()


def test_fits_in_the_panel(view):
    v, _ = view
    assert v.winfo_reqwidth() <= 960 and v.winfo_reqheight() <= 560
```

- [ ] **Step 2: Correr y ver que fallan**

Run: `./.venv/bin/python -m pytest tests/test_drums_view.py -q`
Expected: FAIL con `ModuleNotFoundError: No module named 'nomad.gui.drums_view'`.

- [ ] **Step 3: Implementar `nomad/gui/drums_view.py`**

Layout: transporte arriba (`PLAY/STOP` como `tk.Button` propio, `Knob` de `bpm` y `swing`, botón `CLEAR`), después una fila por instrumento: botón con el nombre (audiciona con `drums.trigger`), tres `Knob` (`tune`, `decay`, `level`) y 16 botones de paso.

- Paso encendido: `bg=GREEN, fg=BG`; acento: `bg=ACCENT, fg=BG`; apagado: `bg=TROUGH, fg=GREY`.
- Playhead en `refresh()`: `highlightthickness=2` y `highlightbackground=PLAYHEAD` en la columna `drums.position`; el resto en `BG`.
- `toggle_step(track, step, accent)`: si `accent`, `2 ↔ 0`; si no, `0 ↔ 1`; llama `drums.set_step`.
- Bindings: `<Button-1>` → `toggle_step(..., accent=bool(e.state & 1))`; los 16 botones se guardan en `self._steps[track][step]`.
- `refresh()` además sincroniza `knobs[name].set_value(drums.values[name])` (silencioso, como el poll del panel).
- `register_help(widget, key)` y `on_context(name, event)` se delegan si vienen; claves de ayuda: `{track}_{tune,decay,level}`, `bpm`, `swing`, y elementos `drums_view`, `drum_steps`, `drum_transport`.

- [ ] **Step 4: Correr y ver que pasan**

Run: `./.venv/bin/python -m pytest tests/test_drums_view.py -q`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add nomad/gui/drums_view.py tests/test_drums_view.py
git commit -S -m "feat(gui): drum machine view with step grid and transport"
```

---

### Task 5: Pestaña en el panel, ayuda y textos

**Files:**
- Modify: `nomad/gui/app.py` (construcción, `show_view`, poll, `_menu_items`/reset de perillas de batería)
- Modify: `nomad/gui/help.py` (`_DRUMS`, `_ELEMENTS`)
- Modify: `tests/test_help.py`, `tests/test_layout.py`
- Test: `tests/test_drums_view.py`

**Interfaces:**
- Consumes: `DrumView` de Task 4; `App._btn`, `App._help_register`, `App._menu`.
- Produces: `App.view: str` (`"synth"` | `"drums"`), `App.show_view(name: str) -> None`, `App.drums_view: DrumView`.

- [ ] **Step 1: Escribir los tests que fallan**

```python
# tests/test_layout.py (agregar)
def test_drums_tab_switches_views_and_hides_keyboard(app):
    assert app.view == "synth" and app.keyboard_frame.winfo_ismapped()
    app.show_view("drums")
    app.root.update()
    assert app.view == "drums"
    assert not app.keyboard_frame.winfo_ismapped()
    assert app.drums_view.winfo_ismapped()
    app.show_view("synth")
    app.root.update()
    assert app.keyboard_frame.winfo_ismapped()


def test_drums_tab_is_reachable_from_the_header(app):
    assert app.synth_btn.cget("text") == "SYNTH" and app.drums_btn.cget("text") == "DRUMS"


# tests/test_help.py (agregar)
def test_every_drum_control_has_help():
    from nomad.engine.drums import DRUM_REGISTRY
    for name in DRUM_REGISTRY:
        title, body = help_for(name)
        assert title and len(body) > 40, name
    for key in ("drums_view", "drum_steps", "drum_transport"):
        assert help_for(key)[0]
```

- [ ] **Step 2: Correr y ver que fallan**

Run: `./.venv/bin/python -m pytest tests/test_layout.py::test_drums_tab_switches_views_and_hides_keyboard tests/test_help.py::test_every_drum_control_has_help -q`
Expected: FAIL con `AttributeError: 'App' object has no attribute 'view'`.

- [ ] **Step 3: Modificar `app.py` y `help.py`**

- `_build`: el `left` actual pasa a contener `self.synth_view = tk.Frame(...)` (las secciones se gridean ahí) y `self.drums_view = DrumView(self.left, self.engine.drums, register_help=self._help_register, on_context=self._drum_context)`, ambos en la misma celda; arranca visible `synth_view`.
- `_build_header`: dos `self._btn` `SYNTH` y `DRUMS` (guardados como `self.synth_btn` / `self.drums_btn`), registrados en ayuda con las claves `synth_view`/`drums_view`.
- `show_view(name)`: valida `name in ("synth", "drums")`; `grid_remove` del que sale y `grid` del que entra; pinta el botón activo como el botón de ayuda (`bg=GREEN, fg=BG`); oculta/muestra `self.keyboard_frame` con `grid_remove`/`grid`; mensaje breve en `self.status` al entrar a DRUMS (`"DRUMS: PLAY para arrancar, shift+click acentúa"`).
- `_poll`: llamar `self.drums_view.refresh()` en cada vuelta.
- `_context`/`_menu_items`/reset: si `name` no está en `self.widgets`, resolverlo contra `self.drums_view.knobs`; el menú ofrece sólo "Valor por defecto" y llama `drums_view.reset(name)`.
- `help.py`: `_DRUMS` con textos para `tune` ("afina el instrumento"), `decay` ("largo del golpe") y `level` ("volumen del instrumento"); `help_for` resuelve `head in INSTRUMENTS` (importar `INSTRUMENTS` de `drums.py`) y agrega `bpm`/`swing` a `_FIXED`. En `_ELEMENTS`: `drums_view` ("vista de la drum machine"), `drum_steps` ("grilla de 16 pasos; shift+click acentúa"), `drum_transport` ("PLAY/STOP, tempo, swing y borrar patrón").

- [ ] **Step 4: Correr toda la suite**

Run: `./.venv/bin/python -m pytest -q`
Expected: PASS (incluye `test_window_fits_1366x768`).

- [ ] **Step 5: Commit**

```bash
git add nomad/gui/app.py nomad/gui/help.py tests/test_layout.py tests/test_help.py
git commit -S -m "feat(gui): synth/drums tabs, drum help texts and panel wiring"
```

---

### Task 6: Documentación y verificación manual

**Files:**
- Modify: `README.md` (sección "Drum machine" y tabla de controles)
- Test: ninguno nuevo; la verificación manual del spec §9

**Interfaces:**
- Consumes: todo lo anterior.
- Produces: README al día.

- [ ] **Step 1: Actualizar `README.md`**

Agregar en "Controles" las filas: pestañas `SYNTH / DRUMS`, click en un paso (prende/apaga), shift+click (acento), click en el nombre del instrumento (audiciona), y una sección corta "Drum machine" que liste los seis instrumentos, 16 pasos, `bpm`, `swing`, `CLEAR`, y aclare que no hay samples ni guardado de patrones.

- [ ] **Step 2: Verificación manual en la app**

Run: `./.venv/bin/python -m nomad.main --device 6`
Expected: la vista DRUMS entra en pantalla; PLAY suena afinado y sin xruns; mover `bpm`/`swing` no repite pasos; cambiar `tune`/`decay` con el patrón andando no clickea; volver a SYNTH y tocar un acorde suena junto a la batería; XRUNS no sube en un minuto.

- [ ] **Step 3: Correr la suite completa una última vez**

Run: `./.venv/bin/python -m pytest -q`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -S -m "docs: document the drum machine view and gestures"
```
