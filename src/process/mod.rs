//! The `process` subcommand: parse a markdown file, apply actions to its tree in order, print the tree.

pub mod append_reference_list;
pub mod code_to_image;
pub mod codespan_to_text;
pub mod download_images;
pub mod drop_front_matter;
pub mod embed_markdown;
pub mod flatten_lists;
pub mod graphviz_to_image;
pub mod image_to_asset;
pub mod join_math_block;
pub mod math_block_to_one_line;
pub mod math_inline_to_text;
pub mod math_to_image;
pub mod math_to_img_tag;
pub mod mermaid_to_image;
pub mod preset;
pub mod refs;
pub mod rewrite_urls;
pub mod table_to_html;
pub mod table_to_image;

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use comrak::nodes::NodeLink;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;
use comrak::Options;
use fancy_regex::Regex;

use crate::process::refs::Refs;
use crate::process::rewrite_urls::UrlRewrite;
use crate::render::chrome::ChromeRenderer;
use crate::render::math_img::MathService;
use crate::render::math_img::SERVICE_NAMES;

/// What an action reads and writes besides the tree.
pub struct ActionContext {
    /// The directory of the input file; a path in the markdown resolves against it, also `/x`.
    pub input_dir: PathBuf,
    /// The directory that receives the files the actions create.
    pub assets_dir: PathBuf,
    /// The directory of the output file; a link in the output is relative to it.
    pub output_dir: PathBuf,
    /// The URL that serves `output_dir`; a link to a file that an action creates starts with it.
    pub url_base: Option<String>,
    /// The output file's stem; it prefixes the names of the files the actions create.
    pub stem: String,
    /// The link reference definitions for the references that the markdown does not define.
    pub refs: Refs,
    /// The regexes of the image URLs that `embed-markdown` embeds; an image is embedded when one of
    /// them matches somewhere in its URL.
    pub embed_patterns: Vec<Regex>,
    /// Renders an HTML page to a PNG.
    pub renderer: ChromeRenderer,
}

impl ActionContext {
    /// The link to `file`, which an action created or copied: the path of `file` relative to
    /// `output_dir`, behind `url_base` and one `/` when there is a URL base.
    pub fn link_to(&self, file: &Path) -> anyhow::Result<String> {
        let path = relative_url(&self.output_dir, file)?;
        let Some(url_base) = &self.url_base else {
            return Ok(path);
        };
        let url_base = url_base.trim_end_matches('/');
        Ok(format!("{url_base}/{path}"))
    }

    /// Whether `url` lies under `url_base`, as a link that `link_to` made does.
    pub fn is_under_url_base(&self, url: &str) -> bool {
        self.path_under_url_base(url).is_some()
    }

    /// The path relative to `output_dir` that `url` names, when `url` lies under `url_base`.
    fn path_under_url_base<'u>(&self, url: &'u str) -> Option<&'u str> {
        let url_base = self.url_base.as_ref()?;
        let prefix = format!("{}/", url_base.trim_end_matches('/'));
        url.strip_prefix(&prefix)
    }

    /// The local file that the image or link URL `url` in the tree names; None for a URL that names
    /// no local file, such as `https://x`, `//x` or `data:x`.
    ///
    /// A URL under `url_base`, and a relative URL whose file is in `output_dir`, name a file in
    /// `output_dir`, as a link that `link_to` made does. A root path `/x`, and any other relative URL,
    /// name a file in `input_dir`, as a link in the input does.
    pub(crate) fn local_file(&self, url: &str) -> Option<PathBuf> {
        if let Some(path) = self.path_under_url_base(url) {
            return Some(self.output_dir.join(path));
        }
        if embed_markdown::is_root_path(url) {
            let file = embed_markdown::resolve(&self.input_dir, &self.input_dir, url);
            return Some(file);
        }
        if !embed_markdown::is_relative(url) {
            return None;
        }
        let in_output = self.output_dir.join(url);
        if in_output.exists() {
            return Some(in_output);
        }
        Some(self.input_dir.join(url))
    }
}

