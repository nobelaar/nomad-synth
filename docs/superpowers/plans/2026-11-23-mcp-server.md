# NOMAD MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `nomad gui --mcp [PORT]` opens a local MCP server inside the running GUI so an AI can read and drive the same state the user sees.

**Architecture:** A `tiny_http` thread on `127.0.0.1` speaks minimal MCP (JSON-RPC over HTTP POST). Each `tools/call` crosses an `mpsc` channel to the UI thread; `NomadApp::ui` drains it once per frame and runs the tool with the same `AppModel`/`NomadApp` methods the mouse uses. `nomad-core` and `nomad-runtime` are not touched.

**Tech Stack:** Rust 2024, `tiny_http` 0.12 (new, the only new dependency), `serde_json` (already used), `eframe`/`egui` 0.37.

**Spec:** `docs/superpowers/specs/2026-11-23-mcp-server-design.md`

Work on a branch `feature/mcp` (the repo uses `feature/*` branches). All commands run from `/home/imn0p/synth-lab/rust`.

## Global Constraints

- Server bound to `127.0.0.1` only; off unless `--mcp` is passed.
- Requests whose `Host` header is not `127.0.0.1[:port]` or `localhost[:port]`, or whose `Origin` is present and not an `http://127.0.0.1`/`http://localhost` origin, get 403.
- `GET`/`DELETE` on `/mcp` return 405; notifications return 202 with an empty body; JSON-RPC batches (arrays) are rejected.
- A tool call waits at most 2 s for the UI; on timeout the request is marked cancelled and `drain` skips it.
- `note_on`: `duration_ms` default 500, max 10000; note 0..=127; velocity 0..=1.
- No `panic!`/`unwrap` on client input; invalid input returns a tool error (`isError: true`) with a message that says what is valid.
- No changes under `rust/nomad-core` or `rust/nomad-runtime`. No new dependency besides `tiny_http`. `cargo clippy --all-targets` stays clean.
- Tool names, argument names and error text are English (the audience is the AI); status-bar messages are Spanish like the rest of the app.
- Param names are the `name` fields of `SYNTH_PARAMS`, in real units (the same values `presets::capture` stores).

## Review Focus

1. `set_param` with unknown name, non-finite or out-of-range value: error listing valid names / value clamped by `ParamSpec::clamp`, never a crash (Task 5).
2. Any `id` that does not exist or is closing: error `no instrument with id N`, nothing sent (Tasks 4-7).
3. `note_on` with `note` 200, `duration_ms` 10^9, or the target synth changing before the `note_off` is due: rejected / clamped / no stuck note (Task 6).
4. Request with `Host: evil.example` or `Origin: http://evil.example`: 403 and the tool is not run (Task 2).
5. Client times out and retries: the first request is not applied twice (Task 3).
6. `play`/`stop`/`set_bpm` on a follower instance: error explaining that its transport belongs to its source (Task 5).

## File Structure

- Create `nomad-app/src/mcp/rpc.rs`: pure JSON-RPC/MCP message handling, no I/O.
- Create `nomad-app/src/mcp/http.rs`: the `tiny_http` server, header checks, status codes.
- Create `nomad-app/src/mcp/mod.rs`: `Mcp` handle, channel bridge, `drain`, pending note-offs.
- Create `nomad-app/src/mcp/tools.rs`: tool schemas and execution against `&mut NomadApp`.
- Modify `nomad-app/src/lib.rs` (add `pub mod mcp;`), `nomad-app/src/ui.rs` (field `mcp`, drain call), `nomad-app/src/transport.rs` (`load_preset` becomes `pub(crate)`), `nomad-app/src/main.rs` (`--mcp` flag), `nomad-app/Cargo.toml`, `README.md`.
- Tests: unit tests in `rpc.rs`; integration tests `nomad-app/tests/mcp_http.rs` and `nomad-app/tests/mcp_tools.rs` (the latter builds the app like `tests/presets_ui.rs::harness`).

---

### Task 1: JSON-RPC / MCP message layer

