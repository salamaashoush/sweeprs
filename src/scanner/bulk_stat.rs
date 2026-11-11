//! macOS-specific fast directory scanning using `getattrlistbulk`.
//!
//! This syscall fetches file name, type, and size for all entries in a directory
//! in a single batched call, avoiding per-file `stat()` overhead that dominates
//! scan time on large trees (e.g. `node_modules` with 100k+ files).

#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::os::raw::c_void;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::tree::Listing;

// --- Constants from <sys/attr.h> ---
const ATTR_BIT_MAP_COUNT: u16 = 5;
const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;
const ATTR_CMN_NAME: u32 = 0x0000_0001;
const ATTR_CMN_DEVID: u32 = 0x0000_0002;
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const ATTR_CMN_OBJTYPE: u32 = 0x0000_0008;
const ATTR_CMN_FILEID: u32 = 0x0200_0000;
const ATTR_FILE_LINKCOUNT: u32 = 0x0000_0001;
const ATTR_FILE_ALLOCSIZE: u32 = 0x0000_0004;
// Read through the fork slot when `FSOPT_ATTR_CMN_EXTENDED` is set.
/// The bytes of a file not shared with any APFS clone: what deleting it frees.
const ATTR_CMNEXT_PRIVATESIZE: u32 = 0x0000_0008;
/// Per-file flags, among them whether the file shares blocks with a clone.
const ATTR_CMNEXT_EXT_FLAGS: u32 = 0x0000_0200;
const FSOPT_NOFOLLOW: u64 = 0x0000_0001;
const FSOPT_ATTR_CMN_EXTENDED: u64 = 0x0000_0020;
const EF_MAY_SHARE_BLOCKS: u64 = 0x0000_0001;
const EF_SHARES_ALL_BLOCKS: u64 = 0x0000_0040;

// --- Constants from <sys/vnode.h> ---
const VREG: u32 = 1;
const VDIR: u32 = 2;

// errno value for APFS bug workaround
const ERANGE: i32 = 34;
const EINVAL: i32 = 22;

/// Cleared the first time a volume rejects the extended attribute group.
static EXTENDED_SUPPORTED: AtomicBool = AtomicBool::new(true);

// 256 KiB buffer -- large enough to batch hundreds of entries per syscall.
const BUF_SIZE: usize = 256 * 1024;

// --- FFI types ---

#[repr(C)]
struct AttrList {
    bitmapcount: u16,
    reserved: u16,
    commonattr: u32,
    volattr: u32,
    dirattr: u32,
    fileattr: u32,
    forkattr: u32,
}

#[allow(unsafe_code)]
unsafe extern "C" {
    fn getattrlist(
        path: *const std::os::raw::c_char,
        attr_list: *mut AttrList,
        attr_buf: *mut c_void,
        attr_buf_size: usize,
        options: u64,
    ) -> i32;
    fn getattrlistbulk(
        dirfd: i32,
        attr_list: *mut AttrList,
        attr_buf: *mut c_void,
        attr_buf_size: usize,
        options: u64,
    ) -> i32;
}

// Thread-local reusable buffer for `getattrlistbulk` syscalls.
// Avoids allocating a fresh 256 KiB buffer per directory in recursive scans.
thread_local! {
    static SCAN_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; BUF_SIZE]);
}

/// One directory's files, hard-linked files and subdirectories, read with
/// `getattrlistbulk`. Subdirectories on another device are left out.
#[allow(unsafe_code)]
pub fn list_dir(dir: &Path, root_dev: u64) -> Option<Listing> {
    let dir_file = std::fs::File::open(dir).ok()?;
    let mut listing = Listing::default();
    let fd = dir_file.as_raw_fd();

    let mut al = AttrList {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: ATTR_CMN_RETURNED_ATTRS
            | ATTR_CMN_NAME
            | ATTR_CMN_DEVID
            | ATTR_CMN_ERROR
            | ATTR_CMN_OBJTYPE
            | ATTR_CMN_FILEID,
        volattr: 0,
        dirattr: 0,
        fileattr: ATTR_FILE_LINKCOUNT | ATTR_FILE_ALLOCSIZE,
        forkattr: 0,
    };
    let mut options = 0u64;
    if EXTENDED_SUPPORTED.load(Ordering::Relaxed) {
        al.forkattr = ATTR_CMNEXT_EXT_FLAGS;
        options = FSOPT_ATTR_CMN_EXTENDED;
    }

    SCAN_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();

        loop {
            let ret = unsafe {
                getattrlistbulk(
                    fd,
                    &raw mut al,
                    buf.as_mut_ptr().cast::<c_void>(),
                    buf.len(),
                    options,
                )
            };

            if ret == 0 {
                break;
            }
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(ERANGE) {
                    continue;
                }
                if err.raw_os_error() == Some(EINVAL) && options != 0 {
                    EXTENDED_SUPPORTED.store(false, Ordering::Relaxed);
                    al.forkattr = 0;
                    options = 0;
                    continue;
                }
                break;
            }

            let entry_count = ret as usize;
            let mut offset = 0usize;

            for _ in 0..entry_count {
                if offset + 4 > buf.len() {
                    break;
                }

                let entry_len =
                    u32::from_ne_bytes(buf[offset..offset + 4].try_into().unwrap()) as usize;
                if entry_len == 0 || offset + entry_len > buf.len() {
                    break;
                }

                let entry = &buf[offset..offset + entry_len];
                let parsed = parse_entry(entry);

                listing.entries += 1;

                record(&mut listing, dir, parsed, root_dev);
                offset += entry_len;
            }
        }
    });
    Some(listing)
}

