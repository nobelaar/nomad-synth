# NOMAD SYNTH — Diseño

Fecha: 2026-11-13
Estado: propuesto (pendiente de aprobación)

## 1. Intención

Convertir el prototipo de un archivo (`oscillator.py`, un seno global con una
GUI) en un instrumento polifónico de 8 voces, inspirado en el Moog Matriarch:
control continuo, ergonomía de instrumento, experimentación.

El teclado MIDI es el controlador principal. La GUI es el panel: perillas
grandes, secciones separadas, sin menús desplegables. El osciloscopio muestra
la señal real que se está escuchando.

Qué significa "listo": sentarse, tocar un acorde, mover el cutoff y escuchar
la resonancia, detunear los dos osciladores y oír el batido, modular el pitch
con un LFO, y que nada clickee, crashee ni se corte.

## 2. Punto de partida (lo que hay hoy)

- `oscillator.py` (281 líneas): tkinter + `sounddevice.OutputStream`, un solo
  oscilador senoidal global, dispositivo 5 hardcodeado, salida mono,
  `blocksize=256`, 44.1 kHz.
- El osciloscopio **no muestra audio**: dibuja `sin(2*pi*f*t)` a partir de la
  perilla de frecuencia.
- No hay MIDI, ni voces, ni filtro, ni envelope.
- Sin separación de responsabilidades: la GUI muta globales que lee el
  callback de audio.

Lo que se conserva: la estética oscura, la paleta verde, la idea de un scope
grande arriba/abajo del panel, y `sounddevice` como salida.

## 3. Objetivos

1. Polifonía configurable 1..8 con asignación, robo de voces sin clicks y
   estado independiente por voz.
2. Dos osciladores por voz: sine, triangle, saw, square; fine tune; PWM;
   sync; mezclador con volúmenes individuales; afinación continua (no notas
   fijas).
3. Dos ADSR (amplitud y filtro) reutilizables para más destinos.
4. Filtro pasa-bajos resonante de 4 polos con autoscilación al máximo.
5. Dos LFO (sine, triangle, saw, square, S&H) con rate, depth y fase, y una
   matriz de modulación con source/destination/amount.
6. MIDI: descubrimiento de puertos, notas con velocity, sustain, pitch bend,
   mod wheel, y **MIDI learn** para mapear cualquier CC a cualquier perilla.
7. Teclado en pantalla para probar sin hardware.
8. Osciloscopio sobre el buffer real de salida, con trigger y escala de tiempo.
9. Panel que entre en 1366×768 y se sienta como un instrumento.

## 4. No-objetivos (v1)

- Presets/banco de patches (el mapa MIDI sí persiste; el patch no).
- Grabación o exportación de audio.
- Arpegiador, secuenciador, efectos (reverb/delay/chorus).
- Modos HP/BP del filtro expuestos en la GUI (la arquitectura los permite).
- Más de 8 voces, MPE, aftertouch polifónico.
- Windows/macOS. Linux + ALSA/PipeWire solamente.
- Escalas microtonales por archivo.

## 5. Anti-objetivos

- Que el callback de audio haga cualquier cosa que no sea DSP: nada de
  locks, I/O, GUI, `print`, ni allocations por bloque.
- Que el peor caso de un bloque supere la mitad del presupuesto de tiempo.
- Que la GUI sea un dashboard con menús o controles diminutos.
- Que un cambio de perilla produzca un click o un salto de frecuencia.

## 6. Restricciones del entorno (verificadas)

- Python 3.14.5, Arch Linux, X11 (`DISPLAY=:0`), pantalla **1366×768**.
- Instalado: numpy 2.5.4, sounddevice 0.5.6, matplotlib, pillow.
- **scipy 1.18.1** tiene wheel cp314 → se agrega (filtro resonante a velocidad C).
- **python-rtmidi compila en 3.14** (verificado) + `mido` 1.3.3 → se agregan.
  Es el único camino estándar para abrir puertos MIDI reales desde Python.
- Salidas medidas (corregido respecto del prototipo):
  - idx 5, ALSA `pipewire` plugin: en la prueba entregó **1 callback en 400 ms**
    con `latency='low'`. No sirve como camino principal.
  - idx 6, `Built-in Audio Analog Stereo`: es el **host API JACK** de PortAudio
    sobre PipeWire (48 kHz), y anduvo limpio con bloques de 256 (76 callbacks
    en 0.4 s). **Es el default.**
