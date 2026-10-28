//! Headless companion to the app: `atlas scan <root>` prints a summary, `atlas serve [port]` exposes the same API
//! over HTTP so the frontend can run in a normal browser (Vite proxies /api here).
use disk_atlas_lib::engine::Engine;
use disk_atlas_lib::http;
use disk_atlas_lib::geo::WORLD;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn gb(b: u64) -> String {
    format!("{:.2} GB", b as f64 / 1e9)
}

fn wait(engine: &Engine) -> Result<(), String> {
    loop {
        let s = engine.status();
        match s["phase"].as_str() {
            Some("ready") => return Ok(()),
            Some("failed") => return Err(s["message"].to_string()),
            _ => {
                eprint!("\r{} files, {} dirs, {}   ", s["files"], s["dirs"], gb(s["bytes"].as_u64().unwrap_or(0)));
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

fn scan(root: String) -> Result<(), String> {
    let engine = Arc::new(Engine::default());
    let t = Instant::now();
    engine.start(root)?;
    wait(&engine)?;
    let s = engine.status();
    eprintln!();
    println!("scanned {} files / {} dirs ({} unreadable) in {:.1}s", s["files"], s["dirs"], s["denied"], t.elapsed().as_secs_f64());
    let geo = engine.geo().unwrap();
    println!("capacity {} free {} regions {}", gb(geo.capacity), gb(geo.free), geo.regions.len());
    println!("world: {}", engine.patch_stats(WORLD).unwrap());
    for &c in &geo.regions[WORLD as usize].children {
        let r = &geo.regions[c as usize];
        println!("  {:<10?} {:<26} {:>10}  {:>8} files  {} kids  label={:?} path={}", r.kind, r.name, gb(r.size), r.count, r.children.len(), r.label, r.path);
        for &k in r.children.iter().take(4) {
            let k = &geo.regions[k as usize];
            println!("      {:<8?} {:<20} {:>10}  label={:?} city={:?}", k.kind, k.name, gb(k.size), k.label, k.city);
        }
    }
    let depth = geo.regions.iter().map(|r| r.depth).max().unwrap_or(0);
    println!("max depth {depth}");
    if let Some(&big) = geo.regions[WORLD as usize].children.iter().find(|&&c| !geo.regions[c as usize].children.is_empty()) {
        let t = Instant::now();
        engine.patch(big)?;
        println!("patch {} ({}): {} in {:.2}s", big, geo.regions[big as usize].name, engine.patch_stats(big).unwrap(), t.elapsed().as_secs_f64());
        if let Some(&next) = geo.regions[big as usize].children.iter().find(|&&c| !geo.regions[c as usize].children.is_empty()) {
            let t = Instant::now();
            engine.patch(next)?;
            println!("patch {} ({}): {} in {:.2}s", next, geo.regions[next as usize].name, engine.patch_stats(next).unwrap(), t.elapsed().as_secs_f64());
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("scan") => {
            let root = args.get(2).cloned().unwrap_or_else(|| std::env::var("HOME").unwrap());
            if let Err(e) = scan(root) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("serve") => http::serve(Arc::new(Engine::default()), args.get(2).and_then(|p| p.parse().ok()).unwrap_or(7878)),
        _ => eprintln!("usage: atlas scan [root] | atlas serve [port]"),
    }
}
