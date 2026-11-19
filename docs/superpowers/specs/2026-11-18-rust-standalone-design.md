# NOMAD en Rust: app standalone (diseño)

Estado: borrador para aprobación. Autor: Claude con Nobel. Fecha: 2026-11-18.

## Intención y por qué
NOMAD es un sinte de inspiración analógica (Synth con secuenciador de acordes, Machine Drum, pestañas con
relojes compartidos, presets) hecho en Python. El objetivo es compartirlo con otra gente, y no sólo gente
técnica, Dos problemas del Python actual:

1. **Rendimiento y fiabilidad.** Medido en el i5-4300M del autor (máquina cargada, orden de magnitud): 8 voces
   + batería usan ~61 % del presupuesto de audio con numba y >130 % sin él; hay picos de 4 a 30 veces el
   presupuesto (GIL, recolector de basura, Tkinter). Es sobrecarga del intérprete (~1.500 llamadas Python por
   bloque de 256 muestras), no matemática. El test de 5 instancias es inestable por esto.
2. **Distribución.** Requiere Python, numpy, scipy, numba (LLVM) y Tk. No es instalable por un no técnico.

La solución es una app nativa en Rust, de un solo binario, que reemplaza a la de Python.

## Metas
- Paridad funcional 1:1 con la app de Python (inventario abajo), **incluyendo pestañas, varias instancias de
  Synth, Machine Drum, fuentes de reloj/seguidores, presets, glide, acordes y grabación (REC)**.
- Sin xruns en la máquina del autor (i5-4300M) con 2 Synths a 8 voces + batería, durante 10 minutos, a 48 kHz y
  bloque de 512 muestras: 0 xruns y tiempo máximo de callback < 50 % del presupuesto (criterio medible en M2).
- Binarios para Linux, Windows y macOS.
- El motor es una librería (`nomad-core`) sin dependencias de audio ni de interfaz, para poder probarlo solo.

## No-metas y anti-metas
- No es una reescritura "creativa": el comportamiento se porta, no se rediseña. Las mejoras (SLIDE por paso,
  sync MIDI, piano roll) van después de la paridad y sólo en Rust.
- Anti-meta: que la versión Python deje de funcionar. **No se modifica la app de Python** (queda congelada,
  sólo arreglos de errores): el autor la sigue usando hasta el cambio.

## Restricciones
- Cero asignaciones y cero bloqueos en el hilo de audio (el Python ya lo exige y lo testea).
- Los presets de usuario existentes (JSON en `~/.config/nomad-synth/presets/`) y el mapa MIDI
  (`midi-map.json`) deben seguir funcionando con el mismo formato.
- El código de Rust vive en `rust/` dentro de este repo; Python queda como oráculo para comparar audio.

## Enfoques considerados
- **A. Seguir en Python y empaquetar.** Días de esfuerzo, instalador, pero los picos de GIL siguen y el paquete
  es pesado. Descartado (no arregla la fiabilidad).
- **B. Motor en Rust + interfaz Python.** Arregla el audio pero la interfaz Tkinter toca arrays y estado interno
  del motor directamente: el puente sería grande y se tira al reemplazar la interfaz. Descartado.
- **C. Todo nativo en Rust (elegido por el autor).** Reemplazo completo.

Interfaz: **egui (eframe)** para el standalone. Elegida por mí: widgets propios (perillas, grilla de pasos,
osciloscopio) se dibujan directo y se itera rápido; no depende de nada más. Si luego se quiere otro
aspecto, la interfaz sólo lee estado y manda comandos, así que se puede cambiar.

## Arquitectura
Workspace Cargo en `rust/`:
- `nomad-core`: parámetros (registro declarativo, mismos nombres/rangos/curvas que `params.py`), osciladores
  (PolyBLEP), envolventes, filtro ladder, LFOs, matriz de modulación, voz, gestor de voces (con glide y legato),
  secuenciador (acordes, REC), `Clock`/`StepGrid`/`StepTracker`, batería, mezclador con topología y relojes
  seguidores. DSP en `f64` (igual que numpy) para poder comparar contra el oráculo.
- `nomad-app`: `cpal` (audio), `midir` (MIDI), `eframe/egui` (interfaz), presets y mapa MIDI con `serde_json`.

Modelo de hilos: **el hilo de audio es dueño del motor entero** (grafo de instancias, relojes, voces). La
interfaz le manda *comandos* por una cola SPSC sin bloqueo (parámetros, notas, edición de pasos, cambios de
topología: crear/cerrar instancia, cambiar fuente de reloj) y lee *estado* publicado por el audio: valores de
parámetros (átomos), posición del secuenciador/playhead, notas activas, osciloscopio (ring buffer). El
"traspaso" de relojes al cambiar de fuente (hoy `Engine.publish` + `handovers`) pasa a ser un comando que el
hilo de audio ejecuta por sí solo, sin estructuras compartidas.

