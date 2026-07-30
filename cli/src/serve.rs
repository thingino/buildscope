//! Minimal local HTTP server: same-origin API for the viewer plus static
//! files from the built viewer bundle.
//!
//! IPv6 is a first-class citizen rather than an afterthought: the server binds
//! a socket per requested address, so it listens on IPv4 and IPv6 at the same
//! time (the default is both loopbacks), and every address is formatted with
//! the brackets a URL needs. Relying on a single dual-stack `::` socket would
//! have made IPv4 reachability depend on the host's `bindv6only` setting.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tiny_http::{Header, Response, Server};

pub struct ReportEntry {
    pub name: String,
    pub json: String,
}

/// Bind specs that mean "every interface", expanded to one socket per family.
pub const ALL_INTERFACES: [&str; 2] = ["0.0.0.0", "::"];

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header")
}

/// Join a host and port the way a URL must: IPv6 literals need brackets.
pub fn url_authority(host: &str, port: u16) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("[{host}]:{port}"),
        _ => format!("{host}:{port}"),
    }
}

/// A wildcard address is not useful in a URL; point at the matching loopback.
fn browsable(addr: &SocketAddr) -> String {
    let port = addr.port();
    match addr.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => format!("http://127.0.0.1:{port}/"),
        IpAddr::V6(v6) if v6.is_unspecified() => format!("http://[::1]:{port}/"),
        IpAddr::V4(v4) => format!("http://{v4}:{port}/"),
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}/"),
    }
}

fn resolve(spec: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    // An IP literal binds exactly what was asked for; a hostname may resolve
    // to several addresses across both families, and all of them are used.
    if let Ok(ip) = spec.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addrs: Vec<SocketAddr> = url_authority(spec, port).to_socket_addrs()?.collect();
    if addrs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("{spec} resolved to no addresses"),
        ));
    }
    Ok(addrs)
}

struct Shared {
    reports: Vec<ReportEntry>,
    index_json: String,
    viewer_dir: Option<PathBuf>,
}