**Files:**
- Create: `nomad-app/src/mcp/rpc.rs`, `nomad-app/src/mcp/mod.rs` (only `pub mod rpc;` for now)
- Modify: `nomad-app/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub trait ToolHost {
      fn list(&self) -> Vec<serde_json::Value>;                      // tool descriptors
      fn call(&mut self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value, String>;
  }
  /// `None` for notifications (no `id`); otherwise the full JSON-RPC response.
  pub fn handle(msg: &serde_json::Value, host: &mut dyn ToolHost) -> Option<serde_json::Value>;
  ```

- [ ] **Step 1: Write failing unit tests in `rpc.rs`** with a `FakeHost` (one tool `echo` returning its args, one failing tool): `initialize` echoes a supported `protocolVersion` (`2025-06-18`, `2025-03-26`, `2024-11-05`) and falls back to `2025-06-18` for an unknown one, and returns `capabilities.tools`, `serverInfo.name == "nomad"`; `ping` returns `{}`; `tools/list` returns the host's list under `result.tools`; `tools/call` success returns `result.content[0].type == "text"` whose text is the JSON of the host's value and no `isError`; a host `Err(msg)` returns `isError: true` with `msg` as text; unknown method gives error code `-32601`; missing `params.name` gives `-32602`; a message without `id` (e.g. `notifications/initialized`) returns `None`; a JSON array returns an error `-32600`; string and number `id`s are echoed unchanged.
- [ ] **Step 2: Run** `cargo test -p nomad-app --lib mcp::rpc` — Expected: FAIL (not defined).
- [ ] **Step 3: Implement `handle`** as above in `rpc.rs`; `mod.rs` just declares `pub mod rpc;`; add `pub mod mcp;` to `lib.rs`.
- [ ] **Step 4: Run** the same command — Expected: PASS.
- [ ] **Step 5: Commit** `feat(mcp): JSON-RPC and MCP message layer`

### Task 2: HTTP server with local-only checks

**Files:**
- Create: `nomad-app/src/mcp/http.rs`; Modify: `nomad-app/src/mcp/mod.rs` (`pub mod http;`), `nomad-app/Cargo.toml` (`tiny_http = "0.12"`)
- Test: `nomad-app/tests/mcp_http.rs`

**Interfaces:**
- Consumes: `rpc::handle`, `rpc::ToolHost` (Task 1)
- Produces:
  ```rust
  pub struct HttpServer { /* tiny_http::Server + join handle */ }
  impl HttpServer {
      /// Binds 127.0.0.1:port (0 = ephemeral). `make_host` is called once on the server thread.
      pub fn start(port: u16, make_host: impl FnOnce() -> Box<dyn ToolHost> + Send + 'static) -> Result<HttpServer, String>;
      pub fn port(&self) -> u16;
  }
  impl Drop for HttpServer { /* unblock + join */ }
  ```

- [ ] **Step 1: Write failing tests** in `tests/mcp_http.rs`, speaking raw HTTP/1.1 over `std::net::TcpStream` (no HTTP client dependency) against `HttpServer::start(0, ...)` with a fake host: POST `/mcp` `initialize` → 200 and `application/json`; `notifications/initialized` → 202 empty; `GET /mcp` and `DELETE /mcp` → 405; POST `/other` → 404; invalid JSON → 400 with JSON-RPC error `-32700`; body over 1 MiB → 413; `Host: evil.example` → 403 and the fake host's call counter stays 0; `Origin: http://evil.example` → 403; no `Origin` header → accepted; `Origin: http://localhost:3000` → accepted; dropping the server frees the port (rebind succeeds).
- [ ] **Step 2: Run** `cargo test -p nomad-app --test mcp_http` — Expected: FAIL.
- [ ] **Step 3: Implement** `HttpServer` with one handler thread: validate method/path/headers, read body (cap 1 MiB), `serde_json::from_slice`, `rpc::handle`, respond. Header check is a small pure function `fn local_origin_ok(host: Option<&str>, origin: Option<&str>) -> bool` with its own unit test cases.
- [ ] **Step 4: Run** the test — Expected: PASS.
- [ ] **Step 5: Commit** `feat(mcp): local-only HTTP transport`

### Task 3: UI-thread bridge, `drain`, and app wiring

