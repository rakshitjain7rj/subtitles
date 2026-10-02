//! A loopback-only HTTP server for preview videos. Webviews (WebKitGTK in
//! particular) can't stream media through Tauri's custom protocols, but they
//! all play `http://127.0.0.1` with range requests.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use tiny_http::{Header, Request, Response, Server, StatusCode};

use crate::error::{msg, Result};
use crate::project::{valid_id, PREVIEW_FILE};

pub struct PreviewServer {
    port: u16,
    /// Random per launch, so other local programs can't guess preview URLs.
    token: String,
}

impl PreviewServer {
    pub fn start(projects_root: PathBuf) -> Result<PreviewServer> {
        let server =
            Server::http("127.0.0.1:0").map_err(|e| msg(format!("Could not start the preview server: {e}")))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| msg("Preview server has no port."))?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let expected = token.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let root = projects_root.clone();
                let token = expected.clone();
                std::thread::spawn(move || serve(request, &root, &token));
            }
        });
        Ok(PreviewServer { port, token })
    }

    /// `version` changes when the preview is regenerated, to defeat caching.
    pub fn url(&self, project_id: &str, version: u64) -> String {
        format!(
            "http://127.0.0.1:{}/{}/{}/{PREVIEW_FILE}?v={version}",
            self.port, self.token, project_id
        )
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("static header is valid")
}

/// Parses `Range: bytes=a-b` against a file of `len` bytes into an inclusive range.
pub fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    let spec = value.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    if len == 0 || spec.contains(',') {
        return None;
    }
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (start, "") => (start.parse().ok()?, len - 1),
        (start, end) => (start.parse().ok()?, end.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

fn resolve(url: &str, root: &std::path::Path, token: &str) -> Option<PathBuf> {
    let path = url.split('?').next()?;
    let mut parts = path.trim_start_matches('/').split('/');
    let (tok, id, file) = (parts.next()?, parts.next()?, parts.next()?);
    (parts.next().is_none() && tok == token && valid_id(id) && file == PREVIEW_FILE).then(|| root.join(id).join(file))
}

fn serve(request: Request, root: &std::path::Path, token: &str) {
    let Some(path) = resolve(request.url(), root, token) else {
        let _ = request.respond(Response::empty(StatusCode(404)));
        return;
    };
    let Ok(mut file) = File::open(&path) else {
        let _ = request.respond(Response::empty(StatusCode(404)));
        return;
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let range = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_string());

    let common = [
        header("Content-Type", "video/mp4"),
        header("Accept-Ranges", "bytes"),
        header("Cache-Control", "no-store"),
    ];
    let result = match range {
        Some(value) => match parse_range(&value, len) {
            Some((start, end)) => {
                if file.seek(SeekFrom::Start(start)).is_err() {
                    let _ = request.respond(Response::empty(StatusCode(500)));
                    return;
                }
                let count = end - start + 1;
                let mut headers = common.to_vec();
                headers.push(header("Content-Range", &format!("bytes {start}-{end}/{len}")));
                request.respond(Response::new(
                    StatusCode(206),
                    headers,
                    file.take(count),
                    Some(count as usize),
                    None,
                ))
            }
            None => {
                let headers = vec![header("Content-Range", &format!("bytes */{len}"))];
                request.respond(Response::new(StatusCode(416), headers, std::io::empty(), Some(0), None))
            }
        },
        None => request.respond(Response::new(
            StatusCode(200),
            common.to_vec(),
            file,
            Some(len as usize),
            None,
        )),
    };
    // The player closing the connection mid-stream is normal.
    let _ = result;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serves_byte_ranges_over_loopback() {
        let root = std::env::temp_dir().join(format!("subtitles-preview-{}", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        std::fs::create_dir_all(root.join(&id)).unwrap();
        std::fs::write(root.join(&id).join(PREVIEW_FILE), b"0123456789").unwrap();
        let server = PreviewServer::start(root.clone()).unwrap();
        let url = server.url(&id, 1);
        assert!(url.starts_with("http://127.0.0.1:"));

        let client = reqwest::Client::new();
        let part = client.get(&url).header("Range", "bytes=2-5").send().await.unwrap();
        assert_eq!(part.status(), 206);
        assert_eq!(part.headers()["content-range"], "bytes 2-5/10");
        assert_eq!(part.text().await.unwrap(), "2345");

        let whole = client.get(&url).send().await.unwrap();
        assert_eq!(whole.status(), 200);
        assert_eq!(whole.text().await.unwrap(), "0123456789");

        let wrong_token = url.replace(&server.token, "nope");
        assert_eq!(client.get(&wrong_token).send().await.unwrap().status(), 404);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=900-5000", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=5-2", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }

    #[test]
    fn only_preview_files_with_the_token_resolve() {
        let root = std::path::Path::new("/p");
        let id = "3f2b8c1e-6a4d-4e0b-9c57-0d2f8a1b7e55";
        assert_eq!(
            resolve(&format!("/tok/{id}/preview.mp4?v=3"), root, "tok"),
            Some(root.join(id).join("preview.mp4"))
        );
        assert_eq!(resolve(&format!("/bad/{id}/preview.mp4"), root, "tok"), None);
        assert_eq!(resolve(&format!("/tok/{id}/project.json"), root, "tok"), None);
        assert_eq!(resolve("/tok/../preview.mp4", root, "tok"), None);
    }
}
