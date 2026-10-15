//! `link-to-asset`: copy every local file that a link names into the assets dir and link the copy,
//! as md2zhihu's `store_linked_file` does. The copy keeps the file's name, which the link
//! percent-encodes, where md2zhihu writes a `-` for each space, `%`, `#` and `?` in it.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Node;

use super::ActionContext;

/// The extensions of a markdown file; a link to one names another article, so it stays as written.
const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

/// Copy each local file that a link under `root` names into `ctx.assets_dir` under the file's own
/// name, which a download keeps, and link the copy with the link's query and fragment, such as the
/// `#page=2` of a PDF.
///
/// A link stays as written when it names no file, such as a web URL, an anchor, a page path or a
/// directory, when it names a markdown file, another article, and when it links an asset. Two files
/// of one name fail the copy when their contents differ, because one copy would replace the other.
pub fn apply(root: Node<'_>, ctx: &ActionContext) -> anyhow::Result<()> {
    // The file that each copy holds, by the copy's path.
    let mut sources = HashMap::new();
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Link(link) = &mut ast.value else {
            continue;
        };
        let Some(source) = linked_file(ctx, &link.url) else {
            continue;
        };
        let path = copy(&source, &ctx.assets_dir, &mut sources)?;
        let copy_link = ctx.link_to(&path)?;
        let (_, suffix) = super::split_suffix(&link.url);
        link.url = format!("{copy_link}{suffix}");
    }
    Ok(())
}

/// The local file that the link URL `url` names, when the link takes a copy: a file other than
/// markdown, which no action created or copied. None for any other URL.
fn linked_file(ctx: &ActionContext, url: &str) -> Option<PathBuf> {
    if ctx.is_asset(url) {
        return None;
    }
    let file = ctx.local_file(url)?;
    let is_file = file.is_file();
    if !is_file || is_markdown(&file) {
        return None;
    }
    Some(file)
}

/// Whether `file` has the extension of a markdown file, in any case.
fn is_markdown(file: &Path) -> bool {
    let Some(extension) = file.extension().and_then(OsStr::to_str) else {
        return false;
    };
    MARKDOWN_EXTENSIONS
        .iter()
        .any(|markdown| extension.eq_ignore_ascii_case(markdown))
}

/// Copy `source` into `assets_dir` under its own name, and return the copy's path. `sources` holds
/// the file of each copy made before, and gets this one; a file whose content differs from the
/// earlier copy of its name fails.
fn copy(
    source: &Path,
    assets_dir: &Path,
    sources: &mut HashMap<PathBuf, PathBuf>,
) -> anyhow::Result<PathBuf> {
    let content = fs::read(source)
        .with_context(|| format!("Failed to read linked file: {}", source.display()))?;
    let name = source
        .file_name()
        .with_context(|| format!("Linked file has no name: {}", source.display()))?;
    let path = assets_dir.join(name);

    if let Some(earlier) = sources.get(&path) {
        let copied = fs::read(&path)
            .with_context(|| format!("Failed to read linked file: {}", path.display()))?;
        if copied != content {
            anyhow::bail!(
                "Linked files {} and {} differ, but both would be copied to {}",
                earlier.display(),
                source.display(),
                path.display()
            );
        }
        return Ok(path);
    }

    fs::write(&path, &content)
        .with_context(|| format!("Failed to write linked file: {}", path.display()))?;
    sources.insert(path.clone(), source.to_path_buf());
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The copy keeps the file's name. A file of the same name and content shares the copy; one of
    /// other content fails and leaves the copy.
    #[test]
    fn test_copy() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let assets_dir = dir.path().join("assets");
        fs::create_dir_all(&assets_dir)?;
        let mut files = Vec::new();
        for (sub_dir, content) in [("a", "A"), ("b", "A"), ("c", "C")] {
            fs::create_dir_all(dir.path().join(sub_dir))?;
            let file = dir.path().join(sub_dir).join("x.txt");
            fs::write(&file, content)?;
            files.push(file);
        }
        let mut sources = HashMap::new();

        let path = copy(&files[0], &assets_dir, &mut sources)?;
        assert_eq!(path, assets_dir.join("x.txt"));
        let copied = fs::read(&path)?;
        assert_eq!(copied, b"A");

        let same_content = copy(&files[1], &assets_dir, &mut sources)?;
        assert_eq!(same_content, path);

        let error = copy(&files[2], &assets_dir, &mut sources).unwrap_err();
        let expected_error = format!(
            "Linked files {} and {} differ, but both would be copied to {}",
            files[0].display(),
            files[2].display(),
            path.display()
        );
        assert_eq!(error.to_string(), expected_error);
        let copied = fs::read(&path)?;
        assert_eq!(copied, b"A");
        Ok(())
    }

    #[test]
    fn test_is_markdown() {
        let files = ["a/b.md", "b.MD", "b.markdown", "b.mdx", "md", "b.md.txt"];
        let mut markdown = Vec::new();
        for file in files {
            let is_md = is_markdown(Path::new(file));
            if is_md {
                markdown.push(file);
            }
        }
        assert_eq!(markdown, ["a/b.md", "b.MD", "b.markdown"]);
    }
}
