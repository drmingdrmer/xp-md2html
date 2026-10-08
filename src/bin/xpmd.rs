use std::fs;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use xp_md2html::process::Action;
use xp_md2html::process::ActionContext;
use xp_md2html::render::chrome::ChromeRenderer;
use xp_md2html::render::chrome::RenderConfig;
use xp_md2html::render::code::code_to_html;
use xp_md2html::render::code::load_theme;
use xp_md2html::render::code::CodeStyle;
use xp_md2html::render::code::DEFAULT_THEME;
use xp_md2html::render::graphviz::graphviz_to_svg;
use xp_md2html::render::markdown::markdown_page;
use xp_md2html::render::markdown::markdown_to_html;
use xp_md2html::render::math::math_page;
use xp_md2html::render::math::math_to_svg;
use xp_md2html::render::mermaid::mermaid_to_svg;
use xp_md2html::render::page::svg_to_image;

#[derive(Parser)]
#[command(name = "xpmd")]
#[command(about = "A markdown to HTML/image converter with Chrome rendering")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render HTML content to image using headless Chrome
    RenderMarkup(RenderArgs),
    /// Apply actions to a markdown file in order and write the result
    Process(ProcessArgs),
    /// Render a code snippet to an HTML page with syntax colors
    RenderCode(RenderCodeArgs),
    /// Render LaTeX math to SVG or an image with MathJax
    RenderMath(RenderMathArgs),
    /// Render a DOT graph to SVG or an image with Graphviz
    RenderGraphviz(DiagramArgs),
    /// Render a mermaid diagram to SVG or an image
    RenderMermaid(DiagramArgs),
    /// Render markdown to an HTML page in GitHub's style, or to bare HTML
    RenderMarkdown(RenderMarkdownArgs),
}

/// The options of the `render-markup` subcommand.
#[derive(Args)]
struct RenderArgs {
    /// Input file path (HTML content) [default: stdin]
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output file path [default: stdout]
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format: png, jpg, jpeg, pdf [default: the output file's extension, else png]
    #[arg(short, long)]
    format: Option<String>,

    /// Window width for rendering
    #[arg(short, long, default_value = "1000")]
    width: u32,

    /// Window height for rendering
    #[arg(long, default_value = "2000")]
    height: u32,

    /// Device scale factor: 2 renders every CSS pixel as 2x2 image pixels, for HiDPI screens
    #[arg(long, default_value = "2")]
    scale: u32,

    /// MIME type of input content (auto-detected if not specified)
    #[arg(short, long)]
    mime: Option<String>,

    /// Directory for relative asset paths in HTML input [default: the input file's directory]
    #[arg(short, long)]
    base: Option<PathBuf>,
}

/// The options of the `process` subcommand.
#[derive(Args)]
struct ProcessArgs {
    /// Input markdown file
    #[arg(short, long)]
    input: PathBuf,

    /// Output markdown file
    #[arg(short, long)]
    output: PathBuf,

    /// Directory for the files the actions create [default: the output file's directory]
    #[arg(long)]
    assets: Option<PathBuf>,

    /// Window width for rendering images
    #[arg(short, long, default_value = "1000")]
    width: u32,

    /// Window height for rendering images
    #[arg(long, default_value = "2000")]
    height: u32,

    /// Device scale factor: 2 renders every CSS pixel as 2x2 image pixels, for HiDPI screens
    #[arg(long, default_value = "2")]
    scale: u32,

    /// An action to apply, in the given order; one of: table-to-image, download-images, embed-markdown, image-to-asset, table-to-html, mermaid-to-image, graphviz-to-image, code-to-image[=WIDTH]
    #[arg(long = "action", required = true)]
    actions: Vec<Action>,
}

/// The options of the `render-code` subcommand.
#[derive(Args)]
struct RenderCodeArgs {
    /// Input file with the code [default: stdin]
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output HTML file [default: stdout]
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Language of the code as a fenced block names it: rust, rs, py, go [default: the extension of --input, else plain text]
    #[arg(short, long)]
    lang: Option<String>,

