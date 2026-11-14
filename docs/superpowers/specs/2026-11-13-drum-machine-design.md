# NOMAD SYNTH — Drum machine (diseño)

Fecha: 2026-11-13
Estado: propuesto (pendiente de aprobación)
Alcance: una drum machine chica dentro de la app existente, como segunda vista del panel.

## 1. Intención

Sumar a NOMAD una caja de ritmos de 16 pasos para tocar encima del sintetizador sin abrir otra
ventana ni otro programa. "Chica" es parte del pedido: seis instrumentos, un patrón, nada de
song mode.

Qué significa "listo": abrir la app, cambiar a la vista DRUMS, darle PLAY y que suene un patrón;
prender y apagar pasos mientras corre; mover `bpm` y `swing` y que el groove responda; volver a
SYNTH y tocar un acorde encima, con los dos sonando juntos y sin xruns.

## 2. Punto de partida (lo que hay hoy)

- Panel Tk fijo de 1366×768 armado en `nomad/gui/app.py`: encabezado, columna izquierda con las
  secciones de perillas, columna derecha con scope y global, teclado en pantalla abajo.
  **No hay pestañas.**
- Audio: un único `AudioStream` llama `engine.render(left)` sobre un buffer mono y lo espeja a
  estéreo. El master es `tanh(x * master_volume)` dentro de `Engine.render`.
- El motor ya tiene una cola sin locks (`Engine._q`, `SimpleQueue`) para comandos desde cualquier
  hilo, y `render` procesa en bloques de control de 256 frames (ruling R6).
- No hay samples, ni reproductor de archivos, ni ruido en el motor de voces.
- 38 tests verdes; `pytest` corre con `./.venv/bin/python -m pytest`.

## 3. Objetivos

1. Seis instrumentos sintetizados, sin archivos: bombo, caja, clap, tom, hi-hat cerrado, hi-hat
   abierto.
2. Secuenciador de 16 pasos (semicorcheas, 4/4) con acentos por paso, `bpm` 60–180 y `swing`.
3. Los tambores suenan mezclados con las voces del sintetizador, por la misma salida y el mismo
   master.
4. Vista DRUMS como segunda pestaña; el scope y la sección global siguen visibles en las dos.
5. Cero asignaciones de memoria en el callback de audio; sin dependencias nuevas.
6. Con la batería parada, la salida del motor es idéntica a la de hoy.

## 4. No-objetivos (esta vuelta)

- Samples, importar WAV, ni cajas externas.
- Varios patrones, encadenado, song mode, copiar/pegar.
- Velocidad por paso más allá de normal/acento.
- Guardar el patrón en disco.
- Disparar tambores con notas MIDI: en esta vuelta el teclado y el MIDI siguen siendo sólo del
  sintetizador (ver R7).
- MIDI learn de los controles de batería, salida MIDI de la batería, tambores en estéreo.
- Cambiar el largo del patrón (16 fijo) ni la métrica.

## 5. Anti-objetivos

- Que la batería cambie el sonido del sintetizador cuando está parada.
- Que mover una perilla de tambor produzca un xrun o un click.
- Que la vista DRUMS no entre en 1366×768.
- Que el motor importe Tkinter (la regla de siempre: `nomad/engine` no conoce GUI ni MIDI).

## 6. Restricciones

- Python 3.14, numpy; sin librerías nuevas.
- `nomad/engine` no importa GUI ni MIDI; la GUI no toca el estado del motor directamente.
- Sólo el hilo de Tk toca widgets; los comandos al motor van por cola.
- El callback de audio no reserva memoria ni puede levantar excepciones.
- El panel entero tiene que entrar en 1366×768.

## 7. Enfoques considerados

**A. Síntesis en vivo, muestra por muestra.** Cada instrumento se calcula dentro de `render`.
Máxima flexibilidad (todo se puede modular mientras suena), pero es el camino más caro: bucles
por muestra en Python compitiendo con 8 voces que ya van justas. Descartado por CPU.

**B. One-shots pre-renderizados (elegido).** Cada instrumento se genera una vez a un array numpy
de ~1 s con `tune` y `decay` aplicados. Tocar un paso es copiar/sumar un pedazo de ese array.
Cuando se mueve `tune` o `decay`, ese instrumento se regenera **en el hilo de la GUI** y el array
nuevo se entrega por la cola (el hilo de audio sólo cambia una referencia). Costo de CPU casi
nulo, sample-accurate y testeable sin tarjeta de sonido. Concesión: el golpe ya está "cocinado",
no se modula mientras suena.

**C. La batería como voces del motor existente.** Reusar oscilador, filtro y ADSR. El motor no
tiene ruido, el robo de voces pelearía con las notas del teclado y la matriz de modulación
terminaría afectando a la batería. Descartado.

## 8. Diseño de la parte elegida

### 8.1 Motor (`nomad/engine/drums.py`)

**Instrumentos y receta** (todos float32, mono, determinísticos con semilla fija por instrumento):

