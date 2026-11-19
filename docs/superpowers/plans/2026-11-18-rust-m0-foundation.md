# NOMAD en Rust, M0 (esqueleto) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dejar listo el workspace de Rust con el registro de parámetros y el `Smoother` portados, verificados contra un exportador de referencia de Python, y con CI.

**Architecture:** Un workspace Cargo en `rust/` con la librería `nomad-core` (sin dependencias de audio ni de interfaz). Un script de Python (`tools/export_reference.py`) vuelca el registro de parámetros y trazas del `Smoother` a `rust/reference/params.json`; los tests de Rust cargan ese archivo y exigen coincidencia. Un test de Python detecta si el archivo quedó viejo respecto del Python.

**Tech Stack:** Rust 1.100 (edición 2024), `serde`/`serde_json` sólo como dev-dependencies, pytest, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M0). Los hitos M1 a M4 tendrán planes propios.

## Global Constraints

- El código de Rust vive en `rust/`; **no se modifica ningún archivo de `nomad/`** (la app de Python queda congelada).
- DSP en `f64` (igual que numpy). Tolerancia de comparación contra el oráculo: relativa 1e-12 para matemática pura (parámetros), 1e-6 para audio (hitos siguientes).
- Mismos nombres, rangos, curvas, unidades y orden que `nomad/engine/params.py` (63 parámetros de Synth, incluido `master_volume`) y `DRUM_REGISTRY` de `nomad/engine/drums.py` (20 parámetros).
- Cero asignaciones y cero bloqueos en el hilo de audio (aplica desde M1; el registro es `static`, sin heap).
- Rust se usa con `export PATH="$HOME/.cargo/bin:$PATH"`; edición 2024.

## Review Focus

1. `clamp` con `NaN`, `+inf` o `-inf` devuelve el valor por defecto (como Python), no el límite: lo fija un test en Task 2.
2. `to_norm` con valor fuera de rango y `from_norm` con `n` fuera de [0, 1] se acotan antes de calcular: Task 2.
3. Curva `Exp` en los extremos (`from_norm(1.0)` ≈ `hi`, `to_norm(lo)` = 0): Task 2, contra la referencia.
4. Si alguien cambia `params.py` y olvida regenerar la referencia, un test de Python falla: Task 1.
5. `Smoother::advance(0)` y con frames negativos devuelven el valor actual sin cambios, y `time_ms = 0` no produce `NaN` (el `tau` mínimo es `1e-9`, como en Python): Task 3.
6. Nombre desconocido: Rust devuelve `None` (Python lanza `KeyError`); la diferencia queda documentada en `find`: Task 2.

---

### Task 1: Exportador de referencia de parámetros (Python)

**Files:**
- Create: `tools/export_reference.py`
- Create: `rust/reference/params.json` (generado)
- Test: `tests/test_reference_export.py`

**Interfaces:**
- Produces: `build_params() -> dict` y `write(out_dir: pathlib.Path) -> None` en `tools/export_reference.py`; archivo `rust/reference/params.json` con este esquema (los Tasks 2 a 4 lo leen):
  - `"version": 1`
  - `"synth"`: lista, en el orden de `REGISTRY`, de objetos `{"name","default","lo","hi","curve" ("lin"|"exp"),"unit","smooth_ms","clamp_in":[5 floats],"clamp_out":[5],"to_norm_out":[5],"from_norm_in":[6],"from_norm_out":[6]}`. `clamp_in = [-1e9, lo, (lo+hi)/2, hi, 1e9]`; `from_norm_in = [-0.5, 0.0, 0.25, 0.5, 1.0, 1.5]`; los `*_out` salen de `clamp`, `to_norm`, `from_norm` de Python aplicados a esas entradas.
  - `"drums"`: lista, en el orden de `DRUM_REGISTRY`, con `{"name","default","lo","hi","curve","unit","smooth_ms","clamp_in","clamp_out"}`; `clamp_out` sale de `nomad.engine.drums._clamp(spec, v)`.
  - `"smoother"`: lista de `{"start","target","sample_rate","time_ms","steps":[ints],"values":[floats]}` con dos casos: `start=0.0, target=1.0, sample_rate=48000.0, time_ms=15.0, steps=[64, 64, 256, 512, 0]` y `start=0.25, target=0.75, sample_rate=44100.0, time_ms=0.0, steps=[1, 16, 0]`; `values[i]` es el valor devuelto por `Smoother.advance(steps[i])` tras un único `set(target)` previo. Valores no finitos no aparecen en el archivo.

- [ ] **Step 1: Write the failing tests** en `tests/test_reference_export.py`:
  - `test_reference_file_is_up_to_date`: `json.loads(Path("rust/reference/params.json").read_text()) == export_reference.build_params()` (importar con `from tools import export_reference`).
  - `test_reference_shape`: `len(build_params()["synth"]) == 63`, `len(...["drums"]) == 20`, `[p["name"] for p in synth][:3] == ["polyphony", "glide", "master_volume"]`, y que `smoother[0]["values"][-1] == smoother[0]["values"][-2]` (el paso de 0 frames no cambia el valor).
