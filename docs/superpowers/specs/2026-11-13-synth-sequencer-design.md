# Secuenciador por instancia de Synth — diseño

Fecha: 2026-11-13. Estado: **pendiente de aprobación**.

## Intención
NOMAD evoluciona hacia instrumentos independientes (pestañas) que comparten un transporte. Primer paso: un secuenciador de notas que pertenece a cada Synth, activable desde su propio panel, que solo emite eventos de nota al motor existente.

## Metas
- Synth reproduce una secuencia interna, recibe MIDI, o ambas a la vez, por el mismo `VoiceManager`, envolventes y filtros.
- `Transport` global (BPM, swing, play, posición) que Synth y Drums consumen.
- Sin asignaciones ni bloqueos en el callback; GUI separada del audio.

## No-metas
Pestañas dinámicas con selector de instrumento, destino MIDI por pestaña, segundo Synth en la UI, acordes por paso, grabación, persistencia, Sampler.

## Anti-metas
Una ruta de audio propia del secuenciador; duplicar el motor; romper el comportamiento actual de synth y drums.

## Enfoques considerados (decididos por el usuario)
1. Instancias: **A** extraer `SynthInstrument` del `Engine` ahora (B, meterlo en el singleton, descartado: hay que moverlo después).
2. Reloj: **A** extraer `Transport` y que Drums lo consuma (B, dos relojes, descartado: sincronía solo aparente).
3. PLAY: global (decisión mía, no confirmada): un PLAY arranca transporte, drums y secuencias.

## Diseño
```
Transport            bpm, swing, playing, position, phase (por muestra)
Engine (host)        lista de instrumentos, suma, master tanh, cola de comandos
 ├ SynthInstrument   VoiceManager + params/smoothers + LFO + ModMatrix  (hoy dentro de Engine)
 │   └ StepSequencer 16 pasos monofónicos; emite note_on/off
 └ DrumMachine       lee Transport; conserva bank/steps
```
- **Transport:** algoritmo de fase de `DrumMachine.render` extraído tal cual (chunk partido en el borde del paso, swing por paso par/impar). `Engine.render` consulta `transport.frames_to_next_step()` y acorta el bloque de control en ese borde, para que las notas caigan sample-accurate. Smoothers y LFO avanzan con el `n` real.
- **StepSequencer:** arrays preasignados `on/note/vel/gate` (uint8), escritos por la GUI con stores atómicos (estilo `DrumMachine.steps`, sin cola). En cada paso llama `synth.vm.note_on`; el note_off sale tras `gate` × duración del paso. Llama directo al `VoiceManager` desde el hilo de audio porque `SimpleQueue.put` asigna.
- **Colisión MIDI/secuencia:** `note_off` libera por número de nota. El secuenciador omite su note_off si esa nota está sostenida en vivo (atajo documentado `shortcut:`; solución completa = etiquetar el origen de cada voz).
- **Panic/stop:** `Engine.panic` también resetea secuenciadores y transporte; stop envía note_off de lo que el secuenciador mantiene.
- **Parámetros:** el registro pasa a ser por instancia (`SynthInstrument` posee sus valores y cola). `Engine.set_param` sigue funcionando sobre la instancia por defecto para no romper MIDI learn ni tests.
- **MIDI:** `MidiRouter` apunta a un instrumento destino (hoy el único).
- **GUI:** en el panel Synth, toggle `SEQ` y un editor de 16 pasos (nota, velocidad, gate). El botón PLAY de Drums pasa a controlar el Transport. Las pestañas dinámicas quedan para después.

## Hallazgos del self-review (verificados en el código)
- MIDI learn (`mapping.py`) depende solo de `REGISTRY`, no de `Engine`: el riesgo del refactor es menor al supuesto.
- `tests/test_main.py` usa `rt.engine.vm` y la GUI usa `engine.drums`: `Engine` conserva `.vm` y `.drums` (apuntan a la instancia por defecto).
- `tests/test_drums.py` construye `DrumMachine` solo, y la GUI usa `position/playing/toggle/set_value("bpm"|"swing")`: `DrumMachine` crea su propio `Transport` si no se le inyecta uno y mantiene esa API (propiedades que delegan). `Engine` inyecta el compartido.
- Los tests de salto de BPM/swing a mitad de paso (`test_drums.py:240+`) son la red de seguridad de la extracción del reloj; se corren antes y después de mover el algoritmo.
- `bpm`/`swing` siguen en `DRUM_REGISTRY` (la GUI de drums y su ayuda dependen de eso); el `Transport` es el dueño del valor.

## Llamadas del plan que ajustan este diseño
- El gate es un valor único por secuenciador (no por paso); por paso van nota y velocidad (acento 127).
- `Transport.run(n, render_span, on_step, limit)` es el único driver del reloj: Engine y `DrumMachine` standalone lo usan.

## Compatibilidad
`Engine.note_on/note_off/set_param/set_control/panic/snapshot/render` mantienen firma y delegan a la instancia por defecto. Los tests existentes deben pasar sin cambios salvo los que toquen internals de `Engine`.

## Riesgos
- Refactor de `engine.py` toca parámetros y MIDI learn (no verificado si dependen de un engine único).
- Drums sobre `Transport`: regresión posible en la fase tras saltos de tempo (commit `8f2e672`); `test_drums` es la red.
- CPU: dos synths duplican el peor caso (R9).

## Delegado al implementador
Nombres de módulos (`nomad/engine/transport.py`, `sequencer.py`, `instrument.py`), resolución de ticks, rango de notas del editor, estética del editor de pasos.
