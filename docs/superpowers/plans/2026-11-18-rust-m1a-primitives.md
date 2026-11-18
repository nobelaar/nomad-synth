# NOMAD en Rust, M1-A (primitivas de audio) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Portar a `nomad-core` el oscilador (con sync), la envolvente, el filtro ladder, el LFO (con el generador de numpy) y la matriz de modulación, cada uno verificado contra audio de referencia exportado de Python.

**Architecture:** Cada pieza de Python (`nomad/engine/*.py`) **es la especificación**: se porta con las mismas fórmulas. Un exportador por componente (`tools/ref_*.py`) corre escenarios en Python y guarda las salidas como binarios `f64` + un manifiesto JSON; los tests de Rust los cargan y comparan. Los tests de Python detectan referencias viejas regenerando en un directorio temporal.

**Tech Stack:** Rust 1.100 (edición 2024), `serde_json` (sólo dev), pytest, numpy (sólo para generar referencias).

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M1, primer tramo). Siguen: M1-B (voz, gestor de voces, glide), M1-C (reloj, secuenciador), M1-D (batería, mezclador, topología).

**Branch:** todo el port va en `feature/rust-m0` (decisión del autor), un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/`. Lo nuevo de Python vive en `tools/` y `tests/`.
- `f64` en todo el DSP. Tolerancia contra la referencia: `|a-b| <= 1e-9 + 1e-9*|b|` por muestra (los `*.f64` se escriben con `<f8` little-endian).
- Sin asignaciones ni bloqueos en `process`/`render_into`/`advance`/`resolve`: todo scratch se reserva en `new` (para bloques de hasta `MAX_BLOCK = 2048` muestras; un bloque mayor puede crecer una vez, documentado).
- Sin `unsafe`, sin dependencias nuevas en `[dependencies]` de `nomad-core`.
- Entradas no finitas o fuera de rango se tratan igual que en Python (no panics en el camino de audio).
- Los escenarios exportados son deterministas y chicos (cada `.f64` < 200 KB).
- Rust: `export PATH="$HOME/.cargo/bin:$PATH"`. Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` y `cargo test --workspace` en verde.

## Review Focus

1. Frecuencia 0, `NaN`, negativa y por encima de Nyquist en el oscilador: sin `NaN`, `|f|` acotado a `0.499*sr` (Task 2).
2. `pw` = `NaN`, 0 y 1 en el cuadrado: se acota a [0.05, 0.95] o 0.5 (Task 2).
3. Continuidad entre bloques: renderizar `2N` en un bloque o en dos de `N` da lo mismo, y un wrap del master justo en el límite de bloque se detecta en el sync (Tasks 2 y 3).
4. Envolvente: empujar los mismos parámetros en cada bloque no reinicia el ataque; reataque y release arrancan desde el valor actual; tiempos `NaN`/0 se acotan a 1 ms (Task 4).
5. Filtro a resonancia 1.0 y `fc` o `res` `NaN`: se mantiene acotado (límite de estado) y sin `NaN` (Task 5).
6. LFO con `rate`/`depth`/`seconds` no finitos y onda inválida: finito, `|v| <= depth`, onda inválida no cambia el estado (Task 6).
7. Matriz: `amount` `NaN` pasa a 0, fuera de [-1, 1] se acota, índices inválidos se rechazan sin tocar el estado (Task 7).

---

### Task 1: Infraestructura de referencias binarias

**Files:**
- Create: `tools/refio.py`, `tests/test_refio.py`
- Modify: `rust/nomad-core/tests/common/mod.rs`
- Test: `rust/nomad-core/tests/refio.rs`

