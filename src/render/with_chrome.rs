use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use tempfile::TempDir;

use crate::mime::Mime;

pub struct WithChrome;

impl WithChrome {
    /// Render content that is renderable in chrome to image.
    /// Such as html, svg etc into image.
    /// It uses a headless chrome browser via direct command execution.
    ///
    /// # Arguments
    ///
    /// * `mime` - a full mime type such as "image/jpeg" or a shortcut "jpg"
    /// * `input` - content of the input, such as html source or svg data
    /// * `output_type` - specifies output image type such as "png", "jpg"
    /// * `width` - specifies the window width to render a page. Default 1000
    /// * `height` - specifies the window height to render a page. Default 2000
    /// * `asset_base` - specifies the path to assets dir. E.g. the image base path in a html page
    ///
    /// # Returns
    ///
    /// bytes of the image data
    pub async fn render_markup(
        mime: &str,
        input: &str,
        output_type: &str,
        width: Option<u32>,
        height: Option<u32>,
        asset_base: Option<&Path>,
    ) -> anyhow::Result<Vec<u8>> {
        // Create temporary directory
        let temp_dir = TempDir::new()?;
        let cwd = temp_dir.path();

        let input_file_path = Self::create_markup_file(cwd, input, mime, asset_base)?;

        let mut cmd = Self::build_chrome_snapshot_cmd(&input_file_path, width, height, cwd)?;

        let mes = format!(
            "Failed take snapshot with chrome: {:?}; cwd: {}",
            cmd,
            cwd.display()
        );

        // Set working directory and environment for the command
        cmd.current_dir(cwd);
        cmd.env("DISPLAY", ":99"); // Virtual display for headless CI

        let chrome_status = cmd.status().context(mes.clone())?;

        println!("chrome_status: {:?}; cmd: {:?}", chrome_status, cmd);

        if !chrome_status.success() {
            anyhow::bail!("{}: exit code: {:?}", mes, chrome_status.code());
        }

        println!("chrome_status success: {:?}; cmd: {:?}", chrome_status, cmd);

        // The default screenshot path.
        let screenshot_path = cwd.join("screenshot.png");

        // show the content of cwd dir for debug
        println!("cwd: {}", cwd.display());
        let files = fs::read_dir(cwd).context("Failed to read cwd")?;
        for file in files {
            let file = file?;
            println!("{}", file.path().display());
        }

        // Process the screenshot based on output type
        let final_image_data = Self::trim_image(&screenshot_path, output_type)?;

        Ok(final_image_data)
    }

    /// Setup html context, such as encoding and url base
    fn setup_html_page_context(input: &str, asset_base: Option<&Path>) -> String {
        let meta_tag = r#"<meta http-equiv="Content-Type" content="text/html; charset=utf-8"/>"#;
        let mut html_content = meta_tag.to_string();

        // Add base href if asset_base is provided
        if let Some(base_path) = asset_base {
            let base_href = format!(r#"<base href="file://{}/">"#, base_path.display());
            html_content.push_str(&base_href);
        }

        html_content.push_str(input);

        html_content
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
    fn trim_image(screenshot_path: &Path, output_type: &str) -> anyhow::Result<Vec<u8>> {
        let mut cmd = Self::build_trim_image_cmd(screenshot_path, output_type)?;

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
            Self::setup_html_page_context(markup_content, asset_base)
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

    /// Build a chrome command to take screenshot, the output is a png file "screenshot.png" in the current directory
    fn build_chrome_snapshot_cmd(
        markup_file_path: &Path,
        width: Option<u32>,
        height: Option<u32>,
        cwd: &Path,
    ) -> anyhow::Result<Command> {
        let width = width.unwrap_or(1000);
        let height = height.unwrap_or(2000);

        let chrome_path = Self::find_chrome_executable()?;

        let mut cmd = Command::new(chrome_path);

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
            "--screenshot",
            &format!("--window-size={},{}", width, height),
            "--default-background-color=00000000",
            markup_file_path.to_str().unwrap(),
        ])
        .current_dir(cwd);

        Ok(cmd)
    }

    /// Return the first available command from a list
    fn find_available_command(commands: &[&str]) -> anyhow::Result<String> {
        for cmd in commands {
            // output debug info about the command:
            let mut probe = Command::new("which");
            probe.arg(cmd);

            let output = probe.output().unwrap();

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            println!("--------------------------------");
            println!("command: {:?}", probe);
            println!("exit code: {}", output.status);
            println!("stdout:");
            println!("{}", stdout);
            println!("stderr:");
            println!("{}", stderr);
            println!("--------------------------------");

            if output.status.success() {
                println!("Found command: {} at {}", cmd, stdout);
                return Ok(cmd.to_string());
            }
        }
        anyhow::bail!("No available command found in PATH: {:?}", commands)
    }

    /// Build a ImageMagick command to trim image that output directly to stdout
    fn build_trim_image_cmd(screenshot_path: &Path, output_type: &str) -> anyhow::Result<Command> {
        // Find the first available `convert` command:
        // ImageMagick's `convert` command is deprecated and replaced by `magick convert`
        let commands = ["magick", "convert"];

        let executable = Self::find_available_command(&commands)?;

        let mut cmd = Command::new(executable);
        cmd.arg(screenshot_path).arg("-trim").arg("+repage");

        if output_type == "png" {
            // Nothing to do, keep transparent background
        } else {
            // flatten alpha channel
            cmd.args(["-background", "white", "-flatten", "-alpha", "off"]);
        }

        // Output to stdout
        cmd.arg(format!("{}:-", output_type));

        Ok(cmd)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_setup_html_context() {
        let input = "<html><body>Hello</body></html>";
        let result = WithChrome::setup_html_page_context(input, None);

        assert!(result.contains(r#"<meta http-equiv="Content-Type""#));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_setup_html_context_with_base() {
        let input = "<html><body>Hello</body></html>";
        let base_path = PathBuf::from("/tmp/assets");
        let result = WithChrome::setup_html_page_context(input, Some(&base_path));

        assert!(result.contains(r#"<base href="file:///tmp/assets/">"#));
    }

    #[test]
    fn test_get_file_suffix() {
        // Test known MIME types
        assert_eq!(WithChrome::get_file_suffix("text/html"), "html");

        // Test fallback
        assert_eq!(WithChrome::get_file_suffix("custom"), "custom");
    }

    // Note: Integration tests require Chrome and ImageMagick to be installed
}
