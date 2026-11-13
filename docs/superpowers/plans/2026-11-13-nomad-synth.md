# NOMAD SYNTH — Plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Construir NOMAD SYNTH: un sintetizador polifónico de 8 voces en Python, con dos osciladores por voz, dos ADSR, filtro escalera resonante, dos LFO, matriz de modulación, MIDI con learn, osciloscopio real y panel tipo instrumento.

**Architecture:** `nomad/engine` es DSP puro y no importa GUI ni MIDI; la GUI y el MIDI sólo hablan con `Engine` por `note_on`, `note_off`, `set_param` y `render`, a través de una cola sin locks. El audio se procesa en granos de 16 muestras sobre bloques de cualquier tamaño (el quantum de PipeWire puede cambiar), con todo preasignado. El filtro es una escalera de 4 polos con coeficientes en forma cerrada y `scipy.signal.lfilter`.

**Tech Stack:** Python 3.14, numpy 2.5, scipy 1.18 (`lfilter`), sounddevice 0.5, mido 1.3 + python-rtmidi, tkinter (Canvas).

**Spec:** `docs/superpowers/specs/2026-11-13-nomad-synth-design.md`

## Global Constraints

- 48 kHz, mono interno; el stream es estéreo con la misma señal.
- Grano de 16 muestras; `render()` acepta cualquier cantidad de frames y lleva estado entre llamadas.
- En el callback de audio: sin locks, sin I/O, sin `print`, sin allocations de numpy (buffers preasignados en `__init__`).
- `nomad/engine/**` no importa `nomad/gui/**`, `nomad/midi/**` ni `sounddevice`.
- Polifonía 1..8, default 8. Robo con fade de 5 ms.
- Cutoff 20 Hz..18 kHz; el filtro nunca produce NaN ni infinitos.
- Salida del master siempre ≤ 1.0 (limitador `tanh`).
- Dependencias permitidas: numpy, scipy, sounddevice, mido, python-rtmidi. Nada más.
- El panel entra en 1366×768 (y se puede achicar a 1200×700 sin romper).
- **Commits firmados:** todos los `git commit` van con `-S`. Si el agente no tiene TTY para GPG, deja staging hecho y pasa el comando exacto al humano; no commitea sin firma y no usa `--no-verify`.
- Rama de trabajo: `feature/nomad-synth`.
- Idioma: identificadores, comentarios y docstrings en inglés; mensajes de commit en inglés.

## Review Focus

Estos son los casos que el espíritu del spec cubre pero que ningún test de tarea obvia: cada uno tiene su test en la tarea que lo posee.

1. **Cambio de quantum en runtime** — PipeWire puede entregar 128, 512 o 1024 frames aunque el prototipo use 256. Un bloque de tamaño distinto no puede cambiar el sonido ni desincronizar el LFO (Tarea 10).
2. **Extremos de parámetros** — cutoff 20 Hz con resonancia al máximo, attack 1 ms, release 8 s, 8 voces a la vez: nada de NaN, nada de divergencia, nada por encima de 1.0 (Tareas 5 y 10).
3. **Cerrar la ventana con notas sostenidas** — el stream y el puerto MIDI se cierran y no queda un click ni un hilo vivo (Tarea 18).
4. **No hay MIDI o el puerto desaparece** — arrancar sin controlador y desenchufarlo en caliente no tumba la app (Tareas 13 y 18).
5. **Bajar la polifonía con un acorde sonando** — de 8 a 1 las voces sobrantes se van con fade, sin click ni notas colgadas (Tarea 9).

---

### Task 1: Paquete, dependencias y parámetros

**Files:**
- Create: `requirements.txt`, `nomad/__init__.py`, `nomad/engine/__init__.py`, `nomad/engine/params.py`, `tests/__init__.py`, `tests/test_params.py`, `pyproject.toml`

**Interfaces:**
- Produces: `ParamSpec`, `REGISTRY: dict[str, ParamSpec]`, `clamp(name, value) -> float`, `to_norm(name, value) -> float`, `from_norm(name, norm) -> float`, `Smoother(target: float, sample_rate: float, time_ms: float)` con `.target`, `.value`, `.set(target)`, `.advance(frames) -> float`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_params.py
import pytest
from nomad.engine.params import REGISTRY, ParamSpec, clamp, to_norm, from_norm

def test_every_param_has_sane_default():
    for name, p in REGISTRY.items():
        assert p.lo <= p.default <= p.hi, name

def test_clamp_respects_range():
    assert clamp("filt_cutoff", 1e9) == REGISTRY["filt_cutoff"].hi
    assert clamp("filt_cutoff", -5) == REGISTRY["filt_cutoff"].lo

def test_exp_curve_roundtrip():
    v = 1200.0
    assert from_norm("filt_cutoff", to_norm("filt_cutoff", v)) == pytest.approx(v, rel=1e-9)

def test_unknown_param_raises():
    with pytest.raises(KeyError):
        clamp("no_existe", 1.0)

def test_smoother_approaches_target_without_overshoot():
    from nomad.engine.params import Smoother
    s = Smoother(0.0, 48000.0, 15.0)
    s.set(1.0)
    vals = [s.advance(16) for _ in range(400)]   # 133 ms = 8.9 tau
    assert vals[-1] == pytest.approx(1.0, abs=1e-3)
    assert all(0.0 <= v <= 1.0 for v in vals)
