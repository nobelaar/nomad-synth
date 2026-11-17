"""Help-mode texts: one (title, body) per parameter and per panel element.

Texts describe what the engine really does (voice.py, engine.py, lfo.py,
modmatrix.py); keep them in sync when the engine changes. The test in
test_help.py fails if a visible parameter has no text.
"""

from __future__ import annotations

from nomad.engine.drums import INSTRUMENTS

_OSC = {
    "wave": ("Forma de onda",
             "La forma básica del sonido del oscilador.\n"
             "• SIN: puro, sin armónicos (flauta, sub-bajo).\n"
             "• TRI: suave, pocos armónicos impares.\n"
             "• SAW: brillante, todos los armónicos (cuerdas, bajos, leads).\n"
             "• SQR: hueco, armónicos impares (clarinete). Es la única onda en la que "
             "actúa PW."),
    "mode": ("Modo de afinación",
             "Decide de dónde sale la frecuencia del oscilador.\n"
             "• KEY: sigue la tecla que tocás. Afinás con COARSE y FINE; el pitch bend "
             "también actúa. HZ no hace nada.\n"
             "• FREE: frecuencia fija en hercios, la pone HZ. Ignora la tecla, COARSE "
             "y el pitch bend; FINE y la matrix (PIT y P1/P2) siguen actuando.\n"
             "FREE sirve para drones, efectos y FM con una frecuencia fija."),
    "coarse": ("Coarse (semitonos)",
               "Transpone el oscilador en semitonos, de −24 a +24 (dos octavas). +12 es "
               "una octava arriba, +7 una quinta.\n"
               "Solo actúa en modo KEY. En FREE no hace nada: ahí manda HZ.\n"
               "Si muestra −0 o +0 está apenas corrido de cero (el valor es continuo); "
               "doble click lo deja en 0 exacto."),
    "hz": ("Hz (frecuencia libre)",
           "Frecuencia fija del oscilador, de 20 Hz a 8 kHz.\n"
           "Solo actúa en modo FREE: el oscilador suena siempre a esa frecuencia, "
           "toques la tecla que toques. En KEY no hace nada.\n"
           "La perilla es logarítmica: el mismo giro sube la misma cantidad de octavas "
           "en todo el recorrido."),
    "fine": ("Fine (cents)",
             "Desafinación fina en cents, de −50 a +50 (un semitono = 100 cents).\n"
             "Actúa en KEY y en FREE. Desafinar un poco un oscilador contra el otro da "
             "batido y un sonido más ancho (unos 5 a 10 cents)."),
    "pw": ("Ancho de pulso (PW)",
           "Proporción de la onda cuadrada que queda arriba, del 5 % al 95 %. El 50 % "
           "es la cuadrada hueca; hacia los extremos suena más fina y nasal.\n"
           "Solo actúa con la onda SQR.\n"
           "Modularlo con un LFO lento desde la matrix (destino PW1/PW2) da el clásico "
           "PWM que suena a coro."),
    "level": ("Nivel en el mixer",
              "Volumen del oscilador antes del filtro. Los dos niveles se suman y van "
              "juntos al filtro.\n"
              "Con los dos altos, el filtro y el limitador de salida saturan más."),
}

_LFO = {
    "wave": ("Forma del LFO",
             "Cómo se mueve la modulación en el tiempo.\n"
             "• SIN / TRI: ida y vuelta suave (vibrato, trémolo).\n"
             "• SAW: rampa que sube y salta abajo.\n"
             "• SQR: salta entre dos valores (trinos).\n"
             "• S&H: un valor al azar nuevo en cada ciclo (efecto «computadora»)."),
    "rate": ("Velocidad del LFO",
             "Cuántos ciclos por segundo hace, de 0,02 Hz (uno cada 50 s) a 40 Hz.\n"
             "Debajo de 1 Hz da movimientos lentos y barridos; de 4 a 7 Hz, vibrato; "
             "arriba de 20 Hz empieza a ensuciar el timbre."),
    "depth": ("Profundidad del LFO",
              "Cuánto se mueve la salida del LFO, de 0 a 100 %. Por defecto está en 0: "
              "el LFO no hace nada hasta que subís esto Y lo ruteás en la matrix.\n"
              "Se multiplica con AMT del slot de la matrix."),
    "phase": ("Fase del LFO",
              "Corre el punto del ciclo en el que está el LFO, de 0 a 1 ciclo.\n"
              "El LFO es uno solo para todas las voces y corre siempre: no vuelve a "
              "empezar con cada nota. Sirve para desfasar LFO1 contra LFO2 cuando "
              "tienen la misma velocidad."),
}

