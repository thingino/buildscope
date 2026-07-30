//! Terminal summary of a report: partition bars, image table, top packages.

use buildscope_core::report::{Report, UNATTRIBUTED};

pub fn human(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b >= K * K * K {
        format!("{:.2} GiB", b / K / K / K)
    } else if b >= K * K {
        format!("{:.2} MiB", b / K / K)
    } else if b >= K {
        format!("{:.1} KiB", b / K)
    } else {
        format!("{bytes} B")
    }
}

fn bar(frac: f64, width: usize) -> String {
    let frac = frac.clamp(0.0, 1.0);
    let filled = (frac * width as f64).round() as usize;
    format!(
        "[{}{}]",
        "#".repeat(filled.min(width)),
        ".".repeat(width - filled.min(width))
    )
}

pub fn print_report(r: &Report) {
    println!("== {} ==", r.build.name);
    let mut facts: Vec<String> = Vec::new();
    for v in [
        r.build.arch.as_deref(),
        r.build.libc.as_deref(),
        r.build.kernel_version.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        facts.push(v.to_string());
    }
    if let Some(w) = r.build.build_active_seconds {
        facts.push(format!(
            "build time {}m{:02}s",
            (w as u64) / 60,
            (w as u64) % 60
        ));
    }
    if !facts.is_empty() {
        println!("   {}", facts.join(" | "));
    }

    if let Some(flash) = &r.flash {
        let total = flash
            .total_bytes
            .map(human)
            .unwrap_or_else(|| "?".to_string());
        println!("   flash {} {} via {}", flash.mtd_id.as_deref().unwrap_or("-"), total, flash.source);
        for p in &flash.partitions {
            if p.overlaps {
                continue;
            }
            let size = p.size.unwrap_or(0);
            let used = p.used_bytes.or(p.content_bytes).unwrap_or(0);
            let frac = if size > 0 { used as f64 / size as f64 } else { 0.0 };
            let verified = match p.verified {
                Some(true) => " ok",
                Some(false) => " MISMATCH",
                None => "",
            };
            println!(
                "   {:<10} {:>9} {} {:>9} used ({:>5.1}%){}{}",
                p.name,
                human(size),
                bar(frac, 22),
                human(used),
                frac * 100.0,
                p.image
                    .as_deref()
                    .map(|i| format!("  <- {i}"))
                    .unwrap_or_default(),
                verified,
            );
        }
    }

    if !r.images.is_empty() {
        println!("   images:");
        for i in &r.images {
            let extra = match i.format.as_str() {
                "squashfs" => i
                    .detail
                    .get("bytes_used")
                    .and_then(|v| v.as_u64())
                    .map(|u| {
                        format!(
                            "{} used, {}",
                            human(u),
                            i.detail
                                .get("compression")
                                .and_then(|c| c.as_str())
                                .unwrap_or("?")
                        )
                    }),
                "jffs2" => i.detail.get("used_bytes").and_then(|v| v.as_u64()).map(|u| {
                    format!(
                        "{} used, {} free",
                        human(u),
                        human(
                            i.detail
                                .get("free_bytes")
                                .and_then(|f| f.as_u64())
                                .unwrap_or(0)
                        )
                    )
                }),
                "uimage" => i.detail.get("compression").and_then(|v| v.as_str()).map(|c| {
                    format!(
                        "{} payload, {}",
                        human(
                            i.detail
                                .get("declared_size")
                                .and_then(|d| d.as_u64())
                                .unwrap_or(0)
                        ),
                        c
                    )
                }),
                "uboot-env" => i.detail.get("used_bytes").and_then(|v| v.as_u64()).map(|u| {
                    format!(
                        "{} used of {}, crc {}",
                        human(u),
                        human(i.bytes),
                        if i.detail.get("crc_ok").and_then(|c| c.as_bool()).unwrap_or(false) {
                            "ok"
                        } else {
                            "BAD"
                        }
                    )
                }),
                "flash-image" => i
                    .detail
                    .get("content_end")
                    .and_then(|v| v.as_u64())
                    .map(|c| format!("content to {}", human(c))),
                _ => None,
            };
            println!(
                "     {:<44} {:>10}  {:<12} {}",
                i.name,
                human(i.bytes),
                i.format,
                extra.unwrap_or_default()
            );
        }
    }

    if !r.packages.is_empty() {
        let total: u64 = r.packages.iter().map(|p| p.bytes).sum();
        println!("   top packages (of {} in {} packages):", human(total), r.packages.len());
        for p in r.packages.iter().take(10) {
            let label = if p.name == UNATTRIBUTED {
                "(overlay/post-build)"
            } else {
                &p.name
            };
            println!(
                "     {:<32} {:>10}  {:>4} files",
                label,
                human(p.bytes),
                p.file_count
            );
        }
    }

    for w in &r.scan.warnings {
        println!("   warning: {w}");
    }
    println!();
}
