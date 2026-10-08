//! `code-to-image`: render every code block to a PNG and link the PNG where the block was.

use std::fs;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;
use sha2::Digest;
use sha2::Sha256;

use super::ActionContext;
use crate::render::code::code_to_html;
use crate::render::code::load_theme;
use crate::render::code::CodeStyle;
use crate::render::code::DEFAULT_THEME;
use crate::render::code::PAGE_PADDING;

/// The width in pixels at which a block without a language wraps, unless `code-to-image=WIDTH`
/// gives another.
pub const DEFAULT_WIDTH: u32 = 1000;

/// The width in pixels at which a block with a language wraps, as in md2zhihu's
/// `block_code_to_fixwidth_jpg`.
const LANG_WIDTH: u32 = 600;

/// How many hex digits of the block's hash the file name keeps.
const HASH_LEN: usize = 12;

/// The language of a block that comrak renders as display math, not as code.
const MATH_LANG: &str = "math";

/// Replace every code block under `root` with a PNG that `ctx.renderer` renders into
/// `ctx.assets_dir`; a block without a language wraps at `width` pixels, one with a language at
/// `LANG_WIDTH`.
pub fn apply<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    width: u32,
    ctx: &ActionContext,
) -> anyhow::Result<()> {
    let theme = load_theme(DEFAULT_THEME)?;
    let plain_style = CodeStyle {
        theme: theme.clone(),
        width,
    };
    let lang_style = CodeStyle {
        theme,
        width: LANG_WIDTH,
    };

    replace_all_code_blocks(arena, root, |lang, code| {
        let style = if lang.is_some() {
            &lang_style
        } else {
            &plain_style
        };
        render(lang, code, style, ctx)
    })
}

/// Replace every code block under `root`, fenced or indented, with an image whose URL `image_url`
/// returns for the block's language and content; a ```` ```math ```` block is math and stays.
pub fn replace_all_code_blocks<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    mut image_url: impl FnMut(Option<&str>, &str) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    // Edits while walking would confuse the walk, so collect the blocks first.
    let blocks: Vec<(Node<'a>, Option<String>, String)> = root
        .descendants()
        .filter_map(|node| {
            let (lang, code) = code_of(node)?;
            Some((node, lang, code))
        })
        .collect();

    for (block, lang, code) in blocks {
        let url = image_url(lang.as_deref(), &code)?;
        let paragraph = super::image_paragraph(arena, url);
        block.insert_after(paragraph);
        block.detach();
    }
    Ok(())
}

/// The language, the first word of the info string, and the content of `node` when it is a code
/// block other than math.
fn code_of(node: Node<'_>) -> Option<(Option<String>, String)> {
    let ast = node.data();
    let NodeValue::CodeBlock(code) = &ast.value else {
        return None;
    };
    let lang = code.info.split_whitespace().next();
    if lang == Some(MATH_LANG) {
        return None;
    }
    let lang = lang.map(str::to_string);
    Some((lang, code.literal.clone()))
}

/// Render `code` with the colors of `lang` to a PNG in `ctx.assets_dir` and return the PNG's URL
/// relative to the output file.
fn render(
    lang: Option<&str>,
    code: &str,
    style: &CodeStyle,
    ctx: &ActionContext,
) -> anyhow::Result<String> {
    let page = code_to_html(lang, code, style)?;
    // A window narrower than the `<pre>` would wrap the lines before `style.width`.
    let renderer = ctx
        .renderer
        .with_window_width(style.width + 2 * PAGE_PADDING);
    let png = renderer.render_markup(&page)?;

    let markdown = format!("```{}\n{}```\n", lang.unwrap_or(""), code);
    let digest = Sha256::digest(markdown.as_bytes());
    let hash = format!("{digest:x}");
    let name = format!("{}-code-{}.png", ctx.stem, &hash[..HASH_LEN]);
    let path = ctx.assets_dir.join(name);
    fs::write(&path, png).with_context(|| format!("Failed to write image: {}", path.display()))?;

    super::relative_url(&ctx.output_dir, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code block, fenced or indented, also one inside a list, becomes a paragraph that holds
    /// one image; a ```` ```math ```` block is math and stays.
    #[test]
    fn test_replace_all_code_blocks() -> anyhow::Result<()> {
        let markdown = "```rust title\nfn main() {}\n```\n\n    indented\n\n\
                        ```math\nx^2\n```\n\n\
                        - ```\n  plain\n  ```\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut seen = Vec::new();
        replace_all_code_blocks(&arena, root, |lang, code| {
            seen.push((lang.map(str::to_string), code.to_string()));
            Ok(format!("c{}.png", seen.len()))
        })?;
        let expected_seen = vec![
            (Some("rust".to_string()), "fn main() {}\n".to_string()),
            (None, "indented\n".to_string()),
            (None, "plain\n".to_string()),
        ];
        assert_eq!(seen, expected_seen);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(
            out,
            "![](c1.png)\n\n![](c2.png)\n\n```math\nx^2\n```\n\n- ![](c3.png)\n"
        );
        Ok(())
    }
}
