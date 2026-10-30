//! Headless companion to the app: `atlas scan <root>` prints a summary, `atlas serve [port]` exposes the same API
//! over HTTP so the frontend can run in a normal browser (Vite proxies /api here).
use disk_atlas_lib::{Engine, Error, Geo, Phase, RegionId, http};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 7878;
const POLL_INTERVAL: Duration = Duration::from_millis(20);

fn gb(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

/// The first child of `id` that has children of its own, so a patch for it adds detail.
fn refinable(geo: &Geo, id: RegionId) -> Option<RegionId> {
    geo.region(id)
        .children
        .iter()
        .copied()
        .find(|&c| !geo.region(c).children.is_empty())
}

fn scan(root: String) -> Result<(), Box<dyn std::error::Error>> {
    let engine = Arc::new(Engine::default());
    let started = Instant::now();
    engine.start(root)?;
    let status = loop {
        let status = engine.status();
        match status.phase {
            Phase::Ready => break status,
            Phase::Failed => return Err(status.message.into()),
            _ => {
                eprint!("\r{} files, {} dirs, {}   ", status.files, status.dirs, gb(status.bytes));
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    };
    eprintln!();
    let elapsed = started.elapsed().as_secs_f64();
    println!(
        "scanned {} files / {} dirs ({} unreadable) in {elapsed:.1}s",
        status.files, status.dirs, status.denied
    );
    let geo = engine.geo().ok_or(Error::NoMap)?;
    println!(
        "capacity {} free {} regions {}",
        gb(geo.capacity),
        gb(geo.free),
        geo.regions().len()
    );
    println!("world: {}", engine.world()?.summary);
    for &c in &geo.region(RegionId::WORLD).children {
        let r = geo.region(c);
        let kids = r.children.len();
        println!(
            "  {:<10?} {:<26} {:>10}  {:>8} files  {kids} kids  label={:?} path={}",
            r.kind,
            r.word.as_deref().unwrap_or("-"),
            gb(r.size),
            r.count,
            r.label,
            r.path
        );
        for &k in r.children.iter().take(4) {
            let k = geo.region(k);
            println!(
                "      {:<8?} {:<20} {:>10}  label={:?} settled={}",
                k.kind,
                k.word.as_deref().unwrap_or("-"),
                gb(k.size),
                k.label,
                k.settled
            );
        }
    }
    println!("max depth {}", geo.regions().iter().map(|r| r.depth).max().unwrap_or(0));
    let big = refinable(&geo, RegionId::WORLD);
    for id in [big, big.and_then(|b| refinable(&geo, b))].into_iter().flatten() {
        let started = Instant::now();
        let built = engine.patch(id.into())?;
        println!(
            "patch {id} ({}): {} in {:.2}s",
            geo.region(id).label,
            built.summary,
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("scan") => match args
            .get(2)
            .cloned()
            .or_else(|| std::env::home_dir().map(|h| h.to_string_lossy().into_owned()))
        {
            Some(root) => scan(root),
            None => Err("no root given and no home folder found".into()),
        },
        Some("serve") => {
            let port = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
            http::serve(&Arc::new(Engine::default()), port).map_err(|e| e as Box<dyn std::error::Error>)
        }
        _ => Err("usage: atlas scan [root] | atlas serve [port]".into()),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