**Files:**
- Modify: `nomad-app/src/mcp/mod.rs`, `nomad-app/src/ui.rs` (field `pub mcp: Option<crate::mcp::Mcp>`, init `None`; in `ui()` before the final `request_repaint_after`: take the field, call `drain(self)`, put it back — the take/put-back avoids borrowing `self` twice)
- Create: `nomad-app/src/mcp/tools.rs` (stub for now)
- Test: `nomad-app/tests/mcp_tools.rs` (create with the `harness` helper copied from `tests/presets_ui.rs`, plus `fn call(h, name, args) -> Result<Value, String>` that runs `tools::call` on `h.state_mut().app` then `step(h)`)

**Interfaces:**
- Consumes: `HttpServer::start`, `ToolHost` (Task 2)
- Produces:
  ```rust
  pub struct Mcp { /* server, request receiver, note-off list */ }
  impl Mcp {
      pub fn start(port: u16, ctx: egui::Context) -> Result<Mcp, String>;   // ChannelHost sends, then ctx.request_repaint()
      pub fn port(&self) -> u16;
      pub fn drain(&mut self, app: &mut NomadApp);   // flush due note-offs, then run queued requests that are not cancelled
  }
  // tools.rs
  pub fn descriptors() -> Vec<serde_json::Value>;   // MCP tool descriptors (name, description, inputSchema); grows each task
  pub fn call(app: &mut NomadApp, offs: &mut Vec<PendingOff>, name: &str, args: &Value) -> Result<Value, String>;
  pub struct PendingOff { pub due: Instant, pub note: u8, pub target: Option<InstrumentId> }
  ```

- [ ] **Step 1: Write failing tests** in `mcp/mod.rs` unit tests with a `NomadApp` built as in the harness: (a) a request queued through the channel is executed by `drain` and its reply arrives (stub `call` returns `Err("unknown tool: x")`, check the reply carries it); (b) a request whose `cancelled` flag is set before `drain` is not executed; (c) `ChannelHost::call` returns `Err("timed out waiting for the UI")` after the timeout (make the timeout a field, 50 ms in the test, 2 s default) and sets `cancelled`; (d) a due `PendingOff` is sent as `NoteOff` by `drain` and removed; a not-yet-due one stays.
- [ ] **Step 2: Run** `cargo test -p nomad-app --lib mcp::` — Expected: FAIL.
- [ ] **Step 3: Implement** `Mcp`, `ChannelHost` (implements `ToolHost`: `list()` = `tools::descriptors()`, `call()` sends `Request { name, args, reply: Sender<Result<Value,String>>, cancelled: Arc<AtomicBool> }`, waits with `recv_timeout`), `drain`, and the stub `tools::call`.
- [ ] **Step 4: Run** — Expected: PASS; also `cargo test -p nomad-app` (existing tests still green).
- [ ] **Step 5: Commit** `feat(mcp): bridge requests to the UI thread`

### Task 4: Read tools

**Files:** Modify `nomad-app/src/mcp/tools.rs`; Test `nomad-app/tests/mcp_tools.rs`

**Interfaces:** Produces tools (all args JSON objects):
- `get_state {}` → `{ master, selected, target, spiral_open, telemetry:{peak,voices,xruns,dropped_commands}, instruments:[{id,kind:"synth"|"drums",name,source,bpm,running,muted,level,step}] }` from `AppModel` and `InstrumentShared` (`clock_bpm`, `eff_running`, `step`, `level`).
- `get_instrument {id}` → `presets::capture(view, name)` JSON.
- `list_params {kind}` → `[{name, lo, hi, default, unit}]` from `SYNTH_PARAMS` / `DRUM_PARAMS`; `kind` is `"synth"` or `"drums"`, otherwise error.
- `list_presets {kind}` → `[{name, origin}]` from `app.presets.list`.

