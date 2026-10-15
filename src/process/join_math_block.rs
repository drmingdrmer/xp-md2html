//! `join-math-block`: join the paragraphs of a `$$` formula that blank lines split, as md2zhihu's
//! `join_math_paragraphs` does. comrak, as github.com, ends a paragraph at a blank line, so it reads
//! such a formula as paragraphs that hold each `$$` as text, and parses the TeX between as markdown.
//!
//! A `$$` alone on a line opens a formula that takes every line up to the next `$$`, as md2zhihu's
//! `math_lines` rule reads it: comrak would also read a line of the TeX such as `=`, `- x` or `> x`
//! as markdown, such as the underline of a setext heading, and split the formula there. Unlike
//! md2zhihu, a code fence before the next `$$` keeps its code block, and the `$$` opens no formula.
//!
//! The joined formula loses the blank lines that md2zhihu keeps: printed with one, the formula would
//! split again wherever the output is read.

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

use super::gfm_math_options;

/// The indent before a line of a paragraph after which comrak starts no block; comrak drops it from
/// the paragraph's text, so a formula keeps its TeX.
const INDENT: &str = "    ";

/// What [`join_once`] does to one line of the markdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEdit {
    Keep,
    /// Drop the line, a blank line inside a formula.
    Drop,
    /// Put [`INDENT`] before the byte at this index, where a line inside a formula would start a
    /// block.
    Indent(usize),
}

/// `markdown` with each `$$` formula on lines that comrak reads as one paragraph, so that comrak
/// reads the formula as one, with its TeX as written: without the blank lines inside it, and with
/// an indent of 4 spaces before each line of the TeX that would start a block.
///
/// A paragraph opens a formula when comrak keeps a `$$` in it as text, and the next paragraph with a
/// `$$` closes the formula. A `$$` alone on a line opens a formula up to the next line with a `$$`,
/// unless a code fence comes first. Each pass joins the formulas that the paragraphs open, and one
/// more line of each formula that a `$$` alone on a line opens; a joined paragraph can open one
/// more formula, and a joined line changes how comrak reads the lines after it, so the next pass
/// parses again.
pub fn join(markdown: &str) -> String {
    let mut text = markdown.to_string();
    loop {
        let joined = join_once(&text);
        if joined == text {
            return text;
        }
        text = joined;
    }
}

/// `markdown` without the blank lines between each paragraph that opens a formula and the paragraph
/// that closes it, and with the [`split_edit`] of each formula that a `$$` alone on a line opens.
fn join_once(markdown: &str) -> String {
    let arena = Arena::new();
    let mut options = gfm_math_options();
    // An escaped `\$` stays out of the text nodes, so a `$$` in a text node is unescaped.
    options.parse.escaped_char_spans = true;
    let root = comrak::parse_document(&arena, markdown, &options);
    let lines = split_lines(markdown);

    let mut edits = vec![LineEdit::Keep; lines.len()];
    // The last line of the last formula found; the blocks up to it belong to that formula.
    let mut formula_end = 0;
    for node in root.descendants() {
        let start = node.data().sourcepos.start.line;
        let in_formula = start <= formula_end;
        if in_formula {
            continue;
        }
        if let Some(opening) = opening_line(node) {
            let Some((split, end)) = formula_split(root, node, opening, &lines) else {
                continue;
            };
            if let Some((number, edit)) = split {
                edits[number - 1] = edit;
            }
            formula_end = end;
            continue;
        }
        if !opens_formula(node) {
            continue;
        }
        let Some((gaps, end)) = formula_gaps(node, &lines) else {
            continue;
        };
        for number in gaps {
            edits[number - 1] = LineEdit::Drop;
        }
        formula_end = end;
    }

    let mut joined = String::new();
    for (line, edit) in lines.iter().zip(edits) {
        match edit {
            LineEdit::Keep => joined.push_str(line),
            LineEdit::Drop => {}
            LineEdit::Indent(byte) => {
                joined.push_str(&line[..byte]);
                joined.push_str(INDENT);
                joined.push_str(&line[byte..]);
            }
        }
    }
    joined
}

