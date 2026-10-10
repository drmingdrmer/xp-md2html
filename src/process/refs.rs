//! `--refs` and the front matter keys `refs` and `platform_refs`: link reference definitions for
//! the references that the markdown does not define, as md2zhihu's `load_external_refs` and
//! `FrontMatter.get_refs` load them.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::options::BrokenLinkCallback;
use comrak::options::BrokenLinkReference;
use comrak::Arena;
use comrak::ResolvedReference;
use yaml_rust2::Yaml;
use yaml_rust2::YamlLoader;

/// The shape of a list of definitions, as an error message names it.
const LIST_FORM: &str = "must be a mapping of names to URLs, or a list of such mappings";

/// The definition `[label]: value`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Definition {
    /// The label as the YAML writes it.
    label: String,
    /// The URL and the optional title in markdown, such as `https://grpc.io "gRPC"`.
    value: String,
    /// The URL that comrak reads from `value`.
    url: String,
    /// The title that comrak reads from `value`, or an empty one.
    title: String,
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

    /// The callback that resolves a reference that the markdown does not define.
    pub fn broken_link_callback(&self) -> Arc<dyn BrokenLinkCallback> {
        let mut resolved: HashMap<String, ResolvedReference> = HashMap::new();
        for (key, definition) in &self.definitions {
            let reference = ResolvedReference {
                url: definition.url.clone(),
                title: definition.title.clone(),
            };
            resolved.insert(key.clone(), reference);
        }
        let callback = move |reference: BrokenLinkReference| {
            let found = resolved.get(reference.normalized)?;
            Some(found.clone())
        };
        Arc::new(callback)
    }

    /// `markdown` followed by the line `[label]: value` of each definition, for a parser that takes
    /// no callback; the markdown's own definition of a label comes first, so it wins.
    pub fn append_definitions(&self, markdown: &str) -> String {
        let mut text = markdown.to_string();
        if self.definitions.is_empty() {
            return text;
        }
        // A blank line ends the last block of the markdown.
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
        for definition in self.definitions.values() {
            let line = format!("[{}]: {}\n", definition.label, definition.value);
            text.push_str(&line);
        }
        text
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
            value: value.to_string(),
            url,
            title,
        };
        self.definitions.insert(normalize_label(label), definition);
        Ok(())
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
fn normalize_label(label: &str) -> String {
    let words: Vec<&str> = label.split_whitespace().collect();
    let joined = words.join(" ");
    caseless::default_case_fold_str(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let text = refs.append_definitions("Text.\n");
        let expected = "Text.\n\n\
                        [a]: http://u/a\n\
                        [b]: http://z/b\n\
                        [c]: http://u2/c\n\
                        [d]: http://f/d\n\
                        [e]: http://fz/e\n";
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

        let text = refs.append_definitions("");
        assert_eq!(text, "\n\n[a]: http://u/a\n[c]: http://f/c\n");
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

        let text = embedded.append_definitions("");
        assert_eq!(
            text,
            "\n\n[a]: http://o/a\n[b]: http://i/b\n[c]: http://iz/c\n"
        );
        Ok(())
    }

    /// comrak resolves a reference that the markdown does not define with the callback, by a label
    /// in any case and spacing; the markdown's own definition wins.
    #[test]
    fn test_broken_link_callback() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("refs.yaml");
        fs::write(
            &file,
            "universal:\n  Big  Name: <http://u/big> \"Big\"\n  own: http://u/own\n",
        )?;
        let refs = Refs::load(&[file], "", None)?;

        let markdown = "[a][big name], [own] and [none].\n\n[own]: http://f/own\n";
        let mut options = super::super::gfm_math_options();
        options.parse.broken_link_callback = Some(refs.broken_link_callback());
        let arena = Arena::new();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "[a](http://u/big \"Big\"), [own](http://f/own) and \\[none\\].\n";
        assert_eq!(out, expected);
        Ok(())
    }
}
