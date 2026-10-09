//! `embed-markdown`: replace a paragraph that holds only an image, such as `![](x.md)`, with the
//! content of the markdown file at the image's URL.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;
use fancy_regex::Regex;

use super::ActionContext;

/// The regex of the image URLs that md2zhihu embeds when its `--embed` names none: every URL that
/// ends with `.md`.
pub const DEFAULT_PATTERN: &str = "[.]md$";

/// Replace every paragraph under `root` that holds only an image whose URL a regex of
/// `ctx.embed_patterns` matches with the content of the file at the URL.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>, ctx: &ActionContext) -> anyhow::Result<()> {
    let dir = &ctx.input_dir;
    embed(arena, root, dir, dir, &ctx.embed_patterns, &mut Vec::new())
}

/// Embed into the tree under `root`, whose file sits in `base_dir`; `/x` resolves against `root_dir`,
/// the input file's directory. A paragraph is embedded when it holds only an image whose URL a regex
/// of `patterns` matches somewhere.
///
/// `chain` holds the canonical paths of the files being embedded, outermost first, to catch a cycle.
/// The embedded file's own embeds are resolved first, then its image and link URLs are rebased to
/// `base_dir`, and its front matter is dropped.
pub fn embed<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    root_dir: &Path,
    base_dir: &Path,
    patterns: &[Regex],
    chain: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    // Edits while walking would confuse the walk, so collect the paragraphs first.
    let mut embeds = Vec::new();
    for node in root.descendants() {
        if let Some(url) = embedded_url(node, patterns)? {
            embeds.push((node, url));
        }
    }

    for (paragraph, url) in embeds {
        let path = resolve(root_dir, base_dir, &url);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read embedded markdown: {}", path.display()))?;
        let identity = fs::canonicalize(&path)?;
        if chain.contains(&identity) {
            let mut cycle: Vec<String> = chain.iter().map(|p| p.display().to_string()).collect();
            cycle.push(identity.display().to_string());
            anyhow::bail!("Embed cycle: {}", cycle.join(" -> "));
        }
        let embedded_dir = path
            .parent()
            .with_context(|| format!("Embedded path has no directory: {}", path.display()))?;

        let options = super::gfm_math_options();
        let embedded = comrak::parse_document(arena, &text, &options);

        chain.push(identity);
        embed(arena, embedded, root_dir, embedded_dir, patterns, chain)?;
        chain.pop();
        rebase_urls(embedded, embedded_dir, base_dir)?;

        let children: Vec<Node<'a>> = embedded.children().collect();
        for child in children {
            let is_front_matter = matches!(child.data().value, NodeValue::FrontMatter(_));
            if is_front_matter {
                continue;
            }
            paragraph.insert_before(child);
        }
        paragraph.detach();
    }
    Ok(())
}

/// The URL of the image when `node` is a paragraph that holds only an image whose URL a regex of
/// `patterns` matches somewhere.
fn embedded_url(node: Node<'_>, patterns: &[Regex]) -> anyhow::Result<Option<String>> {
    let is_paragraph = matches!(node.data().value, NodeValue::Paragraph);
    if !is_paragraph {
        return Ok(None);
    }

    let mut children = node.children();
    let Some(child) = children.next() else {
        return Ok(None);
    };
    if children.next().is_some() {
        return Ok(None);
    }

    let data = child.data();
    let NodeValue::Image(link) = &data.value else {
        return Ok(None);
    };
    for pattern in patterns {
        let found = pattern
            .is_match(&link.url)
            .with_context(|| format!("Failed to match the regex {pattern}: {}", link.url))?;
        if found {
            return Ok(Some(link.url.clone()));
        }
    }
    Ok(None)
}

/// `/x` is relative to `root_dir`, the input file's directory; any other path is relative to
/// `base_dir`, the directory of the file that holds the link.
pub(super) fn resolve(root_dir: &Path, base_dir: &Path, url: &str) -> PathBuf {
    if let Some(from_root) = url.strip_prefix('/') {
        return root_dir.join(from_root);
    }
    base_dir.join(url)
}

/// Make every relative image and link URL under `root`, which is relative to `from_dir`, relative to `to_dir`.
fn rebase_urls(root: Node<'_>, from_dir: &Path, to_dir: &Path) -> anyhow::Result<()> {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let link = match &mut ast.value {
            NodeValue::Image(link) => link,
            NodeValue::Link(link) => link,
            _ => continue,
        };
        if !is_relative(&link.url) {
            continue;
        }
        let target = from_dir.join(&link.url);
        link.url = super::relative_url(to_dir, &target)?;
    }
    Ok(())
}

/// A URL that is a root path `/x`; not a protocol-relative URL `//x`, which names a host.
pub(super) fn is_root_path(url: &str) -> bool {
    url.starts_with('/') && !url.starts_with("//")
}