    /// Color theme: a built-in syntect theme name, or the path of a .tmTheme file
    #[arg(long, default_value = DEFAULT_THEME)]
    theme: String,

    /// Width in pixels at which a line wraps
    #[arg(short, long, default_value = "1000")]
    width: u32,
}

/// The options of the `render-math` subcommand.
#[derive(Args)]
struct RenderMathArgs {
    /// Input file with the TeX source, without the $ delimiters [default: stdin]
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output file [default: stdout]
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format: svg, png, jpg, jpeg [default: the output file's extension, else svg]
    #[arg(short, long)]
    format: Option<String>,

    /// Typeset as inline math instead of display math
    #[arg(long)]
    inline: bool,

    /// Device scale factor of an image: 2 renders every CSS pixel as 2x2 image pixels, for HiDPI screens
    #[arg(long, default_value = "2")]
    scale: u32,
}

/// The options of the `render-graphviz` and `render-mermaid` subcommands.
#[derive(Args)]
struct DiagramArgs {
    /// Input file with the diagram source [default: stdin]
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output file [default: stdout]
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format: svg, png, jpg, jpeg [default: the output file's extension, else svg]
    #[arg(short, long)]
    format: Option<String>,

    /// Device scale factor of an image: 2 renders every CSS pixel as 2x2 image pixels, for HiDPI screens
    #[arg(long, default_value = "2")]
    scale: u32,
}

/// The options of the `render-markdown` subcommand.
#[derive(Args)]
struct RenderMarkdownArgs {
    /// Input markdown file [default: stdin]
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output HTML file [default: stdout]
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Write the bare HTML of the content, without the page and GitHub's style sheet around it
    #[arg(long)]
    bare: bool,
}

/// The window that a formula is rendered in; the trim cuts the image down to the formula.
const MATH_WINDOW_WIDTH: u32 = 1000;
const MATH_WINDOW_HEIGHT: u32 = 2000;

/// The context of the error when Chrome or ImageMagick is missing.
const INSTALL_HELP: &str = "Failed to render content. Make sure Chrome/Chromium and ImageMagick are installed and accessible.\n\
    Chrome: On macOS: Install from https://www.google.com/chrome/\n\
    Chrome: On Linux: sudo apt install chromium-browser (Ubuntu/Debian) or equivalent\n\
    Chrome: On Windows: Install from https://www.google.com/chrome/\n\
    ImageMagick: On macOS: brew install imagemagick\n\
    ImageMagick: On Linux: sudo apt install imagemagick\n\
    ImageMagick: On Windows: Install from https://imagemagick.org/";

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::RenderMarkup(args) => {
            render_command(args)?;
        }
        Commands::Process(args) => {
            process_command(args)?;
        }
        Commands::RenderCode(args) => {
            render_code_command(args)?;
        }
        Commands::RenderMath(args) => {
            render_math_command(args)?;
        }
        Commands::RenderGraphviz(args) => {
            render_diagram_command(args, graphviz_to_svg)?;
        }
        Commands::RenderMermaid(args) => {
            render_diagram_command(args, mermaid_to_svg)?;
        }
        Commands::RenderMarkdown(args) => {
            render_markdown_command(args)?;
        }
    }

    Ok(())
}

fn render_command(args: RenderArgs) -> Result<()> {
    let RenderArgs {
        input,
        output,
        format,
        width,
        height,
        scale,
        mime,
        base,
    } = args;

    let content = read_input(input.as_deref())?;

    // Determine MIME type
    let mime_type = mime.unwrap_or_else(|| {
        // Try to determine from file extension
        input
            .as_deref()
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext.to_lowercase().as_str() {
                "html" | "htm" => "text/html",
                "svg" => "image/svg+xml",
                "xml" => "application/xml",
                _ => "text/html", // Default fallback
            })
            .unwrap_or("text/html")
            .to_string()
    });

