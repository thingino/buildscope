//! FIT images (`.itb`), and plain device tree blobs.
//!
//! A FIT is a device tree whose `/images` nodes each carry a payload -- kernel,
//! ramdisk, device tree -- with the type, compression and load address beside
//! it, and `/configurations` naming which combinations are bootable. Reading it
//! turns a single opaque blob into an itemised list of what is inside and what
//! each part costs.
//!
//! Payloads live either in a `data` property or, for a FIT built with external
//! data, at a `data-offset` past the end of the tree.

use super::dtb;
use super::fdt::{self, Event};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct FitImage {
    pub name: String,
    pub description: String,
    /// "kernel" | "ramdisk" | "flat_dt" | ...
    pub image_type: String,
    pub arch: String,
    pub os: String,
    pub compression: String,
    pub bytes: u64,
    /// True when the payload sits past the tree rather than inside it.
    pub external: bool,
    pub load: Option<u32>,
    pub entry: Option<u32>,
    /// Algorithms of the hash nodes attached to this image.
    pub hashes: Vec<String>,
    /// For a device-tree payload carried inline, the board it describes. A FIT
    /// often holds several, one per board variant, and the file names inside
    /// it are only `fdt-1`, `fdt-2` and so on.
    pub board: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FitConfig {
    pub name: String,
    pub description: String,
    /// The images this configuration names, as (role, image).
    pub uses: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FitInfo {
    pub description: String,
    /// Bytes the tree itself occupies, payloads included when they are inline.
    pub tree_bytes: u64,
    /// Everything: the tree plus any external payloads behind it.
    pub total_bytes: u64,
    pub payload_bytes: u64,
    pub images: Vec<FitImage>,
    pub configs: Vec<FitConfig>,
    pub default_config: String,
}

type Props = BTreeMap<String, Vec<u8>>;

fn text(p: &Props, key: &str) -> String {
    p.get(key)
        .and_then(|v| fdt::prop_str(v))
        .unwrap_or("")
        .to_string()
}

fn num(p: &Props, key: &str) -> Option<u32> {
    p.get(key).and_then(|v| fdt::prop_u32(v))
}

/// A device tree that carries images is a FIT; one that does not is a plain
/// blob, which is worth reporting as such rather than as raw bytes.
pub fn parse(data: &[u8]) -> Option<FitInfo> {
    let header = fdt::parse_header(data)?;

    // Collect every property, keyed by the node holding it.
    let mut nodes: BTreeMap<String, Props> = BTreeMap::new();
    fdt::walk(data, |e| {
        match e {
            Event::Node { path } => {
                nodes.entry(path).or_default();
            }
            Event::Prop { path, name, value } => {
                nodes
                    .entry(path.to_string())
                    .or_default()
                    .insert(name.to_string(), value.to_vec());
            }
        }
        true
    })?;

    let mut images = Vec::new();
    let mut payload_bytes = 0u64;
    let mut external_end = 0u64;
    for (path, props) in &nodes {
        let Some(name) = path.strip_prefix("/images/") else {
            continue;
        };
        // Only the image nodes themselves; a hash node is a level deeper.
        if name.contains('/') {
            continue;
        }
        let inline = props.get("data").map(|d| d.len() as u64);
        let external_size = num(props, "data-size").map(|v| v as u64);
        let bytes = inline.or(external_size).unwrap_or(0);
        if let (Some(off), Some(size)) = (num(props, "data-offset"), external_size) {
            // External payloads are measured from the end of the tree.
            external_end = external_end.max(header.total_size as u64 + off as u64 + size);
        }
        payload_bytes += bytes;

        let hashes = nodes
            .iter()
            .filter(|(p, _)| {
                p.strip_prefix(path)
                    .is_some_and(|rest| rest.starts_with('/') && !rest[1..].contains('/'))
            })
            .filter_map(|(_, hp)| {
                let a = text(hp, "algo");
                (!a.is_empty()).then_some(a)
            })
            .collect();

        // A device tree payload can say which board it is for.
        let board = props
            .get("data")
            .and_then(|d| dtb::parse(d))
            .map(|d| {
                if d.model.is_empty() {
                    d.compatible.first().cloned().unwrap_or_default()
                } else {
                    d.model
                }
            })
            .unwrap_or_default();

        images.push(FitImage {
            name: name.to_string(),
            description: text(props, "description"),
            image_type: text(props, "type"),
            arch: text(props, "arch"),
            os: text(props, "os"),
            compression: text(props, "compression"),
            bytes,
            external: inline.is_none() && external_size.is_some(),
            load: num(props, "load"),
            entry: num(props, "entry"),
            hashes,
            board,
        });
    }

    // No images means this is a device tree, not a FIT.
    if images.is_empty() {
        return None;
    }

    let mut configs = Vec::new();
    for (path, props) in &nodes {
        let Some(name) = path.strip_prefix("/configurations/") else {
            continue;
        };
        if name.contains('/') {
            continue;
        }
        let uses = [
            "kernel",
            "ramdisk",
            "fdt",
            "firmware",
            "loadables",
            "script",
        ]
        .iter()
        .filter_map(|role| {
            let v = text(props, role);
            (!v.is_empty()).then(|| (role.to_string(), v))
        })
        .collect();
        configs.push(FitConfig {
            name: name.to_string(),
            description: text(props, "description"),
            uses,
        });
    }

    let root = nodes.get("/").cloned().unwrap_or_default();
    let default_config = nodes
        .get("/configurations")
        .map(|p| text(p, "default"))
        .unwrap_or_default();

    Some(FitInfo {
        description: text(&root, "description"),
        tree_bytes: header.total_size as u64,
        total_bytes: external_end.max(header.total_size as u64),
        payload_bytes,
        images,
        configs,
        default_config,
    })
}

/// A device tree blob that is not a FIT: worth naming, and its size is exactly
/// what its own header declares.
pub fn parse_dtb(data: &[u8]) -> Option<u64> {
    let h = fdt::parse_header(data)?;
    // A FIT is reported by `parse`; anything else is a plain blob.
    parse(data).is_none().then_some(h.total_size as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::fdt::build::Builder;

    fn u32be(v: u32) -> Vec<u8> {
        v.to_be_bytes().to_vec()
    }

    fn synth() -> Vec<u8> {
        let mut b = Builder::new();
        b.begin("")
            .prop_str("description", "Buildroot FIT")
            .begin("images")
            .begin("kernel-1")
            .prop_str("description", "Linux")
            .prop_str("type", "kernel")
            .prop_str("arch", "arm")
            .prop_str("os", "linux")
            .prop_str("compression", "gzip")
            .prop("load", &u32be(0x8000_0000))
            .prop("entry", &u32be(0x8000_0040))
            .prop("data", &vec![0xAA; 4096])
            .begin("hash-1")
            .prop_str("algo", "sha256")
            .prop("value", &[0u8; 32])
            .end()
            .end()
            .begin("fdt-1")
            .prop_str("type", "flat_dt")
            .prop_str("compression", "none")
            .prop("data", &vec![0xBB; 512])
            .end()
            .end()
            .begin("configurations")
            .prop_str("default", "conf-1")
            .begin("conf-1")
            .prop_str("description", "kernel and dtb")
            .prop_str("kernel", "kernel-1")
            .prop_str("fdt", "fdt-1")
            .end()
            .end()
            .end();
        b.finish()
    }

    #[test]
    fn reads_images_and_configurations() {
        let img = synth();
        let f = parse(&img).expect("fit");
        assert_eq!(f.description, "Buildroot FIT");
        assert_eq!(f.images.len(), 2);
        assert_eq!(f.payload_bytes, 4096 + 512);
        assert_eq!(f.tree_bytes, img.len() as u64);
        assert_eq!(f.total_bytes, img.len() as u64);

        let k = f.images.iter().find(|i| i.name == "kernel-1").unwrap();
        assert_eq!(k.image_type, "kernel");
        assert_eq!(k.arch, "arm");
        assert_eq!(k.os, "linux");
        assert_eq!(k.compression, "gzip");
        assert_eq!(k.bytes, 4096);
        assert!(!k.external);
        assert_eq!(k.load, Some(0x8000_0000));
        assert_eq!(k.entry, Some(0x8000_0040));
        assert_eq!(k.hashes, vec!["sha256"]);

        let d = f.images.iter().find(|i| i.name == "fdt-1").unwrap();
        assert_eq!(d.image_type, "flat_dt");
        assert_eq!(d.bytes, 512);
        assert!(d.hashes.is_empty());

        assert_eq!(f.default_config, "conf-1");
        assert_eq!(f.configs.len(), 1);
        assert_eq!(f.configs[0].name, "conf-1");
        assert_eq!(
            f.configs[0].uses,
            vec![
                ("kernel".to_string(), "kernel-1".to_string()),
                ("fdt".to_string(), "fdt-1".to_string())
            ]
        );
    }

    /// A FIT built with external data keeps its payloads past the tree, so the
    /// blob is larger than the tree the header describes.
    #[test]
    fn external_payloads_extend_past_the_tree() {
        let mut b = Builder::new();
        b.begin("")
            .begin("images")
            .begin("kernel-1")
            .prop_str("type", "kernel")
            .prop("data-offset", &u32be(0))
            .prop("data-size", &u32be(100_000))
            .end()
            .end()
            .end();
        let img = b.finish();
        let f = parse(&img).expect("fit");
        assert!(f.images[0].external);
        assert_eq!(f.images[0].bytes, 100_000);
        assert_eq!(f.tree_bytes, img.len() as u64);
        assert_eq!(f.total_bytes, img.len() as u64 + 100_000);
    }

    /// A FIT usually carries one device tree per board variant, named only
    /// `fdt-1`, `fdt-2` and so on. The payload knows better.
    #[test]
    fn a_device_tree_payload_names_its_board() {
        let mut inner = Builder::new();
        inner.begin("").prop_str("model", "Acme Widget Board").end();
        let board_dtb = inner.finish();

        let mut b = Builder::new();
        b.begin("")
            .begin("images")
            .begin("fdt-1")
            .prop_str("type", "flat_dt")
            .prop("data", &board_dtb)
            .end()
            .begin("kernel-1")
            .prop_str("type", "kernel")
            .prop("data", &[0u8; 64])
            .end()
            .end()
            .end();
        let f = parse(&b.finish()).expect("fit");
        let fdt = f.images.iter().find(|i| i.name == "fdt-1").unwrap();
        assert_eq!(fdt.board, "Acme Widget Board");
        // A kernel payload is not a device tree and claims no board.
        let k = f.images.iter().find(|i| i.name == "kernel-1").unwrap();
        assert_eq!(k.board, "");
    }

    #[test]
    fn a_plain_device_tree_is_not_a_fit() {
        let mut b = Builder::new();
        b.begin("")
            .prop_str("model", "Some Board")
            .begin("chosen")
            .prop_str("bootargs", "console=ttyS0")
            .end()
            .end();
        let dtb = b.finish();
        assert!(parse(&dtb).is_none());
        assert_eq!(parse_dtb(&dtb), Some(dtb.len() as u64));
        // ...and a FIT is not reported as a plain blob.
        assert_eq!(parse_dtb(&synth()), None);
    }

    #[test]
    fn rejects_non_fdt() {
        assert!(parse(&[0u8; 1024]).is_none());
        assert!(parse(&vec![0xFFu8; 1024]).is_none());
        assert!(parse_dtb(&[0u8; 1024]).is_none());
    }
}
