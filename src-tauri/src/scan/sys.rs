//! The crate's only unsafe code: the two macOS filesystem calls std doesn't wrap. Everything else, including
//! parsing the kernel's packed attribute buffers, is safe and bounds-checked.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ffi::CString;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Not exported by libc: ask for a per-entry error code instead of failing the whole batch.
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
/// `enum vtype` values from `<sys/vnode.h>`.
const VREG: u32 = 1;
const VDIR: u32 = 2;
const BULK_BUFFER_BYTES: usize = 256 << 10;

const BULK_ATTRS: libc::attrlist = libc::attrlist {
    bitmapcount: libc::ATTR_BIT_MAP_COUNT,
    reserved: 0,
    commonattr: libc::ATTR_CMN_RETURNED_ATTRS
        | libc::ATTR_CMN_NAME
        | libc::ATTR_CMN_OBJTYPE
        | libc::ATTR_CMN_MODTIME
        | libc::ATTR_CMN_FILEID
        | ATTR_CMN_ERROR,
    volattr: 0,
    dirattr: libc::ATTR_DIR_ENTRYCOUNT | libc::ATTR_DIR_MOUNTSTATUS,
    fileattr: libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_ALLOCSIZE,
    forkattr: 0,
};

thread_local! {
    static BULK_BUFFER: RefCell<Vec<u8>> = RefCell::new(vec![0; BULK_BUFFER_BYTES]);
}

pub enum EntryKind {
    Dir {
        entries: u32,
        mount_point: bool,
    },
    File {
        inode: u64,
        links: u32,
        allocated: u64,
        mtime: i64,
    },
}

/// Lists `dir` with getattrlistbulk: names, types and sizes in one syscall per batch instead of an lstat per entry.
/// `visit` gets each directory and regular file; other entry types and entries the kernel couldn't read are skipped.
pub fn read_dir_bulk(dir: &Path, mut visit: impl FnMut(&[u8], EntryKind)) -> io::Result<()> {
    let dir = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY).open(dir)?;
    BULK_BUFFER.with_borrow_mut(|buf| {
        loop {
            let mut attrs = BULK_ATTRS;
            // SAFETY: `dir` is an open directory for the whole call, `attrs` is a valid attrlist on our stack, and
            // `buf` is a writable allocation of `buf.len()` bytes borrowed exclusively by this thread.
            let count = unsafe { libc::getattrlistbulk(dir.as_raw_fd(), (&raw mut attrs).cast(), buf.as_mut_ptr().cast(), buf.len(), 0) };
            let Ok(count) = usize::try_from(count) else { return };
            if count == 0 {
                return;
            }
            let mut start = 0;
            for _ in 0..count {
                let Some(len) = Reader { buf, pos: start }.u32().filter(|&len| len > 0) else {
                    return;
                };
                let entry = buf.get(start..start + len as usize).unwrap_or_default();
                if let Some((name, kind)) = parse_entry(entry) {
                    visit(name, kind);
                }
                start += len as usize;
            }
        }
    });
    Ok(())
}

/// One packed entry: length, returned-attribute bitmap, optional error, then the requested attributes in bit order.
fn parse_entry(entry: &[u8]) -> Option<(&[u8], EntryKind)> {
    let mut r = Reader { buf: entry, pos: size_of::<u32>() };
    let common = r.u32()?;
    let _vol = r.u32()?;
    let dir_attrs = r.u32()?;
    let _file = r.u32()?;
    let _fork = r.u32()?;
    if common & ATTR_CMN_ERROR != 0 && r.u32()? != 0 {
        return None;
    }
    let name_ref_at = r.pos;
    let name_offset = r.i32()?;
    let name_len = r.u32()? as usize;
    let name_start = name_ref_at.checked_add_signed(name_offset as isize)?;
    let name = entry.get(name_start..name_start + name_len.checked_sub(1)?)?;
    let obj_type = r.u32()?;
    let mtime = r.i64()?;
    let _mtime_nanos = r.i64()?;
    let inode = r.u64()?;
    let kind = match obj_type {
        VDIR => {
            let entries = if dir_attrs & libc::ATTR_DIR_ENTRYCOUNT != 0 { r.u32()? } else { 0 };
            let mount = if dir_attrs & libc::ATTR_DIR_MOUNTSTATUS != 0 { r.u32()? } else { 0 };
            EntryKind::Dir {
                entries,
                mount_point: mount & libc::DIR_MNTSTATUS_MNTPOINT != 0,
            }
        }
        VREG => {
            let links = r.u32()?;
            let allocated = r.i64()?.max(0) as u64;
            EntryKind::File { inode, links, allocated, mtime }
        }
        _ => return None,
    };
    Some((name, kind))
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let bytes = self.buf.get(self.pos..self.pos + N)?.try_into().ok()?;
        self.pos += N;
        Some(bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_ne_bytes)
    }

    fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_ne_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_ne_bytes)
    }

    fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_ne_bytes)
    }
}

#[derive(Default)]
pub struct VolumeSpace {
    pub capacity: u64,
    pub free: u64,
}

/// Size and free space of the volume holding `path`. On APFS free space is shared across the container.
pub fn volume_space(path: &Path) -> Option<VolumeSpace> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statfs is plain old data, so all-zero bytes are a valid value.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is NUL-terminated and `st` is a valid, writable statfs.
    if unsafe { libc::statfs(path.as_ptr(), &raw mut st) } != 0 {
        return None;
    }
    let block = u64::from(st.f_bsize);
    Some(VolumeSpace { capacity: st.f_blocks * block, free: st.f_bavail * block })
}
