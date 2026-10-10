use std::ffi::OsString;
use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use image::Rgba;
use sha2::Digest;
use sha2::Sha256;

/// Every PNG file starts with these bytes.
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// A red image of 40 by 30 pixels.
const RED_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="red"/></svg>"#;
pub(crate) const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);

/// How long the server of `serve_once` waits for a request, and then for each read.
const SERVER_WAIT: Duration = Duration::from_secs(10);

/// A blue image of 20 by 10 pixels.
const BLUE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="blue"/></svg>"#;
pub(crate) const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);

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

/// `table-to-image` draws each image of a table from the input's file, also one that a root path
/// `/x` links: Chrome loads the table's page from a temporary directory, where the image's URL
/// names no file.
#[test]
fn test_process_table_to_image_input_images() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input_dir = dir.path().join("posts");
    fs::create_dir_all(input_dir.join("img"))?;
    fs::write(input_dir.join("img/r.svg"), RED_SVG)?;
    fs::write(input_dir.join("b.svg"), BLUE_SVG)?;
    let input = input_dir.join("post.md");
    let table = "| r | b |\n|---|---|\n| ![r](img/r.svg) | ![b](/b.svg) |\n";
    fs::write(&input, table)?;
    let output_dir = dir.path().join("out");
    let output = output_dir.join("post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "table-to-image", "--scale", "1"])
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "![](post-table-d295bbb3e858.png)\n");

    // At scale 1, the PNG holds every pixel of each image.
    let png = output_dir.join("post-table-d295bbb3e858.png");
    let red = count_pixels(&png, RED)?;
    assert_eq!(red, 40 * 30);
    let blue = count_pixels(&png, BLUE)?;
    assert_eq!(blue, 20 * 10);
    Ok(())
}

/// After `image-to-asset`, `table-to-image` draws the copy of the table's image, which the table
/// links relative to the output file, or under `--url-base` before the copy is online.
#[test]
fn test_process_table_to_image_copied_image() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input_dir = dir.path().join("posts");
    fs::create_dir_all(&input_dir)?;
    fs::write(input_dir.join("r.svg"), RED_SVG)?;
    let input = input_dir.join("post.md");
    fs::write(&input, "| r |\n|---|\n| ![r](r.svg) |\n")?;

    // The `--url-base` arguments, the start of the link that they give, and the name of the PNG.
    let cases = [
        (vec![], "", "post-table-7ab66c2bb4b8.png"),
        (
            vec!["--url-base", "https://cdn.invalid/out"],
            "https://cdn.invalid/out/",
            "post-table-572a6847bf48.png",
        ),
    ];
    for (url_base_args, link_start, png_name) in cases {
        let output_dir = tempfile::tempdir()?;
        let output = output_dir.path().join("post.md");

        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["process", "--scale", "1", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(["--action", "image-to-asset", "--action", "table-to-image"])
            .args(url_base_args)
            .output()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let markdown = fs::read_to_string(&output)?;
        let expected_markdown = format!("![]({link_start}{png_name})\n");
        assert_eq!(markdown, expected_markdown);

        let png = output_dir.path().join(png_name);
        let red = count_pixels(&png, RED)?;
        assert_eq!(red, 40 * 30, "{expected_markdown}");
    }
    Ok(())
}

/// `table-to-image` draws a table's image from the input's directory, also when the output's
/// directory holds another file of that name.
#[test]
fn test_process_table_to_image_output_collision() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input_dir = dir.path().join("posts");
    fs::create_dir_all(&input_dir)?;
    fs::write(input_dir.join("pic.svg"), RED_SVG)?;
    let input = input_dir.join("post.md");
    fs::write(&input, "| p |\n|---|\n| ![p](pic.svg) |\n")?;
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&output_dir)?;
    fs::write(output_dir.join("pic.svg"), BLUE_SVG)?;
    let output = output_dir.join("post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "table-to-image", "--scale", "1"])
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "![](post-table-4d9d4f8cfc27.png)\n");

    let png = output_dir.join("post-table-4d9d4f8cfc27.png");
    let red = count_pixels(&png, RED)?;
    assert_eq!(red, 40 * 30);
    let blue = count_pixels(&png, BLUE)?;
    assert_eq!(blue, 0);
    Ok(())
}

