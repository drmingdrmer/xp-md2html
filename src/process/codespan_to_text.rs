//! `codespan-to-text`: replace every code span with its text, as md2zhihu's `to_plaintext` does for
//! Weibo, which shows no code style.

use comrak::nodes::NodeValue;
use comrak::Node;

/// Replace every `` `..` `` code span under `root` with its text. The printer escapes the text, so
/// `<b>` or `*a*` in it stays literal.
pub fn apply(root: Node<'_>) {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Code(code) = &ast.value else {
            continue;
        };
        let text = code.literal.clone();
        ast.value = NodeValue::Text(text.into());
    }
    merge_texts(root);
}

/// Join every text node under `root` to the text node before it. The printer escapes an `&` only
/// when a letter follows it in the same node, so the text `&` of a code span followed by the text
/// `amp;` would print as the entity `&amp;`.
fn merge_texts(root: Node<'_>) {
    let nodes: Vec<Node<'_>> = root.descendants().collect();
    for node in nodes {
        let ast = node.data();
        let NodeValue::Text(text) = &ast.value else {
            continue;
        };
        let Some(previous) = node.previous_sibling() else {
            continue;
        };
        let mut previous_ast = previous.data_mut();
        let NodeValue::Text(previous_text) = &mut previous_ast.value else {
            continue;
        };
        previous_text.to_mut().push_str(text);
        node.detach();
    }
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// A code span becomes text that renders as it did, also next to an `&` or in a table; a code
    /// block stays.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "Run `a <b> *c*` and `&`amp;.\n\n\
                        | `x \\| y` |\n|---|\n\n\
                        ```text\n`z`\n```\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(root);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "Run a \\<b\\> \\*c\\* and \\&amp;.\n\n\
                        | x \\| y |\n| --- |\n\n\
                        ```text\n`z`\n```\n";
        assert_eq!(out, expected);
        Ok(())
    }
}