/// One edit of the markdown tree, as `--action` names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Replace every table with a PNG of it.
    TableToImage,
    /// Download every `http(s)://` image into the assets dir and link the copy.
    DownloadImages,
    /// Replace a paragraph that holds only an image, such as `![](x.md)`, with the content of the
    /// markdown file at the image's URL, when a regex of `ActionContext::embed_patterns` matches it.
    EmbedMarkdown,
    /// Copy every local image into the assets dir and link the copy.
    ImageToAsset,
    /// Replace every table with a bare `<table>`.
    TableToHtml,
    /// Replace every ```` ```mermaid ```` block with a PNG of the diagram.
    MermaidToImage,
    /// Replace every ```` ```graphviz ```` block with a PNG of the graph.
    GraphvizToImage,
    /// Replace every code block with a PNG of it. It takes ```` ```mermaid ```` and
    /// ```` ```graphviz ```` blocks too, so it runs after `mermaid-to-image` and `graphviz-to-image`.
    CodeToImage {
        /// The width in pixels at which a block without a language wraps.
        width: u32,
    },
    /// Replace every formula, also a ```` ```math ```` block, with a PNG of it, or with the image
    /// of an online formula service.
    MathToImage {
        /// The service whose image URL replaces each formula; without one, each formula becomes a
        /// PNG in the assets dir.
        service: Option<MathService>,
    },
    /// Replace every formula, also a ```` ```math ```` block, with the `<img>` tag of an online
    /// formula service.
    MathToImgTag {
        /// The service that draws the formulas.
        service: MathService,
    },
    /// Remove the `---` front matter block at the top of the file.
    DropFrontMatter,
    /// Append a list of the link references that the file uses.
    AppendReferenceList,
    /// Print every `$$` formula in a list item on one line.
    MathBlockToOneLine,
    /// Replace every inline formula with Unicode text.
    MathInlineToText,
    /// Replace every code span with its text.
    CodespanToText,
    /// Replace every list and block quote with the blocks it holds.
    FlattenLists,
    /// Rewrite the URL of every image with a regex.
    RewriteImageUrls {
        /// The `/REGEX/REPL/` rule.
        rule: UrlRewrite,
    },
    /// Rewrite the URL of every link with a regex.
    RewriteLinkUrls {
        /// The `/REGEX/REPL/` rule.
        rule: UrlRewrite,
    },
    /// Join the paragraphs of a `$$` formula that blank lines split. It edits the input file's text
    /// before the parse, so it takes effect wherever it is listed.
    JoinMathBlock,
}

impl FromStr for Action {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, String> {
        // `name=arg` passes one argument to the action.
        let (action, arg) = match name.split_once('=') {
            Some((action, arg)) => (action, Some(arg)),
            None => (name, None),
        };

        match (action, arg) {
            ("table-to-image", None) => Ok(Self::TableToImage),
            ("download-images", None) => Ok(Self::DownloadImages),
            ("embed-markdown", None) => Ok(Self::EmbedMarkdown),
            ("image-to-asset", None) => Ok(Self::ImageToAsset),
            ("table-to-html", None) => Ok(Self::TableToHtml),
            ("mermaid-to-image", None) => Ok(Self::MermaidToImage),
            ("graphviz-to-image", None) => Ok(Self::GraphvizToImage),
            ("code-to-image", None) => Ok(Self::CodeToImage {
                width: code_to_image::DEFAULT_WIDTH,
            }),
            ("code-to-image", Some(width)) => {
                let width = width.parse().map_err(|_| {
                    format!("invalid action: {name}; code-to-image=WIDTH takes a width in pixels")
                })?;
                Ok(Self::CodeToImage { width })
            }
            ("math-to-image", None) => Ok(Self::MathToImage { service: None }),
            ("math-to-image", Some(service)) => {
                let service = service
                    .parse()
                    .map_err(|error| format!("invalid action: {name}; {error}"))?;
                Ok(Self::MathToImage {
                    service: Some(service),
                })
            }
            ("math-to-img-tag", Some(service)) => {
                let service = service
                    .parse()
                    .map_err(|error| format!("invalid action: {name}; {error}"))?;
                Ok(Self::MathToImgTag { service })
            }
            ("math-to-img-tag", None) => Err(format!(
                "invalid action: {name}; math-to-img-tag=SERVICE takes one of: {SERVICE_NAMES}"
            )),
            ("drop-front-matter", None) => Ok(Self::DropFrontMatter),
            ("append-reference-list", None) => Ok(Self::AppendReferenceList),
            ("math-block-to-one-line", None) => Ok(Self::MathBlockToOneLine),
            ("math-inline-to-text", None) => Ok(Self::MathInlineToText),
            ("codespan-to-text", None) => Ok(Self::CodespanToText),
            ("flatten-lists", None) => Ok(Self::FlattenLists),
            ("rewrite-image-urls", Some(rule)) => {
                let rule = rule
                    .parse()
                    .map_err(|error| format!("invalid action: {name}; {error}"))?;
                Ok(Self::RewriteImageUrls { rule })
            }
            ("rewrite-link-urls", Some(rule)) => {
                let rule = rule
                    .parse()
                    .map_err(|error| format!("invalid action: {name}; {error}"))?;
                Ok(Self::RewriteLinkUrls { rule })
            }
            ("rewrite-image-urls" | "rewrite-link-urls", None) => Err(format!(
                "invalid action: {name}; {name}=/REGEX/REPL/ takes a rule"
            )),
            ("join-math-block", None) => Ok(Self::JoinMathBlock),
            _ => Err(format!("unknown action: {name}")),
        }
    }
}

