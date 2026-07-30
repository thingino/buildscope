//! Flattened device tree, enough of it to read a FIT image.
//!
//! A `.itb` is a device tree whose nodes describe payloads, so reading one
//! means walking the same structure a kernel dtb uses. The format is a header,
//! a token stream of nodes and properties, and a string table the property
//! names point into. All big endian, and none of it compressed.
//!
//! This is a reader, not a device-tree library: it walks the tree once and
//! hands each node and property to a visitor.

use super::be_u32;

pub const MAGIC: u32 = 0xD00D_FEED;
const TOKEN_BEGIN_NODE: u32 = 1;
const TOKEN_END_NODE: u32 = 2;
const TOKEN_PROP: u32 = 3;
const TOKEN_NOP: u32 = 4;
const TOKEN_END: u32 = 9;
/// Deep enough for any real tree; stops a malformed one from running away.
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct FdtHeader {
    pub total_size: u32,
    pub struct_offset: u32,
    pub struct_size: u32,
    pub strings_offset: u32,
    pub strings_size: u32,
    pub version: u32,
}

pub fn parse_header(data: &[u8]) -> Option<FdtHeader> {
    if be_u32(data, 0)? != MAGIC {
        return None;
    }
    let h = FdtHeader {
        total_size: be_u32(data, 4)?,
        struct_offset: be_u32(data, 8)?,
        strings_offset: be_u32(data, 12)?,
        version: be_u32(data, 20)?,
        strings_size: be_u32(data, 32)?,
        struct_size: be_u32(data, 36)?,
    };
    // Every region has to live inside the blob the header describes.
    let end = h.total_size as usize;
    if end < 64
        || h.struct_offset as usize + h.struct_size as usize > end
        || h.strings_offset as usize + h.strings_size as usize > end
        || h.version == 0
        || h.version > 32
    {
        return None;
    }
    Some(h)
}

/// What the walker reports, in the order it is found.
pub enum Event<'a> {
    /// Entering a node, with its path from the root (`/images/kernel-1`).
    Node { path: String },
    /// A property of the node most recently entered.
    Prop {
        path: &'a str,
        name: &'a str,
        value: &'a [u8],
    },
}

