use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Every PNG file starts with these bytes.
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// `xpmd process --action table-to-image` writes the PNG into `--assets` and links it relative to the output file.
#[test]
fn test_process_table_to_image() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/table.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");
    let assets = output_dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "table-to-image", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    let expected_stdout = format!("✅ Successfully wrote: {}\n", output.display());
    assert_eq!(stdout, expected_stdout);

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "# Tables\n\n\
        Before the table.\n\n\
        ![](assets/post-table-ef022588a517.png)\n\n\
        After the table.\n";
    assert_eq!(markdown, expected_markdown);

    let image = fs::read(assets.join("post-table-ef022588a517.png"))?;
    let magic = image.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// `xpmd process --action mermaid-to-image` writes the diagram's PNG into `--assets`, named by the
/// hash of the block's content, and links it where the block was.
#[test]
fn test_process_mermaid_to_image() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/mermaid.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");
    let assets = output_dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "mermaid-to-image", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "# Flow\n\n\
        ![](assets/post-mermaid-2a403c0fada7.png)\n\n\
        Done.\n";
    assert_eq!(markdown, expected_markdown);

    let image = fs::read(assets.join("post-mermaid-2a403c0fada7.png"))?;
    let magic = image.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// `xpmd process --action graphviz-to-image` writes the graph's PNG into `--assets`, named by the
/// hash of the block's content, and links it where the block was.
#[test]
fn test_process_graphviz_to_image() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/graphviz.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");
    let assets = output_dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "graphviz-to-image", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "# Graph\n\n\
        ![](assets/post-graphviz-7f7eca5c2cac.png)\n\n\
        Done.\n";
    assert_eq!(markdown, expected_markdown);

    let image = fs::read(assets.join("post-graphviz-7f7eca5c2cac.png"))?;
    let magic = image.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// `xpmd process --action code-to-image=800` replaces each code block with a PNG: a long line
/// wraps at 600 pixels in a block with a language, and at the given 800 in a block without one.
#[test]
fn test_process_code_to_image() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/code.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");
    let assets = output_dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args([
            "process",
            "--action",
            "code-to-image=800",
            "--scale",
            "1",
            "-i",
        ])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "# Code\n\n\
        ![](assets/post-code-8384d441da95.png)\n\n\
        ![](assets/post-code-e39ad6284dc4.png)\n\n\
        Done.\n";
    assert_eq!(markdown, expected_markdown);

    let (rust_width, _) = image::image_dimensions(assets.join("post-code-8384d441da95.png"))?;
    assert_eq!(rust_width, 600);

    let (plain_width, _) = image::image_dimensions(assets.join("post-code-e39ad6284dc4.png"))?;
    assert_eq!(plain_width, 800);
    Ok(())
}

/// `xpmd process --action math-to-image` replaces inline math with an inline image, and a `$$`
/// formula or a ```` ```math ```` block with a paragraph that holds one image.
#[test]
fn test_process_math_to_image() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/math.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");
    let assets = output_dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "math-to-image", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "# Math\n\n\
        Inline ![](assets/post-math-7f408224ea7b.png) here.\n\n\
        ![](assets/post-math-ed3f5d5080ba.png)\n\n\
        ![](assets/post-math-905b5391d6ab.png)\n\n\
        Done.\n";
    assert_eq!(markdown, expected_markdown);

    for name in [
        "post-math-7f408224ea7b.png",
        "post-math-ed3f5d5080ba.png",
        "post-math-905b5391d6ab.png",
    ] {
        let image = fs::read(assets.join(name))?;
        let magic = image.get(..PNG_MAGIC.len());
        assert_eq!(magic, Some(PNG_MAGIC.as_slice()), "{name}");
    }
    Ok(())
}

