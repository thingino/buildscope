//! build/build-time.log: Buildroot step instrumentation. Lines look like
//!
//!   1785362504.748140168:start:download            : host-skeleton
//!   1785362504.878965580:end  :download            : host-skeleton
//!
//! We pair start/end per (package, step) and aggregate per package.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct StepTime {
    pub step: String,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PkgTiming {
    pub package: String,
    pub seconds: f64,
    pub steps: Vec<StepTime>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BuildTimes {
    pub packages: Vec<PkgTiming>,
    /// Last end minus first start: wall time of the instrumented build.
    pub wall_seconds: Option<f64>,
    /// Unix timestamp of the last recorded step end.
    pub finished_at: Option<f64>,
}

pub fn parse(text: &str) -> BuildTimes {
    let mut open: HashMap<(String, String), f64> = HashMap::new();
    let mut acc: HashMap<String, HashMap<String, f64>> = HashMap::new();
    let mut first_start: Option<f64> = None;
    let mut last_end: Option<f64> = None;

    for line in text.lines() {
        let mut it = line.splitn(4, ':');
        let (Some(ts), Some(phase), Some(step), Some(pkg)) =
            (it.next(), it.next(), it.next(), it.next())
        else {
            continue;
        };
        let Ok(ts) = ts.trim().parse::<f64>() else {
            continue;
        };
        let phase = phase.trim();
        let step = step.trim().to_string();
        let pkg = pkg.trim().to_string();
        if pkg.is_empty() || step.is_empty() {
            continue;
        }
        match phase {
            "start" => {
                first_start = Some(first_start.map_or(ts, |f: f64| f.min(ts)));
                open.insert((pkg, step), ts);
            }
            "end" => {
                last_end = Some(last_end.map_or(ts, |l: f64| l.max(ts)));
                if let Some(start) = open.remove(&(pkg.clone(), step.clone())) {
                    let d = (ts - start).max(0.0);
                    *acc.entry(pkg).or_default().entry(step).or_default() += d;
                }
            }
            _ => {}
        }
    }

    let mut packages: Vec<PkgTiming> = acc
        .into_iter()
        .map(|(package, steps)| {
            let mut steps: Vec<StepTime> = steps
                .into_iter()
                .map(|(step, seconds)| StepTime { step, seconds })
                .collect();
            steps.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));
            let seconds = steps.iter().map(|s| s.seconds).sum();
            PkgTiming {
                package,
                seconds,
                steps,
            }
        })
        .collect();
    packages.sort_by(|a, b| b.seconds.total_cmp(&a.seconds));

    BuildTimes {
        wall_seconds: match (first_start, last_end) {
            (Some(f), Some(l)) if l >= f => Some(l - f),
            _ => None,
        },
        finished_at: last_end,
        packages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
100.0:start:download            :  pkg-a
101.5:end  :download            :  pkg-a
101.5:start:build               :  pkg-a
110.0:end  :build               :  pkg-a
102.0:start:build               :  pkg-b
104.0:end  :build               :  pkg-b
";

    #[test]
    fn aggregates_per_package() {
        let bt = parse(SAMPLE);
        assert_eq!(bt.packages.len(), 2);
        assert_eq!(bt.packages[0].package, "pkg-a");
        assert!((bt.packages[0].seconds - 10.0).abs() < 1e-9);
        assert!((bt.packages[1].seconds - 2.0).abs() < 1e-9);
        assert_eq!(bt.wall_seconds, Some(10.0));
        assert_eq!(bt.finished_at, Some(110.0));
        assert_eq!(bt.packages[0].steps[0].step, "build");
    }
}
