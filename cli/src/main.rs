mod serve;
mod summary;
mod walker;

use buildscope_core::analyze::analyze;
use buildscope_core::report::{Report, REPORT_FILENAME};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// Do not write the report file into images/
        #[arg(long)]
        no_write: bool,
        /// Suppress the terminal summary
        #[arg(long, short)]
        quiet: bool,
        /// Explicit flash layout, e.g. "mtdparts=nor0:256k(boot),-(rootfs)"
        #[arg(long)]
        flash_map: Option<String>,
    },
    /// Scan, then serve reports and the web viewer locally
    Serve {
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
        #[arg(long, default_value_t = 8380)]
        port: u16,
        /// Address to bind (use 0.0.0.0 to expose on the network)
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
        /// Directory with the built viewer (index.html + assets)
        #[arg(long)]
        viewer_dir: Option<PathBuf>,
        #[arg(long)]
        flash_map: Option<String>,
    },
}

fn scan_dirs(dirs: &[PathBuf], hook: bool, flash_map: Option<&str>) -> Vec<(walker::BuildPaths, Report)> {
    let mut out = Vec::new();
    if hook {
        if dirs.len() != 1 {
            eprintln!("buildscope: --hook takes exactly one directory (BINARIES_DIR)");
            std::process::exit(2);
        }
        let paths = walker::from_hook(&dirs[0]);
        match walker::build_snapshot(&paths, flash_map) {
            Ok(snap) => out.push((paths, analyze(&snap))),
            Err(e) => eprintln!("buildscope: {}: {e}", dirs[0].display()),
        }
        return out;
    }
    for dir in dirs {
        let builds = walker::find_builds(dir);
        if builds.is_empty() {
            eprintln!(
                "buildscope: {}: no Buildroot output tree found (need .config plus images/, target/ or build/)",
                dir.display()
            );
            continue;
        }
        for paths in builds {
            match walker::build_snapshot(&paths, flash_map) {
                Ok(snap) => out.push((paths.clone(), analyze(&snap))),
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
        } => {
            let results = scan_dirs(&dirs, hook, flash_map.as_deref());
            if results.is_empty() {
                std::process::exit(1);
            }
            for (paths, report) in &results {
                let json = serde_json::to_string_pretty(report).expect("serialize report");
                if !no_write {
                    if let Some(img_dir) = &paths.images_dir {
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
        } => {
            let results = scan_dirs(&dirs, false, flash_map.as_deref());
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