_ENV = {
    "attack": ("Attack",
               "Tiempo que tarda en subir de 0 al máximo cuando apretás la tecla, de "
               "1 ms a 5 s. Corto = golpe; largo = entra de a poco (pads)."),
    "decay": ("Decay",
              "Tiempo que tarda en bajar del máximo al nivel de SUSTAIN mientras mantenés "
              "la tecla, de 1 ms a 5 s."),
    "sustain": ("Sustain",
                "Nivel en el que se queda mientras mantenés la tecla, de 0 a 100 %. Es un "
                "nivel, no un tiempo. En 0, el sonido muere después del decay aunque "
                "sigas apretando."),
    "release": ("Release",
                "Tiempo que tarda en apagarse cuando soltás la tecla, de 5 ms a 8 s. "
                "Largo = cola que sigue sonando."),
}

_SRC = ("Fuente de modulación",
        "Qué señal mueve este slot.\n"
        "• -: slot apagado.\n"
        "• LFO1 / LFO2: los LFOs (necesitan DEPTH arriba de 0). Son globales: mueven "
        "todas las voces juntas.\n"
        "• MW: la rueda de modulación del controlador MIDI.\n"
        "• VEL: la fuerza con la que tocaste cada nota (por voz).\n"
        "• ENV2: la envolvente del filtro de cada voz (FILT ENV), como fuente extra.")
_DST = ("Destino de modulación",
        "Qué parámetro mueve este slot.\n"
        "• -: slot apagado.\n"
        "• PIT: afinación de los DOS osciladores juntos (vibrato).\n"
        "• P1: afinación solo del oscilador 1.\n"
        "• P2: afinación solo del oscilador 2.\n"
        "• AMP: volumen (trémolo).\n"
        "• CUT: corte del filtro (barridos, «wah»).\n"
        "• PW1 / PW2: ancho de pulso de cada oscilador (solo con onda SQR).")
_AMT = ("Cantidad de modulación (AMT)",
        "Cuánto y hacia dónde mueve la fuente al destino, de −100 % a +100 %. "
        "Negativo invierte la modulación.\n"
        "Al 100 % con la fuente al máximo: PIT/P1/P2 mueven una octava, CUT dos "
        "octavas, PW1/PW2 un 45 % de ancho y AMP duplica o apaga el volumen.\n"
        "Para vibrato sutil alcanzan valores chicos (2 a 5 %).\n"
        "Si varios slots van al mismo destino, se suman.")

_FIXED = {
    "osc2_sync": ("Hard sync",
                  "Con SYNC, el oscilador 2 vuelve a empezar su ciclo cada vez que el "
                  "oscilador 1 empieza el suyo. El 2 queda atado a la afinación del 1 y "
                  "su propia frecuencia cambia el timbre en lugar de la nota.\n"
                  "Para el sonido típico: OSC2 más agudo que OSC1 y moverlo con P2 en la "
                  "matrix (por ejemplo desde ENV2)."),
    "filt_cutoff": ("Cutoff del filtro",
                    "Frecuencia de corte del filtro pasa-bajos (ladder de 4 polos, "
                    "24 dB/oct), de 20 Hz a 18 kHz. Abajo de eso deja pasar, arriba "
                    "recorta: bajarlo oscurece el sonido.\n"
                    "Lo mueven también ENV AMT (con FILT ENV) y CUT en la matrix."),
    "filt_reso": ("Resonancia",
                  "Realza las frecuencias justo en el corte, de 0 a 100 %. Medio = sonido "
                  "más «chillón»; alto = silbido que casi suena solo.\n"
                  "Con resonancia alta el volumen puede subir mucho; el limitador de "
                  "salida lo contiene."),
    "filt_env_amt": ("Env amt del filtro",
                     "Cuánto mueve FILT ENV al cutoff en cada nota, de −100 % a +100 %. Al "
                     "100 % la envolvente sube el corte hasta 5 octavas; negativo lo "
                     "baja.\n"
                     "En 0 la envolvente del filtro no actúa sobre el corte (igual "
                     "puede usarse como ENV2 en la matrix)."),
    "master_volume": ("Volumen general",
                      "Volumen de salida. Pasa por un limitador suave: arriba de cierto "
                      "punto no sube más, satura (comprime y agrega armónicos)."),
    "polyphony": ("Polifonía",
                  "Cuántas notas pueden sonar a la vez, de 1 a 8. Con 1 es monofónico.\n"
                  "Si tocás más notas que voces, se roba la más vieja (primero las que "
                  "están en release)."),
    "bend_range": ("Rango de pitch bend",
                   "Cuántos semitonos mueve la rueda de pitch bend del controlador al "
                   "extremo, de 1 a 12. 2 es lo habitual; 12 = una octava.\n"
                   "No actúa sobre un oscilador en modo FREE."),
    "bpm": ("Tempo de la batería",
            "Velocidad del secuenciador en negras por minuto, de 60 a 180. Cada paso es "
            "una semicorchea: los 16 pasos del patrón son un compás de 4/4.\n"
            "Cambiarlo mientras suena re-temporiza el resto del paso en curso, sin "
            "repetir el golpe."),
    "swing": ("Swing de la batería",
              "Retrasa los pasos impares y acorta los pares, de 0 % (recto) a 60 % "
              "(muy shuffle).\n"
              "El golpe fuerte sigue en el pulso; el débil se corre hacia atrás como en "
              "el jazz o el funk."),
}

