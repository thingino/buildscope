//! os-release: `KEY=VALUE` lines, values optionally quoted.
//!
//! Buildroot generates one into the target, so it is the portable way to learn
//! what a build calls itself. Projects tend to record their branch and
//! revision here too -- `VERSION_CODENAME` and `BUILD_ID` are the usual
//! places -- which is otherwise nowhere in a build tree that this can read.

use std::collections::BTreeMap;

/// Parse into key/value pairs. Unparseable lines are skipped rather than
/// failing the whole file: this is metadata, and a build is still worth
/// reporting on when one line of it is odd.
pub fn parse(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            continue;
        }
        out.insert(key.to_string(), unquote(value.trim()));
    }
    out
}

fn unquote(value: &str) -> String {
    let b = value.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        let inner = &value[1..value.len() - 1];
        // Only double quotes take backslash escapes, per the spec.
        return if b[0] == b'"' {
            unescape(inner)
        } else {
            inner.to_string()
        };
    }
    value.to_string()
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(next) => out.push(next),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_real_buildroot_os_release() {
        // Trimmed from a Thingino build: the shapes that actually occur.
        let text = r#"NAME=Thingino
ID=thingino
VERSION="1 (Ciao)"
VERSION_ID=1
VERSION_CODENAME=ciao
PRETTY_NAME="Thingino 1 (Ciao)"
BUILD_ID="ciao+b04c33f, 2026-07-31 05:26:44 +0000"
IMAGE_ID=teacup_t31x
BUILDROOT_VERSION_ID=2026.05
"#;
        let m = parse(text);
        assert_eq!(m.get("VERSION_CODENAME").unwrap(), "ciao");
        assert_eq!(m.get("PRETTY_NAME").unwrap(), "Thingino 1 (Ciao)");
        assert_eq!(
            m.get("BUILD_ID").unwrap(),
            "ciao+b04c33f, 2026-07-31 05:26:44 +0000"
        );
        assert_eq!(m.get("IMAGE_ID").unwrap(), "teacup_t31x");
        assert_eq!(m.len(), 9);
    }

    #[test]
    fn quoting_rules() {
        let m = parse("A=\"q\"\nB='s'\nC=bare\nD=\"has \\\"inner\\\"\"\nE='no \\escape'\n");
        assert_eq!(m.get("A").unwrap(), "q");
        assert_eq!(m.get("B").unwrap(), "s");
        assert_eq!(m.get("C").unwrap(), "bare");
        // Double quotes unescape; single quotes are literal.
        assert_eq!(m.get("D").unwrap(), "has \"inner\"");
        assert_eq!(m.get("E").unwrap(), "no \\escape");
    }

    #[test]
    fn skips_junk_without_losing_the_rest() {
        let m = parse("# a comment\n\nNOEQUALS\n=novalue\nBAD KEY=x\nGOOD=y\n");
        assert_eq!(m.len(), 1);
        assert_eq!(m.get("GOOD").unwrap(), "y");
    }
}
