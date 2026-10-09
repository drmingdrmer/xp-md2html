//! `join-math-block`: join the paragraphs of a `$$` formula that blank lines split, as md2zhihu's
//! `join_math_paragraphs` does. comrak, as github.com, ends a paragraph at a blank line, so it reads
//! such a formula as paragraphs that hold each `$$` as text, and parses the TeX between as markdown.
//!
//! The joined formula loses the blank lines that md2zhihu keeps: printed with one, the formula would
//! split again wherever the output is read.

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

use super::gfm_math_options;

/// `markdown` without the blank lines inside each `$$` formula that spans paragraphs, so that comrak
/// reads the formula as one, with its TeX as written.
///
/// A paragraph opens a formula when comrak keeps a `$$` in it as text, and the next paragraph with a
/// `$$` closes the formula. Each pass joins the formulas that the paragraphs open; a joined paragraph
/// can open one more, which the next pass joins.
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
/// that closes it.
fn join_once(markdown: &str) -> String {
    let arena = Arena::new();
    let mut options = gfm_math_options();
    // An escaped `\$` stays out of the text nodes, so a `$$` in a text node is unescaped.
    options.parse.escaped_char_spans = true;
    let root = comrak::parse_document(&arena, markdown, &options);
    let lines = split_lines(markdown);

    let mut keep = vec![true; lines.len()];
    // The last line of the last formula found; the paragraphs up to it belong to that formula.
    let mut formula_end = 0;
    for node in root.descendants() {
        let start = node.data().sourcepos.start.line;
        let in_formula = start <= formula_end;
        if in_formula || !opens_formula(node) {
            continue;
        }
        let Some((gaps, end)) = formula_gaps(node, &lines) else {
            continue;
        };
        for number in gaps {
            keep[number - 1] = false;
        }
        formula_end = end;
    }

    let mut joined = String::new();
    for (line, keep) in lines.iter().zip(keep) {
        if keep {
            joined.push_str(line);
        }
    }
    joined
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
            // A blank line in a block quote holds its `>`.
            let blank = lines[number - 1]
                .chars()
                .all(|c| c == '>' || c.is_ascii_whitespace());
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

    /// These join nothing: a `$$` that a heading follows before the next `$$`, as in md2zhihu's
    /// `math-unclosed` case; an escaped `\$\$`; and a `$$` whose closing `$$` comes after a code
    /// block or a link reference definition.
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
                        $$\nd\n\n[e]: https://e.com\n\nf\n$$\n";
        let joined = join(markdown);
        assert_eq!(joined, markdown);
    }

    #[test]
    fn test_split_lines() {
        let lines = split_lines("a\r\nb\rc\nd");
        assert_eq!(lines, vec!["a\r\n", "b\r", "c\n", "d"]);
    }
}