/// The line of the `$$` that `node`, a paragraph or a setext heading, keeps as text alone on a line;
/// None for any other node, or when it has no such `$$`. comrak pairs the `$$` that it can close, so
/// this one opens a formula.
fn opening_line(node: Node<'_>) -> Option<usize> {
    let holds_lines = match &node.data().value {
        NodeValue::Paragraph => true,
        NodeValue::Heading(heading) => heading.setext,
        _ => false,
    };
    if !holds_lines {
        return None;
    }
    for child in node.children() {
        let data = child.data();
        let NodeValue::Text(text) = &data.value else {
            continue;
        };
        let starts_line = child.previous_sibling().is_none_or(is_line_break);
        let ends_line = child.next_sibling().is_none_or(is_line_break);
        if text == "$$" && starts_line && ends_line {
            return Some(data.sourcepos.start.line);
        }
    }
    None
}

/// Whether `node` ends a line of its paragraph.
fn is_line_break(node: Node<'_>) -> bool {
    matches!(
        node.data().value,
        NodeValue::SoftBreak | NodeValue::LineBreak
    )
}

/// The [`split_edit`] of the formula that the `$$` alone on line `opening` of `block` opens, and
/// the line that closes the formula: the next line with a `$$`. None when no line after `opening`
/// holds a `$$`. No edit when a code fence starts before the closing line: the code block stays,
/// and the `$$` on the closing line still pairs with the one at `opening` instead of opening a
/// formula.
fn formula_split(
    root: Node<'_>,
    block: Node<'_>,
    opening: usize,
    lines: &[&str],
) -> Option<(Option<(usize, LineEdit)>, usize)> {
    let end = (opening + 1..=lines.len()).find(|&number| lines[number - 1].contains("$$"))?;
    for node in root.descendants() {
        let data = node.data();
        let NodeValue::CodeBlock(code) = &data.value else {
            continue;
        };
        let start = data.sourcepos.start.line;
        let in_formula = opening < start && start <= end;
        if code.fenced && in_formula {
            return Some((None, end));
        }
    }
    let split = split_edit(root, block, end, lines);
    Some((split, end))
}

/// The edit of the first line up to `end`, the line that closes a formula, at which comrak ends
/// `block`, the paragraph or setext heading that opens the formula. A blank line goes. A line that
/// starts a block takes [`INDENT`] before that block, as do the underline of a setext heading and
/// the delimiter row of a table, which turn the lines above them into a block. The edited line is a
/// line of `block`, which can change how comrak reads the lines after it, such as a `>` that now
/// starts a block quote, so the next pass of [`join`] parses again. None when `block` reaches `end`.
fn split_edit(
    root: Node<'_>,
    block: Node<'_>,
    end: usize,
    lines: &[&str],
) -> Option<(usize, LineEdit)> {
    let sourcepos = block.data().sourcepos;
    let is_heading = matches!(block.data().value, NodeValue::Heading(_));
    if is_heading {
        let underline = sourcepos.end.line;
        return indent_text(lines, underline, end);
    }

    let next = sourcepos.end.line + 1;
    if next > end {
        return None;
    }
    let Some(started) = first_block_at(root, next) else {
        // Only a blank line ends a paragraph without starting a block.
        let blank = is_blank(lines[next - 1]);
        if !blank {
            return None;
        }
        return Some((next, LineEdit::Drop));
    };
    let is_table = matches!(started.data().value, NodeValue::Table(_));
    if is_table {
        let delimiter_row = next + 1;
        return indent_text(lines, delimiter_row, end);
    }
    let byte = started.data().sourcepos.start.column - 1;
    Some((next, LineEdit::Indent(byte)))
}

/// The outermost block that starts on line `number`, which comes first in document order.
fn first_block_at<'a>(root: Node<'a>, number: usize) -> Option<Node<'a>> {
    root.descendants().find(|node| {
        let data = node.data();
        data.value.block() && data.sourcepos.start.line == number
    })
}

