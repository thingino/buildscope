//! Self-contained HTML export: take the built viewer bundle, inline its CSS
//! (fonts become data URIs) and JS, and inject the report as
//! window.__BUILDSCOPE_REPORT__. The result is one file that renders the
//! full viewer anywhere, no server, no network.

use std::fs;
use std::io;
use std::path::Path;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64[(n >> 18 & 63) as usize] as char);
        out.push(B64[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// Replace `url(<rel>)` references in CSS with data URIs, resolving
/// relative to `css_dir`.
fn inline_css_urls(css: &str, css_dir: &Path) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(pos) = rest.find("url(") {
        out.push_str(&rest[..pos + 4]);
        rest = &rest[pos + 4..];
        let Some(end) = rest.find(')') else {
            break;
        };
        let raw = rest[..end].trim().trim_matches('"').trim_matches('\'');
        if raw.starts_with("data:") || raw.starts_with("http") {
            out.push_str(&rest[..end]);
        } else {
            let clean = raw.split(['?', '#']).next().unwrap_or(raw);
            let file = css_dir.join(clean.trim_start_matches("./"));
            match fs::read(&file) {
                Ok(bytes) => {
                    out.push_str(&format!("data:{};base64,{}", mime_of(clean), base64(&bytes)));
                }
                Err(_) => out.push_str(&rest[..end]),
            }
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn extract_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let key = format!("{attr}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Guard against a literal `</script>` (or `</`) terminating the inline
/// script early. Inside JS/JSON strings `<\/` is identical to `</`.
fn script_safe(s: &str) -> String {
    s.replace("</", "<\\/")
}

pub fn build_single_file(dist: &Path, report_json: &str) -> io::Result<String> {
    let mut html = fs::read_to_string(dist.join("index.html"))?;

    // Inline every stylesheet link.
    while let Some(pos) = html.find("<link rel=\"stylesheet\"") {
        let end = html[pos..]
            .find('>')
            .map(|e| pos + e + 1)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unterminated <link>"))?;
        let tag = html[pos..end].to_string();
        let href = extract_attr(&tag, "href")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "stylesheet without href"))?;
        let css_path = dist.join(href.trim_start_matches("./"));
        let css_dir = css_path.parent().unwrap_or(dist).to_path_buf();
        let css = fs::read_to_string(&css_path)?;
        let inlined = inline_css_urls(&css, &css_dir);
        html.replace_range(pos..end, &format!("<style>{inlined}</style>"));
    }

    // Inline the module script and inject the report just before it.
    let Some(pos) = html.find("<script type=\"module\"") else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "no module script in dist index.html"));
    };
    let tag_end = html[pos..]
        .find("></script>")
        .map(|e| pos + e + "></script>".len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unterminated script tag"))?;
    let tag = html[pos..tag_end].to_string();
    let src = extract_attr(&tag, "src")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "module script without src"))?;
    let js = fs::read_to_string(dist.join(src.trim_start_matches("./")))?;
    let replacement = format!(
        "<script>window.__BUILDSCOPE_REPORT__={};</script>\n<script type=\"module\">{}</script>",
        script_safe(report_json),
        script_safe(&js),
    );
    html.replace_range(pos..tag_end, &replacement);

    Ok(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn script_escape() {
        assert_eq!(script_safe("a</script>b"), "a<\\/script>b");
    }
}
