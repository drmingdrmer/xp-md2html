//! Read md2zhihu's dollar math before Markdown can interpret the TeX as inline markup.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::LazyLock;

use comrak::nodes::LineColumn;
use comrak::nodes::NodeValue;
use comrak::nodes::Sourcepos;
use comrak::options::BrokenLinkReference;
use comrak::Arena;
use comrak::Node;
use comrak::ResolvedReference;
use fancy_regex::Regex;

static MATH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\A(?:\$\$([^$][\s\S]*?)\$\$|",
        r"\$((?=\s)(?:[^$\\]|\\[\s\S])+|",
        r"(?:[^$\\]|\\[\s\S])*(?:[^\s$\\]|\\[\s\S]))\$(?!\d))"
    ))
    .expect("valid math syntax")
});

pub(super) struct Prepared {
    pub markdown: String,
    literals: BTreeMap<LineColumn, String>,
}

struct Scanner<'s> {
    source: &'s str,
    line_starts: Vec<usize>,
    consumed: usize,
    prepared: Prepared,
}

/// Hide TeX behind byte-for-byte placeholders, keeping source positions for refs and emphasis.
pub(super) fn prepare(markdown: &str, options: &comrak::Options) -> anyhow::Result<Prepared> {
    let arena = Arena::new();
    let mut options = options.clone();
    options.extension.math_dollars = false;
    options.parse.escaped_char_spans = true;
    // Recognize reference syntax even when its definition comes from --refs or front matter.
    options.parse.broken_link_callback = Some(Arc::new(|_: BrokenLinkReference| {
        Some(ResolvedReference {
            url: String::new(),
            title: String::new(),
        })
    }));
    let root = comrak::parse_document(&arena, markdown, &options);
    let mut scanner = Scanner::new(markdown);
    for node in root.descendants() {
        scanner.scan(node)?;
    }
    Ok(scanner.prepared)
}

impl<'s> Scanner<'s> {
    fn new(source: &'s str) -> Self {
        let mut line_starts = vec![0];
        let starts = source.match_indices('\n').map(|(index, _)| index + 1);
        line_starts.extend(starts);
        let prepared = Prepared {
            markdown: source.to_string(),
            literals: BTreeMap::new(),
        };
        Self {
            source,
            line_starts,
            consumed: 0,
            prepared,
        }
    }

    fn offset(&self, position: LineColumn) -> usize {
        self.line_starts[position.line - 1] + position.column - 1
    }

    fn position(&self, index: usize) -> LineColumn {
        let line = self.line_starts.partition_point(|start| *start <= index);
        let column = index - self.line_starts[line - 1] + 1;
        LineColumn { line, column }
    }

    /// Only text can open math: escaped dollars, code, HTML and destinations stay opaque.
    fn text_range(&self, node: Node<'_>) -> Option<(Range<usize>, Sourcepos)> {
        let data = node.data();
        if !matches!(data.value, NodeValue::Text(_)) {
            return None;
        }
        let parent = node.parent()?;
        let escaped = matches!(parent.data().value, NodeValue::Escaped);
        if escaped || self.autolink(parent) {
            return None;
        }
        let block = node
            .ancestors()
            .find(|parent| parent.data().value.block())?;
        let position = block.data().sourcepos;
        let start = self.offset(data.sourcepos.start);
        let end = self.offset(data.sourcepos.end) + 1;
        Some((start..end.min(self.source.len()), position))
    }

    fn autolink(&self, node: Node<'_>) -> bool {
        let data = node.data();
        if !matches!(data.value, NodeValue::Link(_)) {
            return false;
        }
        let start = data.sourcepos.start;
        if start.line == 0 || start.column == 0 {
            return true;
        }
        let index = self.offset(start);
        self.source.as_bytes()[index] != b'['
    }

    fn scan(&mut self, node: Node<'_>) -> anyhow::Result<()> {
        let Some((range, block)) = self.text_range(node) else {
            return Ok(());
        };
        for index in range {
            if index < self.consumed || self.source.as_bytes()[index] != b'$' {
                continue;
            }
            self.formula(index, block)?;
        }
        Ok(())
    }

    fn formula(&mut self, index: usize, block: Sourcepos) -> anyhow::Result<()> {
        let limit = self.offset(block.end) + 1;
        let Some(found) = MATH_PATTERN.captures(&self.source[index..limit])? else {
            return Ok(());
        };
        let whole = found.get(0).unwrap();
        let body = found.get(1).or_else(|| found.get(2)).unwrap();
        let position = self.position(index);
        let width = block.start.column - 1;
        let literal = unprefix(body.as_str(), width);
        self.prepared.literals.insert(position, literal);
        let range = index + body.start()..index + body.end();
        mask(&mut self.prepared.markdown, range, width);
        self.consumed = index + whole.end();
        Ok(())
    }
}

fn prefix(line: &str, width: usize) -> usize {
    let bytes = line.as_bytes();
    let end = width.min(bytes.len());
    let is_prefix = bytes[..end]
        .iter()
        .all(|byte| byte.is_ascii_whitespace() || *byte == b'>');
    if is_prefix {
        end
    } else {
        0
    }
}