/// The edit that puts [`INDENT`] before the text of line `number`, a setext heading's underline or
/// a table's delimiter row; None when the line comes after `end`, outside the formula.
fn indent_text(lines: &[&str], number: usize, end: usize) -> Option<(usize, LineEdit)> {
    if number > end {
        return None;
    }
    let byte = text_start(lines[number - 1]);
    Some((number, LineEdit::Indent(byte)))
}

/// The byte at which the text of `line` starts, after the indent and the `>` of each block quote:
/// the start of a setext heading's underline or of a table's delimiter row, neither of which a
/// container's prefix holds.
fn text_start(line: &str) -> usize {
    let start = line.find(|c: char| c != '>' && !c.is_ascii_whitespace());
    start.unwrap_or(line.len())
}

/// Whether `node` is a paragraph that opens a formula: comrak keeps a `$$` in it as text, because
/// nothing after it in the paragraph closes it.
fn opens_formula(node: Node<'_>) -> bool {
    let is_paragraph = matches!(node.data().value, NodeValue::Paragraph);
    if !is_paragraph {
        return false;
    }
    for descendant in node.descendants() {
        let data = descendant.data();
        let NodeValue::Text(text) = &data.value else {
            continue;
        };
        // comrak reads no formula at a longer run of `$`.
        let has_pair = text.split(|c: char| c != '$').any(|run| run.len() == 2);
        if has_pair {
            return true;
        }
    }
    false
}

/// The blank lines between `opening`, a paragraph that opens a formula, and the next paragraph with
/// a `$$`, which closes it, and the last line of that paragraph. None when a block other than a
/// paragraph comes first, or when a line between two of the paragraphs is not blank, such as a link
/// reference definition.
fn formula_gaps(opening: Node<'_>, lines: &[&str]) -> Option<(Vec<usize>, usize)> {
    let mut gaps = Vec::new();
    let mut previous_end = opening.data().sourcepos.end.line;
    let mut next = opening.next_sibling();
    while let Some(node) = next {
        let is_paragraph = matches!(node.data().value, NodeValue::Paragraph);
        if !is_paragraph {
            return None;
        }
        let sourcepos = node.data().sourcepos;
        for number in previous_end + 1..sourcepos.start.line {
            let blank = is_blank(lines[number - 1]);
            if !blank {
                return None;
            }
            gaps.push(number);
        }
        // comrak closes the formula at the first `$$` after it, also one in a code span.
        let own_lines = &lines[sourcepos.start.line - 1..sourcepos.end.line];
        let closes = own_lines.iter().any(|line| line.contains("$$"));
        if closes {
            return Some((gaps, sourcepos.end.line));
        }
        previous_end = sourcepos.end.line;
        next = node.next_sibling();
    }
    None
}

/// Whether `line` is blank; a blank line in a block quote holds its `>`.
fn is_blank(line: &str) -> bool {
    line.chars().all(|c| c == '>' || c.is_ascii_whitespace())
}

