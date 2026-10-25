use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use comrak::nodes::NodeCodeBlock;
use comrak::nodes::NodeLink;
use comrak::nodes::NodeList;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;
use image::RgbaImage;
use serde::Deserialize;
use xp_md2html::process::preset::PRESET_NAMES;
use xp_md2html::process::process_markdown;
use xp_md2html::process::refs::Refs;
use xp_md2html::process::Action;
use xp_md2html::process::ActionContext;
use xp_md2html::process::Loader;
use xp_md2html::render::chrome::LazyRenderer;
use xp_md2html::render::chrome::OutputFormat;
use xp_md2html::render::chrome::RenderConfig;

use super::xpmd_tests::BoundedOutput;
use super::xpmd_tests::TestDir;

const HTML_COMMENT_BLOCK: u8 = 2;

#[derive(Debug, PartialEq, Eq)]
struct Meaning {
    value: NodeValue,
    children: Vec<Meaning>,
}

// Keep semantic fields, including exact URLs, titles and code, while ignoring fence and marker spelling.
fn semantic_value(value: &NodeValue) -> NodeValue {
    let mut value = value.clone();
    match &mut value {
        NodeValue::CodeBlock(code) => {
            **code = NodeCodeBlock {
                info: code.info.clone(),
                literal: code.literal.clone(),
                ..Default::default()
            };
        }
        NodeValue::Code(code) => code.num_backticks = 0,
        NodeValue::Heading(heading) => {
            heading.setext = false;
            heading.closed = false;
        }
        NodeValue::SoftBreak => value = NodeValue::Text(" ".into()),
        NodeValue::Link(link) | NodeValue::Image(link) => {
            // A URI cannot contain literal spaces; percent encoding preserves its destination.
            link.url = link.url.replace(' ', "%20");
        }
        NodeValue::List(list) => {
            *list = NodeList {
                list_type: list.list_type,
                start: list.start,
                tight: list.tight,
                is_task_list: list.is_task_list,
                ..Default::default()
            };
        }
        NodeValue::Item(list) => *list = NodeList::default(),
        _ => {}
    }
    value
}

fn meaning(node: Node<'_>) -> Meaning {
    let value = semantic_value(&node.data().value);
    let mut children: Vec<Meaning> = Vec::new();
    for child in node.children() {
        let child = meaning(child);
        if let NodeValue::HtmlBlock(html) = &child.value {
            // Comrak inserts comments to separate adjacent lists and code blocks.
            if html.block_type == HTML_COMMENT_BLOCK {
                continue;
            }
        }
        if let NodeValue::Text(text) = &child.value {
            if text.is_empty() {
                continue;
            }
            if let Some(Meaning {
                value: NodeValue::Text(previous),
                ..
            }) = children.last_mut()
            {
                previous.to_mut().push_str(text);
                continue;
            }
        }
        children.push(child);
    }
    Meaning { value, children }
}

fn document_meaning(markdown: &str, autolink: bool) -> Result<Meaning> {
    let arena = Arena::new();
    let loader = Loader::with_autolink(&arena, &[], autolink);
    let refs = Refs::load(&[], markdown, None)?;
    let root = loader.load(markdown, &refs)?;
    Ok(meaning(root))
}

fn render_config() -> RenderConfig {
    RenderConfig {
        mime: "text/html".into(),
        output_type: OutputFormat::Png,
        width: 1000,
        height: 2000,
        scale: 1,
        asset_base: None,
    }
}

fn context(dir: &Path, autolink: bool) -> ActionContext {
    ActionContext {
        autolink,
        input_dir: dir.to_path_buf(),
        output_dir: dir.to_path_buf(),
        assets_dir: dir.to_path_buf(),
        stem: "post".into(),
        url_base: None,
        md2zhihu_image_names: false,
        refs: Refs::default(),
        embed_patterns: Vec::new(),
        renderer: LazyRenderer::new(render_config()),
        assets: RefCell::default(),
        rendered: RefCell::default(),
    }
}

