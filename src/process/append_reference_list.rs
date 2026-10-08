//! `append-reference-list`: append a list of the link references that the file uses, as md2zhihu's
//! `render_ref_list` does.

use std::collections::HashMap;
use std::collections::HashSet;

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;
use markdown::mdast;

/// A link reference definition, `[label]: url "title"`.
struct Reference {
    label: String,
    url: String,
    title: Option<String>,
}

/// Append to `root` a "Reference:" list with one `- <title, or label> : [url](url)` item for each
/// definition in `source` that a `[text][label]`, `[label][]` or `[label]` link uses, sorted by
/// label.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>, source: &str) -> anyhow::Result<()> {
    let references = used_references(source)?;
    if references.is_empty() {
        return Ok(());
    }

    let mut lines = vec!["Reference:".to_string()];
    for reference in references {
        let text = reference.title.unwrap_or(reference.label);
        let url = reference.url;
        lines.push(format!("- {text} : [{url}]({url})"));
    }
    let mut list = lines.join("\n\n");
    list.push('\n');

    // comrak writes a link whose text is its URL as `<url>`, so the list is raw markdown, which
    // keeps md2zhihu's `[url](url)`.
    let raw = arena.alloc(NodeValue::Raw(list).into());
    root.append(raw);
    Ok(())
}

/// The definitions in `source` that a link uses, sorted by label.
///
/// comrak drops the definitions and keeps no trace of which link was a reference, so the `markdown`
/// crate parses `source` again, with the same GFM, math and front matter syntax.
fn used_references(source: &str) -> anyhow::Result<Vec<Reference>> {
    let options = markdown::ParseOptions {
        constructs: markdown::Constructs {
            frontmatter: true,
            math_flow: true,
            math_text: true,
            ..markdown::Constructs::gfm()
        },
        ..markdown::ParseOptions::gfm()
    };
    let tree = markdown::to_mdast(source, &options)
        .map_err(|message| anyhow::anyhow!("Failed to parse markdown: {message}"))?;

    let mut definitions = HashMap::new();
    let mut used = HashSet::new();
    collect(&tree, &mut definitions, &mut used);

    definitions.retain(|identifier, _| used.contains(identifier));
    let mut references: Vec<Reference> = definitions.into_values().collect();
    references.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(references)
}

/// Add every definition under `node` to `definitions` and the identifier of every link reference to
/// `used`; both are keyed by the normalized label.
fn collect(
    node: &mdast::Node,
    definitions: &mut HashMap<String, Reference>,
    used: &mut HashSet<String>,
) {
    match node {
        mdast::Node::Definition(definition) => {
            let label = definition.label.clone();
            let reference = Reference {
                label: label.unwrap_or_else(|| definition.identifier.clone()),
                url: definition.url.clone(),
                title: definition.title.clone(),
            };
            // As in CommonMark, the first definition of a label wins.
            definitions
                .entry(definition.identifier.clone())
                .or_insert(reference);
        }
        mdast::Node::LinkReference(link) => {
            used.insert(link.identifier.clone());
        }
        _ => {}
    }

    let Some(children) = node.children() else {
        return;
    };
    for child in children {
        collect(child, definitions, used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each used definition gets an item, sorted by label and named by its title if it has one; an
    /// unused definition, an inline link and a definition in a code block get none.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "Read [the post][b], [A], [x] and [c](http://c.com).\n\n\
                        [b]: http://b.com \"Post B\"\n\
                        [A]: http://a.com\n\
                        [unused]: http://u.com\n\n\
                        ```text\n[x]: http://x.com\n```\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(&arena, root, markdown)?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "Read [the post](http://b.com \"Post B\"), [A](http://a.com), \\[x\\] and \
                        [c](http://c.com).\n\n\
                        ```text\n[x]: http://x.com\n```\n\n\
                        Reference:\n\n\
                        - A : [http://a.com](http://a.com)\n\n\
                        - Post B : [http://b.com](http://b.com)\n";
        assert_eq!(out, expected);
        Ok(())
    }

    /// A file whose links use no definition gets no list.
    #[test]
    fn test_apply_without_references() -> anyhow::Result<()> {
        let markdown = "See [c](http://c.com).\n\n[unused]: http://u.com\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(&arena, root, markdown)?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, "See [c](http://c.com).\n");
        Ok(())
    }
}
