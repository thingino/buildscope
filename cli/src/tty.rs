//! Making report text safe to print on a terminal.
//!
//! Nearly every string in a report is copied out of the artifact being read:
//! partition names come from an `mtdparts=` string, image names from a
//! directory listing, package names from a manifest, and the build name from a
//! `--name` or a JSON file someone was handed. A terminal treats some byte
//! sequences in those as commands rather than as text, so printing one
//! unfiltered lets the file being *inspected* drive the screen it is inspected
//! on: `ESC ] 0 ; ... BEL` retitles the window, `ESC [ ... m` recolours it, and
//! a stray carriage return can scroll a line away so the reader never sees it.
//!
//! Nothing here emits escapes of its own, so the whole formatted line can be
//! filtered rather than each untrusted field: filtering a field at a time only
//! works while nobody adds the next `println!` that forgets to.
//!
//! Non-printable bytes become `?`, which is what `ls` does with them and for
//! the same reason.

/// A string with nothing in it a terminal will act on.
pub fn plain(s: &str) -> String {
    if !s.chars().any(is_control) {
        return s.to_string();
    }
    s.chars()
        .map(|c| if is_control(c) { '?' } else { c })
        .collect()
}

/// C0, DEL, and C1. C1 matters because `U+009B` is a CSI all by itself, so
/// dropping only the ASCII range would leave a working escape behind.
fn is_control(c: char) -> bool {
    (c as u32) < 0x20 || c == '\x7f' || matches!(c as u32, 0x80..=0x9f)
}

/// The same problem for JSON written to stdout, solved without touching the
/// data.
///
/// `serde_json` escapes C0, so the `ESC` that matters most never survives
/// serialization, but it emits `DEL` and C1 as themselves -- and `U+009B` is a
/// CSI with no `ESC` in front of it, so `buildscope carve x.bin --out -` read
/// straight in a terminal would hand one over. Filtering is the wrong tool
/// here: this output is meant to be parsed, and replacing bytes would corrupt
/// the report. `\u009b` is the *same JSON string*, so escaping loses nothing
/// and any reader decodes it back to the byte that was in the image.
///
/// Only string literals can hold these, every structural character in JSON
/// being ASCII, so this needs no parser to know where it is.
pub fn json_escape_c1(s: &str) -> String {
    if !s.chars().any(|c| matches!(c as u32, 0x7f..=0x9f)) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c as u32 {
            n @ 0x7f..=0x9f => out.push_str(&format!("\\u{n:04x}")),
            _ => out.push(c),
        }
    }
    out
}

/// `println!` that cannot be talked into doing anything else.
macro_rules! outln {
    () => { println!() };
    ($($arg:tt)*) => { println!("{}", $crate::tty::plain(&format!($($arg)*))) };
}

/// The same for the error stream.
macro_rules! errln {
    ($($arg:tt)*) => { eprintln!("{}", $crate::tty::plain(&format!($($arg)*))) };
}

pub(crate) use {errln, outln};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_what_a_terminal_would_act_on() {
        // Set the window title, then paint the rest red.
        let attack = "\x1b]0;OWNED\x07\x1b[31mrootfs";
        let out = plain(attack);
        assert!(!out.contains('\x1b'), "escape survived: {out:?}");
        assert!(!out.contains('\x07'));
        assert!(
            out.ends_with("rootfs"),
            "text should still be readable: {out:?}"
        );
    }

    #[test]
    fn strips_the_single_byte_csi() {
        // U+009B is a CSI on its own; an ASCII-only filter misses it.
        assert_eq!(plain("a\u{9b}31mb"), "a?31mb");
    }

    #[test]
    fn strips_line_control() {
        // A bare CR redraws over the line just printed, hiding it.
        assert_eq!(plain("real\rfake"), "real?fake");
        assert_eq!(plain("a\nb\tc\x7f"), "a?b?c?");
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(
            plain("rootfs (squashfs, 99.8% full)"),
            "rootfs (squashfs, 99.8% full)"
        );
        // Including text that is merely not ASCII.
        assert_eq!(plain("ölçü 中文 ✓"), "ölçü 中文 ✓");
    }

    #[test]
    fn json_keeps_its_meaning_while_losing_the_raw_byte() {
        let hostile = format!("{{\"name\":\"a{}31mb\"}}", '\u{9b}');
        let safe = json_escape_c1(&hostile);
        assert!(!safe.contains('\u{9b}'), "raw C1 survived: {safe:?}");
        assert!(safe.contains("\\u009b"));
        // Escaping is not filtering: a reader still gets the original bytes.
        let back: serde_json::Value = serde_json::from_str(&safe).unwrap();
        assert_eq!(back["name"], format!("a{}31mb", '\u{9b}'));
    }

    #[test]
    fn json_without_c1_is_returned_untouched() {
        let plain_json = r#"{"name":"rootfs","bytes":1024}"#;
        assert_eq!(json_escape_c1(plain_json), plain_json);
    }

    #[test]
    fn the_macro_filters_interpolated_values() {
        // The point of filtering the whole line: the value is what is hostile.
        let name = "\x1b]0;x\x07";
        let line = format!("== {name} ==");
        // Only the two bytes the terminal acts on are replaced; the rest of
        // the sequence is ordinary text and stays visible, which is the point.
        assert_eq!(plain(&line), "== ?]0;x? ==");
    }
}