    // Relative asset paths in HTML resolve against `--base`, by default the input file's directory.
    let is_html = mime_type.contains("html");
    if base.is_some() && !is_html {
        anyhow::bail!("--base only works with HTML input, not {}", mime_type);
    }
    let input_dir = match &input {
        Some(path) => {
            let absolute = std::path::absolute(path)
                .with_context(|| format!("Failed to resolve input path: {}", path.display()))?;
            absolute.parent().map(Path::to_path_buf)
        }
        None => None,
    };
    let base = if is_html { base.or(input_dir) } else { None };

    let format = resolve_format(format.as_deref(), output.as_deref())?;

    // Create output directory if it doesn't exist
    if let Some(parent) = output.as_deref().and_then(Path::parent) {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
    }

    // Render using Chrome
    let config = RenderConfig {
        mime: mime_type,
        output_type: format.to_string(),
        width,
        height,
        scale,
        asset_base: base,
    };
    let renderer = ChromeRenderer::new(config).context(INSTALL_HELP)?;
    let image_data = renderer.render_markup(&content)?;

    write_output(output.as_deref(), &image_data)
}

fn render_code_command(args: RenderCodeArgs) -> Result<()> {
    let RenderCodeArgs {
        input,
        output,
        lang,
        theme,
        width,
    } = args;

    let code = read_input(input.as_deref())?;

    let extension = input
        .as_deref()
        .and_then(Path::extension)
        .and_then(|ext| ext.to_str())
        .map(str::to_string);
    let lang = lang.or(extension);

    let theme = load_theme(&theme)?;
    let style = CodeStyle { theme, width };
    let html = code_to_html(lang.as_deref(), &code, &style)?;

    write_output(output.as_deref(), html.as_bytes())
}

fn render_math_command(args: RenderMathArgs) -> Result<()> {
    let RenderMathArgs {
        input,
        output,
        format,
        inline,
        scale,
    } = args;

    let tex = read_input(input.as_deref())?;
    let format = resolve_svg_format(format.as_deref(), output.as_deref())?;

    let config = RenderConfig {
        mime: "text/html".to_string(),
        output_type: format.to_string(),
        width: MATH_WINDOW_WIDTH,
        height: MATH_WINDOW_HEIGHT,
        scale,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config).context(INSTALL_HELP)?;

    let display = !inline;
    let data = if format == "svg" {
        let mut svg = math_to_svg(&renderer, &tex, display)?;
        svg.push('\n');
        svg.into_bytes()
    } else {
        renderer.render_markup(&math_page(&tex, display))?
    };

    write_output(output.as_deref(), &data)
}

fn render_markdown_command(args: RenderMarkdownArgs) -> Result<()> {
    let RenderMarkdownArgs {
        input,
        output,
        bare,
    } = args;

    let markdown = read_input(input.as_deref())?;
    let html = markdown_to_html(&markdown);
    let out = if bare { html } else { markdown_page(&html) };

    write_output(output.as_deref(), out.as_bytes())
}

/// Run `render-graphviz` or `render-mermaid`: `to_svg` draws the source that `args` names.
fn render_diagram_command(
    args: DiagramArgs,
    to_svg: fn(&ChromeRenderer, &str) -> Result<String>,
) -> Result<()> {
    let DiagramArgs {
        input,
        output,
        format,
        scale,
    } = args;

    let source = read_input(input.as_deref())?;
    let format = resolve_svg_format(format.as_deref(), output.as_deref())?;

    // The DOM dump that makes the SVG does not depend on the window; the image gets its own.
    let config = RenderConfig {
        mime: "text/html".to_string(),
        output_type: format.to_string(),
        width: MATH_WINDOW_WIDTH,
        height: MATH_WINDOW_HEIGHT,
        scale,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config).context(INSTALL_HELP)?;
    let mut svg = to_svg(&renderer, &source)?;
    if format == "svg" {
        svg.push('\n');
        return write_output(output.as_deref(), svg.as_bytes());
    }

    let image = svg_to_image(&renderer, &svg)?;
    write_output(output.as_deref(), &image)
}