```

- [ ] **Step 2: Correr el test y ver que falla**

Run: `pytest tests/test_params.py -v`
Expected: FAIL `ModuleNotFoundError: No module named 'nomad'`

- [ ] **Step 3: Crear el paquete y el registro**

`requirements.txt`:

```
numpy>=2.5
scipy>=1.18
sounddevice>=0.5
mido>=1.3
python-rtmidi>=1.5
pytest>=8
```

`params.py` define `ParamSpec(name, default, lo, hi, curve, unit="", smooth_ms=15.0)`, `REGISTRY` con exactamente estos parámetros y rangos:

| grupo | nombres |
|---|---|
| motor | `polyphony` 1..8 (8), `master_volume` 0..1 (0.7) |
| osc1/osc2 | `osc{n}_wave` 0..3 (2), `osc{n}_mode` 0..1 (0), `osc{n}_coarse` −24..24 (0), `osc{n}_hz` exp 20..8000 (220), `osc{n}_fine` −50..50 (0), `osc{n}_pw` 0.05..0.95 (0.5), `osc{n}_level` 0..1 (0.5), `osc{n}_sync` 0..1 (0) |
| filtro | `filt_cutoff` exp 20..18000 (1200), `filt_reso` 0..1 (0.3), `filt_env_amt` −1..1 (0) |
| amp env | `amp_attack` exp 1..5000 ms (5), `amp_decay` exp 1..5000 (300), `amp_sustain` 0..1 (0.8), `amp_release` exp 5..8000 (400) |
| filt env | `filt_attack` exp 1..5000 (5), `filt_decay` exp 1..5000 (250), `filt_sustain` 0..1 (0.3), `filt_release` exp 5..8000 (400) |
| lfo1/lfo2 | `lfo{n}_rate` exp 0.02..40 (1.0), `lfo{n}_wave` 0..4 (0), `lfo{n}_depth` 0..1 (0), `lfo{n}_phase` 0..1 (0) |
| matriz (i=0..7) | `mod{i}_src` 0..5 (0), `mod{i}_dst` 0..7 (0), `mod{i}_amt` −1..1 (0) |
| midi | `bend_range` 1..12 (2) |

`to_norm`/`from_norm` usan `lin` o `exp` según `curve`; `exp` es logarítmica entre `lo` y `hi`.

`Smoother` es un one-pole exponencial: cada `advance(frames)` acerca `value` a `target` con coeficiente `1 - exp(-frames / (time_ms/1000 * sr))` y devuelve el valor nuevo, sin sobrepasar.

`pyproject.toml` sólo lleva la configuración de pytest:

```toml
[tool.pytest.ini_options]
pythonpath = ["."]
testpaths = ["tests"]
```

- [ ] **Step 4: Instalar dependencias**

Run: `.venv/bin/pip install -r requirements.txt`
Expected: termina con `Successfully installed ... scipy ... mido ... python-rtmidi ...`

- [ ] **Step 5: Correr el test y ver que pasa**

Run: `pytest tests/test_params.py -v`
Expected: 5 passed

- [ ] **Step 6: Commit**

```bash
git add requirements.txt pyproject.toml nomad tests
git commit -S -m "feat(engine): add parameter registry with linear and exponential curves"
```

---

### Task 2: Oscilador — cuatro formas con PolyBLEP

**Files:**
- Create: `nomad/engine/oscillator.py`, `tests/test_oscillator.py`

**Interfaces:**
- Consumes: nada.
- Produces: `Oscillator(sample_rate: float)` con `wave: int`, `phase: float`, y `render_into(out: np.ndarray, freq: float, pw: float = 0.5, sync_master_phase: np.ndarray | None = None) -> float` que **suma** la onda en `out[:len(out)]`, avanza la fase y devuelve la fase nueva.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_oscillator.py
import numpy as np
from nomad.engine.oscillator import Oscillator

SR = 48000.0

def render(wave, freq, n=64, **kw):
    o = Oscillator(SR); o.wave = wave
    out = np.zeros(n); o.render_into(out, freq, **kw)
    return out

def test_sine_matches_reference():
    out = render(0, 440.0, 64)
    ref = np.sin(2*np.pi*440.0*np.arange(64)/SR)
    assert np.allclose(out, ref, atol=1e-6)

def test_phase_continues_across_calls():
    o = Oscillator(SR); o.wave = 2
    a = np.zeros(64); b = np.zeros(64)
    o.render_into(a, 440.0); o.render_into(b, 440.0)
    o2 = Oscillator(SR); o2.wave = 2
    whole = np.zeros(128); o2.render_into(whole, 440.0)
    assert np.allclose(np.concatenate([a, b]), whole, atol=1e-9)

def test_polyblep_reduces_aliasing_vs_naive():
    # 5236 Hz a 48 kHz: los alias caen por debajo de -30 dB con BLEP
    out = render(2, 5236.0, 4096)
    spec = np.abs(np.fft.rfft(out * np.hanning(len(out))))
    f = np.fft.rfftfreq(len(out), 1/SR)
    alias_band = (f > 8000) & (f < 16000)
    assert 20*np.log10(spec[alias_band].max() / spec.max()) < -30

def test_square_duty_is_half_at_pw_50():
    out = render(3, 200.0, 4800, pw=0.5)
    assert abs((out > 0).mean() - 0.5) < 0.02
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_oscillator.py -v`
Expected: FAIL `ModuleNotFoundError: nomad.engine.oscillator`

- [ ] **Step 3: Implementar**

`phase` en ciclos [0,1). `inc = freq / sample_rate`. La corrección BLEP es esta (va literal, es la parte que el test no determina):

```python
def _blep(t, dt):
    """Corrección PolyBLEP; t es la fase [0,1), dt el incremento."""
    if t < dt:
        x = t / dt
        return x + x - x * x - 1.0
    if t > 1.0 - dt:
        x = (t - 1.0) / dt
        return x * x + x + x + 1.0
    return 0.0
```

- sine: `np.sin(2π·t)`.
- saw: `2t − 1 − blep(t)`.
- square: dos `blep` (en `t` y en `t − pw`), amplitud ±1.
- triangle: integrar el square y normalizar por la amplitud máxima teórica (`4·freq/sr` acumulado), con reset de deriva a cero cada bloque.
- `wave` fuera de 0..3 → `ValueError`.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_oscillator.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/oscillator.py tests/test_oscillator.py
git commit -S -m "feat(engine): add four-wave oscillator with PolyBLEP anti-aliasing"
```

---

### Task 3: Oscilador — pulse width y sync

**Files:**
- Modify: `nomad/engine/oscillator.py`
- Modify: `tests/test_oscillator.py`

**Interfaces:**
- Consumes: `Oscillator` de Task 2.
- Produces: `render_into(..., pw: float, sync_master_phase: np.ndarray | None)`; `pw` se recorta a 0.05..0.95.

- [ ] **Step 1: Escribir el test que falla**

```python
def test_pulse_width_changes_duty():
    out = render(3, 200.0, 4800, pw=0.25)
    assert abs((out > 0).mean() - 0.25) < 0.02

def test_pw_is_clamped():
    out = render(3, 200.0, 4800, pw=0.0)
    assert abs((out > 0).mean() - 0.05) < 0.02

def test_sync_resets_phase_at_master_wrap():
    master = np.concatenate([np.linspace(0, 0.9, 32), np.linspace(0, 0.9, 32)])
    out = render(2, 700.0, 64, sync_master_phase=master)
    assert np.isfinite(out).all() and np.abs(out).max() <= 1.5