/// `table-to-image` draws the file that an image URL's decoded path names, with the URL's
/// fragment: `#icon` makes the SVG's `rect:target` rule paint the rectangle blue.
#[test]
fn test_process_table_to_image_url_escapes() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><style>rect { fill: red; } rect:target { fill: blue; }</style><rect id="icon" width="40" height="30"/></svg>"#;
    fs::write(dir.path().join("my pic.svg"), svg)?;
    let input = dir.path().join("post.md");
    fs::write(&input, "| p |\n|---|\n| ![p](my%20pic.svg#icon) |\n")?;
    let output_dir = dir.path().join("out");
    let output = output_dir.join("post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "table-to-image", "--scale", "1"])
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "![](post-table-057b5e00a0eb.png)\n");

    let png = output_dir.join("post-table-057b5e00a0eb.png");
    let blue = count_pixels(&png, BLUE)?;
    assert_eq!(blue, 40 * 30);
    let red = count_pixels(&png, RED)?;
    assert_eq!(red, 0);
    Ok(())
}

/// How many pixels of the PNG at `path` have `color`.
pub(crate) fn count_pixels(path: &Path, color: Rgba<u8>) -> Result<usize> {
    let image = image::open(path)?;
    let rgba = image.to_rgba8();
    let pixels = rgba.pixels();
    let count = pixels.filter(|pixel| **pixel == color).count();
    Ok(count)
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

/// `xpmd process --action code-to-image=4294967295` fails with an error instead of a panic: the
/// width plus the page padding passes `u32::MAX`, a window that Chrome cannot take.
#[test]
fn test_process_code_to_image_too_wide() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "```\ncode\n```\n")?;
    let output = dir.path().join("out.md");
    let assets = dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "code-to-image=4294967295", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        // CI sets RUST_BACKTRACE, which would add a backtrace to the error.
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .output()?;

    assert_eq!(result.status.code(), Some(1));

    let stderr = String::from_utf8(result.stderr)?;
    let expected_stderr = "Error: Unsupported window of 4294967295x2000 pixels at scale 2: each \
                           side and the scale must be at least 1, and each side times the scale \
                           at most 2147483647\n";
    assert_eq!(stderr, expected_stderr);
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

/// `math-to-image` keeps both ends of a formula wider than the window: the image holds the blue
/// square at its start and the red square at its end.
#[test]
fn test_process_math_to_image_wide() -> Result<()> {
    let blue_square = r"{\color{blue}\rule{1em}{1em}}";
    let red_square = r"{\color{red}\rule{1em}{1em}}";
    let terms = vec!["x"; 100].join("+");
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, format!("$${blue_square}+{terms}+{red_square}$$\n"))?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "math-to-image", "--scale", "1", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "![](post-math-eb057363ada7.png)\n");

    let png = dir.path().join("out/post-math-eb057363ada7.png");
    let blue = count_pixels(&png, BLUE)?;
    assert!(blue > 0, "the formula lost its start");
    let red = count_pixels(&png, RED)?;
    assert!(red > 0, "the formula lost its end");
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

/// `xpmd process --action append-reference-list` lists the definition a link uses at the end.
#[test]
fn test_process_append_reference_list() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(
        &input,
        "See [the post][p] and [c](http://c.com).\n\n[p]: http://p.com \"Post\"\n",
    )?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "append-reference-list", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "See [the post](http://p.com \"Post\") and [c](http://c.com).\n\n\
                             Reference:\n\n\
                             - Post : <http://p.com>\n";
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// The reference list holds the URL that `rewrite-link-urls` gives, whether it runs before or after
/// `append-reference-list`.
#[test]
fn test_process_rewrite_reference_list() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "See [p][p].\n\n[p]: https://old.com/p \"P\"\n")?;
    let output = dir.path().join("out/post.md");
    let link_rule = "rewrite-link-urls=|^https://old\\.com/|https://new.com/|";

    let orders = [["append-reference-list", link_rule], [
        link_rule,
        "append-reference-list",
    ]];
    for [first, second] in orders {
        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["process", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(["--action", first, "--action", second])
            .output()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let markdown = fs::read_to_string(&output)?;
        let expected_markdown = "See [p](https://new.com/p \"P\").\n\n\
                                 Reference:\n\n\
                                 - P : <https://new.com/p>\n";
        assert_eq!(markdown, expected_markdown);
    }
    Ok(())
}

