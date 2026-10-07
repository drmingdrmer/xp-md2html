use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

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
        .output()?;

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

/// A DOT syntax error is reported with Graphviz's message, and nothing is written.
#[test]
fn test_render_graphviz_error() -> Result<()> {
    let output_dir = tempfile::tempdir()?;
    let input = output_dir.path().join("bad.dot");
    fs::write(&input, "digraph { a -> ; }\n")?;

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-graphviz", "-i"])
        .arg(&input)
        .output()?;

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