fn check_roundtrips(fixtures: &str, known_changes: &str) -> Result<()> {
    let cases: BTreeMap<String, String> = serde_json::from_str(fixtures)?;
    let known: BTreeMap<String, String> = serde_json::from_str(known_changes)?;
    let dir = TestDir::new()?;
    let mut changed = BTreeMap::new();
    let mut expected_changes = BTreeMap::new();
    for autolink in [false, true] {
        for (name, written) in &known {
            expected_changes.insert((name, autolink), written.clone());
        }
        let mut ctx = context(dir.path(), autolink);
        for (name, markdown) in &cases {
            ctx.refs = Refs::load(&[], markdown, None)?;
            let expected = document_meaning(markdown, autolink)?;
            let written = process_markdown(markdown, &[], &ctx)?;
            let actual = document_meaning(&written, autolink)?;
            if actual != expected {
                if known.get(name) != Some(&written) {
                    eprintln!("CASE: {name} {autolink}\nEXPECTED:\n{expected:#?}\nACTUAL:\n{actual:#?}\nWRITTEN:{written:?}");
                }
                changed.insert((name, autolink), written);
            }
        }
    }
    assert_eq!(changed, expected_changes);
    dir.close()
}

#[test]
fn test_commonmark_roundtrips() -> Result<()> {
    check_roundtrips(include_str!("../fixtures/commonmark.json"), "{}")
}

#[test]
fn test_md2zhihu_roundtrips_with_known_losses() -> Result<()> {
    // Tight-list block boundaries and TeX pipe escapes are existing losses. Pin their exact
    // output so both new regressions and fixes require an explicit baseline update.
    check_roundtrips(
        include_str!("../fixtures/md2zhihu-cases.json"),
        include_str!("../fixtures/md2zhihu-known-changes.json"),
    )
}

#[test]
fn test_empty_links_and_brackets_in_code_and_math() -> Result<()> {
    let dir = TestDir::new()?;
    let ctx = context(dir.path(), false);
    let cases = [
        ("[](u)", "<p><a href=\"u\"></a></p>\n"),
        (
            "![](empty.png)",
            "<p><img src=\"empty.png\" alt=\"\" /></p>\n",
        ),
        ("[`]`](u)", "<p><a href=\"u\"><code>]</code></a></p>\n"),
        (
            "[$a]b$](u)",
            "<p><a href=\"u\"><span data-math-style=\"inline\">a]b</span></a></p>\n",
        ),
        (
            "![a`]`b](x.png)",
            "<p><img src=\"x.png\" alt=\"a]b\" /></p>\n",
        ),
    ];
    for (input, expected) in cases {
        let written = process_markdown(input, &[], &ctx)?;
        let actual = xp_md2html::render::markdown::markdown_to_html(&written);
        assert_eq!(actual, expected, "{input:?} became {written:?}");
    }
    dir.close()
}

#[test]
fn test_link_titles_preserve_literal_entities() -> Result<()> {
    let dir = TestDir::new()?;
    let mut ctx = context(dir.path(), false);
    let cases = [
        ("[x](u \"&amp;copy;\")", "&copy;"),
        (
            "![x](u \"&amp;#169; &amp;#xA9; &amp;#XA9;\")",
            "&#169; &#xA9; &#XA9;",
        ),
        ("[x][r]\n\n[r]: u '&amp;amp;copy;'", "&amp;copy;"),
        (
            "---\nrefs:\n  r: 'u \"&amp;copy;\"'\n---\n\n[x][r]",
            "&copy;",
        ),
        (
            "![x][r]\n\n[r]: u \"&copy; &amp;noSuchEntity;\"",
            "© &noSuchEntity;",
        ),
        (
            "[x](u \"a&amp;b &amp;copy &amp;#1 &quot;quoted&quot;\")",
            "a&b &copy &#1 \"quoted\"",
        ),
    ];
    for (source, title) in cases {
        ctx.refs = Refs::load(&[], source, None)?;
        let written = process_markdown(source, &[], &ctx)?;
        let arena = Arena::new();
        let loader = Loader::new(&arena, &[]);
        let root = loader.load(&written, &Refs::default())?;
        let mut links = Vec::new();
        for node in root.descendants() {
            if let NodeValue::Link(link) | NodeValue::Image(link) = &node.data().value {
                links.push((**link).clone());
            }
        }
        let expected = vec![NodeLink {
            url: "u".into(),
            title: title.into(),
        }];
        assert_eq!(links, expected, "{source:?} became {written:?}");
        let repeated = process_markdown(&written, &[], &ctx)?;
        assert_eq!(repeated, written);
    }
    dir.close()
}

