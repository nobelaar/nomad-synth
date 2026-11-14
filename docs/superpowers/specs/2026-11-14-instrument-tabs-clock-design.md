# Pestañas de instrumentos y reloj compartido — diseño

Fecha: 2026-11-14. Estado: **aprobado por el usuario con cuatro precisiones (incorporadas abajo); implementación por etapas**. Cubre las secciones 1–11 del pedido del usuario (pestañas, instancias independientes, mezcla, clock master).

## Intención
NOMAD pasa de un instrumento con vistas a un entorno que aloja varias instancias independientes (Synth, Machine Drum) que suenan a la vez, con una barra de pestañas para editarlas y un reloj compartible entre ellas.

## Metas
Pestañas con `+` (elegir tipo), cambio y cierre; N Synths y N Machine Drums con estado propio; mezcla en un master; cambiar de pestaña no toca el audio; cada pestaña elige `Clock Source`: Internal o una instancia en Internal; cerrar/crear instancias sin cortar el audio.

## No-metas
Cadenas de sincronización, MIDI Clock/Link, piano roll, efectos nuevos, persistencia, routing MIDI avanzado, tope de instancias, edición de división/largo de patrón en la UI.

## Enfoque decidido por el usuario
`Clock` como objeto compartido por referencia (no un transporte global obligatorio ni relojes independientes que se sincronizan por copia). Comportamientos aceptados: la instancia sincronizada hereda BPM, posición y transporte; su PLAY queda deshabilitado; si la fuente desaparece pasa a Internal conservando BPM y posición musical y continúa; si la fuente se detiene, los seguidores también; solo instancias Internal son fuentes; PLAY/STOP y MUTE son controles independientes (una instancia sincronizada no controla su transporte pero sí puede silenciarse); compartir clock no comparte patrones, longitud ni subdivisión; al cambiar de fuente se preserva la continuidad y no se reinician patrones.

## Arquitectura
```
InstrumentRegistry  tipo → fábrica de motor + fábrica de panel (GUI), módulos separados
InstrumentManager   (hilo GUI) autoridad sobre el grafo: add / remove / set_source / lista; numera nombres
Engine              (hilo audio) dueño de todo el estado de audio; suma instancias → tanh
Clock               beats absolutos (float64), bpm, running
TabBar / paneles    GUI; un panel persistente por instancia (mostrar/ocultar)
```
- **Instancia:** `id`, `type`, `name`, `clock` propio (siempre reservado), `source` (None = Internal), `muted`, estado propio. Protocolo de motor: `drain()`, `render_block(b, n)`, `frames_to_event()`, `on_chunk_end()`, `panic()`.
- **Clock en beats:** la posición no depende del tempo, así que un salto de BPM es correcto sin lógica extra. Cada instrumento calcula sus límites de paso desde `pos` con su propia división, largo y swing (defaults 0.25 beat y 16 pasos; sin UI todavía). Reemplaza la fase por paso de `Transport`.
- **Reloj efectivo:** `source.clock` si sigue a otra instancia, si no el propio. Los seguidores leen el `Clock` de la fuente; no integran su propio tiempo.

## Propiedad y ciclo de vida de los Clock (verificado en modelo)
1. Cada instancia posee su `Clock` durante toda su vida, y solo el motor lo avanza. Un seguidor tiene el suyo congelado y sin avanzar.
2. **Avance único por bloque:** `Engine` avanza exactamente una vez por bloque cada Clock de las instancias en Internal (las "raíces"). Ninguna instancia avanza relojes. Los seguidores nunca figuran como raíz.
3. **Handover en el hilo de audio:** cuando una fuente desaparece, o un seguidor pasa a Internal, el hilo de audio copia `bpm`, `pos` y `running` de la fuente al reloj propio del seguidor, una sola vez, al adoptar la nueva topología y antes de avanzar. Los objetos ya están creados por el hilo GUI; copiar tres floats no asigna memoria. La continuidad es exacta, y no se reinicia el patrón.
4. **Seguir a una fuente:** el seguidor deriva su paso de `pos` y por tanto se alinea con la fuente sin reiniciar el patrón. Si la fuente está detenida, el seguidor se detiene y sus notas se sueltan.

