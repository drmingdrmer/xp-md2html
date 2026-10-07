use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::Result;

/// `xpmd render-code` prints the page to stdout and nothing else; `-o` writes the same bytes to a file.
#[test]
fn test_render_code_to_stdout_and_file() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/code.rs");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-code", "-l", "rust", "-i"])
        .arg(&input)
        .output()?;

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

    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("code.html");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-code", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let file_stdout = String::from_utf8(result.stdout)?;
    assert_eq!(file_stdout, "");

    // Without `-l`, the extension `rs` of `-i` picks the language, so the page is the same.
    let written = fs::read_to_string(&output)?;
    assert_eq!(written, stdout);
    Ok(())
}

/// Without `-i`, `render-code` reads the code from stdin.
#[test]
fn test_render_code_reads_stdin() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-code", "-l", "rust"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"fn main() {}\n")?;
    drop(stdin);
    let result = child.wait_with_output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    assert!(
        stdout.contains("<span style=\"color:#b48ead;\">fn </span>"),
        "{stdout}"
    );
    Ok(())
}