/// `xpmd process --action math-block-to-one-line` prints a `$$` formula in a list item on one line.
#[test]
fn test_process_math_block_to_one_line() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "- Sum $$\n  a + b\n  $$\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "math-block-to-one-line", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "- Sum $$a + b$$\n");
    Ok(())
}

/// `xpmd process --action join-math-block` reads a `$$` formula that a blank line splits as one,
/// with its TeX as written, also for an action listed before it.
#[test]
fn test_process_join_math_block() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "- $$\n  a\\,b_{i} *c*\n\n  d\n  $$\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "math-block-to-one-line"])
        .args(["--action", "join-math-block", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "- $$a\\,b_{i} *c* d$$\n");
    Ok(())
}

/// `join-math-block` joins a split `$$` formula in each embedded file too, also in a file that an
/// embedded file embeds, wherever the action is listed.
#[test]
fn test_process_embed_join_math_block() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(
        dir.path().join("sub.md"),
        "$$\nc\n\nd\n$$\n\n![](inner.md)\n",
    )?;
    fs::write(dir.path().join("inner.md"), "$$\ne\n\nf\n$$\n")?;
    let input = dir.path().join("post.md");
    fs::write(&input, "$$\na\n\nb\n$$\n\n![](sub.md)\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "embed-markdown"])
        .args(["--action", "join-math-block", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "$$\na\nb\n$$\n\n$$\nc\nd\n$$\n\n$$\ne\nf\n$$\n";
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// `xpmd process --action math-inline-to-text` replaces an inline formula with Unicode text.
#[test]
fn test_process_math_inline_to_text() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "Let $x^2 \\in \\mathbb{R}$.\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "math-inline-to-text", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "Let x² ∈ ℝ.\n");
    Ok(())
}

/// `xpmd process --action codespan-to-text` writes each code span as escaped text.
#[test]
fn test_process_codespan_to_text() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "Run `a <b>` now.\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "codespan-to-text", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "Run a \\<b\\> now.\n");
    Ok(())
}

/// `xpmd process` keeps an escaped `\$` escaped, and escapes the `$` of a code span that
/// `codespan-to-text` turns into text, so neither becomes a formula.
#[test]
fn test_process_escaped_dollar() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "Price \\$a\\$ and `$x$`.\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "codespan-to-text", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "Price \\$a\\$ and \\$x\\$.\n");
    Ok(())
}

/// `xpmd process --action flatten-lists` writes each list item and quote as a plain paragraph.
#[test]
fn test_process_flatten_lists() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "- a\n- b\n\n> c\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "flatten-lists", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "a\n\nb\n\nc\n");
    Ok(())
}

/// `xpmd process --action rewrite-image-urls=.. --action rewrite-link-urls=..` rewrites the image
/// and the link URLs, with any delimiter and Python's `\1`.
#[test]
fn test_process_rewrite_urls() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "![a](assets/x.png) [b](posts/y.md)\n")?;
    let output = dir.path().join("out/post.md");
    let image_rule = "rewrite-image-urls=|^assets/|https://cdn.com/|";
    let link_rule = "rewrite-link-urls=#^posts/(.*)\\.md$#https://blog.com/\\1/#";

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--action", image_rule, "--action", link_rule])
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected = "![a](https://cdn.com/x.png) [b](https://blog.com/y/)\n";
    assert_eq!(markdown, expected);
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

