# HANDOFF — NOMAD SYNTH

Fecha: 2026-11-13. Escrito para que otro agente continúe sin releer la conversación.

## Estado en una línea

3 de 19 tareas hechas y revisadas (38 tests verdes), `main` ya tiene el merge del avance, y **falta sólo la revisión de la Tarea 3** para seguir con el ADSR.

## Dónde está todo

| Cosa | Ruta |
|---|---|
| Worktree de trabajo (acá se implementa) | `/home/imn0p/synth-lab-nomad` — branch `feature/nomad-synth` |
| `main` (mergeado hasta `a429ec7`) | `/home/imn0p/synth-lab` |
| Diseño aprobado | `docs/superpowers/specs/2026-11-13-nomad-synth-design.md` |
| Plan de 19 tareas | `docs/superpowers/plans/2026-11-13-nomad-synth.md` |
| Ledger del proceso (leer primero) | `.superpowers/sdd/2026-11-13-nomad-synth/progress.md` |
| Briefs y reportes por tarea | `.superpowers/sdd/2026-11-13-nomad-synth/task-N-{brief,report}.md` |
| Diff de la revisión pendiente | `.superpowers/sdd/2026-11-13-nomad-synth/review-454a6b9..3c3243d.diff` |

No hay nada sin commitear. `git status` limpio en los dos worktrees.

## Lo primero que hay que hacer (exacto)

**Revisar la Tarea 3.** El dispatch del revisor se abortó: no existe veredicto.

1. Leer el ledger (`.superpowers/sdd/.../progress.md`), la sección "Task log".
2. Dispatch de un subagente revisor con la plantilla `task-reviewer-prompt.md` de la skill (ver abajo), usando:
   - brief: `task-3-brief.md`
   - report: `task-3-report.md`
   - diff: `review-454a6b9..3c3243d.diff`
   - base `454a6b9`, head `3c3243d`
3. Si aprueba: escribir en el ledger `Task 3: complete (commits 454a6b9..3c3243d, review clean)` y dispatch de la Tarea 4.
4. Si encuentra algo: fix loop (resumir el implementador original: `ses_e293983a6ffeXPJIrhKmtIimNo`).

## Proceso que se está siguiendo

Skill: `superpowers:subagent-driven-development`
Base: `/home/imn0p/.cache/opencode/npm/git-superpowers-0958a5557860/1793626282640/node_modules/superpowers/skills/subagent-driven-development/`

Por tarea: `scripts/task-brief PLAN N` → dispatch implementador → `scripts/review-package PLAN BASE HEAD` → dispatch revisor → fix loop (máx. 5 rondas) → línea de cierre en el ledger.

**No pausar entre tareas.** El único motivo para parar es: operación irreversible, acción sensible de seguridad, efecto fuera del worktree que requiera permiso (merge/push), o un plan tan roto que todo camino es una adivinanza.

Comandos (desde el worktree):

```bash
bash <skill>/scripts/task-brief docs/superpowers/plans/2026-11-13-nomad-synth.md 4
bash <skill>/scripts/review-package docs/superpowers/plans/2026-11-13-nomad-synth.md <BASE> <HEAD>
```

## Qué está hecho

| Tarea | Contenido | Commits |
|---|---|---|
| 1 | `nomad/engine/params.py`: registro de 62 parámetros, curvas lin/exp, `clamp`, `to_norm`/`from_norm`, `Smoother` | `9b634ff..974a5be` |
| 2 | `nomad/engine/oscillator.py`: sine/tri/saw/square, PolyBLEP, fase por voz | `974a5be..723b7c5` |
| 3 | Pulse width (sólo square) + hard sync con reset fraccional | `454a6b9..3c3243d` (revisión **pendiente**) |
| — | `tools/preview.py`: demo audible de los osciladores (no es producto) | `07ff550` |

## Rulings ya tomados (no re-litigar)

