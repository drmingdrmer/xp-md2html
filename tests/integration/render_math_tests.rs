use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Every PNG file starts with these bytes.
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// `xpmd render-math` prints one self-contained `<svg>` to stdout by default, and a PNG when `-o` says so.
#[test]
fn test_render_math_svg_and_png() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/math.tex");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-math", "-i"])
        .arg(&input)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let svg = String::from_utf8(result.stdout)?;
    assert!(
        svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" "),
        "{svg}"
    );
    assert!(svg.ends_with("</svg>\n"), "{svg}");
    // The glyphs are paths inside the SVG, so it needs no font; MathJax's wrapper is cut off.
    assert!(
        svg.contains("<defs><path id=\"MJX-1-TEX-I-1D465\" "),
        "{svg}"
    );
    assert!(!svg.contains("mjx-container"), "{svg}");
    assert_eq!(svg.matches("<svg").count(), 1);

    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("math.png");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-math", "--inline", "-i"])
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
