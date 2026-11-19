# NOMAD en Rust, M4 (publicación para Linux) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Que NOMAD en Rust se pueda bajar y abrir en Linux sin instalar nada técnico: licencia GPL-3.0 con avisos de terceros, binario `release` optimizado, AppImage y tarball con suma SHA-256, y un workflow de CI que los publica en GitHub Releases al empujar una etiqueta `v*`.

**Architecture:** Nada cambia en el motor ni en la interfaz. Se agregan metadatos (licencia, versión, entrada de escritorio, ícono), un script reproducible que arma el AppDir y llama a `appimagetool`, un script de Python (sólo biblioteca estándar) que genera `THIRD_PARTY_NOTICES.md` desde `cargo metadata` y verifica que todas las licencias sean compatibles con GPL-3.0, y un workflow de release. Windows y macOS quedan para después (decisión del autor: Linux primero).

**Tech Stack:** Cargo (perfil `release`), `appimagetool` (descargado con versión y SHA-256 fijos en el script), GitHub Actions, Python 3 de la biblioteca estándar para los avisos.

**Spec:** `docs/superpowers/specs/2026-11-18-rust-standalone-design.md` (hito M4). Decisiones del autor: Linux primero; licencia **GPL-3.0-or-later**; la app de Rust ya está mergeada en `main`.

**Branch:** `feature/rust-m4` creada desde `main` (1e03258) en el worktree `/home/imn0p/synth-lab-rust` (así se reusa `rust/target` y no se recompila todo en otro árbol; el disco está justo). Al terminar se mergea a `main`. Un commit por tarea.

## Global Constraints

- No se modifica nada de `nomad/` ni el comportamiento de `rust/`: sólo metadatos, empaquetado, documentación y CI.
- Licencia del proyecto `GPL-3.0-or-later`: archivo `LICENSE` con el texto oficial completo de la GPL v3 y `license = "GPL-3.0-or-later"` en todos los `Cargo.toml` del workspace. El encabezado BSD-3-Clause de numpy en `rust/nomad-core/src/ziggurat.rs` se conserva y numpy aparece en los avisos de terceros.
- **`panic = "unwind"` se mantiene en `release`**: el callback de audio depende de `catch_unwind`. Perfil `release`: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`.
- El AppImage **no** empaqueta `libasound`, ni drivers de GPU, ni `libGL`/Vulkan: se usan los del sistema, como recomienda la lista de exclusión de AppImage. La compilación de CI usa `ubuntu-22.04` para que el binario corra en distros con glibc ≥ 2.35.
- Nada se publica sin que el autor empuje una etiqueta: no se hace push ni se crean etiquetas en este plan.
- El disco de `/home` está justo: no dejar AppDirs ni AppImages de prueba fuera de `rust/target` y borrarlos al terminar cada verificación.
- Cada tarea cierra con `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` y los pytest sin regresión.

## Review Focus

1. Una dependencia con licencia incompatible con GPL-3.0 (o sin licencia declarada, o sin texto de licencia encontrable) hace fallar el chequeo de avisos, en local y en CI (Task 1).
2. El binario `release` conserva `catch_unwind` (un panic de prueba en el callback no aborta el proceso) (Task 2).
3. El AppImage abre en una máquina sin Rust ni Python: `--version` funciona y la app encuentra su configuración en `~/.config/nomad-synth` como siempre (Task 3).
4. Un `appimagetool` descargado con suma SHA-256 distinta aborta el armado (Task 3).
5. El workflow de release no corre en pushes normales, sólo en etiquetas `v*`, y falla si los tests fallan (Task 4).

---

### Task 1: Licencia y avisos de terceros

**Files:**
- Create: `LICENSE`, `tools/third_party_notices.py`, `THIRD_PARTY_NOTICES.md` (generado), `tests/test_third_party_notices.py`
- Modify: `rust/Cargo.toml` (`[workspace.package] license = "GPL-3.0-or-later"` y `license.workspace = true` en cada crate)

**Interfaces:**
- `tools/third_party_notices.py`: `collect(metadata: dict) -> list[dict]` (por cada paquete de terceros de `cargo metadata --format-version 1 --manifest-path rust/Cargo.toml` alcanzable desde los binarios —excluye los crates del workspace y las dependencias sólo de desarrollo—: `name`, `version`, `license` (expresión SPDX), `texts` (contenidos de `LICENSE*`/`COPYING*`/`NOTICE*` junto al `manifest_path`)); `check(entries) -> list[str]` (errores: expresión vacía, ningún texto, o una expresión que no puede satisfacerse con licencias de la lista permitida `MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, Unicode-DFS-2016, BSL-1.0, CC0-1.0, Unlicense, MPL-2.0, 0BSD`; las expresiones `A OR B` se cumplen si alguna rama se cumple, `A AND B` si todas); `render(entries) -> str` (Markdown: encabezado, entrada fija para numpy —tablas del zigurat, BSD-3-Clause, con su texto—, y una sección por paquete con nombre, versión, licencia y textos); `main()` que con `--check` sólo verifica y con `--write PATH` escribe.
- `THIRD_PARTY_NOTICES.md` versionado en la raíz.

- [ ] **Step 1: Write the failing tests** `tests/test_third_party_notices.py`: `test_check_accepts_or_expressions` (`"MIT OR Apache-2.0"` sin errores), `test_check_rejects_gpl_incompatible` (`"SSPL-1.0"`, `"BUSL-1.1"` y vacío dan error), `test_check_requires_license_text`, `test_render_includes_numpy_entry`, y `test_committed_notices_are_up_to_date` (si `cargo` está en el `PATH`, regenerar y comparar con el archivo versionado; si no, `skip`).
- [ ] **Step 2: Run** `python -m pytest tests/test_third_party_notices.py -v`. Expected: FAIL.
- [ ] **Step 3: Implement** el script, agregar `LICENSE` (texto oficial de la GPL v3, sin modificar), los campos `license` y generar `THIRD_PARTY_NOTICES.md` con `python tools/third_party_notices.py --write THIRD_PARTY_NOTICES.md`. Si `--check` encuentra una dependencia incompatible, **parar y reportar** (no reemplazar dependencias sin consultar).
- [ ] **Step 4: Run** los tests + `python tools/third_party_notices.py --check` + el gate. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "chore: GPL-3.0-or-later license and third-party notices"`.

