//! `--refs` and the front matter keys `refs` and `platform_refs`: link reference definitions for
//! the references that the markdown does not define, as md2zhihu's `load_external_refs` and
//! `FrontMatter.get_refs` load them; and the parse that tells which links the references made.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::options::BrokenLinkReference;
use comrak::Arena;
use comrak::Node;
use comrak::ResolvedReference;
use fancy_regex::Regex;
use markdown::mdast;
use yaml_rust2::Yaml;
use yaml_rust2::YamlLoader;

/// The shape of a list of definitions, as an error message names it.
const LIST_FORM: &str = "must be a mapping of names to URLs, or a list of such mappings";

/// The definition `[label]: url "title"`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Definition {
    /// The label as the YAML or the markdown writes it.
    label: String,
    /// The URL as comrak reads it.
    url: String,
    /// The title as comrak reads it, or an empty one.
    title: String,
}

/// A parsed document and the source information needed when printing the edited tree.
pub(crate) struct Parsed<'a> {
    pub root: Node<'a>,
    pub references: Vec<(Node<'a>, String)>,
    pub emphasis: Vec<(Node<'a>, &'static str)>,
    pub undefined: Vec<String>,
}

/// Link reference definitions from outside the markdown file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refs {
    /// The definitions by the label as comrak looks it up; a later one replaces an earlier one.
    definitions: BTreeMap<String, Definition>,
    /// The platform whose lists apply, also in the front matter of an embedded file.
    platform: Option<String>,
}

impl Refs {
    /// Load the definitions that md2zhihu adds to `markdown` on `platform`, in this order, a later
    /// definition of a label replacing an earlier one: the `universal` list and the list named by
    /// `platform` of each file in `ref_files`, then the front matter's `refs` list and its
    /// `platform_refs.<platform>` list. Without a platform, only `universal` and `refs` apply.
    pub fn load(
        ref_files: &[PathBuf],
        markdown: &str,
        platform: Option<&str>,
    ) -> anyhow::Result<Self> {
        let mut refs = Self {
            platform: platform.map(str::to_string),
            ..Self::default()
        };
        for path in ref_files {
            refs.add_file(path, platform)?;
        }
        refs.add_front_matter(markdown, platform)?;
        Ok(refs)
    }

    /// The definitions for an embedded file whose content is `markdown`: these definitions, then
    /// the definitions of its front matter as [`Refs::load`] adds them, so its front matter
    /// replaces a definition of the same label.
    pub fn with_front_matter(&self, markdown: &str) -> anyhow::Result<Self> {
        let mut refs = self.clone();
        let platform = self.platform.as_deref();
        refs.add_front_matter(markdown, platform)?;
        Ok(refs)
    }