/// Add one parsed entry to its directory's listing.
fn record(listing: &mut Listing, dir: &Path, parsed: ParsedEntry, root_dev: u64) {
    match parsed {
        ParsedEntry::File {
            name,
            size,
            inode,
            nlink,
            ext_flags,
        } => {
            let size = if ext_flags & EF_SHARES_ALL_BLOCKS != 0 {
                0
            } else if ext_flags & EF_MAY_SHARE_BLOCKS != 0 {
                private_size(&dir.join(&name)).map_or(size, |private| private.min(size))
            } else {
                size
            };
            if nlink > 1 {
                listing.linked.push((inode, nlink, size));
            } else {
                listing.files += size;
            }
        }
        ParsedEntry::Dir { name, dev } => {
            let other_device = root_dev != 0 && dev != 0 && dev != root_dev;
            if !other_device && !name.is_empty() {
                listing.subdirs.push(name);
            }
        }
        ParsedEntry::Other | ParsedEntry::Error => {}
    }
}

/// The bytes of `path` no APFS clone shares, asked of one file at a time.
///
/// Computing it reads the file's extent records, which costs seconds across a
/// million files, so it is only asked for files flagged as sharing blocks.
#[allow(unsafe_code)]
fn private_size(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut al = AttrList {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: ATTR_CMNEXT_PRIVATESIZE,
    };
    let mut buf = [0u8; 32];
    // SAFETY: `c_path` is NUL-terminated, `al` and `buf` outlive the call, and
    // the length passed is the buffer's own.
    let ret = unsafe {
        getattrlist(
            c_path.as_ptr(),
            &raw mut al,
            buf.as_mut_ptr().cast::<c_void>(),
            buf.len(),
            FSOPT_ATTR_CMN_EXTENDED | FSOPT_NOFOLLOW,
        )
    };
    if ret != 0 {
        return None;
    }
    // u32 length, then the requested off_t.
    let private = i64::from_ne_bytes(buf.get(4..12)?.try_into().ok()?);
    u64::try_from(private).ok()
}

enum ParsedEntry {
    File {
        name: String,
        size: u64,
        inode: u64,
        nlink: u32,
        ext_flags: u64,
    },
    Dir {
        name: String,
        dev: u64,
    },
    Other,
    Error,
}

