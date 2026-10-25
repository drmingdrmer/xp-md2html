//! Preserve block boundaries when CommonMark formatting suppresses blank lines in lists.

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

pub(super) fn prepare<'a>(arena: &'a Arena<'a>, root: Node<'a>) {
    let nodes: Vec<_> = root.descendants().collect();
    for node in nodes {
        let in_item = node.ancestors().any(|parent| {
            matches!(
                parent.data().value,
                NodeValue::Item(_) | NodeValue::TaskItem(_)
            )
        });
        if !in_item {
            continue;
        }
        if matches!(node.data().value, NodeValue::ThematicBreak) {
            // Hyphens can become a setext underline or merge with a '-' list marker.
            node.data_mut().value = NodeValue::Paragraph;
            let marker = arena.alloc(NodeValue::Raw("***".into()).into());
            node.append(marker);
        } else if needs_quote_boundary(node) {
            // A quoted empty line ends lazy continuation without loosening the list.
            let paragraph = arena.alloc(NodeValue::Paragraph.into());
            let newline = arena.alloc(NodeValue::Raw("\n".into()).into());
            paragraph.append(newline);
            node.append(paragraph);
        }
    }
}

fn needs_quote_boundary(node: Node<'_>) -> bool {
    let quote = matches!(node.data().value, NodeValue::BlockQuote);
    let following_paragraph = node
        .next_sibling()
        .is_some_and(|next| matches!(next.data().value, NodeValue::Paragraph));
    quote && following_paragraph
}
