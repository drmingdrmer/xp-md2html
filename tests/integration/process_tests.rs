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
