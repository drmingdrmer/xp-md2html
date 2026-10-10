//! `append-reference-list`: append a list of the link references that the file uses, as md2zhihu's
//! `render_ref_list` does.

use comrak::nodes::NodeLink;
use comrak::nodes::NodeList;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

use super::Loader;

/// A link reference definition, `[label]: url "title"`.
#[derive(PartialEq)]
struct Reference {
    label: String,
    url: String,
    title: Option<String>,
}

/// Append to `root` a "Reference:" list with one `- <title, or label> : <url>` item for each
/// definition that a `[text][label]`, `[label][]` or `[label]` link of `loader` uses, sorted by
/// label.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>, loader: &Loader<'a>) {
    let references = used_references(loader);
    if references.is_empty() {
        return;
    }

    let label = arena.alloc(NodeValue::Text("Reference:".into()).into());
    let heading = arena.alloc(NodeValue::Paragraph.into());
    heading.append(label);
    root.append(heading);

    // A loose list, with a blank line between the items, as md2zhihu writes it.
    let list_meta = NodeList {
        tight: false,
        ..NodeList::default()
    };
    let list = arena.alloc(NodeValue::List(list_meta).into());
    for reference in references {
        let paragraph = reference_paragraph(arena, reference);
        let item = arena.alloc(NodeValue::Item(list_meta).into());
        item.append(paragraph);
        list.append(item);
    }
    root.append(list);
}

/// The paragraph `<title, or label> : <url>` of `reference`. The URL is a link node, so a later
/// `rewrite-link-urls` rewrites it; comrak writes a link whose text is its URL as `<url>`, which
/// renders the same as md2zhihu's `[url](url)`.
fn reference_paragraph<'a>(arena: &'a Arena<'a>, reference: Reference) -> Node<'a> {
    let name = reference.title.unwrap_or(reference.label);
    let text = arena.alloc(NodeValue::Text(format!("{name} : ").into()).into());

    let url_text = arena.alloc(NodeValue::Text(reference.url.clone().into()).into());
    let link = NodeLink {
        url: reference.url,
        title: String::new(),
    };
    let link = arena.alloc(NodeValue::Link(Box::new(link)).into());
    link.append(url_text);

    let paragraph = arena.alloc(NodeValue::Paragraph.into());
    paragraph.append(text);
    paragraph.append(link);
    paragraph
}

/// The definition that each link of `loader` made by a reference uses, once each, sorted by label.
/// Each takes its link's current URL and title, which shows the edits of the earlier actions; a
/// link that an action took out of the tree, such as into the image of a table, counts too.
fn used_references(loader: &Loader<'_>) -> Vec<Reference> {
    let mut references = Vec::new();
    for (node, label) in loader.references() {
        let data = node.data();
        let NodeValue::Link(link) = &data.value else {
            continue;
        };
        let title = if link.title.is_empty() {
            None
        } else {
            Some(link.title.clone())
        };
        let reference = Reference {
            label,
            url: link.url.clone(),
            title,
        };
        if !references.contains(&reference) {
            references.push(reference);
        }
    }
    references.sort_by(|a, b| a.label.cmp(&b.label));
    references
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::refs::Refs;

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
        let loader = Loader::new(&arena, &[]);
        let root = loader.load(markdown, &Refs::default())?;

        apply(&arena, root, &loader);

        let mut out = String::new();
        let options = super::super::gfm_math_options();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "Read [the post](http://b.com \"Post B\"), [A](http://a.com), \\[x\\] and \
                        [c](http://c.com).\n\n\
                        ```text\n[x]: http://x.com\n```\n\n\
                        Reference:\n\n\
                        - A : <http://a.com>\n\n\
                        - Post B : <http://b.com>\n";
        assert_eq!(out, expected);
        Ok(())
    }

    /// A file whose links use no definition gets no list.
    #[test]
    fn test_apply_without_references() -> anyhow::Result<()> {
        let markdown = "See [c](http://c.com).\n\n[unused]: http://u.com\n";
        let arena = Arena::new();
        let loader = Loader::new(&arena, &[]);
        let root = loader.load(markdown, &Refs::default())?;

        apply(&arena, root, &loader);

        let mut out = String::new();
        let options = super::super::gfm_math_options();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, "See [c](http://c.com).\n");
        Ok(())
    }

    /// The list takes the definitions of every parse of the loader, also of a tree out of `root`,
    /// once each, with the current URL of the link; an image that a reference made gets no item.
    #[test]
    fn test_apply_current_links() -> anyhow::Result<()> {
        let arena = Arena::new();
        let loader = Loader::new(&arena, &[]);
        let refs = Refs::default();
        let markdown = "[a], [A] and ![i][b].\n\n[a]: http://old.com/a\n[b]: http://b.com\n";
        let root = loader.load(markdown, &refs)?;
        loader.load("[c]\n\n[c]: http://c.com \"C\"\n", &refs)?;
        for node in root.descendants() {
            if let NodeValue::Link(link) = &mut node.data_mut().value {
                link.url = link.url.replace("old", "new");
            }
        }

        apply(&arena, root, &loader);

        let mut out = String::new();
        let options = super::super::gfm_math_options();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "[a](http://new.com/a), [A](http://new.com/a) and ![i](http://b.com).\n\n\
                        Reference:\n\n\
                        - a : <http://new.com/a>\n\n\
                        - C : <http://c.com>\n";
        assert_eq!(out, expected);
        Ok(())
    }
}
