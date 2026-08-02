//! Every path that writes JSON to stdout must still write JSON.
//!
//! There is a filter over terminal output, so a partition name copied out of a
//! crafted image cannot drive the terminal reading it. Machine output has to
//! bypass that filter: it is parsed, not read, and changing a byte corrupts the
//! file a redirect produces. `diff --json` went through the filter by mistake
//! once, which turned every newline of the pretty-printed JSON into `?` and
//! left it unparseable, so the rule gets a test rather than a comment.
//!
//! Fixtures are produced by the tool instead of being checked in, so they
//! cannot fall behind the report schema.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_buildscope");

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("buildscope-machine-output-{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

/// Carve a small image into a report, which is the cheapest way to a valid one.
fn report_from(dir: &Path, image: &str, fill: u8, len: usize) -> PathBuf {
    let img = dir.join(image);
    std::fs::write(&img, vec![fill; len]).expect("write image");
    let out = img.with_extension("json");
    let st = Command::new(BIN)
        .arg("carve")
        .arg(&img)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run carve");
    assert!(
        st.status.success(),
        "carve failed: {}",
        String::from_utf8_lossy(&st.stderr)
    );
    out
}

#[test]
fn diff_json_is_parseable_json() {
    let dir = tmpdir("diff");
    let a = report_from(&dir, "a.bin", 0x00, 4096);
    let b = report_from(&dir, "b.bin", 0xAB, 8192);

    let out = Command::new(BIN)
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .arg("--json")
        .output()
        .expect("run diff");
    assert!(
        out.status.success(),
        "diff failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = String::from_utf8(out.stdout).expect("stdout is utf-8");
    // The symptom when this regresses: newlines become `?`, so it is not JSON.
    assert!(
        !text.starts_with("{?"),
        "newlines were filtered out of machine output: {:?}",
        &text[..text.len().min(60)]
    );
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "diff --json is not JSON ({e}): {:?}",
            &text[..text.len().min(80)]
        )
    });
    assert_eq!(parsed["schema"], 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_json_to_stdout_is_parseable_json() {
    let dir = tmpdir("carve-stdout");
    std::fs::write(dir.join("c.bin"), vec![0x5A; 4096]).expect("write image");

    let out = Command::new(BIN)
        .arg("carve")
        .arg(dir.join("c.bin"))
        .arg("--out")
        .arg("-")
        .output()
        .expect("run carve");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).expect("utf-8");
    // The summary shares this stream, so take the object that follows it.
    let at = text.find('{').expect("some JSON on stdout");
    let parsed: serde_json::Value =
        serde_json::from_str(&text[at..]).expect("carve --out - is not JSON");
    assert_eq!(parsed["schema"], 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hostile_name_reaches_json_intact_but_never_the_terminal() {
    let dir = tmpdir("hostile");
    // Carve names a build after the file, so an escape in the name gets one
    // into the report the same way a crafted mtdparts partition name would.
    let hostile = format!("board{}[2Jx.bin", '\u{1b}');
    let a = report_from(&dir, &hostile, 0x11, 4096);
    let b = report_from(&dir, "plain.bin", 0x22, 8192);

    let json = Command::new(BIN)
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .arg("--json")
        .output()
        .expect("run diff --json");
    let text = String::from_utf8(json.stdout).expect("utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("still JSON");
    // Machine output keeps the bytes: serde escapes the ESC rather than losing it.
    assert!(
        parsed["a"]["name"].as_str().unwrap().contains('\u{1b}'),
        "machine output should preserve the name exactly"
    );
    assert!(!text.contains('\u{1b}'), "raw ESC in machine output");

    // The human summary must carry nothing the terminal will act on.
    let summary = Command::new(BIN)
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .output()
        .expect("run diff");
    let shown = String::from_utf8_lossy(&summary.stdout);
    assert!(!shown.contains('\u{1b}'), "escape reached the terminal");
    assert!(
        shown.contains("board?[2Jx"),
        "the name should still be readable: {shown:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