- [ ] **Step 2: Run** `python -m pytest tests/test_reference_export.py -v`. Expected: FAIL con `ImportError`/archivo inexistente.
- [ ] **Step 3: Implement** `build_params() -> dict`, `write(out_dir: Path) -> None` y un `main()` que acepte `python tools/export_reference.py rust/reference` en `tools/export_reference.py`. Usar `REGISTRY`, `DRUM_REGISTRY`, `clamp`, `to_norm`, `from_norm`, `Smoother` y `drums._clamp` sin modificarlos; escribir JSON con `indent=1` y `\n` final para que el diff sea estable.
- [ ] **Step 4: Generate** con `python tools/export_reference.py rust/reference`.
- [ ] **Step 5: Run** `python -m pytest tests/test_reference_export.py -v`. Expected: PASS.
- [ ] **Step 6: Commit**
  ```bash
  git add tools/export_reference.py rust/reference/params.json tests/test_reference_export.py
  git commit -m "feat(rust): reference exporter for the parameter registry"
  ```

### Task 2: Workspace y registro de parámetros del Synth

**Files:**
- Create: `rust/Cargo.toml`, `rust/nomad-core/Cargo.toml`, `rust/nomad-core/src/lib.rs`, `rust/nomad-core/src/params.rs`
- Modify: `.gitignore` (agregar `rust/target/`)
- Test: `rust/nomad-core/tests/params_synth.rs`

**Interfaces:**
- Consumes: `rust/reference/params.json` del Task 1.
- Produces en `nomad_core::params`:
  - `pub enum Curve { Lin, Exp }` (`Copy`, `PartialEq`, `Debug`).
  - `pub struct ParamSpec { pub name: &'static str, pub default: f64, pub lo: f64, pub hi: f64, pub curve: Curve, pub unit: &'static str, pub smooth_ms: f64 }`.
  - `impl ParamSpec { pub fn clamp(&self, v: f64) -> f64; pub fn to_norm(&self, v: f64) -> f64; pub fn from_norm(&self, n: f64) -> f64 }`.
  - `pub static SYNTH_PARAMS: [ParamSpec; 63]` en el orden del Python.
  - `pub fn find<'a>(list: &'a [ParamSpec], name: &str) -> Option<&'a ParamSpec>`.

- [ ] **Step 1: Write the failing test** `rust/nomad-core/tests/params_synth.rs` con `serde` (derive) + `serde_json` como dev-dependencies. Tests (cada uno carga `../reference/params.json` con `concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/params.json")`):
  - `registry_matches_python`: mismo largo (63) y, para cada índice, `name`, `unit`, `curve`, y `default`/`lo`/`hi`/`smooth_ms` iguales (`assert_eq!` exacto).
  - `clamp_matches_python`: para cada spec y cada par `(clamp_in, clamp_out)`, `spec.clamp(x)` ≈ `out` (relativa 1e-12).
  - `clamp_non_finite_returns_default`: `NaN`, `INFINITY`, `NEG_INFINITY` dan `spec.default` en todas las specs.
  - `to_norm_matches_python`: `spec.to_norm(x)` ≈ `to_norm_out` para `clamp_in`; incluye el caso fuera de rango (usa el valor acotado).
  - `from_norm_matches_python`: `spec.from_norm(n)` ≈ `from_norm_out` para `from_norm_in` (incluye −0.5 y 1.5).
  - `specs_are_valid`: `lo < hi`, `lo <= default <= hi`, y las `Exp` tienen `lo > 0`.
  - `find_known_and_unknown`: `find(&SYNTH_PARAMS, "glide")` es `Some` con `hi == 1000.0`; `find(&SYNTH_PARAMS, "nope")` es `None`.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-core` . Expected: FAIL (el crate no existe todavía; crear primero `Cargo.toml` del workspace con `members = ["nomad-core"]`, `resolver = "3"`, `[workspace.package] edition = "2024"` y el del crate, para ver el fallo por símbolos faltantes).
- [ ] **Step 3: Implement** `Curve`, `ParamSpec`, `SYNTH_PARAMS` y `find` en `params.rs`, y `pub mod params;` en `lib.rs`. La tabla se escribe a mano a partir de `rust/reference/params.json` (el test es la red de seguridad); fórmulas idénticas a `nomad/engine/params.py`: `clamp` (no finito → `default`), `to_norm` (acota y luego `ln(v/lo)/ln(hi/lo)` para `Exp`, lineal para `Lin`), `from_norm` (acota `n` a [0, 1], luego `lo*(hi/lo)^n` o `lo + n*(hi-lo)`). Agregar `rust/target/` a `.gitignore`.
- [ ] **Step 4: Run** `cd rust && cargo test -p nomad-core`. Expected: todos PASS.
- [ ] **Step 5: Commit**
  ```bash
  git add rust/Cargo.toml rust/nomad-core .gitignore
  git commit -m "feat(rust): nomad-core workspace and synth parameter registry"
  ```

### Task 3: `Smoother`

**Files:**
- Create: `rust/nomad-core/src/smoother.rs`
- Modify: `rust/nomad-core/src/lib.rs` (`pub mod smoother;`)
- Test: `rust/nomad-core/tests/smoother.rs`

**Interfaces:**
- Consumes: sección `"smoother"` de `rust/reference/params.json`.
- Produces en `nomad_core::smoother`: `pub struct Smoother { pub target: f64, pub value: f64, pub sample_rate: f64, pub time_ms: f64 }` con `pub fn new(target: f64, sample_rate: f64, time_ms: f64) -> Smoother`, `pub fn set(&mut self, target: f64)`, `pub fn advance(&mut self, frames: i64) -> f64`.

- [ ] **Step 1: Write the failing tests** en `tests/smoother.rs`:
  - `matches_python_traces`: para cada caso de la referencia, `Smoother::new(start, sr, ms)`, `set(target)`, y cada `advance(steps[i])` ≈ `values[i]` (relativa 1e-12).
  - `zero_or_negative_frames_do_not_move`: `advance(0)` y `advance(-5)` devuelven el valor actual y no lo cambian.
  - `zero_time_does_not_nan`: con `time_ms = 0.0`, `advance(1)` es finito y está entre `start` y `target`.
  - `never_overshoots`: con `time_ms = 15`, 1000 llamadas de `advance(64)` hacia `target = 1.0` desde 0 nunca superan 1.0 y son monótonas.
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-core --test smoother`. Expected: FAIL (símbolos faltantes).
- [ ] **Step 3: Implement** la recursión de `Smoother` de `nomad/engine/params.py`: `tau = max(time_ms / 1000 * sample_rate, 1e-9)`; `advance(frames)`: si `frames <= 0` devuelve `value`; si no, `value += (1 - exp(-frames / tau)) * (target - value)` y devuelve `value`. `new` inicializa `value = target`.
- [ ] **Step 4: Run** el mismo comando. Expected: PASS.
- [ ] **Step 5: Commit**
  ```bash
  git add rust/nomad-core
  git commit -m "feat(rust): one-pole parameter Smoother"
  ```

