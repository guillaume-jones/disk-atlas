pub mod engine;
pub mod geo;
pub mod http;
mod names;
pub mod scan;
mod util;
pub mod world;

use engine::{Engine, roots};
use std::sync::Arc;
use tauri::State;
use tauri::ipc::Response;

#[tauri::command]
fn start_scan(engine: State<Arc<Engine>>, root: String) -> Result<(), String> {
    engine.start(root)
}

#[tauri::command]
fn scan_status(engine: State<Arc<Engine>>) -> serde_json::Value {
    engine.status()
}

#[tauri::command(async)]
fn world(engine: State<'_, Arc<Engine>>) -> Result<Response, String> {
    engine.world().map(Response::new)
}

#[tauri::command(async)]
fn patch(engine: State<'_, Arc<Engine>>, id: u32) -> Result<Response, String> {
    engine.patch(id).map(Response::new)
}

#[tauri::command(async)]
fn tile(engine: State<'_, Arc<Engine>>, level: u32, x: i64, y: i64) -> Result<Response, String> {
    engine.tile(level, x, y).map(Response::new)
}

#[tauri::command]
fn scan_roots() -> serde_json::Value {
    roots()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = Arc::new(Engine::default());
    if let Some(port) = std::env::var("DISK_ATLAS_HTTP").ok().and_then(|p| p.parse().ok()) {
        let engine = engine.clone();
        std::thread::spawn(move || http::serve(engine, port));
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(engine)
        .invoke_handler(tauri::generate_handler![start_scan, scan_status, world, patch, tile, scan_roots])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
