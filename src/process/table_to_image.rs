//! `table-to-image`: render every table to a PNG and link the PNG where the table was.

use std::collections::HashMap;
use std::fs;
use std::sync::Arc;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;
use sha2::Digest;
use sha2::Sha256;

use super::ActionContext;
use crate::render::chrome::ChromeRenderer;
use crate::render::markdown::html_options;

/// How many hex digits of the table's hash the file name keeps.
const HASH_LEN: usize = 12;

/// The page around one table: GitHub's table style on a transparent page, which the trim cuts down to the table.
const PAGE_HEAD: &str = r#"<!DOCTYPE html>
<html><head><style>
body { margin: 0; padding: 8px; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif; font-size: 16px; line-height: 1.5; color: #1f2328; }
table { border-collapse: collapse; border-spacing: 0; }
th, td { border: 1px solid #d1d9e0; padding: 6px 13px; }
th { font-weight: 600; }
tr { background-color: #ffffff; }
tr:nth-child(2n) { background-color: #f6f8fa; }
</style></head><body>
"#;
const PAGE_TAIL: &str = "</body></html>\n";

/// Replace every table under `root` with a PNG that `ctx.renderer` renders into `ctx.assets_dir`.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>, ctx: &ActionContext) -> anyhow::Result<()> {
    replace_tables(arena, root, |table| render(table, ctx))
}

/// Replace every table under `root` with an image whose URL `image_url` returns for the table.
pub fn replace_tables<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    mut image_url: impl FnMut(Node<'a>) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    // Edits while walking would confuse the walk, so collect the tables first.
    let tables: Vec<Node<'a>> = root.descendants().filter(|node| is_table(node)).collect();

    for table in tables {
        let url = image_url(table)?;
        let paragraph = super::image_paragraph(arena, url);
        table.insert_after(paragraph);
        table.detach();
    }
    Ok(())
}

pub(super) fn is_table(node: Node<'_>) -> bool {
    matches!(node.data().value, NodeValue::Table(_))
}

/// Render `table` to a PNG in `ctx.assets_dir` and return the link to the PNG.
fn render(table: Node<'_>, ctx: &ActionContext) -> anyhow::Result<String> {
    let mut options = html_options();

    let mut markdown = String::new();
    comrak::format_commonmark(table, &options, &mut markdown)?;

    // Chrome loads the page from a temporary directory: a relative URL there names no file, a URL
    // under the URL base names a file that is not online yet, and a protocol-relative URL `//x`
    // becomes `file://x`.
    let file_urls = file_urls(table, ctx)?;
    let rewrite = move |url: &str| match file_urls.get(url) {
        Some(file_url) => file_url.clone(),
        None => super::with_scheme(url),
    };
    options.extension.image_url_rewriter = Some(Arc::new(rewrite));
    let mut html = String::new();
    comrak::format_html(table, &options, &mut html)?;

    let page = format!("{PAGE_HEAD}{html}{PAGE_TAIL}");
    let png = ctx.renderer.render_markup(&page)?;

    let digest = Sha256::digest(markdown.as_bytes());
    let hash = format!("{digest:x}");
    let name = format!("{}-table-{}.png", ctx.stem, &hash[..HASH_LEN]);
    let path = ctx.assets_dir.join(name);
    fs::write(&path, png).with_context(|| format!("Failed to write image: {}", path.display()))?;

    ctx.link_to(&path)
}

/// The `file://` URL of the local file that each image under `table` names, by the image's URL.
fn file_urls(table: Node<'_>, ctx: &ActionContext) -> anyhow::Result<HashMap<String, String>> {
    let mut urls = HashMap::new();
    for node in table.descendants() {
        let data = node.data();
        let NodeValue::Image(link) = &data.value else {
            continue;
        };
        let Some(file) = ctx.local_file(&link.url) else {
            continue;
        };
        let file_url = ChromeRenderer::file_url(&file)?;
        urls.insert(link.url.clone(), file_url);
    }
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every table, also one inside another block, becomes a paragraph that holds one image.
    #[test]
    fn test_replace_tables() -> anyhow::Result<()> {
        let markdown =
            "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nText.\n\n> | c |\n> |---|\n> | 3 |\n";
        let options = super::super::gfm_math_options();
        let arena = Arena::new();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut seen = Vec::new();
        replace_tables(&arena, root, |table| {
            let mut table_markdown = String::new();
            comrak::format_commonmark(table, &options, &mut table_markdown)?;
            seen.push(table_markdown);
            Ok(format!("img{}.png", seen.len()))
        })?;

        let expected_seen = vec![
            "| a | b |\n| --- | --- |\n| 1 | 2 |\n".to_string(),
            "| c |\n| --- |\n| 3 |\n".to_string(),
        ];
        assert_eq!(seen, expected_seen);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, "# T\n\n![](img1.png)\n\nText.\n\n> ![](img2.png)\n");
        Ok(())
    }
}
