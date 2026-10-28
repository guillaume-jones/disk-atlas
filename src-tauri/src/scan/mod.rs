//! Walks a folder tree in parallel into a size-summarised tree of [`Node`]s.
#[cfg(target_os = "macos")]
mod sys;

use crate::lock;
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::UNIX_EPOCH;

/// Largest files kept as individual nodes per directory; the rest become dust.
const KEEP_FILES: usize = 4;
/// Anything smaller can never get its own region, so fold it into dust while scanning to save memory.
const MIN_NODE_BYTES: u64 = 512 * 1024;
/// Empty files still weigh one block towards a folder's average age.
const MIN_FILE_WEIGHT: u64 = 4096;
/// getattrlistbulk reads one directory sequentially; bigger ones are stat-ed in parallel to keep more reads in
/// flight on a cold cache.
#[cfg(target_os = "macos")]
const HUGE_DIR_ENTRIES: u32 = 2000;
/// The walk recurses once per directory level.
const SCAN_STACK_BYTES: usize = 64 << 20;
/// Unit of `st_blocks`.
#[cfg(unix)]
const STAT_BLOCK_BYTES: u64 = 512;
const JUNK: &[&str] = &[".DS_Store", ".localized", "desktop.ini", "Thumbs.db"];
/// Extensions of folders macOS presents as single documents or apps.
pub const BUNDLE_EXTENSIONS: &[&str] = &[
    "app", "photoslibrary", "photolibrary", "musiclibrary", "tvlibrary", "imovielibrary", "fcpbundle", "aplibrary", "lrlibrary", "lrdata",
    "logicx", "band", "bundle", "framework", "plugin", "kext", "appex", "xpc", "xcarchive", "dSYM", "docarchive", "playground",
    "xcodeproj", "xcworkspace", "sparsebundle", "vmwarevm", "pvm", "utm", "pkg", "mpkg",
];

#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dir,
    File,
    Bundle,
}

/// Size, file count and size-weighted modification time of a set of files.
#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    pub size: u64,
    pub count: u64,
    weight: f64,
    mtime_sum: f64,
}

impl Stats {
    fn file(size: u64, mtime: f64) -> Self {
        let weight = size.max(MIN_FILE_WEIGHT) as f64;
        Stats { size, count: 1, weight, mtime_sum: weight * mtime }
    }

    pub fn bytes(size: u64) -> Self {
        Stats { size, ..Stats::default() }
    }

    pub fn add(&mut self, other: &Stats) {
        self.size += other.size;
        self.count += other.count;
        self.weight += other.weight;
        self.mtime_sum += other.mtime_sum;
    }

    pub fn has_files(&self) -> bool {
        self.weight > 0.0
    }

    /// Size-weighted mean modification time, in Unix seconds.
    pub fn mtime(&self) -> f64 {
        if self.has_files() { self.mtime_sum / self.weight } else { 0.0 }
    }
}

pub struct Node {
    pub name: String,
    pub kind: Kind,
    pub total: Stats,
    /// Files and folders too small to keep as children.
    pub dust: Stats,
    pub children: Vec<Node>,
}

impl Node {
    fn new(name: String, kind: Kind, total: Stats) -> Self {
        Node {
            name,
            kind,
            total,
            dust: Stats::default(),
            children: Vec::new(),
        }
    }

    fn absorb(&mut self, child: Node) {
        self.total.add(&child.total);
        if child.total.size < MIN_NODE_BYTES {
            self.dust.add(&child.total);
        } else {
            self.children.push(child);
        }
    }

    fn absorb_dust(&mut self, stats: &Stats) {
        self.total.add(stats);
        self.dust.add(stats);
    }
}

pub fn is_bundle(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && BUNDLE_EXTENSIONS.iter().any(|b| b.eq_ignore_ascii_case(ext)))
}

struct Walk<'a> {
    device: u64,
    progress: &'a Progress,
    /// Inodes of hard-linked files already counted, so each is counted once.
    linked: Mutex<HashSet<u64>>,
}

struct Subdir {
    path: PathBuf,
    name: String,
    huge: bool,
}

enum Entry {
    Dir(Subdir),
    File {
        name: String,
        inode: u64,
        links: u64,
        stats: Stats,
    },
}

/// Device, inode, hard-link count and allocated bytes. Windows' std doesn't expose the first three, so there every
/// file counts as unlinked and every folder as on the same volume.
struct Identity {
    device: u64,
    inode: u64,
    links: u64,
    allocated: u64,
}

#[cfg(unix)]
fn identity(meta: &fs::Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    Identity {
        device: meta.dev(),
        inode: meta.ino(),
        links: meta.nlink(),
        allocated: meta.blocks() * STAT_BLOCK_BYTES,
    }
}