- [ ] **Step 1: Write failing tests** on the harness: `get_state` on the default engine lists exactly the instruments `app.model.instruments()` has, with the right `kind`; `get_instrument` on the first synth has `params.cutoff`-style keys equal to `SYNTH_PARAMS` names (minus master) and a 16-element `seq.on`; `get_instrument {id: 999}` is `Err("no instrument with id 999")`; `list_params {kind:"synth"}` has 63 entries (62 params plus master, same as `SYNTH_PARAMS.len()`); `list_params {kind:"x"}` is an error; `list_presets {kind:"synth"}` includes the factory `Dark techno` presets.
- [ ] **Step 2: Run** `cargo test -p nomad-app --test mcp_tools` — Expected: FAIL.
- [ ] **Step 3: Implement** the four tools, with a shared helper `fn arg_id(app: &NomadApp, args: &Value) -> Result<&InstrumentView, String>` that returns the `no instrument with id N` error (also for instances with `shared.closing` set). Tool descriptors for each.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(mcp): read tools`

### Task 5: Instances, sound and transport tools

**Files:** Modify `nomad-app/src/mcp/tools.rs`, `nomad-app/src/ui.rs` (new `pub fn set_master(&mut self, v: f64)` on `NomadApp`: updates the local `master` knob with `KnobModel::set_value` and calls `model.set_master`, because the MASTER knob is a UI-local model, not a mirror); Test `nomad-app/tests/mcp_tools.rs`

**Interfaces:** Produces tools:
- `add_instrument {kind}` → `{id}`; error `instance limit reached` when `AppModel::add_instrument` returns `None`. `close_instrument {id}`, `select_instrument {id}`, `set_target {id}` (synths only), `set_source {id, source: int|null}` (error text when `AppModel::set_source` returns `false`).
- `set_param {id, name, value}` (synth only): look up `name` in `SYNTH_PARAMS`, clamp with `ParamSpec::clamp`, send `Command::SetParam`. Unknown name error lists all valid names. `master_volume` is rejected with "use set_master".
- `set_master {value}`, `set_level {id, value}`, `set_mute {id, muted}` (idempotent via `AppModel::muted`).
- `set_bpm {id, bpm}`, `play {id}`, `stop {id}`: idempotent through `shared.clock_running`, calling `toggle_play` only when state differs; when `AppModel` returns `false` because the instance is a follower, error: `instrument N follows instrument M's clock; control M instead`.

- [ ] **Step 1: Write failing tests:** `add_instrument` then `get_state` shows one more instrument and a new id; `set_param` cutoff to a value, `step`, then `get_instrument` reports that value and the `SynthParams` of the synth panel adopts it after one UI frame (knob follows); `set_param` with value `1e9` is stored clamped to `hi`; with `"nope"` the error contains `cutoff`; with NaN-like input (`null`) error; `set_master 0.3` leaves `app.master`-backed `get_state.master == 0.3`; `play` twice leaves it running (not toggled off); `play` on a follower errors with the message above; `set_mute true` twice stays muted.
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement** the tools.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(mcp): instance, sound and transport tools`

### Task 6: Notes, sequencer and drums tools

**Files:** Modify `nomad-app/src/mcp/tools.rs`; Test `nomad-app/tests/mcp_tools.rs`

**Interfaces:** Produces tools:
- `note_on {note, velocity?, duration_ms?}`: validate (note 0..=127, velocity 0..=1 default 0.8, duration 1..=10000 default 500), send `Command::NoteOn` to the target synth, push `PendingOff { due, note, target: app.model.target_synth() }`. In `Mcp::drain`, when a due off's `target` differs from the current target, send `Command::Panic` instead of `NoteOff` (shortcut: panics all voices, documented in a code comment; upgrade if `Command::NoteOff` ever gets an id).
- `panic {}` → `Command::Panic` and clears pending offs.
- `drum_trigger {id, track 0..=5, velocity? default 0.8}`.
- `seq_set_step {id, step 0..=15, on, note?, vel?}`, `seq_clear {id}`, `seq_set {id, length? 1..=16, gate?, division?, swing?}` (each given field sent as its `Seq*` command).
- `drum_set_step {id, track, step, level}` (level range as accepted by `drums_view.rs::toggled`; verify there), `drum_clear {id}`.
- Each rejects the wrong instrument kind with `instrument N is a synth, not drums` (and the reverse).

- [ ] **Step 1: Write failing tests:** `note_on` makes `telemetry.voices_total` ≥ 1 after a `step`; after the duration passes (drive `drain` with a manual clock by constructing `PendingOff` with a past `due`, or sleep 60 ms with `duration_ms: 10`) voices return to 0; `note_on {note:200}` and `{duration_ms: 1000000000}` are errors/clamped per the constraints; `note_on` then `set_target` to another synth then drain past due sends Panic (voices 0); `seq_set_step` then `get_instrument` shows `seq.on[3] == 1` and the note; `seq_set_step {step:16}` errors; `drum_set_step` on a synth errors with the kind message; `drum_trigger {track:6}` errors.
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement** the tools and the retarget rule in `drain`.
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(mcp): note, sequencer and drum tools`