def test_pw_ignored_by_sine():
    a = render(0, 200.0, 256, pw=0.2); b = render(0, 200.0, 256, pw=0.8)
    assert np.allclose(a, b)
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_oscillator.py -v`
Expected: FAIL con assertion de duty (no existe `pw` real) o `TypeError`

- [ ] **Step 3: Implementar**

Pulse = dos saws desfasadas `pw` con BLEP en ambos bordes (con `pw=0.5` coincide con square). Sync: donde `sync_master_phase` retrocede (wrap), reiniciar la fase local y aplicar corrección BLEP por la discontinuidad; como el reinicio puede caer entre muestras, calcular el offset fraccionario con el cruce y restar la corrección escalada.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_oscillator.py -v`
Expected: 8 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/oscillator.py tests/test_oscillator.py
git commit -S -m "feat(engine): add pulse width and hard sync to oscillator"
```

---

### Task 4: Envelope ADSR

**Files:**
- Create: `nomad/engine/envelope.py`, `tests/test_envelope.py`

**Interfaces:**
- Produces: `Envelope(sample_rate: float)` con `set_params(attack_s, decay_s, sustain, release_s)`, `note_on()`, `note_off()`, `process(out: np.ndarray) -> None`, `value: float`, `is_idle() -> bool`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_envelope.py
import numpy as np
from nomad.engine.envelope import Envelope

SR = 48000.0

def test_attack_reaches_one_at_attack_time():
    e = Envelope(SR); e.set_params(0.01, 0.05, 0.6, 0.2); e.note_on()
    out = np.zeros(int(SR * 0.02)); e.process(out)
    assert out.max() >= 0.99
    assert abs(int(np.argmax(out)) - int(SR * 0.01)) <= 2  # llega a 1 en attack

def test_decays_to_sustain():
    e = Envelope(SR); e.set_params(0.005, 0.05, 0.4, 0.2); e.note_on()
    out = np.zeros(int(SR * 0.2)); e.process(out)
    assert abs(out[-1] - 0.4) < 0.01

def test_release_starts_from_current_level():
    e = Envelope(SR); e.set_params(0.1, 0.1, 0.8, 0.2); e.note_on()
    a = np.zeros(int(SR * 0.05)); e.process(a)      # a mitad del attack
    level = a[-1]
    e.note_off()
    b = np.zeros(64); e.process(b)
    assert abs(b[0] - level) < 0.01                 # sin salto

def test_release_goes_silent_and_idles():
    e = Envelope(SR); e.set_params(0.001, 0.01, 0.5, 0.05); e.note_on()
    e.process(np.zeros(2000)); e.note_off()
    out = np.zeros(4000); e.process(out)
    assert out[-1] < 0.001 and e.is_idle()

def test_sustain_zero_decays_to_zero():
    e = Envelope(SR); e.set_params(0.001, 0.02, 0.0, 0.05); e.note_on()
    out = np.zeros(int(SR * 0.1)); e.process(out)
    assert out[-1] < 0.001
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_envelope.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

Segmentos exponenciales con coeficiente por muestra calculado por bloque (nada de `exp` por muestra). Release calculado desde `value`, no desde sustain. `is_idle()` cuando release terminó (nivel < 1e-4).

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_envelope.py -v`
Expected: 5 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/envelope.py tests/test_envelope.py
git commit -S -m "feat(engine): add ADSR envelope with release from current level"
```

---

### Task 5: Filtro escalera de 4 polos

**Files:**
- Create: `nomad/engine/filter.py`, `tests/test_filter.py`

**Interfaces:**
- Produces: `ladder_coeffs(fc_hz: float, k: float, sample_rate: float) -> tuple[np.ndarray, np.ndarray]` y `Ladder4Pole(sample_rate)` con `set(fc_hz, resonance)`, `process(buf: np.ndarray) -> None` (in place), `reset()`.
- `resonance` 0..1 mapea a `k = resonance * 4.3`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_filter.py
import numpy as np
from scipy.signal import freqz
from nomad.engine.filter import Ladder4Pole, ladder_coeffs

SR = 48000.0

def peak_db(b, a, lo=200, hi=4000):
    w, h = freqz(b, a, worN=8192, fs=SR)
    m = (w > lo) & (w < hi)
    return 20*np.log10(np.abs(h[m]).max() + 1e-12)

def test_resonance_raises_the_peak():
    b0, a0 = ladder_coeffs(1000.0, 0.0, SR)
    b1, a1 = ladder_coeffs(1000.0, 3.9, SR)
    assert peak_db(b1, a1) - peak_db(b0, a0) > 15

def test_stable_below_threshold():
    b, a = ladder_coeffs(1000.0, 3.9, SR)
    assert np.abs(np.roots(a)).max() < 1.0

def test_self_oscillation_is_bounded():
    f = Ladder4Pole(SR); f.set(440.0, 1.0)
    x = np.zeros(48000); rng = np.random.default_rng(0)
    x[:200] = rng.standard_normal(200)
    for i in range(0, len(x), 16):
        f.process(x[i:i+16])
    assert np.isfinite(x).all()
    assert np.abs(x[-24000:]).max() < 10.0

def test_no_nan_at_extremes():
    for fc in (20.0, 18000.0):
        for res in (0.0, 0.5, 1.0):
            f = Ladder4Pole(SR); f.set(fc, res)
            buf = np.zeros(256); buf[:] = 0.5
            f.process(buf)
            assert np.isfinite(buf).all()
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_filter.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

La forma cerrada (parte que el test no determina), con `g = tan(pi·fc/fs)`:

```
P = [1+g, -(1-g)]      V = [1, 1]
den = P^4 + k·g^4·V^4
num = g^4·V^4          # ambos normalizados por den[0]
```

`process` recalcula coeficientes por llamada (grano), corre `lfilter(b, a, buf, zi=self._zi)` y guarda `zi`. Si `k > 3.2`, limita el estado después del grano: si `max|zi| > 0.8`, escala `zi` por `0.8/max|zi|`. Grano distinto de 16 no está permitido por el filtro (el engine siempre lo llama con ≤16); agregar `if len(buf) > 16: raise ValueError`.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_filter.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/filter.py tests/test_filter.py
git commit -S -m "feat(engine): add 4-pole ladder filter with closed-form coefficients"
```

---

### Task 6: LFO

**Files:**
- Create: `nomad/engine/lfo.py`, `tests/test_lfo.py`

**Interfaces:**
- Produces: `LFO(sample_rate: float, seed: int = 0)` con `set(rate_hz, wave, depth, phase_offset)` y `advance(seconds: float) -> float` (valor en −1..1, ya multiplicado por `depth`).

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_lfo.py
from nomad.engine.lfo import LFO

def test_rate_is_correct():
    lfo = LFO(48000); lfo.set(2.0, 0, 1.0, 0.0)   # 2 Hz, sine, 1 s
    vals = [lfo.advance(0.001) for _ in range(1000)]
    crossings = sum(1 for a, b in zip(vals, vals[1:]) if a < 0 <= b)
    assert 1 <= crossings <= 3

def test_depth_scales_output():
    lfo = LFO(48000); lfo.set(1.0, 0, 0.0, 0.0)
    assert all(lfo.advance(0.001) == 0.0 for _ in range(100))

def test_sample_and_hold_is_reproducible():
    a = [LFO(48000, seed=7).advance(0.001) for _ in range(1)]
    l1 = LFO(48000, seed=7); l2 = LFO(48000, seed=7)
    l1.set(5.0, 4, 1.0, 0.0); l2.set(5.0, 4, 1.0, 0.0)
    seq1 = [l1.advance(0.001) for _ in range(500)]
    seq2 = [l2.advance(0.001) for _ in range(500)]
    assert seq1 == seq2

def test_phase_offset_shifts_start():
    lfo = LFO(48000); lfo.set(1.0, 0, 1.0, 0.25)   # sine arranca en el pico
    assert abs(lfo.advance(0.0001) - 1.0) < 0.01
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_lfo.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

Ondas 0..4: sine, triangle, saw, square, S&H. S&H toma un valor nuevo del `default_rng` cada ciclo completo. `wave` fuera de rango → `ValueError`.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_lfo.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/lfo.py tests/test_lfo.py
git commit -S -m "feat(engine): add LFO with five waveforms including sample and hold"
```

---

### Task 7: Matriz de modulación

**Files:**
- Create: `nomad/engine/modmatrix.py`, `tests/test_modmatrix.py`

**Interfaces:**
- Consumes: `REGISTRY` (nombres `mod{i}_*`).
- Produces: `ModMatrix()` con `set_slot(i, src, dst, amount)`, `resolve(sources: dict[str, float]) -> dict[str, float]`, `DESTINATIONS` y `SOURCES` como listas ordenadas (la GUI las usa para etiquetas).
- Claves de salida: `pitch`, `pitch1`, `pitch2`, `amp`, `cutoff`, `pw1`, `pw2`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_modmatrix.py
from nomad.engine.modmatrix import ModMatrix, DESTINATIONS, SOURCES

def test_sums_two_sources_on_same_destination():
    m = ModMatrix()
    m.set_slot(0, 1, 5, 0.5)    # LFO1 -> CUTOFF
    m.set_slot(1, 2, 5, 0.25)   # LFO2 -> CUTOFF
    out = m.resolve({"lfo1": 1.0, "lfo2": 1.0})
    assert out["cutoff"] == 0.75

def test_off_slots_contribute_nothing():
    m = ModMatrix(); m.set_slot(0, 0, 0, 1.0)
    assert all(v == 0.0 for v in m.resolve({"lfo1": 1.0}).values())

def test_amount_is_bipolar_and_clamped():
    m = ModMatrix(); m.set_slot(0, 1, 1, -2.0)
    assert m.resolve({"lfo1": 1.0})["pitch"] == -1.0

def test_destination_tables_are_stable():
    assert SOURCES[0] == "off" and DESTINATIONS[0] == "off"
    assert len(DESTINATIONS) == 8
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_modmatrix.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

Ocho slots. `SOURCES = ["off","lfo1","lfo2","modwheel","velocity","env2"]`, `DESTINATIONS = ["off","pitch","pitch1","pitch2","amp","cutoff","pw1","pw2"]`. `resolve` devuelve todas las claves en cero si no hay nada.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_modmatrix.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/modmatrix.py tests/test_modmatrix.py
git commit -S -m "feat(engine): add modulation matrix with eight slots"
```

