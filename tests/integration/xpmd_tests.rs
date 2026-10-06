use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

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
