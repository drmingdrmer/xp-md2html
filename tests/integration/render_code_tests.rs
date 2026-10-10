use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use super::xpmd_tests::BoundedOutput;
use super::xpmd_tests::TestDir;

/// `xpmd render-code` prints the page to stdout and nothing else; `-o` writes the same bytes to a file.
#[test]
fn test_render_code_to_stdout_and_file() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/code.rs");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-code", "-l", "rust", "-i"])
        .arg(&input)
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    assert!(stdout.starts_with("<!DOCTYPE html>\n"), "{stdout}");
    assert!(stdout.ends_with("</pre>\n</body></html>\n"), "{stdout}");
    assert!(
        stdout.contains("<span style=\"color:#b48ead;\">fn </span>"),
        "{stdout}"
    );
    assert!(stdout.contains("&lt;hello&gt; &amp; goodbye"), "{stdout}");

    let output_dir = TestDir::new()?;
    let output = output_dir.path().join("code.html");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-code", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let file_stdout = String::from_utf8(result.stdout)?;
    assert_eq!(file_stdout, "");

    // Without `-l`, the extension `rs` of `-i` picks the language, so the page is the same.
    let written = fs::read_to_string(&output)?;
    assert_eq!(written, stdout);
    output_dir.close()
}

/// Without `-i`, `render-code` reads the code from stdin.
#[test]
fn test_render_code_reads_stdin() -> Result<()> {
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-code", "-l", "rust"])
        .output_ok_with_stdin(b"fn main() {}\n")?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    assert!(
        stdout.contains("<span style=\"color:#b48ead;\">fn </span>"),
        "{stdout}"
    );
    Ok(())
}
