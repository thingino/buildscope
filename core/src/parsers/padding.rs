//! Trailing-padding detection. Flash images are padded with 0xFF (NOR erase
//! state) or zeros; the content end is where the padding run begins.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaddingInfo {
    /// The pad byte (0xFF or 0x00) if the image ends in a run of it.
    pub pad_byte: Option<u8>,
    pub trailing_bytes: u64,
    pub content_end: u64,
}

pub fn analyze(data: &[u8]) -> PaddingInfo {
    if data.is_empty() {
        return PaddingInfo {
            pad_byte: None,
            trailing_bytes: 0,
            content_end: 0,
        };
    }
    let last = data[data.len() - 1];
    if last != 0xFF && last != 0x00 {
        return PaddingInfo {
            pad_byte: None,
            trailing_bytes: 0,
            content_end: data.len() as u64,
        };
    }
    let run = data.iter().rev().take_while(|&&b| b == last).count() as u64;
    PaddingInfo {
        pad_byte: Some(last),
        trailing_bytes: run,
        content_end: data.len() as u64 - run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ff_padding() {
        let mut d = vec![1u8, 2, 3];
        d.extend(std::iter::repeat(0xFF).take(100));
        let p = analyze(&d);
        assert_eq!(p.pad_byte, Some(0xFF));
        assert_eq!(p.trailing_bytes, 100);
        assert_eq!(p.content_end, 3);
    }

    #[test]
    fn no_padding() {
        let p = analyze(&[1, 2, 3]);
        assert_eq!(p.pad_byte, None);
        assert_eq!(p.content_end, 3);
    }
}
