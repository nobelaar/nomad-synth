# NOMAD SYNTH

Sintetizador polifónico de estilo analógico en Python, pensado como un instrumento (perillas
continuas, sin menús desplegables, sin presets): 8 voces, 2 osciladores por voz, filtro
escalera de 4 polos con resonancia, 2 envolventes ADSR, 2 LFOs y una matriz de modulación de 8 slots.

## Instalar

```bash
python -m venv .venv
.venv/bin/pip install -r requirements.txt
```

Hace falta Tkinter del sistema (`tk` en Arch, `python3-tk` en Debian/Ubuntu).
`python-rtmidi` se compila desde el código fuente (~40 s) y necesita las cabeceras de ALSA/JACK.
`numba` es **opcional**: compila el filtro y las envolventes y baja el uso de CPU a ~25–30 % con
8 voces. Sin `numba` todo funciona (con `NOMAD_NO_NUMBA=1` se fuerza ese camino), pero con 8 voces
y resonancia alta el procesador va justo: bajá la polifonía o la resonancia.

## Correr

```bash
.venv/bin/python -m nomad.main                 # app completa
.venv/bin/python -m nomad.main --no-midi       # sin entrada MIDI
.venv/bin/python -m nomad.main --device 6 --volume 0.3 --demo-note 60
.venv/bin/python -m nomad.render --demo chord out.wav   # chord | sweep | lfo | poly, WAV mono 16 bit
.venv/bin/python -m pytest                     # tests
```

La primera vez se compilan los kernels de `numba` (unos 2 s); después quedan en caché.

## Controles

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

## Drum machine

Seis instrumentos sintetizados (bombo, caja, clap, tom, hi-hat cerrado y abierto) en un patrón de
16 pasos (un compás de 4/4). No usa samples ni archivos: cada golpe se genera con osciladores y
ruido y se pre-renderiza al arrancar; mover `tune` o `decay` lo regenera al instante.
El patrón vive en memoria (no se guarda en disco) y arranca con un ritmo básico de bombo, caja y
hats. La batería se suma a las voces del sintetizador antes del master, así que comparten volumen,
limitador y osciloscopio.

Las ondas, modos, fuentes y destinos de modulación son selectores segmentados. El osciloscopio
muestra la señal real de salida con trigger; los botones de 5/10/20/50 ms cambian la escala.

## Secuenciador del synth

En la vista SYNTH, debajo de la matriz, `SEQ` activa un secuenciador de 16 pasos que toca notas con
el mismo motor (voces, envolventes, filtro) que el teclado, así que podés tocar encima a la vez por
MIDI. Click prende un paso, shift+click lo acentúa, la rueda del mouse cambia la nota (shift: una
octava) y `GATE` fija la duración. `PLAY` es el transporte global: la secuencia y la batería
comparten reloj y BPM (la perilla BPM está en la vista DRUMS). El patrón no se guarda en disco.

## Audio y MIDI

* **Dispositivo de audio:** el botón `AUDIO:` del encabezado cicla las salidas. Por defecto se usa
  la primera salida estéreo de JACK/PipeWire; si no hay, la del sistema.
* **MIDI:** el botón `MIDI:` cicla los puertos (los reales antes que `Midi Through`). Soporta notas,
  velocidad, pitch bend (rango en la perilla `bend_range`), rueda de modulación (CC1, 0..1 hacia la
  matriz como fuente `MW`), pedal de sustain (CC64), CC120/CC123 (todo apagado).
* **Mapa MIDI learn:** se guarda en `$XDG_CONFIG_HOME/nomad-synth/midi-map.json`
  (por defecto `~/.config/nomad-synth/`), asociado a puerto y canal.
* Si el encabezado muestra `XRUNS` creciendo, el procesador no da abasto: bajá la polifonía,
  la resonancia o instalá `numba`.

## Notas de sonido

* La resonancia máxima autooscila a propósito: el estado del filtro se limita para que el
  chirrido sea áspero pero acotado. El master es un `tanh` (no hay compensación por voz).
* Los osciladores usan PolyBLEP (saw/square); el triángulo tiene algo de alias a frecuencias altas.
* El modo `FREE` de cada oscilador usa su perilla `hz` (+ `fine` en cents) e ignora la tecla.
* `osc1_sync` no se muestra: el oscilador 1 es el maestro del sync; `osc2 SYNC` lo activa.

## Arquitectura

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
# nomad-synth
