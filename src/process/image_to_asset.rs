//! `image-to-asset`: copy every local image into the assets dir and link the copy.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Node;
use sha2::Digest;
use sha2::Sha256;

use super::embed_markdown;
use super::ActionContext;

/// How many hex digits of the content's hash the file name keeps.
const HASH_LEN: usize = 12;

/// Copy every local image under `root` into `ctx.assets_dir` and link the copy; the link to an
/// asset, a file that an action created or copied, keeps its URL.
pub fn apply(root: Node<'_>, ctx: &ActionContext) -> anyhow::Result<()> {
    relink_local_images(root, |url| {
        if ctx.is_asset(url) {
            return Ok(url.to_string());
        }
        let path = copy(url, &ctx.input_dir, &ctx.assets_dir)?;
        ctx.link_to(&path)
    })
}

/// Replace the URL of every local image under `root` with what `asset_url` returns for it.
pub fn relink_local_images(
    root: Node<'_>,
    mut asset_url: impl FnMut(&str) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Image(link) = &mut ast.value else {
            continue;
        };
        if !is_local(&link.url) {
            continue;
        }
        link.url = asset_url(&link.url)?;
    }
    Ok(())
}

/// A URL that names a file: a relative path, or a root path `/x`; not a scheme URL, a
/// protocol-relative URL `//x` or an anchor.
fn is_local(url: &str) -> bool {
    embed_markdown::is_root_path(url) || embed_markdown::is_relative(url)
}

/// Copy the image at `url`, which resolves against `input_dir`, into `assets_dir` under a name
/// that hashes its content, and return the copy's path.
fn copy(url: &str, input_dir: &Path, assets_dir: &Path) -> anyhow::Result<PathBuf> {
    let source = embed_markdown::resolve(input_dir, input_dir, url);
    let content =
        fs::read(&source).with_context(|| format!("Failed to read image: {}", source.display()))?;
    let path = assets_dir.join(file_name(url, &content));
    fs::write(&path, &content)
        .with_context(|| format!("Failed to write image: {}", path.display()))?;
    Ok(path)
}

/// `<sha256(content)[..12]>-<basename>`, where the basename is the last `/` segment of `url`.
fn file_name(url: &str, content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    let hex = format!("{digest:x}");
    let hash = &hex[..HASH_LEN];
    let basename = url.rsplit('/').next().unwrap_or("");
    format!("{hash}-{basename}")
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    #[test]
    fn test_relink_local_images() -> anyhow::Result<()> {
        let markdown = "![a](img/a.png) ![r](/r.png) ![h](https://x.io/h.png) ![p](//x.io/p.png) ![d](data:image/png;base64,AA==) [l](l.png)\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut seen = Vec::new();
        relink_local_images(root, |url| {
            seen.push(url.to_string());
            Ok(format!("assets/{}", url.trim_start_matches('/')))
        })?;
        assert_eq!(seen, ["img/a.png", "/r.png"]);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(
            out,
            "![a](assets/img/a.png) ![r](assets/r.png) ![h](https://x.io/h.png) ![p](//x.io/p.png) ![d](data:image/png;base64,AA==) [l](l.png)\n"
        );
        Ok(())
    }

    #[test]
    fn test_copy() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let input_dir = dir.path().join("posts");
        let assets_dir = dir.path().join("out/assets");
        fs::create_dir_all(input_dir.join("img"))?;
        fs::create_dir_all(&assets_dir)?;
        fs::write(input_dir.join("img/a.png"), b"PNG-A")?;
        fs::write(input_dir.join("r.png"), b"PNG-R")?;

        let path = copy("img/a.png", &input_dir, &assets_dir)?;
        assert_eq!(path, assets_dir.join("e793c41f42c3-a.png"));
        let copied = fs::read(&path)?;
        assert_eq!(copied, b"PNG-A");

        // `/r.png` resolves against the input directory, not the file system root.
        let path = copy("/r.png", &input_dir, &assets_dir)?;
        assert_eq!(path, assets_dir.join("0ced803886f0-r.png"));
        let copied = fs::read(&path)?;
        assert_eq!(copied, b"PNG-R");

        let error = copy("img/missing.png", &input_dir, &assets_dir).unwrap_err();
        let expected = format!(
            "Failed to read image: {}",
            input_dir.join("img/missing.png").display()
        );
        assert_eq!(error.to_string(), expected);
        Ok(())
    }

    #[test]
    fn test_file_name() {
        let name = file_name("img/a.png", b"PNG-A");
        assert_eq!(name, "e793c41f42c3-a.png");

        let name = file_name("/r.png", b"PNG-R");
        assert_eq!(name, "0ced803886f0-r.png");
    }
}