/// The lines of `text` with their line ends, split where comrak splits them: after `\n`, `\r\n` and
/// a lone `\r`.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, c) in text.char_indices() {
        let end = index + 1;
        let crlf = c == '\r' && text[end..].starts_with('\n');
        let ends_line = (c == '\n' || c == '\r') && !crlf;
        if ends_line {
            lines.push(&text[start..end]);
            start = end;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A formula that blank lines split loses them, also in a list item and in a quote, and its TeX
    /// stays as written. A paragraph that closes one formula and opens the next joins both.
    #[test]
    fn test_join() {
        let markdown = "$$\na\\,b_{i}\n\n\nc\n$$\n\n\
                        - $$\n  d\n\n  e\n  $$\n\n\
                        > $$\n> f\n>\n> g\n> $$\n\n\
                        $$\nh\n\ni\n$$ and $$\nj\n\nk\n$$\n";
        let joined = join(markdown);
        let expected = "$$\na\\,b_{i}\nc\n$$\n\n\
                        - $$\n  d\n  e\n  $$\n\n\
                        > $$\n> f\n> g\n> $$\n\n\
                        $$\nh\ni\n$$ and $$\nj\nk\n$$\n";
        assert_eq!(joined, expected);
    }

    /// A `$$` alone on a line takes every line up to the next `$$` as TeX: a line that would start a
    /// block takes the indent, such as a `=` that would underline a setext heading, also after a
    /// blank line, or the delimiter row of a table, and a link reference definition stays in the
    /// TeX, as md2zhihu reads it.
    #[test]
    fn test_join_lines() {
        let markdown = "$$\na\n=\nb\n$$\n\n\
                        $$\nb =\n\n\\frac{1}{2}\n\n=\n\nc\n$$\n\n\
                        $$\n-\n- c\n> d\n# e\n1. f\n***\nx | y\n-|-\n<div>\n$$\n\n\
                        $$\nd\n\n[e]: https://e.com\n\nf\n$$\n";
        let joined = join(markdown);
        let expected = "$$\na\n    =\nb\n$$\n\n\
                        $$\nb =\n\\frac{1}{2}\n    =\nc\n$$\n\n\
                        $$\n    -\n    - c\n    > d\n    # e\n    1. f\n    ***\n\
                        x | y\n    -|-\n    <div>\n$$\n\n\
                        $$\nd\n[e]: https://e.com\nf\n$$\n";
        assert_eq!(joined, expected);
    }

    /// In a list item and in a quote, the indent goes after the item's indent and after the `>`, and
    /// a `>` of the TeX stays in the TeX; a `$$` after a line of text opens a formula too.
    #[test]
    fn test_join_lines_in_containers() {
        let markdown = "- In an item:\n\n  $$\n  p\n  =\n\n  - q\n  $$\n\n\
                        > $$\n> u\n> =\n>\n> > v\n> $$\n\n\
                        Text before it:\n$$\nx\n=\ny\n$$\n";
        let joined = join(markdown);
        let expected = "- In an item:\n\n  $$\n  p\n      =\n      - q\n  $$\n\n\
                        > $$\n> u\n>     =\n>     > v\n> $$\n\n\
                        Text before it:\n$$\nx\n    =\ny\n$$\n";
        assert_eq!(joined, expected);
    }

    /// Formulas that prose separates stay apart. The front matter, the prose with its inline
    /// formula, and a code block that holds a formula stay as written.
    #[test]
    fn test_join_lines_between_blocks() {
        let markdown = "---\ntitle: a\n---\n\n\
                        $$\na\n\n=\nb\n$$\n\n\
                        Prose, with $$x$$ inline.\n\n\
                        $$\nc\n$$\n\n\
                        More prose.\n\n\
                        $$\nd\n=\ne\n$$\n\n\
                        ```tex\n$$\nf\n=\ng\n$$\n```\n";
        let joined = join(markdown);
        let expected = "---\ntitle: a\n---\n\n\
                        $$\na\n    =\nb\n$$\n\n\
                        Prose, with $$x$$ inline.\n\n\
                        $$\nc\n$$\n\n\
                        More prose.\n\n\
                        $$\nd\n    =\ne\n$$\n\n\
                        ```tex\n$$\nf\n=\ng\n$$\n```\n";
        assert_eq!(joined, expected);
    }

    /// These join nothing: a `$$` that a heading follows before the next `$$`, as in md2zhihu's
    /// `math-unclosed` case; an escaped `\$\$`; a `$$` alone on a line whose closing `$$` comes after
    /// a code block, which stays, while the closing `$$` opens no formula; and a `$$` after text
    /// whose closing `$$` comes after a link reference definition.
    #[test]
    fn test_join_nothing() {
        let markdown = "In bash, $$ is the PID.\n\n\
                        Second, with *emphasis\n\n\
                        Third*.\n\n\
                        # Escaped\n\n\
                        Price \\$\\$\n\n\
                        Text $$\n\n\
                        # Code block\n\n\
                        $$\na\n\n```\nb\n```\n\nc\n$$\n\n\
                        # Definition\n\n\
                        Text $$\nd\n\n[e]: https://e.com\n\nf $$\n";
        let joined = join(markdown);
        assert_eq!(joined, markdown);
    }

    #[test]
    fn test_split_lines() {
        let lines = split_lines("a\r\nb\rc\nd");
        assert_eq!(lines, vec!["a\r\n", "b\r", "c\n", "d"]);
    }
}
