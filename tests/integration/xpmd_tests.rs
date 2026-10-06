use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Every JPEG file starts with these bytes.
const JPEG_MAGIC: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// `xpmd render` prints only its own summary: `ChromeRenderer` prints nothing, and Chrome's noise is captured.
#[test]
fn test_render_prints_only_its_summary() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/simple.html");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("simple.png");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render", "-w", "800", "--height", "600", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    let output_size = fs::metadata(&output)?.len();
    let expected_stdout = format!(
        "Rendering {} to {} (800x600, format: png)\n\
         ✅ Successfully rendered to: {}\n\
         📊 Output size: {} bytes\n",
        input.display(),
        output.display(),
        output.display(),
        output_size
    );
    assert_eq!(stdout, expected_stdout);
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
        .args(["render", "-i"])
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