/// `embed-markdown` embeds a `.md` image by default; `--embed REGEX` replaces that default, so the
/// `.md` image stays and the image whose URL REGEX matches is embedded.
#[test]
fn test_process_embed() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.md"), "A\n")?;
    fs::write(dir.path().join("b.txt"), "B\n")?;
    let input = dir.path().join("post.md");
    fs::write(&input, "![](a.md)\n\n![](b.txt)\n")?;
    let output = dir.path().join("out/post.md");

    // The `--embed` arguments, and the markdown that they give.
    let cases = [
        (vec![], "A\n\n![](b.txt)\n"),
        (vec!["--embed", "[.]txt$"], "![](a.md)\n\nB\n"),
    ];
    for (embed_args, expected_markdown) in cases {
        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["process", "--action", "embed-markdown", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(embed_args)
            .output()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let markdown = fs::read_to_string(&output)?;
        assert_eq!(markdown, expected_markdown);
    }
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

/// `image-to-asset` keeps the link to an asset: a second `image-to-asset` keeps the copy that the
/// first one made, and one after `table-to-image` keeps the table's PNG.
#[test]
fn test_process_image_to_asset_keeps_assets() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input_dir = dir.path().join("posts");
    fs::create_dir_all(&input_dir)?;
    fs::write(input_dir.join("pic.svg"), RED_SVG)?;
    let input = input_dir.join("post.md");

    // The input markdown, the `--action` arguments, the markdown that they give, and the one asset
    // that they create.
    let cases = [
        (
            "![p](pic.svg)\n",
            ["image-to-asset", "image-to-asset"],
            "![p](aa5290abd426-pic.svg)\n",
            "aa5290abd426-pic.svg",
        ),
        (
            "| p |\n|---|\n| 1 |\n",
            ["table-to-image", "image-to-asset"],
            "![](post-table-37a852938d98.png)\n",
            "post-table-37a852938d98.png",
        ),
    ];
    for (input_markdown, actions, expected_markdown, asset) in cases {
        fs::write(&input, input_markdown)?;
        let output_dir = tempfile::tempdir()?;
        let output = output_dir.path().join("post.md");

        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["process", "--action", actions[0], "--action", actions[1]])
            .arg("-i")
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let markdown = fs::read_to_string(&output)?;
        assert_eq!(markdown, expected_markdown);

        let mut files = Vec::new();
        for entry in fs::read_dir(output_dir.path())? {
            files.push(entry?.file_name());
        }
        files.sort();
        assert_eq!(files, [asset, "post.md"]);
    }
    Ok(())
}

/// `image-to-asset` copies the file that an image URL's decoded path names, and links the copy with
/// each part of its path encoded, also the `#` of `--assets`, and with the URL's query and fragment.
#[test]
fn test_process_image_to_asset_url_escapes() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input_dir = dir.path().join("posts");
    fs::create_dir_all(&input_dir)?;
    for name in ["my pic.svg", "图.svg", "100%.svg", "pic.svg"] {
        fs::write(input_dir.join(name), RED_SVG)?;
    }
    let input = input_dir.join("post.md");
    fs::write(
        &input,
        "![a](my%20pic.svg) ![b](<my pic.svg>) ![c](%E5%9B%BE.svg) ![d](图.svg) \
         ![e](100%25.svg) ![f](pic.svg?v=2) ![g](pic.svg#icon)\n",
    )?;
    let output = dir.path().join("out/post.md");
    let assets = dir.path().join("out/assets#1");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "image-to-asset", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .args(["--url-base", "https://cdn.invalid/out"])
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let base = "https://cdn.invalid/out/assets%231/aa5290abd426";
    let expected_markdown = format!(
        "![a]({base}-my%20pic.svg) ![b]({base}-my%20pic.svg) ![c]({base}-图.svg) \
         ![d]({base}-图.svg) ![e]({base}-100%25.svg) ![f]({base}-pic.svg?v=2) \
         ![g]({base}-pic.svg#icon)\n"
    );
    assert_eq!(markdown, expected_markdown);

    let mut copies = Vec::new();
    for entry in fs::read_dir(&assets)? {
        let entry = entry?;
        let copied = fs::read(entry.path())?;
        assert_eq!(copied, RED_SVG.as_bytes());
        copies.push(entry.file_name());
    }
    copies.sort();
    let expected_copies = [
        "aa5290abd426-100%.svg",
        "aa5290abd426-my pic.svg",
        "aa5290abd426-pic.svg",
        "aa5290abd426-图.svg",
    ];
    assert_eq!(copies, expected_copies);
    Ok(())
}

/// `xpmd process --url-base URL` links the copy as URL plus its path relative to the output file;
/// a trailing `/` on URL is not doubled.
#[test]
fn test_process_url_base() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input_dir = dir.path().join("posts");
    fs::create_dir_all(input_dir.join("img"))?;
    fs::write(input_dir.join("img/a.png"), b"PNG-A")?;
    let input = input_dir.join("post.md");
    fs::write(&input, "![a](img/a.png)\n")?;
    let output = dir.path().join("out/post.md");
    let assets = dir.path().join("out/assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "image-to-asset", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .args(["--url-base", "https://cdn.com/gh/u/r@b/out/"])
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected = "![a](https://cdn.com/gh/u/r@b/out/assets/e793c41f42c3-a.png)\n";
    assert_eq!(markdown, expected);
    Ok(())
}