**Interfaces:**
- Produces (Python, `tools/refio.py`): `class BlobWriter(name: str)` con `add(label: str, data: np.ndarray, **meta) -> None` y `write(out_dir: Path) -> None`, que escribe `<out_dir>/<name>.f64` (todas las muestras concatenadas, `<f8`) y `<out_dir>/<name>.json` con `{"entries": {label: {"offset": int, "len": int, "meta": {...}}}}` (offset y len en muestras, orden de inserción); `def regenerate_matches(write_fn, ref_dir: Path, tmp: Path) -> list[str]` que llama `write_fn(tmp)` y devuelve los nombres de archivo que difieren byte a byte de `ref_dir` (lista vacía = al día).
- Produces (Rust, `tests/common/mod.rs`, ya existente con el loader de params): `pub struct Blob`; `Blob::open(name: &str) -> Blob` (lee `rust/reference/<name>.f64/.json`), `blob.get(label: &str) -> &[f64]`, `blob.meta(label: &str) -> &serde_json::Value`, `blob.labels() -> Vec<String>`; `pub fn assert_close(got: &[f64], want: &[f64], what: &str)` con la tolerancia global (mensaje con índice y valores).

- [ ] **Step 1: Write the failing tests.**
  - `tests/test_refio.py::test_blob_roundtrip`: escribir un `BlobWriter("t")` con dos entradas (`np.arange(5.0)` con `meta={"k": 1}` y `np.array([0.5])`) en `tmp_path`; releer el `.f64` con `np.fromfile("<f8")` y el JSON; `entries["a"] == {"offset":0,"len":5,"meta":{"k":1}}`, `entries["b"]["offset"] == 5`.
  - `tests/test_refio.py::test_regenerate_matches_detects_diff`: `regenerate_matches` devuelve `[]` si coincide y `["t.f64"]` si se cambia un byte.
  - `rust/nomad-core/tests/refio.rs::blob_loads_python_fixture`: carga `rust/reference/refio_selftest.{f64,json}` (generado en Step 3 por `tools/refio.py` con entradas `"ramp"` = `0..8` y `"half"` = `[0.5]`) y comprueba `get("ramp") == [0.0..7.0]`, `meta("ramp")["tag"] == "x"`, `labels() == ["ramp","half"]`; y `assert_close` falla (usar `#[should_panic]`) con un valor fuera de 1e-9.
- [ ] **Step 2: Run** `python -m pytest tests/test_refio.py -v` y `cd rust && cargo test -p nomad-core --test refio`. Expected: FAIL.
- [ ] **Step 3: Implement** lo declarado arriba. Generar `rust/reference/refio_selftest.*` con un `main()` en `tools/refio.py` (`python tools/refio.py rust/reference`).
- [ ] **Step 4: Run** ambos comandos más `cargo fmt/clippy`. Expected: PASS.
- [ ] **Step 5: Commit** `git add tools tests rust && git commit -m "feat(rust): binary reference blobs and loader"`.

### Task 2: Oscilador libre (seno, triángulo, sierra, cuadrada)

**Files:**
- Create: `tools/ref_oscillator.py`, `rust/nomad-core/src/oscillator.rs`
- Modify: `tools/export_reference.py` (llamar a `ref_oscillator.write`), `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_oscillator.py`, `rust/nomad-core/tests/oscillator.rs`

**Interfaces:**
- Consumes: `Blob`, `assert_close` (Task 1).
- Produces (Rust, `nomad_core::oscillator`): `pub const MAX_INC: f64 = 0.499; pub const MAX_BLOCK: usize = 2048;` `#[derive(Clone, Copy, PartialEq, Debug)] pub enum Wave { Sine, Triangle, Saw, Square }` con `pub fn from_index(i: i64) -> Option<Wave>` (0..=3); `pub struct Oscillator { pub wave: Wave, pub phase: f64, /* privados: sample_rate, scratch */ }`; `Oscillator::new(sample_rate: f64) -> Oscillator` (panic si `sample_rate <= 0`, sólo en construcción); `pub fn render_into(&mut self, out: &mut [f64], freq: f64, pw: f64, sync_master_phase: Option<&[f64]>) -> f64` que **suma** en `out`, avanza `phase` y la devuelve (en esta tarea `sync_master_phase` se ignora: `Some(_)` se implementa en Task 3, por ahora tratarlo como `None`). Las funciones auxiliares `blep` y `square` son privadas.
- Python: `ref_oscillator.write(out_dir: Path) -> None` escribe `oscillator.{f64,json}` con escenarios `"free/<wave>/<freq>/<pw>"`: `wave ∈ {0,1,2,3}`, `freq ∈ {55.0, 440.0, 5200.0, 12000.0, 21000.0, 0.0}`, `pw ∈ {0.5, 0.2}`; cada uno renderiza bloques `[256, 100, 1, 17]` contiguos (osc a `sample_rate=48000`, `phase=0`) y guarda la concatenación de las salidas (cada bloque parte de ceros) con `meta = {"wave","freq","pw","blocks":[...], "phase_end": float}`. Más `"edge/nan_freq"` (`freq=nan` → ceros + fase) , `"edge/neg_freq"` (`-440.0`), `"edge/pw_nan"`, `"edge/pw_0"`, `"edge/pw_1"` (saw irrelevante; cuadrada a 440 Hz, 256 muestras).

