# NOMAD SYNTH

Sintetizador polifónico de inspiración analógica, pensado como un instrumento: 8 voces, 2
osciladores por voz, filtro escalera de 4 polos con resonancia, 2 envolventes ADSR, 2 LFOs, matriz
de modulación de 8 slots, secuenciador de 16 pasos con acordes y grabación, Machine Drum de seis
instrumentos sintetizados, pestañas con varias instancias y relojes compartidos, presets, MIDI learn
y modo ayuda.

La app actual está escrita en Rust y es un solo programa: no hace falta instalar Python, numpy ni
nada técnico. La versión anterior en Python sigue más abajo.

## Descargar y abrir (Linux)

1. Bajá `NOMAD-<versión>-x86_64.AppImage` de la página
   [Releases](https://github.com/nobelaar/nomad-synth/releases).
2. Dale permiso de ejecución y abrilo:

   ```bash
   chmod +x NOMAD-0.1.0-x86_64.AppImage
   ./NOMAD-0.1.0-x86_64.AppImage
   ```

   Si tu sistema no tiene FUSE, agregá `--appimage-extract-and-run`.

También hay un `nomad-<versión>-x86_64-linux.tar.gz` con el binario suelto, y `SHA256SUMS` para
verificar las descargas (`sha256sum -c SHA256SUMS`).

**Requisitos del sistema:** Linux x86_64 con glibc 2.35 o más nuevo (Ubuntu 22.04, Debian 12, Fedora
36 o posteriores), audio por ALSA o PipeWire, y gráficos con OpenGL o Vulkan. El AppImage usa la
biblioteca de audio y los drivers de video del sistema.

## Configuración

NOMAD guarda sus archivos en `~/.config/nomad-synth/` (o `$XDG_CONFIG_HOME/nomad-synth/`), los
mismos que usa la versión de Python:

* `midi-map.json`: los CC asignados con MIDI learn.
* `presets/synth/` y `presets/drums/`: los presets guardados con `SAVE`.

## Compilar desde el código fuente

Hace falta Rust (`rustup`) y las cabeceras de ALSA (`libasound2-dev` en Debian/Ubuntu,
`alsa-lib` en Arch).

```bash
cd rust
cargo build --release -p nomad-app
./target/release/nomad
```

Para armar el AppImage y el tarball en `rust/target/dist/`: `packaging/linux/build.sh`.

## Comandos

```
nomad                 abre la ventana (igual que `nomad gui`)
nomad --version       muestra la versión
nomad devices         lista las salidas de audio
nomad demo            toca una demo por consola (--device N, --seconds S, --midi)
nomad midi            muestra lo que llega por MIDI (--port TEXT, --seconds 20)
nomad bench           mide el costo del motor (--voices 8, --synths 2, --block 512)
nomad soak            prueba larga de audio y cuenta xruns (--minutes 10)
```

## Licencia

NOMAD se distribuye bajo **GPL-3.0-or-later** (ver `LICENSE`). Los componentes de terceros y sus
licencias están en `THIRD_PARTY_NOTICES.md`, que se genera con `tools/third_party_notices.py`.

## Versión anterior (Python)

La app original en Python queda como referencia (los tests de la versión en Rust comparan contra
ella) y sigue funcionando.

Sintetizador polifónico de estilo analógico en Python, pensado como un instrumento (perillas
continuas, sin menús desplegables, sin presets): 8 voces, 2 osciladores por voz, filtro
escalera de 4 polos con resonancia, 2 envolventes ADSR, 2 LFOs y una matriz de modulación de 8 slots.

### Instalar

```bash
python -m venv .venv
.venv/bin/pip install -r requirements.txt
```

Hace falta Tkinter del sistema (`tk` en Arch, `python3-tk` en Debian/Ubuntu).
`python-rtmidi` se compila desde el código fuente (~40 s) y necesita las cabeceras de ALSA/JACK.
`numba` es **opcional**: compila el filtro y las envolventes y baja el uso de CPU a ~25–30 % con
8 voces. Sin `numba` todo funciona (con `NOMAD_NO_NUMBA=1` se fuerza ese camino), pero con 8 voces
y resonancia alta el procesador va justo: bajá la polifonía o la resonancia.

### Correr

```bash
.venv/bin/python -m nomad.main                 # app completa
.venv/bin/python -m nomad.main --no-midi       # sin entrada MIDI
.venv/bin/python -m nomad.main --device 6 --volume 0.3 --demo-note 60
.venv/bin/python -m nomad.render --demo chord out.wav   # chord | sweep | lfo | poly, WAV mono 16 bit
.venv/bin/python -m pytest                     # tests
```

La primera vez se compilan los kernels de `numba` (unos 2 s); después quedan en caché.

### Controles

| Gesto | Efecto |
|---|---|
| Botones `SYNTH` / `DRUMS` del encabezado | Cambian la vista: panel del sintetizador o drum machine. La batería sigue sonando al volver a SYNTH |
| Arrastrar una perilla (vertical) | Cambia el valor continuamente (200 px = recorrido completo) |
| Shift + arrastrar | Ajuste fino (10x) |
| Rueda del mouse | Pasos |
| Doble click | Vuelve al valor por defecto |
| Click derecho / central en una perilla o selector | Menú: **Valor por defecto**, **MIDI learn** (después mové un CC del controlador; si ya está armado, **Cancelar learn**) y **Quitar MIDI** si el parámetro tiene un CC asignado |
| Botón `LEARN` | Arma el último control tocado |
| Botón `?` / `F1` | **Modo ayuda**: cursor de pregunta; al pasar el mouse por un control se abre una ventanita que explica qué es, cómo suena y con qué interactúa. Los controles no responden mientras está prendido. Se sale con `?`, `F1` o `Escape` |
| Teclado en pantalla | Click/arrastre toca notas (glissando); se ilumina con las notas MIDI |

En la vista DRUMS:

| Gesto | Efecto |
|---|---|
| Click en un paso de la grilla | Prende o apaga ese paso del patrón |
| Shift + click en un paso | Marca el paso como acento (suena más fuerte) |
| Click en el nombre del instrumento | Audiciona ese tambor suelto |
| `PLAY/STOP` | Arranca o para el patrón; STOP vuelve al paso 1 |
| Perillas `BPM` / `SWING` | Tempo (60–180) y shuffle (0–60 %), en vivo |
| `CLEAR` | Borra todos los pasos |
| Perillas `tune` / `decay` / `level` | Afinación, largo y volumen de cada instrumento |

### Drum machine

Seis instrumentos sintetizados (bombo, caja, clap, tom, hi-hat cerrado y abierto) en un patrón de
16 pasos (un compás de 4/4). No usa samples ni archivos: cada golpe se genera con osciladores y
ruido y se pre-renderiza al arrancar; mover `tune` o `decay` lo regenera al instante.
El patrón vive en memoria (no se guarda en disco) y arranca con un ritmo básico de bombo, caja y
hats. La batería se suma a las voces del sintetizador antes del master, así que comparten volumen,
limitador y osciloscopio.

Las ondas, modos, fuentes y destinos de modulación son selectores segmentados. El osciloscopio
muestra la señal real de salida con trigger; los botones de 5/10/20/50 ms cambian la escala.

### Secuenciador del synth

En la vista SYNTH, debajo de la matriz, `SEQ` activa un secuenciador de 16 pasos que toca notas con
el mismo motor (voces, envolventes, filtro) que el teclado, así que podés tocar encima a la vez por
MIDI. Click prende un paso, shift+click lo acentúa, la rueda del mouse cambia la nota (shift: una
octava) y `GATE` fija la duración. `PLAY` arranca solo la secuencia; la batería tiene su propio
PLAY y las dos comparten el BPM (la perilla está en la vista DRUMS). Si las arrancás en momentos
distintos no quedan alineadas entre sí. El patrón no se guarda en disco.

### Pestañas, instancias y reloj compartido

Cada pestaña del encabezado es una **instancia real** de un instrumento (Synth o Machine Drum), con sus
propios parámetros, patrón y estado. `+` crea otra (menú con los tipos disponibles) y `×` cierra la
pestaña. Cambiar de pestaña solo cambia qué instrumento editás: **todos siguen sonando** y se mezclan en
el master. El teclado en pantalla y el MIDI van al Synth seleccionado; al cambiar de pestaña se sueltan las
notas que quedaban sostenidas en el anterior. Los CC mapeados con LEARN también actúan sobre el Synth
seleccionado.

* **Volumen:** `MASTER` (columna derecha) es el volumen de toda la mezcla, como siempre. `LEVEL`, de cada
  instrumento, es su volumen individual (0–100 %, por defecto 100 %). `MUTE` silencia con un fade corto sin
  detener el instrumento.
* **Reloj:** `CLOCK:` elige la fuente. `Internal` = tempo propio. Elegir otro instrumento en Internal lo
  hace *seguidor*: hereda BPM, posición y PLAY/STOP, y su BPM y PLAY quedan bloqueados. Una fuente puede
  tener varios seguidores; no hay cadenas (un seguidor no puede ser fuente). La pestaña de la fuente
  muestra `●` y la del seguidor `⇄ nombre`.
* **Cambio de fuente:** es inmediato, sin cuantizar. Puede saltarse o repetirse un paso en el instante del
  cambio; los patrones no se reinician. Si la fuente se cierra, sus seguidores pasan a Internal conservando
  BPM y posición y siguen desde ahí; si la fuente se detiene, los seguidores también.
* El reloj no comparte patrones: cada instrumento mantiene su secuencia, longitud y subdivisión.

#### Límites conocidos (medidos con `tools/stream_stress.py` en esta máquina)
* **CPU:** un Synth con 8 voces ya ocupa ~3.4 ms de los 10.7 ms que dura un bloque de 512 frames (máx. 10.5).
  Cinco instrumentos con secuencias y batería suenan sin xruns, pero sumar 8 voces sostenidas encima genera
  xruns (63 en 10 s en la medición). Con 4 voces sostenidas, 5.
* **Crear / cambiar pestañas** con el audio sonando puede causar un xrun ocasional (3 en 12 creaciones): mostrar
  un panel por primera vez cuesta 40–90 ms del hilo de Tk. Los paneles se arman por secciones para repartir
  el costo, pero no se eliminó.

### Audio y MIDI

* **Dispositivo de audio:** el botón `AUDIO:` del encabezado cicla las salidas. Por defecto se usa
  la primera salida estéreo de JACK/PipeWire; si no hay, la del sistema.
* **MIDI:** el botón `MIDI:` cicla los puertos (los reales antes que `Midi Through`). Soporta notas,
  velocidad, pitch bend (rango en la perilla `bend_range`), rueda de modulación (CC1, 0..1 hacia la
  matriz como fuente `MW`), pedal de sustain (CC64), CC120/CC123 (todo apagado).
* **Mapa MIDI learn:** se guarda en `$XDG_CONFIG_HOME/nomad-synth/midi-map.json`
  (por defecto `~/.config/nomad-synth/`), asociado a puerto y canal.
* Si el encabezado muestra `XRUNS` creciendo, el procesador no da abasto: bajá la polifonía,
  la resonancia o instalá `numba`.

### Notas de sonido

* La resonancia máxima autooscila a propósito: el estado del filtro se limita para que el
  chirrido sea áspero pero acotado. El master es un `tanh` (no hay compensación por voz).
* Los osciladores usan PolyBLEP (saw/square); el triángulo tiene algo de alias a frecuencias altas.
* El modo `FREE` de cada oscilador usa su perilla `hz` (+ `fine` en cents) e ignora la tecla.
* `osc1_sync` no se muestra: el oscilador 1 es el maestro del sync; `osc2 SYNC` lo activa.

### Arquitectura

```
nomad/engine/   params, oscillator, envelope, filter, lfo, modmatrix, voice, voice_manager, engine
nomad/audio/    stream (sounddevice), scope (buffer circular)
nomad/midi/     midi_input (parseo, puerto, router/sustain), mapping (learn)
nomad/gui/      knob, selector, app (panel), scope_view, keyboard
nomad/main.py   entrypoint y cierre limpio   nomad/render.py   render offline
```

El motor no importa GUI ni MIDI: se maneja con `note_on/note_off`, `set_param`, `set_control` y
`render(out)`, por una cola sin locks, así que puede reemplazarse por una extensión en C sin tocar el resto.
`oscillator.py` en la raíz es el prototipo original y no forma parte del producto.
