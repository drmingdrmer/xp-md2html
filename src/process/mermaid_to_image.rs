//! `mermaid-to-image`: render every ```` ```mermaid ```` block to a PNG and link the PNG where the block was.

use std::fs;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;
use sha2::Digest;
use sha2::Sha256;

use super::ActionContext;
use crate::render::chrome::ChromeRenderer;
use crate::render::mermaid::mermaid_to_svg;
use crate::render::page::svg_to_image;

/// How many hex digits of the source's hash the file name keeps.
const HASH_LEN: usize = 12;

/// Replace every ```` ```mermaid ```` block under `root` with a PNG that `ctx.renderer` renders into
/// `ctx.assets_dir`.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>, ctx: &ActionContext) -> anyhow::Result<()> {
    replace_code_blocks(arena, root, "mermaid", |source| {
        render(source, "mermaid", mermaid_to_svg, ctx)
    })
}

/// Replace every fenced code block under `root` whose language is `lang` with an image whose URL
/// `image_url` returns for the block's content.
pub fn replace_code_blocks<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    lang: &str,
    mut image_url: impl FnMut(&str) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    // Edits while walking would confuse the walk, so collect the blocks first.
    let blocks: Vec<(Node<'a>, String)> = root
        .descendants()
        .filter_map(|node| {
            let source = source_of(node, lang)?;
            Some((node, source))
        })
        .collect();

    for (block, source) in blocks {
        let url = image_url(&source)?;
        let paragraph = super::image_paragraph(arena, url);
        block.insert_after(paragraph);
        block.detach();
    }
    Ok(())
}

/// The content of `node` when it is a fenced code block whose info string starts with `lang`.
fn source_of(node: Node<'_>, lang: &str) -> Option<String> {
    let ast = node.data();
    let NodeValue::CodeBlock(code) = &ast.value else {
        return None;
    };
    let language = code.info.split_whitespace().next().unwrap_or("");
    let is_match = code.fenced && language == lang;
    if !is_match {
        return None;
    }
    Some(code.literal.clone())
}

/// Render the diagram `source` with `to_svg` to a PNG in `ctx.assets_dir` and return the link to
/// the PNG; the PNG is named `<stem>-<lang>-<hash of source>.png`.
pub(crate) fn render(
    source: &str,
    lang: &str,
    to_svg: fn(&ChromeRenderer, &str) -> anyhow::Result<String>,
    ctx: &ActionContext,
) -> anyhow::Result<String> {
    let svg = to_svg(&ctx.renderer, source)?;
    let png = svg_to_image(&ctx.renderer, &svg)?;

    let digest = Sha256::digest(source.as_bytes());
    let hash = format!("{digest:x}");
    let name = format!("{}-{}-{}.png", ctx.stem, lang, &hash[..HASH_LEN]);
    let path = ctx.assets_dir.join(name);
    fs::write(&path, png).with_context(|| format!("Failed to write image: {}", path.display()))?;

    ctx.link_to(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_code_blocks() -> anyhow::Result<()> {
        let markdown = "Before.\n\n\
            ```mermaid\n\
            graph LR\n  a --> b\n\
            ```\n\n\
            ```rust\n\
            fn main() {}\n\
            ```\n\n\
            ~~~mermaid title\n\
            pie\n\
            ~~~\n\n\
            \x20   indented\n\n\
            After.\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut sources = Vec::new();
        replace_code_blocks(&arena, root, "mermaid", |source| {
            sources.push(source.to_string());
            Ok(format!("assets/d{}.png", sources.len()))
        })?;
        assert_eq!(sources, ["graph LR\n  a --> b\n", "pie\n"]);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(
            out,
            "Before.\n\n\
             ![](assets/d1.png)\n\n\
             ```rust\n\
             fn main() {}\n\
             ```\n\n\
             ![](assets/d2.png)\n\n\
             \x20   indented\n\n\
             After.\n"
        );
        Ok(())
    }
}