/// `xpmd process --action math-to-image=codecogs` links every formula to the image at the URL where
/// CodeCogs draws it, and writes no file into `--assets`.
#[test]
fn test_process_math_to_image_service() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/math.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");
    let assets = output_dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "math-to-image=codecogs", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "# Math\n\n\
        Inline ![](https://latex.codecogs.com/svg.image?x%5E2) here.\n\n\
        ![](https://latex.codecogs.com/svg.image?%5Cdisplaystyle%20%5Csum_%7Bi%3D1%7D%5E%7Bn%7D%20i)\n\n\
        ![](https://latex.codecogs.com/svg.image?%5Cdisplaystyle%20E%20%3D%20mc%5E2)\n\n\
        Done.\n";
    assert_eq!(markdown, expected_markdown);

    let mut written = Vec::new();
    for entry in fs::read_dir(&assets)? {
        let entry = entry?;
        written.push(entry.file_name());
    }
    assert_eq!(written, Vec::<OsString>::new());
    Ok(())
}

/// `xpmd process --action math-to-img-tag=zhihu` replaces every formula with the `<img>` tag of zhihu's
/// equation service; the TeX of a `$$` formula or a ```` ```math ```` block ends with `\\`.
#[test]
fn test_process_math_to_img_tag() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/math.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "math-to-img-tag=zhihu", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = concat!(
        "# Math\n\n",
        r#"Inline <img src="https://www.zhihu.com/equation?tex=x%5E2" alt="x^2" class="ee_img tr_noresize" eeimg="1"> here."#,
        "\n\n",
        r#"<img src="https://www.zhihu.com/equation?tex=%5Csum_%7Bi%3D1%7D%5E%7Bn%7D%20i%5C%5C" alt="\sum_{i=1}^{n} i\\" class="ee_img tr_noresize" eeimg="1">"#,
        "\n\n",
        r#"<img src="https://www.zhihu.com/equation?tex=E%20%3D%20mc%5E2%5C%5C" alt="E = mc^2\\" class="ee_img tr_noresize" eeimg="1">"#,
        "\n\n",
        "Done.\n",
    );
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// `xpmd process --action drop-front-matter` removes the `---` block at the top of the file.
#[test]
fn test_process_drop_front_matter() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "---\ntitle: T\n---\n\n# Title\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "drop-front-matter", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "# Title\n");
    Ok(())
}

/// `xpmd process --action table-to-html` writes the table as a bare `<table>` of rows.
#[test]
fn test_process_table_to_html() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/table.md");
    let output_dir = tempfile::tempdir()?;
    let output = output_dir.path().join("post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "table-to-html", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "# Tables\n\n\
        Before the table.\n\n\
        <table>\n\
        <tr>\n\
        <th align=\"left\">name</th>\n\
        <th align=\"right\">value</th>\n\
        <th>note</th>\n\
        </tr>\n\
        <tr>\n\
        <td align=\"left\">a</td>\n\
        <td align=\"right\">1</td>\n\
        <td><code>code</code> and 中文</td>\n\
        </tr>\n\
        <tr>\n\
        <td align=\"left\">b</td>\n\
        <td align=\"right\">22</td>\n\
        <td><strong>bold</strong></td>\n\
        </tr>\n\
        </table>\n\n\
        After the table.\n";
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// `xpmd process --action image-to-asset` copies a local image into `--assets` under a name that
/// hashes its content, and links the copy relative to the output file.
#[test]
fn test_process_image_to_asset() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input_dir = dir.path().join("posts");
    fs::create_dir_all(input_dir.join("img"))?;
    fs::write(input_dir.join("img/a.png"), b"PNG-A")?;
    let input = input_dir.join("post.md");
    fs::write(&input, "![a](img/a.png)\n\n![h](https://x.io/h.png)\n")?;
    let output = dir.path().join("out/post.md");
    let assets = dir.path().join("out/assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "image-to-asset", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(
        markdown,
        "![a](assets/e793c41f42c3-a.png)\n\n![h](https://x.io/h.png)\n"
    );

    let copied = fs::read(assets.join("e793c41f42c3-a.png"))?;
    assert_eq!(copied, b"PNG-A");
    Ok(())
}
