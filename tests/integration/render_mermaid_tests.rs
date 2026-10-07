use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Every PNG file starts with these bytes.
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// `xpmd render-mermaid` prints one `<svg>` to stdout by default, and a PNG when `-o` says so.
#[test]
fn test_render_mermaid_svg_and_png() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/flow.mmd");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-mermaid", "-i"])
        .arg(&input)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let svg = String::from_utf8(result.stdout)?;
    assert!(svg.starts_with("<svg id=\"d\" width=\"100%\" "), "{svg}");
    assert!(svg.ends_with("</svg>\n"), "{svg}");
    assert!(svg.contains("class=\"flowchart\""), "{svg}");
    // The node labels are HTML inside the SVG; mermaid's style sheet is inside it too.
    assert!(svg.contains(">开始 start</"), "{svg}");
    assert!(svg.contains(">ok?</"), "{svg}");
    assert!(svg.contains("<style>#d{"), "{svg}");

    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("flow.png");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-mermaid", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");

    let png = fs::read(&output)?;
    let magic = png.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// A diagram error is reported with mermaid's message, and nothing is written.
#[test]
fn test_render_mermaid_error() -> Result<()> {
    let output_dir = tempfile::tempdir()?;
    let input = output_dir.path().join("bad.mmd");
    fs::write(&input, "not a diagram\n")?;

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-mermaid", "-i"])
        .arg(&input)
        .output()?;

    assert!(!result.status.success());
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");
    let stderr = String::from_utf8(result.stderr)?;
    assert!(
        stderr.contains("mermaid: No diagram type detected matching given configuration for text: not a diagram"),
        "{stderr}"
    );
    Ok(())
}