- Latencia objetivo: **≤ 10 ms** de ida y vuelta. No se exige tocar la
  configuración de PipeWire; si el quantum se puede bajar, se baja desde la
  app con `PIPEWIRE_QUANTUM` como hint, sin depender de eso.
- El quantum de PipeWire **puede cambiar en runtime** (otro cliente aparece):
  `render()` tiene que aceptar cualquier cantidad de frames.
- **Ahora mismo no hay teclado MIDI conectado** (solo `Midi Through`). El
  desarrollo se testea con puertos virtuales de rtmidi + `aconnect`.
- No hay que asumir que el usuario tiene un controlador con knobs: MIDI learn
  funciona con cualquier CC que llegue.

## 7. Enfoques considerados

### 7.1 Filtro

| Opción | Qué da | Costo |
|---|---|---|
| **A. Escalera de 4 polos (elegida)** | 24 dB/oct, pico resonante real, autoscila | Coeficientes en forma cerrada por grano |
| B. Biquad de 2 polos | 12 dB/oct, más simple | Menos cuerpo, no autoscila |
| C. Dos biquads en cascada | 24 dB/oct sin feedback | Resonancia con dos picos |

Se elige A porque el carácter de la escalera es el sonido de referencia y es
la única que llega a autoscilar. Verificado: con `k=3.9` el pico está 23 dB
arriba y los polos quedan en |z|=0.9992; con `k>4` oscila y el limitador de
estado la mantiene acotada (amplitud estable, frecuencia a ~3% del cutoff).

### 7.2 Entrada MIDI

| Opción | Qué da | Costo |
|---|---|---|
| **A. mido + python-rtmidi (elegida)** | API estándar, puertos virtuales para testear | Compila desde fuente (~40 s) |
| B. ctypes sobre ALSA seq | Cero dependencias | ~250 líneas propias, solo Linux |

Se elige A: el código de binding no es el problema que queremos resolver, y
los puertos virtuales permiten testear notes/bend/CC sin hardware.

### 7.3 GUI y scope

| Opción | Qué da | Costo |
|---|---|---|
| **A. tkinter Canvas (elegida)** | Perillas propias, control total del redibujado, cero deps | Knobs y escala dibujados a mano |
| B. matplotlib embebido | Scope fácil | Lento, estética de plot |

Se elige A: el panel es un instrumento, no un gráfico.

## 8. Arquitectura

```
nomad/
  engine/                 # sin imports de GUI ni MIDI
    params.py             # ParamSpec: nombre, rango, default, curva, unidad
    smoothing.py          # one-pole por parámetro, valores por bloque
    oscillator.py         # 4 ondas + PolyBLEP + PWM + sync, fase por voz
    envelope.py           # ADSR (amp, filtro, y lo que venga)
    filter.py             # Ladder4Pole (modo lp); hp/bp después
    lfo.py                # 5 ondas, rate/depth/fase, free-run
    modmatrix.py          # slots source -> destination -> amount
    voice.py              # 2 osc + mixer + filtro + 2 ADSR
    voice_manager.py      # 8 slots, asignación, steal con fade
    engine.py             # fachada DSP: render(frames), set_param, note_on/off
  audio/
    stream.py             # sounddevice; el callback sólo llama a render
    scope.py              # ring buffer de salida + ventana para la GUI
  midi/
    midi_input.py         # hilo mido -> cola de eventos
    mapping.py            # MIDI learn + persistencia JSON
  gui/
    app.py                # ventana y layout
    knob.py               # perilla continua (drag vertical + rueda)
    selector.py           # botones de waveform / segmentados
    keyboard.py           # teclado en pantalla
    scope_view.py         # dibujo del buffer real
  main.py                 # arma todo, cierra limpio
tests/
```

Contrato que sobrevive a un reemplazo por C:

```python
engine.note_on(note: int, velocity: float)
engine.note_off(note: int)
engine.set_param(name: str, value: float)
engine.render(out: np.ndarray)      # escribe in-place, sin allocar
engine.panic()
```

GUI y MIDI nunca comparten estado con el audio: empujan comandos a una cola
`queue.SimpleQueue` que `render()` drena al inicio de cada bloque. El estado
de lectura (perillas, LEDs, voces activas) se publica en un snapshot
inmutable que la GUI lee cuando quiere.

### 8.1 Hilo de audio

