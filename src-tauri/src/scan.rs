use rayon::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::{CString, OsStr};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Largest files kept as individual nodes per directory; the rest become dust.
const KEEP_FILES: usize = 4;
/// Anything smaller can never get its own region, so fold it into dust while scanning to save memory.
const MIN_NODE: u64 = 512 * 1024;
const JUNK: &[&str] = &[".DS_Store", ".localized"];
pub const BUNDLES: &[&str] = &[
    "app", "photoslibrary", "photolibrary", "musiclibrary", "tvlibrary", "imovielibrary", "fcpbundle",
    "aplibrary", "lrlibrary", "lrdata", "logicx", "band", "bundle", "framework", "plugin", "kext",
    "appex", "xpc", "xcarchive", "dSYM", "docarchive", "playground", "xcodeproj", "xcworkspace",
    "sparsebundle", "vmwarevm", "pvm", "utm", "pkg", "mpkg",
];

#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
}

impl Progress {
    pub fn reset(&self) {
        for c in [&self.files, &self.dirs, &self.bytes, &self.denied] {
            c.store(0, Relaxed);
        }
    }
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
    pub weight: f64,
    pub mtime_sum: f64,
}

impl Stats {
    fn file(size: u64, mtime: f64) -> Self {
        let weight = size.max(4096) as f64;
        Stats { size, count: 1, weight, mtime_sum: weight * mtime }
    }

    pub fn add(&mut self, o: &Stats) {
        self.size += o.size;
        self.count += o.count;
        self.weight += o.weight;
        self.mtime_sum += o.mtime_sum;
    }

    pub fn mtime(&self) -> f64 {
        if self.weight > 0.0 { self.mtime_sum / self.weight } else { 0.0 }
    }
}

pub struct Node {
    pub name: String,
    pub kind: Kind,
    pub total: Stats,
    pub dust: Stats,
    pub children: Vec<Node>,
}

impl Node {
    fn new(name: String, kind: Kind) -> Self {
        Node { name, kind, total: Stats::default(), dust: Stats::default(), children: Vec::new() }
    }

    fn absorb(&mut self, child: Node) {
        self.total.add(&child.total);
        if child.total.size < MIN_NODE {
            self.dust.add(&child.total);
        } else {
            self.children.push(child);
        }
    }

    fn absorb_dust(&mut self, s: &Stats) {
        self.total.add(s);
        self.dust.add(s);
    }
}

struct Ctx<'a> {
    dev: u64,
    progress: &'a Progress,
    links: Mutex<HashSet<u64>>,
}

fn is_bundle(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && BUNDLES.iter().any(|b| b.eq_ignore_ascii_case(ext)))
}

const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const VREG: u32 = 1;
const VDIR: u32 = 2;
const HUGE: usize = 2000;
const ATTRS: libc::attrlist = libc::attrlist {
    bitmapcount: libc::ATTR_BIT_MAP_COUNT,
    reserved: 0,
    commonattr: libc::ATTR_CMN_RETURNED_ATTRS | libc::ATTR_CMN_NAME | libc::ATTR_CMN_OBJTYPE | libc::ATTR_CMN_MODTIME | libc::ATTR_CMN_FILEID | ATTR_CMN_ERROR,
    volattr: 0,
    dirattr: libc::ATTR_DIR_ENTRYCOUNT | libc::ATTR_DIR_MOUNTSTATUS,
    fileattr: libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_ALLOCSIZE,
    forkattr: 0,
};

thread_local! {
    static BUF: RefCell<Vec<u8>> = RefCell::new(vec![0; 256 << 10]);
}

fn take<T: Copy>(buf: &[u8], pos: &mut usize) -> T {
    let v = unsafe { std::ptr::read_unaligned(buf[*pos..*pos + size_of::<T>()].as_ptr().cast()) };
    *pos += size_of::<T>();
    v
}

enum Entry {
    Dir(CString, String, bool),
    File(String, u64, u64, Stats),
}

