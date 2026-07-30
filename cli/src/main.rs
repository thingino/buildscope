mod export;
mod serve;
mod summary;
mod walker;

use buildscope_core::analyze::analyze;
use buildscope_core::carve::carve_flash_image;
use buildscope_core::diff::diff;
use buildscope_core::report::{Report, REPORT_FILENAME};
use buildscope_core::snapshot::ScanMode;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "buildscope",
    version,
    about = "Size and composition analyzer for Buildroot output trees"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan one or more Buildroot output directories and emit report.json
    Scan {
        /// Output dirs (a build dir, or a parent containing several)
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
        /// Run as a Buildroot post-image hook: the single dir is BINARIES_DIR
        /// and TARGET_DIR/BUILD_DIR/BR2_CONFIG come from the environment
        #[arg(long)]
        hook: bool,
        /// Also write the report to this path ("-" for stdout)
        #[arg(long)]
        out: Option<String>,
        /// Do not write buildscope-report.json into images/
        #[arg(long)]
        no_write: bool,
        /// Suppress the terminal summary
        #[arg(long, short)]
        quiet: bool,
        /// Explicit flash layout, e.g. "mtdparts=nor0:256k(boot),-(rootfs)"
        #[arg(long)]
        flash_map: Option<String>,
        /// Path to a genimage config describing the flash/disk layout
        #[arg(long)]
        genimage: Option<PathBuf>,
    },
    /// Scan, then serve reports and the web viewer locally
    Serve {
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
        #[arg(long, default_value_t = 8380)]
        port: u16,
        /// Addresses to listen on, comma separated. Both loopbacks by
        /// default; "all" means every interface on both IP families
        #[arg(long, value_delimiter = ',', default_value = "127.0.0.1,::1")]
        bind: Vec<String>,
        /// Directory with the built viewer (index.html + assets)
        #[arg(long)]
        viewer_dir: Option<PathBuf>,
        #[arg(long)]
        flash_map: Option<String>,
        #[arg(long)]
        genimage: Option<PathBuf>,
    },
    /// Analyze bare firmware artifacts with no build tree (a released .bin,
    /// a flash dump, a lone rootfs image)
    Carve {
        /// Image files, or directories of images
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Also write the report to this path ("-" for stdout)
        #[arg(long)]
        out: Option<String>,
        /// Write a report file next to each analyzed image
        #[arg(long)]
        write: bool,
        /// Suppress the terminal summary
        #[arg(long, short)]
        quiet: bool,
    },
    /// Compare two builds (report.json files or output dirs)
    Diff {
        /// Baseline: report.json or a build dir
        a: PathBuf,
        /// Comparison: report.json or a build dir
        b: PathBuf,
        /// Emit the full drift as JSON instead of a summary
        #[arg(long)]
        json: bool,
    },
    /// Write a self-contained HTML report (viewer + data in one file)
    Export {
        /// report.json or a build dir
        input: PathBuf,
        /// Output file (default: <build-name>.html)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Directory with the built viewer (index.html + assets)
        #[arg(long)]
        viewer_dir: Option<PathBuf>,
    },
}

/// Extensions that are never firmware artifacts, skipped when carving a
/// directory of release assets.
const NON_ARTIFACT_EXT: &[&str] = &[
    "json", "md", "txt", "sha256sum", "sha256", "sha1", "md5", "asc", "sig", "log", "cfg", "html",
];

fn is_artifact_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    !NON_ARTIFACT_EXT.contains(&ext.as_str())
}

/// Analyze one bare artifact file with no build tree.
fn carve_file(path: &Path) -> Result<Report, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if data.is_empty() {
        return Err(format!("{}: empty file", path.display()));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let root = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(carve_flash_image(&name, &data, &root, ScanMode::Native))
}