fn cstr(data: &[u8], at: usize) -> Option<&str> {
    let rest = data.get(at..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    std::str::from_utf8(&rest[..end]).ok()
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// The root node is named with the empty string, so it contributes nothing to
/// a path: its children are `/images`, not `//images`.
fn join_path(stack: &[String]) -> String {
    let named: Vec<&str> = stack
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    if named.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", named.join("/"))
    }
}

/// Walk the tree, handing each node and property to `visit`. Stops early if
/// the visitor returns false.
pub fn walk(data: &[u8], mut visit: impl FnMut(Event) -> bool) -> Option<()> {
    let h = parse_header(data)?;
    let strings =
        data.get(h.strings_offset as usize..(h.strings_offset + h.strings_size) as usize)?;
    let start = h.struct_offset as usize;
    let end = start + h.struct_size as usize;

    let mut pos = start;
    let mut stack: Vec<String> = Vec::new();
    let mut path = String::from("/");

    while pos + 4 <= end {
        let token = be_u32(data, pos)?;
        pos += 4;
        match token {
            TOKEN_BEGIN_NODE => {
                let name = cstr(data, pos)?;
                pos = align4(pos + name.len() + 1);
                if stack.len() >= MAX_DEPTH {
                    return None;
                }
                stack.push(name.to_string());
                path = join_path(&stack);
                if !visit(Event::Node { path: path.clone() }) {
                    return Some(());
                }
            }
            TOKEN_END_NODE => {
                stack.pop();
                path = join_path(&stack);
            }
            TOKEN_PROP => {
                let len = be_u32(data, pos)? as usize;
                let nameoff = be_u32(data, pos + 4)? as usize;
                pos += 8;
                let value = data.get(pos..pos + len)?;
                pos = align4(pos + len);
                let name = cstr(strings, nameoff)?;
                if !visit(Event::Prop {
                    path: &path,
                    name,
                    value,
                }) {
                    return Some(());
                }
            }
            TOKEN_NOP => {}
            TOKEN_END => break,
            _ => return None,
        }
    }
    Some(())
}

/// A property holding a NUL-terminated string, which is how a device tree
/// stores text.
pub fn prop_str(value: &[u8]) -> Option<&str> {
    let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
    std::str::from_utf8(&value[..end]).ok()
}

pub fn prop_u32(value: &[u8]) -> Option<u32> {
    (value.len() == 4).then(|| be_u32(value, 0)).flatten()
}

#[cfg(test)]
pub(crate) mod build {
    //! Assembling a blob, used by the tests here and by the FIT tests.
    pub struct Builder {
        pub structs: Vec<u8>,
        pub strings: Vec<u8>,
    }

    impl Builder {
        pub fn new() -> Self {
            Builder {
                structs: Vec::new(),
                strings: Vec::new(),
            }
        }

        fn string_ref(&mut self, name: &str) -> u32 {
            let at = self.strings.len() as u32;
            self.strings.extend_from_slice(name.as_bytes());
            self.strings.push(0);
            at
        }

        pub fn begin(&mut self, name: &str) -> &mut Self {
            self.structs.extend_from_slice(&1u32.to_be_bytes());
            self.structs.extend_from_slice(name.as_bytes());
            self.structs.push(0);
            while !self.structs.len().is_multiple_of(4) {
                self.structs.push(0);
            }
            self
        }

        pub fn end(&mut self) -> &mut Self {
            self.structs.extend_from_slice(&2u32.to_be_bytes());
            self
        }

        pub fn prop(&mut self, name: &str, value: &[u8]) -> &mut Self {
            let at = self.string_ref(name);
            self.structs.extend_from_slice(&3u32.to_be_bytes());
            self.structs
                .extend_from_slice(&(value.len() as u32).to_be_bytes());
            self.structs.extend_from_slice(&at.to_be_bytes());
            self.structs.extend_from_slice(value);
            while !self.structs.len().is_multiple_of(4) {
                self.structs.push(0);
            }
            self
        }

        pub fn prop_str(&mut self, name: &str, value: &str) -> &mut Self {
            let mut v = value.as_bytes().to_vec();
            v.push(0);
            self.prop(name, &v)
        }

        pub fn finish(&mut self) -> Vec<u8> {
            self.structs.extend_from_slice(&9u32.to_be_bytes()); // FDT_END
            let header_len = 64usize;
            let struct_off = header_len;
            let strings_off = struct_off + self.structs.len();
            let total = strings_off + self.strings.len();
            let mut out = vec![0u8; header_len];
            out[0..4].copy_from_slice(&super::MAGIC.to_be_bytes());
            out[4..8].copy_from_slice(&(total as u32).to_be_bytes());
            out[8..12].copy_from_slice(&(struct_off as u32).to_be_bytes());
            out[12..16].copy_from_slice(&(strings_off as u32).to_be_bytes());
            out[20..24].copy_from_slice(&17u32.to_be_bytes()); // version
            out[32..36].copy_from_slice(&(self.strings.len() as u32).to_be_bytes());
            out[36..40].copy_from_slice(&(self.structs.len() as u32).to_be_bytes());
            out.extend_from_slice(&self.structs);
            out.extend_from_slice(&self.strings);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build::Builder;
    use super::*;

    #[test]
    fn walks_nodes_and_properties_in_order() {
        let blob = Builder::new()
            .begin("")
            .prop_str("description", "a tree")
            .begin("images")
            .begin("kernel-1")
            .prop_str("type", "kernel")
            .prop("data", &[1, 2, 3, 4, 5])
            .end()
            .end()
            .end()
            .finish();

        let mut nodes = Vec::new();
        let mut props = Vec::new();
        walk(&blob, |e| {
            match e {
                Event::Node { path } => nodes.push(path),
                Event::Prop { path, name, value } => {
                    props.push((path.to_string(), name.to_string(), value.to_vec()))
                }
            }
            true
        })
        .expect("walk");

        assert_eq!(nodes, vec!["/", "/images", "/images/kernel-1"]);
        assert_eq!(props[0].1, "description");
        assert_eq!(prop_str(&props[0].2), Some("a tree"));
        assert_eq!(props[2].1, "data");
        assert_eq!(props[2].2, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn rejects_non_fdt() {
        assert!(parse_header(&[0u8; 64]).is_none());
        assert!(parse_header(&vec![0xFFu8; 1024]).is_none());
        // magic right, sizes impossible
        let mut bad = Builder::new().begin("").end().finish();
        bad[4..8].copy_from_slice(&8u32.to_be_bytes());
        assert!(parse_header(&bad).is_none());
    }

    #[test]
    fn a_visitor_can_stop_early() {
        let blob = Builder::new()
            .begin("")
            .begin("a")
            .end()
            .begin("b")
            .end()
            .end()
            .finish();
        let mut seen = 0;
        walk(&blob, |_| {
            seen += 1;
            false
        });
        assert_eq!(seen, 1);
    }
}
