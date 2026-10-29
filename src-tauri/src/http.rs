//! The Tauri command API mirrored over HTTP, so the frontend can run in a normal browser for debugging.
use crate::engine::{self, Engine};
use crate::world::TileKey;
use serde::Serialize;
use std::str::FromStr;
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response, Server};

enum Reply {
    Json(Vec<u8>),
    Binary(Vec<u8>),
}

#[derive(Debug, thiserror::Error)]
enum HttpError {
    #[error("missing or invalid `{0}`")]
    BadParam(&'static str),
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Engine(#[from] engine::Error),
}

pub fn serve(engine: &Arc<Engine>, port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(("127.0.0.1", port))?;
    println!("atlas API on http://127.0.0.1:{port}");
    for request in server.incoming_requests() {
        let engine = engine.clone();
        std::thread::spawn(move || respond(&engine, request));
    }
    Ok(())
}

fn respond(engine: &Arc<Engine>, request: Request) {
    let (status, content_type, body) = match handle(engine, request.method(), request.url()) {
        Ok(Reply::Json(body)) => (200, "application/json", body),
        Ok(Reply::Binary(body)) => (200, "application/octet-stream", body),
        Err(e) => (
            if matches!(e, HttpError::NotFound) { 404 } else { 400 },
            "text/plain",
            e.to_string().into_bytes(),
        ),
    };
    let mut response = Response::from_data(body).with_status_code(status);
    if let Ok(header) = Header::from_bytes("Content-Type", content_type) {
        response.add_header(header);
    }
    // The client may have gone away; nothing to do about it.
    let _ = request.respond(response);
}

fn handle(engine: &Arc<Engine>, method: &Method, url: &str) -> Result<Reply, HttpError> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    match (method, path) {
        (_, "/api/roots") => json(&engine::roots()),
        (_, "/api/status") => json(&engine.status()),
        (_, "/api/access") => json(&engine::needs_full_disk_access(
            &param(query, "root").ok_or(HttpError::BadParam("root"))?,
        )),
        (Method::Post, "/api/scan") => {
            engine.start(param(query, "root").ok_or(HttpError::BadParam("root"))?)?;
            Ok(Reply::Json(b"{}".to_vec()))
        }
        (_, "/api/world") => Ok(Reply::Binary(engine.world()?.payload.clone())),
        (_, "/api/patch") => Ok(Reply::Binary(engine.patch(number(query, "id")?)?.payload.clone())),
        (_, "/api/tile") => {
            let tile = TileKey {
                level: number(query, "l")?,
                x: number(query, "x")?,
                y: number(query, "y")?,
            };
            Ok(Reply::Binary(engine.tile(tile)?.payload.clone()))
        }
        _ => Err(HttpError::NotFound),
    }
}

fn param(query: &str, name: &str) -> Option<String> {
    form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

fn number<T: FromStr>(query: &str, name: &'static str) -> Result<T, HttpError> {
    param(query, name).and_then(|v| v.parse().ok()).ok_or(HttpError::BadParam(name))
}

fn json(value: &impl Serialize) -> Result<Reply, HttpError> {
    serde_json::to_vec(value).map(Reply::Json).map_err(|e| HttpError::Engine(e.into()))
}