#[cfg(not(unix))]
fn identity(meta: &fs::Metadata) -> Identity {
    Identity { device: 0, inode: 0, links: 1, allocated: meta.len() }
}

#[derive(Default)]
pub struct VolumeSpace {
    pub capacity: u64,
    pub free: u64,
}

/// Size and free space of the volume holding `path`. On APFS free space is shared across the container.
pub fn volume_space(path: &Path) -> Option<VolumeSpace> {
    let stats = fs4::statvfs(path).ok()?;
    Some(VolumeSpace { capacity: stats.total_space(), free: stats.available_space() })
}

pub fn scan(root: &Path, progress: &Progress) -> io::Result<Node> {
    let walk = Walk {
        device: identity(&fs::metadata(root)?).device,
        progress,
        linked: Mutex::new(HashSet::new()),
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(SCAN_STACK_BYTES)
        .build()
        .map_err(io::Error::other)?;
    let name = root.file_name().map_or_else(|| "/".into(), |n| n.to_string_lossy().into_owned());
    Ok(pool.install(|| walk.dir(Subdir { path: root.to_path_buf(), name, huge: false })))
}

impl Walk<'_> {
    fn dir(&self, dir: Subdir) -> Node {
        self.progress.dirs.fetch_add(1, Relaxed);
        let mut node = Node::new(dir.name, Kind::Dir, Stats::default());
        let listing = if dir.huge {
            self.list_parallel(&dir.path)
        } else {
            self.list_bulk(&dir.path)
        };
        let Ok(entries) = listing else {
            self.progress.denied.fetch_add(1, Relaxed);
            return node;
        };
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for entry in entries {
            match entry {
                Entry::Dir(subdir) => subdirs.push(subdir),
                Entry::File { name, inode, links, stats } => {
                    if links > 1 && !lock(&self.linked).insert(inode) {
                        continue;
                    }
                    self.progress.files.fetch_add(1, Relaxed);
                    self.progress.bytes.fetch_add(stats.size, Relaxed);
                    files.push(Node::new(name, Kind::File, stats));
                }
            }
        }
        let subdirs: Vec<Node> = subdirs
            .into_par_iter()
            .map(|subdir| {
                let bundle = is_bundle(&subdir.name);
                let mut child = self.dir(subdir);
                if bundle {
                    child.kind = Kind::Bundle;
                    child.dust = child.total;
                    child.children = Vec::new();
                }
                child
            })
            .collect();
        files.sort_by_key(|f| Reverse(f.total.size));
        for (rank, file) in files.into_iter().enumerate() {
            if rank < KEEP_FILES {
                node.absorb(file);
            } else {
                node.absorb_dust(&file.total);
            }
        }
        for child in subdirs {
            node.absorb(child);
        }
        node
    }

    #[cfg(not(target_os = "macos"))]
    fn list_bulk(&self, path: &Path) -> io::Result<Vec<Entry>> {
        self.list_parallel(path)
    }

    #[cfg(target_os = "macos")]
    fn list_bulk(&self, path: &Path) -> io::Result<Vec<Entry>> {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let mut entries = Vec::new();
        sys::read_dir_bulk(path, |raw_name, kind| {
            let name = String::from_utf8_lossy(raw_name).into_owned();
            if JUNK.contains(&name.as_str()) {
                return;
            }
            match kind {
                sys::EntryKind::Dir { entries: count, mount_point } => {
                    let path = path.join(OsStr::from_bytes(raw_name));
                    let foreign = mount_point && fs::symlink_metadata(&path).map_or(true, |m| identity(&m).device != self.device);
                    if !foreign {
                        entries.push(Entry::Dir(Subdir { path, name, huge: count > HUGE_DIR_ENTRIES }));
                    }
                }
                sys::EntryKind::File { inode, links, allocated, mtime } => {
                    entries.push(Entry::File {
                        name,
                        inode,
                        links: u64::from(links),
                        stats: Stats::file(allocated, mtime as f64),
                    });
                }
            }
        })?;
        Ok(entries)
    }

    fn list_parallel(&self, path: &Path) -> io::Result<Vec<Entry>> {
        let listing: Vec<fs::DirEntry> = fs::read_dir(path)?.flatten().collect();
        Ok(listing
            .into_par_iter()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if JUNK.contains(&name.as_str()) {
                    return None;
                }
                let meta = entry.metadata().ok()?;
                let id = identity(&meta);
                if meta.is_dir() && id.device == self.device {
                    Some(Entry::Dir(Subdir { path: entry.path(), name, huge: false }))
                } else if meta.is_file() {
                    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
                    let stats = Stats::file(id.allocated, mtime);
                    Some(Entry::File { name, inode: id.inode, links: id.links, stats })
                } else {
                    None
                }
            })
            .collect())
    }
}
