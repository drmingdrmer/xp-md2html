use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;

use super::xpmd_tests::BoundedOutput;
use super::xpmd_tests::TestDir;

// Include directories and symlinks so a rejected batch cannot leave any work behind.
fn tree(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = PathBuf::from(entry.file_name());
        let kind = entry.file_type()?;
        let data = if kind.is_symlink() {
            let target = fs::read_link(entry.path())?;
            target.as_os_str().as_encoded_bytes().to_vec()
        } else if kind.is_dir() {
            let children = tree(&entry.path())?;
            for (child, data) in children {
                result.insert(name.join(child), data);
            }
            Vec::new()
        } else {
            fs::read(entry.path())?
        };
        result.insert(name, data);
    }
    Ok(result)
}

fn reject(root: &Path, args: &[&str], message: &str) -> Result<()> {
    let before = tree(root)?;
    let output = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .current_dir(root)
        .arg("process")
        .args(args)
        .output_bounded()?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stdout, b"");
    let stderr = String::from_utf8(output.stderr)?;
    let contains_message = stderr.contains(message);
    assert!(contains_message, "{stderr}");
    let after = tree(root)?;
    assert_eq!(after, before);
    Ok(())
}

#[test]
fn templates_copy_complete_per_article_assets() -> Result<()> {
    let dir = TestDir::new()?;
    fs::write(dir.path().join("2021-06-11-first.md"), "![a](a.svg)\n")?;
    fs::write(dir.path().join("second.md"), "![b](b.svg)\n")?;
    fs::write(dir.path().join("a.svg"), "first image")?;
    fs::write(dir.path().join("b.svg"), "second image")?;
    Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .current_dir(dir.path())
        .args(["process", "2021-06-11-first.md", "second.md"])
        .args(["--preset", "transparent", "-o", "out/{title}.md"])
        .output_ok()?;
    let mut expected = BTreeMap::new();
    for (title, basename, content, hash) in [
        ("first", "a.svg", "first image", "fb4516b50946"),
        ("second", "b.svg", "second image", "cd2c3f6bc272"),
    ] {
        let asset = format!("{title}/{hash}-{basename}");
        let alt = basename.trim_end_matches(".svg");
        let markdown = format!("![{alt}]({asset})\n");
        expected.insert(PathBuf::from(format!("{title}.md")), markdown.into_bytes());
        expected.insert(PathBuf::from(title), Vec::new());
        expected.insert(PathBuf::from(asset), content.as_bytes().to_vec());
    }
    let actual = tree(&dir.path().join("out"))?;
    assert_eq!(actual, expected);
    dir.close()
}

#[test]
fn directory_output_and_jekyll_preserve_the_expected_names_and_front_matter() -> Result<()> {
    let dir = TestDir::new()?;
    let markdown = "---\ntitle: Front matter title\n---\n\nBody\n";
    fs::write(dir.path().join("2021-06-11-first.md"), markdown)?;
    fs::write(dir.path().join("second.md"), markdown)?;
    fs::create_dir(dir.path().join("plain"))?;
    for jekyll in [false, true] {
        let destination = if jekyll { "jekyll/" } else { "plain/" };
        let mut command = Command::new(env!("CARGO_BIN_EXE_xpmd"));
        command.current_dir(dir.path());
        command.args(["process", "2021-06-11-first.md", "second.md"]);
        command.args(["--preset", "transparent", "-o", destination]);
        if jekyll {
            command.arg("--jekyll");
        }
        command.output_ok()?;
        let first = if jekyll {
            "2021-06-11-first.md"
        } else {
            "first.md"
        };
        let expected_markdown = if jekyll { markdown } else { "Body\n" };
        let mut expected = BTreeMap::new();
        expected.insert(PathBuf::from(first), expected_markdown.as_bytes().to_vec());
        expected.insert(
            PathBuf::from("second.md"),
            expected_markdown.as_bytes().to_vec(),
        );
        expected.insert(PathBuf::from("first"), Vec::new());
        expected.insert(PathBuf::from("second"), Vec::new());
        let actual = tree(&dir.path().join(destination))?;
        assert_eq!(actual, expected);
    }
    dir.close()
}

#[test]
fn asset_and_url_templates_and_literal_braces() -> Result<()> {
    let dir = TestDir::new()?;
    fs::write(dir.path().join("2021-06-11-first.md"), "![a](a.svg)\n")?;
    fs::write(dir.path().join("a.svg"), "image")?;
    Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .current_dir(dir.path())
        .args(["process", "-i", "2021-06-11-first.md"])
        .args(["--preset", "transparent", "-o", "out/{title}/{{index}}.md"])
        .args([
            "--assets",
            "out/{title}/assets",
            "--url-base",
            "/posts/{title}",
        ])
        .output_ok()?;
    let hash = "6105d6cc76af";
    let expected = format!("![a](/posts/first/assets/{hash}-a.svg)\n");
    let actual = fs::read_to_string(dir.path().join("out/first/{index}.md"))?;
    assert_eq!(actual, expected);
    let asset = dir.path().join(format!("out/first/assets/{hash}-a.svg"));
    let actual = fs::read(asset)?;
    assert_eq!(actual, b"image");
    dir.close()
}

