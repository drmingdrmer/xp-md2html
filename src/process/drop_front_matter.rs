//! `drop-front-matter`: remove the `---` block at the top of the file, as md2zhihu does unless
//! `--keep-meta`.

use comrak::nodes::NodeValue;
use comrak::Node;

/// Detach the front matter of the file under `root`; comrak parses it only at the top, so it can
/// only be the first child.
pub fn apply(root: Node<'_>) {
    let Some(first) = root.first_child() else {
        return;
    };
    let is_front_matter = matches!(first.data().value, NodeValue::FrontMatter(_));
    if is_front_matter {
        first.detach();
    }
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// The `---` block at the top goes; a `---` rule later in the file stays.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "---\ntitle: T\n---\n\n# Title\n\nText.\n\n---\n\nEnd.\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(root);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, "# Title\n\nText.\n\n-----\n\nEnd.\n");
        Ok(())
    }
}
