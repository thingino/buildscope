//! Native filesystem snapshot builder: discovers Buildroot output trees and
//! materializes them into the core's IO-free Snapshot.

use buildscope_core::inputs::pfl;
use buildscope_core::report::REPORT_FILENAME;
use buildscope_core::snapshot::{
    ContextSource, ImageInput, NamedText, RemovedCandidate, ScanMode, Snapshot, TargetEntry,
};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Refuse to slurp anything bigger than this into memory (per file).
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone)]
pub struct BuildPaths {
    pub root: PathBuf,
    pub config: Option<PathBuf>,
    pub build_dir: Option<PathBuf>,
    pub target_dir: Option<PathBuf>,
    pub images_dir: Option<PathBuf>,
    pub context: ContextSource,
}

fn looks_like_build_dir(dir: &Path) -> bool {
    let config = dir.join(".config").is_file();
    let images = dir.join("images").is_dir();
    let target = dir.join("target").is_dir();
    let build = dir.join("build").is_dir();
    (config && (images || target || build)) || images
}

fn looks_like_bare_images_dir(dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    let names: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.iter().any(|n| {
        n.starts_with("rootfs.")
            || n == "uImage"
            || n == "zImage"
            || n == "Image"
            || n.ends_with(".itb")
            || n == "sdcard.img"
    })
}

pub fn discover(dir: &Path) -> Option<BuildPaths> {
    if looks_like_build_dir(dir) {
        let sub = |name: &str| {
            let p = dir.join(name);
            p.exists().then_some(p)
        };
        return Some(BuildPaths {
            root: dir.to_path_buf(),
            config: sub(".config"),
            build_dir: sub("build"),
            target_dir: sub("target"),
            images_dir: sub("images"),
            context: ContextSource::Inferred,
        });
    }
    if looks_like_bare_images_dir(dir) {
        return Some(BuildPaths {
            root: dir.to_path_buf(),
            config: None,
            build_dir: None,
            target_dir: None,
            images_dir: Some(dir.to_path_buf()),
            context: ContextSource::Inferred,
        });
    }
    None
}

/// A path may be one build, or a directory of builds (one or two levels).
pub fn find_builds(dir: &Path) -> Vec<BuildPaths> {
    if let Some(b) = discover(dir) {
        return vec![b];
    }
    let mut out = Vec::new();
    let mut subdirs: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    for sub in &subdirs {
        if let Some(b) = discover(sub) {
            out.push(b);
        }
    }
    if out.is_empty() {
        for sub in &subdirs {
            let mut inner: Vec<PathBuf> = fs::read_dir(sub)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            inner.sort();
            for d in inner {
                if let Some(b) = discover(&d) {
                    out.push(b);
                }
            }
        }
    }
    out
}

/// Build paths from Buildroot's post-image hook contract: images dir as the
/// argument, the rest in the environment.
pub fn from_hook(images_dir: &Path) -> BuildPaths {
    let env_path = |k: &str| {
        std::env::var_os(k)
            .map(PathBuf::from)
            .filter(|p| p.exists())
    };
    let root = env_path("BASE_DIR")
        .or_else(|| images_dir.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| images_dir.to_path_buf());
    BuildPaths {
        config: env_path("BR2_CONFIG").or_else(|| {
            let c = root.join(".config");
            c.is_file().then_some(c)
        }),
        build_dir: env_path("BUILD_DIR").or_else(|| {
            let b = root.join("build");
            b.is_dir().then_some(b)
        }),
        target_dir: env_path("TARGET_DIR").or_else(|| {
            let t = root.join("target");
            t.is_dir().then_some(t)
        }),
        images_dir: Some(images_dir.to_path_buf()),
        root,
        context: ContextSource::Hook,
    }
}

