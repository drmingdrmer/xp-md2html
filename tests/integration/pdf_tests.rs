use std::fs;
use std::path::Path;

use anyhow::Result;
use xp_md2html::render::chrome::ChromeRenderer;
use xp_md2html::render::chrome::OutputFormat;
use xp_md2html::render::chrome::RenderConfig;

/// The PDF draws the page's text as text, which a screenshot PDF would hold only as an image: the
/// words of the PDF's text are those of the heading and the paragraph of `simple.html`.
#[test]
fn test_pdf_keeps_text() -> Result<()> {
    let input_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple.html");
    let input = fs::read_to_string(&input_path)?;

    let config = RenderConfig {
        mime: "text/html".to_string(),
        output_type: OutputFormat::Pdf,
        width: 800,
        height: 600,
        scale: 1,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config)?;
    let pdf = renderer.render_markup(&input)?;

    let text = pdf_extract::extract_text_from_mem(&pdf)?;
    let words: Vec<&str> = text.split_whitespace().collect();
    let expected_words = [
        "Hello", "World", "This", "is", "a", "simple", "test", "page.",
    ];
    assert_eq!(words, expected_words);
    Ok(())
}
