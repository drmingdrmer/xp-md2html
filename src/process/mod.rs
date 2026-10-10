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

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
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
use sha2::Digest;
use sha2::Sha256;

use crate::process::refs::Refs;
use crate::process::rewrite_urls::UrlRewrite;
use crate::render::chrome::LazyRenderer;
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
    /// The markdown's name, such as the output file's stem; it prefixes the names of the files the
    /// actions create.
    pub stem: String,
    /// The link reference definitions for the references that the markdown does not define.
    pub refs: Refs,
    /// The regexes of the image URLs that `embed-markdown` embeds; an image is embedded when one of
    /// them matches somewhere in its URL.
    pub embed_patterns: Vec<Regex>,
    /// Renders an HTML page to a PNG.
    pub renderer: LazyRenderer,
    /// The file of each asset, a file that an action created or copied, by the link to it;
    /// [`ActionContext::link_to`] adds them. A link `//x` is keyed as `https://x`, the URL that
    /// `download-images` sees.
    pub assets: RefCell<HashMap<String, PathBuf>>,
    /// The link to each PNG that `ActionContext::render_once` wrote, by the PNG's name.
    pub rendered: RefCell<HashMap<String, String>>,
}

impl ActionContext {
    /// The link to `file`, which an action created or copied: the path of `file` relative to
    /// `output_dir`, behind `url_base` and one `/` when there is a URL base. It adds `file` to
    /// `assets`, so a later action finds the file by the link.
    pub fn link_to(&self, file: &Path) -> anyhow::Result<String> {
        let path = relative_url(&self.output_dir, file)?;
        let link = match &self.url_base {
            Some(url_base) => {
                let url_base = url_base.trim_end_matches('/');
                format!("{url_base}/{path}")
            }
            None => path,
        };
        let key = with_scheme(&link);
        self.assets.borrow_mut().insert(key, file.to_path_buf());
        Ok(link)
    }

    /// Whether `url` is the link to an asset, a file that an action created or copied.
    pub fn is_asset(&self, url: &str) -> bool {
        self.asset_file(url).is_some()
    }

    /// The file of the asset that `url` links, also with a query or a fragment after the link.
    fn asset_file(&self, url: &str) -> Option<PathBuf> {
        let (link, _) = split_suffix(url);
        let key = with_scheme(link);
        let assets = self.assets.borrow();
        let file = assets.get(&key)?;
        Some(file.clone())
    }

    /// The local file that the image or link URL `url` in the tree names; None for a URL that names
    /// no local file, such as `https://x`, `//x` or `data:x`.
    ///
    /// The link to an asset names the file that an action created or copied. A root path `/x`, and
    /// any other relative URL, name a file in `input_dir`, as a link in the input does; the file
    /// path is the URL's path, decoded by [`url_path`].
    pub(crate) fn local_file(&self, url: &str) -> Option<PathBuf> {
        if let Some(file) = self.asset_file(url) {
            return Some(file);
        }
        let is_local = embed_markdown::is_root_path(url) || embed_markdown::is_relative(url);
        if !is_local {
            return None;
        }
        let file = embed_markdown::resolve(&self.input_dir, &self.input_dir, url);
        Some(file)
    }

