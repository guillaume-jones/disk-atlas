use crate::names::Lang;
use crate::scan::{BUNDLES, Kind, Node, Stats};
use crate::util::{NONE, Rng, hash_str, join};
use serde::Serialize;
use std::collections::HashSet;

pub const WORLD: u32 = 0;
/// Collapse a folder into its child when the child holds this share of it.
const COLLAPSE: f64 = 0.98;
/// Children smaller than this share of their largest sibling become dust.
const SIBLING_CUT: f64 = 0.02;
const MAX_CHILDREN: usize = 60;
const CONTINENT_SHARE: f64 = 0.04;
const ISLAND_SHARE: f64 = 0.0005;
const MAX_ISLANDS: usize = 40;
const MIN_CITY: u64 = 50;
/** The ice is sparsely settled: only big file clusters get an outpost. */
const MIN_OUTPOST: u64 = 5000;
const OUTPOST: &[&str] = &["Fort {}", "{} Base", "{} Station", "Camp {}", "Outpost {}"];
const POLAR: &[&str] = &["System", "Library", "private", "usr", "cores", "sw", "opt", "bin", "sbin", "Volumes"];
const GENERIC: &[&str] = &[
    "data", "library", "contents", "resources", "caches", "cache", "src", "lib", "bin", "files", "tmp", "temp",
    "default", "application support", "containers", "group containers", "users", "private", "var", "folders",
    "shared", "local", "share", "store", "storage", "blobs", "objects", "current", "latest", "versions", "t", "c",
    "x", "0", "documents and settings",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RKind {
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
    pub id: u32,
    pub parent: u32,
    pub depth: u8,
    pub kind: RKind,
    pub name: String,
    pub label: String,
    pub path: String,
    pub size: u64,
    pub count: u64,
    pub mtime: f64,
    #[serde(skip)]
    pub dust: Stats,
    #[serde(skip)]
    word: Option<String>,
    pub city: Option<String>,
    pub children: Vec<u32>,
}

pub struct Geo {
    pub regions: Vec<Region>,
    /// Bytes represented by the whole map; equals `disk` for whole-disk surveys.
    pub capacity: u64,
    pub disk: u64,
    pub scanned: u64,
    pub free: u64,
    pub now: f64,
    pub root: String,
}

fn generic(n: &str) -> bool {
    let l = n.to_lowercase();
    GENERIC.contains(&l.as_str())
        || (n.len() >= 16 && n.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        || n.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The most meaningful real name in a collapsed chain of folders: the deepest non-generic one.
fn informative(chain: &[String]) -> String {
    chain.iter().rev().find(|n| !generic(n)).or(chain.last()).cloned().unwrap_or_default()
}

/// Tokens that never make a place name on their own.
const STOP: &[&str] = &[
    "com", "org", "net", "www", "the", "and", "for", "this", "that", "with", "from", "into", "apple", "google",
    "microsoft", "mozilla", "app", "apps", "lib", "libs", "src", "tmp", "temp", "data", "file", "files", "default",
    "stable", "latest", "current", "main", "master", "darwin", "users", "user",
];
const CONTINENT: &[&str] = &["{}land", "{}ia", "Greater {}"];
const ISLAND: &[&str] = &["Isle of {}", "{} Island", "The {} Isles"];
const NATION: &[&str] = &["Kingdom of {}", "{}stan", "Republic of {}", "Duchy of {}", "{}mark"];
const PROVINCE: &[&str] = &["{}shire", "{} Province", "March of {}", "{} Vale"];
const LOCALE: &[&str] = &["{} Downs", "{} Reach", "{} Fells", "{} Heath", "{} Hollow", "{} Weald"];
const GLACIER: &[&str] = &["{} Glacier", "{} Icefield", "{} Shelf"];
const BARRENS: &[&str] = &["{} Barrens", "{} Expanse", "{} Plateau"];
const BUNDLE: &[&str] = &["Free City of {}", "{} Free City"];
const CITY: &[&str] = &[
    "{}ton", "{}burg", "Port {}", "{}ville", "{}ham", "Fort {}", "{}ford", "{}field", "{}stead", "{}minster", "{}holm",
    "{}wick", "{}by", "{}mouth", "{}dale", "{}haven", "Saint {}", "{}polis", "{}grad", "Mount {}", "{}bridge", "{}port",
];
const QUALIFIERS: &[&str] = &["Upper", "Lower", "North", "South", "East", "West", "New", "Old", "Far", "Little"];

/// Rejects random identifiers like `ekpyefnfmdbiwhbx`: too long, or four consonants in a row. Short acronyms (npm, nvm) pass.
fn pronounceable(t: &str) -> bool {
    let lower = t.to_lowercase();
    let run = lower.split(|c| "aeiouy".contains(c)).map(str::len).max().unwrap_or(0);
    lower.len() <= 4 || (lower.len() <= 16 && run < 4 && lower.chars().any(|c| "aeiouy".contains(c)))
}

fn singular(w: &str) -> String {
    if w.len() > 4 && w.ends_with("ies") {
        format!("{}y", &w[..w.len() - 3])
    } else if w.len() > 4 && w.ends_with('s') && !["ss", "us", "is"].iter().any(|e| w.ends_with(e)) {
        w[..w.len() - 1].to_string()
    } else {
        w.to_string()
    }
}

/// The most distinctive word in a folder or file name, e.g. `com.apple.Safari` → Safari, `.gradle` → Gradle.
fn word_of(name: &str) -> Option<String> {
    if name == "node_modules" {
        return Some("Node".into());
    }
    let stem = match name.rsplit_once('.') {
        Some((s, ext))
            if !s.is_empty()
                && (BUNDLES.iter().any(|b| b.eq_ignore_ascii_case(ext))
                    || (ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))) =>
        {
            s
        }
        _ => name,
    };
    let mut tokens = Vec::new();
    for part in stem.split(|c: char| !c.is_alphanumeric()) {
        let mut cur = String::new();
        let mut prev_lower = false;
        for ch in part.chars() {
            if ch.is_uppercase() && prev_lower {
                tokens.push(std::mem::take(&mut cur));
            }
            prev_lower = ch.is_lowercase();
            cur.push(ch);
        }
        tokens.push(cur);
    }
    tokens
        .into_iter()
        .filter(|t| t.chars().count() >= 3 && t.chars().all(char::is_alphabetic) && !STOP.contains(&t.to_lowercase().as_str()) && pronounceable(t))
        .map(|t| singular(&t.to_lowercase()))
        .fold(None::<String>, |best, t| if best.as_ref().is_some_and(|b| b.len() >= t.len()) { best } else { Some(t) })
        .map(|t| {
            let mut c = t.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        })
}

/// The outermost meaningful name in a collapsed chain: `.pyenv/versions/3.14/lib` is Pyenv, not Version.
fn distinctive(chain: &[String]) -> Option<String> {
    chain.iter().find_map(|n| word_of(n))
}

/// Fill a pattern; suffixes fuse with the word ("Cache" + "{}ia" → "Cachia").
fn fill(pattern: &str, w: &str) -> String {
    match pattern.strip_prefix("{}") {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_lowercase()) => {
            let vowel = |c: char| "aeiouy".contains(c);
            let base = if rest.starts_with(vowel) && w.ends_with(vowel) { &w[..w.len() - 1] } else { w };
            format!("{base}{rest}")
        }
        _ => pattern.replace("{}", w),
    }
}

struct Item {
    node: Node,
    path: String,
    polar: bool,
}

impl Geo {
    pub fn ancestor_at(&self, mut r: u32, depth: u8) -> u32 {
        while r != NONE && self.regions[r as usize].depth > depth {
            r = self.regions[r as usize].parent;
        }
        r
    }

    pub fn is_within(&self, mut r: u32, x: u32) -> bool {
        while r != NONE {
            if r == x {
                return true;
            }
            r = self.regions[r as usize].parent;
        }
        false
    }

    pub fn top(&self, r: u32) -> &Region {
        &self.regions[self.ancestor_at(r, 1) as usize]
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, parent: u32, kind: RKind, name: String, label: String, path: String, total: Stats, dust: Stats) -> u32 {
        let id = self.regions.len() as u32;
        let depth = if parent == NONE { 0 } else { self.regions[parent as usize].depth + 1 };
        self.regions.push(Region {
            id,
            parent,
            depth,
            kind,
            name,
            label,
            path,
            size: total.size,
            count: total.count,
            mtime: total.mtime(),
            dust,
            word: None,
            city: None,
            children: Vec::new(),
        });
        if parent != NONE {
            self.regions[parent as usize].children.push(id);
        }
        id
    }

    fn add_node(&mut self, node: Node, path: String, parent: u32, kind: Option<RKind>, lang: &Lang, used: &mut HashSet<String>, min_size: u64) -> u32 {
        let outer = node.total;
        let mut chain = vec![node.name.clone()];
        let (mut node, mut path) = (node, path);
        let mut dust = Stats::default();
        while node.kind == Kind::Dir {
            let total = node.total.size as f64;
            let Some(big) = node.children.iter().position(|c| c.total.size as f64 >= COLLAPSE * total) else { break };
            let child = node.children.swap_remove(big);
            dust.add(&node.dust);
            node.children.iter().for_each(|c| dust.add(&c.total));
            path = join(&path, &child.name);
            chain.push(child.name.clone());
            node = child;
        }
        dust.add(&node.dust);
        // Only small bundles read as city-states; a 10 GB Xcode is a whole territory.
        let kind = kind.unwrap_or(match node.kind {
            Kind::Bundle if (outer.size as f64) < 0.002 * self.capacity as f64 => RKind::Bundle,
            Kind::File => RKind::File,
            _ => RKind::Land,
        });
        let mut children = std::mem::take(&mut node.children);
        children.sort_by(|a, b| b.total.size.cmp(&a.total.size));
        let cut = ((children.first().map_or(0, |c| c.total.size) as f64 * SIBLING_CUT) as u64).max(min_size);
        let keep = children.iter().take(MAX_CHILDREN).take_while(|c| c.total.size >= cut).count();
        children.drain(keep..).for_each(|c| dust.add(&c.total));

        let mut rng = Rng::new(hash_str(&path));
        let word = distinctive(&chain).or_else(|| self.regions[parent as usize].word.clone());
        let depth = self.regions[parent as usize].depth + 1;
        let polar = self.top(parent).kind == RKind::Polar;
        let patterns = match kind {
            RKind::Continent => CONTINENT,
            RKind::Island => ISLAND,
            RKind::Bundle => BUNDLE,
            RKind::File => BARRENS,
            _ if polar => GLACIER,
            _ if depth <= 2 => NATION,
            _ if depth == 3 => PROVINCE,
            _ => LOCALE,
        };
        let name = unique(word.as_deref(), patterns, lang, &mut rng, used);
        let id = self.push(parent, kind, name, informative(&chain), path.clone(), outer, dust);
        self.regions[id as usize].word = word.clone();
        for c in children {
            let p = join(&path, &c.name);
            self.add_node(c, p, id, None, lang, used, min_size);
        }
        let leaf = self.regions[id as usize].children.is_empty();
        let pop = if leaf { outer.count } else { dust.count };
        if pop >= if polar { MIN_OUTPOST } else { MIN_CITY } {
            let city = unique(word.as_deref(), if polar { OUTPOST } else { CITY }, lang, &mut rng, used);
            self.regions[id as usize].city = Some(city);
        }
        id
    }

}

/// A name for `word` (or a generated one) in the first free pattern, falling back to qualifiers like "Upper".
fn unique(word: Option<&str>, patterns: &[&str], lang: &Lang, rng: &mut Rng, used: &mut HashSet<String>) -> String {
    let start = rng.below(patterns.len());
    for attempt in 0..40 {
        let w = match word {
            Some(w) if attempt < 20 => w.to_string(),
            _ => lang.word(rng),
        };
        let pattern = patterns[(start + attempt) % patterns.len()];
        let base = fill(pattern, &w);
        let name = if attempt % 20 < patterns.len() { base } else { format!("{} {base}", QUALIFIERS[(attempt % 20) % QUALIFIERS.len()]) };
        if used.insert(name.clone()) {
            return name;
        }
    }
    format!("{} {}", lang.word(rng), rng.below(1000))
}

pub fn build(root: Node, root_path: &str, capacity: u64, free: u64, now: f64) -> Geo {
    let system_root = root_path == "/" || root_path == "/System/Volumes/Data";
    let is_polar = |name: &str, top: bool| name == ".Trash" || (top && system_root && (POLAR.contains(&name) || name.starts_with('.')));
    let mut items: Vec<Item> = root
        .children
        .into_iter()
        .map(|c| Item { path: join(root_path, &c.name), polar: is_polar(&c.name, true), node: c })
        .collect();
    let mut loose = root.dust;

    // A single dominant folder (e.g. /Users or ~) would make a one-continent world, so promote its children.
    loop {
        let land: u64 = items.iter().filter(|i| !i.polar).map(|i| i.node.total.size).sum();
        let Some(i) = (0..items.len()).filter(|&i| !items[i].polar).max_by_key(|&i| items[i].node.total.size) else { break };
        let n = &items[i].node;
        if n.kind != Kind::Dir || n.children.len() < 2 || (n.total.size as f64) < 0.45 * land as f64 {
            break;
        }
        let it = items.swap_remove(i);
        loose.add(&it.node.dust);
        items.extend(it.node.children.into_iter().map(|c| Item { path: join(&it.path, &c.name), polar: is_polar(&c.name, false), node: c }));
    }

    let (polar, mut land): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|i| i.polar);
    land.sort_by(|a, b| b.node.total.size.cmp(&a.node.total.size));
    let land_total = land.iter().map(|i| i.node.total.size).sum::<u64>() + loose.size;
    let scanned = land_total + polar.iter().map(|i| i.node.total.size).sum::<u64>();
    let disk = capacity.max(scanned + free);
    // A folder survey only maps that folder; the ocean gets a proportionate share instead of the whole disk's free space.
    let (capacity, unseen) = if system_root {
        (disk, disk.saturating_sub(free).saturating_sub(scanned))
    } else {
        (scanned + free.clamp(scanned / 3, scanned * 6 / 5), 0)
    };
    let min_size = (capacity as f64 * 1e-6) as u64;

    let mut geo = Geo { regions: Vec::new(), capacity, disk, scanned, free, now, root: root_path.into() };
    let mut used = HashSet::new();
    let world_stats = Stats { size: capacity, count: 0, weight: 0.0, mtime_sum: 0.0 };
    geo.push(NONE, RKind::World, "The Known World".into(), root_path.into(), root_path.into(), world_stats, Stats::default());

    let mut langs = Vec::new();
    let mut islands = 0;
    for it in land {
        let share = it.node.total.size as f64 / land_total.max(1) as f64;
        let kind = if share >= CONTINENT_SHARE {
            RKind::Continent
        } else if share >= ISLAND_SHARE && islands < MAX_ISLANDS {
            islands += 1;
            RKind::Island
        } else {
            loose.add(&it.node.total);
            continue;
        };
        let lang = if kind == RKind::Continent || langs.is_empty() {
            langs.push(Lang::new(hash_str(&it.path)));
            langs.last().unwrap()
        } else {
            &langs[hash_str(&it.path) as usize % langs.len()]
        };
        geo.add_node(it.node, it.path, WORLD, Some(kind), lang, &mut used, min_size);
    }

    let polar_lang = Lang::new(hash_str("polar"));
    if !polar.is_empty() || unseen > 0 {
        let mut total = Stats { size: unseen, ..Stats::default() };
        polar.iter().for_each(|i| total.add(&i.node.total));
        used.insert("The System Ice".into());
        let pid = geo.push(WORLD, RKind::Polar, "The System Ice".into(), "System files".into(), root_path.into(), total, Stats::default());
        for it in polar {
            geo.add_node(it.node, it.path, pid, None, &polar_lang, &mut used, min_size);
        }
        if unseen > 0 {
            let s = Stats { size: unseen, ..Stats::default() };
            geo.push(pid, RKind::Unseen, "The Unseen Ice".into(), "Sealed system volume, snapshots & unreadable folders".into(), "".into(), s, s);
        }
    }
    if loose.size > 0 {
        geo.push(WORLD, RKind::Isles, "The Scattered Isles".into(), "Small top-level items".into(), root_path.into(), loose, loose);
    }
    let sea = Stats { size: free, ..Stats::default() };
    geo.push(WORLD, RKind::Ocean, "The Unclaimed Sea".into(), "Free space".into(), "".into(), sea, sea);
    geo
}