fn parse_entry(entry: &[u8]) -> ParsedEntry {
    // Minimum entry: 4 (length) + 20 (attribute_set_t) = 24 bytes
    if entry.len() < 24 {
        return ParsedEntry::Error;
    }

    let mut pos = 4; // skip length field

    // attribute_set_t: 5 x u32 = 20 bytes (common, vol, dir, file, fork)
    let returned_common = u32::from_ne_bytes(entry[pos..pos + 4].try_into().unwrap());
    let returned_file = u32::from_ne_bytes(entry[pos + 12..pos + 16].try_into().unwrap());
    let returned_fork = u32::from_ne_bytes(entry[pos + 16..pos + 20].try_into().unwrap());
    pos += 20;

    // ATTR_CMN_ERROR (if returned) -- always first after returned_attrs
    if returned_common & ATTR_CMN_ERROR != 0 {
        if pos + 4 > entry.len() {
            return ParsedEntry::Error;
        }
        let error = u32::from_ne_bytes(entry[pos..pos + 4].try_into().unwrap());
        pos += 4;
        if error != 0 {
            return ParsedEntry::Error;
        }
    }

    // ATTR_CMN_NAME (attrreference_t: offset i32 + length u32 = 8 bytes)
    let mut name = String::new();
    if returned_common & ATTR_CMN_NAME != 0 {
        if pos + 8 > entry.len() {
            return ParsedEntry::Error;
        }
        let data_offset = i32::from_ne_bytes(entry[pos..pos + 4].try_into().unwrap());
        let name_start = pos.wrapping_add(data_offset as usize);
        if name_start < entry.len() {
            let name_bytes: &[u8] = &entry[name_start..];
            let end = memchr::memchr(0, name_bytes).unwrap_or(name_bytes.len());
            name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
        }
        pos += 8;
    }

    // ATTR_CMN_DEVID (dev_t: i32) -- device number, follows NAME in bit order
    let mut dev = 0u64;
    if returned_common & ATTR_CMN_DEVID != 0 {
        if pos + 4 > entry.len() {
            return ParsedEntry::Error;
        }
        let raw_dev = i32::from_ne_bytes(entry[pos..pos + 4].try_into().unwrap());
        dev = raw_dev as u64;
        pos += 4;
    }

    // ATTR_CMN_OBJTYPE (fsobj_type_t: u32)
    let mut obj_type = 0u32;
    if returned_common & ATTR_CMN_OBJTYPE != 0 {
        if pos + 4 > entry.len() {
            return ParsedEntry::Error;
        }
        obj_type = u32::from_ne_bytes(entry[pos..pos + 4].try_into().unwrap());
        pos += 4;
    }

    // ATTR_CMN_FILEID (u64) -- inode number
    let mut inode = 0u64;
    if returned_common & ATTR_CMN_FILEID != 0 {
        if pos + 8 > entry.len() {
            return ParsedEntry::Error;
        }
        inode = u64::from_ne_bytes(entry[pos..pos + 8].try_into().unwrap());
        pos += 8;
    }

    // ATTR_FILE_LINKCOUNT (u32) -- only present for files
    let mut nlink = 1u32;
    if returned_file & ATTR_FILE_LINKCOUNT != 0 {
        if pos + 4 > entry.len() {
            return ParsedEntry::Error;
        }
        nlink = u32::from_ne_bytes(entry[pos..pos + 4].try_into().unwrap());
        pos += 4;
    }

    // ATTR_FILE_ALLOCSIZE (off_t: i64) -- physical on-disk size, only present for files
    let mut file_size = 0i64;
    if returned_file & ATTR_FILE_ALLOCSIZE != 0 {
        if pos + 8 > entry.len() {
            return ParsedEntry::Error;
        }
        file_size = i64::from_ne_bytes(entry[pos..pos + 8].try_into().unwrap());
        pos += 8;
    }

    // ATTR_CMNEXT_EXT_FLAGS (u64). A clone is counted by its private part
    // only, so when both clones are inside the tree their shared blocks are
    // left out: an undercount, where counting each in full would promise
    // space that never comes back.
    let mut ext_flags = 0u64;
    if returned_fork & ATTR_CMNEXT_EXT_FLAGS != 0 && pos + 8 <= entry.len() {
        ext_flags = u64::from_ne_bytes(entry[pos..pos + 8].try_into().unwrap());
    }

    if obj_type == VDIR {
        ParsedEntry::Dir { name, dev }
    } else if obj_type == VREG {
        ParsedEntry::File {
            name,
            size: file_size.max(0) as u64,
            inode,
            nlink,
            ext_flags,
        }
    } else {
        ParsedEntry::Other
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    const MIB: usize = 1024 * 1024;

    #[test]
    fn an_apfs_clone_frees_only_what_it_does_not_share() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tree = tmp.path().join("tree");
        std::fs::create_dir(&tree).expect("tree");
        std::fs::write(tmp.path().join("original"), vec![9u8; 4 * MIB]).expect("write");
        let cloned = std::process::Command::new("cp")
            .args(["-c"])
            .arg(tmp.path().join("original"))
            .arg(tree.join("clone"))
            .status()
            .expect("cp");
        assert!(cloned.success());

        let size = crate::scanner::tree::measure(&tree, false).0;
        assert!(
            size < MIB as u64,
            "a clone sharing all its blocks reported {size} bytes"
        );
    }

    #[test]
    fn a_directory_lists_its_files_and_subdirectories() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tmp.path().join("sub")).expect("sub");
        std::fs::write(tmp.path().join("a"), vec![1u8; 8192]).expect("a");
        let dev = tmp.path().metadata().expect("meta").dev();
        let listing = list_dir(tmp.path(), dev).expect("listing");
        assert_eq!(listing.entries, 2);
        assert_eq!(listing.subdirs, ["sub"]);
        assert!(listing.files >= 8192);
    }
}