/// A URL that is a relative path: not a scheme (`https:`, `mailto:`), a root path `/x`, or an anchor `#x`.
pub(super) fn is_relative(url: &str) -> bool {
    if url.starts_with('/') || url.starts_with('#') {
        return false;
    }
    let Some((scheme, _)) = url.split_once(':') else {
        return true;
    };
    let is_scheme = !scheme.contains('/')
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c));
    !is_scheme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_relative() {
        assert!(is_relative("a.md"));
        assert!(is_relative("sub/b.md"));
        assert!(is_relative("../x.png"));
        assert!(is_relative("a/b:c.png"));
        assert!(!is_relative("https://h/a.png"));
        assert!(!is_relative("mailto:a@b"));
        assert!(!is_relative("/root.png"));
        assert!(!is_relative("#top"));
    }

    /// `b.md` sits in `sub/`, drops its front matter, keeps absolute URLs, rebases relative ones,
    /// and embeds `/x.md` from the input file's directory, whose URL is rebased twice.
    #[test]
    fn test_embed() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir(dir.path().join("sub"))?;
        fs::write(
            dir.path().join("sub/b.md"),
            "---\ntitle: b\n---\n\nB ![p](img/p.png) [c](c.md) [h](https://h/) [t](#top)\n\n![](/x.md)\n",
        )?;
        fs::write(dir.path().join("x.md"), "X ![y](y.png)\n")?;

        let markdown = "# A\n\n![](sub/b.md)\n\n- item\n\n  ![](sub/b.md)\n\nEnd.\n";
        let options = super::super::gfm_math_options();
        let arena = Arena::new();
        let root = comrak::parse_document(&arena, markdown, &options);

        embed_md(&arena, root, dir.path())?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        // comrak indents the blank lines inside a list item.
        let expected = "# A\n\n\
            B ![p](sub/img/p.png) [c](sub/c.md) [h](https://h/) [t](#top)\n\n\
            X ![y](y.png)\n\n\
            - item\n  \n  \
              B ![p](sub/img/p.png) [c](sub/c.md) [h](https://h/) [t](#top)\n  \n  \
              X ![y](y.png)\n\n\
            End.\n";
        assert_eq!(out, expected);
        Ok(())
    }

    #[test]
    fn test_embed_cycle() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("a.md"), "![](b.md)\n")?;
        fs::write(dir.path().join("b.md"), "![](a.md)\n")?;

        let options = super::super::gfm_math_options();
        let arena = Arena::new();
        let root = comrak::parse_document(&arena, "![](a.md)\n", &options);

        let result = embed_md(&arena, root, dir.path());
        let message = result.unwrap_err().to_string();
        let canonical = fs::canonicalize(dir.path())?;
        let expected = format!(
            "Embed cycle: {0}/a.md -> {0}/b.md -> {0}/a.md",
            canonical.display()
        );
        assert_eq!(message, expected);
        Ok(())
    }

    #[test]
    fn test_embed_missing_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let options = super::super::gfm_math_options();
        let arena = Arena::new();
        let root = comrak::parse_document(&arena, "![](none.md)\n", &options);

        let result = embed_md(&arena, root, dir.path());
        let message = result.unwrap_err().to_string();
        let expected = format!(
            "Failed to read embedded markdown: {}",
            dir.path().join("none.md").display()
        );
        assert_eq!(message, expected);
        Ok(())
    }

    /// `/x.md` resolves against the input file's directory, not the current directory, which has no `x.md`.
    #[test]
    fn test_embed_root_path() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("x.md"), "X text.\n")?;
        assert!(!Path::new("x.md").exists());

        let options = super::super::gfm_math_options();
        let arena = Arena::new();
        let root = comrak::parse_document(&arena, "![](/x.md)\n", &options);

        embed_md(&arena, root, dir.path())?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, "X text.\n");
        Ok(())
    }

    /// An image is embedded when a regex matches somewhere in its URL, also in an embedded file; a
    /// `.md` image that no regex matches stays.
    #[test]
    fn test_embed_patterns() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("a.txt"), "A\n\n![](c.inc)\n")?;
        fs::write(dir.path().join("c.inc"), "C\n")?;

        let options = super::super::gfm_math_options();
        let arena = Arena::new();
        let root = comrak::parse_document(&arena, "![](a.txt)\n\n![](b.md)\n", &options);

        let patterns = [Regex::new("[.]txt$")?, Regex::new("inc")?];
        let path = dir.path();
        embed(&arena, root, path, path, &patterns, &mut Vec::new())?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, "A\n\nC\n\n![](b.md)\n");
        Ok(())
    }

    /// `embed` into a file in `dir` with md2zhihu's default regex, which embeds every `.md` URL.
    fn embed_md<'a>(arena: &'a Arena<'a>, root: Node<'a>, dir: &Path) -> anyhow::Result<()> {
        let patterns = [Regex::new(DEFAULT_PATTERN)?];
        embed(arena, root, dir, dir, &patterns, &mut Vec::new())
    }
}