Dos detalles de diseño que el Python resuelve con hilos y hay que decidir bien en Rust:
- **Batería:** los one-shots se vuelven a sintetizar cuando cambian `tune`/`decay` (hoy en el hilo de la interfaz,
  con asignaciones, y se intercambian de forma atómica). En Rust se regeneran **fuera** del hilo de audio y se
  entregan por la cola; el hilo de audio nunca asigna memoria.
- **Frecuencia de muestreo y bloque:** el motor es genérico en la frecuencia (como el Python) y siempre renderiza en
  bloques de control de 256 muestras, sea cual sea el tamaño de buffer del dispositivo. Los tests de referencia
  usan 48 kHz; el dispositivo puede pedir 44,1 kHz, y entonces se crea el motor a esa frecuencia.

## Inventario de paridad (lo que hay que portar)
Motor: 62 parámetros por Synth (más `master_volume`, que vive en el mezclador; osc ×2, filtro, 2 envolventes, 2 LFOs, 8 slots de matriz, polifonía, glide,
bend), 8 voces, pitch bend y modwheel, limitador maestro (tanh), nivel/mute por instancia, topología con
relojes y seguidores, secuenciador de 16 pasos con acordes (hasta 6 notas), DIV/LEN/GATE/swing, REC por pasos y
en vivo, Machine Drum (6 instrumentos sintetizados, 16 pasos, acento, swing).
Interfaz: barra de pestañas (crear/cerrar/seleccionar), barra de transporte por instrumento (PLAY, BPM, TAP,
reloj, preset, SAVE, mute, nivel), panel del Synth (secciones, matriz plegable, secuenciador), vista de la
Machine Drum, osciloscopio, volumen maestro, selector de audio/MIDI, MIDI learn con clic derecho, sistema de
ayuda (textos de `help.py` reusados), estado/voces/xruns.
Persistencia: presets de fábrica (4 Synth, 3 batería) y de usuario, mapa MIDI.

## Estrategia de verificación (oráculo)
- Un script de Python (`tools/export_reference.py`, nuevo, no toca la app) vuelca escenarios a `.npy`/JSON:
  por componente (formas de onda, envolvente, filtro, LFO, voz, gestor de voces con glide/legato, secuenciador)
  y renders completos del motor (el `engine_golden.npy` ya existe).
- Los tests de Rust cargan esos archivos y comparan con tolerancia (objetivo 1e-6 relativo).
- **Excepción conocida:** el ruido de la batería y el sample-and-hold del LFO usan `numpy.random.default_rng(seed)`
  (PCG64 + ziggurat) y no se puede reproducir bit a bit en Rust sin portar ese generador. Esas partes se verifican por
  espectro y envolvente RMS, o con el ruido exportado como fixture. (Decisión del constructor, ver abajo.)
- Los 450+ tests de comportamiento de Python se portan como tests de Rust donde tengan sentido.

## Hitos
- **M0 – Esqueleto:** workspace, registro de parámetros, exportador de referencia, CI.
- **M1 – Motor:** todo `nomad-core`; pasa los tests contra el oráculo.
- **M2 – Audio sin interfaz:** binario de consola con audio y MIDI reales; medición de xruns (criterio de la meta).
- **M3 – Interfaz con paridad:** pestañas, Synth, Machine Drum, presets, osciloscopio, MIDI learn, ayuda.
- **M4 – Instaladores** para las 3 plataformas; Python deja de ser la app principal.

## A cargo del constructor
Detalles de crate (versiones de `cpal`/`midir`/`eframe`, estructura de módulos), cómo verificar la batería
(espectro vs. fixture), nombres de comandos y esquema de la cola, formato del instalador por plataforma,
cuándo bajar a `f32` si hiciera falta.

## Prerrequisitos y trabajo previo
- **Rust no está instalado** en la máquina del autor (`cargo` y `rustc` no existen). Hace falta `rustup` antes
  de M0; lo instala el autor (comando interactivo).
- El formato de presets tiene validación en `nomad/presets.py` (`check`): la versión Rust debe aplicar las mismas
  reglas (rangos, `division` ∈ {0.25, 0.5, 1, 2, 4}, formas de arrays) para aceptar los mismos archivos.

## Riesgos
- Relojes/topología con el audio sonando: el comportamiento de los traspasos debe reproducirse exacto (hay
  tests de Python que lo fijan: `test_follow`, `test_clock_ui`, `test_engine_transport`).
- El tamaño de la interfaz (perillas, secuenciador, drum, ayuda) es la parte más larga.
- Sólo hay una máquina de pruebas (Linux). Windows y macOS se compilan en CI pero el audio y el MIDI ahí no se
  pueden probar sin alguien que lo haga; se necesita al menos un tester por plataforma antes de M4.
- Disponibilidad de audio en cada plataforma (`cpal` en Linux depende de ALSA/PipeWire); no probado aún.
- Estimación total a ojo: 1–2 meses de trabajo concentrado.