#[derive(Deserialize)]
struct PresetFixtures {
    files: BTreeMap<String, String>,
    renders: BTreeMap<String, RenderCase>,
    outputs: BTreeMap<String, PresetOutput>,
}

#[derive(Deserialize)]
struct RenderCase {
    action: String,
    markdown: String,
    asset: String,
}

#[derive(Deserialize)]
struct PresetOutput {
    markdown: String,
    assets: BTreeMap<String, String>,
}

fn files_under(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().context("Asset name is not UTF-8")?;
        if entry.file_type()?.is_dir() {
            for (child, bytes) in files_under(&path)? {
                files.insert(format!("{name}/{child}"), bytes);
            }
        } else {
            let bytes = fs::read(path)?;
            files.insert(name.to_string(), bytes);
        }
    }
    Ok(files)
}

fn render_examples(
    dir: &Path,
    cases: &BTreeMap<String, RenderCase>,
) -> Result<BTreeMap<String, RgbaImage>> {
    let mut images = BTreeMap::new();
    for (name, case) in cases {
        let output = dir.join("renders").join(name);
        fs::create_dir_all(&output)?;
        let mut ctx = context(&output, true);
        ctx.input_dir = dir.to_path_buf();
        let action: Action = case.action.parse().map_err(anyhow::Error::msg)?;
        process_markdown(&case.markdown, &[action], &ctx)?;
        let mut files = files_under(&output)?;
        let names: Vec<_> = files.keys().collect();
        assert_eq!(names, vec![&case.asset]);
        let bytes = files
            .remove(&case.asset)
            .context("Missing reference render")?;
        let image = image::load_from_memory(&bytes)?;
        let pixels = image.to_rgba8();
        images.insert(name.clone(), pixels);
    }
    Ok(images)
}

fn asset_matches(
    bytes: &[u8],
    reference: &str,
    fixtures: &PresetFixtures,
    images: &BTreeMap<String, RgbaImage>,
) -> Result<bool> {
    let (kind, name) = reference
        .split_once(':')
        .context("Invalid asset reference")?;
    if kind == "file" {
        let source = fixtures.files.get(name).context("Missing source fixture")?;
        return Ok(bytes == source.as_bytes());
    }
    anyhow::ensure!(kind == "render", "Unknown asset reference: {reference}");
    let expected = images.get(name).context("Missing reference image")?;
    let image = image::load_from_memory(bytes)?;
    // Complete pixel equality includes dimensions and alpha, independently of PNG metadata.
    let actual = image.to_rgba8();
    Ok(actual == *expected)
}

#[test]
fn test_all_preset_outputs_and_complete_assets() -> Result<()> {
    let fixtures: PresetFixtures = serde_json::from_str(include_str!("../fixtures/presets.json"))?;
    let names = PRESET_NAMES.split(", ");
    let mut expected_presets: Vec<_> = names.map(str::to_string).collect();
    expected_presets.sort();
    let names = fixtures.outputs.keys();
    let actual_presets: Vec<_> = names.cloned().collect();
    assert_eq!(actual_presets, expected_presets);
    let dir = TestDir::new()?;
    for (name, text) in &fixtures.files {
        fs::write(dir.path().join(name), text)?;
    }
    let images = render_examples(dir.path(), &fixtures.renders)?;
    for (preset, expected) in &fixtures.outputs {
        let output = dir.path().join(preset).join("post.md");
        let assets = dir.path().join(preset).join("assets");
        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .current_dir(dir.path())
            .args([
                "process", "--preset", preset, "--scale", "1", "-i", "post.md", "-o",
            ])
            .arg(&output)
            .arg("--assets")
            .arg(&assets)
            .output_ok()?;
        assert_eq!(result.stdout, b"");
        let markdown = fs::read_to_string(&output)?;
        assert_eq!(markdown, expected.markdown, "{preset}");
        let actual_files = files_under(&assets)?;
        let actual_names: Vec<_> = actual_files.keys().collect();
        let expected_names: Vec<_> = expected.assets.keys().collect();
        assert_eq!(actual_names, expected_names, "{preset}");
        for (name, reference) in &expected.assets {
            let bytes = &actual_files[name];
            let matches = asset_matches(bytes, reference, &fixtures, &images)?;
            assert!(matches, "{preset}/{name} differs from {reference}");
        }
    }
    dir.close()
}
