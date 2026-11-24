# NOMAD: servidor MCP dentro de la GUI

Fecha: 2026-11-23. Estado: borrador para aprobar.

## Intención

Que una IA (Claude Code u otro cliente MCP) pueda **ver y manejar el NOMAD que el usuario tiene abierto**: lo mismo que hace el usuario con el mouse (mover params, tocar notas, editar el secuenciador y los drums, cargar presets, abrir instancias y pestañas) mientras el usuario mira y escucha. La GUI y la IA comparten el mismo estado; lo que la IA cambia se ve moverse en pantalla.

## Metas

- Control con paridad de lo que la GUI ya permite (lista en "Herramientas").
- La IA lee el estado actual antes de decidir (no a ciegas).
- Cero cambios en `nomad-core` y `nomad-runtime`: el hilo de audio no se toca.

## No metas

- Capturas de pantalla o "ver" píxeles (la IA lee estado estructurado, no la imagen).
- Acceso desde otra máquina, autenticación, TLS.
- Modo sin ventana (headless) y transporte stdio. El stdio se puede agregar después con un puente, sin tocar esto.
- Grabar o exportar audio.

## Anti-metas (sería un fallo aunque "funcione")

- Que la IA pueda dejar notas colgadas o un volumen que dañe los oídos/parlantes sin que el usuario pueda cortarlo.
- Que una página web abierta en el navegador pueda controlar el sintetizador.
- Que la GUI se congele si el cliente MCP se cuelga.

## Restricciones encontradas en el código

- `AppModel` y `NomadApp` viven en el hilo de la UI y no son `Send` (`Rc<RefCell>`, closures `FnMut`, `ControlHandle` de un solo productor). El servidor no puede tocarlos desde otro hilo.
- Todo cambio de estado debe pasar por los mismos métodos que usa la UI (`add_instrument`, `set_bpm`, `toggle_play`, `presets::apply_commands`, `open_spiral`, ...) para que `ParamSync`, el espejo y las pestañas no se desincronicen.
- El estado legible ya existe en `InstrumentShared` (átomos) y `presets::capture` lo vuelca a JSON (params + patrón). La lectura reutiliza eso.
- La UI se repinta cada 33 ms (`request_repaint_after`), pero con la ventana minimizada o tapada eso puede frenarse. El servidor guarda el `egui::Context` (es `Send`; sale de `cc.egui_ctx` en `run_native`) y llama `request_repaint()` al encolar cada petición.
- Un cambio externo de un param se refleja solo en el knob: `ParamSync::observe` adopta el valor del espejo salvo durante `HOLD` (150 ms) tras un gesto del usuario. Si el usuario mueve ese mismo knob justo a la vez, gana el usuario.
- Play/stop/bpm de una instancia con fuente de reloj externa (follower) son no-op en `AppModel` (`own_clock`). Esas herramientas devuelven un error que lo explica.
- Los parámetros de drums `tune`/`decay` no son un `SetParam` simple: hay que renderizar el one-shot fuera del hilo de audio y mandar `DrumSetBank` (`drums_view.rs:268`, `presets.rs:226`).

## Enfoques considerados

| | Qué es | Trade-off |
|---|---|---|
| **A. HTTP local en la GUI** (elegido) | La GUI abre `127.0.0.1:<puerto>`, MCP sobre HTTP. Se registra con `claude mcp add --transport http`. | Una sola pieza. Hay que fijar puerto y blindar localhost. |
| B. Socket Unix + puente `nomad mcp` | La GUI escucha en un socket; un subcomando traduce stdio. | Dos piezas; sirve a clientes solo-stdio. Posible extensión futura. |
| C. El cliente lanza la GUI por stdio | NOMAD como proceso hijo del cliente. | Rompe "con la GUI que ya tengo abierta". Descartado. |

El usuario eligió **A**.

## Diseño

```
Claude ──HTTP POST /mcp──▶ hilo servidor ──mpsc(Request{json, reply})──▶ NomadApp::ui (cada frame)
                           (tiny_http)    ◀──────── reply (oneshot) ────── aplica con los métodos de siempre
```

