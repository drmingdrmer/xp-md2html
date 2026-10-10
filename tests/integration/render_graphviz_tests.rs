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

/// `xpmd render-graphviz` prints one `<svg>` to stdout by default, and a PNG when `-o` says so.
#[test]
fn test_render_graphviz_svg_and_png() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/graph.dot");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-graphviz", "-i"])
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
    assert_eq!(svg.matches("<svg").count(), 1);
    // The three nodes and the edge label are text; Graphviz lays the graph out left to right.
    assert!(svg.contains(">a</text>"), "{svg}");
    assert!(svg.contains(">c</text>"), "{svg}");
    assert!(svg.contains(">边 edge</text>"), "{svg}");
    assert!(svg.contains("rotate(0)"), "{svg}");

    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("graph.png");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-graphviz", "-i"])
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

/// A graph wider or taller than the window of the DOM dump keeps both ends at the default scale 2:
/// the PNG holds the whole blue square at the start and the whole red square at the end.
///
/// Each square is 72 points, 96 CSS pixels, so 192 pixels on a side at scale 2. The ranks are 25
/// inches apart and the pad is 3 points, so the graph is 1950 points long and 78 across: 2600 and
/// 104 CSS pixels, with each edge on a whole pixel, and 5200 and 208 pixels at scale 2.
#[test]
fn test_render_graphviz_wide_and_tall() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let cases = [("wide", "LR", (5200, 208)), ("tall", "TB", (208, 5200))];
    for (name, rankdir, expected_size) in cases {
        let graph = format!(
            "digraph {{\n\
             pad = 0.0416667; rankdir = {rankdir}; ranksep = 25;\n\
             node [shape = box, style = filled, penwidth = 0, fixedsize = true, width = 1, \
             height = 1, label = \"\"];\n\
             a [fillcolor = blue]; b [fillcolor = red];\n\
             a -> b [style = invis];\n\
             }}\n"
        );
        let input = dir.path().join(format!("{name}.dot"));
        fs::write(&input, graph)?;
        let output = dir.path().join(format!("{name}.png"));

        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["render-graphviz", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output_ok()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let size = image::image_dimensions(&output)?;
        assert_eq!(size, expected_size, "the size of the {name} graph");
        let blue = count_pixels(&output, BLUE)?;
        assert_eq!(blue, 192 * 192, "the start of the {name} graph");
        let red = count_pixels(&output, RED)?;
        assert_eq!(red, 192 * 192, "the end of the {name} graph");
    }
    Ok(())
}

/// A DOT syntax error is reported with Graphviz's message, and nothing is written.
#[test]
fn test_render_graphviz_error() -> Result<()> {
    let output_dir = tempfile::tempdir()?;
    let input = output_dir.path().join("bad.dot");
    fs::write(&input, "digraph { a -> ; }\n")?;

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-graphviz", "-i"])
        .arg(&input)
        .output_bounded()?;

    assert!(!result.status.success());
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");
    let stderr = String::from_utf8(result.stderr)?;
    assert!(
        stderr.contains("Graphviz: syntax error in line 1 near ';'"),
        "{stderr}"
    );
    Ok(())
}
