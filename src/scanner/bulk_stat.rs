//! macOS-specific fast directory scanning using `getattrlistbulk`.
//!
//! This syscall fetches file name, type, and size for all entries in a directory
//! in a single batched call, avoiding per-file `stat()` overhead that dominates
//! scan time on large trees (e.g. `node_modules` with 100k+ files).

#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use std::os::raw::c_void;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

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

// --- Constants from <sys/vnode.h> ---
const VREG: u32 = 1;
const VDIR: u32 = 2;

// errno value for APFS bug workaround
const ERANGE: i32 = 34;

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
    fn getattrlistbulk(
        dirfd: i32,
        attr_list: *mut AttrList,
        attr_buf: *mut c_void,
        attr_buf_size: usize,
        options: u64,
    ) -> i32;
}

/// Compute total recursive file size under `path` using `getattrlistbulk`.
pub fn dir_size_bulk(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let root_dev = path.metadata().map(|m| m.dev()).unwrap_or(0);
    let mut total = 0u64;
    let mut seen_inodes = rustc_hash::FxHashSet::default();
    scan_recursive(path, &mut total, None, &mut seen_inodes, root_dev);
    total
}

/// Compute total recursive file size AND top-level item count using `getattrlistbulk`.
pub fn dir_size_and_count_bulk(path: &Path) -> (u64, usize) {
    if !path.exists() {
        return (0, 0);
    }
    if path.is_file() {
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        return (size, 1);
    }
    let root_dev = path.metadata().map(|m| m.dev()).unwrap_or(0);
    let mut total = 0u64;
    let mut count = 0usize;
    let mut seen_inodes = rustc_hash::FxHashSet::default();
    scan_recursive(path, &mut total, Some(&mut count), &mut seen_inodes, root_dev);
    (total, count)
}

#[allow(unsafe_code)]
fn scan_recursive(
    dir: &Path,
    total: &mut u64,
    mut top_level_count: Option<&mut usize>,
    seen_inodes: &mut rustc_hash::FxHashSet<u64>,
    root_dev: u64,
) {
    // Open directory using safe std::fs::File, then extract the raw fd.
    let Ok(dir_file) = std::fs::File::open(dir) else {
        return;
    };
    let fd = dir_file.as_raw_fd();

    // Request DEVID so we can detect volume boundaries from the bulk response
    // without needing a separate stat() per subdirectory.
    let mut al = AttrList {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_NAME | ATTR_CMN_DEVID | ATTR_CMN_ERROR | ATTR_CMN_OBJTYPE | ATTR_CMN_FILEID,
        volattr: 0,
        dirattr: 0,
        fileattr: ATTR_FILE_LINKCOUNT | ATTR_FILE_ALLOCSIZE,
        forkattr: 0,
    };

    let mut buf = vec![0u8; BUF_SIZE];
    let mut subdirs: Vec<std::path::PathBuf> = Vec::new();

    loop {
        let ret = unsafe {
            getattrlistbulk(
                fd,
                &raw mut al,
                buf.as_mut_ptr().cast::<c_void>(),
                buf.len(),
                0,
            )
        };

        if ret == 0 {
            break;
        }
        if ret < 0 {
            // APFS bug: ERANGE at end of directory, retry
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(ERANGE) {
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

            if let Some(ref mut c) = top_level_count.as_deref_mut() {
                **c += 1;
            }

            match parsed {
                ParsedEntry::File { size, inode, nlink } => {
                    if nlink > 1 && !seen_inodes.insert(inode) {
                        // Already counted this hard-linked file
                        offset += entry_len;
                        continue;
                    }
                    *total += size;
                }
                ParsedEntry::Dir { name, dev } => {
                    // Skip directories on a different volume (mount points)
                    if root_dev != 0 && dev != 0 && dev != root_dev {
                        offset += entry_len;
                        continue;
                    }
                    if !name.is_empty() {
                        subdirs.push(dir.join(&name));
                    }
                }
                ParsedEntry::Other | ParsedEntry::Error => {}
            }

            offset += entry_len;
        }
    }

    // dir_file drops here, closing the fd.
    drop(dir_file);

    // Recurse into subdirectories (no top-level counting for children).
    for subdir in &subdirs {
        scan_recursive(subdir, total, None, seen_inodes, root_dev);
    }
}

enum ParsedEntry {
    File { size: u64, inode: u64, nlink: u32 },
    Dir { name: String, dev: u64 },
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
            let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
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
    }

    if obj_type == VDIR {
        ParsedEntry::Dir { name, dev }
    } else if obj_type == VREG {
        ParsedEntry::File {
            size: file_size.max(0) as u64,
            inode,
            nlink,
        }
    } else {
        ParsedEntry::Other
    }
}