| id | sonido | receta | decay típico |
|---|---|---|---|
| `kick` | bombo | seno con caída exponencial de pitch (≈110→45 Hz) + click de ruido muy corto | 0.35 s |
| `snare` | caja | dos senos (~180/330 Hz) + ruido pasado por high-pass de un polo | 0.18 s |
| `clap` | palmas | ruido high-pass con 3–4 rebotes (~10 ms) y cola corta | 0.22 s |
| `tom` | tom | seno con caída de pitch más suave y cola larga | 0.45 s |
| `chat` | hat cerrado | ruido high-pass, caída rápida | 0.05 s |
| `ohat` | hat abierto | ruido high-pass, caída lenta | 0.30 s |

`tune` escala las frecuencias base (y en los de ruido, el corte del high-pass); `decay` escala la
constante de caída. `level` es ganancia del instrumento.

**`DrumMachine`**: clase única que junta banco de sonidos, secuenciador y mezcla.

- Estado del patrón: matriz `uint8` de 6×16 (0 = apagado, 1 = normal, 2 = acento).
- `bpm` (60–180, default 110) y `swing` (0–0.6, default 0). Semifrases de 16 pasos; el swing
  alarga los pasos pares y acorta los impares en la misma proporción.
- Reloj por muestras: `samples_per_step = sr * 60 / bpm / 4`. Un paso se dispara cuando el
  contador de fase lo cruza dentro del bloque, no "al principio del bloque": así el groove no
  depende del tamaño de callback.
- Arranque y parada (R8): `start()` dispara el paso 0 en el render siguiente; `stop()` resetea la
  fase al paso 0 (volver a arrancar empieza de nuevo). Cambiar `bpm` re-temporiza el resto del paso
  en curso con el intervalo nuevo, sin re-disparar; `swing` se aplica en el próximo límite de paso.
- `panic()` del motor también para la batería y corta las colas con un fade de 5 ms (R9).
- Mezcla: por instrumento se guarda `(array, offset)`; `render(out, n)` suma como máximo
  `min(n, len(array) - offset)` muestras y avanza `offset`. Re-disparar un instrumento que ya
  suena reinicia su `offset` (comportamiento natural de batería).
- Acento: ganancia 1.0 para acento y 0.6 para paso normal.
- Audición: `trigger(track, velocity)` dispara fuera del patrón (para clickear el nombre).

**API desde otros hilos** (valida en el hilo que llama y escribe el estado caller-side con stores
atómicos —floats, `uint8` y referencias de arrays—, sin cola: el orden no importa y el peor caso es
un bloque de latencia, igual que `snapshot` lee `active_notes` sin lock):

- `set_step(track, step, level)`, `clear_pattern()`
- `set_value(name, value)` para `bpm`, `swing` y `{track}_{tune,decay,level}`. Si el nombre es de
  tono (`tune`/`decay`), regenera el one-shot acá y reemplaza la referencia del banco.
- `trigger(track, velocity=1.0)`
- `start()`, `stop()`, `toggle()`
- `position` (paso actual) y `playing`: lectura sin lock para el playhead de la GUI (benign race).
- `render(out, n)`: hilo de audio; drena la cola primero, dentro de `try/except`, y suma al buffer.

**Especificaciones de parámetros**: `DRUM_REGISTRY` propio en `drums.py`, con `ParamSpec` de
`params.py` (6×3 + `bpm` + `swing`), para que las perillas de la GUI reusen `KnobModel` sin tocar
`REGISTRY` del sintetizador.

**Patrón inicial** (mi decisión, para que la vista suene al abrir): bombo en 0/4/8/12, caja en 4/12,
hat cerrado en los pares, acento en el 0 de cada compás.

### 8.2 Integración con el motor (`nomad/engine/engine.py`)

- `Engine.__init__` crea `self.drums = DrumMachine(self.sr)`.
- En `render`, después de `self.vm.render(b, ...)` y antes de la ganancia master, `self.drums.render(b, n)`.
  Así el `master_volume` aplica a todo y el `tanh` sigue siendo el limitador.
- **R7 — sin notas MIDI para la batería.** El teclado en pantalla arranca en la nota 36, que en el
  mapa GM es el bombo: rutear esas notas rompería el teclado y el invariante "con la batería parada
  todo suena igual". La batería se dispara sólo desde el secuenciador y desde el click en el nombre
  del instrumento. `Engine.note_on/note_off` no cambian.
- `Engine.panic()` (y por lo tanto CC120/CC123) llama `drums.stop_all()` además de `vm.panic()`
  (R9). Cambiar de dispositivo de audio no toca el estado de la batería: el `Engine` sobrevive al
  cierre del stream.
- Niveles por defecto conservadores (bombo 0.6, caja 0.5, clap 0.45, tom 0.5, hats 0.35/0.3) para
  que un patrón lleno no aplaste el master.
- Sin cambios en `nomad/audio/stream.py`: el scope y el stream ya ven la mezcla.

