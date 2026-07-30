//! Minimal local HTTP server: same-origin API for the viewer plus static
//! files from the built viewer bundle.

use std::path::{Path, PathBuf};
use tiny_http::{Header, Response, Server};

pub struct ReportEntry {
    pub name: String,
    pub json: String,
}

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
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header")
}

pub fn serve(bind: &str, port: u16, reports: Vec<ReportEntry>, viewer_dir: Option<PathBuf>) -> ! {
    let addr = format!("{bind}:{port}");
    let server = Server::http(&addr).unwrap_or_else(|e| {
        eprintln!("buildscope: cannot bind {addr}: {e}");
        std::process::exit(1);
    });
    println!("buildscope: serving {} report(s) at http://{addr}/", reports.len());
    if viewer_dir.is_none() {
        println!("buildscope: no viewer bundle found; API only (/api/index, /api/report/<n>)");
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

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("/");

        let respond_json = |req: tiny_http::Request, body: &str| {
            let resp = Response::from_string(body)
                .with_header(header("Content-Type", "application/json"));
            let _ = req.respond(resp);
        };

        if path == "/api/index" {
            respond_json(request, &index_json);
            continue;
        }
        if let Some(idx) = path.strip_prefix("/api/report/") {
            if let Ok(i) = idx.parse::<usize>() {
                if let Some(r) = reports.get(i) {
                    respond_json(request, &r.json);
                    continue;
                }
            }
            let _ = request.respond(Response::from_string("not found").with_status_code(404));
            continue;
        }

        // Static viewer files.
        if let Some(dir) = &viewer_dir {
            let rel = path.trim_start_matches('/');
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
                    let _ = request
                        .respond(Response::from_string("not found").with_status_code(404));
                }
            }
            continue;
        }

        let body = "buildscope API: /api/index, /api/report/<n>\n";
        let _ = request.respond(Response::from_string(body));
    }
    unreachable!("server loop ended");
}