- [ ] **Step 1: Write the failing tests.**
  - Python `test_ref_oscillator.py::test_reference_is_up_to_date`: `regenerate_matches(ref_oscillator.write, Path("rust/reference"), tmp_path)` filtrado a `oscillator.*` es `[]`.
  - Rust `oscillator.rs`: `free_waves_match_python` (replica cada escenario `free/...` con los mismos bloques y compara salida con `assert_close` y `phase_end` con 1e-12); `edge_cases_match_python` (los 5 `edge/*`); `no_nan_for_any_wave_at_non_finite_params` (frecuencias `NaN, ±inf, 0, 1e9, -1e9` y `pw` `NaN, ±inf, -1, 2` en las 4 ondas: todas las muestras finitas y `|x| <= 2.5`); `block_split_is_invisible` (500 muestras en un bloque == bloques `[256, 244]` para cada onda a 5200 Hz, con 1e-12); `sine_matches_libm` (seno a 1000 Hz: `x[n] ≈ sin(2π·n·1000/48000)` con 1e-9); `render_adds_into_buffer` (con `out` inicial de unos, el resultado es `1 + referencia`).
- [ ] **Step 2: Run** `python -m pytest tests/test_ref_oscillator.py` y `cd rust && cargo test -p nomad-core --test oscillator`. Expected: FAIL.
- [ ] **Step 3: Implement** `Oscillator` portando `nomad/engine/oscillator.py` literalmente (`_blep`, `_square`, los cuatro `wave`, `inc = min(|f|, MAX_INC*sr)/sr`, `inc = 0` si no finita, `pw` acotado a [0.05, 0.95] y `NaN` → 0.5, fase = `(phase + n*inc) % 1.0`). Scratch (`ramp`, `t`) reservado en `new` para `MAX_BLOCK`. Generar con `python tools/export_reference.py rust/reference`.
- [ ] **Step 4: Run** los dos comandos + fmt/clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): free-running PolyBLEP oscillator"`.

### Task 3: Oscilador con hard sync

**Files:**
- Modify: `tools/ref_oscillator.py`, `rust/nomad-core/src/oscillator.rs`
- Test: `tests/test_ref_oscillator.py`, `rust/nomad-core/tests/oscillator_sync.rs`

**Interfaces:**
- Consumes: `Oscillator`, `Blob` (Tasks 1-2).
- Produces: `render_into(..., sync_master_phase: Some(&[f64]))` con la semántica de `_synced_ramp` y `_sync_correction` de Python. Estado interno nuevo `last_master: Option<f64>` (se pone en `None` cuando el oscilador corre libre o `inc <= 0`). Si `sync_master_phase.len() != out.len()` se devuelve la fase sin escribir nada (Python lanza `ValueError`; en Rust no hay panics en el camino de audio) y un `debug_assert!`.
- Python: escenarios `"sync/<wave>/<ratio>"`: esclavo `wave ∈ {0,1,2,3}` a 300 Hz, maestro a `300*ratio` Hz con `ratio ∈ {0.5, 1.37, 2.0, 3.9}` (fase del maestro = un `Oscillator` aparte cuya `phase` se lee con la rampa `phase + n*inc` por bloque, igual que `Voice.render`: `master = (phase + ramp*inc) % 1`), bloques `[64, 64, 100, 7, 256]`; `meta` con las frecuencias y `phase_end`. Más `"sync/boundary"`: maestro a exactamente 1 ciclo cada 64 muestras (3000 Hz a 48 kHz) con bloques de 64 (el wrap cae en el límite de bloque).

