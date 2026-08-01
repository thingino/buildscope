//! Self-contained HTML export: take the built viewer bundle, inline its CSS
//! (fonts become data URIs) and JS, and inject the report as
//! window.__BUILDSCOPE_REPORT__. The result is one file that renders the
//! full viewer anywhere, no server, no network.

use buildscope_core::report::Report;
use std::fs;
use std::io;
use std::path::Path;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64[(n >> 18 & 63) as usize] as char);
        out.push(B64[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
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
                    out.push_str(&format!(
                        "data:{};base64,{}",
                        mime_of(clean),
                        base64(&bytes)
                    ));
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

/// Guard the viewer bundle against a literal `</script>` terminating the
/// inline script early. Inside a JS string `<\/` is identical to `</`.
///
/// The bundle is our own build, so this only has to survive its own string
/// literals; report data goes through `json_safe` instead, which is stricter.
fn script_safe(s: &str) -> String {
    s.replace("</", "<\\/")
}

/// Escape every `<` in a JSON payload before it is inlined into a `<script>`.
///
/// `</` alone is not enough. `<!--` puts the HTML tokenizer into script-data
/// escaped state, where the closing `</script>` no longer closes anything, so
/// a crafted string in an image swallows the rest of the document and the page
/// renders nothing at all. Escaping the `<` itself removes the whole class:
/// JSON has no `<` outside a string literal, and `\u003C` is that character in
/// every one of them, so the payload is unchanged and no `<` reaches the
/// tokenizer.
fn json_safe(s: &str) -> String {
    s.replace('<', "\\u003C")
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
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no module script in dist index.html",
        ));
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
        json_safe(report_json),
        script_safe(&js),
    );
    html.replace_range(pos..tag_end, &replacement);

    Ok(html)
}

/// The cheap facts a picker needs: enough to name a build and say which are
/// nearest their limits, without fetching a single report. Shared by the
/// static site and the fleet snapshot so the two indexes cannot drift apart;
/// each caller adds its own locator (`id` for the site, `file` for the tar).
/// The branch and revision a build came from, as its own os-release recorded
/// it. BUILD_ID conventionally reads "<branch>+<rev>, <date>", so the part
/// before the comma is the useful half; the codename alone is the fallback.
fn build_ref(r: &Report) -> Option<String> {
    if let Some(id) = r.build.os_release.get("BUILD_ID") {
        let head = id.split(',').next().unwrap_or(id).trim();
        if !head.is_empty() {
            return Some(head.to_string());
        }
    }
    r.build
        .os_release
        .get("VERSION_CODENAME")
        .filter(|v| !v.is_empty())
        .cloned()
}

pub fn index_entry(r: &Report) -> serde_json::Map<String, serde_json::Value> {
    let fullest = r.flash.as_ref().and_then(|f| {
        f.partitions
            .iter()
            .filter(|p| !p.overlaps)
            .filter_map(|p| {
                let size = p.size?;
                let used = p.used_bytes.or(p.content_bytes)?;
                (size > 0).then(|| (p.name.clone(), used as f64 / size as f64))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
    });
    let mut entry = serde_json::Map::new();
    entry.insert("name".into(), serde_json::json!(r.build.name));
    entry.insert("build_ref".into(), serde_json::json!(build_ref(r)));
    entry.insert(
        "flash_bytes".into(),
        serde_json::json!(r.flash.as_ref().and_then(|f| f.total_bytes)),
    );
    entry.insert(
        "rootfs_bytes".into(),
        serde_json::json!(r.rootfs.as_ref().and_then(|x| x.compressed_bytes)),
    );
    entry.insert(
        "fullest_partition".into(),
        serde_json::json!(fullest.as_ref().map(|(n, _)| n)),
    );
    entry.insert(
        "fullest_fill".into(),
        serde_json::json!(fullest.as_ref().map(|(_, f)| f)),
    );
    // The layout itself, so a fleet can be compared partition by partition
    // without opening a single report. Positional -- [name, offset, size,
    // used] -- because this is read by the viewer beside it, and at fleet
    // scale the key names would outweigh the values they label.
    entry.insert(
        "partitions".into(),
        serde_json::json!(r
            .flash
            .as_ref()
            .map(|f| f
                .partitions
                .iter()
                .filter(|p| !p.overlaps)
                .map(|p| serde_json::json!([
                    p.name,
                    p.offset,
                    p.size,
                    p.used_bytes.or(p.content_bytes).unwrap_or(0)
                ]))
                .collect::<Vec<_>>())
            .unwrap_or_default()),
    );
    entry
}

/// Copy the built viewer and write one report per build beside it, in the
/// layout the viewer already looks for.
///
/// The alternative -- one file with every build inlined -- means downloading
/// all of them to read any one of them, which at fleet scale is tens of
/// megabytes before the first pixel. Here the index is a few hundred bytes per
/// build and a report is fetched only when it is opened. It is all static, so
/// any web host will do and nothing needs to be running.
pub fn build_site(dist: &Path, reports: &[Report], out: &Path) -> io::Result<()> {
    fs::create_dir_all(out.join("api").join("report"))?;
    copy_tree(dist, out)?;

    // The index the viewer asks for first, one entry per build, located by
    // the position its report is written at below.
    let entries: Vec<serde_json::Value> = reports
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut entry = index_entry(r);
            entry.insert("id".into(), serde_json::json!(i));
            serde_json::Value::Object(entry)
        })
        .collect();
    fs::write(
        out.join("api").join("index"),
        serde_json::json!({ "reports": entries }).to_string(),
    )?;

    for (i, r) in reports.iter().enumerate() {
        fs::write(
            out.join("api").join("report").join(i.to_string()),
            serde_json::to_string(r).expect("serialize report"),
        )?;
    }
    Ok(())
}

/// Copy the viewer bundle as it is: the site is served over HTTP, so the
/// module scripts that a file:// page cannot load are fine here.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
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
