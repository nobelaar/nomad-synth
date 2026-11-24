//! JSON-RPC 2.0 / MCP message layer: pure, no I/O.

use serde_json::{Value, json};

/// What the MCP server exposes as tools.
pub trait ToolHost {
    fn list(&self) -> Vec<Value>;
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String>;
}

const SUPPORTED: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// `None` for notifications (no `id`); otherwise the full JSON-RPC response.
pub fn handle(msg: &Value, host: &mut dyn ToolHost) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(error(Value::Null, -32600, "invalid request"));
    };
    let id = obj.get("id")?.clone();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Some(error(id, -32600, "invalid request"));
    };
    let params = obj.get("params").unwrap_or(&Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|v| SUPPORTED.contains(v))
                .unwrap_or(SUPPORTED[0]);
            ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "nomad", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": host.list()})),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, -32602, "missing params.name"));
            };
            let args = params.get("arguments").unwrap_or(&Value::Null);
            let (text, is_error) = match host.call(name, args) {
                Ok(v) => (serde_json::to_string(&v).unwrap_or_default(), false),
                Err(e) => (e, true),
            };
            let mut result = json!({"content": [{"type": "text", "text": text}]});
            if is_error {
                result["isError"] = json!(true);
            }
            ok(id, result)
        }
        _ => error(id, -32601, "method not found"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHost;
    impl ToolHost for FakeHost {
        fn list(&self) -> Vec<Value> {
            vec![json!({"name": "echo"}), json!({"name": "boom"})]
        }
        fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
            match name {
                "echo" => Ok(args.clone()),
                _ => Err("it broke".into()),
            }
        }
    }

    fn run(msg: Value) -> Option<Value> {
        handle(&msg, &mut FakeHost)
    }

    fn req(method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    }

    #[test]
    fn initialize_echoes_supported_versions_and_falls_back() {
        for v in ["2025-06-18", "2025-03-26", "2024-11-05"] {
            let r = run(req("initialize", json!({"protocolVersion": v}))).unwrap();
            assert_eq!(r["result"]["protocolVersion"], v);
        }
        let r = run(req("initialize", json!({"protocolVersion": "1999-01-01"}))).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "nomad");
        let r = run(req("initialize", json!({}))).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn ping_returns_empty_object() {
        assert_eq!(run(req("ping", json!({}))).unwrap()["result"], json!({}));
    }

    #[test]
    fn tools_list_returns_host_tools() {
        let r = run(req("tools/list", json!({}))).unwrap();
        assert_eq!(
            r["result"]["tools"],
            json!([{"name": "echo"}, {"name": "boom"}])
        );
    }

    #[test]
    fn tools_call_success_is_json_text() {
        let r = run(req(
            "tools/call",
            json!({"name": "echo", "arguments": {"a": 1}}),
        ))
        .unwrap();
        assert_eq!(r["result"]["content"][0]["type"], "text");
        assert_eq!(r["result"]["content"][0]["text"], "{\"a\":1}");
        assert!(r["result"].get("isError").is_none());
    }

    #[test]
    fn tools_call_host_error_sets_is_error() {
        let r = run(req("tools/call", json!({"name": "boom"}))).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "it broke");
    }

    #[test]
    fn protocol_errors() {
        assert_eq!(
            run(req("nope", json!({}))).unwrap()["error"]["code"],
            -32601
        );
        assert_eq!(
            run(req("tools/call", json!({}))).unwrap()["error"]["code"],
            -32602
        );
        assert_eq!(run(json!([1, 2])).unwrap()["error"]["code"], -32600);
    }

    #[test]
    fn notifications_get_no_reply() {
        assert!(run(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
    }

    #[test]
    fn ids_are_echoed_unchanged() {
        let r = run(json!({"jsonrpc": "2.0", "id": "abc", "method": "ping"})).unwrap();
        assert_eq!(r["id"], "abc");
        let r = run(json!({"jsonrpc": "2.0", "id": 42, "method": "ping"})).unwrap();
        assert_eq!(r["id"], 42);
    }
}