### Task 2: Binario `release`, versión y metadatos de escritorio

**Files:**
- Create: `packaging/linux/nomad.desktop`, `packaging/linux/nomad.svg`, `rust/nomad-app/tests/release_profile.rs`
- Modify: `rust/Cargo.toml` (`[profile.release]`), `rust/nomad-app/src/main.rs` (`--version` y `-V`), `rust/nomad-app/Cargo.toml` (`description`, `repository`, `homepage`)

**Interfaces:**
- `nomad --version` imprime `nomad <version de Cargo.toml>` (con `env!("CARGO_PKG_VERSION")`) y sale con 0.
- `[profile.release]` con `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"` y **sin** `panic = "abort"`.
- `nomad.desktop`: `Type=Application`, `Name=NOMAD Synth`, `Comment=Sintetizador de inspiración analógica`, `Exec=nomad`, `Icon=nomad`, `Categories=AudioVideo;Audio;Music;`, `Terminal=false`. `nomad.svg`: ícono simple (una perilla verde `#55e69a` sobre fondo `#101310`), vectorial y legible a 32 px.

- [ ] **Step 1: Write the failing tests.** `release_profile.rs`: `version_flag_prints_version` (correr el binario compilado por el test con `--version` vía `env!("CARGO_BIN_EXE_nomad")`: sale 0 e imprime `nomad 0.1.0`). Python o shell: `desktop-file-validate packaging/linux/nomad.desktop` si el comando existe (si no, un test de Python que valide las claves obligatorias). Un test que lea `rust/Cargo.toml` y falle si `[profile.release]` tiene `panic = "abort"`.
- [ ] **Step 2: Run** los tests. Expected: FAIL.
- [ ] **Step 3: Implement** el perfil, el flag y los archivos de escritorio.
- [ ] **Step 4: Run** el gate y `cargo build --release -p nomad-app && ./rust/target/release/nomad --version`. Expected: PASS y la versión.
- [ ] **Step 5: Commit** `git commit -m "build: release profile, --version and desktop metadata"`.

### Task 3: AppImage y tarball

**Files:**
- Create: `packaging/linux/build.sh`, `packaging/linux/AppRun`, `tests/test_packaging_linux.py`

