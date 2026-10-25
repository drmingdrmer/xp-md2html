//! `append-reference-list`: append a list of the link references that the file uses, as md2zhihu's
//! `render_ref_list` does.

use std::collections::HashMap;

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
        let paragraph = reference_paragraph(arena, reference, loader);
        let item = arena.alloc(NodeValue::Item(list_meta).into());
        item.append(paragraph);
        list.append(item);
    }
    root.append(list);
}

/// The paragraph `<title, or label> : <url>` of `reference`. The URL is a link node, so a later
/// `rewrite-link-urls` rewrites it; comrak writes a link whose text is its URL as `<url>`, which
/// renders the same as md2zhihu's `[url](url)`.
fn reference_paragraph<'a>(
    arena: &'a Arena<'a>,
    reference: Reference,
    loader: &Loader<'a>,
) -> Node<'a> {
    let paragraph = match reference.title {
        Some(title) => {
            let paragraph: Node<'a> = arena.alloc(NodeValue::Paragraph.into());
            let title = title.trim_start_matches([' ', '\t']);
            let text = arena.alloc(NodeValue::Text(title.to_string().into()).into());
            paragraph.append(text);
            paragraph
        }
        None => label_paragraph(loader, &reference.label),
    };
    let separator = arena.alloc(NodeValue::Text(" : ".into()).into());
    paragraph.append(separator);
    let displayed_url = super::percent_decode(&reference.url);
    let url_text = arena.alloc(NodeValue::Text(displayed_url.into()).into());
    let link = NodeLink {
        url: reference.url,
        title: String::new(),
    };
    let link = arena.alloc(NodeValue::Link(Box::new(link)).into());
    link.append(url_text);

    paragraph.append(link);
    paragraph
}

/// A plain prefix keeps `1. Intro` and other block markers in inline content.
fn label_paragraph<'a>(loader: &Loader<'a>, label: &str) -> Node<'a> {
    let label = label.replace(['\r', '\n'], " ");
    let label = label.trim_start_matches([' ', '\t']);
    let markdown = format!("x {label}");
    let options = super::gfm_math_options();
    let root = comrak::parse_document(loader.arena, &markdown, &options);
    let markers = super::emphasis::markers(root, &markdown);
    loader.emphasis.borrow_mut().extend(markers);
    let paragraph = root.first_child().expect("the prefix makes a paragraph");
    let first = paragraph.first_child().expect("the prefix makes text");
    let mut data = first.data_mut();
    let NodeValue::Text(text) = &mut data.value else {
        unreachable!("the prefix is plain text");
    };
    *text = text[2..].to_string().into();
    paragraph.detach();
    paragraph
}

/// The definition that each link of `loader` made by a reference uses, once each, sorted by label.
/// Each takes its link's current URL and title, which shows the edits of the earlier actions; a
/// link that an action took out of the tree, such as into the image of a table, counts too.
fn used_references(loader: &Loader<'_>) -> Vec<Reference> {
    let mut references: Vec<Reference> = Vec::new();
    let mut names = HashMap::<String, usize>::new();
    'links: for (node, label) in loader.references() {
        let data = node.data();
        let NodeValue::Link(link) = &data.value else {
            continue;
        };
        let title = if link.title.is_empty() {
            None
        } else {
            Some(link.title.clone())
        };
        let mut reference = Reference {
            label,
            url: link.url.clone(),
            title,
        };
        let original = reference.label.clone();
        let mut suffix = 1;
        loop {
            let key = super::refs::normalize_label(&reference.label);
            let Some(&index) = names.get(&key) else {
                names.insert(key, references.len());
                references.push(reference);
                break;
            };
            let registered = &references[index];
            let same_url = destination_key(&registered.url) == destination_key(&reference.url);
            if same_url && registered.title == reference.title {
                continue 'links;
            }
            suffix += 1;
            reference.label = format!("{original}-{suffix}");
        }
    }
    references.sort_by(|a, b| a.label.cmp(&b.label));
    references
}

/// Compare local paths after rebasing, without changing the spelling of output links.
fn destination_key(url: &str) -> (bool, String, String) {
    let local = super::embed_markdown::is_relative(url) || super::embed_markdown::is_root_path(url);
    if !local {
        return (false, url.to_string(), String::new());
    }
    let path = super::url_path(url);
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let (_, suffix) = super::split_suffix(url);
    (true, parts.join("/"), suffix.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::refs::Refs;

    #[test]
    fn test_reference_destination_keys() {
        let cases = [
            ("./shared.pdf", "sub/../shared.pdf", true),
            ("/shared.pdf", "shared.pdf", true),
            ("../shared.pdf", "../../shared.pdf", false),
            ("file%20name.pdf?q#f", "./file name.pdf?q#f", true),
            ("file%3Fname", "file?name", false),
            ("file.pdf?q=1", "file.pdf?q=2", false),
            ("file.pdf#one", "file.pdf#two", false),
            ("https://a.test/x", "https://b.test/x", false),
        ];
        for (first, second, expected) in cases {
            let first_key = destination_key(first);
            let second_key = destination_key(second);
            let same = first_key == second_key;
            assert_eq!(same, expected, "{first}, {second}");
        }
    }

    #[test]
    fn test_normalized_reference_names() -> anyhow::Result<()> {
        let arena = Arena::new();
        let loader = Loader::new(&arena, &[]);
        let refs = Refs::default();
        loader.load("[a b]\n\n[a b]: https://a.test\n", &refs)?;
        loader.load("[A  B]\n\n[A  B]: https://a.test\n", &refs)?;
        loader.load("[A B]\n\n[A B]: https://b.test\n", &refs)?;
        loader.load("[a b]\n\n[a b]: https://b.test \"title\"\n", &refs)?;
        let references = used_references(&loader);
        let actual: Vec<_> = references
            .into_iter()
            .map(|reference| (reference.label, reference.url, reference.title))
            .collect();
        let expected = vec![
            ("A B-2".into(), "https://b.test".into(), None),
            ("a b".into(), "https://a.test".into(), None),
            (
                "a b-3".into(),
                "https://b.test".into(),
                Some("title".into()),
            ),
        ];
        assert_eq!(actual, expected);
        Ok(())
    }

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