    /// The link to the PNG that `render` makes, which goes into `assets_dir` as
    /// `<stem>-<kind>-<hash of key>.png`. `key` holds each input of `render` that can change within
    /// a run, such as a diagram's source; the renderer's settings and the local files stay the same.
    /// Every render runs Chrome, so a repeated `kind` and `key` reuses the first PNG and skips
    /// `render`.
    pub(crate) fn render_once(
        &self,
        kind: &str,
        key: &str,
        render: impl FnOnce() -> anyhow::Result<Vec<u8>>,
    ) -> anyhow::Result<String> {
        let hash = short_hash(key.as_bytes());
        let name = format!("{}-{kind}-{hash}.png", self.stem);
        let cached = self.rendered.borrow().get(&name).cloned();
        if let Some(link) = cached {
            return Ok(link);
        }

        let png = render()?;
        let path = self.assets_dir.join(&name);
        fs::write(&path, png)
            .with_context(|| format!("Failed to write image: {}", path.display()))?;
        let link = self.link_to(&path)?;
        self.rendered.borrow_mut().insert(name, link.clone());
        Ok(link)
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
    /// Join the paragraphs of a `$$` formula that blank lines split. It edits the text of the input
    /// file and of each embedded file before the parse, so it takes effect wherever it is listed.
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
    /// Apply this action to the tree under `root`, which `arena` owns and `loader` parsed.
    pub fn apply<'a>(
        &self,
        arena: &'a Arena<'a>,
        root: Node<'a>,
        loader: &Loader<'a>,
        ctx: &ActionContext,
    ) -> anyhow::Result<()> {
        match self {
            Self::TableToImage => table_to_image::apply(arena, root, ctx),
            Self::DownloadImages => download_images::apply(root, ctx),
            Self::EmbedMarkdown => embed_markdown::apply(loader, root, ctx),
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
                append_reference_list::apply(arena, root, loader);
                Ok(())
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
            // `Loader::load` joins the text before the parse.
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

/// Parses the input file and each file that it embeds into a tree in one arena, and keeps each link
/// that a reference made, for `append-reference-list`.
pub struct Loader<'a> {
    /// The arena that owns the trees.
    arena: &'a Arena<'a>,
    /// Whether to join a `$$` formula that blank lines split before the parse, for
    /// `join-math-block`.
    joins: bool,
    /// Each link that a reference made, with the label of its definition, in the order of the
    /// parses.
    references: RefCell<Vec<(Node<'a>, String)>>,
}

impl<'a> Loader<'a> {
    /// A loader that parses into `arena`, and joins split `$$` formulas when `actions` holds
    /// `join-math-block`.
    pub fn new(arena: &'a Arena<'a>, actions: &[Action]) -> Self {
        Self {
            arena,
            joins: actions.contains(&Action::JoinMathBlock),
            references: RefCell::default(),
        }
    }

    /// Parse `markdown` into a tree; a reference that the markdown does not define resolves with
    /// `refs`.
    pub fn load(&self, markdown: &str, refs: &Refs) -> anyhow::Result<Node<'a>> {
        let markdown = if self.joins {
            join_math_block::join(markdown)
        } else {
            markdown.to_string()
        };
        let (root, references) = refs.parse(self.arena, &markdown)?;
        self.references.borrow_mut().extend(references);
        Ok(root)
    }

    /// Each link that a reference `[text][label]`, `[label][]` or `[label]` made, with the label of
    /// its definition, in the order of the parses.
    pub fn references(&self) -> Vec<(Node<'a>, String)> {
        self.references.borrow().clone()
    }
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
    let options = gfm_math_options();
    let arena = Arena::new();
    let loader = Loader::new(&arena, actions);
    let root = loader.load(markdown, &ctx.refs)?;

    for action in actions {
        action.apply(&arena, root, &loader, ctx)?;
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

/// The path of `target` relative to the directory `base`, with `/` between the parts, for a link in a file under `base`;
/// each part is percent-encoded as a URL path segment, so that a `#`, `?` or `%` in a name stays part of the path.
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

    let mut parts: Vec<String> = base_parts.map(|_| "..".to_string()).collect();
    for part in target_parts {
        let part = part
            .as_os_str()
            .to_str()
            .with_context(|| format!("Path is not valid UTF-8: {}", target.display()))?;
        let segment = encode_segment(part);
        parts.push(segment);
    }
    Ok(parts.join("/"))
}

/// The ASCII bytes besides letters and digits that [`encode_segment`] keeps as they are.
const SEGMENT_SAFE_BYTES: &[u8] = b"-._~!$&'()*+,;=@";

/// `name`, a file name, as one segment of a URL path: each ASCII byte other than a letter, a digit
/// or one of [`SEGMENT_SAFE_BYTES`] becomes `%XX`, such as the `#`, `?` and `%` that a URL reads as
/// syntax, or the `:` that would make a relative URL read as a scheme. A non-ASCII character
/// stays, so that a name such as `图.png` stays readable.
fn encode_segment(name: &str) -> String {
    let mut segment = String::new();
    for c in name.chars() {
        let keep =
            !c.is_ascii() || c.is_ascii_alphanumeric() || SEGMENT_SAFE_BYTES.contains(&(c as u8));
        if keep {
            segment.push(c);
        } else {
            segment.push_str(&format!("%{:02X}", c as u8));
        }
    }
    segment
}

/// `url` split before its first `?` or `#`: the path, then the query and the fragment, which are
/// empty when `url` has neither.
pub(crate) fn split_suffix(url: &str) -> (&str, &str) {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url.split_at(end)
}

/// The file path that the path of the local URL `url` spells: the part before `?` or `#`, with each
/// `%XX` decoded. An invalid escape such as `%zz` stays as it is, and the whole path stays as it is
/// when the decoded bytes are not UTF-8.
pub(crate) fn url_path(url: &str) -> String {
    let (path, _) = split_suffix(url);
    let mut decoded = Vec::new();
    let mut rest = path.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let escaped = after.get(..2).and_then(hex_byte);
        match (byte, escaped) {
            (b'%', Some(value)) => {
                decoded.push(value);
                rest = &after[2..];
            }
            _ => {
                decoded.push(byte);
                rest = after;
            }
        }
    }
    match String::from_utf8(decoded) {
        Ok(decoded) => decoded,
        Err(_) => path.to_string(),
    }
}

/// The byte that `digits`, two hex digits, spell.
fn hex_byte(digits: &[u8]) -> Option<u8> {
    let is_hex = digits.iter().all(u8::is_ascii_hexdigit);
    if !is_hex {
        return None;
    }
    let text = std::str::from_utf8(digits).ok()?;
    u8::from_str_radix(text, 16).ok()
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

/// How many hex digits of a SHA-256 hash the name of an asset keeps.
const HASH_LEN: usize = 12;

/// The first [`HASH_LEN`] hex digits of the SHA-256 hash of `content`, which tell assets apart by
/// their names.
pub(crate) fn short_hash(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    let hex = format!("{digest:x}");
    hex[..HASH_LEN].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::chrome::RenderConfig;

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

        let encoded = relative_url(Path::new("/a"), Path::new("/a/s#1?/my pic:100%.png"))?;
        assert_eq!(encoded, "s%231%3F/my%20pic%3A100%25.png");

        let kept = relative_url(
            Path::new("/a"),
            Path::new("/a/图 (1)/a-b_c~d!$&'*+,;=@.png"),
        )?;
        assert_eq!(kept, "图%20(1)/a-b_c~d!$&'*+,;=@.png");
        Ok(())
    }

    #[test]
    fn test_split_suffix() {
        let both = split_suffix("a/b.svg?v=2#icon");
        assert_eq!(both, ("a/b.svg", "?v=2#icon"));

        let fragment = split_suffix("b.svg#x?y");
        assert_eq!(fragment, ("b.svg", "#x?y"));

        let none = split_suffix("b.svg");
        assert_eq!(none, ("b.svg", ""));
    }

    #[test]
    fn test_url_path() {
        let space = url_path("my%20pic.svg#icon");
        assert_eq!(space, "my pic.svg");

        let unicode = url_path("%E5%9B%BE/图.svg?v=2");
        assert_eq!(unicode, "图/图.svg");

        let percent = url_path("100%25.svg");
        assert_eq!(percent, "100%.svg");

        let invalid_escape = url_path("100%.svg %zz %+1 %2");
        assert_eq!(invalid_escape, "100%.svg %zz %+1 %2");

        let not_utf8 = url_path("a%FF.svg");
        assert_eq!(not_utf8, "a%FF.svg");
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

    /// `render_once` writes and links the PNG of a new kind and key; a repeated kind and key gets
    /// the same link without a render.
    #[test]
    fn test_render_once() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let config = RenderConfig {
            mime: "text/html".to_string(),
            output_type: "png".to_string(),
            width: 1000,
            height: 1000,
            scale: 1,
            asset_base: None,
        };
        let ctx = ActionContext {
            input_dir: dir.path().to_path_buf(),
            assets_dir: dir.path().to_path_buf(),
            output_dir: dir.path().to_path_buf(),
            url_base: None,
            stem: "post".to_string(),
            refs: Refs::default(),
            embed_patterns: Vec::new(),
            renderer: LazyRenderer::new(config),
            assets: RefCell::default(),
            rendered: RefCell::default(),
        };

        // The hash is `printf '| a |' | shasum -a 256 | cut -c1-12`.
        let link = ctx.render_once("table", "| a |", || Ok(b"PNG-A".to_vec()))?;
        assert_eq!(link, "post-table-13bb5e1404fe.png");
        let png = fs::read(dir.path().join(&link))?;
        assert_eq!(png, b"PNG-A");

        let link = ctx.render_once("table", "| a |", || anyhow::bail!("rendered again"))?;
        assert_eq!(link, "post-table-13bb5e1404fe.png");

        let link = ctx.render_once("code", "| a |", || Ok(b"PNG-C".to_vec()))?;
        assert_eq!(link, "post-code-13bb5e1404fe.png");
        let png = fs::read(dir.path().join(&link))?;
        assert_eq!(png, b"PNG-C");
        Ok(())
    }
}