- [ ] **Step 1: Write the failing tests.** `sync_matches_python` (todos los `sync/*`, `assert_close` y fase final), `sync_boundary_wrap_detected` (la salida de `sync/boundary` coincide, y además la salida del primer muestra de cada bloque difiere de la del mismo oscilador sin sync), `sync_len_mismatch_writes_nothing` (con `out` de ceros y master de longitud distinta, `out` sigue en ceros), `free_run_clears_last_master` (tras un bloque sin sync, un sync posterior no usa el master viejo: igual que Python en un escenario `"sync/after_free"` que se agrega al exportador).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-core --test oscillator_sync`. Expected: FAIL.
- [ ] **Step 3: Implement** el camino de sync portando `_synced_ramp`, `_sync_correction`, `_value_at`, `_edge_at`, `_blep_at`; los índices de reset se acumulan en un buffer preasignado (`Vec<usize>`/`Vec<f64>` con capacidad `MAX_BLOCK`).
- [ ] **Step 4: Run** pytest + cargo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): oscillator hard sync with sub-sample resets"`.

### Task 4: Envolvente ADSR

**Files:**
- Create: `tools/ref_envelope.py`, `rust/nomad-core/src/envelope.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_envelope.py`, `rust/nomad-core/tests/envelope.rs`

**Interfaces:**
- Produces (Rust): `pub struct Envelope { pub value: f64, /* privados */ }`; `Envelope::new(sample_rate: f64) -> Envelope`; `set_params(&mut self, attack_s: f64, decay_s: f64, sustain: f64, release_s: f64)`; `note_on(&mut self)`; `note_off(&mut self)`; `is_idle(&self) -> bool`; `process(&mut self, out: &mut [f64])`. Constantes `MIN_TIME = 0.001`, sobrepaso 1.3, piso 1e-4, mismas que Python; el `env_run` de `_kernels.py` es la especificación del lazo.
- Python: `ref_envelope.write` escribe `envelope.{f64,json}`; cada escenario es una lista de comandos (`("params", a, d, s, r)`, `("on",)`, `("off",)`, `("run", n)`) y guarda la concatenación de los `run`; `meta` incluye la lista de comandos. Escenarios: `"adsr/basic"` (params 0.01, 0.1, 0.7, 0.2 a 48 kHz: on, run 2000, run 6000, off, run 20000), `"adsr/retrigger"` (on, run 300, off, run 200, on, run 4000), `"adsr/release_from_attack"` (on, run 100, off, run 3000), `"adsr/params_every_block"` (on y 60 bloques de 64 con `("params", 0.02, 0.1, 0.5, 0.2)` antes de cada `run 64`), `"adsr/times_clamped"` (params `nan, 0, -1, 0.2` y on, run 200), `"adsr/sustain_clamped"` (sustain `nan`, `2.0` y `-1.0`).

- [ ] **Step 1: Write the failing tests.** `scenarios_match_python` (cada escenario con `assert_close`, repitiendo los comandos de `meta`, y `is_idle()` esperado al final de `adsr/basic`: `true`), `params_every_block_does_not_restart_attack` (la salida de `params_every_block` es monótona creciente durante el ataque y llega a 1.0), `is_idle_before_note_on_and_after_release`, `output_stays_within_0_and_1`.
- [ ] **Step 2: Run** pytest y `cargo test -p nomad-core --test envelope`. Expected: FAIL.
- [ ] **Step 3: Implement** `Envelope` portando `envelope.py` y el lazo de `env_run`; `dcoef = 1e-3^(1/(decay*sr))` se recalcula en cada `process` como en Python.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): ADSR envelope"`.

### Task 5: Filtro ladder de 4 polos

**Files:**
- Create: `tools/ref_filter.py`, `rust/nomad-core/src/filter.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_filter.py`, `rust/nomad-core/tests/filter.rs`

