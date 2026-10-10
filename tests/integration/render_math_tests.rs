use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use super::process_tests::count_pixels;
use super::process_tests::BLUE;
use super::process_tests::RED;
use super::xpmd_tests::BoundedOutput;
use super::xpmd_tests::TestDir;

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
        .output_ok()?;

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

    let output_dir = TestDir::new()?;
    let output = output_dir.path().join("math.png");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-math", "--inline", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");

    let png = fs::read(&output)?;
    let magic = png.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    output_dir.close()
}

/// A formula wider or taller than the window of the DOM dump keeps both ends: the image holds the
/// blue square at its start and the red square at its end.
#[test]
fn test_render_math_wide_and_tall() -> Result<()> {
    let blue_square = r"{\color{blue}\rule{1em}{1em}}";
    let red_square = r"{\color{red}\rule{1em}{1em}}";
    let terms = vec!["x"; 100].join("+");
    let wide = [blue_square, &terms, red_square].join("+");
    let rows = vec!["x"; 200].join(r"\\");
    let column = [blue_square, &rows, red_square].join(r"\\");
    let tall = format!(r"\begin{{array}}{{c}}{column}\end{{array}}");

    let dir = TestDir::new()?;
    for (name, tex) in [("wide", wide), ("tall", tall)] {
        let input = dir.path().join(format!("{name}.tex"));
        fs::write(&input, tex)?;
        let output = dir.path().join(format!("{name}.png"));

        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["render-math", "--scale", "1", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output_ok()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let blue = count_pixels(&output, BLUE)?;
        assert!(blue > 0, "the {name} formula lost its start");
        let red = count_pixels(&output, RED)?;
        assert!(red > 0, "the {name} formula lost its end");
    }
    dir.close()
}
