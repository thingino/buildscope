//! WebAssembly boundary for buildscope-core.
//!
//! The interface is deliberately a plain C ABI rather than a binding
//! generator: everything crossing it is either a byte buffer or an integer,
//! so there is nothing to generate and no toolchain version to match.
//!
//! Protocol
//! --------
//! * `bs_alloc(len)` / `bs_free(ptr, len)` manage buffers in wasm memory.
//! * Strings are UTF-8 byte buffers passed as (ptr, len).
//! * Functions returning data return a pointer to a buffer whose first four
//!   bytes are the payload length (little endian), followed by the payload.
//!   The caller must release it with `bs_free(ptr, 4 + len)`.
//!
//! Tree scan
//! ---------
//! ```text
//! h = bs_new()
//! bs_set_text(h, KIND, name…, text…)      // config, pfl, env sources, …
//! bs_add_targets(h, blob…)                // "size\tflags\tpath\n" records
//! bs_add_image(h, name…, bytes…)
//! json = bs_analyze(h)                    // length-prefixed report JSON
//! bs_drop(h)
//! ```
//!
//! Artifact scan
//! -------------
//! ```text
//! json = bs_carve(name…, bytes…)
//! ```

use buildscope_core::analyze::analyze;
use buildscope_core::carve::carve_flash_image;
use buildscope_core::snapshot::{
    ContextSource, ImageInput, NamedText, RemovedCandidate, ScanMode, Snapshot, TargetEntry,
};
use std::cell::RefCell;
use std::collections::HashMap;

// Text section kinds, mirrored by the JS caller.
pub const KIND_ROOT: u32 = 1;
pub const KIND_CONFIG: u32 = 2;
pub const KIND_PFL: u32 = 3;
pub const KIND_BUILD_TIME_LOG: u32 = 4;
pub const KIND_ETC_MODULES: u32 = 5;
pub const KIND_MODULES_BUILTIN: u32 = 6;
pub const KIND_ENV_TEXT: u32 = 7;
pub const KIND_GENIMAGE: u32 = 8;

thread_local! {
    static SNAPSHOTS: RefCell<HashMap<u32, Snapshot>> = RefCell::new(HashMap::new());
    static NEXT_HANDLE: RefCell<u32> = const { RefCell::new(1) };
}

/// Allocate `len` bytes for the caller to write into.
#[no_mangle]
pub extern "C" fn bs_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Release a buffer previously handed out by this module.
#[no_mangle]
pub unsafe extern "C" fn bs_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    drop(Vec::from_raw_parts(ptr, len, len));
}

unsafe fn slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}

unsafe fn text(ptr: *const u8, len: usize) -> String {
    String::from_utf8_lossy(slice(ptr, len)).into_owned()
}

/// Wrap a payload as a length-prefixed buffer and hand ownership to JS.
fn emit(payload: String) -> *mut u8 {
    let bytes = payload.into_bytes();
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&bytes);
    let ptr = out.as_mut_ptr();
    std::mem::forget(out);
    ptr
}

/// Start a new tree snapshot. Returns a handle, or 0 on exhaustion.
#[no_mangle]
pub extern "C" fn bs_new() -> u32 {
    let handle = NEXT_HANDLE.with(|n| {
        let mut n = n.borrow_mut();
        let h = *n;
        *n = n.wrapping_add(1).max(1);
        h
    });
    let mut snap = Snapshot::empty("build");
    snap.scan_mode = ScanMode::Browser;
    snap.context_source = ContextSource::Inferred;
    SNAPSHOTS.with(|s| s.borrow_mut().insert(handle, snap));
    handle
}

#[no_mangle]
pub extern "C" fn bs_drop(handle: u32) {
    SNAPSHOTS.with(|s| s.borrow_mut().remove(&handle));
}

/// Attach a text section. `name` is used by the kinds that carry a filename.
#[no_mangle]
pub unsafe extern "C" fn bs_set_text(
    handle: u32,
    kind: u32,
    name_ptr: *const u8,
    name_len: usize,
    text_ptr: *const u8,
    text_len: usize,
) -> u32 {
    let name = text(name_ptr, name_len);
    let body = text(text_ptr, text_len);
    SNAPSHOTS.with(|s| {
        let mut map = s.borrow_mut();
        let Some(snap) = map.get_mut(&handle) else {
            return 0;
        };
        match kind {
            KIND_ROOT => {
                snap.root_name = name;
                snap.root_path = body;
            }
            KIND_CONFIG => snap.config = Some(body),
            KIND_PFL => snap.pfl = Some(body),
            KIND_BUILD_TIME_LOG => snap.build_time_log = Some(body),
            KIND_ETC_MODULES => snap.etc_modules = Some(body),
            KIND_MODULES_BUILTIN => snap.modules_builtin = Some(body),
            KIND_ENV_TEXT => snap.env_texts.push(NamedText { name, text: body }),
            KIND_GENIMAGE => snap.genimage_texts.push(NamedText { name, text: body }),
            _ => return 0,
        }
        1
    })
}