**Interfaces:**
- Produces (Rust): `pub const K_MAX: f64 = 4.3; pub const MAX_GRAIN: usize = 64;` `pub fn ladder_coeffs(fc_hz: f64, k: f64, sample_rate: f64) -> ([f64; 5], [f64; 5])`; `pub struct Ladder4Pole { /* privados */ }`; `Ladder4Pole::new(sample_rate: f64) -> Ladder4Pole`; `set(&mut self, fc_hz: f64, resonance: f64)` (mismas reglas de acotado y de caché `(fc, r)` que Python; `NaN` en fc → `0.45*sr`, `NaN` en resonancia → 0); `reset(&mut self)`; `process(&mut self, buf: &mut [f64])`. **Desvío deliberado de Python:** no hay límite de largo de grano (Python lanza si `len > 64`; esa regla era del bucle de grano de Python), `MAX_GRAIN` queda como constante para el llamador. Recursión DF-II transpuesta y límite de estado `|zi| <= 0.8` cuando `k > 3.2`, como `ladder_run`.
- Python: `ref_filter.write` escribe `filter.{f64,json}`: `"coeffs/<fc>/<res>"` (concatena `b` y `a`, 10 valores) para `fc ∈ {50, 1000, 8000, 20000}`, `res ∈ {0.0, 0.5, 1.0}`; `"run/<fc>/<res>"` filtra 1024 muestras de ruido determinista (`np.random.default_rng(7).standard_normal(1024)*0.3`, guardado también como `"input/noise"`) en granos de `[64, 16, 64, ...]`; `"run/self_osc"` res=1.0 con un impulso (1.0 en la muestra 0) y 4096 muestras en granos de 16; `"set/nan"` (`set(nan, nan)` seguido de `process` del ruido); `"set/extremes"` (`fc=1.0` y `fc=40000`).

- [ ] **Step 1: Write the failing tests.** `coeffs_match_python`, `run_matches_python` (todos los `run/*` y `set/*`), `self_oscillation_stays_bounded` (res=1.0, impulso, 100000 muestras: todas finitas y `|y| < 50`), `long_buffer_equals_chunks` (un buffer de 1000 == 63 granos de 16 + resto, con 1e-12), `reset_clears_state`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test filter`. Expected: FAIL.
- [ ] **Step 3: Implement** `ladder_coeffs` (fórmula cerrada de `ladder_coeffs_into`), `Ladder4Pole` y `process`.
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): 4-pole ladder filter"`.

### Task 6: Generador de numpy y LFO

**Files:**
- Create: `tools/ref_lfo.py`, `rust/nomad-core/src/rng.rs`, `rust/nomad-core/src/lfo.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_lfo.py`, `rust/nomad-core/tests/rng.rs`, `rust/nomad-core/tests/lfo.rs`

**Interfaces:**
- Produces (Rust): `nomad_core::rng::NumpyRng` con `NumpyRng::new(seed: u64) -> NumpyRng` (equivale a `numpy.random.default_rng(seed)` para semillas enteras no negativas: `SeedSequence(seed)` → 4 palabras de 64 bits → estado e incremento de PCG64) y `random(&mut self) -> f64` (equivale a `Generator.random()`: `(next_u64() >> 11) * 2^-53`). `nomad_core::lfo::Lfo`: `Lfo::new(sample_rate: f64, seed: u64) -> Lfo`; `set(&mut self, rate_hz: f64, wave: i64, depth: f64, phase_offset: f64) -> Result<(), InvalidWave>` (`wave` fuera de 0..=4 → `Err` sin cambiar nada); `advance(&mut self, seconds: f64) -> f64`; `pub phase: f64`. `pub struct InvalidWave;` (`Debug`).
- Python: `ref_lfo.write` escribe `lfo.{f64,json}`: `"rng/<seed>"` con las primeras 1000 salidas de `default_rng(seed).random()` para `seed ∈ {0, 1, 2, 12345}`; `"lfo/<wave>"` (`wave ∈ 0..=4`) con rate 3.7 Hz, depth 0.8, offset 0.25, seed 1, 400 llamadas a `advance(0.005)`; `"lfo/seed2_sh"` onda 4 con seed 2, rate 11 Hz, 600 llamadas; `"lfo/nonfinite"` con `rate=nan`, `depth=inf`, `offset=nan` y `advance(nan)`, `advance(-1)`, `advance(0.01)`.

