use std::fs;
use std::path::Path;

use anyhow::Result;
use xp_md2html::render::chrome::ChromeRenderer;
use xp_md2html::render::chrome::RenderConfig;

/// A PDF that embeds a font file draws its text as text; a screenshot PDF holds only an image.
const EMBEDDED_FONT: &[u8] = b"/FontFile";

#[test]
fn test_pdf_keeps_text() -> Result<()> {
    let input_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple.html");
    let input = fs::read_to_string(&input_path)?;

    let config = RenderConfig {
        mime: "text/html".to_string(),
        output_type: "pdf".to_string(),
        width: 800,
        height: 600,
        scale: 1,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config)?;
    let pdf = renderer.render_markup(&input)?;

    let embeds_font = pdf.windows(EMBEDDED_FONT.len()).any(|w| w == EMBEDDED_FONT);
    assert!(embeds_font, "the PDF embeds no font");
    Ok(())
}