/// Lists a directory with getattrlistbulk: names, types and sizes in one syscall per batch instead of an lstat per entry.
/// That call is sequential per directory, so huge directories are listed with readdir and stat-ed in parallel instead,
/// which keeps more metadata reads in flight on a cold cache.
fn scan_dir(path: CString, name: String, huge: bool, ctx: &Ctx) -> Node {
    ctx.progress.dirs.fetch_add(1, Relaxed);
    let mut node = Node::new(name, Kind::Dir);
    let mut entries = Vec::new();
    if huge {
        let Ok(rd) = fs::read_dir(OsStr::from_bytes(path.as_bytes())) else {
            ctx.progress.denied.fetch_add(1, Relaxed);
            return node;
        };
        let rd: Vec<fs::DirEntry> = rd.flatten().collect();
        entries = rd
            .into_par_iter()
            .filter_map(|e| {
                let file_name = e.file_name().to_string_lossy().into_owned();
                if JUNK.contains(&file_name.as_str()) {
                    return None;
                }
                let meta = e.metadata().ok()?;
                if meta.is_dir() && meta.dev() == ctx.dev {
                    Some(Entry::Dir(CString::new(e.path().into_os_string().into_vec()).ok()?, file_name, false))
                } else if meta.is_file() {
                    Some(Entry::File(file_name, meta.ino(), meta.nlink(), Stats::file(meta.blocks() * 512, meta.mtime() as f64)))
                } else {
                    None
                }
            })
            .collect();
    } else {
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
        if fd < 0 {
            ctx.progress.denied.fetch_add(1, Relaxed);
            return node;
        }
        BUF.with_borrow_mut(|buf| loop {
            let mut attrs = ATTRS;
            let n = unsafe { libc::getattrlistbulk(fd, (&raw mut attrs).cast(), buf.as_mut_ptr().cast(), buf.len(), 0) };
            let mut entry = 0;
            for _ in 0..n.max(0) {
                let mut pos = entry;
                entry += take::<u32>(buf, &mut pos) as usize;
                let got: libc::attribute_set_t = take(buf, &mut pos);
                if got.commonattr & ATTR_CMN_ERROR != 0 && take::<u32>(buf, &mut pos) != 0 {
                    continue;
                }
                let name_at = pos;
                let name_ref: libc::attrreference_t = take(buf, &mut pos);
                let raw = &buf[name_at + name_ref.attr_dataoffset as usize..][..name_ref.attr_length as usize - 1];
                let file_name = String::from_utf8_lossy(raw).into_owned();
                if JUNK.contains(&file_name.as_str()) {
                    continue;
                }
                let kind: u32 = take(buf, &mut pos);
                let mtime: libc::timespec = take(buf, &mut pos);
                let ino: u64 = take(buf, &mut pos);
                if kind == VDIR {
                    let count: u32 = if got.dirattr & libc::ATTR_DIR_ENTRYCOUNT != 0 { take(buf, &mut pos) } else { 0 };
                    let mount: u32 = if got.dirattr & libc::ATTR_DIR_MOUNTSTATUS != 0 { take(buf, &mut pos) } else { 0 };
                    let mut child = path.as_bytes().to_vec();
                    child.push(b'/');
                    child.extend(raw);
                    let child = CString::new(child).unwrap();
                    let mut st: libc::stat = unsafe { std::mem::zeroed() };
                    let foreign = mount & libc::DIR_MNTSTATUS_MNTPOINT != 0
                        && (unsafe { libc::lstat(child.as_ptr(), &mut st) } != 0 || st.st_dev as u64 != ctx.dev);
                    if !foreign {
                        entries.push(Entry::Dir(child, file_name, count as usize > HUGE));
                    }
                } else if kind == VREG {
                    let links: u32 = take(buf, &mut pos);
                    let size = take::<libc::off_t>(buf, &mut pos) as u64;
                    entries.push(Entry::File(file_name, ino, links as u64, Stats::file(size, mtime.tv_sec as f64)));
                }
            }
            if n <= 0 {
                break;
            }
        });
        unsafe { libc::close(fd) };
    }
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for e in entries {
        match e {
            Entry::Dir(p, n, huge) => dirs.push((p, n, huge)),
            Entry::File(n, ino, links, s) => {
                if links > 1 && !ctx.links.lock().unwrap().insert(ino) {
                    continue;
                }
                ctx.progress.files.fetch_add(1, Relaxed);
                ctx.progress.bytes.fetch_add(s.size, Relaxed);
                files.push((n, s));
            }
        }
    }
    let subdirs: Vec<Node> = dirs
        .into_par_iter()
        .map(|(p, n, huge)| {
            let bundle = is_bundle(&n);
            let mut child = scan_dir(p, n, huge, ctx);
            if bundle {
                child.kind = Kind::Bundle;
                child.dust = child.total;
                child.children = Vec::new();
            }
            child
        })
        .collect();
    files.sort_by(|a, b| b.1.size.cmp(&a.1.size));
    for (i, (file_name, s)) in files.into_iter().enumerate() {
        if i < KEEP_FILES {
            node.absorb(Node { name: file_name, kind: Kind::File, total: s, dust: Stats::default(), children: Vec::new() });
        } else {
            node.absorb_dust(&s);
        }
    }
    for c in subdirs {
        node.absorb(c);
    }
    node
}

pub fn scan(root: &Path, progress: &Progress) -> std::io::Result<Node> {
    let meta = fs::metadata(root)?;
    let ctx = Ctx { dev: meta.dev(), progress, links: Mutex::new(HashSet::new()) };
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(64 << 20)
        .build()
        .map_err(std::io::Error::other)?;
    let name = root.file_name().map_or_else(|| "/".into(), |n| n.to_string_lossy().into_owned());
    let path = CString::new(root.as_os_str().as_bytes())?;
    Ok(pool.install(|| scan_dir(path, name, false, &ctx)))
}

/// (capacity, free) in bytes of the volume holding `root`. On APFS free space is shared across the container.
pub fn volume(root: &Path) -> (u64, u64) {
    let Ok(c) = CString::new(root.as_os_str().as_bytes()) else { return (0, 0) };
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return (0, 0);
    }
    let bs = st.f_bsize as u64;
    (st.f_blocks * bs, st.f_bavail * bs)
}
