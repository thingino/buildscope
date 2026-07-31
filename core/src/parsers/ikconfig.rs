//! CONFIG_IKCONFIG: the kernel's own .config, gzipped and embedded in the
//! image between `IKCFG_ST` and `IKCFG_ED`. It is what `/proc/config.gz`
//! serves and what `scripts/extract-ikconfig` digs out of a vmlinux.
//!
//! Worth having because it is the only place the build's kernel options
//! survive into the artifact: the `.config` in the build tree is gone once the
//! tree is, and nothing else in an image records why a driver is present.

/// The gzipped config between the markers, if the kernel carries one.
///
/// The markers are searched rather than located: their offset depends on where
/// the linker put `kernel/configs.o`, which varies by kernel and by build.
pub fn find(kernel: &[u8]) -> Option<&[u8]> {
    const START: &[u8] = b"IKCFG_ST";
    const END: &[u8] = b"IKCFG_ED";
    let start = window_find(kernel, START)? + START.len();
    let end = window_find(&kernel[start..], END)? + start;
    let blob = kernel.get(start..end)?;
    // Both markers can appear in a kernel that was built without the config
    // (they are string constants either way), so require the payload to look
    // like the gzip stream it should be.
    blob.starts_with(&[0x1F, 0x8B, 0x08]).then_some(blob)
}

fn window_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// One option, as the file states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    /// `y`, `m`, a number, or a string. `n` for the "is not set" form, which
    /// is how the file spells a disabled option.
    pub value: String,
}

/// Parse a `.config`. Comments are dropped except the "is not set" form,
/// which carries meaning: the option was considered and disabled, as opposed
/// to never having existed in this kernel's Kconfig at all.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# ") {
            if let Some(key) = rest.strip_suffix(" is not set") {
                if is_key(key) {
                    out.push(Entry {
                        key: key.to_string(),
                        value: "n".to_string(),
                    });
                }
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !is_key(key) {
            continue;
        }
        // String options are quoted in the file; the quotes are syntax.
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        out.push(Entry {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    out
}

fn is_key(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with("CONFIG_")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_forms_a_config_uses() {
        let text = "#\n\
                    # Automatically generated file; DO NOT EDIT.\n\
                    #\n\
                    CONFIG_MIPS=y\n\
                    CONFIG_MODULES=m\n\
                    # CONFIG_SMP is not set\n\
                    CONFIG_HZ=100\n\
                    CONFIG_LOCALVERSION=\"__isvp_swan_1.0__\"\n\
                    # a plain comment\n\
                    NOT_A_CONFIG=y\n";
        let e = parse(text);
        assert_eq!(e.len(), 5);
        assert_eq!(
            e[0],
            Entry {
                key: "CONFIG_MIPS".into(),
                value: "y".into()
            }
        );
        assert_eq!(e[1].value, "m");
        // The disabled form survives, as n.
        assert_eq!(
            e[2],
            Entry {
                key: "CONFIG_SMP".into(),
                value: "n".into()
            }
        );
        assert_eq!(e[3].value, "100");
        // Quotes are syntax, not value.
        assert_eq!(e[4].value, "__isvp_swan_1.0__");
    }

    #[test]
    fn finds_the_blob_between_markers() {
        let mut k = b"....IKCFG_ST".to_vec();
        k.extend_from_slice(&[0x1F, 0x8B, 0x08, 0x00, 0x99]);
        k.extend_from_slice(b"IKCFG_ED....");
        assert_eq!(find(&k).unwrap(), &[0x1F, 0x8B, 0x08, 0x00, 0x99]);
    }

    #[test]
    fn markers_without_a_gzip_payload_are_not_a_config() {
        // A kernel built without CONFIG_IKCONFIG still contains the strings.
        let k = b"..IKCFG_STIKCFG_ED..".to_vec();
        assert!(find(&k).is_none());
        let k = b"..IKCFG_ST not gzip IKCFG_ED..".to_vec();
        assert!(find(&k).is_none());
    }

    #[test]
    fn absent_markers_are_absent() {
        assert!(find(b"no config here at all").is_none());
    }
}