---

### Task 8: Voz

**Files:**
- Create: `nomad/engine/voice.py`, `tests/test_voice.py`

**Interfaces:**
- Consumes: `Oscillator`, `Envelope`, `Ladder4Pole`, `ladder_coeffs`.
- Produces: `Voice(sample_rate: float, slot: int)` con `note_on(note, velocity)`, `note_off()`, `begin_steal(note, velocity)`, `render(out: np.ndarray, p: dict[str, float], mods: dict[str, float]) -> None`, `is_active() -> bool`, `note: int`, `age: int`, `velocity: float`, `env2_level: float` (el engine los usa para resolver la matriz por voz).

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_voice.py
import numpy as np
from nomad.engine.voice import Voice
from nomad.engine.params import REGISTRY

SR = 48000.0

def params(**over):
    p = {k: v.default for k, v in REGISTRY.items()}
    p.update(over); return p

def mods(**over):
    d = {k: 0.0 for k in ("pitch","pitch1","pitch2","amp","cutoff","pw1","pw2","bend")}
    d.update(over); return d

def render_voice(v, n, p, m=None):
    out = np.zeros(n)
    for i in range(0, n, 16):
        v.render(out[i:i+16], p, m or mods())
    return out

def test_silent_until_note_on():
    v = Voice(SR, 0)
    assert np.allclose(render_voice(v, 512, params()), 0.0)

def test_note_produces_bounded_audio():
    v = Voice(SR, 0); v.note_on(60, 0.8)
    out = render_voice(v, 24000, params(amp_attack=1.0, amp_decay=50.0, amp_sustain=0.8))
    assert np.abs(out).max() > 0.01
    assert np.abs(out).max() <= 1.0
    assert np.isfinite(out).all()

def test_two_oscillators_beat_when_detuned():
    # 10 cents de detune a 220 Hz => batido de ~1.27 Hz: la envolvente sube y baja
    v = Voice(SR, 0); v.note_on(57, 1.0)
    p = params(osc1_wave=0, osc2_wave=0, osc1_level=0.5, osc2_level=0.5,
               osc2_fine=10.0, amp_attack=1.0, amp_sustain=1.0, filt_cutoff=18000.0)
    out = render_voice(v, 144000, p)          # 3 s: varios ciclos de batido
    env = np.abs(out[10000:]).reshape(-1, 480).max(axis=1)
    assert env.std() > 0.01                      # hay batido
    assert env.mean() > 0.01

