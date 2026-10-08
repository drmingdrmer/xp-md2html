//! `math-to-img-tag=SERVICE`: replace every formula with the `<img>` tag of an online formula service,
//! so that a page that cannot run MathJax shows the formula as an image.

use comrak::nodes::NodeHtmlBlock;
use comrak::nodes::NodeValue;
use comrak::Node;

use super::code_to_image::MATH_LANG;
use crate::render::math_img::math_img_tag;
use crate::render::math_img::MathService;

/// The CommonMark kind of an HTML block that starts with a tag that is not a block-level tag, such
/// as `<img>`.
const HTML_BLOCK_TYPE_OTHER_TAG: u8 = 7;

/// Replace every `$..$` and `$$..$$` formula under `root` with an inline `<img>` tag of `service`
/// and every ```` ```math ```` block with an HTML block that holds one, as md2zhihu's
/// `math_inline_to_imgtag` and `math_block_to_imgtag` do.
pub fn apply(root: Node<'_>, service: MathService) {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let Some(html) = img_html(&ast.value, service) else {
            continue;
        };
        ast.value = html;
    }
}

/// The raw HTML that replaces `value` when it is a formula or a ```` ```math ```` block.
fn img_html(value: &NodeValue, service: MathService) -> Option<NodeValue> {
    match value {
        NodeValue::Math(math) => {
            let tag = math_img_tag(service, &math.literal, math.display_math);
            Some(NodeValue::HtmlInline(tag))
        }
        NodeValue::CodeBlock(code) => {
            let lang = code.info.split_whitespace().next();
            if lang != Some(MATH_LANG) {
                return None;
            }
            let tag = math_img_tag(service, &code.literal, true);
            let block = NodeHtmlBlock {
                block_type: HTML_BLOCK_TYPE_OTHER_TAG,
                literal: format!("{tag}\n"),
            };
            Some(NodeValue::HtmlBlock(block))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// Inline math, also in code syntax, becomes an inline tag; a display formula and a
    /// ```` ```math ```` block become a display tag; a code block in another language stays.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "Inline $x$ and $`y`$.\n\n$$\nz\n$$\n\n\
                        ```math\nw\n```\n\n```text\nv\n```\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(root, MathService::Codecogs);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = format!(
            "Inline {} and {}.\n\n{}\n\n{}\n\n```text\nv\n```\n",
            math_img_tag(MathService::Codecogs, "x", false),
            math_img_tag(MathService::Codecogs, "y", false),
            math_img_tag(MathService::Codecogs, "z", true),
            math_img_tag(MathService::Codecogs, "w", true),
        );
        assert_eq!(out, expected);
        Ok(())
    }
}
