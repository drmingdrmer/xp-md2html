//! `flatten-lists`: replace every list and block quote with the blocks it holds, as md2zhihu's
//! `weibo_specific` does for Weibo, which does not accept a `<p>` in an `<li>`.

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

/// Replace every list, list item and block quote under `root` with the blocks it holds, so each
/// paragraph in them becomes a plain paragraph. A task item keeps its checkbox as the text `[ ]` or
/// `[x]`, as md2zhihu's parser reads it.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>) {
    let nodes: Vec<Node<'a>> = root.descendants().collect();
    for node in nodes {
        match &node.data().value {
            NodeValue::List(_) | NodeValue::Item(_) | NodeValue::BlockQuote => {}
            NodeValue::TaskItem(task) => keep_checkbox(arena, node, task.symbol),
            _ => continue,
        }
        while let Some(child) = node.first_child() {
            node.insert_before(child);
        }
        node.detach();
    }
}

/// Put the checkbox of the task item `item` at the start of its first paragraph, or in a paragraph
/// of its own when the item starts with none, such as an empty item.
fn keep_checkbox<'a>(arena: &'a Arena<'a>, item: Node<'a>, symbol: Option<char>) {
    let symbol = symbol.unwrap_or(' ');
    let checkbox = format!("[{symbol}]");

    let first = item.first_child();
    let paragraph = first.filter(|node| matches!(node.data().value, NodeValue::Paragraph));
    if let Some(paragraph) = paragraph {
        let text = arena.alloc(NodeValue::Text(format!("{checkbox} ").into()).into());
        paragraph.prepend(text);
        return;
    }

    let text = arena.alloc(NodeValue::Text(checkbox.into()).into());
    let paragraph = arena.alloc(NodeValue::Paragraph.into());
    paragraph.append(text);
    item.prepend(paragraph);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each item and quote becomes its paragraphs, also a nested or a loose one; a task item keeps
    /// its checkbox, also when it is empty.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "- a\n  - b\n\n\
                        1. c\n\n   d\n\n\
                        - [x] e\n- [ ]\n\n\
                        > f\n>\n> > g\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(&arena, root);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "a\n\nb\n\nc\n\nd\n\n\\[x\\] e\n\n\\[ \\]\n\nf\n\ng\n";
        assert_eq!(out, expected);
        Ok(())
    }
}