#[test]
fn all_expanded_paths_are_validated_before_work() -> Result<()> {
    let dir = TestDir::new()?;
    fs::write(dir.path().join("a.md"), "first\n")?;
    fs::write(dir.path().join("b.md"), "second\n")?;
    fs::create_dir_all(dir.path().join("out/b.md"))?;
    reject(
        dir.path(),
        &["a.md", "b.md", "-o", "out/{title}.md"],
        "output path",
    )?;
    reject(
        dir.path(),
        &[
            "a.md",
            "b.md",
            "-o",
            "out/{title}/index.md",
            "--assets",
            "out/a/assets",
            "--url-base",
            "/posts/{title}",
        ],
        "With --url-base",
    )?;
    fs::write(dir.path().join("asset-b"), "keep")?;
    reject(
        dir.path(),
        &[
            "a.md",
            "b.md",
            "-o",
            "{title}-out.md",
            "--assets",
            "asset-{title}",
        ],
        "Not a directory",
    )?;
    reject(
        dir.path(),
        &["a.md", "missing.md", "-o", "{title}-out.md"],
        "Invalid input",
    )?;
    reject(dir.path(), &["a.md", "b.md"], "Multiple inputs require")?;
    dir.close()
}

#[test]
fn placeholders_and_unmatched_braces_are_usage_errors() -> Result<()> {
    let dir = TestDir::new()?;
    for template in [
        "{name}",
        "{title:>10}",
        "{title.x}",
        "{}",
        "{title",
        "title}",
    ] {
        reject(
            dir.path(),
            &["-i", "missing.md", "-o", template],
            "only placeholder",
        )?;
    }
    reject(dir.path(), &["--jekyll"], "requires an input")?;
    reject(dir.path(), &["-o", "{title}.md"], "requires an input")?;
    dir.close()
}

#[test]
fn duplicate_outputs_and_other_inputs_are_rejected() -> Result<()> {
    let dir = TestDir::new()?;
    fs::write(dir.path().join("a.md"), "first\n")?;
    fs::write(dir.path().join("ax.md"), "second\n")?;
    fs::write(dir.path().join("2021-06-11-a.md"), "third\n")?;
    fs::write(dir.path().join("aa.md"), "fourth\n")?;
    reject(
        dir.path(),
        &[
            "a.md",
            "aa.md",
            "-o",
            "{title}{title}/../{title}",
            "--assets",
            "assets",
        ],
        "also used as a directory",
    )?;
    fs::create_dir(dir.path().join("nested"))?;
    reject(
        dir.path(),
        &["a.md", "ax.md", "-o", "{title}x.md"],
        "aliases another input",
    )?;
    reject(
        dir.path(),
        &["a.md", "ax.md", "-o", "nested/../{title}x.md"],
        "aliases another input",
    )?;
    reject(
        dir.path(),
        &["a.md", "2021-06-11-a.md", "-o", "out/{title}.md"],
        "Duplicate output",
    )?;
    reject(
        dir.path(),
        &["a.md", "ax.md", "-o", "out/{title}/../same.md"],
        "Duplicate output",
    )?;
    dir.close()
}

#[test]
fn hard_link_aliases_are_rejected() -> Result<()> {
    let dir = TestDir::new()?;
    fs::write(dir.path().join("a.md"), "first\n")?;
    fs::write(dir.path().join("b.md"), "second\n")?;
    fs::hard_link(dir.path().join("b.md"), dir.path().join("ax.md"))?;
    reject(
        dir.path(),
        &["a.md", "b.md", "-o", "{title}x.md"],
        "aliases another input",
    )?;
    fs::write(dir.path().join("ao.md"), "output\n")?;
    fs::hard_link(dir.path().join("ao.md"), dir.path().join("bo.md"))?;
    reject(
        dir.path(),
        &["a.md", "b.md", "-o", "{title}o.md"],
        "Duplicate output",
    )?;
    dir.close()
}

#[cfg(unix)]
#[test]
fn symlink_aliases_and_symlink_before_parent_are_rejected() -> Result<()> {
    use std::os::unix::fs::symlink;

    let dir = TestDir::new()?;
    fs::write(dir.path().join("a.md"), "first\n")?;
    fs::write(dir.path().join("b.md"), "second\n")?;
    symlink("b.md", dir.path().join("ax.md"))?;
    reject(
        dir.path(),
        &["a.md", "b.md", "-o", "{title}x.md"],
        "aliases another input",
    )?;
    fs::create_dir_all(dir.path().join("target/child"))?;
    symlink("target/child", dir.path().join("a"))?;
    symlink("target/child", dir.path().join("b"))?;
    reject(
        dir.path(),
        &["a.md", "b.md", "-o", "{title}/../same.md"],
        "Duplicate output",
    )?;
    fs::write(dir.path().join("target/a.md"), "third\n")?;
    reject(
        dir.path(),
        &["a.md", "target/a.md", "-o", "a/../{title}.md"],
        "aliases another input",
    )?;
    symlink("missing", dir.path().join("dangling"))?;
    reject(
        dir.path(),
        &["a.md", "b.md", "-o", "dangling/{title}.md"],
        "dangling symlink",
    )?;
    dir.close()
}

#[test]
fn batch_can_convert_each_input_in_place() -> Result<()> {
    let dir = TestDir::new()?;
    fs::write(dir.path().join("a.md"), "    first\n")?;
    fs::write(dir.path().join("b.md"), "    second\n")?;
    Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .current_dir(dir.path())
        .args(["process", "a.md", "b.md", "-o", "{title}.md"])
        .output_ok()?;
    let first = fs::read_to_string(dir.path().join("a.md"))?;
    assert_eq!(first, "```\nfirst\n```\n");
    let second = fs::read_to_string(dir.path().join("b.md"))?;
    assert_eq!(second, "```\nsecond\n```\n");
    dir.close()
}