    /// Parse `markdown` into a tree in `arena`; a reference resolves with the markdown's own
    /// definition, else with these definitions. Return the tree, the used reference links and
    /// their labels in order, and the original delimiters of nested emphasis spans.
    /// `original` supplies warning text when math is masked; it must keep the same byte positions.
    pub(crate) fn parse<'a>(
        &self,
        arena: &'a Arena<'a>,
        markdown: &str,
        original: Option<&str>,
        options: &comrak::Options,
    ) -> anyhow::Result<Parsed<'a>> {
        let original = original.unwrap_or(markdown);
        if original.len() != markdown.len() {
            anyhow::bail!("Prepared Markdown must preserve source byte positions");
        }
        // comrak resolves a reference with the markdown's own definition and leaves no trace. So a
        // mark after the `[` of each such definition hides it from comrak: every reference then
        // goes to the callback, which gives a URL that names the label, and the walk after the
        // parse puts the definition's URL and title back. The mark is a noncharacter that the
        // markdown does not hold, so it marks nothing else.
        let Some(mark) = ('\u{FDD0}'..='\u{FDEF}').find(|mark| !markdown.contains(*mark)) else {
            anyhow::bail!("The markdown holds every noncharacter from U+FDD0 to U+FDEF");
        };

        // The first definition of a label wins, so the definitions go in from the last one, which
        // also keeps the offsets of the definitions before it.
        let mut refs = self.clone();
        let mut marked = markdown.to_string();
        let mut warning_source = original.to_string();
        let definitions = document_definitions(markdown, options.extension.autolink)?;
        for (offset, key, definition) in definitions.into_iter().rev() {
            marked.insert(offset + 1, mark);
            warning_source.insert(offset + 1, mark);
            refs.definitions.insert(key, definition);
        }

        let keys: HashSet<String> = refs.definitions.keys().cloned().collect();
        let callback = move |reference: BrokenLinkReference| {
            if !keys.contains(reference.normalized) {
                return None;
            }
            let url = format!("{mark}{}", reference.normalized);
            let resolved = ResolvedReference {
                url,
                title: String::new(),
            };
            Some(resolved)
        };
        let mut options = options.clone();
        options.parse.broken_link_callback = Some(Arc::new(callback));
        let root = comrak::parse_document(arena, &marked, &options);

        let mut links = Vec::new();
        for node in root.descendants() {
            let mut ast = node.data_mut();
            let is_link = matches!(ast.value, NodeValue::Link(_));
            match &mut ast.value {
                NodeValue::Link(link) | NodeValue::Image(link) => {
                    let Some(key) = link.url.strip_prefix(mark) else {
                        continue;
                    };
                    let definition = &refs.definitions[key];
                    link.url = definition.url.clone();
                    link.title = definition.title.clone();
                    if is_link {
                        links.push((node, definition.label.clone()));
                    }
                }
                // A definition can be text to comrak, such as one on the line after a `$$` formula:
                // the `markdown` crate ends a formula block there, while comrak's formula is inline
                // and its paragraph goes on. The text keeps no mark.
                NodeValue::Text(text) if text.contains(mark) => {
                    let unmarked = text.replace(mark, "");
                    *text = unmarked.into();
                }
                _ => {}
            }
        }
        let emphasis = super::emphasis::markers(root, &marked);
        let undefined = refs.undefined_references(root, &warning_source)?;
        Ok(Parsed {
            root,
            references: links,
            emphasis,
            undefined,
        })
    }

    fn undefined_references(&self, root: Node<'_>, markdown: &str) -> anyhow::Result<Vec<String>> {
        let eligible = warning_text(root, markdown);
        let pattern = Regex::new(r"!?\[((?:\\.|[^\[\]\\])*)\]\[((?:\\.|[^\[\]\\])*)\]")?;
        let mut undefined = Vec::new();
        for found in pattern.captures_iter(markdown) {
            let found = found?;
            let whole = found.get(0).unwrap();
            let first = found.get(1).unwrap();
            let second = found.get(2).unwrap();
            let bracket = whole.start() + usize::from(whole.as_str().starts_with('!'));
            if escaped_at(markdown, bracket) {
                continue;
            }
            let plain = [
                whole.start(),
                first.end(),
                second.start() - 1,
                whole.end() - 1,
            ];
            if !plain.iter().all(|index| eligible[*index]) {
                continue;
            }
            let second_label = second.as_str();
            let label = if second_label.is_empty() {
                first.as_str()
            } else {
                second_label
            };
            let key = normalize_label(label);
            if !self.definitions.contains_key(&key) {
                undefined.push(whole.as_str().to_string());
            }
        }
        Ok(undefined)
    }

    /// Add the `universal` list of the YAML file at `path`, then its list named by `platform`.
    fn add_file(&mut self, path: &Path, platform: Option<&str>) -> anyhow::Result<()> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("Failed to read refs file: {}", path.display()))?;
        let documents = YamlLoader::load_from_str(&text)
            .with_context(|| format!("Failed to parse refs file: {}", path.display()))?;
        let place = path.display().to_string();

        let root = documents.first().unwrap_or(&Yaml::Null);
        let is_mapping = matches!(root, Yaml::Null | Yaml::Hash(_));
        if !is_mapping {
            anyhow::bail!("{place}: must be a mapping");
        }

        self.add_list(&root["universal"], &format!("universal in {place}"))?;
        if let Some(platform) = platform {
            self.add_list(&root[platform], &format!("{platform} in {place}"))?;
        }
        Ok(())
    }

    /// Add the front matter's `refs` list, then its `platform_refs.<platform>` list.
    fn add_front_matter(&mut self, markdown: &str, platform: Option<&str>) -> anyhow::Result<()> {
        let Some(front_matter) = front_matter_yaml(markdown)? else {
            return Ok(());
        };
        // Front matter that is not a mapping, such as one word, holds no refs, as in md2zhihu.
        let is_mapping = matches!(front_matter, Yaml::Hash(_));
        if !is_mapping {
            return Ok(());
        }

        self.add_list(&front_matter["refs"], "refs in the front matter")?;

        let platform_refs = &front_matter["platform_refs"];
        let is_mapping = matches!(platform_refs, Yaml::BadValue | Yaml::Null | Yaml::Hash(_));
        if !is_mapping {
            anyhow::bail!("platform_refs in the front matter: must be a mapping");
        }
        if let Some(platform) = platform {
            let place = format!("platform_refs.{platform} in the front matter");
            self.add_list(&platform_refs[platform], &place)?;
        }
        Ok(())
    }

    /// Add the definitions of `list`, a mapping of labels to values or a list of such mappings;
    /// a missing or empty `list` adds none. `place` names `list` in an error.
    fn add_list(&mut self, list: &Yaml, place: &str) -> anyhow::Result<()> {
        let mappings = match list {
            Yaml::BadValue | Yaml::Null => return Ok(()),
            Yaml::Hash(_) => std::slice::from_ref(list),
            Yaml::Array(items) => items.as_slice(),
            _ => anyhow::bail!("{place}: {LIST_FORM}"),
        };
        for mapping in mappings {
            let Yaml::Hash(entries) = mapping else {
                anyhow::bail!("{place}: {LIST_FORM}");
            };
            for (label, value) in entries {
                let (Yaml::String(label), Yaml::String(value)) = (label, value) else {
                    anyhow::bail!("{place}: {LIST_FORM}");
                };
                self.add(label, value, place)?;
            }
        }
        Ok(())
    }

    /// Add `[label]: value`, replacing a definition of the same label.
    fn add(&mut self, label: &str, value: &str, place: &str) -> anyhow::Result<()> {
        let Some((url, title)) = resolve(value) else {
            anyhow::bail!("{label} in {place}: {value:?} is not a URL with an optional title");
        };
        let definition = Definition {
            label: label.to_string(),
            url,
            title,
        };
        self.definitions.insert(normalize_label(label), definition);
        Ok(())
    }
}

