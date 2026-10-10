use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::Result;

/// Every JPEG file starts with these bytes.
const JPEG_MAGIC: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// Every PNG file starts with these bytes.
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// Every PDF file starts with these bytes.
const PDF_MAGIC: &[u8] = b"%PDF-";

/// `xpmd render-markup -o` prints nothing: `ChromeRenderer` prints nothing, and Chrome's noise is captured.
#[test]
fn test_render_prints_nothing() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/simple.html");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("simple.png");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-w", "800", "--height", "600", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");

    let data = fs::read(&output)?;
    let magic = data.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// Without `-f`, the extension of `-o` picks the output format.
#[test]
fn test_render_takes_format_from_output_extension() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/simple.html");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("simple.jpg");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let data = fs::read(&output)?;
    let magic = data.get(..JPEG_MAGIC.len());
    assert_eq!(magic, Some(JPEG_MAGIC.as_slice()));
    Ok(())
}

/// Without `-i` and `-o`, `render-markup` reads HTML from stdin and writes the image to stdout.
#[test]
fn test_render_stdin_to_stdout() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-f", "jpg"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"<html><body><h1>From stdin</h1></body></html>")?;
    drop(stdin);
    let result = child.wait_with_output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let magic = result.stdout.get(..JPEG_MAGIC.len());
    assert_eq!(magic, Some(JPEG_MAGIC.as_slice()));
    Ok(())
}

/// Without ImageMagick, `render-markup` still writes a PDF and `render-math` an SVG: neither trims
/// an image. Only macOS finds Chrome outside PATH, so an empty PATH hides ImageMagick alone.
#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "only macOS finds Chrome outside PATH"
)]
fn test_render_without_imagemagick() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir()?;
    let empty_path = dir.path().join("bin");
    fs::create_dir_all(&empty_path)?;

    let pdf = dir.path().join("simple.pdf");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(root_dir.join("tests/fixtures/simple.html"))
        .arg("-o")
        .arg(&pdf)
        .env("PATH", &empty_path)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let data = fs::read(&pdf)?;
    let magic = data.get(..PDF_MAGIC.len());
    assert_eq!(magic, Some(PDF_MAGIC));

    let svg = dir.path().join("math.svg");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-math", "-i"])
        .arg(root_dir.join("tests/fixtures/math.tex"))
        .arg("-o")
        .arg(&svg)
        .env("PATH", &empty_path)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let data = fs::read_to_string(&svg)?;
    assert!(data.starts_with("<svg "), "{data}");
    Ok(())
}

/// Without ImageMagick, a PNG fails with the help to install ImageMagick, which trims it.
#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "only macOS finds Chrome outside PATH"
)]
fn test_render_png_without_imagemagick() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir()?;
    let empty_path = dir.path().join("bin");
    fs::create_dir_all(&empty_path)?;

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(root_dir.join("tests/fixtures/simple.html"))
        .arg("-o")
        .arg(dir.path().join("simple.png"))
        .env("PATH", &empty_path)
        .output()?;

    let succeeded = result.status.success();
    assert!(!succeeded);

    let stderr = String::from_utf8(result.stderr)?;
    let first_line = stderr.lines().next();
    let expected_first_line =
        "Error: Failed to trim the image. Make sure ImageMagick is installed \
                               and accessible; an SVG or a PDF does not need it.";
    assert_eq!(first_line, Some(expected_first_line));
    Ok(())
}