```
callback(outdata, frames, time, status):
    1. drenar la cola de comandos (note on/off, set_param)
    2. engine.render(outdata[:, 0])
    3. scope.write(outdata[:, 0])
```

Nada más. Sin locks, sin logging, sin numpy allocations grandes (los buffers
están preasignados en `__init__`).

## 9. Decisiones de DSP

### 9.1 Formato y granularidad

- 48 kHz, mono interno, estéreo en el stream (misma señal; el ancho llega con
  el chorus futuro).
- El bloque de callback es de 256 muestras, pero **no se asume**: el engine
  parte cada llamada en *bloques de control* de `min(256, frames_restantes)` y
  lleva su propio estado entre llamadas. Un quantum de 128, 512 o 1024 no
  rompe nada.
- En cada bloque de control avanzan los LFOs, la matriz, los smoothers, los
  envelopes y **los dos osciladores**. El filtro es la excepción: se subdivide
  en granos de 16 muestras cuando `k > 3.2` (calidad de autoscilación) y de 64
  en el resto, porque es lo único que necesita coeficientes de alta cadencia.
- Por qué: medido, renderizar los osciladores en granos de 16 cuesta 250 % de
  CPU en triangle (8 voces × 2 osc) por el overhead de numpy en arrays
  chicos; en bloques de 256 cuesta 20 % (§17). La modulación queda a 187 Hz,
  suficiente para LFOs de hasta ~10 Hz; si un LFO rápido suena escalonado, la
  solución es modulación de fase dentro del oscilador, no volver a granos de
  16.
- Peor caso medido: **≈2 ms por bloque de 256** (osciladores 1.06 ms con
  triangle + filtro ~0.9 ms con granos de 16), 38 % del presupuesto de 5.33 ms.

### 9.2 Parámetros

Registro declarativo (`params.py`): nombre, rango, default, curva
(lineal/exp), unidad. Cada parámetro tiene un smoother one-pole con tiempo
de ~15 ms y un valor "target" que escribe la GUI. El engine sólo lee valores
suavizados → ningún salto genera click.

Afinación continua: `coarse` en semitonos (-24..+24) + `fine` en cents
(-50..+50), sumados a la nota MIDI. El modo de afinación es **por oscilador**
(`osc1_mode` / `osc2_mode`):

- `track` (default): suman al pitch de la tecla.
- `free`: el oscilador ignora la tecla y suena a la frecuencia absoluta que
  marcan sus controles (20 Hz..8 kHz, escala log). Las notas siguen abriendo
  y cerrando la voz, así que se puede tocar el teclado mientras un oscilador
  queda como drone; el teclado en pantalla funciona igual en los dos modos.

En `free`, `coarse` pasa a leer Hz y `fine` sigue siendo cents. Son los mismos
dos parámetros con otra curva; no hay parámetros nuevos.

### 9.3 Osciladores

- saw y square con PolyBLEP (anti-aliasing en el borde de discontinuidad).
- **Cuatro formas** por oscilador: sine, triangle, saw, square. No hay una
  quinta forma "pulse": el ancho `pw` (5%..95%) actúa **sobre el square**, que
  pasa a ser pulse al alejarse del 50%. En las otras tres formas `pw` no hace
  nada (y la perilla se muestra atenuada).
- pulse implementado como diferencia de dos saws desfasadas, con BLEP en los
  dos bordes; con `pw=50%` degrada exactamente a square.
- triangle: square integrada y normalizada (el BLEP del square queda suave).
- sine: directo.
- sync: el osc 2 reinicia fase cuando el osc 1 cruza; corrección BLEP en el
  reinicio para no aliasing.
- Fase independiente por voz y por oscilador, guardada entre bloques.

### 9.4 Envelopes

ADSR con segmentos exponenciales (aproximación analógica con coeficiente por
muestra calculado por bloque), estado por voz. `Envelope` es una clase sin
saber de quién es: la voz instancia dos (amp y filtro). El release se calcula
desde el nivel actual, no desde sustain, así un note-off en pleno attack no
salta.

### 9.5 Filtro

Escalera de 4 polos lineal con feedback `k`:

```
g   = tan(pi * fc / fs)                  # pre-warp
P   = (1+g) - (1-g) z^-1
V   = 1 + z^-1
den = P^4 + k * g^4 * V^4
num = g^4 * V^4                          # normalizado por den[0]
```

