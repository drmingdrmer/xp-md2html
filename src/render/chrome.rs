use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use anyhow::Context;
use tempfile::TempDir;

use crate::mime::Mime;

/// The page time, in milliseconds, that a DOM dump gives the page's timers at most; the waits
/// between the timers are skipped, so the budget costs no real time.
const VIRTUAL_TIME_BUDGET: &str = "--virtual-time-budget=5000";

/// Settings that every render of one [`ChromeRenderer`] uses.
#[derive(Clone)]
pub struct RenderConfig {
    /// A full mime type such as "image/jpeg" or a shortcut "jpg"
    pub mime: String,
    /// Output image type such as "png", "jpg"
    pub output_type: String,
    /// The window width to render a page
    pub width: u32,
    /// The window height to render a page
    pub height: u32,
    /// The device scale factor: 2 renders every CSS pixel as 2x2 image pixels, for HiDPI screens
    pub scale: u32,
    /// The path to assets dir. E.g. the image base path in a html page
    pub asset_base: Option<PathBuf>,
}

/// Render markup to image using headless chrome browser.
pub struct ChromeRenderer {
    config: RenderConfig,
    /// Chrome executable: a path, or a command name in `PATH`
    chrome: String,
    /// ImageMagick command: `magick`, or the older `convert`
    magick: String,
}

impl ChromeRenderer {
    /// Find Chrome and ImageMagick once, for every render with `config`.
    pub fn new(config: RenderConfig) -> anyhow::Result<Self> {
        let chrome = Self::find_chrome_executable()?;

        // Find the first available `convert` command:
        // ImageMagick's `convert` command is deprecated and replaced by `magick convert`
        let commands = ["magick", "convert"];

        let magick = Self::find_available_command(&commands)?;

        Ok(Self {
            config,
            chrome,
            magick,
        })
    }

    /// Render content that is renderable in chrome to image.
    /// Such as html, svg etc into image.
    /// It uses a headless chrome browser via direct command execution.
    /// It blocks the calling thread until Chrome and ImageMagick exit.
    ///
    /// # Arguments
    ///
    /// * `input` - content of the input, such as html source or svg data
    ///
    /// # Returns
    ///
    /// bytes of the image data
    pub fn render_markup(&self, input: &str) -> anyhow::Result<Vec<u8>> {
        // Create temporary directory
        let temp_dir = TempDir::new()?;
        let cwd = temp_dir.path();

        // A PDF is printed so that its text stays text; a screenshot would make it an image.
        let is_pdf = self.config.output_type == "pdf";
        let capture: &[&str] = if is_pdf {
            // Without `--no-pdf-header-footer`, Chrome adds the date, the file URL and page numbers.
            &["--print-to-pdf", "--no-pdf-header-footer"]
        } else {
            &["--screenshot"]
        };
        self.run_chrome(cwd, input, capture)?;

        // The default screenshot path.
        let screenshot_path = cwd.join("screenshot.png");

        if is_pdf {
            // The default `--print-to-pdf` path.
            let pdf_path = cwd.join("output.pdf");
            let pdf = fs::read(&pdf_path)
                .with_context(|| format!("Failed to read PDF: {}", pdf_path.display()))?;
            return Ok(pdf);
        }

        // Process the screenshot based on output type
        let final_image_data = self.trim_image(&screenshot_path)?;

        Ok(final_image_data)
    }

    /// A renderer like this one, with a window of `width` by `height` pixels; a diagram needs the
    /// window that fits it, which is known only once it is drawn.
    pub fn with_window(&self, width: u32, height: u32) -> Self {
        let mut config = self.config.clone();
        config.width = width;
        config.height = height;
        Self {
            config,
            chrome: self.chrome.clone(),
            magick: self.magick.clone(),
        }
    }

    /// A renderer like this one, with a window `width` pixels wide and as tall as this one's.
    pub fn with_window_width(&self, width: u32) -> Self {
        self.with_window(width, self.config.height)
    }

    /// Return the DOM of `input` as Chrome serializes it once the page has loaded and its scripts have run.
    ///
    /// A script that finishes its work in a timer callback, as mermaid does in `setTimeout(..., 0)`,
    /// runs it after the load event. With a virtual time budget Chrome first runs the timers,
    /// skipping the waits between them, and dumps the DOM when none is left or the budget is spent.
    pub fn dump_dom(&self, input: &str) -> anyhow::Result<String> {
        let temp_dir = TempDir::new()?;
        let capture = ["--dump-dom", VIRTUAL_TIME_BUDGET];
        let output = self.run_chrome(temp_dir.path(), input, &capture)?;
        let dom =
            String::from_utf8(output.stdout).context("Chrome printed a DOM that is not UTF-8")?;
        Ok(dom)
    }