- [ ] **Step 1: Write the failing tests.** `rng.rs::first_1000_match_numpy` (las cuatro semillas, comparación **exacta**), `lfo.rs::waves_match_python` (`lfo/0..4` y `lfo/seed2_sh` con `assert_close`), `lfo.rs::nonfinite_inputs_are_finite_and_bounded` (todo finito y `|v| <= depth_efectivo`), `lfo.rs::invalid_wave_rejected_without_state_change`, `lfo.rs::sample_and_hold_changes_only_at_wrap`.
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test rng --test lfo`. Expected: FAIL.
- [ ] **Step 3: Implement** `NumpyRng`: `SeedSequence` (mezcla con `hash_const`/`mix` de numpy, `generate_state(4, uint64)`), luego `pcg64_srandom` con multiplicador 128-bit `0x2360ED051FC65DA44385DF649FCCF645` y salida XSL-RR. Si una semilla no coincide con la referencia, el test de `rng.rs` lo muestra por índice: arreglar `SeedSequence` hasta que las 4 semillas coincidan exacto. `Lfo` porta `lfo.py` (`_draw` usa `2*random() - 1`; el `_held` inicial se dibuja en `new`).
- [ ] **Step 4: Run** todo (+ fmt/clippy). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): numpy-compatible PCG64 and LFO"`.

### Task 7: Matriz de modulación

**Files:**
- Create: `tools/ref_modmatrix.py`, `rust/nomad-core/src/modmatrix.rs`
- Modify: `tools/export_reference.py`, `rust/nomad-core/src/lib.rs`
- Test: `tests/test_ref_modmatrix.py`, `rust/nomad-core/tests/modmatrix.rs`

**Interfaces:**
- Produces (Rust): `pub const NUM_SLOTS: usize = 8;` `#[derive(Clone, Copy, Default, Debug)] pub struct Sources { pub lfo1: f64, pub lfo2: f64, pub modwheel: f64, pub velocity: f64, pub env2: f64 }`; `#[derive(Clone, Copy, Default, Debug, PartialEq)] pub struct Destinations { pub pitch: f64, pub pitch1: f64, pub pitch2: f64, pub amp: f64, pub cutoff: f64, pub pw1: f64, pub pw2: f64 }`; `pub struct ModMatrix`; `ModMatrix::new()`; `set_slot(&mut self, i: usize, src: usize, dst: usize, amount: f64) -> Result<(), SlotError>` (`i` ≥ 8, `src` > 5 o `dst` > 7 → `Err`, sin tocar el estado; `amount` `NaN` → 0, resto acotado a [-1, 1]); `resolve(&mut self, sources: &Sources) -> &Destinations`. Índices: src `0 off, 1 lfo1, 2 lfo2, 3 modwheel, 4 velocity, 5 env2`; dst `0 off, 1 pitch, 2 pitch1, 3 pitch2, 4 amp, 5 cutoff, 6 pw1, 7 pw2`.
- Python: `ref_modmatrix.write` escribe `modmatrix.{f64,json}`: escenarios `"m/<n>"` con una configuración de slots (en `meta`: lista `[i, src, dst, amount]`) y un vector de fuentes `[lfo1, lfo2, modwheel, velocity, env2]`; la entrada guarda las 7 salidas en el orden de `Destinations`. Casos: vacío; un slot por destino; dos slots al mismo destino (suma); `amount` `nan` y `5.0`; `src=0` o `dst=0` (no rutean); fuentes con valores negativos.

- [ ] **Step 1: Write the failing tests.** `scenarios_match_python`, `invalid_slot_rejected_state_untouched` (un `set_slot` inválido no cambia `resolve`), `resolve_resets_between_calls` (dos `resolve` con fuentes distintas no acumulan).
- [ ] **Step 2: Run** pytest + `cargo test -p nomad-core --test modmatrix`. Expected: FAIL.
- [ ] **Step 3: Implement** `ModMatrix` portando `modmatrix.py` (los 8 slots en arrays fijos; `resolve` devuelve una referencia a un campo interno, sin asignar).
- [ ] **Step 4: Run** `cd rust && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` y `python -m pytest tests -q --ignore=tests/test_stream_instances.py`. Expected: todo PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(rust): modulation matrix"`.
