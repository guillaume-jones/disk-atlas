//! Runs surveys in the background and serves their world, patches and tiles, caching the most recent ones.
use crate::geo::{self, Geo, RegionId};
use crate::lock;
use crate::scan::{self, Progress};
use crate::world::{self, Built, MAX_TILE_LEVEL, Patch, TileKey};
use serde::{Serialize, Serializer};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const MAX_PATCHES: usize = 16;
const MAX_TILES: usize = 24;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("A scan is already running")]
    Busy,
    #[error("No map yet")]
    NoMap,
    #[error("Unknown region {0}")]
    UnknownRegion(u32),
    #[error("Bad tile level {0}")]
    BadTileLevel(u32),
    #[error("{root}: {source}")]
    Scan { root: String, source: io::Error },
    #[error("Encoding failed: {0}")]
    Encode(#[from] serde_json::Error),
}

/// The frontend receives errors as their message.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    Scanning,
    Mapping,
    Ready,
    Failed,
}

#[derive(Serialize)]
pub struct Status {
    pub phase: Phase,
    /// The root being surveyed, or the error once failed.
    pub message: String,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub denied: u64,
    /// Seconds since the survey started.
    pub elapsed: f64,
    pub regions: usize,
}

#[derive(Serialize)]
pub struct Roots {
    home: String,
    disk: &'static str,
}

pub fn roots() -> Roots {
    let disk = if cfg!(windows) {
        "C:\\"
    } else if Path::new("/System/Volumes/Data").exists() {
        "/System/Volumes/Data"
    } else {
        "/"
    };
    let home = std::env::home_dir().map_or_else(String::new, |h| h.to_string_lossy().into_owned());
    Roots { home, disk }
}

#[derive(Default)]
struct Job {
    phase: Phase,
    message: String,
    started: Option<Instant>,
    progress: Arc<Progress>,
}

/// A finished survey and everything generated from it.
struct Survey {
    geo: Arc<Geo>,
    world: Arc<Built>,
    patches: Mutex<Recent<RegionId>>,
    tiles: Mutex<Recent<TileKey>>,
}

/// The `capacity` most recently built entries.
struct Recent<K> {
    capacity: usize,
    entries: HashMap<K, Arc<Built>>,
    order: VecDeque<K>,
}

impl<K: Copy + Eq + Hash> Recent<K> {
    fn new(capacity: usize) -> Self {
        Recent { capacity, entries: HashMap::new(), order: VecDeque::new() }
    }

    fn get(&self, key: &K) -> Option<Arc<Built>> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: K, built: Arc<Built>) {
        if self.entries.insert(key, built).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

#[derive(Default)]
pub struct Engine {
    job: Mutex<Job>,
    survey: Mutex<Option<Arc<Survey>>>,
}

impl Engine {
    /// Starts surveying `root` in the background; poll [`Engine::status`] until it's ready.
    pub fn start(self: &Arc<Self>, root: String) -> Result<(), Error> {
        {
            let mut job = lock(&self.job);
            if matches!(job.phase, Phase::Scanning | Phase::Mapping) {
                return Err(Error::Busy);
            }
            *job = Job {
                phase: Phase::Scanning,
                message: root.clone(),
                started: Some(Instant::now()),
                progress: Arc::default(),
            };
        }
        *lock(&self.survey) = None;
        let engine = self.clone();
        std::thread::spawn(move || {
            let (phase, message) = match engine.run_survey(&root) {
                Ok(()) => (Phase::Ready, root),
                Err(e) => (Phase::Failed, e.to_string()),
            };
            engine.set_phase(phase, message);
        });
        Ok(())
    }

    fn set_phase(&self, phase: Phase, message: String) {
        let mut job = lock(&self.job);
        job.phase = phase;
        job.message = message;
    }

    fn run_survey(&self, root: &str) -> Result<(), Error> {
        let path = Path::new(root);
        let progress = lock(&self.job).progress.clone();
        let node = scan::scan(path, &progress).map_err(|source| Error::Scan { root: root.into(), source })?;
        self.set_phase(Phase::Mapping, root.into());
        let space = scan::volume_space(path).unwrap_or_default();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let geo = Arc::new(geo::build(node, root, space, now));
        let world = Arc::new(world::build_world(&geo)?);
        let survey = Survey {
            geo,
            world,
            patches: Mutex::new(Recent::new(MAX_PATCHES)),
            tiles: Mutex::new(Recent::new(MAX_TILES)),
        };
        *lock(&self.survey) = Some(Arc::new(survey));
        Ok(())
    }

    pub fn status(&self) -> Status {
        let job = lock(&self.job);
        let p = &job.progress;
        Status {
            phase: job.phase,
            message: job.message.clone(),
            files: p.files.load(Relaxed),
            dirs: p.dirs.load(Relaxed),
            bytes: p.bytes.load(Relaxed),
            denied: p.denied.load(Relaxed),
            elapsed: job.started.map_or(0.0, |t| t.elapsed().as_secs_f64()),
            regions: self.geo().map_or(0, |g| g.regions().len()),
        }
    }

    fn current(&self) -> Result<Arc<Survey>, Error> {
        lock(&self.survey).clone().ok_or(Error::NoMap)
    }

    pub fn geo(&self) -> Option<Arc<Geo>> {
        lock(&self.survey).as_ref().map(|s| s.geo.clone())
    }

    pub fn world(&self) -> Result<Arc<Built>, Error> {
        Ok(self.current()?.world.clone())
    }

    pub fn patch(&self, id: u32) -> Result<Arc<Built>, Error> {
        let survey = self.current()?;
        let geo = &survey.geo;
        let id = geo.lookup(id).ok_or(Error::UnknownRegion(id))?;
        let Some(parent) = geo.region(id).parent else {
            return Ok(survey.world.clone());
        };
        if let Some(built) = lock(&survey.patches).get(&id) {
            return Ok(built);
        }
        // Layouts depend on the parent patch, so rebuild it if evicted rather than borrowing a coarser ancestor.
        let parent = self.patch(parent.into())?;
        let built = Arc::new(world::build_patch(geo, &parent.patch, id)?);
        lock(&survey.patches).insert(id, built.clone());
        Ok(built)
    }

    pub fn tile(&self, tile: TileKey) -> Result<Arc<Built>, Error> {
        let survey = self.current()?;
        if !(1..=MAX_TILE_LEVEL).contains(&tile.level) {
            return Err(Error::BadTileLevel(tile.level));
        }
        if let Some(built) = lock(&survey.tiles).get(&tile) {
            return Ok(built);
        }
        let mut parents: Vec<Arc<Built>> = vec![survey.world.clone()];
        parents.extend(lock(&survey.patches).entries.values().cloned());
        parents.extend(
            lock(&survey.tiles)
                .entries
                .iter()
                .filter(|(k, _)| k.level < tile.level)
                .map(|(_, b)| b.clone()),
        );
        parents.retain(|b| b.patch.overlaps(tile));
        parents.sort_by(|a, b| a.patch.spacing().total_cmp(&b.patch.spacing()));
        let parents: Vec<&Patch> = parents.iter().map(|b| &b.patch).collect();
        let built = Arc::new(world::build_tile(&survey.geo, &parents, tile)?);
        lock(&survey.tiles).insert(tile, built.clone());
        Ok(built)
    }
}
