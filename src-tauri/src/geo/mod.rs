//! The region tree: the scanned folder tree pruned to what can be drawn, classified into continents, islands, ice,
//! isles and sea, and named.
mod names;

use crate::rng::{Rng, hash_str};
use crate::scan::{Kind, Node, Stats, VolumeSpace};
use names::{Gazetteer, Lang};
use serde::{Serialize, Serializer};
use std::cmp::Reverse;
use std::path::{MAIN_SEPARATOR, Path};

/// Collapse a folder into its child when the child holds this share of it.
const COLLAPSE_SHARE: f64 = 0.98;
/// Children smaller than this share of their largest sibling become dust.
const SIBLING_CUT: f64 = 0.02;
const MAX_CHILDREN: usize = 60;
/// Regions smaller than this share of the map are never drawn.
const MIN_REGION_SHARE: f64 = 1e-6;
/// Top-level folders with this share of the land are continents, smaller ones islands.
const CONTINENT_SHARE: f64 = 0.04;
const ISLAND_SHARE: f64 = 0.0005;
const MAX_ISLANDS: usize = 40;
/// A top-level folder holding this share of the land would make a one-continent world, so its children are promoted.
const DOMINANT_SHARE: f64 = 0.45;
/// Only small bundles read as city-states; a 10 GB Xcode is a whole territory.
const FREE_CITY_SHARE: f64 = 0.002;
const MIN_CITY_FILES: u64 = 50;
/// The ice is sparsely settled: only big file clusters get an outpost.
const MIN_OUTPOST_FILES: u64 = 5000;
/// Top-level folders of a whole-disk survey that freeze into polar ice.
const POLAR: &[&str] = &[
    "System",
    "Library",
    "private",
    "usr",
    "cores",
    "sw",
    "opt",
    "bin",
    "sbin",
    "Volumes",
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "$Recycle.Bin",
    "System Volume Information",
    "Recovery",
    "PerfLogs",
];
/// macOS's writable data volume, surveyed instead of `/` when it exists.
pub const DATA_VOLUME: &str = "/System/Volumes/Data";

/// How payloads spell "no region": the world's parent, cells outside a patch, a tile's root.
pub const NO_REGION: u32 = u32::MAX;

const WORLD_NAME: &str = "The Known World";
const ICE_NAME: &str = "The System Ice";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize)]
#[serde(transparent)]
pub struct RegionId(u32);

impl RegionId {
    pub const WORLD: RegionId = RegionId(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl From<RegionId> for u32 {
    fn from(id: RegionId) -> u32 {
        id.0
    }
}

impl std::fmt::Display for RegionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    World,
    Continent,
    Island,
    Polar,
    Ocean,
    Isles,
    Land,
    File,
    Bundle,
    Unseen,
}

#[derive(Serialize)]
pub struct Region {
    pub id: RegionId,
    #[serde(serialize_with = "parent_or_max")]
    pub parent: Option<RegionId>,
    pub depth: u8,
    pub kind: RegionKind,
    pub name: String,
    /// The real folder name shown under the place name.
    pub label: String,
    pub path: String,
    pub size: u64,
    pub count: u64,
    pub mtime: f64,
    #[serde(skip)]
    pub dust: Stats,
    pub city: Option<String>,
    pub children: Vec<RegionId>,
}

#[allow(
    clippy::ref_option,
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with signature"
)]
fn parent_or_max<S: Serializer>(parent: &Option<RegionId>, s: S) -> Result<S::Ok, S::Error> {
    parent.map_or(NO_REGION, u32::from).serialize(s)
}

pub struct Geo {
    regions: Vec<Region>,
    /// Bytes represented by the whole map; equals `disk` for whole-disk surveys.
    pub capacity: u64,
    pub disk: u64,
    pub scanned: u64,
    pub free: u64,
    /// Unix seconds when the survey was taken.
    pub now: f64,
    pub root: String,
}

