use std::fs;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use anyhow::Result;

/// Without `-i`, `xpmd render-math-img` reads the TeX from stdin and prints the service's `<img>`
/// tag; `--inline` leaves out the `\displaystyle` that a display formula gets.
#[test]
fn test_render_math_img_reads_stdin() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-math-img", "--service", "codecogs", "--inline"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"a<b\n")?;
    drop(stdin);
    let result = child.wait_with_output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    let expected_stdout = concat!(
        r#"<img src="https://latex.codecogs.com/svg.image?a%3Cb" alt="a&lt;b">"#,
        "\n"
    );
    assert_eq!(stdout, expected_stdout);
    Ok(())
}

/// `-i` and `-o` name the TeX file and the tag file; a display formula, the default, ends with `\\`
/// on zhihu.
#[test]
fn test_render_math_img_files() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("sum.tex");
    fs::write(&input, "\\sum_{i=1}^n i\n")?;
    let output = dir.path().join("sum.html");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-math-img", "--service", "zhihu", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");

    let tag = fs::read_to_string(&output)?;
    let expected_tag = concat!(
        r#"<img src="https://www.zhihu.com/equation?tex=%5Csum_%7Bi%3D1%7D%5En%20i%5C%5C" alt="\sum_{i=1}^n i\\" class="ee_img tr_noresize" eeimg="1">"#,
        "\n"
    );
    assert_eq!(tag, expected_tag);
    Ok(())
}