_DRUMS = {
    "tune": ("Afinación del tambor",
             "Mueve la frecuencia base del instrumento, de −100 % a +100 %.\n"
             "En bombo y tom cambia la nota del golpe; en caja, clap y hi-hats cambia el "
             "carácter, de más opaco a más filoso."),
    "decay": ("Duración del golpe",
              "Largo de la caída del instrumento, de 0 % (corto y seco) a 100 % (cola "
              "larga).\n"
              "En el hi-hat abierto, subirlo lo deja sonando varios pasos y emborrona el "
              "patrón; es parte del sonido."),
    "level": ("Nivel del tambor",
              "Volumen de este instrumento en la mezcla, de 0 a 100 %.\n"
              "La batería se suma a las voces del sintetizador y pasa por el mismo master "
              "y limitador: si un patrón lleno satura, bajá el bombo o la caja."),
}

_ELEMENTS = {
    "help": ("Modo ayuda",
             "Pasá el mouse por encima de cualquier control para ver qué hace. Mientras "
             "está prendido, los controles no responden.\n"
             "Salís con este botón, F1 o Escape."),
    "learn": ("MIDI learn",
              "Arma el último control que tocaste: el próximo CC que muevas en el "
              "controlador queda asignado a ese parámetro. Click de nuevo para cancelar.\n"
              "Más cómodo: click derecho en el control → MIDI learn."),
    "audio": ("Salida de audio",
              "Dispositivo de audio en el que suena el synth. Cada click pasa al "
              "siguiente."),
    "midi": ("Entrada MIDI",
             "Puerto MIDI del que llegan las notas y los CC. Cada click vuelve a buscar "
             "puertos y pasa al siguiente (sirve si enchufaste el teclado después)."),
    "voices": ("Voces activas",
               "Cuántas teclas están apretadas ahora. No cuenta las notas que se están "
               "apagando en release."),
    "xruns": ("Xruns",
              "Cortes de audio: veces que el motor no llegó a tiempo. Si sube, bajá la "
              "polifonía o cerrá otros programas."),
    "keyboard": ("Teclado en pantalla",
                 "Click para tocar; arrastrando hacés glissando. Se ilumina con las "
                 "notas que llegan por MIDI. Toca con fuerza (VEL) fija."),
    "scope": ("Osciloscopio",
              "La forma de onda real que sale del synth, ya filtrada. Se dispara en un "
              "cruce por cero para que la imagen quede quieta."),
    "synth_view": ("Vista del sintetizador",
                   "Vuelve al panel de perillas del synth: osciladores, filtro, "
                   "envolventes, LFOs y matriz.\n"
                   "La batería sigue sonando si estaba en PLAY."),
    "drums_view": ("Vista de la batería",
                   "Cambia a la drum machine: seis instrumentos, patrón de 16 pasos, "
                   "tempo y swing.\n"
                   "El scope y la sección global de la derecha no cambian; el teclado en "
                   "pantalla se esconde."),
    "drum_steps": ("Pasos del secuenciador",
                   "Click prende o apaga el paso; shift+click lo marca como acento (suena "
                   "más fuerte).\n"
                   "La columna iluminada es el paso que está sonando."),
    "drum_transport": ("Transporte de la batería",
                       "PLAY/STOP arranca o para el patrón (STOP vuelve al paso 1).\n"
                       "BPM y SWING cambian el groove en vivo; CLEAR borra todos los pasos."),
}
_ELEMENTS.update({
    "clock_source": ("Fuente de reloj",
                     "Internal: este instrumento marca su propio tempo. Si elegís otro instrumento, "
                     "hereda su BPM, su posición y su PLAY/STOP, y ya no podés editarlos aquí. Cambiar "
                     "de fuente es inmediato: puede saltarse o repetirse un paso. Un instrumento que otros "
                     "siguen no puede seguir a otro."),
    "inst_mute": ("MUTE",
                  "Silencia este instrumento con un fade corto, sin detenerlo: sigue su reloj y conserva su "
                  "estado. Es independiente del PLAY."),
    "inst_level": ("LEVEL del instrumento",
                   "Volumen de este instrumento dentro de la mezcla (0 a 100 %). No es el volumen general: "
                   "ese es MASTER, que afecta a todo."),
    "inst_bpm": ("BPM del instrumento",
                 "Tempo de este instrumento mientras usa su reloj Internal. Si sigue a otro, muestra el "
                 "tempo de la fuente y queda bloqueado."),
    "tab_select": ("Pestaña de instrumento",
                   "Cada pestaña es un instrumento independiente que sigue sonando aunque no esté "
                   "seleccionada. Click elige cuál editás; el teclado en pantalla y el MIDI van al "
                   "Synth seleccionado."),
    "tab_close": ("Cerrar instrumento",
                  "Cierra este instrumento: sus notas se apagan con un fade corto. Los instrumentos "
                  "que lo seguían como reloj pasan a reloj propio y continúan desde el mismo punto."),
    "tab_new": ("Nuevo instrumento",
                "Abre un menú para crear otro Synth o una Machine Drum. Cada instancia tiene sus "
                "propios parámetros, patrón y estado."),
    "seq_toggle": ("SEQ: secuenciador del synth",
                   "Prende o apaga el secuenciador de este synth. Toca sus notas con el mismo "
                   "motor que el teclado, así que podés tocar encima a la vez.\n"
                   "Al apagarlo se sueltan sus notas."),
    "seq_transport": ("PLAY / STOP del secuenciador",
                      "Arranca o para solo la secuencia del synth. La batería tiene su propio "
                      "PLAY en la vista DRUMS; las dos usan el BPM que se fija allí."),
    "seq_steps": ("Pasos de la secuencia",
                  "Click prende o apaga el paso; shift+click lo marca como acento.\n"
                  "La rueda del mouse cambia la nota (shift: una octava). "
                  "El paso iluminado es el que suena."),
    "seq_rec": ("REC: grabar notas en la secuencia",
                "Armalo y tocá el teclado (MIDI o el de pantalla). Con el reloj parado graba por pasos: "
                "cada nota se escribe en el paso marcado y avanza al siguiente. Con la secuencia "
                "sonando graba en vivo, cuantizada al paso más cercano. Volvé a tocarlo para salir."),
    "seq_rest": ("REST: silencio en grabación por pasos",
                 "Deja el paso marcado vacío y pasa al siguiente. Solo con REC y el reloj parado."),
    "seq_gate": ("GATE: duración de las notas",
                 "Cuánto dura cada nota respecto de su paso: poco es staccato, "
                 "al máximo las notas se pegan (legato)."),
})
for _track in INSTRUMENTS:
    _ELEMENTS[f"drum_{_track}"] = (
        f"{_track.upper()}: audición",
        "Click para escuchar este instrumento suelto, con el nivel y la afinación que "
        "tiene ahora.\n"
        "Sirve para ajustar tune y decay sin esperar a que el patrón pase por ese paso.")