impl Geo {
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    pub fn region(&self, id: RegionId) -> &Region {
        &self.regions[id.index()]
    }

    pub fn lookup(&self, id: u32) -> Option<RegionId> {
        ((id as usize) < self.regions.len()).then_some(RegionId(id))
    }

    /// `id`, its parent, and so on up to the world.
    pub fn ancestors(&self, id: RegionId) -> impl Iterator<Item = RegionId> + '_ {
        std::iter::successors(Some(id), |&r| self.region(r).parent)
    }

    pub fn ancestor_at(&self, id: RegionId, depth: u8) -> RegionId {
        self.ancestors(id)
            .find(|&r| self.region(r).depth <= depth)
            .unwrap_or(RegionId::WORLD)
    }

    pub fn is_within(&self, id: RegionId, ancestor: RegionId) -> bool {
        self.ancestors(id).any(|r| r == ancestor)
    }

    pub fn common_ancestor(&self, a: RegionId, b: RegionId) -> RegionId {
        let depth = self.region(a).depth.min(self.region(b).depth);
        let mut pairs = self
            .ancestors(self.ancestor_at(a, depth))
            .zip(self.ancestors(self.ancestor_at(b, depth)));
        pairs.find(|(x, y)| x == y).map_or(RegionId::WORLD, |(x, _)| x)
    }

    /// The top-level region (continent, island, ice...) holding `id`.
    pub fn top(&self, id: RegionId) -> &Region {
        self.region(self.ancestor_at(id, 1))
    }
}

struct NewRegion {
    kind: RegionKind,
    name: String,
    label: String,
    path: String,
    total: Stats,
    dust: Stats,
}

impl NewRegion {
    /// A fixed landmark whose content is drawn by its children.
    fn new(kind: RegionKind, name: &str, label: &str, path: &str, total: Stats) -> Self {
        NewRegion {
            kind,
            name: name.into(),
            label: label.into(),
            path: path.into(),
            total,
            dust: Stats::default(),
        }
    }

    /// A fixed landmark that is nothing but dust: sea, sealed ice, loose files.
    fn all_dust(kind: RegionKind, name: &str, label: &str, path: &str, total: Stats) -> Self {
        NewRegion {
            dust: total,
            ..NewRegion::new(kind, name, label, path, total)
        }
    }
}

struct Builder {
    geo: Geo,
    names: Gazetteer,
    min_size: u64,
}

impl Builder {
    fn push(&mut self, parent: Option<RegionId>, new: NewRegion) -> RegionId {
        let id = RegionId(self.geo.regions.len() as u32);
        let depth = parent.map_or(0, |p| self.geo.region(p).depth + 1);
        self.geo.regions.push(Region {
            id,
            parent,
            depth,
            kind: new.kind,
            name: new.name,
            label: new.label,
            path: new.path,
            size: new.total.size,
            count: new.total.count,
            mtime: new.total.mtime(),
            dust: new.dust,
            city: None,
            children: Vec::new(),
        });
        if let Some(parent) = parent {
            self.geo.regions[parent.index()].children.push(id);
        }
        id
    }

