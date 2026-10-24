//! `math-to-hugo`: print every formula on one line between the math delimiters of Hugo's passthrough
//! extension, as md2zhihu's `math_block_to_hugo` and `math_inline_to_hugo` do for its platform
//! `hugo`. Hugo reads `\[x\]` as display math and `$$x$$` as inline math when the passthrough sets
//! them as its block and inline delimiters. A formula on several lines would break there: the
//! passthrough reads no math across a blank line, and in a quote it keeps the `>` of each line.

use comrak::nodes::NodeValue;
use comrak::Node;

use super::math_block_to_one_line::one_line;

/// Print every `$..$` and `$$..$$` formula under `root` on one line: between `\[` and `\]` when it
/// is the only node of its paragraph, also of the paragraph of a list item, and between `$$` and
/// `$$` anywhere else, such as in a sentence, a link or a heading.
pub fn apply(root: Node<'_>) {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Math(math) = &mut ast.value else {
            continue;
        };
        let tex = one_line(&math.literal);
        if !alone_in_paragraph(node) {
            math.dollar_math = true;
            math.display_math = true;
            math.literal = tex;
            continue;
        }
        // comrak prints a formula only between `$` or `$$`, and a raw node prints as it is.
        ast.value = NodeValue::Raw(format!("\\[{tex}\\]"));
    }
}

/// Whether `node` is the only node of its paragraph.
fn alone_in_paragraph(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let in_paragraph = matches!(parent.data().value, NodeValue::Paragraph);
    let only_child = node.previous_sibling().is_none() && node.next_sibling().is_none();
    in_paragraph && only_child
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// A formula alone in its paragraph, also `$x$` and a formula in a list item or a quote, goes
    /// between `\[` and `\]`; a formula in a sentence or a heading between `$$` and `$$`. Each goes
    /// on one line without its `%` comment.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "$$\na = 1, % one\nb = 2\n$$\n\n\
                        - $$c$$\n\n\
                        > $$\n> d\n> $$\n\n\
                        $$e$$\nis $f$, not $$g\nh$$.\n\n\
                        $i$\n\n\
                        # Title $$j$$\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(root);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "\\[a = 1, b = 2\\]\n\n\
                        - \\[c\\]\n\n\
                        > \\[d\\]\n\n\
                        $$e$$\nis $$f$$, not $$g h$$.\n\n\
                        \\[i\\]\n\n\
                        # Title $$j$$\n";
        assert_eq!(out, expected);
        Ok(())
    }
}