def test_amp_env_silences_voice_after_release():
    v = Voice(SR, 0); v.note_on(60, 1.0)
    render_voice(v, 4096, params(amp_release=20.0))
    v.note_off()
    out = render_voice(v, 24000, params(amp_release=20.0))
    assert out[-100:].max() < 1e-3
    assert not v.is_active()
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_voice.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`render` se llama **una vez por bloque de control** (≤256 frames). Por llamada: calcular pitch de cada osc (track: nota MIDI + coarse + fine + mods; free: `osc_hz` + fine), renderizar ambos osciladores con `level`, sumar, y después pasar la señal por el filtro **subdividida en granos** de 16 muestras si `k > 3.2` y de 64 si no, con cutoff = `filt_cutoff · 2^(filt_env_amt · 5 · env2 + mods.cutoff · 2)` recortado a 20..18000 y recalculado por grano. Finalmente aplicar `amp_env` y `mods.amp` sobre el bloque completo y sumar en `out`.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_voice.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/voice.py tests/test_voice.py
git commit -S -m "feat(engine): add polyphonic voice with two oscillators and two envelopes"
```

---

### Task 9: Voice manager

**Files:**
- Create: `nomad/engine/voice_manager.py`, `tests/test_voice_manager.py`

**Interfaces:**
- Consumes: `Voice`.
- Produces: `VoiceManager(sample_rate, max_voices=8)` con `set_polyphony(n)`, `note_on(note, velocity)`, `note_off(note)`, `render(out, p, mods)`, `active_notes() -> list[int]`, `STEAL_FADE_SEC = 0.005`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_voice_manager.py
import numpy as np
from nomad.engine.voice_manager import VoiceManager
from nomad.engine.params import REGISTRY

SR = 48000.0

def params(**over):
    p = {k: v.default for k, v in REGISTRY.items()}; p.update(over); return p

def mods():
    return {k: 0.0 for k in ("pitch","pitch1","pitch2","amp","cutoff","pw1","pw2","bend")}

def render(vm, n, p=None):
    out = np.zeros(n); p = p or params()
    for i in range(0, n, 16):
        vm.render(out[i:i+16], p, mods())
    return out

def test_eight_notes_play_together():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 4096)
    assert sorted(vm.active_notes()) == list(range(60, 68))

def test_ninth_note_steals_the_oldest():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 4096)
    vm.note_on(80, 1.0)
    render(vm, 4096)
    assert 60 not in vm.active_notes() and 80 in vm.active_notes()

def test_steal_has_no_step_discontinuity():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 8192)
    out = np.zeros(2400)   # 50 ms: el fade de 5 ms entra acá
    p = params(osc1_wave=0, osc2_level=0.0, amp_attack=1.0, amp_sustain=1.0)
    for i in range(0, len(out), 16):
        if i == 480: vm.note_on(80, 1.0)
        vm.render(out[i:i+16], p, mods())
    assert np.abs(np.diff(out)).max() < 0.25

def test_polyphony_one_keeps_one_note():
    vm = VoiceManager(SR); vm.set_polyphony(1)
    for n in (60, 64, 67): vm.note_on(n, 1.0)
    render(vm, 4096)
    assert len(vm.active_notes()) == 1

def test_lowering_polyphony_with_chord_releases_extras():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 8192)
    vm.set_polyphony(2)
    render(vm, 8192)
    assert len(vm.active_notes()) <= 2

def test_note_off_of_unknown_note_is_safe():
    vm = VoiceManager(SR); vm.note_off(999); render(vm, 256)
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_voice_manager.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

Criterio de robo: libre → la más vieja en release → la más vieja. `note_on` de una nota ya sonando reutiliza esa voz (retrigger) en vez de asignar otra. `age` se incrementa por voz en cada `note_on`. El fade de robo vive en `Voice.begin_steal`: la voz baja a cero en 5 ms y después dispara el note-on nuevo.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_voice_manager.py -v`
Expected: 6 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/voice_manager.py tests/test_voice_manager.py
git commit -S -m "feat(engine): add voice manager with stealing and smooth fades"
```

---

### Task 10: Fachada del engine

**Files:**
- Create: `nomad/engine/engine.py`, `tests/test_engine.py`

**Interfaces:**
- Consumes: `VoiceManager`, `ModMatrix`, `LFO`, `REGISTRY`, `Smoother`.
- Produces: `Engine(sample_rate=48000, max_voices=8)` con `note_on(note, velocity)`, `note_off(note)`, `set_param(name, value)`, `set_control(name, value)` (acepta `"bend"` y `"modwheel"`, ambos −1..1), `render(out: np.ndarray)`, `panic()`, `snapshot() -> dict`, `GRAIN = 16`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_engine.py
import numpy as np
from nomad.engine.engine import Engine

SR = 48000.0

def test_quantum_agnostic_render():
    def render(e, sizes):
        out = np.zeros(sum(sizes)); i = 0
        for n in sizes:
            e.render(out[i:i+n]); i += n
        return out
    def make():
        e = Engine(SR)
        e.set_param("amp_attack", 1.0); e.set_param("filt_cutoff", 3000.0)
        e.note_on(60, 1.0)
        return e
    # el engine trabaja en bloques de control de 256: 2048 de una vez == 8×256
    a = render(make(), [2048])
    b = render(make(), [256] * 8)
    assert np.allclose(a, b, atol=1e-6)
    # tamaños arbitrarios: finitos y sin discontinuidad en los bordes
    c = render(make(), [37, 1, 128, 500, 88, 1294])
    assert np.isfinite(c).all()
    assert np.abs(np.diff(c)).max() < 0.2

def test_render_does_not_allocate(monkeypatch):
    e = Engine(SR); e.note_on(60, 1.0)
    out = np.zeros(256)
    e.render(out)                      # warm-up
    calls = {"n": 0}
    for name in ("empty", "zeros", "ones", "full", "arange", "linspace"):
        real = getattr(np, name)
        def counted(*a, __real=real, **k):
            calls["n"] += 1; return __real(*a, **k)
        monkeypatch.setattr(np, name, counted)
    for _ in range(20): e.render(out)
    assert calls["n"] == 0

def test_eight_voices_stay_below_full_scale():
    e = Engine(SR); e.set_param("master_volume", 1.0)
    for n in range(48, 56): e.note_on(n, 1.0)
    out = np.zeros(SR); 
    for i in range(0, len(out), 256): e.render(out[i:i+256])
    assert np.abs(out).max() <= 1.0 and np.isfinite(out).all()

def test_extreme_params_stay_finite():
    e = Engine(SR)
    e.set_param("filt_cutoff", 20.0); e.set_param("filt_reso", 1.0)
    e.set_param("amp_attack", 1.0); e.set_param("amp_release", 8000.0)
    for n in (36, 48, 60, 72, 84): e.note_on(n, 1.0)
    out = np.zeros(SR)
    for i in range(0, len(out), 256): e.render(out[i:i+256])
    assert np.isfinite(out).all() and np.abs(out).max() <= 1.0

def test_panic_silences_within_50ms():
    e = Engine(SR); e.note_on(60, 1.0)
    out = np.zeros(4096); e.render(out)
    e.panic()
    tail = np.zeros(2400)
    for i in range(0, len(tail), 256): e.render(tail[i:i+256])
    assert np.abs(tail[-256:]).max() < 1e-3

def test_pitch_bend_shifts_pitch():
    def crossings(e):
        out = np.zeros(SR)
        for i in range(0, len(out), 256): e.render(out[i:i+256])
        return int(np.sum(np.diff(np.signbit(out)) != 0))
    a = Engine(SR); b = Engine(SR)
    for e in (a, b):
        e.set_param("osc1_wave", 0); e.set_param("osc2_level", 0.0)
        e.set_param("filt_cutoff", 18000.0); e.note_on(57, 1.0)
    b.set_param("bend_range", 2.0); b.set_control("bend", 1.0)
    assert crossings(b) > crossings(a) * 1.08   # +2 semitonos ≈ +12%

def test_modwheel_reaches_matrix():
    def run(modwheel):
        e = Engine(SR)
        e.set_param("mod0_src", 3); e.set_param("mod0_dst", 4)
        e.set_param("mod0_amt", -1.0); e.set_param("amp_attack", 1.0)
        e.set_param("amp_sustain", 1.0); e.note_on(60, 1.0)
        if modwheel: e.set_control("modwheel", 1.0)
        out = np.zeros(SR)
        for i in range(0, len(out), 256): e.render(out[i:i+256])
        return np.abs(out[24000:]).max()
    loud, muted = run(False), run(True)
    assert loud > 0.01 and muted < loud * 0.2

def test_unknown_param_raises():
    import pytest
    with pytest.raises(KeyError): Engine(SR).set_param("nope", 1.0)
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_engine.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`note_on/note_off/set_param/set_control` empujan a `queue.SimpleQueue` (thread-safe, sin lock explícito). `render` drena la cola al inicio, después parte `len(out)` en granos de 16. Por grano: avanza los LFO globales (`seconds = 16/SR`) y actualiza los smoothers. Después, **por voz**, arma `sources = {"lfo1":..., "lfo2":..., "modwheel":..., "velocity": voice.velocity, "env2": voice.env2_level}`, resuelve la matriz y llama `voice.render` con ese `mods` más `mods["bend"] = self._bend * p["bend_range"]`. La matriz se resuelve por voz porque `velocity` y `env2` son por voz; los LFO y el modwheel son globales.

`set_param("polyphony", n)` llama `voice_manager.set_polyphony(int(n))` además de guardar el valor. Master: `tanh(x · master_volume)`. `panic()` encola un release de 10 ms en todas las voces.

Los LFO son globales: viven en el engine, no en la voz. `env2` para la matriz se toma de la voz más nueva (documentado; el destino CUTOFF por voz ya usa el ADSR de filtro propio de cada voz).

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_engine.py -v`
Expected: 8 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/engine/engine.py tests/test_engine.py
git commit -S -m "feat(engine): add engine facade with lock-free command queue"
```

---

### Task 11: Buffer del osciloscopio

**Files:**
- Create: `nomad/audio/__init__.py`, `nomad/audio/scope.py`, `tests/test_scope.py`

**Interfaces:**
- Produces: `ScopeBuffer(sample_rate: float, seconds: float = 2.0)` con `write(block: np.ndarray) -> None` (sin allocar) y `window(n: int) -> np.ndarray` (copia, orden cronológico).

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_scope.py
import numpy as np
from nomad.audio.scope import ScopeBuffer

def test_window_returns_latest_samples_in_order():
    s = ScopeBuffer(48000, seconds=0.01)   # 480 muestras de capacidad
    s.write(np.arange(100, dtype=float))
    s.write(np.arange(100, 200, dtype=float))
    w = s.window(50)
    assert np.array_equal(w, np.arange(150, 200, dtype=float))

def test_wraparound_is_ordered():
    s = ScopeBuffer(48000, seconds=0.01)
    s.write(np.arange(400, dtype=float))
    s.write(np.arange(400, 560, dtype=float))   # da la vuelta
    w = s.window(160)
    assert np.array_equal(w, np.arange(400, 560, dtype=float))

def test_write_does_not_allocate(monkeypatch):
    s = ScopeBuffer(48000, seconds=0.01)
    blk = np.zeros(256)
    s.write(blk)
    calls = {"n": 0}; real = np.empty
    monkeypatch.setattr(np, "empty", lambda *a, **k: calls.update(n=calls["n"]+1) or real(*a, **k))
    s.write(blk)
    assert calls["n"] == 0
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_scope.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

Ring preasignado; `write` copia con dos slice assignments (`np.copyto`), `window` arma la copia con `np.concatenate` (la GUI corre en otro hilo y puede allocar).

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_scope.py -v`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/audio/scope.py tests/test_scope.py
git commit -S -m "feat(audio): add allocation-free ring buffer for the oscilloscope"
```

---

### Task 12: Stream de audio

**Files:**
- Create: `nomad/audio/stream.py`, `tests/test_stream.py`

**Interfaces:**
- Produces: `list_output_devices() -> list[OutputDevice]` con `OutputDevice(index, name, hostapi, samplerate, channels)`; `AudioStream(engine, device_index=None, samplerate=48000, blocksize=256, scope=None)` con `start()`, `stop()`, `close()`, `xruns: int`, `callback(outdata, frames, time_info, status)`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_stream.py
import numpy as np
import pytest
from nomad.audio.stream import AudioStream, list_output_devices

def test_devices_have_indices_and_names():
    devs = list_output_devices()
    assert all(isinstance(d.index, int) and d.name for d in devs)

class FakeEngine:
    def __init__(self): self.calls = 0
    def render(self, out): out[:] = 0.1; self.calls += 1

def test_callback_renders_and_counts():
    s = AudioStream(FakeEngine())
    out = np.zeros((256, 2), dtype=np.float32)
    class T: pass
    s.callback(out, 256, T(), None)
    assert np.allclose(out[:, 0], 0.1) and np.allclose(out[:, 1], 0.1)

def test_late_callback_counts_as_xrun():
    s = AudioStream(FakeEngine())
    out = np.zeros((256, 2), dtype=np.float32)
    class T: 
        inputBufferAdcTime = 0.0; currentTime = 0.0
    s.callback(out, 256, T(), None)
    class Late:
        inputBufferAdcTime = 0.0; currentTime = 0.02   # 20 ms tarde
    s.callback(out, 256, Late(), None)
    assert s.xruns >= 1

def test_close_is_idempotent():
    s = AudioStream(FakeEngine()); s.close(); s.close()

def test_callback_feeds_scope_without_allocating():
    from nomad.audio.scope import ScopeBuffer
    sc = ScopeBuffer(48000, seconds=0.01)
    s = AudioStream(FakeEngine(), scope=sc)
    out = np.zeros((256, 2), dtype=np.float32)
    class T: inputBufferAdcTime = 0.0; currentTime = 0.0
    s.callback(out, 256, T(), None)
    assert np.allclose(sc.window(256), 0.1)
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_stream.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`callback` sólo: `engine.render(outdata[:, 0])`, copiar a canal 1, `scope.write(...)` si hay scope, medir `currentTime` contra el esperado (bloque/acumulado) para contar xruns. `start()` usa `sd.OutputStream(samplerate, blocksize, channels=2, dtype="float32", callback=...)`; si `device_index is None`, elige el primer dispositivo con 2+ canales de salida cuyo host API sea **`"JACK Audio Connection Kit"`** (verificado en esta máquina: el idx 6 es ese, a 48 kHz); si no hay JACK, el default del sistema, y si tampoco hay nada, `RuntimeError` con mensaje claro (sin traceback de PortAudio).

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_stream.py -v`
Expected: 5 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/audio/stream.py tests/test_stream.py
git commit -S -m "feat(audio): add sounddevice output stream with xrun detection"
```

---

### Task 13: Entrada MIDI y enrutado

**Files:**
- Create: `nomad/midi/__init__.py`, `nomad/midi/midi_input.py`, `tests/test_midi.py`

**Interfaces:**
- Produces: `MidiEvent(kind: str, note: int, velocity: float, cc: int, value: int, channel: int, port: str, bend: float)`; `parse_message(msg, port: str) -> MidiEvent | None` (puro); `MidiInput(on_event)` con `list_ports() -> list[str]`, `open(port_name=None)`, `close()`, `is_open: bool`; `MidiRouter(engine, on_cc=None)` con `handle(event: MidiEvent) -> None`.
- `kind` ∈ `"note_on" | "note_off" | "cc" | "bend"`.
- `MidiRouter` es lo que traduce eventos a llamadas al engine: notas, sustain (CC64), bend y modwheel (CC1). No conoce la GUI.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_midi.py
import mido
from nomad.midi.midi_input import MidiEvent, parse_message

def test_note_on_with_velocity_zero_is_note_off():
    e = parse_message(mido.Message("note_on", note=60, velocity=0), "p")
    assert e.kind == "note_off" and e.note == 60

def test_velocity_is_normalized():
    e = parse_message(mido.Message("note_on", note=60, velocity=127), "p")
    assert e.kind == "note_on" and e.velocity == 1.0

def test_cc_parsed():
    e = parse_message(mido.Message("control_change", control=64, value=127), "p")
    assert e.kind == "cc" and e.cc == 64 and e.value == 127

def test_pitch_bend_is_bipolar():
    up = parse_message(mido.Message("pitchwheel", pitch=8191), "p")
    down = parse_message(mido.Message("pitchwheel", pitch=-8192), "p")
    assert up.bend > 0.99 and down.bend < -0.99

def test_clock_and_others_are_ignored():
    assert parse_message(mido.Message("clock"), "p") is None

class FakeEngine:
    def __init__(self):
        self.notes_on = []; self.notes_off = []; self.controls = []
    def note_on(self, n, v): self.notes_on.append(n)
    def note_off(self, n): self.notes_off.append(n)
    def set_control(self, name, value): self.controls.append((name, value))

def ev(kind, note=0, cc=0, value=0, bend=0.0):
    return MidiEvent(kind, note, 0.8, cc, value, 0, "p", bend)

def test_router_plays_notes():
    from nomad.midi.midi_input import MidiRouter
    e = FakeEngine(); r = MidiRouter(e)
    r.handle(ev("note_on", note=60)); r.handle(ev("note_off", note=60))
    assert e.notes_on == [60] and e.notes_off == [60]

def test_sustain_holds_notes_until_pedal_release():
    from nomad.midi.midi_input import MidiRouter
    e = FakeEngine(); r = MidiRouter(e)
    r.handle(ev("cc", cc=64, value=127))      # pedal abajo
    r.handle(ev("note_on", note=60))
    r.handle(ev("note_off", note=60))
    assert e.notes_off == []                   # sigue sonando
    r.handle(ev("cc", cc=64, value=0))         # pedal arriba
    assert e.notes_off == [60]

def test_bend_and_modwheel_go_to_engine():
    from nomad.midi.midi_input import MidiRouter
    e = FakeEngine(); r = MidiRouter(e)
    r.handle(ev("bend", bend=1.0))
    r.handle(ev("cc", cc=1, value=127))
    assert ("bend", 1.0) in e.controls
    assert any(n == "modwheel" and v > 0.99 for n, v in e.controls)

def test_open_missing_port_raises_cleanly():
    import pytest
    from nomad.midi.midi_input import MidiInput
    m = MidiInput(lambda e: None)
    with pytest.raises(RuntimeError):
        m.open("no-existe")
    m.close()                                  # no explota
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_midi.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`parse_message` puro (sin hilo, sin rtmidi). `MidiInput` abre con `mido.open_input(name, callback=...)`; si el puerto no existe, `open` levanta `RuntimeError` con la lista disponible. `close()` es idempotente y no explota si nunca se abrió ni si el puerto desapareció.

`MidiRouter.handle`: note on/off derecho; con sustain abajo, los note-off quedan pendientes y se emiten al soltar el pedal (una nota re-tocada en el medio cancela su pendiente); CC1 → `set_control("modwheel", value/127*2-1)`; CC64 → sustain; bend → `set_control("bend", bend)`; cualquier otro CC se pasa a `on_cc` si existe (lo usa el learn).

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_midi.py -v`
Expected: 9 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/midi/midi_input.py tests/test_midi.py
git commit -S -m "feat(midi): parse note, cc and pitch bend messages"
```

---

### Task 14: MIDI learn y persistencia

**Files:**
- Create: `nomad/midi/mapping.py`, `tests/test_mapping.py`

**Interfaces:**
- Consumes: `MidiEvent`, `REGISTRY`.
- Produces: `MidiMap(path: Path | None = None)` con `arm(param: str)`, `handle(event: MidiEvent) -> tuple[str, float] | None`, `bindings: dict[tuple[str,int,int], str]`, `save()`, `load()`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_mapping.py
import json
from pathlib import Path
from nomad.midi.mapping import MidiMap
from nomad.midi.midi_input import MidiEvent

def cc(control, value, port="keys", channel=0):
    return MidiEvent("cc", 0, 0.0, control, value, channel, port, 0.0)

def test_learn_binds_next_cc_to_armed_param(tmp_path):
    from nomad.engine.params import REGISTRY
    m = MidiMap(tmp_path / "map.json")
    m.arm("filt_cutoff")
    out = m.handle(cc(74, 64))
    assert out is not None
    name, value = out
    spec = REGISTRY["filt_cutoff"]
    assert name == "filt_cutoff" and spec.lo <= value <= spec.hi
    assert m.bindings[("keys", 0, 74)] == "filt_cutoff"

def test_learn_disarms_after_one_cc(tmp_path):
    m = MidiMap(tmp_path / "map.json"); m.arm("filt_cutoff")
    m.handle(cc(74, 10)); m.handle(cc(75, 10))
    assert ("keys", 0, 75) not in m.bindings

def test_saved_map_reloads(tmp_path):
    p = tmp_path / "map.json"
    m = MidiMap(p); m.arm("filt_reso"); m.handle(cc(71, 0)); m.save()
    m2 = MidiMap(p); m2.load()
    assert m2.bindings[("keys", 0, 71)] == "filt_reso"
    assert json.loads(p.read_text())["version"] == 1

def test_fallback_matches_same_channel_and_cc_on_other_port(tmp_path):
    m = MidiMap(tmp_path / "map.json"); m.arm("filt_reso"); m.handle(cc(71, 0))
    out = m.handle(cc(71, 100, port="otro"))
    assert out[0] == "filt_reso"

def test_cc_on_unknown_control_does_nothing(tmp_path):
    assert MidiMap(tmp_path / "m.json").handle(cc(3, 10)) is None
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_mapping.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

El valor de `handle` es `value/127` normalizado en el rango del parámetro (usa `from_norm`). El JSON guarda `{"version": 1, "bindings": [{"port":..., "channel":..., "cc":..., "param":...}]}`. `save` crea el directorio si no existe; `load` ignora entradas con parámetros desconocidos en vez de romper.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_mapping.py -v`
Expected: 5 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/midi/mapping.py tests/test_mapping.py
git commit -S -m "feat(midi): add midi learn with persistent port-aware bindings"
```

---

### Task 15: Perilla y selectores

**Files:**
- Create: `nomad/gui/__init__.py`, `nomad/gui/knob.py`, `nomad/gui/selector.py`, `tests/test_knob.py`

**Interfaces:**
- Produces: `KnobModel(spec: ParamSpec, value: float)` con `.value`, `.norm` (propiedad con setter), `set_from_drag(pixels: int, fine: bool)`, `set_from_wheel(delta: int)`, `reset()`; `Knob(parent, model, on_change)` (Canvas, dibuja arco + aguja + valor); `Selector(parent, labels, index, on_change)`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_knob.py
import pytest
from nomad.engine.params import REGISTRY
from nomad.gui.knob import KnobModel

def test_drag_moves_norm_and_clamps():
    k = KnobModel(REGISTRY["filt_cutoff"], 1000.0)
    k.set_from_drag(1000, fine=False)
    assert k.norm == 1.0
    k.set_from_drag(-5000, fine=False)
    assert k.norm == 0.0

def test_fine_drag_is_slower():
    a = KnobModel(REGISTRY["filt_cutoff"], 1000.0); a.set_from_drag(100, fine=False)
    b = KnobModel(REGISTRY["filt_cutoff"], 1000.0); b.set_from_drag(100, fine=True)
    assert b.norm < a.norm

def test_value_respects_curve():
    k = KnobModel(REGISTRY["filt_cutoff"], 1000.0)
    k.norm = 0.5
    assert 100.0 < k.value < 2000.0   # exp: 0.5 no es la media aritmética

def test_reset_returns_default():
    k = KnobModel(REGISTRY["amp_sustain"], 0.1); k.reset()
    assert k.value == pytest.approx(REGISTRY["amp_sustain"].default)
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_knob.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`KnobModel` es lógica pura (sin tkinter) para poder testearla. 200 px de arrastre = recorrido completo; con `fine` (shift) = 10x más fino. La perilla dibuja: círculo, arco de valor, aguja, nombre chico y valor con unidad; se redibuja sólo al cambiar. Doble click = default.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_knob.py -v`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add nomad/gui/knob.py nomad/gui/selector.py tests/test_knob.py
git commit -S -m "feat(gui): add continuous rotary knob and waveform selector"
```

---

### Task 16: Panel

**Files:**
- Create: `nomad/gui/app.py`, `tests/test_layout.py`

**Interfaces:**
- Consumes: `REGISTRY`, `Knob`, `Selector`, `Engine`, `AudioStream`.
- Produces: `LAYOUT: dict[str, list[str]]`, `HIDDEN: set[str]`, `build_app(engine, stream=None, midi_map=None) -> App` con `App.root`, `App.close()`. Si `stream` tiene `scope`, el panel dibuja ese buffer; si no hay stream, dibuja un `ScopeBuffer` propio que nadie escribe. Si hay `midi_map`, el botón `LEARN` lo arma.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_layout.py
from nomad.gui.app import LAYOUT, HIDDEN
from nomad.engine.params import REGISTRY

def test_every_parameter_is_reachable():
    shown = {p for params in LAYOUT.values() for p in params}
    assert set(REGISTRY) - HIDDEN <= shown

def test_no_parameter_appears_twice():
    seen = [p for params in LAYOUT.values() for p in params]
    assert len(seen) == len(set(seen))

def test_layout_names_are_real_parameters():
    assert all(p in REGISTRY for p in {p for v in LAYOUT.values() for p in v})
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_layout.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`LAYOUT` declara las secciones `osc1`, `osc2`, `filter`, `mixer`, `amp_env`, `filt_env`, `lfo1`, `lfo2`, `matrix`, `global`. `HIDDEN = {"bend_range"}`. `build_app` arma una grilla de 1366×768: header (título, dispositivo de audio, puerto MIDI, voces, xruns), tres columnas, scope a la derecha, teclado abajo. Cada cambio de perilla llama `engine.set_param`. Un botón `LEARN` arma el parámetro de la última perilla tocada.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_layout.py -v`
Expected: 3 passed