`scipy.signal.lfilter` con `zi` guardado por voz entre granos. Los
coeficientes se recalculan por grano para todas las voces en una sola
operación vectorizada.

- Grano **64 muestras** con `k ≤ 3.2` (barato), grano **16** por encima: la
  autoscilación se vuelve notablemente más limpia (medido: −9.5 dB fuera de
  banda a 440 Hz con grano 16 contra −0.2 dB con grano 64).
- `k` 0..3.9: resonancia creciente, estable, limpia.
- `k` 3.9..4.3: autoscila. El estado se limita **por grano** para que la
  amplitud no diverja. Es una autoscilación con carácter rasposo, no un seno
  puro: el limitador de estado *es* el mecanismo de saturación, y eso deja
  armónicos. Se acepta a propósito; el seno puro requeriría un `tanh` por
  muestra dentro del lazo, que no entra en Python y es trabajo del futuro
  engine en C.
- La frecuencia de autoscilación queda **+3.6% arriba** del cutoff (medido,
  constante en todo el rango). Se deja así: al tocar por oído se busca el
  tono, no el número.
- Cutoff 20 Hz..18 kHz, en escala exponencial.
- `filter_env_amount`: suma del ADSR de filtro al cutoff en octavas (±5).
- Modos hp/bp: `Filter` expone `mode`, pero sólo `lp` está implementado y
  cableado en la GUI.

### 9.6 LFO y matriz

Dos LFO **globales** (compartidos por todas las voces, como el Matriarch): no
se reinician con cada nota, así un acorde tiene vibrato coherente en vez de
un coro. Rate 0.02..40 Hz, depth propio, fase inicial ajustable y 5 formas.
Corren a tasa de grano (16 muestras). S&H usa `np.random.default_rng` con
semilla fija por LFO para que sea reproducible entre corridas.

Matriz de 8 slots: `source ∈ {LFO1, LFO2, MODWHEEL, VELOCITY, ENV2}`,
`destination ∈ {PITCH, PITCH1, PITCH2, AMP, CUTOFF, PW1, PW2}`, `amount`
bipolar (-1..+1). El engine suma las contribuciones por destino. Agregar un
destino nuevo = agregar una fila a la tabla de destinos, no tocar los módulos.

### 9.7 Voces

8 slots preasignados. Política:

1. Voz libre → usarla.
2. Sin libres → robar la más vieja en release.
3. Sin releases → robar la más vieja en general.

El robo nunca corta en seco: la voz robada entra en un fade de 5 ms y recién
después se reconfigura y dispara el nuevo note-on. `polyphony` (1..8) limita
cuántos slots pueden sonar; bajar la polifonía con notas sonando libera las
voces excedentes con el mismo fade.

### 9.8 Mezcla y clipping

Cada voz suma 2 osciladores → filtro → ADSR. Master: ganancia por voz
compensada (`1/sqrt(n_voices)` para que un acorde no sature), `tanh` suave en
el master como limitador, y un medidor en la GUI. Nunca se permite que la
suma llegue a ±1.0 antes del limitador.

## 10. MIDI

- `mido` con backend `rtmidi`, hilo propio que hace `poll()` y empuja a la
  cola del engine.
- Puertos: lista dinámica con refresco manual (sin polling constante).
- Notas con velocity → `amp = (v/127)^1.5`.
- Sustain (CC64) sostiene notas pendientes de release; al soltar, se sueltan.
- Pitch bend ±2 semitonos (configurable), suavizado.
- Mod wheel (CC1) entra a la matriz.
- **MIDI learn**: botón `LEARN` en el panel; se arma, el usuario mueve una
  perilla física, y el próximo CC queda asociado al parámetro seleccionado
  (el último tocado con el mouse). Un CC aprendido mueve el parámetro en su
  rango completo. El mapa se guarda en
  `~/.config/nomad-synth/midi-map.json` y se recarga al iniciar.
- La clave de un mapeo es **(puerto, canal, CC)**, no sólo el CC: casi todos
  los controladores mandan CC1/CC7/CC64, y con clave sólo-CC conectar un
  segundo teclado reescribiría el mapa del primero.
- Resolución al recibir: primero se busca coincidencia exacta de
  (puerto, canal, CC); si no hay, se acepta (canal, CC) de cualquier puerto.
  Así el mapa sigue sirviendo si el puerto cambia de nombre o de índice, y
  no se pisan parámetros si conviven dos controladores.
