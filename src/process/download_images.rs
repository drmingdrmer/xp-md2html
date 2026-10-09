//! `download-images`: download every `http(s)://` image into the assets dir and link the copy.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Node;
use sha2::Digest;
use sha2::Sha256;

use super::ActionContext;

/// How many hex digits of the URL's hash the file name keeps.
const HASH_LEN: usize = 12;

/// The largest response body a download accepts; `ureq` stops reading at this size.
const MAX_IMAGE_BYTES: u64 = 100 * 1024 * 1024;

/// Download every `http(s)://` image under `root` into `ctx.assets_dir` and link the copy; an image
/// under `ctx.url_base` is a file that an action created, so it keeps its URL.
pub fn apply(root: Node<'_>, ctx: &ActionContext) -> anyhow::Result<()> {
    relink_remote_images(root, |url| {
        if ctx.is_under_url_base(url) {
            return Ok(url.to_string());
        }
        let path = download(url, &ctx.assets_dir)?;
        ctx.link_to(&path)
    })
}

/// Replace the URL of every `http(s)://` image under `root`, also of a protocol-relative one `//x`,
/// with what `local_url` returns for its `http(s)://` URL.
pub fn relink_remote_images(
    root: Node<'_>,
    mut local_url: impl FnMut(&str) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Image(link) = &mut ast.value else {
            continue;
        };
        let url = super::with_scheme(&link.url);
        if !is_remote(&url) {
            continue;
        }
        link.url = local_url(&url)?;
    }
    Ok(())
}

fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Download `url` into `assets_dir`, unless its file is already there, and return the file's path.
///
/// The file name hashes the URL, not the content, so a second run skips the download.
fn download(url: &str, assets_dir: &Path) -> anyhow::Result<PathBuf> {
    let path = assets_dir.join(file_name(url));
    if !path.exists() {
        let body = fetch(url)?;
        fs::write(&path, body)
            .with_context(|| format!("Failed to write image: {}", path.display()))?;
    }
    Ok(path)
}

/// `<sha256(url)[..12]>-<basename>`, where the basename is the last `/` segment of the URL
/// without `?...` and `#...`; the hash alone when the basename is empty.
fn file_name(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let hex = format!("{digest:x}");
    let hash = &hex[..HASH_LEN];

    let path = url.split(['?', '#']).next().unwrap_or("");
    let basename = path.rsplit('/').next().unwrap_or("");
    if basename.is_empty() {
        return hash.to_string();
    }
    format!("{hash}-{basename}")
}

/// Return the body that `GET url` answers with; a status outside 2xx is an error.
fn fetch(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut response = ureq::get(url)
        .call()
        .with_context(|| format!("Failed to download: {url}"))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_IMAGE_BYTES)
        .read_to_vec()
        .with_context(|| format!("Failed to read the download: {url}"))?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    /// Every `http(s)://` image is relinked, also one inside a link, and a protocol-relative one
    /// `//x` by its `https://` URL; a local image is left alone.
    #[test]
    fn test_relink_remote_images() -> anyhow::Result<()> {
        let markdown = "![a](http://h/a.png) ![b](local/b.png) ![d](//h/d.png)\n\n\
                        [![c](https://h/c.png)](https://h/)\n";
        let options = super::super::gfm_math_options();
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut seen = Vec::new();
        relink_remote_images(root, |url| {
            seen.push(url.to_string());
            Ok(format!("assets/img{}.png", seen.len()))
        })?;

        let expected_seen = vec![
            "http://h/a.png".to_string(),
            "https://h/d.png".to_string(),
            "https://h/c.png".to_string(),
        ];
        assert_eq!(seen, expected_seen);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "![a](assets/img1.png) ![b](local/b.png) ![d](assets/img2.png)\n\n\
                        [![c](assets/img3.png)](https://h/)\n";
        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_file_name() {
        // The hashes are `printf '<url>' | shasum -a 256 | cut -c1-12`.
        let name = file_name("https://h/img/a.png?x=1#top");
        assert_eq!(name, "2b402ffe39e1-a.png");

        let name = file_name("https://h/img/");
        assert_eq!(name, "8ad5e0279553");
    }

    /// Answer the first connection with `status_line` and `body`, then close the listener.
    fn serve_once(status_line: &'static str, body: &'static [u8]) -> anyhow::Result<u16> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.ends_with(b"\r\n\r\n") {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
            }
            let head = format!(
                "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        Ok(port)
    }

    /// The first download writes the file; the second finds it and does not connect.
    #[test]
    fn test_download() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let assets_dir = dir.path().join("assets");
        fs::create_dir(&assets_dir)?;

        let port = serve_once("HTTP/1.1 200 OK", b"PNGDATA")?;
        let url = format!("http://127.0.0.1:{port}/img/a.png?x=1");
        let name = file_name(&url);

        let path = download(&url, &assets_dir)?;
        assert_eq!(path, assets_dir.join(&name));
        let content = fs::read(&path)?;
        assert_eq!(content, b"PNGDATA");

        // The server is gone, so this call succeeds only if it skips the download.
        let path = download(&url, &assets_dir)?;
        assert_eq!(path, assets_dir.join(&name));
        Ok(())
    }

    #[test]
    fn test_download_status_error() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;

        let port = serve_once("HTTP/1.1 404 Not Found", b"")?;
        let url = format!("http://127.0.0.1:{port}/missing.png");

        let result = download(&url, dir.path());
        let message = format!("{:#}", result.unwrap_err());
        assert_eq!(
            message,
            format!("Failed to download: {url}: http status: 404")
        );
        assert!(!dir.path().join(file_name(&url)).exists());
        Ok(())
    }
}
