pub mod http;
pub mod rpc;
pub mod tools;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use nomad_runtime::Command;
use serde_json::Value;

use crate::ui::NomadApp;
use http::HttpServer;
use rpc::ToolHost;
use tools::PendingOff;

const UI_TIMEOUT: Duration = Duration::from_secs(2);

/// A tool call queued for the UI thread.
struct Request {
    name: String,
    args: Value,
    reply: Sender<Result<Value, String>>,
    cancelled: Arc<AtomicBool>,
}

/// The HTTP threads' view of the app: queues calls for `Mcp::drain`.
struct ChannelHost {
    tx: Sender<Request>,
    ctx: egui::Context,
    timeout: Duration,
}

impl ToolHost for ChannelHost {
    fn list(&self) -> Vec<Value> {
        tools::descriptors()
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let (reply, rx) = channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let req = Request {
            name: name.to_string(),
            args: args.clone(),
            reply,
            cancelled: cancelled.clone(),
        };
        self.tx
            .send(req)
            .map_err(|_| "the UI is gone".to_string())?;
        self.ctx.request_repaint();
        rx.recv_timeout(self.timeout).unwrap_or_else(|_| {
            cancelled.store(true, Ordering::SeqCst);
            Err("timed out waiting for the UI".to_string())
        })
    }
}

pub struct Mcp {
    server: Option<HttpServer>,
    rx: Receiver<Request>,
    offs: Vec<PendingOff>,
}

impl Mcp {
    pub fn start(port: u16, ctx: egui::Context) -> Result<Mcp, String> {
        let (tx, rx) = channel();
        let server = HttpServer::start(port, move || {
            Box::new(ChannelHost {
                tx,
                ctx,
                timeout: UI_TIMEOUT,
            })
        })?;
        Ok(Mcp {
            server: Some(server),
            rx,
            offs: Vec::new(),
        })
    }

    pub fn port(&self) -> u16 {
        self.server.as_ref().map_or(0, HttpServer::port)
    }

    /// Flush due note-offs, then run the queued requests that nobody gave up on.
    pub fn drain(&mut self, app: &mut NomadApp) {
        flush_offs(&mut self.offs, app, Instant::now());
        while let Ok(r) = self.rx.try_recv() {
            if r.cancelled.load(Ordering::SeqCst) {
                continue;
            }
            let res = tools::call(app, &mut self.offs, &r.name, &r.args);
            let _ = r.reply.send(res);
        }
    }
}

/// Send the note-offs that are due at `now`; keep the rest.
pub fn flush_offs(offs: &mut Vec<PendingOff>, app: &mut NomadApp, now: Instant) {
    offs.retain(|o| {
        let due = o.due <= now;
        if due {
            if o.target == app.model.target_synth() {
                app.model.send(Command::NoteOff { note: o.note });
            } else {
                // shortcut: NoteOff has no instrument id, so a retargeted note is silenced by
                // panicking every voice, upgrade if Command::NoteOff ever carries an id.
                app.model.send(Command::Panic);
            }
        }
        !due
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppModel, initial_views};
    use nomad_core::engine::Engine;
    use nomad_runtime::midi::map::MidiMap;
    use nomad_runtime::{AudioCore, create};
    use std::sync::Mutex;

    fn app() -> (NomadApp, AudioCore) {
        let e = Engine::new(48000.0, 8);
        let views = initial_views(&e);
        let (core, ctl) = create(e, 48000.0);
        let tel = ctl.telemetry();
        let model = AppModel::new(ctl, 48000.0, views);
        let app = NomadApp::new(
            model,
            None,
            None,
            Arc::new(Mutex::new(MidiMap::new(None))),
            tel,
        );
        (app, core)
    }

    fn mcp_and_host(timeout: Duration) -> (Mcp, ChannelHost) {
        let (tx, rx) = channel();
        let host = ChannelHost {
            tx,
            ctx: egui::Context::default(),
            timeout,
        };
        (
            Mcp {
                server: None,
                rx,
                offs: Vec::new(),
            },
            host,
        )
    }

    #[test]
    fn drain_runs_queued_request_and_replies() {
        let (mut mcp, mut host) = mcp_and_host(Duration::from_secs(5));
        let t = std::thread::spawn(move || host.call("x", &Value::Null));
        let (mut a, _core) = app();
        while !t.is_finished() {
            mcp.drain(&mut a);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(t.join().unwrap(), Err("unknown tool: x".to_string()));
    }

    #[test]
    fn drain_skips_cancelled() {
        let (mut mcp, _host) = mcp_and_host(Duration::from_secs(5));
        let (reply, rx) = channel();
        let cancelled = Arc::new(AtomicBool::new(true));
        // queue straight onto the channel the Mcp reads
        let (tx, rx2) = channel();
        mcp.rx = rx2;
        tx.send(Request {
            name: "x".into(),
            args: Value::Null,
            reply,
            cancelled,
        })
        .unwrap();
        let (mut a, _core) = app();
        mcp.drain(&mut a);
        assert!(rx.try_recv().is_err());
        assert!(mcp.rx.try_recv().is_err());
    }

    #[test]
    fn call_times_out_and_cancels() {
        let (mcp, mut host) = mcp_and_host(Duration::from_millis(50));
        assert_eq!(
            host.call("x", &Value::Null),
            Err("timed out waiting for the UI".to_string())
        );
        let r = mcp.rx.try_recv().unwrap();
        assert!(r.cancelled.load(Ordering::SeqCst));
    }

    #[test]
    fn flush_sends_due_offs_only() {
        let (mut a, mut core) = app();
        let now = Instant::now();
        let target = a.model.target_synth();
        a.model.send(Command::NoteOn {
            note: 60,
            velocity: 0.8,
        });
        a.model.send(Command::NoteOn {
            note: 64,
            velocity: 0.8,
        });
        let mut out = vec![0.0f32; 512];
        core.render_stereo(&mut out, 1);
        let mut offs = vec![
            PendingOff {
                due: now,
                note: 60,
                target,
            },
            PendingOff {
                due: now + Duration::from_secs(60),
                note: 64,
                target,
            },
        ];
        flush_offs(&mut offs, &mut a, now);
        core.render_stereo(&mut out, 1);
        assert_eq!(offs.len(), 1);
        assert_eq!(offs[0].note, 64);
        let id = a.model.target_synth().unwrap();
        let held = core.engine_mut().synth_mut(id).unwrap().held_mask();
        assert_eq!(held, 1u128 << 64);
    }
}
