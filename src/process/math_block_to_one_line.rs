//! `math-block-to-one-line`: print every `$$` formula in a list item on one line, as md2zhihu's
//! `math_block_join_dolar_when_nested` does for github.com, which does not render a `$$` formula
//! that spans lines in a list item.

use comrak::nodes::NodeValue;
use comrak::Node;

/// Put every `$$..$$` formula in a list item under `root` on one line. A formula in a quote keeps
/// its lines: github.com renders it, and on one line it would lose the display style.
pub fn apply(root: Node<'_>) {
    for node in root.descendants() {
        let in_list_item = node.ancestors().any(is_list_item);
        if !in_list_item {
            continue;
        }
        let mut ast = node.data_mut();
        let NodeValue::Math(math) = &mut ast.value else {
            continue;
        };
        if !math.display_math {
            continue;
        }
        let tex = one_line(&math.literal);
        math.literal = tex;
    }
}

/// `tex` on one line: each line without its `%` comment and trimmed, joined with spaces. TeX reads
/// a line break as a space and ignores a comment, so the formula stays the same.
fn one_line(tex: &str) -> String {
    let mut parts = Vec::new();
    for line in tex.lines() {
        let code = strip_comment(line).trim();
        if !code.is_empty() {
            parts.push(code);
        }
    }
    parts.join(" ")
}

/// `line` up to its first `%` that is not escaped as `\%`.
fn strip_comment(line: &str) -> &str {
    let mut escaped = false;
    for (index, c) in line.char_indices() {
        if c == '%' && !escaped {
            return &line[..index];
        }
        escaped = c == '\\' && !escaped;
    }
    line
}

/// Whether `node` is a list item, also a `- [ ]` task item.
fn is_list_item(node: Node<'_>) -> bool {
    matches!(
        node.data().value,
        NodeValue::Item(_) | NodeValue::TaskItem(_)
    )
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// A `$$` formula in a list or task item goes on one line without its `%` comment, but keeps an
    /// escaped `\%`; inline math, a formula in a quote and a top-level formula stay.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "- $$\n  a = 1, % one\n  b = 2\n  $$\n\
                        - [ ] Task $$\n  c = 5\\%\n  $$ and $x$.\n\n\
                        > $$\n> d\n> $$\n\n\
                        $$\ne\n$$\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(root);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "- $$a = 1, b = 2$$\n\
                        - [ ] Task $$c = 5\\%$$ and $x$.\n\n\
                        > $$\n> d\n> $$\n\n\
                        $$\ne\n$$\n";
        assert_eq!(out, expected);
        Ok(())
    }

    /// A `%` starts a comment unless a `\` escapes it; `\\` is an escaped `\`, so a `%` after it
    /// starts a comment.
    #[test]
    fn test_strip_comment() {
        let comment = strip_comment("a % b");
        assert_eq!(comment, "a ");

        let escaped = strip_comment("5\\% b");
        assert_eq!(escaped, "5\\% b");

        let after_break = strip_comment("a \\\\% b");
        assert_eq!(after_break, "a \\\\");
    }
}
