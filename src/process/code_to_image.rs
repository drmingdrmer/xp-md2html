//! `code-to-image`: render every code block to a PNG and link the PNG where the block was.

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

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
pub const LANG_WIDTH: u32 = 600;

/// Wrap widths for code images, in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeWidths {
    /// The width of code with a language.
    pub language: u32,
    /// The width of code without a language.
    pub plain: u32,
}

impl Default for CodeWidths {
    fn default() -> Self {
        Self {
            language: LANG_WIDTH,
            plain: DEFAULT_WIDTH,
        }
    }
}

/// The language of a block that comrak renders as display math, not as code.
pub(crate) const MATH_LANG: &str = "math";

/// Replace every code block under `root` with a PNG that `ctx.renderer` renders into
/// `ctx.assets_dir`, using the wrap width for its language or plain text.
pub fn apply<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    widths: CodeWidths,
    ctx: &ActionContext,
) -> anyhow::Result<()> {
    let theme = load_theme(DEFAULT_THEME)?;
    let plain_style = CodeStyle {
        theme: theme.clone(),
        width: widths.plain,
    };
    let lang_style = CodeStyle {
        theme,
        width: widths.language,
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

/// Render `code` with the colors of `lang` to a PNG in `ctx.assets_dir` and return the link to the
/// PNG.
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
        .get()?
        .with_window_width(style.width.saturating_add(2 * PAGE_PADDING));

    // The same block at another width is another PNG.
    let key = format!("{}\n```{}\n{}```\n", style.width, lang.unwrap_or(""), code);
    ctx.render_once("code", &key, || renderer.render_markup(&page))
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