### Task 7: Presets and tabs tools

**Files:** Modify `nomad-app/src/mcp/tools.rs`, `nomad-app/src/transport.rs` (`fn load_preset` → `pub(crate) fn load_preset`); Test `nomad-app/tests/mcp_tools.rs`

**Interfaces:** Produces tools:
- `load_preset {id, name}`: reuses `transport::load_preset`; if `app.status` changed to an error, return it as the tool error (clear it first so a stale message is not reported).
- `save_preset {id, name}`: error `no preset folder` if `!app.presets.can_save()`; empty name error; otherwise `app.presets.save(kind, name, &capture(view, ""))` like `transport::save_dialog` does, returning the saved name.
- `open_spiral {}`, `close_spiral {}` → `NomadApp::open_spiral/close_spiral`.

- [ ] **Step 1: Write failing tests** (temp preset dir as in `presets_ui.rs`): `load_preset` with a factory name changes `get_instrument.params` and `preset_names[id]`; unknown name is an error mentioning the name; `save_preset` then `list_presets` includes it with the user origin, and loading it round-trips the params; `save_preset` with empty name errors; `open_spiral` then `get_state.spiral_open == true`, `close_spiral` false.
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — Expected: PASS.
- [ ] **Step 5: Commit** `feat(mcp): preset and tab tools`

### Task 8: `--mcp` flag, docs, end-to-end check

**Files:** Modify `nomad-app/src/main.rs` (`USAGE` gains `nomad gui [--mcp [PORT]]`; `gui()` parses the flag with `Args`, default port 7777; start `Mcp` inside the `run_native` closure using `cc.egui_ctx.clone()` and store it in `app.mcp`; on `Err(e)` push the message to `app.status` in Spanish, e.g. `MCP: puerto 7777 ocupado`, and keep going; on success set status `MCP en http://127.0.0.1:PORT/mcp`), `README.md` (short "Control desde una IA" section with the `claude mcp add --transport http nomad http://127.0.0.1:7777/mcp` command, the tool list, and the localhost-only note); Test `nomad-app/tests/mcp_http.rs`

- [ ] **Step 1: Write failing test:** `Args::parse` + the gui arg handling function (extract `fn mcp_port(argv: &[String]) -> Result<Option<u16>, String>` in `main.rs` or `lib.rs`) returns `None` without the flag, `Some(7777)` for bare `--mcp`, `Some(9000)` for `--mcp 9000`, and an error for `--mcp abc`.
- [ ] **Step 2: Run** `cargo test -p nomad-app mcp_port` — Expected: FAIL.
- [ ] **Step 3: Implement** the flag, startup and README.
- [ ] **Step 4: Run** the full suite `cargo test --workspace` and `cargo clippy --workspace --all-targets` — Expected: all pass, no warnings.
- [ ] **Step 5: Manual end-to-end:** `cargo run -p nomad-app -- gui --mcp`, then `claude mcp add --transport http nomad http://127.0.0.1:7777/mcp`; ask Claude to read the state, add a synth, load `Dark techno`, set a step pattern, play, and open the spiral; confirm the window shows each change and that closing the window frees the port. Also `curl -H 'Host: evil.example' -d '{}' http://127.0.0.1:7777/mcp` returns 403.
- [ ] **Step 6: Commit** `feat(mcp): --mcp flag and docs`

---

## Self-review notes

- Spec coverage: transport and local-only checks (Task 2), bridge/timeout/cancel/repaint (3), read tools (4), write tools incl. idempotent play/mute and follower error (5), notes with auto-release and panic (6), presets and tabs (7), flag/status/docs/manual verification (8). Drum params as loose tools are out of v1 per the spec; they are changed through `load_preset`.
- The `NomadApp::set_master` addition (Task 5) is not in the spec; it is needed because the MASTER knob is not mirrored.
- Known shortcut: a retarget between `note_on` and its release sends `Panic` (Task 6).