- [ ] **Step 5: Ver la ventana**

Run: `.venv/bin/python -c "from nomad.gui.app import build_app; from nomad.engine.engine import Engine; a=build_app(Engine(48000)); a.root.after(3000, a.close); a.root.mainloop()"`
Expected: aparece el panel completo, sin traceback, y se cierra solo a los 3 s.

- [ ] **Step 6: Commit**

```bash
git add nomad/gui/app.py tests/test_layout.py
git commit -S -m "feat(gui): add instrument panel layout with full parameter coverage"
```

---

### Task 17: Osciloscopio y teclado en pantalla

**Files:**
- Create: `nomad/gui/scope_view.py`, `nomad/gui/keyboard.py`, `tests/test_scope_view.py`
- Modify: `nomad/gui/app.py` (montar el scope y el teclado, y cablear el teclado a `engine.note_on/note_off`)

**Interfaces:**
- Consumes: `ScopeBuffer`.
- Produces: `find_trigger(buf: np.ndarray) -> int` (índice del último cruce por cero ascendente, 0 si no hay); `ScopeView(parent, buffer, width, height)` con `set_time_ms(ms)`, `redraw()`; `Keyboard(parent, on_note_on, on_note_off, octaves=2)` con `note_at(x: int) -> int | None`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_scope_view.py
import numpy as np
from nomad.gui.scope_view import find_trigger