/// Add target-tree entries as newline-separated `size\tflags\tpath` records.
/// `flags` is a bitmask: 1 = symlink, 2 = not charged (hardlink duplicate).
/// The browser cannot see inode links, so it never sets bit 2; the report's
/// `scan_mode` records which kind of walk produced the numbers.
#[no_mangle]
pub unsafe extern "C" fn bs_add_targets(handle: u32, ptr: *const u8, len: usize) -> u32 {
    let blob = text(ptr, len);
    SNAPSHOTS.with(|s| {
        let mut map = s.borrow_mut();
        let Some(snap) = map.get_mut(&handle) else {
            return 0;
        };
        let mut added = 0u32;
        for line in blob.lines() {
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let (Some(size), Some(flags), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Ok(size) = size.parse::<u64>() else {
                continue;
            };
            let flags: u32 = flags.parse().unwrap_or(0);
            snap.target.push(TargetEntry {
                path: path.to_string(),
                size,
                is_symlink: flags & 1 != 0,
                charged: flags & 2 == 0,
            });
            added += 1;
        }
        added
    })
}

/// Add one file from images/. Pass `bytes_len` 0 to record size only.
#[no_mangle]
pub unsafe extern "C" fn bs_add_image(
    handle: u32,
    name_ptr: *const u8,
    name_len: usize,
    size: u64,
    bytes_ptr: *const u8,
    bytes_len: usize,
) -> u32 {
    let name = text(name_ptr, name_len);
    let bytes = if bytes_len == 0 {
        None
    } else {
        Some(slice(bytes_ptr, bytes_len).to_vec())
    };
    SNAPSHOTS.with(|s| {
        let mut map = s.borrow_mut();
        let Some(snap) = map.get_mut(&handle) else {
            return 0;
        };
        snap.images.push(ImageInput { name, size, bytes });
        1
    })
}

/// Add installed-but-not-shipped candidates as newline-separated
/// `source_bytes\tpackage\tpath` records. The caller derives these by
/// diffing packages-file-list.txt against the target tree it enumerated,
/// recovering sizes from per-package/ where those files are available.
#[no_mangle]
pub unsafe extern "C" fn bs_add_removed(handle: u32, ptr: *const u8, len: usize) -> u32 {
    let blob = text(ptr, len);
    SNAPSHOTS.with(|s| {
        let mut map = s.borrow_mut();
        let Some(snap) = map.get_mut(&handle) else {
            return 0;
        };
        let mut added = 0u32;
        for line in blob.lines() {
            let mut parts = line.splitn(3, '\t');
            let (Some(size), Some(pkg), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            snap.removed_candidates.push(RemovedCandidate {
                path: path.to_string(),
                package: pkg.to_string(),
                source_bytes: size.parse().unwrap_or(0),
            });
            added += 1;
        }
        added
    })
}

/// Record the newest images/ modification time (unix seconds).
#[no_mangle]
pub extern "C" fn bs_set_images_mtime(handle: u32, unix_seconds: i64) -> u32 {
    SNAPSHOTS.with(|s| {
        let mut map = s.borrow_mut();
        let Some(snap) = map.get_mut(&handle) else {
            return 0;
        };
        snap.images_mtime = Some(unix_seconds);
        1
    })
}

/// Analyze the snapshot; returns length-prefixed report JSON.
#[no_mangle]
pub extern "C" fn bs_analyze(handle: u32) -> *mut u8 {
    let report = SNAPSHOTS.with(|s| {
        let map = s.borrow();
        map.get(&handle).map(analyze)
    });
    match report {
        Some(r) => emit(serde_json::to_string(&r).unwrap_or_else(|e| error_json(&e.to_string()))),
        None => emit(error_json("unknown snapshot handle")),
    }
}

/// Analyze a bare firmware artifact; returns length-prefixed report JSON.
#[no_mangle]
pub unsafe extern "C" fn bs_carve(
    name_ptr: *const u8,
    name_len: usize,
    bytes_ptr: *const u8,
    bytes_len: usize,
) -> *mut u8 {
    let name = text(name_ptr, name_len);
    let data = slice(bytes_ptr, bytes_len);
    let report = carve_flash_image(&name, data, "(browser)", ScanMode::Browser);
    emit(serde_json::to_string(&report).unwrap_or_else(|e| error_json(&e.to_string())))
}

fn error_json(message: &str) -> String {
    format!(
        "{{\"error\":{}}}",
        serde_json::to_string(message).unwrap_or_else(|_| "\"error\"".into())
    )
}

/// Schema version of the reports this module emits.
#[no_mangle]
pub extern "C" fn bs_schema() -> u32 {
    buildscope_core::report::SCHEMA
}