    /// Write `input` into `cwd`, run Chrome on it with the `capture` flags, and return Chrome's output once it exits.
    fn run_chrome(&self, cwd: &Path, input: &str, capture: &[&str]) -> anyhow::Result<Output> {
        let mime = &self.config.mime;
        let asset_base = self.config.asset_base.as_deref();
        let input_file_path = Self::create_markup_file(cwd, input, mime, asset_base)?;

        let mut cmd = self.build_chrome_cmd(&input_file_path, cwd, capture);

        let mes = format!(
            "Failed take snapshot with chrome: {:?}; cwd: {}",
            cmd,
            cwd.display()
        );

        // Set working directory for the command
        cmd.current_dir(cwd);

        // Chrome's stderr is noise, unless Chrome fails.
        let chrome_output = cmd.output().context(mes.clone())?;

        if !chrome_output.status.success() {
            let stderr = String::from_utf8_lossy(&chrome_output.stderr);
            anyhow::bail!(
                "{}: exit code: {:?}; stderr: {}",
                mes,
                chrome_output.status.code(),
                stderr
            );
        }

        Ok(chrome_output)
    }

    /// Setup html context, such as encoding and url base
    fn setup_html_page_context(input: &str, asset_base: Option<&Path>) -> anyhow::Result<String> {
        // Anything before the doctype makes Chrome render in quirks mode, so the tags go after it.
        let (doctype, rest) = Self::split_doctype(input);

        let meta_tag = r#"<meta http-equiv="Content-Type" content="text/html; charset=utf-8"/>"#;
        let mut html_content = doctype.to_string();
        html_content.push_str(meta_tag);

        // Add base href if asset_base is provided
        if let Some(base_path) = asset_base {
            let base_url = Self::dir_file_url(base_path)?;
            let base_href = format!(r#"<base href="{}">"#, base_url);
            html_content.push_str(&base_href);
        }

        html_content.push_str(rest);

        Ok(html_content)
    }

    /// Build a `file://` URL for directory `dir`, with the trailing `/` that `<base href>` needs.
    fn dir_file_url(dir: &Path) -> anyhow::Result<String> {
        let absolute = std::path::absolute(dir)
            .with_context(|| format!("Failed to make asset base absolute: {}", dir.display()))?;
        let Some(path) = absolute.to_str() else {
            anyhow::bail!("Asset base is not valid UTF-8: {}", absolute.display());
        };

        let mut url = "file://".to_string();
        for byte in path.bytes() {
            let unreserved = byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte);
            if unreserved {
                url.push(char::from(byte));
            } else {
                url.push_str(&format!("%{:02X}", byte));
            }
        }

        if !url.ends_with('/') {
            url.push('/');
        }
        Ok(url)
    }

    /// Split `input` right after its leading doctype; return `("", input)` if there is none.
    fn split_doctype(input: &str) -> (&str, &str) {
        const DOCTYPE_START: &str = "<!doctype";

        let trimmed = input.trim_ascii_start();
        let starts_with_doctype = trimmed
            .get(..DOCTYPE_START.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(DOCTYPE_START));
        if !starts_with_doctype {
            return ("", input);
        }

        let Some(end) = trimmed.find('>') else {
            return ("", input);
        };

        let whitespace_len = input.len() - trimmed.len();
        input.split_at(whitespace_len + end + 1)
    }

    /// Get file suffix from MIME type (matches Python logic)
    fn get_file_suffix(mime: &str) -> String {
        // First try reverse lookup from our MIME mappings
        if let Some(suffix) = Mime::get_suffix(mime) {
            return suffix.to_string();
        }

        // Fallback to the mime parameter itself as suffix
        mime.to_string()
    }

    /// Find Chrome executable by checking common paths
    fn find_chrome_executable() -> anyhow::Result<String> {
        // Check macOS Chrome path first
        let mac_chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
        if Path::new(mac_chrome).exists() {
            return Ok(mac_chrome.to_string());
        }

        // Try common Chrome/Chromium names in PATH
        let chrome_names = [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "chrome",
        ];

        Self::find_available_command(&chrome_names)
        // for name in &chrome_names {
        //     if let Ok(output) = Command::new("which").arg(name).output() {
        //         if output.status.success() {
        //             let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        //             if !path.is_empty() {
        //                 return Ok(path);
        //             }
        //         }
        //     }
        // }
        //
        // anyhow::bail!("Chrome/Chromium executable not found. Please install Chrome or Chromium.")
    }

    /// Trim image using ImageMagick (matches Python logic)
    fn trim_image(&self, screenshot_path: &Path) -> anyhow::Result<Vec<u8>> {
        let mut cmd = self.build_trim_image_cmd(screenshot_path);

        let output = cmd
            .output()
            .context(format!("Failed to execute ImageMagick convert: {:?}", cmd))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ImageMagick convert failed: {}", stderr);
        }

