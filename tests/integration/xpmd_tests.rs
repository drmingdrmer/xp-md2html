use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;

use super::process_tests::count_pixels;
use super::process_tests::RED;
use super::process_tests::RED_SVG;

/// Every JPEG file starts with these bytes.
const JPEG_MAGIC: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// Every PNG file starts with these bytes.
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// Every PDF file starts with these bytes.
const PDF_MAGIC: &[u8] = b"%PDF-";

/// How long a test waits for one run of `xpmd`, which starts Chrome a few times at most.
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// How often a test checks whether a run has exited.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// `xpmd render-markup -o` prints nothing: `ChromeRenderer` prints nothing, and Chrome's noise is captured.
#[test]
fn test_render_prints_nothing() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/simple.html");
    let output_dir = TestDir::new()?;
    let output = output_dir.path().join("simple.png");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-w", "800", "--height", "600", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");
    let stdout = String::from_utf8(result.stdout)?;
    assert_eq!(stdout, "");

    let data = fs::read(&output)?;
    let magic = data.get(..PNG_MAGIC.len());
    assert_eq!(magic, Some(PNG_MAGIC.as_slice()));
    output_dir.close()
}

/// Without `-f`, the extension of `-o` picks the output format.
#[test]
fn test_render_takes_format_from_output_extension() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = root_dir.join("tests/fixtures/simple.html");
    let output_dir = TestDir::new()?;
    let output = output_dir.path().join("simple.jpg");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let data = fs::read(&output)?;
    let magic = data.get(..JPEG_MAGIC.len());
    assert_eq!(magic, Some(JPEG_MAGIC.as_slice()));
    output_dir.close()
}

/// `render-markup` draws each CSS pixel as 2x2 pixels by default, and as 3x3 with `--scale 3`: the
/// PNG of the 40 by 30 red SVG is red all over.
#[test]
fn test_render_scale() -> Result<()> {
    let dir = TestDir::new()?;
    let input = dir.path().join("red.svg");
    fs::write(&input, RED_SVG)?;
    let output = dir.path().join("red.png");

    // The scale arguments, the size of the PNG, and how many of its pixels are red.
    let cases = [
        (vec![], (80, 60), 80 * 60),
        (vec!["--scale", "3"], (120, 90), 120 * 90),
    ];
    for (scale_args, expected_size, expected_red) in cases {
        let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
            .args(["render-markup", "-i"])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(scale_args)
            .output_ok()?;

        let stderr = String::from_utf8(result.stderr)?;
        assert_eq!(stderr, "");

        let size = image::image_dimensions(&output)?;
        assert_eq!(size, expected_size);
        let red = count_pixels(&output, RED)?;
        assert_eq!(red, expected_red);
    }
    dir.close()
}

/// Without `-i` and `-o`, `render-markup` reads HTML from stdin and writes the image to stdout.
#[test]
fn test_render_stdin_to_stdout() -> Result<()> {
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-f", "jpg"])
        .output_ok_with_stdin(b"<html><body><h1>From stdin</h1></body></html>")?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let magic = result.stdout.get(..JPEG_MAGIC.len());
    assert_eq!(magic, Some(JPEG_MAGIC.as_slice()));
    Ok(())
}

/// Without ImageMagick, `render-markup` still writes a PDF and `render-math` an SVG: neither trims
/// an image. `XPMD_MAGICK` names a file that does not exist, so the run has no ImageMagick.
#[test]
fn test_render_without_imagemagick() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = TestDir::new()?;
    let no_magick = dir.path().join("no-magick");

    let pdf = dir.path().join("simple.pdf");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(root_dir.join("tests/fixtures/simple.html"))
        .arg("-o")
        .arg(&pdf)
        .env("XPMD_MAGICK", &no_magick)
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let data = fs::read(&pdf)?;
    let magic = data.get(..PDF_MAGIC.len());
    assert_eq!(magic, Some(PDF_MAGIC));

    let svg = dir.path().join("math.svg");
    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-math", "-i"])
        .arg(root_dir.join("tests/fixtures/math.tex"))
        .arg("-o")
        .arg(&svg)
        .env("XPMD_MAGICK", &no_magick)
        .output_ok()?;

    let stderr = String::from_utf8(result.stderr)?;
    assert_eq!(stderr, "");

    let data = fs::read_to_string(&svg)?;
    assert!(data.starts_with("<svg "), "{data}");
    dir.close()
}