## Publicación de topologías GUI → audio
- **Un solo canal FIFO** (`queue.SimpleQueue`, el mismo mecanismo que ya usa `SynthInstrument`) para topología, start, stop y bpm. El hilo GUI nunca escribe estado propiedad del audio; solo encola comandos. El hilo de audio los aplica en orden al comienzo de cada `render`. Esto elimina la carrera de `start()` que hoy escribe `playing`, `position` y `phase` en varias asignaciones.
- **Instantánea inmutable** (`Topology`: tupla de instancias, relojes efectivos, raíces), adoptada atómicamente como un comando más. El audio nunca ve un estado a medias.
- **Cementerio:** el hilo de audio devuelve la topología anterior al hilo GUI por otra cola, para que la última referencia a instancias cerradas se libere fuera del callback (liberar numpy/objetos en el callback es costoso y no determinista).
- **Relaciones uno-a-muchos permitidas:** una instancia Internal puede tener cualquier cantidad de seguidores a la vez (p. ej. Synth 01 sincroniza a Synth 02 y a Machine Drum). Lo que se evita son **cadenas**.
- **Validación (hilo GUI, en `set_source(inst, src)`)** rechaza solo: (a) `src` es la propia instancia; (b) `src` no es una instancia viva en Internal (seguir a un seguidor sería una cadena); (c) `inst` ya es fuente de otros (tiene seguidores) y quiere pasar a seguir a otra (la convertiría en eslabón de una cadena). Con eso el grafo tiene profundidad 1 y no hay ciclos. Un seguidor puede cambiar de fuente libremente. *(Una versión previa de este texto decía por error que se rechazaba toda fuente con seguidores; la regla modelada y probada es la de arriba.)*
- **Cierre sin click:** `panic()` con fade de 5 ms y baja de la topología ~100 ms después.
- **Python 3.14 con GIL activo:** la semántica de asignación de referencias y de `SimpleQueue` en la que se apoya esto es la actual.

Resultado del modelo (scratchpad, no es código de NOMAD): 8 s con un hilo de audio haciendo 1.2 M de renders de tamaño variable y un hilo GUI con ~1 M de operaciones aleatorias (crear, cerrar, cambiar fuente, start/stop, BPM, hasta 8 instancias): 0 violaciones de "avance único", "ningún seguidor sin reloj que avance" y "ninguna topología inválida". Una mutación que rompe la regla (avanzar dos veces) sí es detectada, así que los chequeos pueden fallar.

## Cambio de fuente: inmediato y sin cuantizar (comportamiento explícito)
El cambio de fuente (Internal ↔ instancia, o entre instancias) se aplica en el **siguiente bloque de audio**, sin esperar a un límite de paso ni de compás. El seguidor recalcula su próximo paso desde el `pos` del nuevo reloj. Consecuencias aceptadas y documentadas: puede haber **un salto de posición** (el patrón se alinea a la grilla de la nueva fuente) y **la repetición u omisión de un paso** en el instante del cambio. Las notas ya sonando terminan por su propio gate. Mejora futura prevista: **cambio cuantizado** al próximo paso, beat o compás; el comando de topología puede llevar luego un `at_beat` opcional sin cambiar el resto del diseño.

## Política de propiedad del estado entre GUI y audio
Regla: **un solo escritor por campo**. Todo estado pertenece a una de tres clases.
| Clase | Quién escribe | Quién lee | Ejemplos |
|---|---|---|---|
| **A. Propiedad del audio** | solo el hilo de audio | el audio; la GUI solo para mostrar, tolerando valores atrasados | `Clock.pos/bpm/running`, voces, ganancia actual de mute, instancia activa de la topología |
| **B. Publicado por la GUI** | solo el hilo GUI, con stores atómicos de escalares o de un elemento de array | el audio, solo lectura | pasos de patrón, `sequencer.on/note/vel/gate/enabled`, `DrumMachine.values` |
| **C. Comandos** | la GUI encola en la cola FIFO; el audio los aplica en orden al inicio del `render` | el audio | topología, start/stop/reset, BPM, mute, parámetros del synth, panic, `trigger` |
Todo lo que necesite ordenamiento o consistencia entre varios campos va por C. La GUI nunca lee estado clase A para decidir; si necesita el estado de intención (p. ej. "estoy en mute") lo guarda ella misma.

**Violaciones que existen hoy y se corrigen en las etapas** (escritores concurrentes sobre el mismo campo): `Transport.start/stop` desde la GUI vs. el avance en audio; `Engine.panic` llamando `drums.stop_all()` (escribe `_fade_left`) desde el hilo que llama; `DrumMachine.trigger` (GUI) escribiendo `_active` que el audio también muta. Pasan a comandos clase C.