        Ok(output.stdout)
    }

    /// Create a markup file for chrome to render
    fn create_markup_file(
        base_dir: &Path,
        markup_content: &str,
        mime: &str,
        asset_base: Option<&Path>,
    ) -> anyhow::Result<PathBuf> {
        // Process input content
        let markup_content = if mime.contains("html") {
            Self::setup_html_page_context(markup_content, asset_base)?
        } else {
            markup_content.to_string()
        };

        let suffix = Self::get_file_suffix(mime);
        let markup_file_path = base_dir.join(format!("input.{}", suffix));

        fs::write(&markup_file_path, markup_content.as_bytes()).with_context(|| {
            format!("Failed to write temp file: {}", markup_file_path.display())
        })?;

        Ok(markup_file_path)
    }

    /// Build a chrome command that loads `markup_file_path` and captures it as `capture` says: `--screenshot`
    /// writes "screenshot.png" in `cwd`, `--print-to-pdf` writes "output.pdf", `--dump-dom` prints the DOM to stdout
    fn build_chrome_cmd(&self, markup_file_path: &Path, cwd: &Path, capture: &[&str]) -> Command {
        let width = self.config.width;
        let height = self.config.height;
        let scale = self.config.scale;

        let mut cmd = Command::new(&self.chrome);

        cmd.args(vec![
            "--headless",
            "--disable-gpu",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--disable-background-timer-throttling",
            "--disable-backgrounding-occluded-windows",
            "--disable-renderer-backgrounding",
            "--disable-features=TranslateUI",
            "--disable-ipc-flooding-protection",
            "--disable-extensions",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-web-security",
            "--disable-features=VizDisplayCompositor",
            // Without it, the scale, and so the image size, follows the machine's display.
            &format!("--force-device-scale-factor={}", scale),
            &format!("--window-size={},{}", width, height),
            "--default-background-color=00000000",
            markup_file_path.to_str().unwrap(),
        ])
        .args(capture)
        .current_dir(cwd);

        cmd
    }

    /// Return the first available command from a list
    fn find_available_command(commands: &[&str]) -> anyhow::Result<String> {
        for cmd in commands {
            let mut probe = Command::new("which");
            probe.arg(cmd);

            let output = probe
                .output()
                .with_context(|| format!("Failed to run {:?}", probe))?;

            if output.status.success() {
                return Ok(cmd.to_string());
            }
        }
        anyhow::bail!("No available command found in PATH: {:?}", commands)
    }

    /// Build a ImageMagick command to trim image that output directly to stdout
    fn build_trim_image_cmd(&self, screenshot_path: &Path) -> Command {
        let output_type = self.config.output_type.as_str();

        let mut cmd = Command::new(&self.magick);
        cmd.arg(screenshot_path).arg("-trim").arg("+repage");

        if output_type == "png" {
            // Nothing to do, keep transparent background
        } else {
            // flatten alpha channel
            cmd.args(["-background", "white", "-flatten", "-alpha", "off"]);
        }

        // Output to stdout
        cmd.arg(format!("{}:-", output_type));

        cmd
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_setup_html_context() {
        let input = "<html><body>Hello</body></html>";
        let result = ChromeRenderer::setup_html_page_context(input, None).unwrap();

        assert!(result.contains(r#"<meta http-equiv="Content-Type""#));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_setup_html_context_with_base() {
        let input = "<html><body>Hello</body></html>";
        let base_path = PathBuf::from("/tmp/assets");
        let result = ChromeRenderer::setup_html_page_context(input, Some(&base_path)).unwrap();

        assert!(result.contains(r#"<base href="file:///tmp/assets/">"#));
    }

    #[test]
    fn test_dir_file_url() {
        let escaped = ChromeRenderer::dir_file_url(Path::new("/tmp/a b#1%/")).unwrap();
        assert_eq!(escaped, "file:///tmp/a%20b%231%25/");

        let relative = ChromeRenderer::dir_file_url(Path::new("assets")).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(relative, format!("file://{}/assets/", cwd.display()));
    }

    #[test]
    fn test_setup_html_context_after_doctype() {
        let input = "\n<!doctype html>\n<html><body>Hello</body></html>";
        let base_path = PathBuf::from("/tmp/assets");
        let result = ChromeRenderer::setup_html_page_context(input, Some(&base_path)).unwrap();

        let expected = concat!(
            "\n<!doctype html>",
            r#"<meta http-equiv="Content-Type" content="text/html; charset=utf-8"/>"#,
            r#"<base href="file:///tmp/assets/">"#,
            "\n<html><body>Hello</body></html>",
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn test_split_doctype() {
        let without_doctype = ChromeRenderer::split_doctype("<html></html>");
        assert_eq!(without_doctype, ("", "<html></html>"));

        let with_doctype = ChromeRenderer::split_doctype(" <!DOCTYPE html><html></html>");
        assert_eq!(with_doctype, (" <!DOCTYPE html>", "<html></html>"));

        let unclosed = ChromeRenderer::split_doctype("<!DOCTYPE html");
        assert_eq!(unclosed, ("", "<!DOCTYPE html"));
    }

    #[test]
    fn test_get_file_suffix() {
        // Test known MIME types
        assert_eq!(ChromeRenderer::get_file_suffix("text/html"), "html");

        // Test fallback
        assert_eq!(ChromeRenderer::get_file_suffix("custom"), "custom");
    }

    // Note: Integration tests require Chrome and ImageMagick to be installed
}
