// Tauri commands receive their state by value.
#![allow(clippy::needless_pass_by_value)]

mod engine;
mod geo;
pub mod http;
mod rng;
mod scan;
mod world;

pub use engine::{Engine, Error, Phase, Roots, Status, needs_full_disk_access, roots};
pub use geo::{Geo, Region, RegionId, RegionKind};
pub use world::TileKey;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tauri::State;
use tauri::ipc::Response;

/// Every critical section is a single assignment or insert, so a panic elsewhere can't leave the data half-updated
/// and a poisoned lock is safe to keep using.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[tauri::command]
fn start_scan(engine: State<Arc<Engine>>, root: String) -> Result<(), Error> {
    engine.start(root)
}

#[tauri::command]
fn scan_status(engine: State<Arc<Engine>>) -> Status {
    engine.status()
}

#[tauri::command(async)]
fn world(engine: State<'_, Arc<Engine>>) -> Result<Response, Error> {
    engine.world().map(|built| Response::new(built.payload.clone()))
}

#[tauri::command(async)]
fn patch(engine: State<'_, Arc<Engine>>, id: u32) -> Result<Response, Error> {
    engine.patch(id).map(|built| Response::new(built.payload.clone()))
}

#[tauri::command(async)]
fn tile(engine: State<'_, Arc<Engine>>, level: u32, x: i64, y: i64) -> Result<Response, Error> {
    engine
        .tile(TileKey { level, x, y })
        .map(|built| Response::new(built.payload.clone()))
}

#[tauri::command]
fn scan_roots() -> Roots {
    roots()
}

#[tauri::command]
fn full_disk_access_needed(root: String) -> bool {
    needs_full_disk_access(&root)
}

/// `WKWebView` holds rendering near 60 Hz unless this private preference is off. Without the API, it stays capped.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn uncap_frame_rate(webview: &tauri::webview::PlatformWebview) {
    use objc2::{class, msg_send, rc::Retained, runtime::AnyObject, sel};
    use objc2_foundation::{NSArray, NSString};

    let preferences_class = class!(WKPreferences);
    if !preferences_class.metaclass().responds_to(sel!(_features)) {
        return;
    }
    // SAFETY: `inner` is a live WKWebView on the main thread, and each private selector is checked before it's sent.
    unsafe {
        let view = webview.inner().cast::<AnyObject>();
        let configuration: Retained<AnyObject> = msg_send![view, configuration];
        let preferences: Retained<AnyObject> = msg_send![&*configuration, preferences];
        if !preferences.class().responds_to(sel!(_setEnabled:forFeature:)) {
            return;
        }
        let features: Retained<NSArray<AnyObject>> = msg_send![preferences_class, _features];
        for feature in &features {
            let key: Retained<NSString> = msg_send![&*feature, key];
            if key.to_string() == "PreferPageRenderingUpdatesNear60FPSEnabled" {
                let _: () = msg_send![&*preferences, _setEnabled: false, forFeature: &*feature];
            }
        }
    }
}

/// Runs the app.
///
/// # Panics
/// If the Tauri runtime fails to start.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = Arc::new(Engine::default());
    if let Some(port) = std::env::var("DISK_ATLAS_HTTP").ok().and_then(|p| p.parse().ok()) {
        let engine = engine.clone();
        std::thread::spawn(move || {
            if let Err(e) = http::serve(&engine, port) {
                eprintln!("HTTP mirror on port {port} failed: {e}");
            }
        });
    }
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.setup(|app| {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window("main") {
            window.with_webview(|webview| uncap_frame_rate(&webview))?;
        }
        Ok(())
    });
    builder
        .plugin(tauri_plugin_opener::init())
        .manage(engine)
        .invoke_handler(tauri::generate_handler![
            start_scan, scan_status, world, patch, tile, scan_roots, full_disk_access_needed
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
