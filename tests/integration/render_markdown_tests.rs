use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::Result;

/// `xpmd render-markdown` prints a page in GitHub's style; `--bare` prints only the content's HTML.
#[test]
fn test_render_markdown_page_and_bare() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/table.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markdown", "-i"])
        .arg(&input)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let page = String::from_utf8(result.stdout)?;
    assert!(page.starts_with("<!DOCTYPE html>\n<html><head>"), "{page}");
    assert!(page.ends_with("</article>\n</body></html>\n"), "{page}");
    assert!(
        page.contains("<article class=\"markdown-body\">\n<h1>Tables</h1>\n"),
        "{page}"
    );
    assert!(
        page.contains("<td><code>code</code> and 中文</td>"),
        "{page}"
    );

    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("table.html");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markdown", "--bare", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");

    let bare = fs::read_to_string(&output)?;
    assert!(
        bare.starts_with("<h1>Tables</h1>\n<p>Before the table.</p>\n<table>\n"),
        "{bare}"
    );
    assert!(
        bare.ends_with("</table>\n<p>After the table.</p>\n"),
        "{bare}"
    );
    // The page holds exactly the bare HTML.
    assert!(page.contains(&bare), "{page}");
    Ok(())
}

/// Without `-i`, `render-markdown` reads the markdown from stdin.
#[test]
fn test_render_markdown_reads_stdin() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markdown", "--bare"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"a <br> b\n")?;
    drop(stdin);
    let result = child.wait_with_output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "<p>a <br> b</p>\n");
    Ok(())
}