/// Without ImageMagick, a PNG fails with the help to install ImageMagick, which trims it.
#[test]
fn test_render_png_without_imagemagick() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = TestDir::new()?;
    let no_magick = dir.path().join("no-magick");

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(root_dir.join("tests/fixtures/simple.html"))
        .arg("-o")
        .arg(dir.path().join("simple.png"))
        .env("XPMD_MAGICK", &no_magick)
        .output_bounded()?;

    let succeeded = result.status.success();
    assert!(!succeeded);

    let stderr = String::from_utf8(result.stderr)?;
    let first_line = stderr.lines().next();
    let expected_first_line =
        "Error: Failed to trim the image. Make sure ImageMagick is installed \
                               and accessible; an SVG or a PDF does not need it.";
    assert_eq!(first_line, Some(expected_first_line));
    dir.close()
}

/// `XPMD_CHROME` names the Chrome to run: one that does not exist fails with the help to install
/// Chrome, also on a machine that has Chrome.
#[test]
fn test_render_without_chrome() -> Result<()> {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = TestDir::new()?;

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(root_dir.join("tests/fixtures/simple.html"))
        .arg("-o")
        .arg(dir.path().join("simple.png"))
        .env("XPMD_CHROME", dir.path().join("no-chrome"))
        .output_bounded()?;

    let succeeded = result.status.success();
    assert!(!succeeded);

    let stderr = String::from_utf8(result.stderr)?;
    let first_line = stderr.lines().next();
    let expected_first_line =
        "Error: Failed to render content. Make sure Chrome/Chromium is installed and accessible.";
    assert_eq!(first_line, Some(expected_first_line));
    dir.close()
}

/// A Chrome that runs longer than `XPMD_TIMEOUT` seconds is killed, and the render fails with the
/// limit. This Chrome is a script that sleeps.
#[test]
#[cfg(unix)]
fn test_render_kills_hanging_chrome() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = TestDir::new()?;
    let chrome = dir.path().join("sleeping-chrome");
    fs::write(&chrome, "#!/bin/sh\nexec sleep 30\n")?;
    let mut permissions = fs::metadata(&chrome)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&chrome, permissions)?;
    let start = Instant::now();

    let result = Command::new(env!("CARGO_BIN_EXE_xpmd"))
        .args(["render-markup", "-i"])
        .arg(root_dir.join("tests/fixtures/simple.html"))
        .arg("-o")
        .arg(dir.path().join("simple.png"))
        .env("XPMD_CHROME", &chrome)
        .env("XPMD_TIMEOUT", "1")
        // CI sets RUST_BACKTRACE, which would add a backtrace to the error.
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .output_bounded()?;

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
    let succeeded = result.status.success();
    assert!(!succeeded);

    let stderr = String::from_utf8(result.stderr)?;
    let last_line = stderr.lines().last();
    let expected_last_line = "    Killed it after 1s; XPMD_TIMEOUT sets the limit in seconds";
    assert_eq!(last_line, Some(expected_last_line));
    dir.close()
}

/// A failed run is an error that names the command, with its exit status and its stderr.
#[test]
fn test_output_ok_fails() -> Result<()> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xpmd"));
    command
        .args(["process", "-i", "/nonexistent/post.md"])
        // CI sets RUST_BACKTRACE, which would add a backtrace to the error.
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE");

    let result = command.output_ok();

    let message = result.unwrap_err().to_string();
    let expected = format!(
        "{command:?} failed with exit status: 1; stderr:\n\
         Error: Failed to read input file: /nonexistent/post.md\n\n\
         Caused by:\n    No such file or directory (os error 2)\n"
    );
    assert_eq!(message, expected);
    Ok(())
}

/// A run that takes longer than its time is killed, and the error names the command.
#[test]
fn test_run_bounded_kills() -> Result<()> {
    let mut command = Command::new("sleep");
    command.arg("10");
    let timeout = Duration::from_millis(100);
    let start = Instant::now();

    let result = run_bounded(&mut command, None, timeout);

    let elapsed = start.elapsed();
    assert!(elapsed < RUN_TIMEOUT, "{elapsed:?}");
    let message = result.unwrap_err().to_string();
    assert_eq!(message, "\"sleep\" \"10\" ran longer than 100ms; stderr:\n");
    Ok(())
}

/// A `TestDir` that the test drops without `close`, as a failing test does, keeps its directory;
/// `close` deletes the directory and the files in it.
#[test]
fn test_test_dir_keeps_unclosed() -> Result<()> {
    let unclosed = TestDir::new()?;
    let unclosed_path = unclosed.path().to_path_buf();
    drop(unclosed);
    let is_kept = unclosed_path.is_dir();
    assert!(is_kept);
    fs::remove_dir(&unclosed_path)?;

    let closed = TestDir::new()?;
    let closed_path = closed.path().to_path_buf();
    fs::write(closed_path.join("a.txt"), "a")?;
    closed.close()?;
    let is_deleted = !closed_path.exists();
    assert!(is_deleted);
    Ok(())
}