def test_trigger_finds_rising_crossing():
    buf = np.sin(2*np.pi*np.arange(1000)/100.0)
    t = find_trigger(buf)
    assert buf[t-1] < 0 <= buf[t]
    assert t > 700   # el último cruce, no el primero

def test_trigger_on_flat_signal_returns_zero():
    assert find_trigger(np.zeros(100)) == 0
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_scope_view.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`find_trigger` usa `np.diff(np.signbit(buf))` invertido y toma el último `True`. `ScopeView` dibuja una `create_line` por frame (`root.after(33, ...)`) leyendo `buffer.window(ms*SR/1000)`; el ancho del Canvas se reescala al alto. `Keyboard` dibuja dos octavas, resalta notas activas y manda note on/off con velocity 0.8.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_scope_view.py -v`
Expected: 2 passed

- [ ] **Step 5: Ver el scope con audio real**

Run:

```bash
.venv/bin/python -c "
from nomad.engine.engine import Engine
from nomad.audio.stream import AudioStream
from nomad.audio.scope import ScopeBuffer
from nomad.gui.app import build_app
e = Engine(48000); sc = ScopeBuffer(48000)
s = AudioStream(e, scope=sc); s.start(); e.note_on(60, 0.9)
a = build_app(e, s); a.root.after(4000, a.close); a.root.mainloop()
"
```

Expected: suena un La, el scope muestra la onda real, el trigger la deja quieta al cambiar la escala de tiempo.

- [ ] **Step 6: Commit**

```bash
git add nomad/gui/scope_view.py nomad/gui/keyboard.py tests/test_scope_view.py
git commit -S -m "feat(gui): add real-signal oscilloscope with trigger and on-screen keyboard"
```

---

### Task 18: main.py, cierre limpio y render offline

**Files:**
- Create: `nomad/main.py`, `nomad/render.py`, `tests/test_main.py`

**Interfaces:**
- Consumes: todo lo anterior.
- Produces: `build_runtime(argv) -> Runtime` con `Runtime.engine`, `Runtime.start()`, `Runtime.close()`, `Runtime.closed: bool`; `python -m nomad.main [--no-midi] [--device N] [--demo-note N] [--dry-run]`; `python -m nomad.render --demo <name> out.wav`.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_main.py
import wave
import numpy as np
from nomad.main import build_runtime

def test_runtime_builds_without_midi():
    rt = build_runtime(["--no-midi", "--dry-run"])
    assert rt.engine is not None

def test_close_with_notes_held_is_clean():
    rt = build_runtime(["--no-midi", "--dry-run"])
    for n in range(60, 68): rt.engine.note_on(n, 1.0)
    rt.close(); rt.close()          # idempotente
    assert rt.closed

def test_render_writes_valid_wav(tmp_path):
    from nomad.render import render_demo
    p = tmp_path / "out.wav"
    render_demo("chord", p, seconds=1.0)
    with wave.open(str(p)) as w:
        assert w.getnchannels() == 1 and w.getframerate() == 48000
        assert w.getnframes() > 40000
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_main.py -v`
Expected: FAIL `ModuleNotFoundError`