**Interfaces:**
- `packaging/linux/build.sh [--skip-build]`: compila `cargo build --release -p nomad-app` (salvo `--skip-build`), arma `rust/target/appimage/NOMAD.AppDir` con `usr/bin/nomad`, `nomad.desktop`, `nomad.svg` (también como `.DirIcon`), `AppRun` (ejecuta `usr/bin/nomad "$@"`), `usr/share/doc/nomad/{LICENSE,THIRD_PARTY_NOTICES.md}`; descarga `appimagetool` de una URL con versión fija a `rust/target/appimage/` y **verifica su SHA-256** contra una constante del script (si no coincide, aborta con error); genera `rust/target/dist/NOMAD-<version>-x86_64.AppImage`, `rust/target/dist/nomad-<version>-x86_64-linux.tar.gz` (binario, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `README.md`, `nomad.desktop`, `nomad.svg`) y `rust/target/dist/SHA256SUMS`. La versión sale de `rust/nomad-app/Cargo.toml`. `set -euo pipefail`; no usa `sudo`; no toca nada fuera del repo.
- `tests/test_packaging_linux.py`: valida la forma del script sin ejecutarlo completo (tiene `set -euo pipefail`, la constante de SHA-256 no está vacía, no usa `sudo`) y, si existe `rust/target/dist/`, que `SHA256SUMS` coincida con los archivos.

- [ ] **Step 1: Write the failing tests** en `tests/test_packaging_linux.py`. Expected: FAIL.
- [ ] **Step 2: Implement** `build.sh` y `AppRun`.
- [ ] **Step 3: Verify a mano:** `packaging/linux/build.sh`, luego `rust/target/dist/NOMAD-0.1.0-x86_64.AppImage --version` (o con `--appimage-extract-and-run` si FUSE no está) y `sha256sum -c` en `rust/target/dist`. Abrir la ventana con el AppImage 5 s (`timeout 5`) **sólo si el autor no está usando la pantalla** (si no se sabe, saltear y decirlo). Anotar el tamaño de cada archivo. Borrar `rust/target/appimage` al terminar (deja `dist/`).
- [ ] **Step 4: Run** los tests + el gate. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "build: Linux AppImage and tarball packaging"`.

### Task 4: Workflow de release y README

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `README.md` (la app de Rust primero; la de Python como versión anterior)

**Interfaces:**
- `release.yml`: se dispara **sólo** con `push` de etiquetas `v*`; `runs-on: ubuntu-22.04`; instala `libasound2-dev`; corre el gate de Rust y `python tools/third_party_notices.py --check`; ejecuta `packaging/linux/build.sh`; verifica que la versión de la etiqueta (`v0.1.0`) coincida con la de `Cargo.toml` (si no, falla); sube los tres archivos de `rust/target/dist/` a una GitHub Release con `gh release create "$GITHUB_REF_NAME" --generate-notes` (permiso `contents: write`).
- `README.md`: qué es NOMAD; descargar el AppImage de Releases, `chmod +x` y abrir; requisitos del sistema (ALSA/PipeWire, OpenGL); dónde guarda la configuración (`~/.config/nomad-synth`, compartida con la versión de Python); cómo compilar desde fuente (`cargo build --release -p nomad-app`); comandos `nomad devices|demo|midi|bench|soak`; la licencia (GPL-3.0-or-later) y los avisos de terceros; la versión de Python como sección "Versión anterior (Python)" con sus instrucciones actuales.

- [ ] **Step 1: Write the failing test**: un test de Python que lea `.github/workflows/release.yml` (como texto) y verifique: disparador sólo de etiquetas `v*`, `ubuntu-22.04`, chequeo de versión de etiqueta, `third_party_notices.py --check`, y que no hay `push:` a ramas.
- [ ] **Step 2: Run** el test. Expected: FAIL.
- [ ] **Step 3: Implement** el workflow y el README.
- [ ] **Step 4: Run** el test + el gate. Si `actionlint` está disponible, correrlo; si no, decirlo.
- [ ] **Step 5: Commit** `git commit -m "ci: Linux release workflow and README for the Rust app"`. Luego mergear `feature/rust-m4` a `main` con `git merge --no-ff` (sin push) y reportar el hash.
