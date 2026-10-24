//! Preserve emphasis markers that comrak joins or drops when printing nested emphasis.

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

/// Keep nested spans' delimiters from the source that produced `root`; new markers can pair differently.
pub(super) fn markers<'a>(root: Node<'a>, markdown: &str) -> Vec<(Node<'a>, &'static str)> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut markers = Vec::new();
    for node in root.descendants().filter(|node| is_nested(node)) {
        let data = node.data();
        let start = data.sourcepos.start;
        let line = lines[start.line - 1].as_bytes();
        let underscore = line[start.column - 1] == b'_';
        let delimiter = match (&data.value, underscore) {
            (NodeValue::Strong, false) => "**",
            (NodeValue::Strong, true) => "__",
            (_, false) => "*",
            (_, true) => "_",
        };
        markers.push((node, delimiter));
    }
    markers
}

fn is_emphasis(node: Node<'_>) -> bool {
    matches!(node.data().value, NodeValue::Emph | NodeValue::Strong)
}

fn is_nested(node: Node<'_>) -> bool {
    if !is_emphasis(node) {
        return false;
    }
    let parent = node.parent().is_some_and(is_emphasis);
    let child = node.children().any(is_emphasis);
    parent || child
}

/// Print the saved delimiters after actions have finished using the emphasis containers.
pub(super) fn prepare<'a>(arena: &'a Arena<'a>, markers: &[(Node<'a>, &str)]) {
    for &(node, delimiter) in markers.iter().rev() {
        if delimiter.starts_with('_') {
            let previous = skip_empty(node.previous_sibling(), false);
            if let Some(previous) = previous {
                escape_edge(arena, previous, false, false);
            }
            let next = skip_empty(node.next_sibling(), true);
            if let Some(next) = next {
                escape_edge(arena, next, true, false);
            }
        }
        unwrap(arena, node, delimiter);
    }
}

fn unwrap<'a>(arena: &'a Arena<'a>, node: Node<'a>, delimiter: &str) {
    let first = skip_empty(node.first_child(), true);
    if let Some(first) = first {
        escape_edge(arena, first, true, true);
    }
    let last = skip_empty(node.last_child(), false);
    if let Some(last) = last {
        escape_edge(arena, last, false, true);
    }
    let opening = arena.alloc(NodeValue::Raw(delimiter.to_string()).into());
    node.insert_before(opening);
    while let Some(child) = node.first_child() {
        node.insert_before(child);
    }
    let closing = arena.alloc(NodeValue::Raw(delimiter.to_string()).into());
    node.insert_before(closing);
    node.detach();
}

fn skip_empty<'a>(mut node: Option<Node<'a>>, forward: bool) -> Option<Node<'a>> {
    while let Some(current) = node {
        let empty = matches!(&current.data().value, NodeValue::Text(text) if text.is_empty());
        if !empty {
            return Some(current);
        }
        node = if forward {
            current.next_sibling()
        } else {
            current.previous_sibling()
        };
    }
    None
}

/// Keep boundary edits from changing underscore flanking or exposing whitespace inside a delimiter.
fn escape_edge<'a>(arena: &'a Arena<'a>, node: Node<'a>, leading: bool, whitespace: bool) {
    let mut data = node.data_mut();
    let NodeValue::Text(text) = &mut data.value else {
        return;
    };
    let edge = edge_character(text, leading);
    let Some(character) = edge.filter(|c| needs_escape(*c, whitespace)) else {
        return;
    };
    let entity = format!("&#{};", character as u32);
    let raw = arena.alloc(NodeValue::Raw(entity).into());
    if leading {
        node.insert_before(raw);
        *text = text[character.len_utf8()..].to_string().into();
    } else {
        node.insert_after(raw);
        *text = text[..text.len() - character.len_utf8()].to_string().into();
    }
}

fn edge_character(text: &str, leading: bool) -> Option<char> {
    if leading {
        text.chars().next()
    } else {
        text.chars().next_back()
    }
}

fn needs_escape(character: char, whitespace: bool) -> bool {
    if whitespace {
        return character.is_whitespace();
    }
    !character.is_whitespace() && !character.is_ascii_punctuation()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::codespan_to_text;
    use crate::process::gfm_math_options;
    use crate::process::math_inline_to_text;

    #[test]
    fn nested_emphasis_keeps_its_tree() -> anyhow::Result<()> {
        let cases = [
            "_*b*c_",
            "_a*b*_",
            "_*强调*文字_",
            "中文 _*强调*文字_",
            "__*b*c__",
            "**_x_**",
            "*__*x*__*",
            "**_x_ y**",
            "a*b*c",
            "中文*强调*文字",
            "__foo, __bar__, baz__",
            "__foo __bar__ baz__",
            "____foo__ bar__",
            "**foo **bar****",
            "****foo****",
            "____foo____",
            "******foo******",
            "_____foo_____",
            "[**_x_**](https://example.com)",
            "> _*b*c_",
            "- **_x_**",
            "| _*b*c_ |\n| --- |",
            "__**b**c__",
            "__c**b**__",
            "**___!b___**",
            "_***b***c_",
            "_*b*\u{301}_",
            "- \t_*b*c_",
            "> \t_*b*c_",
            "*` `*",
            "**` a `**",
            "*_b_`c`*",
            "*`c`_b_*",
            "*_b_$x$*",
            "*__b__`c`*",
            "*_` `_b*",
        ];
        let arena = Arena::new();
        let options = gfm_math_options();
        for markdown in cases {
            let root = comrak::parse_document(&arena, markdown, &options);
            let saved = markers(root, markdown);
            let mut expected = String::new();
            comrak::format_html(root, &options, &mut expected)?;
            prepare(&arena, &saved);
            let mut written = String::new();
            comrak::format_commonmark(root, &options, &mut written)?;
            let actual = comrak::markdown_to_html(&written, &options);
            assert_eq!(actual, expected, "{markdown:?} became {written:?}");
        }
        Ok(())
    }

    #[test]
    fn converted_neighbors_keep_emphasis() -> anyhow::Result<()> {
        let cases = [
            "_*b*`c`_",
            "_`c`*b*_",
            "_*b*$x$_",
            "*_b_`c`*",
            "*`c`_b_*",
            "*_b_$x$*",
            "*_b_$\\label{x}$c*",
            "a$\\label{x}$_**b**_",
            "*__b__`c`*",
            "*_` `_b*",
            "*$`x`$*",
            "*` `*",
            "**` a `**",
        ];
        let arena = Arena::new();
        let options = gfm_math_options();
        for markdown in cases {
            let root = comrak::parse_document(&arena, markdown, &options);
            let saved = markers(root, markdown);
            codespan_to_text::apply(root);
            math_inline_to_text::apply(root)?;
            let mut expected = String::new();
            comrak::format_html(root, &options, &mut expected)?;
            prepare(&arena, &saved);
            let mut written = String::new();
            comrak::format_commonmark(root, &options, &mut written)?;
            let actual = comrak::markdown_to_html(&written, &options);
            assert_eq!(actual, expected, "{markdown:?} became {written:?}");
        }
        Ok(())
    }
}
