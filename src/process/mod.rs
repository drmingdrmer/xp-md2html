//! The `process` subcommand: parse a markdown file, apply actions to its tree in order, print the tree.

pub mod download_images;
pub mod embed_markdown;
pub mod image_to_asset;
pub mod table_to_image;

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use comrak::Arena;
use comrak::Node;
use comrak::Options;

use crate::render::chrome::ChromeRenderer;

/// What an action reads and writes besides the tree.
pub struct ActionContext {
    /// The directory of the input file; a path in the markdown resolves against it, also `/x`.
    pub input_dir: PathBuf,
    /// The directory that receives the files the actions create.
    pub assets_dir: PathBuf,
    /// The directory of the output file; a link in the output is relative to it.
    pub output_dir: PathBuf,
    /// The output file's stem; it prefixes the names of the files the actions create.
    pub stem: String,
    /// Renders an HTML page to a PNG.
    pub renderer: ChromeRenderer,
}

/// One edit of the markdown tree, as `--action` names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Replace every table with a PNG of it.
    TableToImage,
    /// Download every `http(s)://` image into the assets dir and link the copy.
    DownloadImages,
    /// Replace a paragraph that holds only `![](x.md)` with the content of `x.md`.
    EmbedMarkdown,
    /// Copy every local image into the assets dir and link the copy.
    ImageToAsset,
}

impl FromStr for Action {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, String> {
        match name {
            "table-to-image" => Ok(Self::TableToImage),
            "download-images" => Ok(Self::DownloadImages),
            "embed-markdown" => Ok(Self::EmbedMarkdown),
            "image-to-asset" => Ok(Self::ImageToAsset),
            _ => Err(format!("unknown action: {name}")),
        }
    }
}

impl Action {
    /// Apply this action to the tree under `root`, which `arena` owns.
    pub fn apply<'a>(
        &self,
        arena: &'a Arena<'a>,
        root: Node<'a>,
        ctx: &ActionContext,
    ) -> anyhow::Result<()> {
        match self {
            Self::TableToImage => table_to_image::apply(arena, root, ctx),
            Self::DownloadImages => download_images::apply(root, ctx),
            Self::EmbedMarkdown => embed_markdown::apply(arena, root, ctx),
            Self::ImageToAsset => image_to_asset::apply(root, ctx),
        }
    }
}

/// The parser options: GFM plus `$` math, with the rules github.com uses.
pub fn gfm_math_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.autolink = true;
    options.extension.math_dollars = true;
    options.extension.math_code = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    options
}

/// Parse `markdown`, apply `actions` in order, and return the markdown of the edited tree.
pub fn process_markdown(
    markdown: &str,
    actions: &[Action],
    ctx: &ActionContext,
) -> anyhow::Result<String> {
    let options = gfm_math_options();
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, markdown, &options);

    for action in actions {
        action.apply(&arena, root, ctx)?;
    }

    let mut out = String::new();
    comrak::format_commonmark(root, &options, &mut out)?;
    Ok(out)
}

/// The path of `target` relative to the directory `base`, with `/` between the parts, for a link in a file under `base`.
pub fn relative_url(base: &Path, target: &Path) -> anyhow::Result<String> {
    let base = absolute_normalized(base)?;
    let target = absolute_normalized(target)?;

    let mut base_parts = base.components().peekable();
    let mut target_parts = target.components().peekable();
    while base_parts
        .peek()
        .is_some_and(|part| target_parts.peek() == Some(part))
    {
        base_parts.next();
        target_parts.next();
    }

    let mut parts: Vec<&str> = base_parts.map(|_| "..").collect();
    for part in target_parts {
        let part = part
            .as_os_str()
            .to_str()
            .with_context(|| format!("Path is not valid UTF-8: {}", target.display()))?;
        parts.push(part);
    }
    Ok(parts.join("/"))
}

/// `path` made absolute, with every `.` dropped and every `..` folded into the part before it.
fn absolute_normalized(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("Failed to resolve path: {}", path.display()))?;

    let mut normalized = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(part),
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_from_str() {
        let action = Action::from_str("table-to-image");
        assert_eq!(action, Ok(Action::TableToImage));

        let error = Action::from_str("shift");
        assert_eq!(error, Err("unknown action: shift".to_string()));
    }

    #[test]
    fn test_relative_url() -> anyhow::Result<()> {
        let same_dir = relative_url(Path::new("/a/b"), Path::new("/a/b/x.png"))?;
        assert_eq!(same_dir, "x.png");

        let sub_dir = relative_url(Path::new("/a/b"), Path::new("/a/b/assets/x.png"))?;
        assert_eq!(sub_dir, "assets/x.png");

        let sibling = relative_url(Path::new("/a/b/c"), Path::new("/a/img/x.png"))?;
        assert_eq!(sibling, "../../img/x.png");

        let folded = relative_url(Path::new("/a/b"), Path::new("/a/b/sub/.././x.png"))?;
        assert_eq!(folded, "x.png");
        Ok(())
    }
}