1. **Arranque opt-in:** `nomad gui --mcp [PUERTO]` (puerto por defecto fijo, p. ej. 7777). Sin el flag no se abre nada. Si el puerto está ocupado, la GUI arranca igual y lo muestra en la barra de estado.
2. **Hilo servidor:** `tiny_http` (síncrono, sin tokio), enlazado solo a `127.0.0.1`. Rechaza con 403 cualquier petición cuyo `Host` u `Origin` no sea localhost (defensa contra DNS rebinding). Habla JSON-RPC de MCP "Streamable HTTP" en su forma mínima: responde `application/json`, sin SSE.
3. **Métodos MCP implementados a mano:** `initialize`, `ping`, `tools/list`, `tools/call`; las notificaciones (`notifications/*`) devuelven 202. `GET` y `DELETE` en `/mcp` devuelven 405 (el cliente puede intentar abrir un stream SSE; la especificación lo permite). `serde_json` ya es dependencia.
4. **Traspaso a la UI:** cada `tools/call` se envía por un canal `std::sync::mpsc` con un canal de respuesta y una bandera `cancelled` (`AtomicBool`). `NomadApp::ui` llama `mcp.drain(self)` una vez por frame, ejecuta la herramienta con los métodos de `AppModel`/`NomadApp` y responde. El servidor espera máximo ~2 s; si la UI no contesta, marca `cancelled` y devuelve un error de herramienta. `drain` descarta las peticiones canceladas, para que un reintento del cliente no se aplique dos veces. El servidor atiende una petición a la vez (suficiente para un cliente).
5. **Errores:** parámetros o instancias inválidos devuelven `isError: true` con un mensaje que la IA pueda corregir (nombres de params válidos, ids existentes). Nada hace `panic`.
6. **Seguridad de sonido (mi decisión):** `note_on` lleva `duration_ms` (por defecto 500, máximo 10000) y `NomadApp` guarda los `note_off` pendientes y los emite cuando vencen (resolución de un frame, ~33 ms). Si el cliente se cae, las notas igual se sueltan. `note_on` va al synth objetivo MIDI actual, como el teclado; `panic` está siempre disponible como herramienta; los valores se sanean con los rangos de `SYNTH_PARAMS` antes de enviarse.

## Herramientas v1

**Leer**
- `get_state`: instancias (id, tipo, nombre, bpm, play/stop, mute, nivel, paso actual, clock source), instancia seleccionada y objetivo MIDI, master, pestañas abiertas, telemetría básica (peak, voces, xruns).
- `get_instrument(id)`: el JSON de `presets::capture` (todos los params con nombre y valor + patrón/pasos).
- `list_params(kind)`: nombres, rango y valor por defecto de cada param.
- `list_presets(kind)`.

**Escribir** (todas pasan por la UI)
- Instancias: `add_instrument(kind)`, `close_instrument(id)`, `select_instrument(id)`, `set_target(id)`, `set_source(id, src)`.
- Sonido: `set_param(id, name, value)` (solo synths), `set_master`, `set_level`, `set_mute`.
- Transporte: `set_bpm`, `play(id)` / `stop(id)` (idempotentes: miran `clock_running` y solo llaman `toggle_play` si hace falta).
- Tocar: `note_on(note, velocity, duration_ms)`, `panic`, `drum_trigger(id, track, velocity)`.
- Secuenciador: `seq_set_step`, `seq_clear`, `seq_set(length, gate, division, swing)`; drums: `drum_set_step`, `drum_clear`. Los params de drums (`tune`, `decay`, ...) quedan fuera de v1 como herramienta suelta; se cambian cargando un preset (que ya renderiza los one-shots). Si se quieren después, deben reutilizar la ruta de `drums_view.rs`.
- Presets: `load_preset(id, name)`, `save_preset(id, name)`.
- Pestañas: `open_spiral`, `close_spiral` (y las instancias nuevas ya abren su pestaña al crearse).

## Pruebas

- Capa JSON-RPC (initialize, tools/list, tools/call con error, 403 por Host/Origin ajeno) con el servidor real en un puerto efímero.
- Cada herramienta contra una `NomadApp` de prueba (se construye con `NomadApp::new` como en `tests/ui_synth.rs` y `tests/presets_ui.rs`, con `scene::demo_engine`): se envía la petición, se corre un frame, se comprueba el estado del modelo y el espejo.
- Verificación manual final: `claude mcp add`, pedirle a Claude que cree un synth, cargue un preset y lo toque.

## Qué se deja al implementador

- Versión exacta del protocolo MCP a negociar (verificar contra la especificación vigente al implementar).
- Nombres exactos de las herramientas y forma de los esquemas JSON.
- Puerto por defecto y dónde mostrar el estado del servidor en la barra de estado.

## Decisiones tomadas por mí (para revisar)

- `tiny_http` y JSON-RPC a mano en vez del SDK oficial `rmcp` (que arrastra `tokio`); la superficie necesaria es pequeña.
- Servidor desactivado por defecto, activado con `--mcp`.
- Lectura también pasa por la cola de la UI (una sola ruta), aunque `InstrumentShared` se podría leer directo.
- Duración máxima forzada en `note_on` y `panic` siempre expuesto.