fn escaped_at(markdown: &str, index: usize) -> bool {
    let preceding = markdown[..index].bytes().rev();
    let backslashes = preceding.take_while(|byte| *byte == b'\\').count();
    backslashes % 2 == 1
}

/// Source bytes that belong to plain inline content, outside code, links and HTML anchors.
fn warning_text(root: Node<'_>, markdown: &str) -> Vec<bool> {
    let mut lines = vec![0];
    let starts = markdown.match_indices('\n').map(|(index, _)| index + 1);
    lines.extend(starts);
    let mut eligible = vec![false; markdown.len()];
    let mut html_depth: usize = 0;
    for node in root.descendants() {
        let data = node.data();
        if let NodeValue::HtmlInline(html) = &data.value {
            match html_link_tag(html) {
                Some(true) => html_depth = html_depth.saturating_sub(1),
                Some(false) => html_depth += 1,
                None => {}
            }
        }
        if html_depth == 0 && warning_node(node) {
            let start = data.sourcepos.start;
            let end = data.sourcepos.end;
            let start = lines[start.line - 1] + start.column - 1;
            let end = lines[end.line - 1] + end.column;
            eligible[start..end].fill(true);
        }
    }
    eligible
}

fn warning_node(node: Node<'_>) -> bool {
    if !matches!(node.data().value, NodeValue::Text(_)) {
        return false;
    }
    let opaque = node.ancestors().any(|parent| {
        matches!(
            parent.data().value,
            NodeValue::Link(_) | NodeValue::Image(_) | NodeValue::Escaped
        )
    });
    !opaque
}

/// Whether an HTML inline opens or closes an anchor; `<abbr>` is a different tag.
fn html_link_tag(html: &str) -> Option<bool> {
    let html = html.trim_start().to_ascii_lowercase();
    let (closing, after) = if let Some(after) = html.strip_prefix("</a") {
        (true, after)
    } else {
        let after = html.strip_prefix("<a")?;
        (false, after)
    };
    let boundary = after.starts_with('>') || after.starts_with(char::is_whitespace);
    if boundary {
        Some(closing)
    } else {
        None
    }
}

