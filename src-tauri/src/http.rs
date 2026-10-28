//! The Tauri command API mirrored over HTTP, so the frontend can run in a normal browser for debugging.
use crate::engine::{Engine, roots};
use std::sync::Arc;
use tiny_http::{Header, Method, Response, Server};

fn query(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| decode(v)))
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'?'));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn serve(engine: Arc<Engine>, port: u16) {
    let server = Server::http(("127.0.0.1", port)).expect("bind");
    println!("atlas API on http://127.0.0.1:{port}");
    for req in server.incoming_requests() {
        let engine = engine.clone();
        std::thread::spawn(move || {
            let url = req.url().to_string();
            let path = url.split('?').next().unwrap_or("");
            let json = |v: serde_json::Value| Ok(("application/json", v.to_string().into_bytes()));
            let result: Result<(&str, Vec<u8>), String> = match (req.method(), path) {
                (_, "/api/roots") => json(roots()),
                (_, "/api/status") => json(engine.status()),
                (Method::Post, "/api/scan") => query(&url, "root")
                    .ok_or("missing root".to_string())
                    .and_then(|r| engine.start(r))
                    .and_then(|_| json(serde_json::json!({}))),
                (_, "/api/world") => engine.world().map(|b| ("application/octet-stream", b)),
                (_, "/api/patch") => query(&url, "id")
                    .and_then(|i| i.parse().ok())
                    .ok_or("bad id".to_string())
                    .and_then(|id| engine.patch(id))
                    .map(|b| ("application/octet-stream", b)),
                (_, "/api/tile") => {
                    let num = |k: &str| query(&url, k).and_then(|v| v.parse::<i64>().ok());
                    match (num("l"), num("x"), num("y")) {
                        (Some(l), Some(x), Some(y)) => engine.tile(l as u32, x, y).map(|b| ("application/octet-stream", b)),
                        _ => Err("bad tile".into()),
                    }
                }
                _ => Err("not found".into()),
            };
            let resp = match result {
                Ok((ct, body)) => Response::from_data(body).with_header(Header::from_bytes("Content-Type", ct).unwrap()),
                Err(e) => Response::from_data(e.into_bytes()).with_status_code(400),
            };
            let _ = req.respond(resp);
        });
    }
}
