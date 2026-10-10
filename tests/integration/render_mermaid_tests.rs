use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use super::process_tests::count_pixels;
use super::process_tests::BLUE;
use super::process_tests::RED;
use super::xpmd_tests::BoundedOutput;

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
        .output_ok()?;

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
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");

    let png = fs::read(&output)?;
    let magic = png.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// A chart wider or taller than the window of the DOM dump, 1000 by 2000 CSS pixels, keeps its
/// size and both ends: at the default scale 2, the long side of the PNG passes twice the window,
/// and the PNG holds the blue node at the start and the red node at the end. The window of the
/// dump would shrink the wide chart, whose `width` is `100%`, and cut the tall one off.
///
/// Mermaid sizes a node by its font, so the sizes and counts are not exact.
#[test]
fn test_render_mermaid_wide_and_tall() -> Result<()> {
    let dir = tempfile::tempdir()?;
    // Each case ends with the window's side in pixels at scale 2. 10 nodes in a row are about 1900
    // CSS pixels wide, and 36 in a column about 3000 tall.
    let cases = [("wide", "LR", 10, 2000), ("tall", "TB", 36, 4000)];
    for (name, direction, node_count, window_side) in cases {
        let nodes: Vec<String> = (0..node_count).map(|i| format!("n{i}")).collect();
        let chain = nodes.join(" --> ");
        let last = node_count - 1;
        let chart = format!(
            "graph {direction}\n    {chain}\n    \
             style n0 fill:#0000ff,stroke:#0000ff\n    \
             style n{last} fill:#ff0000,stroke:#ff0000\n"
        );
        let input = dir.path().join(format!("{name}.mmd"));
        fs::write(&input, chart)?;
        let output = dir.path().join(format!("{name}.png"));

        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["render-mermaid", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output_ok()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let (width, height) = image::image_dimensions(&output)?;
        let long_side = width.max(height);
        assert!(
            long_side > window_side,
            "the {name} chart shrank to {width}x{height}"
        );
        let blue = count_pixels(&output, BLUE)?;
        assert!(blue > 0, "the {name} chart lost its start");
        let red = count_pixels(&output, RED)?;
        assert!(red > 0, "the {name} chart lost its end");
    }
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
        .output_bounded()?;

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