/// The YAML of the front matter of `markdown`, as comrak finds the front matter.
fn front_matter_yaml(markdown: &str) -> anyhow::Result<Option<Yaml>> {
    let arena = Arena::new();
    let options = super::gfm_math_options();
    let root = comrak::parse_document(&arena, markdown, &options);
    let Some(first) = root.first_child() else {
        return Ok(None);
    };
    let ast = first.data();
    let NodeValue::FrontMatter(text) = &ast.value else {
        return Ok(None);
    };
    // The text holds the `---` lines too, which YAML reads as the start of a document.
    let documents =
        YamlLoader::load_from_str(text).context("Failed to parse the front matter as YAML")?;
    let yaml = documents.into_iter().next();
    Ok(yaml)
}

/// Each definition in `markdown`, with the offset of its `[` and the label as comrak looks it up, in
/// order. comrak drops the definitions from the tree, so the `markdown` crate finds them, with the
/// same GFM, math and front matter syntax.
fn document_definitions(
    markdown: &str,
    autolink: bool,
) -> anyhow::Result<Vec<(usize, String, Definition)>> {
    let options = markdown::ParseOptions {
        constructs: markdown::Constructs {
            frontmatter: true,
            math_flow: true,
            math_text: true,
            gfm_autolink_literal: autolink,
            ..markdown::Constructs::gfm()
        },
        ..markdown::ParseOptions::gfm()
    };
    let tree = markdown::to_mdast(markdown, &options)
        .map_err(|message| anyhow::anyhow!("Failed to parse markdown: {message}"))?;

    let mut definitions = Vec::new();
    collect_definitions(&tree, markdown, &mut definitions)?;
    Ok(definitions)
}

/// Add each definition under `node`, with the offset of its `[` in `markdown` and the label as
/// comrak looks it up, to `definitions`.
fn collect_definitions(
    node: &mdast::Node,
    markdown: &str,
    definitions: &mut Vec<(usize, String, Definition)>,
) -> anyhow::Result<()> {
    if let mdast::Node::Definition(definition) = node {
        let label = definition.label.clone();
        let mut found = Definition {
            label: label.unwrap_or_else(|| definition.identifier.clone()),
            url: definition.url.clone(),
            title: definition.title.clone().unwrap_or_default(),
        };
        // The position starts at the indent before the `[`.
        let position = definition
            .position
            .as_ref()
            .with_context(|| format!("The definition of {} has no position", found.label))?;
        let start = position.start.offset;
        let indent = markdown[start..]
            .find('[')
            .with_context(|| format!("The definition of {} has no [", found.label))?;
        let offset = start + indent;

        // The `markdown` crate decodes the escapes in the label, while comrak looks a reference up
        // by the label as written: up to the first `]` that no backslash escapes.
        let written = written_label(&markdown[offset..])
            .with_context(|| format!("The definition of {} has no ]", found.label))?;
        let key = normalize_label(written);
        found.label = written.to_string();
        definitions.push((offset, key, found));
    }

    let Some(children) = node.children() else {
        return Ok(());
    };
    for child in children {
        collect_definitions(child, markdown, definitions)?;
    }
    Ok(())
}

/// The label between the `[` that starts `text` and the first `]` that no backslash escapes.
fn written_label(text: &str) -> Option<&str> {
    let mut escaped = false;
    for (index, c) in text.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ']' {
            return Some(&text[1..index]);
        }
    }
    None
}

/// The URL and the title of the definition `[label]: value`, as comrak reads them, or `None` when
/// `value` is not a URL with an optional title.
fn resolve(value: &str) -> Option<(String, String)> {
    if value.contains('\n') {
        return None;
    }
    let markdown = format!("[label]: {value}\n\n[label]\n");
    let arena = Arena::new();
    let options = comrak::Options::default();
    let root = comrak::parse_document(&arena, &markdown, &options);
    for node in root.descendants() {
        let ast = node.data();
        if let NodeValue::Link(link) = &ast.value {
            return Some((link.url.clone(), link.title.clone()));
        }
    }
    None
}