### Task 4: Registro de parámetros de la batería

**Files:**
- Modify: `rust/nomad-core/src/params.rs`
- Test: `rust/nomad-core/tests/params_drums.rs`

**Interfaces:**
- Consumes: `ParamSpec`, `find` (Task 2); sección `"drums"` de la referencia.
- Produces: `pub static DRUM_PARAMS: [ParamSpec; 20]` y `pub const INSTRUMENTS: [&str; 6] = ["kick", "snare", "clap", "tom", "chat", "ohat"]` en `nomad_core::params`.

- [ ] **Step 1: Write the failing tests** en `params_drums.rs`: `registry_matches_python` (largo 20; `name`, `unit`, `curve`, `default`, `lo`, `hi`, `smooth_ms` iguales por índice), `clamp_matches_python` (con `clamp_in`/`clamp_out` y `NaN`/`inf` → default), `instrument_order` (`INSTRUMENTS` es `["kick","snare","clap","tom","chat","ohat"]` y cada `"{i}_tune"`, `"{i}_decay"`, `"{i}_level"` existe con `find(&DRUM_PARAMS, …)`), y `bpm_and_swing_ranges` (`bpm` 60..180 default 110; `swing` 0..0.6 default 0).
- [ ] **Step 2: Run** `cd rust && cargo test -p nomad-core --test params_drums`. Expected: FAIL.
- [ ] **Step 3: Implement** `DRUM_PARAMS` e `INSTRUMENTS` en `params.rs`, desde `rust/reference/params.json` (sección `drums`).
- [ ] **Step 4: Run** `cd rust && cargo test --workspace`. Expected: todos PASS.
- [ ] **Step 5: Commit**
  ```bash
  git add rust/nomad-core
  git commit -m "feat(rust): drum parameter registry"
  ```

### Task 5: CI y formato

**Files:**
- Create: `.github/workflows/rust.yml`
- Modify: `rust/nomad-core` (sólo lo que pidan `cargo fmt` y `clippy`)

**Interfaces:** ninguna nueva.

- [ ] **Step 1: Run** `cd rust && cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`. Expected: sin advertencias (corregir lo que aparezca).
- [ ] **Step 2: Create** `.github/workflows/rust.yml`: se dispara con `push` y `pull_request` sobre `main`; matriz `ubuntu-latest`, `windows-latest`, `macos-latest`; en cada una, con `working-directory: rust`, ejecuta `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` y `cargo test --workspace`. No instala dependencias del sistema (todavía no hay audio).
- [ ] **Step 3: Verify localmente** `cd rust && cargo fmt --all -- --check && cargo test --workspace`. Expected: PASS. (Que el workflow corra en GitHub se ve al hacer push; no se puede comprobar localmente.)
- [ ] **Step 4: Commit**
  ```bash
  git add .github rust
  git commit -m "ci(rust): fmt, clippy and tests on Linux, Windows and macOS"
  ```