/// Load a report from a JSON file, carve a bare artifact file, or scan a
/// directory that resolves to exactly one build.
fn load_report(path: &Path) -> Result<Report, String> {
    if path.is_file() {
        let is_json = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if !is_json {
            return carve_file(path);
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let r: Report = serde_json::from_str(&text)
            .map_err(|e| format!("{}: not a buildscope report: {e}", path.display()))?;
        if r.schema != buildscope_core::report::SCHEMA {
            return Err(format!(
                "{}: unsupported schema {} (expected {})",
                path.display(),
                r.schema,
                buildscope_core::report::SCHEMA
            ));
        }
        return Ok(r);
    }
    let builds = walker::find_builds(path);
    match builds.len() {
        0 => Err(format!("{}: no Buildroot output tree found", path.display())),
        1 => {
            let snap = walker::build_snapshot(&builds[0], None, None)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(analyze(&snap))
        }
        n => Err(format!(
            "{}: {n} builds found; point at one build dir or a report.json",
            path.display()
        )),
    }
}

/// Scan build trees, falling back to carving bare artifacts for directories
/// that hold released images rather than a Buildroot output tree. `None`
/// paths mark carved artifacts (no build tree to write a report into).
fn scan_dirs(
    dirs: &[PathBuf],
    hook: bool,
    flash_map: Option<&str>,
    genimage: Option<&std::path::Path>,
) -> Vec<(Option<walker::BuildPaths>, Report)> {
    let mut out = Vec::new();
    if hook {
        if dirs.len() != 1 {
            eprintln!("buildscope: --hook takes exactly one directory (BINARIES_DIR)");
            std::process::exit(2);
        }
        let paths = walker::from_hook(&dirs[0]);
        match walker::build_snapshot(&paths, flash_map, genimage) {
            Ok(snap) => out.push((Some(paths), analyze(&snap))),
            Err(e) => eprintln!("buildscope: {}: {e}", dirs[0].display()),
        }
        return out;
    }
    for dir in dirs {
        // A single artifact file: carve it.
        if dir.is_file() {
            match carve_file(dir) {
                Ok(r) => out.push((None, r)),
                Err(e) => eprintln!("buildscope: {e}"),
            }
            continue;
        }
        let builds = walker::find_builds(dir);
        if builds.is_empty() {
            // Not a build tree: treat it as a directory of firmware
            // artifacts (a downloaded release, a dump collection).
            let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|c| c.is_file() && is_artifact_candidate(c))
                .collect();
            candidates.sort();
            if candidates.is_empty() {
                eprintln!(
                    "buildscope: {}: no Buildroot output tree and no firmware artifacts found",
                    dir.display()
                );
                continue;
            }
            for c in candidates {
                match carve_file(&c) {
                    Ok(r) => out.push((None, r)),
                    Err(e) => eprintln!("buildscope: {e}"),
                }
            }
            continue;
        }
        for paths in builds {
            match walker::build_snapshot(&paths, flash_map, genimage) {
                Ok(snap) => out.push((Some(paths.clone()), analyze(&snap))),
                Err(e) => eprintln!("buildscope: {}: {e}", paths.root.display()),
            }
        }
    }
    out
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Scan {
            dirs,
            hook,
            out,
            no_write,
            quiet,
            flash_map,
            genimage,
        } => {
            let results = scan_dirs(&dirs, hook, flash_map.as_deref(), genimage.as_deref());
            if results.is_empty() {
                std::process::exit(1);
            }
            for (paths, report) in &results {
                let json = serde_json::to_string_pretty(report).expect("serialize report");
                if !no_write {
                    if let Some(img_dir) = paths.as_ref().and_then(|p| p.images_dir.as_ref()) {
                        let dest = img_dir.join(REPORT_FILENAME);
                        match std::fs::write(&dest, format!("{json}\n")) {
                            Ok(()) => {
                                if !quiet {
                                    println!("wrote {}", dest.display());
                                }
                            }
                            Err(e) => eprintln!("buildscope: write {}: {e}", dest.display()),
                        }
                    }
                }
                if !quiet {
                    summary::print_report(report);
                }
            }
            if let Some(out) = out {
                if results.len() != 1 {
                    eprintln!("buildscope: --out requires exactly one scanned build");
                    std::process::exit(2);
                }
                let json = serde_json::to_string_pretty(&results[0].1).expect("serialize report");
                if out == "-" {
                    println!("{json}");
                } else if let Err(e) = std::fs::write(&out, format!("{json}\n")) {
                    eprintln!("buildscope: write {out}: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Serve {
            dirs,
            port,
            bind,
            viewer_dir,
            flash_map,
            genimage,
        } => {
            let results = scan_dirs(&dirs, false, flash_map.as_deref(), genimage.as_deref());
            if results.is_empty() {
                std::process::exit(1);
            }
            let reports: Vec<serve::ReportEntry> = results
                .iter()
                .map(|(_, r)| serve::ReportEntry {
                    name: r.build.name.clone(),
                    json: serde_json::to_string(r).expect("serialize report"),
                })
                .collect();
            let viewer = viewer_dir.or_else(default_viewer_dir);
            serve::serve(&bind, port, reports, viewer);
        }
        Cmd::Carve {
            files,
            out,
            write,
            quiet,
        } => {
            // Expand directories into their artifact candidates.
            let mut targets: Vec<PathBuf> = Vec::new();
            for p in &files {
                if p.is_dir() {
                    let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path())
                        .filter(|c| c.is_file() && is_artifact_candidate(c))
                        .collect();
                    entries.sort();
                    if entries.is_empty() {
                        eprintln!("buildscope: {}: no firmware artifacts found", p.display());
                    }
                    targets.extend(entries);
                } else {
                    targets.push(p.clone());
                }
            }

            let mut reports: Vec<(PathBuf, Report)> = Vec::new();
            for t in &targets {
                match carve_file(t) {
                    Ok(r) => reports.push((t.clone(), r)),
                    Err(e) => eprintln!("buildscope: {e}"),
                }
            }
            if reports.is_empty() {
                std::process::exit(1);
            }
            for (path, report) in &reports {
                if write {
                    let dest = path.with_extension("buildscope.json");
                    let json = serde_json::to_string_pretty(report).expect("serialize report");
                    match std::fs::write(&dest, format!("{json}\n")) {
                        Ok(()) => {
                            if !quiet {
                                println!("wrote {}", dest.display());
                            }
                        }
                        Err(e) => eprintln!("buildscope: write {}: {e}", dest.display()),
                    }
                }
                if !quiet {
                    summary::print_report(report);
                }
            }
            if let Some(out) = out {
                if reports.len() == 1 {
                    let json =
                        serde_json::to_string_pretty(&reports[0].1).expect("serialize report");
                    if out == "-" {
                        println!("{json}");
                    } else if let Err(e) = std::fs::write(&out, format!("{json}\n")) {
                        eprintln!("buildscope: write {out}: {e}");
                        std::process::exit(1);
                    }
                } else {
                    // Many artifacts: emit an array so one file holds the set.
                    let all: Vec<&Report> = reports.iter().map(|(_, r)| r).collect();
                    let json = serde_json::to_string_pretty(&all).expect("serialize reports");
                    if out == "-" {
                        println!("{json}");
                    } else if let Err(e) = std::fs::write(&out, format!("{json}\n")) {
                        eprintln!("buildscope: write {out}: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        Cmd::Diff { a, b, json } => {
            let (ra, rb) = match (load_report(&a), load_report(&b)) {
                (Ok(ra), Ok(rb)) => (ra, rb),
                (Err(e), _) | (_, Err(e)) => {
                    eprintln!("buildscope: {e}");
                    std::process::exit(1);
                }
            };
            let d = diff(&ra, &rb);
            if json {
                println!("{}", serde_json::to_string_pretty(&d).expect("serialize drift"));
            } else {
                summary::print_drift(&d);
            }
        }
        Cmd::Export {
            input,
            out,
            viewer_dir,
        } => {
            let report = match load_report(&input) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("buildscope: {e}");
                    std::process::exit(1);
                }
            };
            let Some(dist) = viewer_dir.or_else(default_viewer_dir) else {
                eprintln!("buildscope: no built viewer found; build viewer/ or pass --viewer-dir");
                std::process::exit(1);
            };
            let json = serde_json::to_string(&report).expect("serialize report");
            let html = match export::build_single_file(&dist, &json) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("buildscope: export: {e}");
                    std::process::exit(1);
                }
            };
            let dest = out.unwrap_or_else(|| PathBuf::from(format!("{}.html", report.build.name)));
            match std::fs::write(&dest, html) {
                Ok(()) => println!("wrote {}", dest.display()),
                Err(e) => {
                    eprintln!("buildscope: write {}: {e}", dest.display());
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Look for a built viewer next to the binary or in the source tree.
fn default_viewer_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("viewer"));
            // target/release/../../viewer/dist during development
            candidates.push(dir.join("../../viewer/dist"));
        }
    }
    candidates.push(PathBuf::from("viewer/dist"));
    candidates.into_iter().find(|c| c.join("index.html").is_file())
}