/// The content of `path`, or of stdin when there is no path.
fn read_input(path: Option<&Path>) -> Result<String> {
    let Some(path) = path else {
        let mut content = String::new();
        io::stdin()
            .read_to_string(&mut content)
            .context("Failed to read stdin")?;
        return Ok(content);
    };
    fs::read_to_string(path)
        .with_context(|| format!("Failed to read input file: {}", path.display()))
}

/// Write `data` to `path`, or to stdout when there is no path.
fn write_output(path: Option<&Path>, data: &[u8]) -> Result<()> {
    let Some(path) = path else {
        return io::stdout()
            .write_all(data)
            .context("Failed to write stdout");
    };
    fs::write(path, data)
        .with_context(|| format!("Failed to write output file: {}", path.display()))
}

/// The output formats of `render-math` and `render-graphviz`, as error messages list them.
const SUPPORTED_SVG_FORMATS: &str = "svg, png, jpg, jpeg";

/// Return the output format of `render-math` and `render-graphviz`: `-f`, else the extension of
/// `-o`, else svg.
fn resolve_svg_format(format: Option<&str>, output: Option<&Path>) -> Result<&'static str> {
    let extension = output
        .and_then(Path::extension)
        .and_then(|ext| ext.to_str());
    let Some(name) = format.or(extension) else {
        return Ok("svg");
    };
    match name.to_lowercase().as_str() {
        "svg" => Ok("svg"),
        "png" => Ok("png"),
        "jpg" | "jpeg" => Ok("jpg"),
        _ => anyhow::bail!(
            "Unsupported output format: {}. Supported: {}",
            name,
            SUPPORTED_SVG_FORMATS
        ),
    }
}

fn process_command(args: ProcessArgs) -> Result<()> {
    let ProcessArgs {
        input,
        output,
        assets,
        width,
        height,
        scale,
        actions,
    } = args;

    let markdown = fs::read_to_string(&input)
        .with_context(|| format!("Failed to read input file: {}", input.display()))?;
    let absolute_input = std::path::absolute(&input)
        .with_context(|| format!("Failed to resolve input path: {}", input.display()))?;
    let input_dir = absolute_input
        .parent()
        .with_context(|| format!("Input path has no directory: {}", input.display()))?
        .to_path_buf();

    let absolute_output = std::path::absolute(&output)
        .with_context(|| format!("Failed to resolve output path: {}", output.display()))?;
    let output_dir = absolute_output
        .parent()
        .with_context(|| format!("Output path has no directory: {}", output.display()))?
        .to_path_buf();
    let stem = absolute_output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("Output path has no file name: {}", output.display()))?
        .to_string();
    let assets_dir = assets.unwrap_or_else(|| output_dir.clone());

    fs::create_dir_all(&output_dir).with_context(|| {
        format!(
            "Failed to create output directory: {}",
            output_dir.display()
        )
    })?;
    fs::create_dir_all(&assets_dir).with_context(|| {
        format!(
            "Failed to create assets directory: {}",
            assets_dir.display()
        )
    })?;

    let config = RenderConfig {
        mime: "text/html".to_string(),
        output_type: "png".to_string(),
        width,
        height,
        scale,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config).context(INSTALL_HELP)?;
    let ctx = ActionContext {
        input_dir,
        assets_dir,
        output_dir,
        stem,
        renderer,
    };

    let processed = xp_md2html::process::process_markdown(&markdown, &actions, &ctx)?;
    fs::write(&output, processed)
        .with_context(|| format!("Failed to write output file: {}", output.display()))?;

    println!("✅ Successfully wrote: {}", output.display());

    Ok(())
}

/// The output formats that `-f` and the output file's extension accept, as error messages list them.
const SUPPORTED_FORMATS: &str = "png, jpg, jpeg, pdf";

/// Return the output format that `name`, a `-f` value or a file extension, names.
fn format_of(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "pdf" => Some("pdf"),
        _ => None,
    }
}