- Un puerto que no está presente simplemente no genera eventos: el mapeo se
  conserva en disco.
- Sin teclado conectado: el teclado en pantalla hace exactamente las mismas
  llamadas al engine.

## 11. GUI

Layout (1366×768, usable también más chico):

```
+--------------------------------------------------------------+
| NOMAD SYNTH        [device ▾] [voices 1..8] ●  xruns: 0      |
+----------------+----------------+----------------------------+
| OSC 1          | OSC 2          | FILTER                     |
| wave + 5 knobs | wave + 5 knobs | cutoff/res/env + scope     |
+----------------+----------------+                            |
| MIXER          | AMP ENV        | (scope grande, 400x160)    |
| 2 faders       | 4 knobs        |                            |
+----------------+----------------+                            |
| LFO 1 / LFO 2  | MOD MATRIX (8 filas)                        |
+--------------------------------------------------------------+
| teclado en pantalla (2 octavas)                              |
+--------------------------------------------------------------+
```

- Perillas: `Knob` en Canvas, arrastre vertical (y con shift = fino), rueda
  del mouse, doble click = default. Arco de valor + aguja, valor numérico
  debajo. Diámetro 46–56 px.
- Selectores de onda: 4–5 botones, no dropdown.
- Feedback: la perilla se ilumina al ser tocada y muestra el valor; en modo
  LEARN parpadea la que está armada.
- El scope se redibuja a 30 fps leyendo el ring buffer; el trigger busca un
  cruce por cero ascendente para estabilizar la onda. Escala de tiempo:
  botones 1/2/5/10/20 ms.
- El contador de `xruns` no depende del backend: el host API JACK de PortAudio
  no siempre llena los flags de estado, así que además se mide el desvío entre
  el tiempo esperado y el real de cada callback, y se muestra el máximo de la
  sesión. Si el audio se corta, se ve ahí.
- La GUI nunca bloquea el audio: los comandos van por cola y el dibujo usa
  snapshots.

## 12. Cierre limpio

`main.py` registra `WM_DELETE_WINDOW`: para el stream, cierra el puerto MIDI,
corta el hilo y destruye la ventana. `panic()` deja las voces en release
rápido antes de cerrar el stream para no dejar un click.

## 13. Testing

Hay tests automáticos y hay escucha. No se declara nada funcionando sin las
dos cosas.

- `tests/test_envelope.py`: forma del ADSR, release desde nivel actual,
  duración de attack/decay con tolerancia.
- `tests/test_voice_manager.py`: asignación, robo (elige la más vieja), fade
  al robar, polifonía 1..8, note-off de nota inexistente no rompe.
- `tests/test_filter.py`: la resonancia sube el pico (respuesta en frecuencia),
  polos dentro del círculo para k<4, autoscilación acotada para k>4, y no hay
  NaN/inf con cutoff en los extremos.
- `tests/test_oscillator.py`: PolyBLEP reduce aliasing vs. saw naive (medido
  por FFT), continuidad de fase, sync no explota.
- `tests/test_modmatrix.py`: suma de fuentes por destino, saturación de amount.
- `tests/test_engine.py`: 1 s de audio con 8 voces no supera 1.0 en el master,
  no produce NaN, y el bloque no aloca buffers nuevos (medido por conteo de
  llamadas de `np.empty` con un wrapper de test).
- `tests/test_midi.py`: parseo de note on/off, sustain, bend con puertos
  virtuales de rtmidi + `aconnect`; learn asigna y persiste.
- Harness offline: `python -m nomad.render --demo <nombre> out.wav` para
  escuchar sin GUI. **Es una herramienta de desarrollo, no un formato de
  patch**: los nombres de parámetros pueden cambiar y no hay presets (§4).
  Está en el repo porque escuchar es parte del test.

## 14. Fases

| # | Entrega | Criterio de aceptación |
|---|---|---|
| 1 | Esqueleto: params, osc, ADSR, filtro, 1 voz, stream, GUI mínima | Suena una nota por 3 s sin clicks; scope muestra el seno real |
| 2 | Polifonía + robo de voces | Acorde de 8 notas, la 9ª roba sin click; tests verdes |
| 3 | 2º oscilador + mixer + sync + PWM | Detune audible (batido), sync audible, PWM audible |
| 4 | Filtro resonante + envelope de filtro | Barrido de cutoff con resonancia audible; autoscila al máximo |
| 5 | LFOs + matriz | Modulación de pitch/cutoff/pw desde la GUI |
| 6 | MIDI + MIDI learn | Tocar desde el teclado, bend, sustain, aprender un CC |
| 7 | GUI final + scope mejorado | Panel completo en 1366×768, trigger estable |