- [ ] **Step 3: Implementar**

`--dry-run` no abre stream ni ventana (lo usan los tests). `Runtime.close()`: para el stream, cierra MIDI, cancela el `after` de la GUI y destruye la ventana; idempotente. Sin puerto MIDI disponible no es error: se avisa en el header y se sigue. `render_demo` renderiza offline con el mismo `Engine` y escribe WAV mono 48 kHz float32.

- [ ] **Step 4: Correr y ver que pasa**

Run: `pytest tests/test_main.py -v`
Expected: 3 passed

- [ ] **Step 5: Arrancar la app de verdad**

Run: `.venv/bin/python -m nomad.main`
Expected: abre el panel, el header muestra el dispositivo JACK y "MIDI: —"; al tocar el teclado en pantalla suena y el scope se mueve; al cerrar la ventana no queda proceso vivo (`ps` sin `python -m nomad.main`).

- [ ] **Step 6: Commit**

```bash
git add nomad/main.py nomad/render.py tests/test_main.py
git commit -S -m "feat: add entrypoint with clean shutdown and offline render harness"
```

---

### Task 19: Verificación final

**Files:**
- Create: `README.md`, `tests/test_end_to_end.py`

**Interfaces:**
- Consumes: todo.

- [ ] **Step 1: Escribir el test que falla**

```python
# tests/test_end_to_end.py
import time
import numpy as np
from nomad.engine.engine import Engine

def test_five_seconds_of_eight_voices_faster_than_realtime():
    e = Engine(48000)
    e.set_param("filt_cutoff", 800.0); e.set_param("filt_reso", 0.7)
    e.set_param("filt_env_amt", 0.6); e.set_param("lfo1_depth", 0.4)
    e.set_param("lfo1_rate", 5.0)
    e.set_param("mod0_src", 1); e.set_param("mod0_dst", 5); e.set_param("mod0_amt", 0.5)
    for n in (48, 52, 55, 60, 64, 67, 71, 72): e.note_on(n, 0.9)
    out = np.zeros(48000 * 5)
    t0 = time.perf_counter()
    for i in range(0, len(out), 256): e.render(out[i:i+256])
    elapsed = time.perf_counter() - t0
    assert np.isfinite(out).all()
    assert np.abs(out).max() <= 1.0
    assert elapsed < 5.0 * 0.5          # menos de la mitad del tiempo real
```

- [ ] **Step 2: Correr y ver que falla**

Run: `pytest tests/test_end_to_end.py -v`
Expected: PASS (todo existe) o FAIL si la carga excede el 50%; si falla, optimizar antes de seguir.

- [ ] **Step 3: Escribir el README**

Qué es, cómo instalar (`pip install -r requirements.txt`, aviso de que python-rtmidi compila), cómo correr, atajos de la GUI, cómo seleccionar dispositivo y puerto MIDI, y el estado de la autoscilación (rasposa a propósito).

- [ ] **Step 4: Correr toda la suite**

Run: `pytest -v`
Expected: todos los tests en verde.

- [ ] **Step 5: Escuchar**

Run: `.venv/bin/python -m nomad.render --demo chord /tmp/opencode/nomad-chord.wav`
Expected: WAV que suena a acorde con filtro; sin clicks en el ataque ni en el release.

- [ ] **Step 6: Commit**

```bash
git add README.md tests/test_end_to_end.py
git commit -S -m "test: add end-to-end performance check and document the instrument"
```