/// With `--url-base`, an `--assets` outside the output file's directory is an error, and `process`
/// writes nothing.
#[test]
fn test_process_url_base_assets_outside() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "Text.\n")?;
    let output_dir = dir.path().join("out");
    let output = output_dir.join("post.md");
    let assets = dir.path().join("assets");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "image-to-asset", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--assets")
        .arg(&assets)
        .args(["--url-base", "https://cdn.com/"])
        // CI sets RUST_BACKTRACE, which would add a backtrace to the error.
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .output()?;

    assert_eq!(result.status.code(), Some(1));

    let stderr = String::from_utf8(result.stderr)?;
    let expected_stderr = format!(
        "Error: With --url-base, --assets must be inside the output file's directory {}: {}\n",
        output_dir.display(),
        assets.display()
    );
    assert_eq!(stderr, expected_stderr);

    assert!(!output_dir.exists());
    Ok(())
}

/// `download-images` after `image-to-asset` keeps the link under `--url-base`: it links a file that
/// `image-to-asset` created, which is not online yet.
#[test]
fn test_process_url_base_download_images() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.png"), b"PNG-A")?;
    let input = dir.path().join("post.md");
    fs::write(&input, "![a](a.png)\n")?;
    let output = dir.path().join("out/post.md");

    // The `.invalid` domain never resolves, so a download from it fails.
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--url-base", "https://cdn.invalid/out"])
        .args(["--action", "image-to-asset", "--action", "download-images"])
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected = "![a](https://cdn.invalid/out/e793c41f42c3-a.png)\n";
    assert_eq!(markdown, expected);
    Ok(())
}

/// `download-images` downloads a source image under `--url-base`: only a file that an action
/// created or copied keeps its link there.
#[test]
fn test_process_download_images_under_url_base() -> Result<()> {
    let (port, server) = serve_once(b"PNG-A")?;
    let url_base = format!("http://127.0.0.1:{port}/out");
    let url = format!("{url_base}/a.png");
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, format!("![a]({url})\n"))?;
    let output_dir = dir.path().join("out");
    let output = output_dir.join("post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "download-images", "--url-base"])
        .arg(&url_base)
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let request_line = server.join().unwrap()?;
    assert_eq!(request_line, "GET /out/a.png HTTP/1.1");

    let digest = Sha256::digest(url.as_bytes());
    let hash = format!("{digest:x}");
    let name = format!("{}-a.png", &hash[..12]);
    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, format!("![a]({url_base}/{name})\n"));

    let downloaded = fs::read(output_dir.join(&name))?;
    assert_eq!(downloaded, b"PNG-A");
    Ok(())
}

/// Answer one HTTP request on a free port of 127.0.0.1 with `200 OK` and `body`, in a thread.
/// Return the port and the thread, which returns the request line, or an error when no request
/// comes within `SERVER_WAIT` or the client closes the connection before the request ends.
fn serve_once(body: &'static [u8]) -> Result<(u16, JoinHandle<Result<String>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    // A non-blocking accept lets the thread give up when no request comes.
    listener.set_nonblocking(true)?;

    let server = thread::spawn(move || -> Result<String> {
        let start = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if start.elapsed() > SERVER_WAIT {
                        anyhow::bail!("No request came within {SERVER_WAIT:?}");
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(SERVER_WAIT))?;

        let mut request = Vec::new();
        let mut chunk = [0u8; 1024];
        while !request.ends_with(b"\r\n\r\n") {
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                anyhow::bail!("The client closed the connection before the request ended");
            }
            request.extend_from_slice(&chunk[..n]);
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)?;

        let request = String::from_utf8(request)?;
        let request_line = request.lines().next().unwrap_or_default();
        Ok(request_line.to_string())
    });
    Ok((port, server))
}

