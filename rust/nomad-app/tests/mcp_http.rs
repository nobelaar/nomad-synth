use nomad_app::mcp::http::HttpServer;
use nomad_app::mcp::rpc::ToolHost;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fake(Arc<AtomicUsize>);
impl ToolHost for Fake {
    fn list(&self) -> Vec<Value> {
        self.0.fetch_add(1, Ordering::SeqCst);
        vec![]
    }
    fn call(&mut self, name: &str, _: &Value) -> Result<Value, String> {
        assert_ne!(name, "boom", "fake host panic");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(json!({}))
    }
}

fn server() -> (HttpServer, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let s = HttpServer::start(0, move || Box::new(Fake(n2))).unwrap();
    (s, n)
}

/// Raw HTTP/1.1 exchange: returns (status, lowercased head, body).
fn raw(port: u16, method: &str, path: &str, extra: &[&str], body: &[u8]) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !extra.iter().any(|h| h.to_lowercase().starts_with("host:")) {
        req += &format!("Host: 127.0.0.1:{port}\r\n");
    }
    for h in extra {
        req += &format!("{h}\r\n");
    }
    req += &format!("Content-Length: {}\r\n\r\n", body.len());
    let _ = s.write_all(req.as_bytes());
    let _ = s.write_all(body); // server may answer 413 and close early
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head.to_lowercase(), body.to_string())
}

fn post(port: u16, extra: &[&str], body: &str) -> (u16, String, String) {
    raw(port, "POST", "/mcp", extra, body.as_bytes())
}

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
const LIST: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;

#[test]
fn initialize_is_json_200() {
    let (s, _) = server();
    let (st, head, body) = post(s.port(), &[], INIT);
    assert_eq!(st, 200);
    assert!(head.contains("application/json"));
    assert!(body.contains("protocolVersion"));
}

#[test]
fn notification_is_202_empty() {
    let (s, _) = server();
    let (st, _, body) = post(
        s.port(),
        &[],
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    assert_eq!((st, body.as_str()), (202, ""));
}

#[test]
fn posted_response_is_202_without_calling_handle() {
    let (s, n) = server();
    for msg in [
        r#"{"jsonrpc":"2.0","id":7,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":7,"error":{"code":-1,"message":"x"}}"#,
    ] {
        let (st, _, body) = post(s.port(), &[], msg);
        assert_eq!((st, body.as_str()), (202, ""));
    }
    assert_eq!(n.load(Ordering::SeqCst), 0);
}

#[test]
fn get_and_delete_are_405() {
    let (s, _) = server();
    assert_eq!(raw(s.port(), "GET", "/mcp", &[], b"").0, 405);
    assert_eq!(raw(s.port(), "DELETE", "/mcp", &[], b"").0, 405);
}

#[test]
fn unknown_path_is_404() {
    let (s, _) = server();
    assert_eq!(raw(s.port(), "POST", "/other", &[], b"{}").0, 404);
}

#[test]
fn invalid_json_is_400_parse_error() {
    let (s, _) = server();
    let (st, _, body) = post(s.port(), &[], "{nope");
    assert_eq!(st, 400);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], -32700);
}

#[test]
fn oversized_body_is_413() {
    let (s, _) = server();
    let big = vec![b' '; 1024 * 1024 + 1];
    assert_eq!(raw(s.port(), "POST", "/mcp", &[], &big).0, 413);
}

#[test]
fn foreign_host_is_403_and_host_untouched() {
    let (s, n) = server();
    assert_eq!(post(s.port(), &["Host: evil.example"], LIST).0, 403);
    assert_eq!(n.load(Ordering::SeqCst), 0);
    assert_eq!(post(s.port(), &[], LIST).0, 200); // control: the host is reachable
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[test]
fn origin_rules() {
    let (s, n) = server();
    assert_eq!(
        post(s.port(), &["Origin: http://evil.example"], LIST).0,
        403
    );
    assert_eq!(n.load(Ordering::SeqCst), 0);
    assert_eq!(
        post(s.port(), &["Origin: http://localhost:3000"], LIST).0,
        200
    );
    assert_eq!(post(s.port(), &["Origin: http://127.0.0.1"], LIST).0, 200);
    assert_eq!(post(s.port(), &[], LIST).0, 200);
}

#[test]
fn drop_frees_the_port() {
    let (s, _) = server();
    let port = s.port();
    drop(s);
    TcpListener::bind(("127.0.0.1", port)).expect("port freed after drop");
}

#[test]
fn stalled_client_does_not_block_others_or_drop() {
    let (s, _) = server();
    let port = s.port();
    let mut stalled = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stalled
        .write_all(
            format!(
                "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 100\r\n\r\nabcde"
            )
            .as_bytes(),
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(post(port, &[], INIT).0, 200);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(s);
        let _ = tx.send(());
    });
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .expect("drop must not wait for the stalled request");
    drop(stalled);
}

#[test]
fn panicking_host_is_500_and_server_survives() {
    let (s, _) = server();
    let boom = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"boom"}}"#;
    let (st, _, body) = post(s.port(), &[], boom);
    assert_eq!(st, 500);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], -32603);
    assert_eq!(post(s.port(), &[], boom).0, 500); // mutex was poisoned: still recovers
    assert_eq!(post(s.port(), &[], LIST).0, 200);
}