/// Return the output format of `render-markup`: `-f`, else the extension of `output`, else png.
///
/// When `-f` and the extension both name a format, the two must agree.
fn resolve_format(format: Option<&str>, output: Option<&Path>) -> Result<&'static str> {
    let extension = output
        .and_then(Path::extension)
        .and_then(|ext| ext.to_str());
    let extension_format = extension.and_then(format_of);

    let Some(format) = format else {
        let Some(extension) = extension else {
            return Ok("png");
        };
        return extension_format.with_context(|| {
            format!(
                "Cannot tell the output format from the extension {}. Pass -f with one of: {}",
                extension, SUPPORTED_FORMATS
            )
        });
    };

    let Some(flag_format) = format_of(format) else {
        anyhow::bail!(
            "Unsupported output format: {}. Supported: {}",
            format,
            SUPPORTED_FORMATS
        );
    };

    let Some(extension) = extension else {
        return Ok(flag_format);
    };
    let extension_agrees = extension_format.is_none() || extension_format == Some(flag_format);
    if !extension_agrees {
        anyhow::bail!("-f {} does not match the extension {}", format, extension);
    }
    Ok(flag_format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_format_from_extension() -> Result<()> {
        let format = resolve_format(None, Some(Path::new("out.JPG")))?;
        assert_eq!(format, "jpg");

        let format = resolve_format(None, Some(Path::new("out.jpeg")))?;
        assert_eq!(format, "jpg");

        let format = resolve_format(None, Some(Path::new("out.pdf")))?;
        assert_eq!(format, "pdf");
        Ok(())
    }

    #[test]
    fn test_resolve_format_default() -> Result<()> {
        let stdout = resolve_format(None, None)?;
        assert_eq!(stdout, "png");

        let no_extension = resolve_format(None, Some(Path::new("out")))?;
        assert_eq!(no_extension, "png");
        Ok(())
    }

    #[test]
    fn test_resolve_format_from_flag() -> Result<()> {
        let format = resolve_format(Some("PNG"), Some(Path::new("out")))?;
        assert_eq!(format, "png");

        let format = resolve_format(Some("png"), Some(Path::new("out.tmp")))?;
        assert_eq!(format, "png");

        let format = resolve_format(Some("jpeg"), Some(Path::new("out.jpg")))?;
        assert_eq!(format, "jpg");

        let format = resolve_format(Some("pdf"), None)?;
        assert_eq!(format, "pdf");
        Ok(())
    }

    #[test]
    fn test_resolve_svg_format() -> Result<()> {
        let default = resolve_svg_format(None, None)?;
        assert_eq!(default, "svg");

        let from_extension = resolve_svg_format(None, Some(Path::new("x.PNG")))?;
        assert_eq!(from_extension, "png");

        let flag_wins = resolve_svg_format(Some("jpeg"), Some(Path::new("x.svg")))?;
        assert_eq!(flag_wins, "jpg");

        let no_extension = resolve_svg_format(None, Some(Path::new("x")))?;
        assert_eq!(no_extension, "svg");

        let error = resolve_svg_format(None, Some(Path::new("x.pdf"))).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Unsupported output format: pdf. Supported: svg, png, jpg, jpeg"
        );
        Ok(())
    }

    #[test]
    fn test_resolve_format_errors() {
        let result = resolve_format(None, Some(Path::new("out.gif")));
        let message = result.unwrap_err().to_string();
        assert_eq!(
            message,
            "Cannot tell the output format from the extension gif. Pass -f with one of: png, jpg, jpeg, pdf"
        );

        let result = resolve_format(Some("png"), Some(Path::new("out.jpg")));
        let message = result.unwrap_err().to_string();
        assert_eq!(message, "-f png does not match the extension jpg");

        let result = resolve_format(Some("gif"), Some(Path::new("out.gif")));
        let message = result.unwrap_err().to_string();
        assert_eq!(
            message,
            "Unsupported output format: gif. Supported: png, jpg, jpeg, pdf"
        );
    }
}