/// `label` as comrak looks a reference up: trimmed, with one space for each run of whitespace,
/// and case-folded.
pub(super) fn normalize_label(label: &str) -> String {
    let words: Vec<&str> = label.split_whitespace().collect();
    let joined = words.join(" ");
    caseless::default_case_fold_str(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undefined_references() -> anyhow::Result<()> {
        let markdown = "[text][missing] [collapsed][] ![image][absent] ![alt][] [shortcut]\n\n\
            [*formatted*][unknown] [known][OK] [OK][] [OK] [math $secret$][missing]\n\n\
            `[code][missing]` \\[escaped][missing] $[math][missing]$\n\n\
            [outer [inner][missing]](https://example.invalid)\n\n\
            <a href=\"u\">[html][missing]</a> <A href=\"u\">![html][]</A>\n\n\
            <abbr>[after][missing]</abbr>\n\n\
            ```\n[code][missing]\n```\n\n[ok]: https://example.invalid\n";
        let arena = Arena::new();
        let refs = Refs::default();
        let options = super::super::gfm_math_options();
        let prepared = super::super::math_syntax::prepare(markdown, &options)?;
        let parsed = refs.parse(&arena, &prepared.markdown, Some(markdown), &options)?;
        let expected = [
            "[text][missing]",
            "[collapsed][]",
            "![image][absent]",
            "![alt][]",
            "[*formatted*][unknown]",
            "[math $secret$][missing]",
            "[after][missing]",
        ];
        assert_eq!(parsed.undefined, expected);
        Ok(())
    }

    /// A later definition of a label replaces an earlier one: the files' `universal` lists, then
    /// their lists named by the platform, then the front matter's `refs` and
    /// `platform_refs.<platform>`; a list may be one mapping.
    #[test]
    fn test_load() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let first = dir.path().join("first.yaml");
        fs::write(
            &first,
            "universal:\n  - a: http://u/a\n  - B: http://u/b \"Bee\"\n\
             zhihu:\n  b: http://z/b\ngithub:\n  c: http://g/c\n",
        )?;
        let second = dir.path().join("second.yaml");
        fs::write(&second, "universal:\n  c: http://u2/c\n  d: http://u2/d\n")?;
        let markdown = "---\ntitle: T\nrefs:\n  - d: http://f/d\n  - e: http://f/e\n\
                        platform_refs:\n  zhihu:\n    - e: http://fz/e\n---\n\nText.\n";

        let refs = Refs::load(&[first, second], markdown, Some("zhihu"))?;

        let text = format_parsed(&refs, "[a] [b] [c] [d] [e]\n")?;
        let expected =
            "[a](http://u/a) [b](http://z/b) [c](http://u2/c) [d](http://f/d) [e](http://fz/e)\n";
        assert_eq!(text, expected);
        Ok(())
    }

    /// Without a platform, only the `universal` and the front matter's `refs` lists apply.
    #[test]
    fn test_load_without_platform() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("refs.yaml");
        fs::write(
            &file,
            "universal:\n  a: http://u/a\nzhihu:\n  b: http://z/b\n",
        )?;
        let markdown =
            "---\nrefs:\n  c: http://f/c\nplatform_refs:\n  zhihu:\n    d: http://fz/d\n---\n";

        let refs = Refs::load(&[file], markdown, None)?;

        let text = format_parsed(&refs, "[a] [b] [c] [d]\n")?;
        assert_eq!(text, "[a](http://u/a) \\[b\\] [c](http://f/c) \\[d\\]\n");
        Ok(())
    }

    #[test]
    fn test_load_errors() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("refs.yaml");

        fs::write(&file, "- a\n")?;
        let error = Refs::load(std::slice::from_ref(&file), "", None).unwrap_err();
        let expected = format!("{}: must be a mapping", file.display());
        assert_eq!(error.to_string(), expected);

        fs::write(&file, "universal: http://u/a\n")?;
        let error = Refs::load(std::slice::from_ref(&file), "", None).unwrap_err();
        let expected = format!("universal in {}: {LIST_FORM}", file.display());
        assert_eq!(error.to_string(), expected);

        fs::write(&file, "zhihu:\n  a: not a url\n")?;
        let error = Refs::load(std::slice::from_ref(&file), "", Some("zhihu")).unwrap_err();
        let expected = format!(
            "a in zhihu in {}: \"not a url\" is not a URL with an optional title",
            file.display()
        );
        assert_eq!(error.to_string(), expected);

        let markdown = "---\nplatform_refs: [a]\n---\n";
        let error = Refs::load(&[], markdown, None).unwrap_err();
        let expected = "platform_refs in the front matter: must be a mapping";
        assert_eq!(error.to_string(), expected);

        // Front matter that is not a mapping holds no refs.
        let refs = Refs::load(&[], "---\nword\n---\n", None)?;
        assert_eq!(refs, Refs::default());
        Ok(())
    }

    /// An embedded file's front matter replaces a definition of the same label, also in its
    /// `platform_refs` list of the platform that `Refs::load` got.
    #[test]
    fn test_with_front_matter() -> anyhow::Result<()> {
        let outer = "---\nrefs:\n  a: http://o/a\n  b: http://o/b\n---\n";
        let refs = Refs::load(&[], outer, Some("zhihu"))?;

        let inner =
            "---\nrefs:\n  b: http://i/b\nplatform_refs:\n  zhihu:\n    c: http://iz/c\n---\n";
        let embedded = refs.with_front_matter(inner)?;

        let text = format_parsed(&embedded, "[a] [b] [c]\n")?;
        assert_eq!(text, "[a](http://o/a) [b](http://i/b) [c](http://iz/c)\n");
        Ok(())
    }

    /// A reference resolves with the markdown's own definition, else with the refs, by a label in
    /// any case and spacing. The parse returns each link that a reference made, with the label of
    /// its definition as written, and no image.
    #[test]
    fn test_parse() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("refs.yaml");
        fs::write(
            &file,
            "universal:\n  Big  Name: <http://u/big> \"Big\"\n  own: http://u/own\n",
        )?;
        let refs = Refs::load(&[file], "", None)?;

        let markdown = "[a][big name], [own], ![i][own] and [none].\n\n[Own]: http://f/own\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let Parsed {
            root,
            references: links,
            ..
        } = refs.parse(&arena, markdown, None, &options)?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "[a](http://u/big \"Big\"), [own](http://f/own), ![i](http://f/own) and \
                        \\[none\\].\n";
        assert_eq!(out, expected);

        let mut found = Vec::new();
        for (node, label) in links {
            let data = node.data();
            let NodeValue::Link(link) = &data.value else {
                continue;
            };
            found.push((label, link.url.clone()));
        }
        let expected_found = vec![
            ("Big  Name".to_string(), "http://u/big".to_string()),
            ("Own".to_string(), "http://f/own".to_string()),
        ];
        assert_eq!(found, expected_found);
        Ok(())
    }

    /// Without refs, the parse gives the tree that comrak gives: for the definitions in a block
    /// quote, a list and after non-ASCII text, with escapes, entities, a title on its own line and a
    /// repeated label, and for a definition that comrak reads as text after a `$$` formula.
    #[test]
    fn test_parse_like_comrak() -> anyhow::Result<()> {
        let markdown = "---\ntitle: T\n---\n\n\
                        图 [x][A], [b], [c][], ![i][a], [a\\*b] and [d].\n\n\
                        > [a]: <http://a.com/x y> \"T &amp; U\"\n\
                        >   [b]: http://b\\_c.com\n\n\
                        - [c]:\n  http://c.com\n  'C'\n\n\
                        [a\\*b]: http://ab.com\n\
                        [a]: http://dup.com\n\n\
                        ```\n[d]: http://code.com\n```\n\n\
                        [d]: http://d.com\n\n\
                        $$\nx\n$$\n[q]: http://q.com\n";
        let options = super::super::gfm_math_options();
        let arena = Arena::new();
        let comrak_root = comrak::parse_document(&arena, markdown, &options);
        let mut expected = String::new();
        comrak::format_commonmark(comrak_root, &options, &mut expected)?;

        let Parsed {
            root,
            references: links,
            ..
        } = Refs::default().parse(&arena, markdown, None, &options)?;
        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, expected);

        let labels: Vec<String> = links.into_iter().map(|(_, label)| label).collect();
        assert_eq!(labels, ["a", "b", "c", "a\\*b", "d"]);
        Ok(())
    }

    /// The markdown of the tree that `refs` parses from `markdown`.
    fn format_parsed(refs: &Refs, markdown: &str) -> anyhow::Result<String> {
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let Parsed { root, .. } = refs.parse(&arena, markdown, None, &options)?;
        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        Ok(out)
    }
}