pub fn serve(binds: &[String], port: u16, reports: Vec<ReportEntry>, viewer_dir: Option<PathBuf>) -> ! {
    let mut specs: Vec<String> = Vec::new();
    for b in binds {
        let b = b.trim();
        if b.is_empty() {
            continue;
        }
        if b.eq_ignore_ascii_case("all") || b.eq_ignore_ascii_case("any") {
            specs.extend(ALL_INTERFACES.iter().map(|s| s.to_string()));
        } else {
            specs.push(b.to_string());
        }
    }
    if specs.is_empty() {
        specs.push("127.0.0.1".to_string());
    }

    let mut wanted: Vec<SocketAddr> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for spec in &specs {
        match resolve(spec, port) {
            Ok(addrs) => {
                for addr in addrs {
                    if !wanted.contains(&addr) {
                        wanted.push(addr);
                    }
                }
            }
            Err(e) => failures.push(format!("{spec}: {e}")),
        }
    }

    // IPv6 before IPv4, which is load-bearing rather than cosmetic: a
    // wildcard IPv6 socket on a dual-stack host also accepts IPv4, so
    // binding 0.0.0.0 first makes the subsequent :: bind fail and loses
    // IPv6 entirely. This order keeps both families in either kernel
    // configuration.
    wanted.sort_by_key(|a| if a.is_ipv6() { 0 } else { 1 });

    let mut listeners: Vec<(SocketAddr, TcpListener)> = Vec::new();
    let mut dual_stack = false;
    for addr in wanted {
        match TcpListener::bind(addr) {
            Ok(l) => listeners.push((addr, l)),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                // An IPv4 bind refused because a wildcard IPv6 socket is
                // already carrying IPv4 is the dual-stack case, not an error.
                let covered = addr.is_ipv4()
                    && listeners
                        .iter()
                        .any(|(a, _)| a.is_ipv6() && a.ip().is_unspecified());
                if covered {
                    dual_stack = true;
                } else {
                    failures.push(format!("{addr}: {e}"));
                }
            }
            Err(e) => failures.push(format!("{addr}: {e}")),
        }
    }

    if listeners.is_empty() {
        eprintln!("buildscope: could not listen on any address:");
        for f in &failures {
            eprintln!("  {f}");
        }
        std::process::exit(1);
    }
    for f in &failures {
        eprintln!("buildscope: skipped {f}");
    }

    let index_json = {
        let items: Vec<String> = reports
            .iter()
            .enumerate()
            .map(|(i, r)| {
                format!(
                    "{{\"id\":{},\"name\":{}}}",
                    i,
                    serde_json::to_string(&r.name).unwrap_or_else(|_| "\"?\"".into())
                )
            })
            .collect();
        format!("{{\"reports\":[{}]}}", items.join(","))
    };

    println!("buildscope: serving {} report(s) on:", reports.len());
    for (addr, _) in &listeners {
        println!("  {}", browsable(addr));
        if dual_stack && addr.is_ipv6() && addr.ip().is_unspecified() {
            println!("  http://127.0.0.1:{port}/  (IPv4 through the same dual-stack socket)");
        }
    }
    if viewer_dir.is_none() {
        println!("buildscope: no viewer bundle found; API only (/api/index, /api/report/<n>)");
    }

    let shared = Arc::new(Shared {
        reports,
        index_json,
        viewer_dir,
    });

    // One server per listener, each on its own thread.
    let mut handles = Vec::new();
    for (addr, listener) in listeners {
        let shared = Arc::clone(&shared);
        handles.push(thread::spawn(move || {
            let server = match Server::from_listener(listener, None) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("buildscope: {addr}: {e}");
                    return;
                }
            };
            for request in server.incoming_requests() {
                handle(&shared, request);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    std::process::exit(0);
}

fn handle(shared: &Shared, request: tiny_http::Request) {
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/");

    if path == "/api/index" {
        let resp = Response::from_string(shared.index_json.clone())
            .with_header(header("Content-Type", "application/json"));
        let _ = request.respond(resp);
        return;
    }
    if let Some(idx) = path.strip_prefix("/api/report/") {
        if let Ok(i) = idx.parse::<usize>() {
            if let Some(r) = shared.reports.get(i) {
                let resp = Response::from_string(r.json.clone())
                    .with_header(header("Content-Type", "application/json"));
                let _ = request.respond(resp);
                return;
            }
        }
        let _ = request.respond(Response::from_string("not found").with_status_code(404));
        return;
    }

    // Static viewer files.
    if let Some(dir) = &shared.viewer_dir {
        let rel = path.trim_start_matches('/');
        // Refuse anything that tries to climb out of the bundle.
        if rel.split('/').any(|c| c == "..") {
            let _ = request.respond(Response::from_string("bad path").with_status_code(400));
            return;
        }
        let candidate = if rel.is_empty() {
            dir.join("index.html")
        } else {
            dir.join(rel)
        };
        let candidate = if candidate.is_file() {
            candidate
        } else {
            dir.join("index.html")
        };
        match std::fs::read(&candidate) {
            Ok(bytes) => {
                let resp = Response::from_data(bytes)
                    .with_header(header("Content-Type", content_type(&candidate)));
                let _ = request.respond(resp);
            }
            Err(_) => {
                let _ = request.respond(Response::from_string("not found").with_status_code(404));
            }
        }
        return;
    }

    let body = "buildscope API: /api/index, /api/report/<n>\n";
    let _ = request.respond(Response::from_string(body));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_literals_are_bracketed() {
        assert_eq!(url_authority("::1", 8380), "[::1]:8380");
        assert_eq!(url_authority("2001:db8::5", 80), "[2001:db8::5]:80");
        assert_eq!(url_authority("127.0.0.1", 8380), "127.0.0.1:8380");
        assert_eq!(url_authority("localhost", 8380), "localhost:8380");
    }

    #[test]
    fn wildcards_become_loopback_urls() {
        assert_eq!(
            browsable(&"0.0.0.0:8380".parse().unwrap()),
            "http://127.0.0.1:8380/"
        );
        assert_eq!(browsable(&"[::]:8380".parse().unwrap()), "http://[::1]:8380/");
        assert_eq!(browsable(&"[::1]:9000".parse().unwrap()), "http://[::1]:9000/");
        assert_eq!(
            browsable(&"10.0.0.5:9000".parse().unwrap()),
            "http://10.0.0.5:9000/"
        );
    }

    #[test]
    fn literal_specs_resolve_to_themselves() {
        assert_eq!(
            resolve("::1", 1234).unwrap(),
            vec!["[::1]:1234".parse::<SocketAddr>().unwrap()]
        );
        assert_eq!(
            resolve("127.0.0.1", 1234).unwrap(),
            vec!["127.0.0.1:1234".parse::<SocketAddr>().unwrap()]
        );
    }

    #[test]
    fn localhost_resolves_to_at_least_one_family() {
        let addrs = resolve("localhost", 1234).unwrap();
        assert!(!addrs.is_empty());
        assert!(addrs.iter().all(|a| a.port() == 1234));
    }
}