## MUTE
- **Intención (GUI):** la GUI guarda `muted` (solo para pintar el botón) y encola el comando `("mute", instance_id, bool)`.
- **Estado (audio):** el hilo de audio aplica el comando y escribe `muted_target` (clase A). La **rampa de ganancia** (`gain` actual hacia 0 o 1, ~5 ms) también se calcula y escribe solo en el hilo de audio, bloque a bloque. La GUI no escribe ni lee la ganancia.
- Cada instancia mezcla en su propio buffer preasignado y el motor lo suma al bloque master multiplicado por la rampa. Una instancia silenciada sigue su reloj y conserva el estado; es independiente del transporte.

## Volumen: nivel de instrumento vs. master de la mezcla
Son dos conceptos distintos y el segundo **no cambia de significado**.
- **`master_volume` (existente) = volumen master de la mezcla.** Hoy ya se aplica después de sumar Synth + Machine Drum y antes del `tanh` (README: "comparten volumen, limitador y osciloscopio"). Sigue siendo exactamente eso: un único valor global del `Engine`, no por instancia. Se conserva `Engine.set_param("master_volume", ...)`, `snapshot()["master_volume"]`, el flag `--volume`, la ayuda y los bindings guardados de MIDI learn (todos van por esos nombres, verificado con grep).
- **`level` (nuevo) = volumen individual del instrumento.** Parámetro por instancia, rango 0..1, **default 1.0 (unidad)**: con una sola instancia el sonido es idéntico al actual. Se aplica en el hilo de audio a la salida de la instancia, antes de la rampa de mute y de la suma al master. Es la única adición de funcionalidad de esta aclaración, y es lo mínimo para distinguir ambos conceptos.
- **GUI:** el knob `master_volume` queda en el panel global de la derecha, fuera de las pestañas (es de la sesión). `polyphony` y `bend_range`, hoy en esa sección "global", son del Synth y pasan al panel de cada Synth. El `level` y el MUTE de cada instancia van en una franja pequeña de su pestaña.
- **Migración sin cambio silencioso:** `SynthInstrument` deja de tener `master_volume` entre sus parámetros; el `Engine` lo posee y lo suaviza como hoy (rampa lineal por bloque, test `test_master_volume_jump_has_no_click`). `REGISTRY` conserva la especificación para la GUI y MIDI learn.

## Integración con lo existente
- `SynthInstrument` y `DrumMachine` pasan a ser instancias sin reescribirse. `Engine` deja de tener la batería como caso especial y pierde `seq_transport`.
- `Transport` evoluciona a `Clock`; sus tests de saltos de BPM/swing se portan a beats. `DrumMachine` standalone conserva un `Clock` interno por defecto para no romper sus tests.
- `Engine.vm`, `.synth`, `set_param`, `snapshot` siguen apuntando a la primera instancia Synth.
- **GUI:** el panel de Synth se extrae de `App` a una clase por instancia, con sus propios widgets y SeqView. MIDI y teclado en pantalla van a la pestaña seleccionada (mínimo sin routing avanzado). El BPM sale de la vista DRUMS y se agrega al panel Synth. Los widgets se construyen por secciones con `root.after` para no retener el GIL (una `App` completa tarda 708 ms).

## Llamadas mías (decididas por falta de indicación, revisables)
- No hay tope de instancias.
- El tipo se muestra como "Machine Drum".
- `level` va de 0 a 1 (solo atenúa); subir por encima de la unidad queda para después.

## Riesgos
- Refactor de `Transport`, `DrumMachine`, `StepSequencer` y `Engine`, recién mergeados.
- Construir un panel Synth en caliente puede producir un xrun si Tk retiene el GIL; mitigado con construcción por secciones, **a medir con stream real**.
- Ancho de ventana: 1362 de 1366 px. Las pestañas reemplazan los botones SYNTH/DRUMS del encabezado y no deben agregar ancho ni una fila (740 de 768 px de alto).
- Más fuentes de eventos significan más bloques cortos; medí 0.98 ms por 256 frames en el peor caso con un Synth.

## No verificado
El modelo valida protocolo y propiedad, no el audio real. Falta medir xruns con un stream real al crear un panel, la CPU con varios Synth y el comportamiento del drift en sesiones largas.