/// `xpmd process --preset github` drops the front matter, copies the image, prints the `$$` formula
/// in the list item on one line and lists the reference; an `--action` runs after the preset, so
/// `rewrite-link-urls` rewrites the reference list too.
#[test]
fn test_process_preset() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("a.png"), b"PNG-A")?;
    let input = dir.path().join("post.md");
    fs::write(
        &input,
        "---\ntitle: T\n---\n\n![a](a.png)\n\n- Sum $$\n  a + b\n  $$\n\n\
         See [the post][p].\n\n[p]: https://old.com/p \"Post\"\n",
    )?;
    let output = dir.path().join("out/post.md");
    let link_rule = "rewrite-link-urls=|^https://old\\.com/|https://new.com/|";

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--preset", "github", "--action", link_rule, "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "![a](e793c41f42c3-a.png)\n\n\
                             - Sum $$a + b$$\n\n\
                             See [the post](https://new.com/p \"Post\").\n\n\
                             Reference:\n\n\
                             - Post : <https://new.com/p>\n";
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// `xpmd process --preset transparent --keep-front-matter` keeps the front matter.
#[test]
fn test_process_preset_keep_front_matter() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("post.md");
    fs::write(&input, "---\ntitle: T\n---\n\n# Title\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--preset", "transparent", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--keep-front-matter")
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    assert_eq!(markdown, "---\ntitle: T\n---\n\n# Title\n");
    Ok(())
}

/// `xpmd process --refs FILE --preset zhihu` resolves a reference that the markdown does not
/// define: from the `universal` list of the file, from the front matter's `refs`, and from its
/// `platform_refs.zhihu`, which replaces `refs`; the markdown's own definition wins, and the
/// reference list holds them all.
#[test]
fn test_process_refs() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ref_file = dir.path().join("refs.yaml");
    fs::write(
        &ref_file,
        "universal:\n  - pb: https://pb.com \"protobuf\"\n  - own: https://other.com\n",
    )?;
    let input = dir.path().join("post.md");
    fs::write(
        &input,
        "---\nrefs:\n  - grpc: https://grpc.io\nplatform_refs:\n  zhihu:\n    - grpc: https://z.com/grpc\n---\n\n\
         See [grpc][], [pb][] and [own][].\n\n[own]: https://own.com\n",
    )?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--preset", "zhihu", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--refs")
        .arg(&ref_file)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "See [grpc](https://z.com/grpc), [pb](https://pb.com \"protobuf\") \
                             and [own](https://own.com).\n\n\
                             Reference:\n\n\
                             - grpc : <https://z.com/grpc>\n\n\
                             - own : <https://own.com>\n\n\
                             - protobuf : <https://pb.com>\n";
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// An embedded file resolves a reference that it does not define with the `--refs` files and the
/// front matter of each file that embeds it, and its own front matter replaces those for itself
/// and for the files that it embeds.
#[test]
fn test_process_embed_refs() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ref_file = dir.path().join("refs.yaml");
    fs::write(
        &ref_file,
        "universal:\n  doc: https://example.invalid/doc\n",
    )?;
    fs::write(
        dir.path().join("sub.md"),
        "---\nrefs:\n  b: https://b.invalid/sub\n---\n\n[doc], [a] and [b].\n\n![](inner.md)\n",
    )?;
    fs::write(
        dir.path().join("inner.md"),
        "[b] and [c].\n\n[c]: https://c.invalid/inner\n",
    )?;
    let input = dir.path().join("post.md");
    fs::write(
        &input,
        "---\nrefs:\n  a: https://a.invalid/post\n  b: https://b.invalid/post\n---\n\n\
         ![](sub.md)\n\nPost [b].\n",
    )?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "embed-markdown"])
        .args(["--action", "drop-front-matter", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--refs")
        .arg(&ref_file)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "[doc](https://example.invalid/doc), [a](https://a.invalid/post) \
                             and [b](https://b.invalid/sub).\n\n\
                             [b](https://b.invalid/sub) and [c](https://c.invalid/inner).\n\n\
                             Post [b](https://b.invalid/post).\n";
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// `append-reference-list` lists the definitions that the links of an embedded file use: the
/// file's own and those of `--refs`.
#[test]
fn test_process_embed_reference_list() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let ref_file = dir.path().join("refs.yaml");
    fs::write(
        &ref_file,
        "universal:\n  doc: https://example.invalid/doc\n",
    )?;
    fs::write(
        dir.path().join("sub.md"),
        "See [used][spec] and [doc].\n\n[spec]: https://example.invalid/spec\n",
    )?;
    let input = dir.path().join("post.md");
    fs::write(&input, "![](sub.md)\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--preset", "transparent", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--refs")
        .arg(&ref_file)
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let markdown = fs::read_to_string(&output)?;
    let expected_markdown = "See [used](https://example.invalid/spec) and \
                             [doc](https://example.invalid/doc).\n\n\
                             Reference:\n\n\
                             - doc : <https://example.invalid/doc>\n\n\
                             - spec : <https://example.invalid/spec>\n";
    assert_eq!(markdown, expected_markdown);
    Ok(())
}

/// Without `-i`, `-o` and `--action`, `xpmd process` reads stdin, and writes only the markdown,
/// parsed and printed again, to stdout.
#[test]
fn test_process_stdin_to_stdout() -> Result<()> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .arg("process")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"_a_\n")?;
    drop(stdin);
    let result = child.wait_with_output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "*a*\n");
    Ok(())
}