    /// Adds `node` and its drawable descendants under `parent`. `inherited` names regions whose folders have no
    /// meaningful word of their own.
    fn add_node(&mut self, node: Node, path: String, parent: RegionId, kind: Option<RegionKind>, lang: Lang, inherited: Option<&str>) {
        let outer = node.total;
        let mut chain = vec![node.name.clone()];
        let mut node = node;
        let mut path = path;
        let mut dust = Stats::default();
        while node.kind == Kind::Dir {
            let total = node.total.size as f64;
            let Some(big) = node.children.iter().position(|c| c.total.size as f64 >= COLLAPSE_SHARE * total) else {
                break;
            };
            let child = node.children.swap_remove(big);
            dust.add(&node.dust);
            node.children.iter().for_each(|c| dust.add(&c.total));
            path = join(&path, &child.name);
            chain.push(child.name.clone());
            node = child;
        }
        dust.add(&node.dust);
        let kind = kind.unwrap_or(match node.kind {
            Kind::Bundle if (outer.size as f64) < FREE_CITY_SHARE * self.geo.capacity as f64 => RegionKind::Bundle,
            Kind::File => RegionKind::File,
            _ => RegionKind::Land,
        });

        let mut children = std::mem::take(&mut node.children);
        children.sort_by_key(|c| Reverse(c.total.size));
        let cut = ((children.first().map_or(0, |c| c.total.size) as f64 * SIBLING_CUT) as u64).max(self.min_size);
        let keep = children.iter().take(MAX_CHILDREN).take_while(|c| c.total.size >= cut).count();
        children.drain(keep..).for_each(|c| dust.add(&c.total));

        let mut rng = Rng::new(hash_str(&path));
        let word = names::distinctive(&chain).or_else(|| inherited.map(str::to_owned));
        let depth = self.geo.region(parent).depth + 1;
        let polar = self.geo.top(parent).kind == RegionKind::Polar;
        let name = self
            .names
            .claim(word.as_deref(), names::region_patterns(kind, depth, polar), lang, &mut rng);
        let label = names::informative(&chain);
        let id = self.push(
            Some(parent),
            NewRegion { kind, name, label, path: path.clone(), total: outer, dust },
        );
        for child in children {
            let child_path = join(&path, &child.name);
            self.add_node(child, child_path, id, None, lang, word.as_deref());
        }
        let is_leaf = self.geo.region(id).children.is_empty();
        let population = if is_leaf { outer.count } else { dust.count };
        if population >= if polar { MIN_OUTPOST_FILES } else { MIN_CITY_FILES } {
            let city = self.names.claim(word.as_deref(), names::city_patterns(polar), lang, &mut rng);
            self.geo.regions[id.index()].city = Some(city);
        }
    }
}

fn join(base: &str, name: &str) -> String {
    if base.ends_with(['/', MAIN_SEPARATOR]) {
        format!("{base}{name}")
    } else {
        format!("{base}{MAIN_SEPARATOR}{name}")
    }
}

struct TopLevel {
    node: Node,
    path: String,
    polar: bool,
}

/// The root's children split into polar and land items (largest first), with a dominant folder like /Users or ~
/// replaced by its children so the world doesn't become one continent. Also returns the root's loose files.
fn top_level(root: Node, root_path: &str, whole_disk: bool) -> (Vec<TopLevel>, Vec<TopLevel>, Stats) {
    let is_polar = |name: &str, top: bool| name == ".Trash" || (top && whole_disk && (POLAR.contains(&name) || name.starts_with('.')));
    let item = |node: Node, parent: &str, top: bool| TopLevel {
        path: join(parent, &node.name),
        polar: is_polar(&node.name, top),
        node,
    };
    let mut items: Vec<TopLevel> = root.children.into_iter().map(|c| item(c, root_path, true)).collect();
    let mut loose = root.dust;
    loop {
        let land: u64 = items.iter().filter(|i| !i.polar).map(|i| i.node.total.size).sum();
        let Some(i) = (0..items.len())
            .filter(|&i| !items[i].polar)
            .max_by_key(|&i| items[i].node.total.size)
        else {
            break;
        };
        let node = &items[i].node;
        if node.kind != Kind::Dir || node.children.len() < 2 || (node.total.size as f64) < DOMINANT_SHARE * land as f64 {
            break;
        }
        let dominant = items.swap_remove(i);
        loose.add(&dominant.node.dust);
        items.extend(dominant.node.children.into_iter().map(|c| item(c, &dominant.path, false)));
    }
    let (polar, mut land): (Vec<TopLevel>, Vec<TopLevel>) = items.into_iter().partition(|i| i.polar);
    land.sort_by_key(|i| Reverse(i.node.total.size));
    (polar, land, loose)
}

