//! `download-images`: download every `http(s)://` image into the assets dir and link the copy.

use std::fs;
use std::path::Path;

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

/// Download every `http(s)://` image under `root` into `ctx.assets_dir` and link the copy.
pub fn apply(root: Node<'_>, ctx: &ActionContext) -> anyhow::Result<()> {
    relink_remote_images(root, |url| download(url, &ctx.assets_dir, &ctx.output_dir))
}

/// Replace the URL of every `http(s)://` image under `root` with what `local_url` returns for it.
pub fn relink_remote_images(
    root: Node<'_>,
    mut local_url: impl FnMut(&str) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Image(link) = &mut ast.value else {
            continue;
        };
        if !is_remote(&link.url) {
            continue;
        }
        link.url = local_url(&link.url)?;
    }
    Ok(())
}

fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Download `url` into `assets_dir`, unless its file is already there, and return the file's URL
/// relative to the output file.
///
/// The file name hashes the URL, not the content, so a second run skips the download.
fn download(url: &str, assets_dir: &Path, output_dir: &Path) -> anyhow::Result<String> {
    let path = assets_dir.join(file_name(url));
    if !path.exists() {
        let body = fetch(url)?;
        fs::write(&path, body)
            .with_context(|| format!("Failed to write image: {}", path.display()))?;
    }
    super::relative_url(output_dir, &path)
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

    /// Every `http(s)://` image is relinked, also one inside a link; a local image is left alone.
    #[test]
    fn test_relink_remote_images() -> anyhow::Result<()> {
        let markdown =
            "![a](http://h/a.png) ![b](local/b.png)\n\n[![c](https://h/c.png)](https://h/)\n";
        let options = super::super::gfm_math_options();
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut seen = Vec::new();
        relink_remote_images(root, |url| {
            seen.push(url.to_string());
            Ok(format!("assets/img{}.png", seen.len()))
        })?;

        let expected_seen = vec!["http://h/a.png".to_string(), "https://h/c.png".to_string()];
        assert_eq!(seen, expected_seen);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected =
            "![a](assets/img1.png) ![b](local/b.png)\n\n[![c](assets/img2.png)](https://h/)\n";
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

        let link = download(&url, &assets_dir, dir.path())?;
        assert_eq!(link, format!("assets/{name}"));
        let content = fs::read(assets_dir.join(&name))?;
        assert_eq!(content, b"PNGDATA");

        // The server is gone, so this call succeeds only if it skips the download.
        let link = download(&url, &assets_dir, dir.path())?;
        assert_eq!(link, format!("assets/{name}"));
        Ok(())
    }

    #[test]
    fn test_download_status_error() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;

        let port = serve_once("HTTP/1.1 404 Not Found", b"")?;
        let url = format!("http://127.0.0.1:{port}/missing.png");

        let result = download(&url, dir.path(), dir.path());
        let message = format!("{:#}", result.unwrap_err());
        assert_eq!(
            message,
            format!("Failed to download: {url}: http status: 404")
        );
        assert!(!dir.path().join(file_name(&url)).exists());
        Ok(())
    }
}