Cada fase termina con tests verdes, la app arrancando, y un commit en
`feature/nomad-synth`. Si una fase rompe la anterior, no avanza.

## 15. Riesgos

- **Costo de redibujado de tkinter**: ~35 perillas + scope a 30 fps. Mitigación:
  redibujar sólo el arco de la perilla que cambia y el scope; medir antes de
  agregar más decoración.
- **Latencia de PipeWire**: el quantum puede subir y el callback llega con más
  frames. Mitigación: engine agnóstico al tamaño de bloque (§9.1) y contador
  propio de xruns. El dispositivo ALSA `pipewire` (idx 5) quedó descartado por
  la medición, así que el default es el host API JACK.
- **Autoscilación inestable** en cutoff bajo: el limitador de estado tiene que
  testearse en todo el rango, con `k` máximo y cutoff mínimo (donde el polo
  está más cerca del borde).
- **python-rtmidi necesita compilador** en la máquina. Ya verificado acá;
  queda anotado en el README por si se reinstala el venv.
- **Sin hardware MIDI**: todo el camino MIDI se prueba con puertos virtuales;
  la primera prueba con el teclado real queda pendiente del usuario.

## 16. Lo que queda a criterio de quien implemente

- Nombres exactos de las perillas y su orden dentro de cada sección.
- Paleta exacta dentro de la estética actual (fondo casi negro, verde como
  acento).
- Forma de los selectores de onda (botones vs. dibujos de la onda).
- Si el teclado en pantalla es de 2 o 3 octavas.

## 17. Anexo: mediciones del prototipo

- 8 voces, 2 osciladores saw con PolyBLEP, filtro de 4 polos cada 64 muestras:
  **1.25 ms / bloque de 256** (presupuesto 5.33 ms, 23% de carga) en esta
  máquina.
- Coeficientes del filtro para 8 voces × 4 granos en una pasada vectorizada:
  **0.17 ms / bloque**.
- Filtro a 1 kHz: `k=0` → 0 dB planos; `k=2` → pico -1.7 dB @ 820 Hz;
  `k=3.9` → pico +23 dB @ 993 Hz, |z| máximo 0.9992.
- `k=4.6` sin limitador diverge (esperado); con limitador de estado queda
  acotado en 0.5 y oscila a 456 Hz con cutoff en 440 Hz.
- Calidad de la autoscilación (fc=440 Hz, k=4.6, energía fuera de la banda de
  ±8 Hz de la fundamental): grano 16 → **−9.5 dB**, grano 32 → −5.2 dB,
  grano 64 → −0.2 dB. De ahí la regla de grano 16 por encima de `k=3.2`.
  El desvío de pitch es **+3.6%** y constante en 110/440/2000 Hz.
- Stream: idx 5 (ALSA `pipewire`) → 1 callback en 400 ms; idx 6 (JACK/PipeWire)
  → 76 callbacks en 0.4 s con bloques de 256 y sin errores.

## 18. Decisiones surgidas del chequeo de construcción

Un revisor independiente leyó el documento antes de esta entrega. Estas son
las decisiones que tomé para cerrar sus preguntas, para que las revises:

1. **Filtro**: la autoscilación es la versión con limitador de estado a grano
   16, no un `tanh` por muestra. Suena rasposa a propósito. El `tanh` real
   queda para el engine en C.
2. **Tamaño de bloque**: el engine es agnóstico a la cantidad de frames; parte
   todo en granos de 16. No se asume 256.
3. **Camino de audio**: default al host API JACK (idx 6), no al plugin ALSA
   `pipewire` (idx 5), que midió 1 callback en 400 ms.
4. **LFOs**: globales y compartidos, no uno por voz. Vibrato coherente en
   acordes; la perilla de fase tiene sentido.
5. **MIDI learn**: clave (puerto, canal, CC) con fallback a (canal, CC).
6. **Modo free**: por oscilador, no global; las notas siguen abriendo la voz.
7. **Ondas**: cuatro; `pw` actúa sobre el square (que pasa a pulse). No hay
   quinta forma.
8. **Render offline**: herramienta de test, no formato de patch. Los nombres
   de parámetros no son contrato público.
