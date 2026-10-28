use crate::geo::{self, Geo};
use crate::scan::{self, Progress};
use crate::util::NONE;
use crate::world::{self, Patch};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Compacted patch (kept as a parent for finer ones), its encoded payload, and a summary line.
type Built = (Arc<Patch>, Arc<Vec<u8>>, String);
const MAX_PATCHES: usize = 16;
const MAX_TILES: usize = 24;

fn built(geo: &Geo, mut patch: Patch) -> Built {
    let bin = world::encode(geo, &patch);
    let summary = format!("{} ({:.1} MB)", world::stats(&patch), bin.len() as f64 / 1e6);
    patch.compact();
    (Arc::new(patch), Arc::new(bin), summary)
}

/// Insert and evict the oldest entries beyond `cap`, never evicting `keep`.
fn insert_capped<K: std::hash::Hash + Eq + Copy>(map: &mut HashMap<K, (Built, u64)>, key: K, value: Built, cap: usize, keep: Option<K>) {
    let tick = map.values().map(|v| v.1).max().unwrap_or(0) + 1;
    map.insert(key, (value, tick));
    while map.len() > cap {
        let Some(old) = map.iter().filter(|(k, _)| Some(**k) != keep).min_by_key(|(_, v)| v.1).map(|(k, _)| *k) else { break };
        map.remove(&old);
    }
}

#[derive(Default)]
pub struct Engine {
    progress: Progress,
    phase: Mutex<(String, String)>,
    started: Mutex<Option<Instant>>,
    geo: RwLock<Option<Arc<Geo>>>,
    patches: Mutex<HashMap<u32, (Built, u64)>>,
    tiles: Mutex<HashMap<(u32, i64, i64), (Built, u64)>>,
}

pub fn roots() -> Value {
    let disk = if Path::new("/System/Volumes/Data").exists() { "/System/Volumes/Data" } else { "/" };
    json!({ "home": std::env::var("HOME").unwrap_or_default(), "disk": disk })
}

impl Engine {
    fn set_phase(&self, phase: &str, message: &str) {
        *self.phase.lock().unwrap() = (phase.into(), message.into());
    }

    pub fn start(self: &Arc<Self>, root: String) -> Result<(), String> {
        {
            let mut phase = self.phase.lock().unwrap();
            if phase.0 == "scanning" || phase.0 == "mapping" {
                return Err("A scan is already running".into());
            }
            *phase = ("scanning".into(), root.clone());
        }
        self.progress.reset();
        *self.started.lock().unwrap() = Some(Instant::now());
        *self.geo.write().unwrap() = None;
        self.patches.lock().unwrap().clear();
        self.tiles.lock().unwrap().clear();
        let me = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = me.run(&root) {
                me.set_phase("failed", &e);
            }
        });
        Ok(())
    }

    fn run(&self, root: &str) -> Result<(), String> {
        let path = Path::new(root);
        let node = scan::scan(path, &self.progress).map_err(|e| format!("{root}: {e}"))?;
        self.set_phase("mapping", root);
        let (capacity, free) = scan::volume(path);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let geo = Arc::new(geo::build(node, root, capacity, free, now));
        let world = built(&geo, world::build_world(&geo));
        self.patches.lock().unwrap().insert(geo::WORLD, (world, 0));
        *self.geo.write().unwrap() = Some(geo);
        self.set_phase("ready", root);
        Ok(())
    }

    pub fn status(&self) -> Value {
        let (phase, message) = self.phase.lock().unwrap().clone();
        let elapsed = self.started.lock().unwrap().map_or(0.0, |t| t.elapsed().as_secs_f64());
        let p = &self.progress;
        json!({
            "phase": if phase.is_empty() { "idle" } else { &phase },
            "message": message,
            "files": p.files.load(Relaxed),
            "dirs": p.dirs.load(Relaxed),
            "bytes": p.bytes.load(Relaxed),
            "denied": p.denied.load(Relaxed),
            "elapsed": elapsed,
            "regions": self.geo.read().unwrap().as_ref().map_or(0, |g| g.regions.len()),
        })
    }

    pub fn geo(&self) -> Option<Arc<Geo>> {
        self.geo.read().unwrap().clone()
    }

    pub fn world(&self) -> Result<Vec<u8>, String> {
        self.patch(geo::WORLD)
    }

    pub fn patch(&self, id: u32) -> Result<Vec<u8>, String> {
        let geo = self.geo().ok_or("No map yet")?;
        let parent = {
            let patches = self.patches.lock().unwrap();
            if let Some(((_, bin, _), _)) = patches.get(&id) {
                return Ok(bin.to_vec());
            }
            let region = geo.regions.get(id as usize).ok_or("Unknown region")?;
            let mut a = region.parent;
            while a != NONE && !patches.contains_key(&a) {
                a = geo.regions[a as usize].parent;
            }
            (patches.get(&a).ok_or("No parent patch")?.0).0.clone()
        };
        let b = built(&geo, world::build_patch(&geo, &parent, id));
        let out = b.1.to_vec();
        insert_capped(&mut self.patches.lock().unwrap(), id, b, MAX_PATCHES, Some(geo::WORLD));
        Ok(out)
    }

    pub fn tile(&self, level: u32, ix: i64, iy: i64) -> Result<Vec<u8>, String> {
        let geo = self.geo().ok_or("No map yet")?;
        if !(1..=24).contains(&level) {
            return Err("Bad tile level".into());
        }
        if let Some(((_, bin, _), _)) = self.tiles.lock().unwrap().get(&(level, ix, iy)) {
            return Ok(bin.to_vec());
        }
        let size = 0.5f64.powi(level as i32);
        let (x0, y0) = (ix as f64 * size, iy as f64 * size);
        let overlaps = |p: &Patch| {
            let [a, b, c, d] = p.bounds();
            a < x0 + size && c > x0 && b < y0 + size && d > y0
        };
        let mut parents: Vec<Arc<Patch>> = self.patches.lock().unwrap().values().map(|((p, ..), _)| p.clone()).filter(|p| overlaps(p)).collect();
        parents.extend(self.tiles.lock().unwrap().iter().filter(|(k, ((p, ..), _))| k.0 < level && overlaps(p)).map(|(_, ((p, ..), _))| p.clone()));
        parents.sort_by(|a, b| a.spacing().total_cmp(&b.spacing()));
        let refs: Vec<&Patch> = parents.iter().map(|p| p.as_ref()).collect();
        let b = built(&geo, world::build_tile(&geo, &refs, level, ix, iy));
        let out = b.1.to_vec();
        insert_capped(&mut self.tiles.lock().unwrap(), (level, ix, iy), b, MAX_TILES, None);
        Ok(out)
    }

    pub fn patch_stats(&self, id: u32) -> Option<String> {
        self.patches.lock().unwrap().get(&id).map(|((.., summary), _)| summary.clone())
    }
}