/// Runs a command for a test, which waits for it at most [`RUN_TIMEOUT`].
pub(crate) trait BoundedOutput {
    /// The output of the command, which reads no stdin, whatever its exit status.
    fn output_bounded(&mut self) -> Result<Output>;

    /// The output of the command, which reads no stdin; an error with the exit status and the
    /// stderr when the command fails.
    fn output_ok(&mut self) -> Result<Output>;

    /// [`BoundedOutput::output_ok`] with `stdin` as the input of the command.
    fn output_ok_with_stdin(&mut self, stdin: &[u8]) -> Result<Output>;
}

impl BoundedOutput for Command {
    fn output_bounded(&mut self) -> Result<Output> {
        run_bounded(self, None, RUN_TIMEOUT)
    }

    fn output_ok(&mut self) -> Result<Output> {
        let output = run_bounded(self, None, RUN_TIMEOUT)?;
        succeeded(self, output)
    }

    fn output_ok_with_stdin(&mut self, stdin: &[u8]) -> Result<Output> {
        let output = run_bounded(self, Some(stdin), RUN_TIMEOUT)?;
        succeeded(self, output)
    }
}

/// The output of `command` with `stdin` as its input, or none, once it exits, whatever its exit
/// status; an error with its stderr when it runs longer than `timeout`, after it is killed.
fn run_bounded(command: &mut Command, stdin: Option<&[u8]>, timeout: Duration) -> Result<Output> {
    let input = if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    command
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;

    // The pipes are read while the command runs: a full pipe would stop it.
    let stdout = child
        .stdout
        .take()
        .context("The stdout of the command is not piped")?;
    let stderr = child
        .stderr
        .take()
        .context("The stderr of the command is not piped")?;
    let stdout_reader = thread::spawn(move || read_all(stdout));
    let stderr_reader = thread::spawn(move || read_all(stderr));

    if let Some(data) = stdin {
        let mut pipe = child
            .stdin
            .take()
            .context("The stdin of the command is not piped")?;
        pipe.write_all(data)?;
        // Dropping the pipe closes it, so the command reads to its end.
    }

    let deadline = Instant::now() + timeout;
    let mut status = child.try_wait()?;
    while status.is_none() && Instant::now() < deadline {
        thread::sleep(POLL_INTERVAL);
        status = child.try_wait()?;
    }
    if status.is_none() {
        child.kill()?;
        child.wait()?;
    }

    let stdout = join(stdout_reader)?;
    let stderr = join(stderr_reader)?;
    let Some(status) = status else {
        let stderr = String::from_utf8_lossy(&stderr);
        anyhow::bail!("{command:?} ran longer than {timeout:?}; stderr:\n{stderr}");
    };
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// `output` of `command`, or an error with the exit status and the stderr when the command failed.
fn succeeded(command: &Command, output: Output) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "{command:?} failed with {}; stderr:\n{stderr}",
        output.status
    );
}

/// The bytes of `pipe` until the command closes it.
fn read_all(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    pipe.read_to_end(&mut data)?;
    Ok(data)
}

/// The bytes that `reader`, a thread of [`read_all`], read.
fn join(reader: JoinHandle<io::Result<Vec<u8>>>) -> Result<Vec<u8>> {
    let Ok(read) = reader.join() else {
        anyhow::bail!("The thread that reads a pipe of the command panicked");
    };
    let data = read?;
    Ok(data)
}

/// A temporary directory for one test. A passing test ends with [`TestDir::close`], which deletes
/// the directory. A failing test panics or returns an error before that, so the directory stays,
/// and its path goes to stderr, which the report of the failed test shows.
pub(crate) struct TestDir {
    path: PathBuf,
    is_closed: bool,
}

impl TestDir {
    pub(crate) fn new() -> io::Result<TestDir> {
        let dir = tempfile::tempdir()?;
        let path = dir.keep();
        Ok(TestDir {
            path,
            is_closed: false,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Delete the directory: the last step of a passing test.
    pub(crate) fn close(mut self) -> Result<()> {
        fs::remove_dir_all(&self.path)
            .with_context(|| format!("Failed to delete {}", self.path.display()))?;
        self.is_closed = true;
        Ok(())
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.is_closed {
            return;
        }
        eprintln!("Kept the test directory: {}", self.path.display());
    }
}