impl Action {
    /// Apply this action to the tree under `root`, which `arena` owns and comrak parsed from
    /// `source`.
    pub fn apply<'a>(
        &self,
        arena: &'a Arena<'a>,
        root: Node<'a>,
        source: &str,
        ctx: &ActionContext,
    ) -> anyhow::Result<()> {
        match self {
            Self::TableToImage => table_to_image::apply(arena, root, ctx),
            Self::DownloadImages => download_images::apply(root, ctx),
            Self::EmbedMarkdown => embed_markdown::apply(arena, root, ctx),
            Self::ImageToAsset => image_to_asset::apply(root, ctx),
            Self::TableToHtml => table_to_html::apply(arena, root),
            Self::MermaidToImage => mermaid_to_image::apply(arena, root, ctx),
            Self::GraphvizToImage => graphviz_to_image::apply(arena, root, ctx),
            Self::CodeToImage { width } => code_to_image::apply(arena, root, *width, ctx),
            Self::MathToImage { service } => math_to_image::apply(arena, root, *service, ctx),
            Self::MathToImgTag { service } => {
                math_to_img_tag::apply(root, *service);
                Ok(())
            }
            Self::DropFrontMatter => {
                drop_front_matter::apply(root);
                Ok(())
            }
            Self::AppendReferenceList => {
                // The `markdown` crate, which finds the used references, takes no callback.
                let source = ctx.refs.append_definitions(source);
                append_reference_list::apply(arena, root, &source)
            }
            Self::MathBlockToOneLine => {
                math_block_to_one_line::apply(root);
                Ok(())
            }
            Self::MathInlineToText => math_inline_to_text::apply(root),
            Self::CodespanToText => {
                codespan_to_text::apply(root);
                Ok(())
            }
            Self::FlattenLists => {
                flatten_lists::apply(arena, root);
                Ok(())
            }
            Self::RewriteImageUrls { rule } => rewrite_urls::apply_to_images(root, rule),
            Self::RewriteLinkUrls { rule } => rewrite_urls::apply_to_links(root, rule),
            // `process_markdown` joins the text before the parse.
            Self::JoinMathBlock => Ok(()),
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

/// An image of `url` without alt text, the inline that replaces a formula.
pub(crate) fn image_node<'a>(arena: &'a Arena<'a>, url: String) -> Node<'a> {
    let link = NodeLink {
        url,
        title: String::new(),
    };
    arena.alloc(NodeValue::Image(Box::new(link)).into())
}

/// A paragraph that holds one image of `url`, the block that replaces a table or a diagram.
pub(crate) fn image_paragraph<'a>(arena: &'a Arena<'a>, url: String) -> Node<'a> {
    let image = image_node(arena, url);
    let paragraph = arena.alloc(NodeValue::Paragraph.into());
    paragraph.append(image);
    paragraph
}

/// Parse `markdown`, apply `actions` in order, and return the markdown of the edited tree.
pub fn process_markdown(
    markdown: &str,
    actions: &[Action],
    ctx: &ActionContext,
) -> anyhow::Result<String> {
    let mut options = gfm_math_options();
    options.parse.broken_link_callback = Some(ctx.refs.broken_link_callback());
    let joins = actions.contains(&Action::JoinMathBlock);
    let markdown = if joins {
        join_math_block::join(markdown)
    } else {
        markdown.to_string()
    };
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, &markdown, &options);

    for action in actions {
        action.apply(&arena, root, &markdown, ctx)?;
    }
    escape_dollars(&arena, root);

    let mut out = String::new();
    comrak::format_commonmark(root, &options, &mut out)?;
    Ok(out)
}

/// Write each `$` in a text node under `root` as `\$`. comrak prints a text `$` bare, also with
/// `math_dollars` on, so the text of `\$a\$` would come back as the formula `$a$`.
fn escape_dollars<'a>(arena: &'a Arena<'a>, root: Node<'a>) {
    let nodes: Vec<Node<'a>> = root.descendants().collect();
    for node in nodes {
        let text = match &node.data().value {
            NodeValue::Text(text) if text.contains('$') => text.to_string(),
            _ => continue,
        };
        // A raw node prints as it is, while a text node would escape the backslash.
        for (index, part) in text.split('$').enumerate() {
            if index > 0 {
                let dollar = arena.alloc(NodeValue::Raw("\\$".to_string()).into());
                node.insert_before(dollar);
            }
            if !part.is_empty() {
                let part = arena.alloc(NodeValue::Text(part.to_string().into()).into());
                node.insert_before(part);
            }
        }
        node.detach();
    }
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

/// `url`, with `https:` in front when it is a protocol-relative URL `//x`: github.com, served over
/// https, loads it from `https://x`.
fn with_scheme(url: &str) -> String {
    if url.starts_with("//") {
        return format!("https:{url}");
    }
    url.to_string()
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

        let code = Action::from_str("code-to-image");
        assert_eq!(code, Ok(Action::CodeToImage { width: 1000 }));

        let code_800 = Action::from_str("code-to-image=800");
        assert_eq!(code_800, Ok(Action::CodeToImage { width: 800 }));

        let bad_width = Action::from_str("code-to-image=wide");
        let expected_error =
            "invalid action: code-to-image=wide; code-to-image=WIDTH takes a width in pixels";
        assert_eq!(bad_width, Err(expected_error.to_string()));

        let math_image = Action::from_str("math-to-image");
        assert_eq!(math_image, Ok(Action::MathToImage { service: None }));

        let math_url_image = Action::from_str("math-to-image=codecogs");
        let expected_math_url_image = Action::MathToImage {
            service: Some(MathService::Codecogs),
        };
        assert_eq!(math_url_image, Ok(expected_math_url_image));

        let img_tag = Action::from_str("math-to-img-tag=upmath");
        let expected_img_tag = Action::MathToImgTag {
            service: MathService::Upmath,
        };
        assert_eq!(img_tag, Ok(expected_img_tag));

        let no_service = Action::from_str("math-to-img-tag");
        let expected_no_service = "invalid action: math-to-img-tag; \
                                   math-to-img-tag=SERVICE takes one of: zhihu, codecogs, upmath, wordpress";
        assert_eq!(no_service, Err(expected_no_service.to_string()));

        let bad_service = Action::from_str("math-to-img-tag=mathjax");
        let expected_bad_service = "invalid action: math-to-img-tag=mathjax; \
                                    unknown math service: mathjax; one of: zhihu, codecogs, upmath, wordpress";
        assert_eq!(bad_service, Err(expected_bad_service.to_string()));

        let link_rewrite = Action::from_str("rewrite-link-urls=|^a/|b/|");
        let rule = UrlRewrite::from_str("|^a/|b/|").unwrap();
        assert_eq!(link_rewrite, Ok(Action::RewriteLinkUrls { rule }));

        let no_rule = Action::from_str("rewrite-image-urls");
        let expected_no_rule =
            "invalid action: rewrite-image-urls; rewrite-image-urls=/REGEX/REPL/ takes a rule";
        assert_eq!(no_rule, Err(expected_no_rule.to_string()));
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

    /// An escaped `\$` stays escaped, also at the start or in a link, and a formula stays.
    #[test]
    fn test_escape_dollars() -> anyhow::Result<()> {
        let markdown = "\\$5 and \\$a\\$ in [\\$b](u), $c$\n";
        let arena = Arena::new();
        let options = gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        escape_dollars(&arena, root);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, markdown);
        Ok(())
    }
}
