//! Drift between two reports: what grew, what shrank, what appeared or
//! disappeared. Pure arithmetic over two Report values; the same shape is
//! rendered by the CLI and mirrored client-side by the viewer.

use crate::report::Report;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Debug, Clone)]
pub struct SideRef {
    pub name: String,
    pub completed_at_unix: Option<i64>,
    pub root: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct TotalDelta {
    pub before: u64,
    pub after: u64,
    pub delta: i64,
}

fn total(before: u64, after: u64) -> TotalDelta {
    TotalDelta {
        before,
        after,
        delta: after as i64 - before as i64,
    }
}

/// One named quantity on both sides. `None` on a side means the entry does
/// not exist there (added/removed).
#[derive(Serialize, Debug, Clone)]
pub struct NamedDelta {
    pub name: String,
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub delta: i64,
}

#[derive(Serialize, Debug, Clone)]
pub struct PartitionDelta {
    pub name: String,
    pub size_before: Option<u64>,
    pub size_after: Option<u64>,
    pub used_before: Option<u64>,
    pub used_after: Option<u64>,
    pub used_delta: i64,
}

#[derive(Serialize, Debug, Clone)]
pub struct Drift {
    pub schema: u32,
    pub a: SideRef,
    pub b: SideRef,
    pub rootfs_uncompressed: Option<TotalDelta>,
    pub rootfs_compressed: Option<TotalDelta>,
    /// Only partitions whose used bytes changed (or that exist on one side).
    pub partitions: Vec<PartitionDelta>,
    /// Changed/added/removed only, sorted by |delta| descending.
    pub images: Vec<NamedDelta>,
    pub packages: Vec<NamedDelta>,
    pub modules: Vec<NamedDelta>,
}

fn named_deltas(
    a: impl Iterator<Item = (String, u64)>,
    b: impl Iterator<Item = (String, u64)>,
) -> Vec<NamedDelta> {
    let ma: BTreeMap<String, u64> = a.collect();
    let mb: BTreeMap<String, u64> = b.collect();
    let mut names: Vec<&String> = ma.keys().chain(mb.keys()).collect();
    names.sort();
    names.dedup();
    let mut out: Vec<NamedDelta> = names
        .into_iter()
        .filter_map(|n| {
            let before = ma.get(n).copied();
            let after = mb.get(n).copied();
            if before == after {
                return None; // unchanged
            }
            Some(NamedDelta {
                name: n.clone(),
                before,
                after,
                delta: after.unwrap_or(0) as i64 - before.unwrap_or(0) as i64,
            })
        })
        .collect();
    out.sort_by(|x, y| y.delta.abs().cmp(&x.delta.abs()).then(x.name.cmp(&y.name)));
    out
}

fn side(r: &Report) -> SideRef {
    SideRef {
        name: r.build.name.clone(),
        completed_at_unix: r.build.completed_at_unix,
        root: r.scan.root.clone(),
    }
}

pub fn diff(a: &Report, b: &Report) -> Drift {
    let rootfs_uncompressed = match (&a.rootfs, &b.rootfs) {
        (Some(ra), Some(rb)) => Some(total(ra.uncompressed_bytes, rb.uncompressed_bytes)),
        _ => None,
    };
    let rootfs_compressed = match (
        a.rootfs.as_ref().and_then(|r| r.compressed_bytes),
        b.rootfs.as_ref().and_then(|r| r.compressed_bytes),
    ) {
        (Some(ca), Some(cb)) => Some(total(ca, cb)),
        _ => None,
    };

    let mut partitions: Vec<PartitionDelta> = Vec::new();
    {
        let pa: BTreeMap<&str, _> = a
            .flash
            .iter()
            .flat_map(|f| f.partitions.iter())
            .map(|p| (p.name.as_str(), p))
            .collect();
        let pb: BTreeMap<&str, _> = b
            .flash
            .iter()
            .flat_map(|f| f.partitions.iter())
            .map(|p| (p.name.as_str(), p))
            .collect();
        let mut names: Vec<&&str> = pa.keys().chain(pb.keys()).collect();
        names.sort();
        names.dedup();
        for n in names {
            let x = pa.get(*n);
            let y = pb.get(*n);
            let used_b = x.and_then(|p| p.used_bytes.or(p.content_bytes));
            let used_a = y.and_then(|p| p.used_bytes.or(p.content_bytes));
            if x.is_some() && y.is_some() && used_b == used_a {
                continue;
            }
            partitions.push(PartitionDelta {
                name: n.to_string(),
                size_before: x.and_then(|p| p.size),
                size_after: y.and_then(|p| p.size),
                used_before: used_b,
                used_after: used_a,
                used_delta: used_a.unwrap_or(0) as i64 - used_b.unwrap_or(0) as i64,
            });
        }
    }

    Drift {
        schema: 1,
        a: side(a),
        b: side(b),
        rootfs_uncompressed,
        rootfs_compressed,
        partitions,
        images: named_deltas(
            a.images.iter().map(|i| (i.name.clone(), i.bytes)),
            b.images.iter().map(|i| (i.name.clone(), i.bytes)),
        ),
        packages: named_deltas(
            a.packages.iter().map(|p| (p.name.clone(), p.bytes)),
            b.packages.iter().map(|p| (p.name.clone(), p.bytes)),
        ),
        modules: named_deltas(
            a.modules.iter().map(|m| (m.name.clone(), m.bytes)),
            b.modules.iter().map(|m| (m.name.clone(), m.bytes)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::analyze;
    use crate::snapshot::{Snapshot, TargetEntry};

    fn mini_report(pkg_size: u64, extra_pkg: bool) -> Report {
        let mut s = Snapshot::empty("t");
        s.pfl = Some(if extra_pkg {
            "alpha,./bin/alpha\nnewpkg,./bin/new\n".to_string()
        } else {
            "alpha,./bin/alpha\n".to_string()
        });
        s.target = vec![TargetEntry {
            path: "bin/alpha".into(),
            size: pkg_size,
            is_symlink: false,
            charged: true,
        }];
        if extra_pkg {
            s.target.push(TargetEntry {
                path: "bin/new".into(),
                size: 500,
                is_symlink: false,
                charged: true,
            });
        }
        analyze(&s)
    }

    #[test]
    fn packages_delta_and_added() {
        let a = mini_report(1000, false);
        let b = mini_report(1300, true);
        let d = diff(&a, &b);
        let alpha = d.packages.iter().find(|p| p.name == "alpha").unwrap();
        assert_eq!(alpha.delta, 300);
        let newpkg = d.packages.iter().find(|p| p.name == "newpkg").unwrap();
        assert_eq!(newpkg.before, None);
        assert_eq!(newpkg.after, Some(500));
        let unc = d.rootfs_uncompressed.unwrap();
        assert_eq!(unc.delta, 800);
    }

    #[test]
    fn unchanged_entries_omitted() {
        let a = mini_report(1000, false);
        let b = mini_report(1000, false);
        let d = diff(&a, &b);
        assert!(d.packages.is_empty());
        assert!(d.images.is_empty());
    }
}