/// With stdin and stdout, `xpmd process` takes the current directory as the directory of the input
/// and of the output, and names the files that the actions create after stdin.
#[test]
fn test_process_stdin_to_stdout_files() -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::create_dir_all(dir.path().join("img"))?;
    fs::write(dir.path().join("img/a.png"), b"PNG-A")?;

    let mut child = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "image-to-asset"])
        .args(["--action", "table-to-image"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"![a](img/a.png)\n\n| x |\n|---|\n| 1 |\n")?;
    drop(stdin);
    let result = child.wait_with_output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    let expected_stdout = "![a](e793c41f42c3-a.png)\n\n![](stdin-table-6755bbc6e713.png)\n";
    assert_eq!(stdout, expected_stdout);

    let copied = fs::read(dir.path().join("e793c41f42c3-a.png"))?;
    assert_eq!(copied, b"PNG-A");

    let image = fs::read(dir.path().join("stdin-table-6755bbc6e713.png"))?;
    let magic = image.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// With `-i` and without `-o`, `xpmd process` names the files that the actions create after the
/// input file, and puts them in the current directory.
#[test]
fn test_process_file_to_stdout() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/table.md");
    let dir = tempfile::tempdir()?;

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "table-to-image", "-i"])
        .arg(&input)
        .current_dir(dir.path())
        .output()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let stdout = String::from_utf8(result.stdout)?;
    let expected_stdout = "# Tables\n\n\
        Before the table.\n\n\
        ![](table-table-ef022588a517.png)\n\n\
        After the table.\n";
    assert_eq!(stdout, expected_stdout);

    let image = fs::read(dir.path().join("table-table-ef022588a517.png"))?;
    let magic = image.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    Ok(())
}

/// `xpmd process` looks for Chrome and ImageMagick only when an action renders something: with
/// neither in PATH, a run without actions, or whose actions render nothing, still works.
#[test]
fn test_process_without_renderer() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let empty_path = dir.path().join("bin");
    fs::create_dir_all(&empty_path)?;
    let input = dir.path().join("post.md");
    fs::write(&input, "_a_ `b`\n")?;
    let output = dir.path().join("out/post.md");

    // The `--action` arguments, and the markdown that they give.
    let cases = [
        (vec![], "*a* `b`\n"),
        (vec!["--action", "codespan-to-text"], "*a* b\n"),
        (vec!["--action", "table-to-image"], "*a* `b`\n"),
    ];
    for (action_args, expected_markdown) in cases {
        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["process", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(action_args)
            .env("PATH", &empty_path)
            .output()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let markdown = fs::read_to_string(&output)?;
        assert_eq!(markdown, expected_markdown);
    }
    Ok(())
}

/// Without Chrome in PATH, an action that renders fails with the help to install Chrome. macOS
/// finds Chrome outside PATH, so the test cannot hide it there.
#[test]
#[cfg_attr(target_os = "macos", ignore = "macOS finds Chrome outside PATH")]
fn test_process_renderer_missing() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let empty_path = dir.path().join("bin");
    fs::create_dir_all(&empty_path)?;
    let input = dir.path().join("post.md");
    fs::write(&input, "| x |\n|---|\n| 1 |\n")?;
    let output = dir.path().join("out/post.md");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["process", "--action", "table-to-image", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .env("PATH", &empty_path)
        .output()?;

    let succeeded = result.status.success();
    assert!(!succeeded);

    let stderr = String::from_utf8(result.stderr)?;
    let first_line = stderr.lines().next();
    let expected_first_line = "Error: Failed to render content. \
                               Make sure Chrome/Chromium is installed and accessible.";
    assert_eq!(first_line, Some(expected_first_line));
    Ok(())
}