def help_for(key: str) -> tuple[str, str]:
    """(title, body) for a parameter name or panel element; KeyError if unknown."""
    if key in _FIXED:
        return _FIXED[key]
    if key in _ELEMENTS:
        return _ELEMENTS[key]
    head, _, rest = key.partition("_")
    if head in ("osc1", "osc2") and rest in _OSC:
        t, b = _OSC[rest]
        return f"{t} — OSC{head[3]}", b
    if head in ("lfo1", "lfo2") and rest in _LFO:
        t, b = _LFO[rest]
        return f"{t} — LFO{head[3]}", b
    if head in ("amp", "filt") and rest in _ENV:
        t, b = _ENV[rest]
        which = ("AMP ENV: la envolvente del volumen de cada nota." if head == "amp" else
                 "FILT ENV: la envolvente que mueve el cutoff (según ENV AMT) y que la "
                 "matrix usa como ENV2.")
        return f"{t} — {head.upper()} ENV", f"{which}\n{b}"
    if head in INSTRUMENTS and rest in _DRUMS:
        t, b = _DRUMS[rest]
        return f"{t} — {head.upper()}", b
    if head.startswith("mod") and head[3:].isdigit():
        slot = int(head[3:]) + 1
        t, b = {"src": _SRC, "dst": _DST, "amt": _AMT}[rest]
        return f"{t} — slot {slot}", b
    raise KeyError(key)
