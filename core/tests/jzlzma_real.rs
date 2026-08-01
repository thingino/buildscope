//! Decode real Ingenic JZLZMA streams and check the output byte for byte
//! against a known-good decode.
//!
//! There is no compressor to build a fixture with, so this runs against real
//! firmware: point `BUILDSCOPE_JZLZMA_DIR` at a directory holding `<name>` and
//! `<name>.ref` pairs, where the `.ref` is the expected output. Skipped when
//! the variable is unset, so the suite still runs anywhere.
//!
//! Worth checking against a reference rather than against the header's size:
//! the distance decoder has two fields that are read unreversed, and getting
//! that wrong still produces output of exactly the promised length, with
//! correct literals and subtly wrong matches.

use buildscope_core::parsers::jzlzma;

#[test]
fn decodes_real_streams_byte_for_byte() {
    let Ok(dir) = std::env::var("BUILDSCOPE_JZLZMA_DIR") else {
        eprintln!("BUILDSCOPE_JZLZMA_DIR unset, skipping");
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("{dir}: unreadable, skipping");
        return;
    };

    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "ref") {
            continue;
        }
        let expected_path = path.with_extension(format!(
            "{}.ref",
            path.extension().and_then(|e| e.to_str()).unwrap_or("")
        ));
        let expected_path = if expected_path.exists() {
            expected_path
        } else {
            let mut p = path.clone().into_os_string();
            p.push(".ref");
            std::path::PathBuf::from(p)
        };
        let (Ok(input), Ok(expected)) = (std::fs::read(&path), std::fs::read(&expected_path))
        else {
            continue;
        };

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let header = jzlzma::parse(&input)
            .unwrap_or_else(|| panic!("{name}: not recognised as a jzlzma container"));
        let out = jzlzma::decompress(&input)
            .unwrap_or_else(|| panic!("{name}: {} container failed to decode", header.container));

        assert_eq!(
            out.len(),
            expected.len(),
            "{name}: decoded {} bytes, reference has {}",
            out.len(),
            expected.len()
        );
        // Byte for byte, not just the length: a distance bug keeps the length.
        assert!(
            out == expected,
            "{name}: decoded output differs from the reference at byte {}",
            out.iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(0)
        );
        eprintln!(
            "  {name}: {} container, dict 0x{:x} -> {} bytes, identical",
            header.container,
            header.dict_size,
            out.len()
        );
        checked += 1;
    }
    assert!(checked > 0, "{dir}: no <name>/<name>.ref pairs found");
}
