//! Device tree blobs, identified rather than merely recognised.
//!
//! A Buildroot build for anything but a raw-flash SoC drops a pile of `.dtb`
//! files into `images/`, one per board it can boot. Reporting each as "a device
//! tree" tells you nothing about which is which, and they are all about the
//! same size, so the useful facts are the ones that name the board: the `model`
//! string and the `compatible` list in the root node. The kernel command line
//! in `/chosen` is worth having too, since on many boards that is where the
//! root device is chosen.
//!
//! An overlay (`.dtbo`) is the same container with a different shape -- nodes
//! named `fragment@N` carrying an `__overlay__` -- and no model of its own,
//! because it names what it patches rather than what it is.

use super::fdt::{self, Event};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DtbInfo {
    /// The board's own name, from the root `model` property.
    pub model: String,
    /// Root `compatible`, most specific first, which is what the kernel
    /// matches a board against.
    pub compatible: Vec<String>,
    /// Kernel command line the tree carries, if any.
    pub bootargs: String,
    pub node_count: u32,
    pub property_count: u32,
    pub total_bytes: u64,
    pub struct_bytes: u32,
    pub strings_bytes: u32,
    /// True for an overlay, which patches a tree rather than describing a board.
    pub is_overlay: bool,
    /// Fragments an overlay applies, and what each targets.
    pub targets: Vec<String>,
}

/// A `compatible` property is several NUL-separated strings in one value.
fn string_list(value: &[u8]) -> Vec<String> {
    value
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

pub fn parse(data: &[u8]) -> Option<DtbInfo> {
    let header = fdt::parse_header(data)?;
    let mut info = DtbInfo {
        total_bytes: header.total_size as u64,
        struct_bytes: header.struct_size,
        strings_bytes: header.strings_size,
        ..Default::default()
    };

    fdt::walk(data, |e| {
        match e {
            Event::Node { path } => {
                info.node_count += 1;
                // An overlay is a set of fragments, each naming its target.
                if path.starts_with("/fragment@") && !path[1..].contains("/__overlay__") {
                    info.is_overlay = true;
                }
            }
            Event::Prop { path, name, value } => {
                info.property_count += 1;
                match (path, name) {
                    ("/", "model") => {
                        info.model = fdt::prop_str(value).unwrap_or("").to_string()
                    }
                    ("/", "compatible") => info.compatible = string_list(value),
                    ("/chosen", "bootargs") => {
                        info.bootargs = fdt::prop_str(value).unwrap_or("").to_string()
                    }
                    (p, "target-path") if p.starts_with("/fragment@") => {
                        if let Some(t) = fdt::prop_str(value) {
                            info.targets.push(t.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        true
    })?;

    Some(info)
}

/// Device trees carried *inside* another artifact.
///
/// A board that boots from raw flash usually ships no `.dtb` of its own: the
/// bootloader's tree is appended to its binary (`CONFIG_OF_SEPARATE`) and the
/// kernel's is linked into the kernel (`CONFIG_BUILTIN_DTB`). Both occupy real
/// flash and neither appears as a file, so the only way to account for them is
/// to look. The header is self-describing enough to find safely: a magic, then
/// a total size that has to fit, then a structure that has to walk.
pub fn find_embedded(data: &[u8], max_hits: usize) -> Vec<(usize, DtbInfo)> {
    const MAGIC: [u8; 4] = [0xD0, 0x0D, 0xFE, 0xED];
    let mut out = Vec::new();
    let mut at = 0usize;
    // A blob is 4-byte aligned wherever a linker or a build put it.
    while at + 8 <= data.len() && out.len() < max_hits {
        if data[at..at + 4] != MAGIC {
            at += 4;
            continue;
        }
        match parse(&data[at..]) {
            // A tree with nothing in it is a coincidence, not a device tree.
            Some(info) if info.node_count > 1 => {
                let skip = (info.total_bytes as usize).max(4);
                out.push((at, info));
                at += skip;
            }
            _ => at += 4,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::fdt::build::Builder;

    fn board() -> Vec<u8> {
        let mut compat = Vec::new();
        for s in ["acme,widget-c", "acme,widget"] {
            compat.extend_from_slice(s.as_bytes());
            compat.push(0);
        }
        let mut b = Builder::new();
        b.begin("")
            .prop_str("model", "Acme Widget Board")
            .prop("compatible", &compat)
            .begin("chosen")
            .prop_str("bootargs", "console=ttyS0 root=/dev/mmcblk0p2")
            .end()
            .begin("soc")
            .begin("serial@1c28000")
            .prop_str("status", "okay")
            .end()
            .end()
            .end();
        b.finish()
    }

    #[test]
    fn names_the_board_it_describes() {
        let d = parse(&board()).expect("dtb");
        assert_eq!(d.model, "Acme Widget Board");
        assert_eq!(d.compatible, vec!["acme,widget-c", "acme,widget"]);
        assert_eq!(d.bootargs, "console=ttyS0 root=/dev/mmcblk0p2");
        assert!(!d.is_overlay);
        assert_eq!(d.total_bytes, board().len() as u64);
        // root, chosen, soc, serial
        assert_eq!(d.node_count, 4);
        assert!(d.property_count >= 4);
        assert!(d.struct_bytes > 0 && d.strings_bytes > 0);
    }

    #[test]
    fn an_overlay_is_told_apart_by_its_fragments() {
        let mut b = Builder::new();
        b.begin("")
            .begin("fragment@0")
            .prop_str("target-path", "/soc/mmc@1c0f000")
            .begin("__overlay__")
            .prop_str("status", "disabled")
            .end()
            .end()
            .end();
        let d = parse(&b.finish()).expect("dtbo");
        assert!(d.is_overlay);
        assert_eq!(d.targets, vec!["/soc/mmc@1c0f000"]);
        assert!(d.model.is_empty(), "an overlay names its target, not itself");
    }

    #[test]
    fn rejects_non_fdt() {
        assert!(parse(&[0u8; 256]).is_none());
        assert!(parse(&vec![0xFFu8; 4096]).is_none());
    }

    /// A bootloader appends its tree to its own binary and a kernel links one
    /// in, so neither is a file to be found -- only bytes inside one.
    #[test]
    fn finds_a_tree_buried_in_another_artifact() {
        let tree = board();
        let mut blob = vec![0x5Au8; 5000];
        blob.extend_from_slice(&tree);
        blob.extend(std::iter::repeat(0x00).take(3000));

        let hits = find_embedded(&blob, 8);
        assert_eq!(hits.len(), 1);
        let (at, info) = &hits[0];
        assert_eq!(*at, 5000);
        assert_eq!(info.model, "Acme Widget Board");
        assert_eq!(info.total_bytes, tree.len() as u64);
    }

    #[test]
    fn does_not_invent_trees_in_noise() {
        assert!(find_embedded(&vec![0u8; 1 << 16], 8).is_empty());
        assert!(find_embedded(&vec![0xFFu8; 1 << 16], 8).is_empty());
        // The magic alone, with nothing behind it, is not a tree.
        let mut fake = vec![0u8; 4096];
        fake[100..104].copy_from_slice(&[0xD0, 0x0D, 0xFE, 0xED]);
        assert!(find_embedded(&fake, 8).is_empty());
    }
}