### 8.3 Interfaz (`nomad/gui/drums_view.py` + cambios en `app.py`)

- Encabezado: dos botones de vista `SYNTH` / `DRUMS`. El activo se pinta en verde (mismo lenguaje
  visual que el botón de ayuda).
- La columna izquierda (hoy las secciones del sintetizador) pasa a ser un área intercambiable:
  se construyen las dos vistas al inicio y se muestra una con `grid`/`grid_remove`. La columna
  derecha (scope + global) y el teclado en pantalla quedan como están; el teclado se oculta en
  DRUMS porque no se usa ahí y libera alto.
- Vista DRUMS, una fila por instrumento: nombre clickeable (audiciona el sonido), perillas
  `tune`, `decay`, `level`, y 16 botones de paso. Los pasos encendidos van en verde; los acentos
  con borde/fondo más claro. El paso actual se marca en el poll de 100 ms.
- Fila de transporte arriba de la grilla: `PLAY/STOP`, perilla `BPM`, perilla `SWING`, botón
  `CLEAR`.
- Click normal en un paso alterna apagado/normal; Shift+click alterna acento. Click derecho en
  una perilla de batería ofrece sólo "Valor por defecto" (sin MIDI learn en esta vuelta).
- Modo ayuda: las perillas y botones nuevos se registran con su explicación, como el resto.
- **R10 — plomería de parámetros separada.** Los 20 nombres de `DRUM_REGISTRY` no entran en
  `REGISTRY`: `Engine.__init__` construye `_values`/`_p`/`_smoothers`/`_SMOOTHED` desde `REGISTRY`
  y los tests de layout/help comparan ese conjunto contra `LAYOUT`. La vista DRUMS mantiene su
  propio diccionario de widgets (`self.drum_widgets`), lee los valores de un `drums.values`
  caller-side que `DrumMachine.set_value` actualiza al instante, y se refresca en el mismo poll de
  100 ms. `App.apply_param`, `Engine.set_param` y `MidiMap` no cambian, así que las perillas de
  batería no son MIDI-learnables en esta vuelta (coherente con §5). Los tests existentes de layout
  y help quedan intactos.
- Todo entra en 1366×768: 6 filas de ~70 px + transporte ≈ 500 px de alto y ~900 px de ancho.

### 8.4 Persistencia

Ninguna en esta vuelta. El patrón vive en memoria y arranca con el patrón inicial.

## 9. Verificación

Tests nuevos (`tests/test_drums.py`, sin tarjeta de sonido):

1. Cada one-shot: no vacío, largo esperado, cae a casi cero al final, determinístico.
2. `tune` y `decay` cambian el sonido (largo o contenido espectral distinto).
3. Reloj: a 120 BPM un paso cada 6000 muestras a 48 kHz; renderizar 4096 en bloques de 256 da
   los mismos eventos que renderizar 4096 de una vez.
4. Acento suena más fuerte que paso normal.
5. Re-disparo reinicia el golpe (no se solapa ni se suma dos veces).
6. `set_step`/`set_value` desde otro hilo se aplican en el siguiente `render`.
7. `Engine` con la batería parada rinde exactamente igual que antes (comparación contra un motor
   sin `drums`); con la batería sonando y sin notas, la salida no es cero.
8. `Engine.panic()` para la batería y deja la salida en cero al cabo del fade; `stop()` vuelve al
   paso 0 y `start()` dispara el paso 0 en el render siguiente.

Manual: patrón + acorde simultáneos sin xruns durante un minuto; mover `tune`/`decay` mientras
corre no clickea; la vista entra en 1366×768; volver a SYNTH no corta la batería.

## 10. Riesgos y mitigaciones

- **Regenerar un one-shot en el hilo de Tk** cuesta milisegundos por evento de arrastre. Si la
  medición supera ~5 ms, se limita a un re-render cada 50 ms (el valor final siempre se aplica).
- **Mezcla más caliente**: el `tanh` ya limita, pero un patrón denso puede comer headroom.
  Niveles por defecto bajos y `level` por instrumento para corregir.
- **Playhead a 100 ms**: alcanza para marcar el paso; no se promete precisión visual de muestra.

## 11. Lo que queda a criterio del que implemente

- Nombres exactos de funciones internas y forma de los arrays de mezcla.
- Recetas de síntesis al detalle (frecuencias, orden del high-pass, cantidad de rebotes del clap),
  respetando la tabla y que suenen a lo que dicen ser.
- Cómo se dibuja exactamente el acento y el playhead dentro de la paleta actual.
- Si el teclado en pantalla se oculta o se deja visible en DRUMS.

## 12. Dónde se construye

En `/home/imn0p/synth-lab` (checkout actual, `main` en `8c24b88`), sobre una rama nueva
`feat/drum-machine`. El worktree `/home/imn0p/synth-lab-nomad` quedó una revisión atrás y no se usa
para esto. Commits firmados con `-S` (ruling R1 del proyecto).