fn walk_target(target_dir: &Path) -> io::Result<Vec<TargetEntry>> {
    let mut entries = Vec::new();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut stack = vec![target_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let Ok(md) = fs::symlink_metadata(&path) else {
                continue;
            };
            let ft = md.file_type();
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            if !ft.is_file() && !ft.is_symlink() {
                continue; // devices, fifos, sockets
            }
            let rel = match path.strip_prefix(target_dir) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => continue,
            };
            let mut charged = true;
            if ft.is_file() && md.nlink() > 1 {
                charged = seen.insert((md.dev(), md.ino()));
            }
            entries.push(TargetEntry {
                path: rel,
                size: md.len(),
                is_symlink: ft.is_symlink(),
                charged,
            });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn read_text_capped(path: &Path) -> Option<String> {
    let md = fs::metadata(path).ok()?;
    if !md.is_file() || md.len() > MAX_TEXT_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn push_env_text(out: &mut Vec<NamedText>, p: &Path, require_mtdparts: bool) {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if out.iter().any(|t| t.name == name) {
        return;
    }
    let Some(text) = read_text_capped(p) else {
        return;
    };
    if require_mtdparts && !text.contains("mtdparts=") {
        return;
    }
    out.push(NamedText { name, text });
}

fn collect_env_texts(paths: &BuildPaths) -> Vec<NamedText> {
    let mut out = Vec::new();

    // Conventional environment source names in the output root.
    for name in ["uenv.txt", "uEnv.txt", "u-boot-env.txt", "boot.env"] {
        let p = paths.root.join(name);
        if p.is_file() {
            push_env_text(&mut out, &p, false);
        }
    }
    // Any small text file in the output root or images/ that mentions mtdparts.
    let mut dirs = vec![paths.root.clone()];
    if let Some(img) = &paths.images_dir {
        dirs.push(img.clone());
    }
    for dir in dirs {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        names.sort();
        for p in names {
            let is_texty = p
                .extension()
                .map(|e| {
                    let e = e.to_string_lossy().to_ascii_lowercase();
                    e == "txt" || e == "md" || e == "env" || e == "cfg"
                })
                .unwrap_or(false);
            if is_texty {
                push_env_text(&mut out, &p, true);
            }
        }
    }
    out
}

pub fn build_snapshot(
    paths: &BuildPaths,
    extra_env_text: Option<&str>,
    genimage_path: Option<&Path>,
) -> io::Result<Snapshot> {
    let root_name = paths
        .root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| paths.root.to_string_lossy().into_owned());
    let mut snap = Snapshot::empty(&root_name);
    snap.root_path = crate::provenance(&paths.root);
    snap.context_source = paths.context;
    snap.scan_mode = ScanMode::Native;

    if let Some(c) = &paths.config {
        snap.config = fs::read_to_string(c).ok();
    }
    if let Some(b) = &paths.build_dir {
        snap.pfl = fs::read_to_string(b.join("packages-file-list.txt")).ok();
        snap.build_time_log = fs::read_to_string(b.join("build-time.log")).ok();
    }
    if let Some(t) = &paths.target_dir {
        snap.target = walk_target(t)?;
        snap.etc_modules = fs::read_to_string(t.join("etc/modules")).ok();
        // modules.builtin from the first kernel version dir found; check the
        // merged-usr location too (read_dir follows the /lib symlink anyway,
        // but a tree may lack the symlink entirely).
        for mods in [t.join("lib/modules"), t.join("usr/lib/modules")] {
            let Ok(rd) = fs::read_dir(&mods) else {
                continue;
            };
            let mut vers: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            vers.sort();
            if let Some(v) = vers.first() {
                snap.modules_builtin = fs::read_to_string(v.join("modules.builtin")).ok();
                break;
            }
        }
    }
    if let Some(img_dir) = &paths.images_dir {
        let mut newest: Option<i64> = None;
        let mut files: Vec<PathBuf> = fs::read_dir(img_dir)?.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let Ok(md) = fs::metadata(&f) else {
                continue;
            };
            if !md.is_file() {
                continue;
            }
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name == REPORT_FILENAME {
                continue;
            }
            if let Ok(modt) = md.modified() {
                if let Ok(secs) = modt.duration_since(std::time::UNIX_EPOCH) {
                    let s = secs.as_secs() as i64;
                    newest = Some(newest.map_or(s, |n| n.max(s)));
                }
            }
            let bytes = if md.len() <= MAX_IMAGE_BYTES {
                fs::read(&f).ok()
            } else {
                eprintln!(
                    "buildscope: {} is {} bytes, larger than the in-memory cap; introspection skipped",
                    f.display(),
                    md.len()
                );
                None
            };
            snap.images.push(ImageInput {
                name,
                size: md.len(),
                bytes,
            });
        }
        snap.images_mtime = newest;
    }

    snap.env_texts = collect_env_texts(paths);
    if let Some(extra) = extra_env_text {
        snap.env_texts.insert(
            0,
            NamedText {
                name: "command line".to_string(),
                text: extra.to_string(),
            },
        );
    }

    // genimage configs: explicit path first, then conventional locations.
    if let Some(p) = genimage_path {
        if let Ok(text) = fs::read_to_string(p) {
            snap.genimage_texts.push(NamedText {
                name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "genimage.cfg".into()),
                text,
            });
        } else {
            eprintln!("buildscope: cannot read genimage config {}", p.display());
        }
    }
    let mut gi_dirs = vec![paths.root.clone()];
    if let Some(img) = &paths.images_dir {
        gi_dirs.push(img.clone());
    }
    if let Some(b) = &paths.build_dir {
        gi_dirs.push(b.clone());
    }
    for dir in gi_dirs {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("genimage") && n.ends_with(".cfg"))
                    .unwrap_or(false)
            })
            .collect();
        names.sort();
        for p in names {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if snap.genimage_texts.iter().any(|t| t.name == name) {
                continue;
            }
            if let Some(text) = read_text_capped(&p) {
                snap.genimage_texts.push(NamedText { name, text });
            }
        }
    }

    // Installed-but-not-shipped: paths in packages-file-list.txt missing
    // from target/, with source sizes recovered from per-package/.
    if let (Some(pfl_text), false) = (&snap.pfl, snap.target.is_empty()) {
        let map = pfl::parse(pfl_text);
        let on_disk: HashSet<&str> = snap.target.iter().map(|e| e.path.as_str()).collect();
        let per_pkg_root = paths.root.join("per-package");
        for (rel, pkg) in &map {
            if on_disk.contains(rel.as_str()) {
                continue;
            }
            let mut source_bytes = 0u64;
            let candidate = per_pkg_root.join(pkg).join("target").join(rel);
            if let Ok(md) = fs::symlink_metadata(&candidate) {
                source_bytes = md.len();
            }
            snap.removed_candidates.push(RemovedCandidate {
                path: rel.clone(),
                package: pkg.clone(),
                source_bytes,
            });
        }
        snap.removed_candidates.sort_by(|a, b| a.path.cmp(&b.path));
    }

    Ok(snap)
}