- **R1 — GPG.** `git commit -S` funciona cuando el agente de GPG tiene la passphrase en caché. Si falla con `cannot open '/dev/tty'`, pedirle al humano que corra `echo unlock | gpg --clearsign > /dev/null` en su terminal. **Nunca** commitear sin `-S`.
- **R2 — Matriz de modulación por voz.** `velocity` y `env2` son por voz; `lfo1/2` y `modwheel` son globales. El engine resuelve por voz (plan, Tarea 10).
- **R3 — `build_app(engine, stream=None, midi_map=None)`.** El botón LEARN recibe el `MidiMap` por parámetro.
- **R4 — `oscillator.py` de la raíz** es el prototipo viejo: no se toca ni se borra.
- **R5 — `Smoother`.** `time_ms` es constante de tiempo (no tiempo de asentamiento): `tau = time_ms/1000*sr`, `1 - exp(-frames/tau)`. El test usa 400 avances.
- **R6 — Granularidad.** Bloques de control de `min(256, frames)` para LFO, matriz, smoothers, envelopes y **ambos osciladores**. El filtro se subdivide en granos de 16 muestras sólo si `k > 3.2`, si no de 64. Motivo medido: osciladores en granos de 16 costaban 250% de CPU; en 256 cuestan 20%.

## Gotchas de entorno

- Venv: `./.venv` es un symlink a `/home/imn0p/synth-lab/.venv` (Python 3.14). Correr siempre `./.venv/bin/python -m pytest`; no hay shell activado.
- Dependencias ya instaladas: numpy, scipy 1.18, sounddevice, mido, python-rtmidi, pytest. `python-rtmidi` compila desde fuente (~40 s) si se rehace el venv.
- Audio: el dispositivo bueno es el **idx 6** ("Built-in Audio Analog Stereo", host API JACK sobre PipeWire, 48 kHz). El **idx 5** (plugin ALSA `pipewire`) midió 1 callback en 400 ms: descartado.
- **No hay teclado MIDI conectado.** Para testear MIDI: puertos virtuales de rtmidi + `aconnect <src>:0 <dst>:0`.
- Pantalla 1366×768: el panel tiene que entrar ahí.
- `DISPLAY=:0` funciona (Tk abre).

## Gotchas técnicos

- API del oscilador: `render_into(out, freq, pw=0.5, sync_master_phase=None) -> float`; suma en `out`, devuelve fase nueva en ciclos. El triangle ignora `pw`.
- El triángulo es un integrador cerrado del square ideal: exacto en DC y simetría, pero alias residual de −34 dB a 5.2 kHz y −27.7 dB cerca de 7 kHz (minor diferido; polyBLAMP sería el fix).
- `pw != 0.5` deja DC en el triángulo (la Tarea 3 sólo toca square).
- Cada `Oscillator()` preasigna un ramp de 2048 (16 KB): el note-on no es zero-alloc hasta que haya pooling en las Tareas 9/10.
- El test de "quantum agnóstico" (Tarea 10) ya no exige igualdad bit a bit entre cualquier tamaño de callback: 2048 de una vez == 8×256 exacto, y tamaños raros sólo finitos y sin click.

## Qué falta

Tareas 4 a 19 del plan, en orden:

4. Envelope ADSR · 5. Filtro escalera 4 polos · 6. LFO · 7. Matriz de modulación · 8. Voz (2 osc + mixer + filtro + 2 ADSR) · 9. Voice manager (1..8, robo con fade) · 10. Engine (cola sin locks, mezcla, limitador) · 11. Buffer del scope · 12. Stream de audio · 13. MIDI + enrutado (sustain, bend, modwheel) · 14. MIDI learn + persistencia · 15. Perilla y selectores · 16. Panel · 17. Scope + teclado en pantalla · 18. `main.py` + cierre limpio + render offline · 19. Verificación final + README.

Al terminar la 12 ya se puede escuchar audio real desde la app. La 16–18 es la GUI.

## Verificación rápida al retomar

```bash
cd /home/imn0p/synth-lab-nomad
git status --short            # debe estar limpio
./.venv/bin/python -m pytest -q   # 38 passed
tail -40 .superpowers/sdd/2026-11-13-nomad-synth/progress.md
```

## Deudas registradas en el ledger

Menores diferidos de las Tareas 1 y 2 (import muerto, semántica de `inf`, `from_norm(nan)`, alias del triángulo, DC con `pw != 0.5`, pooling de osciladores). La revisión final de rama tiene que triarlos.