impl Builder {
    /// Continents and islands; items too small for either join `loose`.
    fn add_land(&mut self, landmasses: Vec<TopLevel>, land_total: u64, loose: &mut Stats) {
        let mut langs: Vec<Lang> = Vec::new();
        let mut islands = 0;
        for item in landmasses {
            let share = item.node.total.size as f64 / land_total.max(1) as f64;
            let kind = if share >= CONTINENT_SHARE {
                RegionKind::Continent
            } else if share >= ISLAND_SHARE && islands < MAX_ISLANDS {
                islands += 1;
                RegionKind::Island
            } else {
                loose.add(&item.node.total);
                continue;
            };
            // Each continent speaks its own language; islands borrow one.
            let seed = hash_str(&item.path);
            let lang = if kind == RegionKind::Continent || langs.is_empty() {
                langs.push(Lang::new(seed));
                langs[langs.len() - 1]
            } else {
                langs[seed as usize % langs.len()]
            };
            self.add_node(item.node, item.path, RegionId::WORLD, Some(kind), lang, None);
        }
    }

    /// System folders, plus whatever of the disk the survey couldn't see, as polar ice.
    fn add_ice(&mut self, polar: Vec<TopLevel>, unseen: u64, root_path: &str) {
        let mut total = Stats::bytes(unseen);
        for item in &polar {
            total.add(&item.node.total);
        }
        self.names.reserve(ICE_NAME);
        let ice = self.push(
            Some(RegionId::WORLD),
            NewRegion::new(RegionKind::Polar, ICE_NAME, "System files", root_path, total),
        );
        let lang = Lang::new(hash_str("polar"));
        for item in polar {
            self.add_node(item.node, item.path, ice, None, lang, None);
        }
        if unseen > 0 {
            let label = "Sealed system volume, snapshots & unreadable folders";
            self.push(
                Some(ice),
                NewRegion::all_dust(RegionKind::Unseen, "The Unseen Ice", label, "", Stats::bytes(unseen)),
            );
        }
    }
}

pub fn build(root: Node, root_path: &str, space: VolumeSpace, now: f64) -> Geo {
    let VolumeSpace { capacity, free } = space;
    let whole_disk = root_path == DATA_VOLUME || Path::new(root_path).parent().is_none();
    let (polar, land, mut loose) = top_level(root, root_path, whole_disk);
    let land_total = land.iter().map(|i| i.node.total.size).sum::<u64>() + loose.size;
    let scanned = land_total + polar.iter().map(|i| i.node.total.size).sum::<u64>();
    let disk = capacity.max(scanned + free);
    // A folder survey only maps that folder; the ocean gets a proportionate share instead of the whole disk's free space.
    let (capacity, unseen) = if whole_disk {
        (disk, disk.saturating_sub(free).saturating_sub(scanned))
    } else {
        (scanned + free.clamp(scanned / 3, scanned * 6 / 5), 0)
    };
    let geo = Geo {
        regions: Vec::new(),
        capacity,
        disk,
        scanned,
        free,
        now,
        root: root_path.into(),
    };
    let mut b = Builder {
        geo,
        names: Gazetteer::default(),
        min_size: (capacity as f64 * MIN_REGION_SHARE) as u64,
    };
    b.push(
        None,
        NewRegion::new(RegionKind::World, WORLD_NAME, root_path, root_path, Stats::bytes(capacity)),
    );
    b.add_land(land, land_total, &mut loose);
    if !polar.is_empty() || unseen > 0 {
        b.add_ice(polar, unseen, root_path);
    }
    if loose.size > 0 {
        b.push(
            Some(RegionId::WORLD),
            NewRegion::all_dust(RegionKind::Isles, "The Scattered Isles", "Small top-level items", root_path, loose),
        );
    }
    b.push(
        Some(RegionId::WORLD),
        NewRegion::all_dust(RegionKind::Ocean, "The Unclaimed Sea", "Free space", "", Stats::bytes(free)),
    );
    b.geo
}
