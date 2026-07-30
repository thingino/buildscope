//! build/packages-file-list.txt: `package,./relative/path` per line, the
//! canonical map from an installed file to the package that installed it.
//! Buildroot semantics: the last writer of a path wins.

use std::collections::HashMap;

pub fn parse(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let Some((pkg, path)) = line.split_once(',') else {
            continue;
        };
        if pkg.is_empty() {
            continue;
        }
        let rel = path.strip_prefix("./").unwrap_or(path);
        if rel.is_empty() {
            continue;
        }
        map.insert(rel.to_string(), pkg.to_string());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_last_writer_wins() {
        let m = parse("busybox,./bin/busybox\nfoo,./etc/conf\nbar,./etc/conf\n");
        assert_eq!(m.get("bin/busybox").unwrap(), "busybox");
        assert_eq!(m.get("etc/conf").unwrap(), "bar");
        assert_eq!(m.len(), 2);
    }
}
