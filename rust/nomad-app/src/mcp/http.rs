//! Local-only HTTP transport for MCP (Streamable HTTP, POST /mcp, no SSE).

use super::rpc::{self, ToolHost};
use serde_json::{Value, json};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};

const MAX_BODY: u64 = 1024 * 1024;
const MAX_IN_FLIGHT: usize = 16;

type SharedHost = Arc<Mutex<Box<dyn ToolHost + Send>>>;

/// Counts a live request thread; decrements on drop, even when the thread panics.
struct InFlight(Arc<AtomicUsize>);
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Stands in when `make_host` panicked: every tool call then answers 500.
struct Dead;
impl ToolHost for Dead {
    fn list(&self) -> Vec<Value> {
        panic!("host failed to start")
    }
    fn call(&mut self, _: &str, _: &Value) -> Result<Value, String> {
        panic!("host failed to start")
    }
}

/// `host` is `127.0.0.1` or `localhost`, optionally followed by `:port`.
fn host_ok(h: &str) -> bool {
    let name = match h.split_once(':') {
        Some((n, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => n,
        Some(_) => return false,
        None => h,
    };
    name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")
}

/// DNS-rebinding guard: Host must be local; Origin, when present, a local http origin.
fn local_origin_ok(host: Option<&str>, origin: Option<&str>) -> bool {
    host.is_some_and(host_ok)
        && origin.is_none_or(|o| o.strip_prefix("http://").is_some_and(host_ok))
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn reply(req: Request, status: u16, body: Option<Value>) {
    let mut r = Response::from_string(body.map(|b| b.to_string()).unwrap_or_default())
        .with_status_code(status);
    if let Ok(h) = Header::from_bytes("Content-Type", "application/json") {
        r.add_header(h);
    }
    let _ = req.respond(r);
}

fn serve(mut req: Request, host: &SharedHost) {
    if !local_origin_ok(header(&req, "Host"), header(&req, "Origin")) {
        return reply(req, 403, None);
    }
    if req.url().split('?').next() != Some("/mcp") {
        return reply(req, 404, None);
    }
    if *req.method() != Method::Post {
        return reply(req, 405, None);
    }
    let mut buf = Vec::new();
    let read = req.as_reader().take(MAX_BODY + 1).read_to_end(&mut buf);
    if read.is_err() {
        return reply(req, 400, None);
    }
    if buf.len() as u64 > MAX_BODY {
        return reply(req, 413, None);
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&buf) else {
        let err = json!({"jsonrpc": "2.0", "id": null,
            "error": {"code": -32700, "message": "parse error"}});
        return reply(req, 400, Some(err));
    };
    // A client's reply to a server request: accepted, never dispatched.
    let is_response = msg.get("method").is_none()
        && msg.get("id").is_some()
        && (msg.get("result").is_some() || msg.get("error").is_some());
    if is_response {
        return reply(req, 202, None);
    }
    // Requests run one at a time through the mutex; a panicking host answers 500.
    let handled = catch_unwind(AssertUnwindSafe(|| {
        let mut h = host.lock().unwrap_or_else(PoisonError::into_inner);
        rpc::handle(&msg, h.as_mut())
    }));
    match handled {
        Ok(Some(resp)) => reply(req, 200, Some(resp)),
        Ok(None) => reply(req, 202, None),
        Err(_) => {
            let err = json!({"jsonrpc": "2.0", "id": msg.get("id"),
                "error": {"code": -32603, "message": "internal error"}});
            reply(req, 500, Some(err));
        }
    }
}

pub struct HttpServer {
    server: Option<Arc<Server>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    port: u16,
}

impl HttpServer {
    /// Binds 127.0.0.1:port (0 = ephemeral). `make_host` runs once on the accept thread; each request gets its own short-lived thread.
    pub fn start(
        port: u16,
        make_host: impl FnOnce() -> Box<dyn ToolHost + Send> + Send + 'static,
    ) -> Result<HttpServer, String> {
        let server = Arc::new(Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?);
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or("no ip address")?;
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (server.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let host: SharedHost = Arc::new(Mutex::new(
                catch_unwind(AssertUnwindSafe(make_host)).unwrap_or_else(|_| Box::new(Dead)),
            ));
            let in_flight = Arc::new(AtomicUsize::new(0));
            while !st.load(Ordering::SeqCst) {
                match s.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(req)) => {
                        if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                            reply(req, 503, None);
                            continue;
                        }
                        let guard = InFlight(in_flight.clone());
                        let h = host.clone();
                        // A failed spawn drops the request (client sees a closed connection).
                        let _ = std::thread::Builder::new().spawn(move || {
                            let _guard = guard;
                            serve(req, &h);
                        });
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(HttpServer {
            server: Some(server),
            stop,
            thread: Some(thread),
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(s) = &self.server {
            s.unblock();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.server = None;
        // tiny_http closes the listener on its own accept thread, asynchronously:
        // wait (bounded) until the port is really free so a restart can rebind it.
        for _ in 0..100 {
            if std::net::TcpListener::bind(("127.0.0.1", self.port)).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::local_origin_ok as ok;

    #[test]
    fn host_and_origin_rules() {
        assert!(ok(Some("127.0.0.1"), None));
        assert!(ok(Some("127.0.0.1:8080"), None));
        assert!(ok(Some("localhost:1"), Some("http://localhost:3000")));
        assert!(ok(Some("LocalHost"), Some("http://127.0.0.1")));
        assert!(!ok(None, None));
        assert!(!ok(Some("evil.example"), None));
        assert!(!ok(Some("127.0.0.1.evil.example"), None));
        assert!(!ok(Some("127.0.0.1:"), None));
        assert!(!ok(Some("localhost:80a"), None));
        assert!(!ok(Some("127.0.0.1"), Some("http://evil.example")));
        assert!(!ok(Some("127.0.0.1"), Some("https://localhost")));
        assert!(!ok(Some("127.0.0.1"), Some("http://localhost.evil.com")));
        assert!(!ok(Some("127.0.0.1"), Some("http://localhost/x")));
        assert!(!ok(Some("127.0.0.1"), Some("null")));
    }
}