fn unprefix(text: &str, width: usize) -> String {
    let mut lines = text.split_inclusive('\n');
    let mut literal = lines.next().unwrap_or_default().to_string();
    for line in lines {
        let text = &line[prefix(line, width)..];
        literal.push_str(text.trim_start_matches([' ', '\t']));
    }
    literal
}

fn mask(markdown: &mut String, range: Range<usize>, width: usize) {
    let mut hidden = String::new();
    for (index, line) in markdown[range.clone()].split_inclusive('\n').enumerate() {
        let keep = if index == 0 { 0 } else { prefix(line, width) };
        hidden.push_str(&line[..keep]);
        for byte in line.as_bytes().iter().skip(keep) {
            hidden.push(if *byte == b'\n' || *byte == b'\r' {
                *byte as char
            } else {
                'x'
            });
        }
    }
    markdown.replace_range(range, &hidden);
}

impl Prepared {
    /// Restore raw TeX while keeping the original dollar count for Markdown formatting.
    pub fn restore(&self, root: Node<'_>) {
        for node in root.descendants() {
            let mut data = node.data_mut();
            let start = data.sourcepos.start;
            let NodeValue::Math(math) = &mut data.value else {
                continue;
            };
            if let Some(literal) = self.literals.get(&start) {
                math.literal.clone_from(literal);
            }
        }
    }
}

/// Display math starts its paragraph and ends a line, independently of its dollar count.
pub(super) fn is_display(node: Node<'_>) -> bool {
    starts_paragraph(node) && ends_line(node)
}

fn starts_paragraph(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let paragraph = matches!(parent.data().value, NodeValue::Paragraph);
    paragraph && node.previous_sibling().is_none()
}

fn ends_line(node: Node<'_>) -> bool {
    let Some(next) = node.next_sibling() else {
        return true;
    };
    matches!(
        next.data().value,
        NodeValue::SoftBreak | NodeValue::LineBreak
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::refs::Refs;
    use crate::process::Loader;

    #[test]
    fn test_dollars_in_urls_and_reference_identifiers() -> anyhow::Result<()> {
        let refs = Refs::load(
            &[],
            "---\nrefs:\n  '$secret$': https://a.invalid\n---\n",
            None,
        )?;
        let cases = [
            "<https://a.invalid/$secret$>",
            "https://a.invalid/$secret$",
            "[x][$secret$]",
            "[x][$secret$]\n\n[$secret$]: https://a.invalid",
            "$x$\n[q]: https://q.invalid\n",
        ];
        for markdown in cases {
            let arena = Arena::new();
            let options = super::super::gfm_math_options();
            let parsed = refs.parse(&arena, markdown, None, &options)?;
            let mut expected = String::new();
            comrak::format_html(parsed.root, &options, &mut expected)?;
            let loader = Loader::new(&arena, &[]);
            let root = loader.load(markdown, &refs)?;
            let mut actual = String::new();
            comrak::format_html(root, &options, &mut actual)?;
            assert_eq!(actual, expected, "{markdown:?}");
        }
        Ok(())
    }

    #[test]
    fn test_math_syntax_and_placement() -> anyhow::Result<()> {
        let cases: &[(&str, &[(&str, bool)])] = &[
            ("$ x $", &[(" x ", true)]),
            ("a $ x $ b", &[(" x ", false)]),
            ("$x $", &[]),
            ("costs $5 and $6; calculate $x$.", &[("x", false)]),
            ("a $$x$$ b", &[("x", false)]),
            ("[a $$x$$](u)", &[("x", false)]),
            ("[$$x$$][r]\n\n[r]: u", &[("x", false)]),
            ("# $x$", &[("x", false)]),
            ("~~$x$~~", &[("x", false)]),
            ("$x$\na", &[("x", true)]),
            ("a $x\ny$ b", &[("x\ny", false)]),
            ("> $ x\n> y $", &[(" x\ny ", true)]),
            ("- $ x\n  y $", &[(" x\ny ", true)]),
            ("a $ \\alpha_{a*b} &amp; \\$ $.", &[(
                r" \alpha_{a*b} &amp; \$ ",
                false,
            )]),
            ("`$ x $` and \\$ x \\$", &[]),
            ("[x](a$y$) ![x](b$z$)", &[]),
            ("```\n$ x $\n```", &[]),
            ("<span title=\"$ x $\">a</span>", &[]),
            ("a $`x`$ b", &[("x", false)]),
            ("中文 **_a_** $ x $", &[(" x ", false)]),
        ];
        for (markdown, expected) in cases {
            let arena = Arena::new();
            let loader = Loader::new(&arena, &[]);
            let root = loader.load(markdown, &Refs::default())?;
            let mut actual = Vec::new();
            for node in root.descendants() {
                if let NodeValue::Math(math) = &node.data().value {
                    let display = is_display(node);
                    actual.push((math.literal.clone(), display));
                }
            }
            let expected: Vec<_> = expected
                .iter()
                .map(|(tex, display)| (tex.to_string(), *display))
                .collect();
            assert_eq!(actual, expected, "{markdown:?}");
        }
        Ok(())
    }
}
