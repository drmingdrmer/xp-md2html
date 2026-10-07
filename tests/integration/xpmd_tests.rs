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
